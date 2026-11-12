//! The Episode Map (M2): the whole file as one strip, each pixel column classed as silence,
//! speech or loud from its waveform peak, and the arithmetic that places markers and Episode
//! Check findings on that strip.
//!
//! Everything here is a pure function over numbers the caller already has (the overview's
//! `(min, max)` columns from [`Document::waveform_columns`](crate::Document::waveform_columns),
//! frame positions), so the map costs nothing beyond the overview it is drawn from.

/// What a column of the map holds, by its peak level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// Peak under [`SILENCE_BELOW_DB`]: digital silence, or room noise between talkers.
    Silence,
    /// Peak from [`SILENCE_BELOW_DB`] up to (not including) [`LOUD_FROM_DB`].
    Speech,
    /// Peak at or above [`LOUD_FROM_DB`]: near clipping.
    Loud,
}

/// A column whose peak is under this (dBFS) is silence.
pub const SILENCE_BELOW_DB: f32 = -60.0;
/// A column whose peak is at or above this (dBFS) is loud.
pub const LOUD_FROM_DB: f32 = -6.0;

/// The class of a column whose peak is `peak_db` dBFS. A value that is not a number (a peak of
/// a damaged column) counts as silence; minus infinity (a peak of exactly zero) is silence too.
pub fn classify(peak_db: f32) -> Class {
    if peak_db.is_nan() || peak_db < SILENCE_BELOW_DB {
        Class::Silence
    } else if peak_db >= LOUD_FROM_DB {
        Class::Loud
    } else {
        Class::Speech
    }
}

/// One [`Class`] per `(min, max)` peak column, in order. The column's peak is the larger of
/// `|min|` and `|max|`; a non-number side is ignored (both non-numbers make the column silent).
pub fn columns(cols: &[(f32, f32)]) -> Vec<Class> {
    cols.iter().map(|&(lo, hi)| classify(peak_db(lo, hi))).collect()
}

/// The peak of one column in dBFS (minus infinity for a silent or unreadable column).
fn peak_db(lo: f32, hi: f32) -> f32 {
    // `f32::max` returns the other side when one is NaN, and 0.0 when both are.
    let peak = lo.abs().max(hi.abs()).max(0.0);
    if peak > 0.0 { 20.0 * peak.log10() } else { f32::NEG_INFINITY }
}

/// The x position (pixels from the strip's left edge) of `frame` on a strip `width_px` wide
/// showing `total_frames`. A frame past the end lands on the right edge. With no frames, or a
/// width that is not a positive finite number, everything lands at 0.0.
pub fn pin_x(frame: u64, total_frames: u64, width_px: f32) -> f32 {
    if total_frames == 0 || !width_px.is_finite() || width_px <= 0.0 {
        return 0.0;
    }
    let frac = frame.min(total_frames) as f64 / total_frames as f64;
    (frac * f64::from(width_px)) as f32
}

/// The `(left, right)` x positions of the frame range `start..end` on the strip, as [`pin_x`]
/// places each end. The ends may come in either order; `left` is never past `right`.
pub fn span_x(start: u64, end: u64, total_frames: u64, width_px: f32) -> (f32, f32) {
    let (a, b) = if start <= end { (start, end) } else { (end, start) };
    (pin_x(a, total_frames, width_px), pin_x(b, total_frames, width_px))
}
