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

    /// Right-click in the waveform (Audition's default): the edge nearest `frame` moves there,
    /// so the selection grows or shrinks to the click and the other edge stays. A cursor with no
    /// selection becomes a selection from the cursor to `frame`. Equally near: the end moves.
    /// `frame` is clamped to `total` frames.
    pub fn extend_to(&self, frame: usize, total: usize) -> Selection {
        let frame = frame.min(total);
        let (start, end) = (self.start().min(total), self.end().min(total));
        if start == end {
            return Selection { anchor: start, head: frame };
        }
        if frame.abs_diff(start) < frame.abs_diff(end) { Selection { anchor: end, head: frame } } else { Selection { anchor: start, head: frame } }
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

/// A band of frequencies for a spectral selection (B135, B141): `lo_hz..hi_hz`, with an optional
/// feather that falls off outside it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Band {
    pub lo_hz: f32,
    pub hi_hz: f32,
    /// Width of the soft edge above and below the band, in Hz (0 is a hard edge).
    pub feather_hz: f32,
    /// Width of the soft edge before and after the time range, in milliseconds.
    pub feather_ms: f32,
}

impl Band {
    /// The band between `a` and `b` Hz in either order, held to `0..=nyquist` (a value that is
    /// not a number counts as 0), with hard edges.
    pub fn new(a: f32, b: f32, nyquist: f32) -> Band {
        let nyquist = if nyquist.is_finite() { nyquist.max(0.0) } else { 0.0 };
        let clean = |x: f32| if x.is_finite() { x.clamp(0.0, nyquist) } else { 0.0 };
        let (a, b) = (clean(a), clean(b));
        Band { lo_hz: a.min(b), hi_hz: a.max(b), feather_hz: 0.0, feather_ms: 0.0 }
    }
}

/// `lo-hi` in Hz, as STATE and the control channel report it.
impl std::fmt::Display for Band {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.lo_hz, self.hi_hz)
    }
}

/// A time range and a frequency band together: a rectangle on the spectral display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralSelection {
    pub time: Selection,
    pub band: Band,
    /// Sample rate, which turns the time feather from milliseconds into frames.
    pub rate: u32,
}

/// 1 at `dist <= 0`, falling along a raised cosine to 0 at `dist >= width`; 0.5 half way.
fn raised_cosine(dist: f64, width: f64) -> f64 {
    if dist <= 0.0 {
        1.0
    } else if width <= 0.0 || dist >= width {
        0.0
    } else {
        0.5 * (1.0 + (std::f64::consts::PI * dist / width).cos())
    }
}

impl SpectralSelection {
    /// How much of the audio at `frame` and `hz` the selection takes, 0 to 1: 1 inside the
    /// rectangle, 0 outside, and across each feather (which lies outside the edge) a raised
    /// cosine, so the middle of a feather is exactly 0.5. The time and frequency parts multiply.
    pub fn mask(&self, frame: usize, hz: f32) -> f32 {
        let (start, end) = (self.time.start(), self.time.end());
        // Frames before the start are `start - frame` away; frame `end` is 1 away from the last one inside.
        let t_dist = if frame < start {
            (start - frame) as f64
        } else if frame >= end {
            (frame - end + 1) as f64
        } else {
            0.0
        };
        let t_width = f64::from(self.band.feather_ms.max(0.0)) * f64::from(self.rate) / 1000.0;
        let hz = f64::from(hz);
        let f_dist = (f64::from(self.band.lo_hz) - hz).max(hz - f64::from(self.band.hi_hz)).max(0.0);
        let f_width = f64::from(self.band.feather_hz.max(0.0));
        (raised_cosine(t_dist, t_width) * raised_cosine(f_dist, f_width)) as f32
    }

    /// The selection moved by `dframes` and `dhz`, stopping at the file (`0..total` frames) and
    /// at `0..=nyquist` Hz with its length and width unchanged. A backwards drag stays backwards.
    pub fn moved(&self, dframes: i64, dhz: f32, total: usize, nyquist: f32) -> SpectralSelection {
        let (start, len) = (self.time.start() as i64, self.time.len() as i64);
        let total = total as i64;
        let new_start = (start.saturating_add(dframes)).clamp(0, (total - len).max(0));
        let shift = new_start - start;
        let shift_frame = |x: usize| (x as i64 + shift).max(0) as usize;
        let time = Selection { anchor: shift_frame(self.time.anchor), head: shift_frame(self.time.head) };
        let nyquist = if nyquist.is_finite() { nyquist.max(0.0) } else { 0.0 };
        let width = self.band.hi_hz - self.band.lo_hz;
        let dhz = if dhz.is_finite() { dhz } else { 0.0 };
        let lo = (self.band.lo_hz + dhz).clamp(0.0, (nyquist - width).max(0.0));
        SpectralSelection { time, band: Band { lo_hz: lo, hi_hz: lo + width, ..self.band }, rate: self.rate }
    }
}

/// Where `hz` sits on the spectral display's frequency axis `lo..hi`, 0 at `lo` (the bottom) to 1
/// at `hi` (the top), on the log axis when `log`. Held to 0..=1.
pub fn fraction_of_hz(lo: f32, hi: f32, log: bool, hz: f32) -> f32 {
    let t = if log { (hz / lo).ln() / (hi / lo).ln() } else { (hz - lo) / (hi - lo) };
    if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
}

/// The inverse of [`fraction_of_hz`]: the frequency at `t` (0 bottom, 1 top) of the axis.
pub fn hz_at_fraction(lo: f32, hi: f32, log: bool, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
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
    fn right_click_moves_the_nearest_edge() {
        let s = Selection { anchor: 100, head: 200 };
        // Outside on the right: the end grows. Outside on the left: the start grows.
        assert_eq!(s.extend_to(300, 1000), Selection { anchor: 100, head: 300 });
        assert_eq!(s.extend_to(40, 1000), Selection { anchor: 200, head: 40 });
        // Inside: the nearer edge shortens the selection.
        assert_eq!(s.extend_to(130, 1000), Selection { anchor: 200, head: 130 });
        assert_eq!(s.extend_to(170, 1000), Selection { anchor: 100, head: 170 });
        // Equally near: the end moves.
        assert_eq!(s.extend_to(150, 1000), Selection { anchor: 100, head: 150 });
    }

    #[test]
    fn right_click_works_whichever_way_the_selection_was_dragged() {
        let s = Selection { anchor: 200, head: 100 };
        let e = s.extend_to(300, 1000);
        assert_eq!((e.start(), e.end()), (100, 300));
        let e = s.extend_to(20, 1000);
        assert_eq!((e.start(), e.end()), (20, 200));
    }

    #[test]
    fn right_click_on_a_cursor_selects_from_it_and_is_clamped() {
        assert_eq!(Selection::at(500).extend_to(800, 1000), Selection { anchor: 500, head: 800 });
        let e = Selection::at(500).extend_to(100, 1000);
        assert_eq!((e.start(), e.end()), (100, 500));
        assert_eq!(Selection::at(500).extend_to(5000, 1000).end(), 1000);
        assert_eq!(Selection::at(0).extend_to(0, 0), Selection::at(0));
        // A selection past the end (the file got shorter) is clamped too.
        assert_eq!(Selection { anchor: 50, head: 900 }.extend_to(10, 100).end(), 100);
    }

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
