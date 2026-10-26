//! Fade curves (v1 E1, issue #27): linear, logarithmic and cosine (S-curve) gain formulas
//! and the pointer-drag rule that chooses between them.

/// Vertical pointer movement (in pixels) needed to switch a handle drag from linear to logarithmic.
pub const DRAG_LOG_THRESHOLD_PX: f32 = 8.0;

/// The shape of a fade-in or fade-out envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FadeCurve {
    /// Straight-line ramp: `g(t) = t`.
    #[default]
    Linear,
    /// Fast initial rise (convex): `g(t) = log10(1 + 9 * t)`.
    Logarithmic,
    /// Raised-cosine S-curve: `g(t) = 0.5 * (1 - cos(pi * t))`.
    Cosine,
}

/// Choose the fade curve for a top-corner handle drag with vertical offset `dy_px` and modifier `ctrl`.
///
/// Holding Ctrl chooses [`FadeCurve::Cosine`]; otherwise dragging up or down by at least
/// [`DRAG_LOG_THRESHOLD_PX`] pixels chooses [`FadeCurve::Logarithmic`]; level drags choose [`FadeCurve::Linear`].
pub fn curve_from_drag(dy_px: f32, ctrl: bool) -> FadeCurve {
    // STUB: card wav-c-27-a fills this.
    let _ = (dy_px, ctrl);
    FadeCurve::Linear
}

/// Amplitude multiplier (`0.0..=1.0`) for a fade-in at normalized position `t` (`0.0` = start, `1.0` = end).
///
/// Non-finite `t` returns `0.0`; values outside `0.0..=1.0` are clamped to `0.0..=1.0`.
pub fn fade_in_gain(t: f64, curve: FadeCurve) -> f64 {
    // STUB: card wav-c-27-a fills this.
    let _ = (t, curve);
    0.0
}

/// Amplitude multiplier (`0.0..=1.0`) for a fade-out at normalized position `t` (`0.0` = start, `1.0` = end).
///
/// Non-finite `t` returns `0.0`; values outside `0.0..=1.0` are clamped to `0.0..=1.0`.
pub fn fade_out_gain(t: f64, curve: FadeCurve) -> f64 {
    // STUB: card wav-c-27-a fills this.
    let _ = (t, curve);
    0.0
}
