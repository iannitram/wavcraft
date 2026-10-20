//! Zoom and scroll arithmetic, selections and time formatting. All positions are in frames.

use std::ops::Range;

use crate::doc::edit::ChannelMask;

/// The closest zoom: one frame spread over this many pixels.
const MAX_PX_PER_FRAME: f64 = 16.0;

/// Which part of the file is on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The frame at the left edge.
    pub start: f64,
    /// How many frames one pixel covers (below 1 when zoomed in past single samples).
    pub frames_per_px: f64,
}

impl Viewport {
    /// The whole file across `width_px` pixels.
    pub fn fit(total_frames: usize, width_px: f32) -> Viewport {
        Viewport { start: 0.0, frames_per_px: fit_scale(total_frames, width_px) }
    }

    /// The frame just past the right edge.
    pub fn end(&self, width_px: f32) -> f64 {
        self.start + self.frames_per_px * f64::from(width_px.max(0.0))
    }

    /// The frame under pixel `x` (0 = left edge).
    pub fn frame_at(&self, x: f32) -> f64 {
        self.start + self.frames_per_px * f64::from(x)
    }

    /// The pixel where `frame` is drawn (may be off screen).
    pub fn x_of(&self, frame: f64) -> f32 {
        ((frame - self.start) / self.frames_per_px) as f32
    }

    /// Zoom by `factor` (above 1 zooms in) keeping the frame under pixel `anchor_x` where it is.
    pub fn zoom_at(&mut self, factor: f64, anchor_x: f32, width_px: f32, total_frames: usize) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.frame_at(anchor_x);
        let widest = fit_scale(total_frames, width_px);
        self.frames_per_px = (self.frames_per_px / factor).clamp((1.0 / MAX_PX_PER_FRAME).min(widest), widest);
        self.start = anchor - self.frames_per_px * f64::from(anchor_x);
        self.clamp(width_px, total_frames);
    }

    /// Scroll by `dx_px` pixels (positive moves later in the file).
    pub fn scroll_by(&mut self, dx_px: f32, width_px: f32, total_frames: usize) {
        if dx_px.is_finite() {
            self.start += self.frames_per_px * f64::from(dx_px);
        }
        self.clamp(width_px, total_frames);
    }

    /// Scroll just enough to bring `frame` on screen, leaving a small margin.
    pub fn reveal(&mut self, frame: f64, width_px: f32, total_frames: usize) {
        let margin = self.frames_per_px * f64::from(width_px) * 0.1;
        if frame < self.start {
            self.start = frame - margin;
        } else if frame > self.end(width_px) {
            self.start = frame + margin - self.frames_per_px * f64::from(width_px);
        }
        self.clamp(width_px, total_frames);
    }

    /// Keep the view inside the file and the zoom inside its limits.
    pub fn clamp(&mut self, width_px: f32, total_frames: usize) {
        let widest = fit_scale(total_frames, width_px);
        if !self.frames_per_px.is_finite() || self.frames_per_px <= 0.0 {
            self.frames_per_px = widest;
        }
        self.frames_per_px = self.frames_per_px.clamp((1.0 / MAX_PX_PER_FRAME).min(widest), widest);
        let last_start = (total_frames as f64 - self.frames_per_px * f64::from(width_px.max(1.0))).max(0.0);
        self.start = if self.start.is_finite() { self.start.clamp(0.0, last_start) } else { 0.0 };
    }
}

fn fit_scale(total_frames: usize, width_px: f32) -> f64 {
    (total_frames.max(1) as f64 / f64::from(width_px.max(1.0))).max(1.0 / MAX_PX_PER_FRAME)
}

/// A time range in frames; `anchor` is where the drag began, `head` where it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    /// A zero-length selection: just the cursor.
    pub fn at(frame: usize) -> Selection {
        Selection { anchor: frame, head: frame }
    }

    /// First selected frame.
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// One past the last selected frame.
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Number of frames selected.
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// Whether nothing is selected (the selection is only a cursor position).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Which channels a drag that starts at `y_fraction` of the editor's height (0 is the top of the
/// waveform area, 1 the bottom) selects. With `context` on (the default) and a stereo file, the
/// top quarter selects the left channel only, the bottom quarter the right only, and the middle
/// both. Files with other channel counts, or `context` off, always select every channel.
pub fn channels_for_drag(y_fraction: f32, channels: usize, context: bool) -> ChannelMask {
    if !context || channels != 2 || !y_fraction.is_finite() {
        return ChannelMask::ALL;
    }
    if y_fraction <= CHANNEL_STRIP {
        ChannelMask::only(0)
    } else if y_fraction >= 1.0 - CHANNEL_STRIP {
        ChannelMask::only(1)
    } else {
        ChannelMask::ALL
    }
}

