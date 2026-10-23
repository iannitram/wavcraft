//! Screen and value mapping for the parametric EQ graph.

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
    /// STUB: always 0.
    pub fn x_of_freq(&self, _freq: f32) -> f32 {
        0.0
    }

    /// STUB: always 0.
    pub fn freq_at_x(&self, _x_px: f32) -> f32 {
        0.0
    }

    /// STUB: always 0.
    pub fn y_of_gain(&self, _gain_db: f32) -> f32 {
        0.0
    }

    /// STUB: always 0.
    pub fn gain_at_y(&self, _y_px: f32) -> f32 {
        0.0
    }
}

/// STUB: always (0, 0).
pub fn node_drag(_view: &GraphView, _x_px: f32, _y_px: f32) -> (f32, f32) {
    (0.0, 0.0)
}

/// STUB: always `None`.
pub fn nearest_node(_view: &GraphView, _nodes: &[(f32, f32)], _x_px: f32, _y_px: f32, _radius_px: f32) -> Option<usize> {
    None
}
