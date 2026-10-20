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

    /// The Zoom tool's click: zoom in (or out, with `out`) by 2x around `frame`, which stays
    /// where it is on screen.
    pub fn zoom_click(&mut self, frame: f64, out: bool, width_px: f32, total_frames: usize) {
        let anchor_x = self.x_of(frame).clamp(0.0, width_px.max(0.0));
        let factor = if out { 0.5 } else { 2.0 };
        self.zoom_at(factor, anchor_x, width_px, total_frames);
    }

    /// Fill the view with the range between frames `a` and `b` (either order), as the Zoom
    /// tool's drag and a right-drag in the ruler do. A range too small to show at the closest
    /// zoom is centred at that zoom; an empty or non-finite range changes nothing.
    pub fn zoom_to_range(&mut self, a: f64, b: f64, width_px: f32, total_frames: usize) {
        if !a.is_finite() || !b.is_finite() || a == b {
            return;
        }
        let (lo, hi) = (a.min(b).max(0.0), a.max(b).min(total_frames as f64));
        if hi <= lo {
            return;
        }
        let width = f64::from(width_px.max(1.0));
        let closest = 1.0 / MAX_PX_PER_FRAME;
        let scale = ((hi - lo) / width).max(closest);
        self.frames_per_px = scale;
        self.start = (lo + hi) * 0.5 - scale * width * 0.5;
        self.clamp(width_px, total_frames);
    }

    /// The Hand tool and middle-button drag: the picture follows the pointer, so dragging right
    /// by `dx_px` shows earlier audio.
    pub fn drag_by(&mut self, dx_px: f32, width_px: f32, total_frames: usize) {
        self.scroll_by(-dx_px, width_px, total_frames);
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
    fn hand_drag_moves_the_picture_with_the_pointer() {
        let mut v = Viewport { start: 100_000.0, frames_per_px: 100.0 };
        v.drag_by(50.0, 1000.0, HOUR);
        assert_eq!(v.start, 95_000.0, "drag right: earlier audio");
        v.drag_by(-50.0, 1000.0, HOUR);
        assert_eq!(v.start, 100_000.0);
        v.drag_by(1e9, 1000.0, HOUR);
        assert_eq!(v.start, 0.0, "stops at the start");
        v.drag_by(-1e12, 1000.0, HOUR);
        assert!((v.end(1000.0) - HOUR as f64).abs() < 1e-6, "stops at the end");
    }

    #[test]
    fn zoom_click_keeps_the_clicked_frame_under_the_pointer() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        let frame = 1_000_000.0;
        let x = v.x_of(frame);
        v.zoom_click(frame, false, 1200.0, HOUR);
        assert!((v.x_of(frame) - x).abs() < 1e-3);
        assert!((v.frames_per_px - Viewport::fit(HOUR, 1200.0).frames_per_px / 2.0).abs() < 1e-9);
        v.zoom_click(frame, true, 1200.0, HOUR);
        assert_eq!(v, Viewport::fit(HOUR, 1200.0), "out again is the whole file");
    }

    #[test]
    fn zoom_to_range_fills_the_view() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        v.zoom_to_range(480_000.0, 240_000.0, 1200.0, HOUR);
        assert!((v.start - 240_000.0).abs() < 1e-6 && (v.end(1200.0) - 480_000.0).abs() < 1e-6);
        // Past the file's ends it is clipped to the file.
        v.zoom_to_range(-5_000.0, 96_000.0, 1200.0, HOUR);
        assert_eq!(v.start, 0.0);
        assert!((v.end(1200.0) - 96_000.0).abs() < 1e-6);
        // Empty or bad ranges change nothing.
        let before = v;
        v.zoom_to_range(500.0, 500.0, 1200.0, HOUR);
        v.zoom_to_range(f64::NAN, 10.0, 1200.0, HOUR);
        assert_eq!(v, before);
        // Tiny ranges stop at the closest zoom, centred on the range.
        v.zoom_to_range(100_000.0, 100_002.0, 1200.0, HOUR);
        assert_eq!(v.frames_per_px, 1.0 / 16.0);
        assert!((v.frame_at(600.0) - 100_001.0).abs() < 1e-6);
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
