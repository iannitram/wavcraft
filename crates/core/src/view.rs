//! Zoom and scroll arithmetic, selections and time formatting. All positions are in frames.

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

/// Which parts of the view follow the owner from one open document to another (View menu,
/// kept in the settings file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncOptions {
    /// The time selection.
    pub selection: bool,
    /// Zoom and scroll: the same seconds on screen at the same scale.
    pub zoom: bool,
    /// The playhead / cursor.
    pub playhead: bool,
}

impl SyncOptions {
    /// Whether anything is carried over.
    pub fn any(&self) -> bool {
        self.selection || self.zoom || self.playhead
    }
}

/// What a document shows: where, what is selected, and the file it is a view of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocView {
    pub view: Viewport,
    pub selection: Selection,
    pub sample_rate: u32,
    pub frames: usize,
}

/// `frame` at `from_rate` as the nearest frame at `to_rate` (the same instant in seconds).
/// A rate of 0 keeps the number.
pub fn convert_frame(frame: usize, from_rate: u32, to_rate: u32) -> usize {
    if from_rate == 0 || to_rate == 0 || from_rate == to_rate {
        return frame;
    }
    let (num, den) = (frame as u128 * u128::from(to_rate), u128::from(from_rate));
    usize::try_from((num + den / 2) / den).unwrap_or(usize::MAX)
}

/// The same instant as a fractional frame count at another rate (for the view's edge).
fn convert_pos(pos: f64, from_rate: u32, to_rate: u32) -> f64 {
    if from_rate == 0 || to_rate == 0 { pos } else { pos * f64::from(to_rate) / f64::from(from_rate) }
}

/// What `to` should show after the owner switched to it from `from`, carrying over what `opts`
/// asks for by time in seconds, converted between the files' sample rates and clamped to
/// `to`'s length. What is not carried over stays as `to` had it. `width_px` is the editor's width.
pub fn carry_over(from: &DocView, to: &DocView, opts: SyncOptions, width_px: f32) -> (Viewport, Selection) {
    let mut view = to.view;
    let mut selection = to.selection;
    let map = |f: usize| convert_frame(f, from.sample_rate, to.sample_rate).min(to.frames);
    if opts.selection {
        selection = Selection { anchor: map(from.selection.anchor), head: map(from.selection.head) };
    } else if opts.playhead {
        // The cursor (the selection's start) moves; the target keeps no range then.
        selection = Selection::at(map(from.selection.start()));
    }
    if opts.zoom {
        view.frames_per_px = convert_pos(from.view.frames_per_px, from.sample_rate, to.sample_rate);
        view.start = convert_pos(from.view.start, from.sample_rate, to.sample_rate);
        view.clamp(width_px, to.frames);
    }
    (view, selection)
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

    fn dv(rate: u32, frames: usize, start: f64, fpp: f64, sel: (usize, usize)) -> DocView {
        DocView { view: Viewport { start, frames_per_px: fpp }, selection: Selection { anchor: sel.0, head: sel.1 }, sample_rate: rate, frames }
    }
    const ALL: SyncOptions = SyncOptions { selection: true, zoom: true, playhead: true };

    #[test]
    fn frames_convert_by_time_between_rates() {
        assert_eq!(convert_frame(48_000, 48_000, 44_100), 44_100);
        assert_eq!(convert_frame(44_100, 44_100, 96_000), 96_000);
        assert_eq!(convert_frame(1, 48_000, 44_100), 1);
        assert_eq!(convert_frame(7, 8_000, 48_000), 42);
        assert_eq!(convert_frame(5, 0, 48_000), 5);
        assert_eq!(convert_frame(usize::MAX, 1, 48_000), usize::MAX);
    }

    #[test]
    fn selection_carries_by_seconds_and_clamps_to_the_length() {
        let from = dv(48_000, 480_000, 0.0, 400.0, (96_000, 240_000)); // 2 s .. 5 s
        let to = dv(44_100, 176_400, 0.0, 200.0, (0, 0)); // 4 s long
        let (_, sel) = carry_over(&from, &to, SyncOptions { selection: true, ..Default::default() }, 1000.0);
        assert_eq!((sel.anchor, sel.head), (88_200, 176_400));
    }

    #[test]
    fn a_reversed_selection_keeps_its_direction() {
        let from = dv(48_000, 480_000, 0.0, 400.0, (96_000, 48_000));
        let to = dv(24_000, 480_000, 0.0, 200.0, (0, 0));
        let (_, sel) = carry_over(&from, &to, SyncOptions { selection: true, ..Default::default() }, 1000.0);
        assert_eq!((sel.anchor, sel.head), (48_000, 24_000));
    }

    #[test]
    fn zoom_keeps_the_same_seconds_on_screen() {
        // 1 s from 10 s across 1000 px at 48 kHz.
        let from = dv(48_000, 48_000 * 60, 480_000.0, 48.0, (0, 0));
        let to = dv(44_100, 44_100 * 60, 0.0, 1000.0, (0, 0));
        let (v, _) = carry_over(&from, &to, SyncOptions { zoom: true, ..Default::default() }, 1000.0);
        assert!((v.start - 441_000.0).abs() < 1e-6);
        assert!((v.frames_per_px - 44.1).abs() < 1e-9);
        assert!((v.end(1000.0) - 485_100.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_is_clamped_to_a_shorter_file() {
        let from = dv(48_000, 48_000 * 60, 48_000.0 * 50.0, 48.0, (0, 0)); // 50..51 s
        let to = dv(48_000, 48_000 * 10, 0.0, 480.0, (0, 0)); // 10 s
        let (v, _) = carry_over(&from, &to, SyncOptions { zoom: true, ..Default::default() }, 1000.0);
        assert!(v.start >= 0.0 && v.end(1000.0) <= 480_000.0 + 1e-6, "{v:?}");
        assert!((v.start - (480_000.0 - 48.0 * 1000.0)).abs() < 1e-6, "scrolled to the end: {v:?}");
    }

    #[test]
    fn playhead_alone_moves_the_cursor_not_the_range() {
        let from = dv(48_000, 480_000, 0.0, 400.0, (96_000, 240_000));
        let to = dv(96_000, 960_000, 100.0, 300.0, (5, 50));
        let (v, sel) = carry_over(&from, &to, SyncOptions { playhead: true, ..Default::default() }, 1000.0);
        assert_eq!((sel.anchor, sel.head), (192_000, 192_000));
        assert_eq!(v, to.view);
    }

    #[test]
    fn nothing_on_leaves_the_target_as_it_was() {
        let from = dv(48_000, 480_000, 0.0, 400.0, (96_000, 240_000));
        let to = dv(96_000, 960_000, 100.0, 300.0, (5, 50));
        assert_eq!(carry_over(&from, &to, SyncOptions::default(), 1000.0), (to.view, to.selection));
        assert!(!SyncOptions::default().any() && ALL.any());
    }

    #[test]
    fn all_three_together() {
        let from = dv(48_000, 480_000, 48_000.0, 48.0, (96_000, 96_000));
        let to = dv(8_000, 40_000, 0.0, 40.0, (0, 0)); // 5 s at 8 kHz
        let (v, sel) = carry_over(&from, &to, ALL, 1000.0);
        assert_eq!((sel.anchor, sel.head), (16_000, 16_000));
        assert!((v.frames_per_px - 8.0).abs() < 1e-9 && (v.start - 8_000.0).abs() < 1e-6, "{v:?}");
    }
}
