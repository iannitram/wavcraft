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

/// Which end of a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// How close (in pixels) the pointer must be to a selection edge to grab it.
pub const GRAB_PX: f32 = 5.0;

impl Selection {
    /// The whole file `0..total_frames`.
    pub fn all(total_frames: usize) -> Selection {
        Selection { anchor: 0, head: total_frames }
    }

    /// What is on screen in `view` (`width_px` wide), clamped to the file.
    pub fn visible(view: &Viewport, width_px: f32, total_frames: usize) -> Selection {
        let a = view.start.max(0.0).round().min(total_frames as f64) as usize;
        let b = view.end(width_px).max(0.0).round().min(total_frames as f64) as usize;
        Selection { anchor: a.min(b), head: b }
    }

    /// The edge of a non-empty selection within `tol_px` of pixel `x_px` (the nearer one when
    /// both are), or `None`. A bare cursor has no edges to grab.
    pub fn edge_near(&self, view: &Viewport, x_px: f32, tol_px: f32) -> Option<Edge> {
        if self.is_empty() {
            return None;
        }
        let ds = (view.x_of(self.start() as f64) - x_px).abs();
        let de = (view.x_of(self.end() as f64) - x_px).abs();
        match (ds <= tol_px, de <= tol_px) {
            (true, true) => Some(if ds <= de { Edge::Start } else { Edge::End }),
            (true, false) => Some(Edge::Start),
            (false, true) => Some(Edge::End),
            _ => None,
        }
    }

    /// Pick up `edge`: the opposite edge becomes the anchor and the grabbed edge the head, so
    /// setting `head` afterwards moves that edge (and flips the selection if it crosses).
    pub fn grab(&mut self, edge: Edge) {
        let (s, e) = (self.start(), self.end());
        *self = match edge {
            Edge::Start => Selection { anchor: e, head: s },
            Edge::End => Selection { anchor: s, head: e },
        };
    }

    /// Shift-click: move the nearer edge to `frame` (a cursor extends toward it).
    pub fn extend_to(&mut self, frame: usize) {
        if !self.is_empty() {
            let (s, e) = (self.start(), self.end());
            let near_start = frame.abs_diff(s) <= frame.abs_diff(e);
            self.grab(if near_start { Edge::Start } else { Edge::End });
        }
        self.head = frame;
    }

    /// Shift+arrow: move the moving end by `delta` frames, within `0..=total_frames`.
    pub fn extend_by(&mut self, delta: i64, total_frames: usize) {
        let h = (self.head as i64).saturating_add(delta).clamp(0, total_frames as i64);
        self.head = h as usize;
    }

    /// Shift+Home / Shift+End: move the moving end to `frame` (clamped to the file).
    pub fn extend_edge_to(&mut self, frame: usize, total_frames: usize) {
        self.head = frame.min(total_frames);
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
    fn selections_are_ordered_whichever_way_they_were_dragged() {
        let s = Selection { anchor: 900, head: 100 };
        assert_eq!((s.start(), s.end(), s.len(), s.is_empty()), (100, 900, 800, false));
        assert!(Selection::at(42).is_empty());
    }

    #[test]
    fn edges_are_grabbed_within_a_few_pixels_and_cross_cleanly() {
        let v = Viewport { start: 0.0, frames_per_px: 10.0 };
        let mut s = Selection { anchor: 1000, head: 500 }; // 500..1000 = px 50..100
        assert_eq!(s.edge_near(&v, 52.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(s.edge_near(&v, 97.0, GRAB_PX), Some(Edge::End));
        assert_eq!(s.edge_near(&v, 75.0, GRAB_PX), None);
        assert_eq!(Selection::at(500).edge_near(&v, 50.0, GRAB_PX), None, "a cursor has no edges");
        // Both edges in reach: the nearer wins.
        let thin = Selection { anchor: 500, head: 540 };
        assert_eq!(thin.edge_near(&v, 51.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(thin.edge_near(&v, 52.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(thin.edge_near(&v, 53.0, GRAB_PX), Some(Edge::End));
        // Drag the start edge right past the end: the selection flips, the other edge stays.
        s.grab(Edge::Start);
        s.head = 1500;
        assert_eq!((s.start(), s.end()), (1000, 1500));
        s.grab(Edge::End);
        s.head = 200;
        assert_eq!((s.start(), s.end()), (200, 1000));
    }

    #[test]
    fn shift_click_moves_the_nearer_edge() {
        let mut s = Selection { anchor: 500, head: 1000 };
        s.extend_to(1400);
        assert_eq!((s.start(), s.end()), (500, 1400));
        s.extend_to(100);
        assert_eq!((s.start(), s.end()), (100, 1400));
        s.extend_to(120);
        assert_eq!((s.start(), s.end()), (120, 1400));
        let mut c = Selection::at(700);
        c.extend_to(300);
        assert_eq!((c.start(), c.end()), (300, 700));
        let mut c = Selection::at(700);
        c.extend_to(900);
        assert_eq!((c.start(), c.end()), (700, 900));
    }

    #[test]
    fn double_click_is_the_visible_range_and_triple_is_everything() {
        let v = Viewport { start: 1000.0, frames_per_px: 10.0 };
        let s = Selection::visible(&v, 200.0, 100_000);
        assert_eq!((s.start(), s.end()), (1000, 3000));
        let past = Selection::visible(&Viewport { start: 99_000.0, frames_per_px: 10.0 }, 500.0, 100_000);
        assert_eq!((past.start(), past.end()), (99_000, 100_000), "clamped to the file");
        let a = Selection::all(100_000);
        assert_eq!((a.start(), a.end()), (0, 100_000));
    }

    #[test]
    fn shift_arrows_move_the_head_within_the_file() {
        let mut s = Selection::at(100);
        s.extend_by(50, 1000);
        assert_eq!((s.start(), s.end()), (100, 150));
        s.extend_by(-80, 1000);
        assert_eq!((s.start(), s.end()), (70, 100), "crosses the anchor and flips");
        s.extend_by(-1_000_000, 1000);
        assert_eq!((s.start(), s.end()), (0, 100));
        s.extend_by(i64::MAX, 1000);
        assert_eq!((s.start(), s.end()), (100, 1000));
        s.extend_edge_to(0, 1000);
        assert_eq!((s.start(), s.end()), (0, 100));
        s.extend_edge_to(5000, 1000);
        assert_eq!((s.start(), s.end()), (100, 1000));
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