/// How tall the top and bottom channel strips are, as a fraction of the editor's height.
pub const CHANNEL_STRIP: f32 = 0.25;

/// `mask` for a `channels`-channel file as letters: `L`, `R`, `LR` for stereo, otherwise the
/// channel numbers from 1 (`1,3`); `-` for none.
pub fn channels_text(mask: ChannelMask, channels: usize) -> String {
    let active = mask.active(channels);
    match (channels, active.as_slice()) {
        (_, []) => "-".to_string(),
        (2, [0]) => "L".to_string(),
        (2, [1]) => "R".to_string(),
        (2, [0, 1]) => "LR".to_string(),
        (_, list) => list.iter().map(|c| (c + 1).to_string()).collect::<Vec<_>>().join(","),
    }
}

/// Select Current View: the frames on screen, limited to the file.
pub fn select_current_view(view: &Viewport, width_px: f32, total_frames: usize) -> Range<u64> {
    let start = view.start.max(0.0).min(total_frames as f64);
    let end = view.end(width_px).max(start).min(total_frames as f64);
    start.round() as u64..end.round() as u64
}

/// Select from the start of the file to the cursor (the selection's first frame).
pub fn select_to_start(selection: &Range<u64>, total_frames: u64) -> Range<u64> {
    0..selection.start.min(total_frames)
}

/// Select from the cursor (the selection's last frame) to the end of the file.
pub fn select_to_end(selection: &Range<u64>, total_frames: u64) -> Range<u64> {
    selection.end.min(total_frames)..total_frames
}

