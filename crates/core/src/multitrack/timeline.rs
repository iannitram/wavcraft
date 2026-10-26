//! The view model for the multitrack timeline: which seconds are on screen, how far zoomed in,
//! how tall a track row is. Pure arithmetic in seconds and pixels (all `f64`); no drawing and no
//! session access. The caller passes the session's end time (in seconds) and the timeline width
//! (in pixels) to each call and casts to `f32` where the UI needs it.
//!
//! The zoom runs from [`MIN_SECONDS_PER_PX`] (closest) out to the fit level (the whole session
//! across the width). Scrolling stops at 0 on the left and a little past the end on the right.

/// The closest zoom: one pixel covers at least this many seconds.
pub const MIN_SECONDS_PER_PX: f64 = 0.0005;
/// The shortest a track row may be, in pixels.
pub const MIN_TRACK_HEIGHT_PX: f64 = 24.0;
/// The tallest a track row may be, in pixels.
pub const MAX_TRACK_HEIGHT_PX: f64 = 400.0;
/// How much one step of [`TimelineView::zoom_vertical`] changes the row height, in pixels.
pub const TRACK_HEIGHT_STEP_PX: f64 = 16.0;
/// The row height of a new view, in pixels.
pub const DEFAULT_TRACK_HEIGHT_PX: f64 = 80.0;
/// The zoom of a new view: 10 ms per pixel.
pub const DEFAULT_SECONDS_PER_PX: f64 = 0.01;
/// How far past the session end the view may scroll, in pixels at the current zoom.
pub const END_MARGIN_PX: f64 = 40.0;

/// What part of the session the timeline shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineView {
    /// The time at the left edge, in seconds.
    pub start_s: f64,
    /// How many seconds one pixel covers.
    pub seconds_per_px: f64,
    /// The height of one track row, in pixels.
    pub track_height_px: f64,
}

impl Default for TimelineView {
    fn default() -> TimelineView {
        TimelineView { start_s: 0.0, seconds_per_px: DEFAULT_SECONDS_PER_PX, track_height_px: DEFAULT_TRACK_HEIGHT_PX }
    }
}

/// The zoom that puts the whole session across the width: `session_end_s / width_px`, but never
/// below [`MIN_SECONDS_PER_PX`].
pub fn fit_seconds_per_px(session_end_s: f64, width_px: f64) -> f64 {
    let end = if session_end_s.is_finite() && session_end_s > 0.0 { session_end_s } else { 0.0 };
    let w = if width_px.is_finite() { width_px.max(1.0) } else { 1.0 };
    (end / w).max(MIN_SECONDS_PER_PX)
}

/// The furthest the left edge may scroll: 0 while the whole session fits in the width, else the
/// session end less the visible seconds plus [`END_MARGIN_PX`] pixels' worth of seconds.
pub fn max_start_s(seconds_per_px: f64, width_px: f64, session_end_s: f64) -> f64 {
    let end = if session_end_s.is_finite() && session_end_s > 0.0 { session_end_s } else { 0.0 };
    let w = if width_px.is_finite() { width_px.max(1.0) } else { 1.0 };
    let spp = if seconds_per_px.is_finite() && seconds_per_px > 0.0 { seconds_per_px } else { MIN_SECONDS_PER_PX };
    let visible = w * spp;
    if end <= visible * (1.0 + 1e-9) { 0.0 } else { end - visible + END_MARGIN_PX * spp }
}

impl TimelineView {
    /// The whole session across `width_px`: starts at 0, default row height.
    pub fn fit(session_end_s: f64, width_px: f64) -> TimelineView {
        TimelineView { start_s: 0.0, seconds_per_px: fit_seconds_per_px(session_end_s, width_px), track_height_px: DEFAULT_TRACK_HEIGHT_PX }
    }

    /// Put the zoom and the left edge back inside their limits.
    pub fn clamp(&mut self, width_px: f64, session_end_s: f64) {
        let fit = fit_seconds_per_px(session_end_s, width_px);
        if !self.seconds_per_px.is_finite() || self.seconds_per_px <= 0.0 {
            self.seconds_per_px = fit;
        }
        self.seconds_per_px = self.seconds_per_px.clamp(MIN_SECONDS_PER_PX, fit);
        let last = max_start_s(self.seconds_per_px, width_px, session_end_s);
        self.start_s = if self.start_s.is_finite() { self.start_s.clamp(0.0, last) } else { 0.0 };
    }

    /// Zoom by `factor` (above 1 zooms in, below 1 out) so the time under pixel `anchor_px`
    /// stays under it. A `factor` that is not a positive finite number changes nothing.
    pub fn zoom_at(&mut self, factor: f64, anchor_px: f64, width_px: f64, session_end_s: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let w = if width_px.is_finite() { width_px.max(1.0) } else { 1.0 };
        let anchor_px = if anchor_px.is_finite() { anchor_px.clamp(0.0, w) } else { 0.0 };
        let anchor_t = self.time_at(anchor_px);
        let fit = fit_seconds_per_px(session_end_s, width_px);
        self.seconds_per_px = (self.seconds_per_px / factor).clamp(MIN_SECONDS_PER_PX, fit);
        self.start_s = anchor_t - self.seconds_per_px * anchor_px;
        self.clamp(width_px, session_end_s);
    }

    /// Scroll by `dx_px` pixels (positive moves later in the session).
    pub fn scroll_by(&mut self, dx_px: f64, width_px: f64, session_end_s: f64) {
        if dx_px.is_finite() {
            self.start_s += dx_px * self.seconds_per_px;
        }
        self.clamp(width_px, session_end_s);
    }

    /// Make the track rows taller (positive `steps`) or shorter (negative).
    pub fn zoom_vertical(&mut self, steps: i32) {
        let h = if self.track_height_px.is_finite() { self.track_height_px } else { DEFAULT_TRACK_HEIGHT_PX };
        self.track_height_px = (h + f64::from(steps) * TRACK_HEIGHT_STEP_PX).clamp(MIN_TRACK_HEIGHT_PX, MAX_TRACK_HEIGHT_PX);
    }

    /// The time in seconds under pixel `x_px`.
    pub fn time_at(&self, x_px: f64) -> f64 {
        self.start_s + x_px * self.seconds_per_px.max(MIN_SECONDS_PER_PX)
    }

    /// The pixel where time `t_s` is drawn (may be off screen).
    pub fn x_of(&self, t_s: f64) -> f64 {
        (t_s - self.start_s) / self.seconds_per_px.max(MIN_SECONDS_PER_PX)
    }
}
