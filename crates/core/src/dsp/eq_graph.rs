//! Screen and value mapping for the parametric EQ graph.
//!
//! The horizontal axis is logarithmic from `f_lo` to `f_hi` Hz across `width_px` pixels. The vertical axis
//! is linear: `+db_range` dB at the top (y = 0) and `-db_range` dB at the bottom (y = `height_px`).
//! A view with a non-finite or non-positive size or range is unusable: every mapping on it returns 0 and
//! [`nearest_node`] returns `None`.

/// The size and ranges of the EQ graph on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphView {
    /// Frequency at the left edge, in Hz. Must be above 0.
    pub f_lo: f32,
    /// Frequency at the right edge, in Hz. Must be above `f_lo`.
    pub f_hi: f32,
    /// The graph shows `-db_range` to `+db_range` dB. Must be above 0.
    pub db_range: f32,
    /// Width of the graph in pixels. Must be above 0.
    pub width_px: f32,
    /// Height of the graph in pixels. Must be above 0.
    pub height_px: f32,
}

impl GraphView {
    fn is_usable(&self) -> bool {
        self.f_lo.is_finite()
            && self.f_lo > 0.0
            && self.f_hi.is_finite()
            && self.f_hi > self.f_lo
            && self.db_range.is_finite()
            && self.db_range > 0.0
            && self.width_px.is_finite()
            && self.width_px > 0.0
            && self.height_px.is_finite()
            && self.height_px > 0.0
    }

    /// Returns the x pixel of a frequency. Frequencies outside the view clamp to its edges; NaN gives 0.
    pub fn x_of_freq(&self, freq: f32) -> f32 {
        if !self.is_usable() || freq.is_nan() {
            return 0.0;
        }
        let lo = f64::from(self.f_lo);
        let hi = f64::from(self.f_hi);
        let freq = f64::from(freq).clamp(lo, hi);
        let t = (freq / lo).ln() / (hi / lo).ln();
        (t * f64::from(self.width_px)) as f32
    }

    /// Returns the frequency at an x pixel. Pixels outside the view clamp to its edges; NaN gives `f_lo`.
    pub fn freq_at_x(&self, x_px: f32) -> f32 {
        if !self.is_usable() {
            return 0.0;
        }
        let lo = f64::from(self.f_lo);
        let hi = f64::from(self.f_hi);
        let width = f64::from(self.width_px);
        let x = if x_px.is_nan() { 0.0 } else { f64::from(x_px) };
        let t = x.clamp(0.0, width) / width;
        (lo * (hi / lo).powf(t)) as f32
    }

    /// Returns the y pixel of a gain in dB. Gains outside the range clamp to the edges; NaN counts as 0 dB.
    pub fn y_of_gain(&self, gain_db: f32) -> f32 {
        if !self.is_usable() {
            return 0.0;
        }
        let range = f64::from(self.db_range);
        let gain = if gain_db.is_nan() { 0.0 } else { f64::from(gain_db).clamp(-range, range) };
        ((range - gain) / (2.0 * range) * f64::from(self.height_px)) as f32
    }

    /// Returns the gain in dB at a y pixel. Pixels outside the view clamp to the edges; NaN gives 0 dB.
    pub fn gain_at_y(&self, y_px: f32) -> f32 {
        if !self.is_usable() {
            return 0.0;
        }
        let range = f64::from(self.db_range);
        let height = f64::from(self.height_px);
        let y = if y_px.is_nan() { height / 2.0 } else { f64::from(y_px).clamp(0.0, height) };
        (range - y / height * 2.0 * range) as f32
    }
}

/// Returns the (frequency in Hz, gain in dB) a band node takes when dragged to a pixel, clamped to the view.
pub fn node_drag(view: &GraphView, x_px: f32, y_px: f32) -> (f32, f32) {
    (view.freq_at_x(x_px), view.gain_at_y(y_px))
}

/// Returns the index of the node (frequency in Hz, gain in dB) whose pixel is closest to the click, if it is
/// within `radius_px` pixels (inclusive). The first of equally close nodes wins. Nodes with a non-finite
/// value are skipped. Returns `None` for an unusable view, a non-finite click, or a NaN or negative radius.
pub fn nearest_node(view: &GraphView, nodes: &[(f32, f32)], x_px: f32, y_px: f32, radius_px: f32) -> Option<usize> {
    if !view.is_usable() || !x_px.is_finite() || !y_px.is_finite() || radius_px.is_nan() || radius_px < 0.0 {
        return None;
    }
    let limit = f64::from(radius_px) * f64::from(radius_px);
    let mut best: Option<(usize, f64)> = None;
    for (index, &(freq, gain)) in nodes.iter().enumerate() {
        if !freq.is_finite() || !gain.is_finite() {
            continue;
        }
        let dx = f64::from(view.x_of_freq(freq)) - f64::from(x_px);
        let dy = f64::from(view.y_of_gain(gain)) - f64::from(y_px);
        let dist = dx * dx + dy * dy;
        if dist > limit {
            continue;
        }
        let closer = match best {
            Some((_, best_dist)) => dist < best_dist,
            None => true,
        };
        if closer {
            best = Some((index, dist));
        }
    }
    best.map(|(index, _)| index)
}