/// `frame` at `sample_rate` as `m:ss.mmm`, or `h:mm:ss.mmm` from one hour up.
pub fn format_time(frame: usize, sample_rate: u32) -> String {
    if sample_rate == 0 {
        return "0:00.000".to_string();
    }
    let rate = u64::from(sample_rate);
    let frame = frame as u64;
    let ms = (frame % rate) * 1000 / rate;
    let secs = frame / rate;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{ms:03}") } else { format!("{m}:{s:02}.{ms:03}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_drags_pick_a_channel_by_height() {
        assert_eq!(channels_for_drag(0.05, 2, true), ChannelMask::only(0));
        assert_eq!(channels_for_drag(0.95, 2, true), ChannelMask::only(1));
        assert_eq!(channels_for_drag(0.5, 2, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.25, 2, true), ChannelMask::only(0), "the strip's edge counts");
        assert_eq!(channels_for_drag(0.26, 2, true), ChannelMask::ALL);
        // Off, mono, surround and bad numbers always take every channel.
        assert_eq!(channels_for_drag(0.05, 2, false), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.05, 1, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.95, 6, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(f32::NAN, 2, true), ChannelMask::ALL);
    }

    #[test]
    fn channel_masks_read_as_text() {
        assert_eq!(channels_text(ChannelMask::only(0), 2), "L");
        assert_eq!(channels_text(ChannelMask::only(1), 2), "R");
        assert_eq!(channels_text(ChannelMask::ALL, 2), "LR");
        assert_eq!(channels_text(ChannelMask::ALL, 1), "1");
        assert_eq!(channels_text(ChannelMask::only(2).with(0), 4), "1,3");
        assert_eq!(channels_text(ChannelMask::NONE, 2), "-");
        assert_eq!(channels_text(ChannelMask::only(1), 1), "-", "a channel the file lacks");
    }

    #[test]
    fn select_variants_follow_the_view_and_the_cursor() {
        let v = Viewport { start: 1000.0, frames_per_px: 10.0 };
        assert_eq!(select_current_view(&v, 500.0, 100_000), 1000..6000);
        assert_eq!(select_current_view(&v, 500.0, 3000), 1000..3000, "limited to the file");
        assert_eq!(select_current_view(&Viewport { start: 9e9, frames_per_px: 1.0 }, 100.0, 50), 50..50);
        assert_eq!(select_to_start(&(400..900), 1000), 0..400);
        assert_eq!(select_to_start(&(0..0), 1000), 0..0);
        assert_eq!(select_to_end(&(400..900), 1000), 900..1000);
        assert_eq!(select_to_end(&(5000..6000), 1000), 1000..1000);
    }

    const HOUR: usize = 48_000 * 3600;

    #[test]
    fn fit_shows_the_whole_file() {
        let v = Viewport::fit(HOUR, 1200.0);
        assert_eq!(v.start, 0.0);
        assert!((v.end(1200.0) - HOUR as f64).abs() < 1e-3);
        assert!((v.frame_at(600.0) - HOUR as f64 / 2.0).abs() < 1e-3);
        assert!((v.x_of(HOUR as f64 / 4.0) - 300.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_the_frame_under_the_cursor() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        let under = v.frame_at(900.0);
        for _ in 0..10 {
            v.zoom_at(1.5, 900.0, 1200.0, HOUR);
            assert!((v.frame_at(900.0) - under).abs() < 1.0, "{} vs {under}", v.frame_at(900.0));
        }
        assert!(v.frames_per_px < Viewport::fit(HOUR, 1200.0).frames_per_px / 50.0);
        // All the way out again: back to the whole file, never past it.
        for _ in 0..40 {
            v.zoom_at(0.5, 900.0, 1200.0, HOUR);
        }
        assert_eq!(v, Viewport::fit(HOUR, 1200.0));
    }

    #[test]
    fn zoom_and_scroll_stop_at_the_limits() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        for _ in 0..200 {
            v.zoom_at(2.0, 0.0, 1200.0, HOUR);
        }
        assert_eq!(v.frames_per_px, 1.0 / 16.0, "closest zoom is 16 px per frame");
        v.scroll_by(-1e9, 1200.0, HOUR);
        assert_eq!(v.start, 0.0);
        v.scroll_by(1e12, 1200.0, HOUR);
        assert!((v.end(1200.0) - HOUR as f64).abs() < 1e-6, "{}", v.end(1200.0));
        // Bad numbers leave a usable view behind.
        v.zoom_at(f64::NAN, 10.0, 1200.0, HOUR);
        v.zoom_at(0.0, 10.0, 1200.0, HOUR);
        v.scroll_by(f32::NAN, 1200.0, HOUR);
        let mut broken = Viewport { start: f64::NAN, frames_per_px: -3.0 };
        broken.clamp(1200.0, HOUR);
        assert_eq!(broken, Viewport::fit(HOUR, 1200.0));
        // A file shorter than the window still fits and cannot scroll.
        let mut tiny = Viewport::fit(10, 1200.0);
        tiny.scroll_by(500.0, 1200.0, 10);
        assert_eq!(tiny.start, 0.0);
    }

    #[test]
    fn reveal_scrolls_only_when_the_frame_is_off_screen() {
        let mut v = Viewport { start: 100_000.0, frames_per_px: 100.0 };
        let before = v;
        v.reveal(150_000.0, 1000.0, HOUR);
        assert_eq!(v, before, "already visible");
        v.reveal(50_000.0, 1000.0, HOUR);
        assert!(v.start < 50_000.0 && v.end(1000.0) > 50_000.0);
        v.reveal(900_000.0, 1000.0, HOUR);
        assert!(v.start < 900_000.0 && v.end(1000.0) > 900_000.0);
    }

    #[test]
    fn selections_are_ordered_whichever_way_they_were_dragged() {
        let s = Selection { anchor: 900, head: 100 };
        assert_eq!((s.start(), s.end(), s.len(), s.is_empty()), (100, 900, 800, false));
        assert!(Selection::at(42).is_empty());
    }

    #[test]
    fn times_read_like_a_clock() {
        assert_eq!(format_time(0, 48_000), "0:00.000");
        assert_eq!(format_time(48_000 * 61 + 24_000, 48_000), "1:01.500");
        assert_eq!(format_time(HOUR + 48_000 * 125 + 48, 48_000), "1:02:05.001");
        assert_eq!(format_time(44_099, 44_100), "0:00.999");
        assert_eq!(format_time(5, 0), "0:00.000");
    }
}
