//! Fade curve shapes (roadmap row A077, sub-job a: the maths behind the corner-handle drag).
//!
//! A fade-in gain curve over `t` in 0..1 comes in four shapes, all exactly 0 at `t = 0` and
//! exactly 1 at `t = 1`, and never decreasing in between:
//!
//! - [`CurveKind::Linear`] is `t`.
//! - [`CurveKind::Logarithmic`] is `log10(1 + 9 t)`. Rises fast, then flattens (0.7404 at the middle).
//! - [`CurveKind::Cosine`] is `0.5 - 0.5 cos(pi t)`. A raised cosine, 0.5 at the middle.
//! - [`CurveKind::Exponential`] is `(10^t - 1) / 9`. Rises slowly, then fast (0.2403 at the middle).
//!
//! The curves are computed in `f64` and rounded to `f32` once at the end.
//!
//! [`fade_in`] and [`fade_out`] shape a slice in place: sample `i` of `n` is multiplied by the gain
//! at `t = i / (n - 1)` (fade-in) or at `t = (n - 1 - i) / (n - 1)` (fade-out), so the first and last
//! samples are scaled by exactly 0 and 1. A slice of fewer than two samples is left unchanged (a
//! curve needs two points). [`curve_from_drag`] turns the point the user dragged the curve to into
//! the nearest shape.
//!
//! This is the core only: drawing the handles and applying the fade to a selection are not here.

use std::f64::consts::PI;

/// The shape of a fade curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    Linear,
    Logarithmic,
    Cosine,
    Exponential,
}

impl CurveKind {
    /// Every shape, in the order that breaks ties in [`curve_from_drag`].
    pub const ALL: [CurveKind; 4] = [CurveKind::Linear, CurveKind::Logarithmic, CurveKind::Cosine, CurveKind::Exponential];
}

/// The fade-in gain of `kind` at `t` (0 = start of the fade, 1 = end).
///
/// `t` is clamped to 0..1 (so `-inf` is 0 and `+inf` is 1) and NaN gives 0.
pub fn fade_gain(kind: CurveKind, t: f32) -> f32 {
    if t.is_nan() {
        return 0.0;
    }
    let x = f64::from(t.clamp(0.0, 1.0));
    let g = match kind {
        CurveKind::Linear => x,
        CurveKind::Logarithmic => (1.0 + 9.0 * x).log10(),
        CurveKind::Cosine => 0.5 - 0.5 * (PI * x).cos(),
        CurveKind::Exponential => (10.0_f64.powf(x) - 1.0) / 9.0,
    };
    g as f32
}

/// Fades the slice in with the given shape, in place (see the module comment for the rule).
pub fn fade_in(samples: &mut [f32], kind: CurveKind) {
    let n = samples.len();
    if n < 2 {
        return;
    }
    let last = (n - 1) as f32;
    for (i, s) in samples.iter_mut().enumerate() {
        *s *= fade_gain(kind, i as f32 / last);
    }
}

/// Fades the slice out with the given shape, in place: the mirror of [`fade_in`], so the result
/// on a slice of ones is the result of `fade_in` reversed.
pub fn fade_out(samples: &mut [f32], kind: CurveKind) {
    let n = samples.len();
    if n < 2 {
        return;
    }
    let last = (n - 1) as f32;
    for (i, s) in samples.iter_mut().enumerate() {
        *s *= fade_gain(kind, (n - 1 - i) as f32 / last);
    }
}

/// The shape that best fits a curve the user dragged through the point (`t`, `gain`): the fade
/// time `t` in 0..1 and the gain 0..1 the handle was dragged to at that time.
///
/// Both are clamped to 0..1. Picks the shape whose [`fade_gain`] at `t` is nearest to `gain`; a
/// tie goes to the earlier shape in [`CurveKind::ALL`]. At `t = 0.5` Linear and Cosine are both
/// 0.5, so a drag to the exact middle gives Linear; a drag at `t = 0.25` tells all four apart. A
/// NaN `t` or `gain`, and `t` of 0 or 1 (where all shapes agree), give Linear.
pub fn curve_from_drag(t: f32, gain: f32) -> CurveKind {
    let gain = gain.clamp(0.0, 1.0);
    let mut best = CurveKind::Linear;
    let mut best_gap = f32::INFINITY;
    for kind in CurveKind::ALL {
        let gap = (fade_gain(kind, t) - gain).abs();
        if gap < best_gap {
            best = kind;
            best_gap = gap;
        }
    }
    best
}
