//! Phase scope numbers (roadmap row A040, sub-job a: the data behind a Phase Analysis panel).
//!
//! The correlation number is in [`super::phase`]. This module adds what a goniometer view needs:
//!
//! - [`scope_points`]: one `(x, y)` point per sample pair, `x = (r - l) / sqrt(2)` and
//!   `y = (l + r) / sqrt(2)`. Identical channels draw a vertical line (`x = 0`), inverted channels a
//!   horizontal line (`y = 0`), left only a line leaning to the left, right only a line leaning right.
//!   A long signal is thinned to at most `max_points` points: point `i` of `count` is sample pair
//!   `i * n / count` (rounded down), so order is kept and the first pair is always included.
//! - [`balance`]: `(rms_r - rms_l) / (rms_r + rms_l)`, from -1 (all left) to +1 (all right); 0 when
//!   both channels are silent or equal.
//! - [`mid_side_rms_db`]: the RMS level in dB of the mid `(l + r) / 2` and the side `(l - r) / 2`
//!   signals, never below [`FLOOR_DB`].
//!
//! All three work on the common prefix of the two channels, and a sample that is not finite counts
//! as 0. This is the core only: drawing the panel is not here.

/// The lowest level [`mid_side_rms_db`] reports, in dB (silence).
pub const FLOOR_DB: f64 = -120.0;

/// The goniometer points `(x, y)` for the common prefix of `left` and `right`, thinned to at most
/// `max_points` (see the module comment). Empty input or `max_points == 0` gives an empty vector.
pub fn scope_points(left: &[f32], right: &[f32], max_points: usize) -> Vec<(f32, f32)> {
    let n = left.len().min(right.len());
    let count = n.min(max_points);
    let mut points = Vec::with_capacity(count);
    let clean = |v: f32| if v.is_finite() { f64::from(v) } else { 0.0 };

    for i in 0..count {
        let index = ((i as u128 * n as u128) / count as u128) as usize;
        if let (Some(&l), Some(&r)) = (left.get(index), right.get(index)) {
            let l = clean(l);
            let r = clean(r);
            points.push((((r - l) * std::f64::consts::FRAC_1_SQRT_2) as f32, ((l + r) * std::f64::consts::FRAC_1_SQRT_2) as f32));
        }
    }

    points
}

/// The left/right balance by RMS, -1.0 (all left) to +1.0 (all right); 0.0 for silence or empty input.
pub fn balance(left: &[f32], right: &[f32]) -> f32 {
    let n = left.len().min(right.len()).max(1) as f64;
    let mut left_sum = 0.0_f64;
    let mut right_sum = 0.0_f64;
    let clean = |v: f32| if v.is_finite() { f64::from(v) } else { 0.0 };

    for (&l, &r) in left.iter().zip(right.iter()) {
        let l = clean(l);
        let r = clean(r);
        left_sum += l * l;
        right_sum += r * r;
    }

    let left_rms = (left_sum / n).sqrt();
    let right_rms = (right_sum / n).sqrt();
    let total = left_rms + right_rms;

    if total == 0.0 {
        return 0.0;
    }
    ((right_rms - left_rms) / total) as f32
}

/// The RMS level in dB of the mid and side signals as `(mid_db, side_db)`, floored at [`FLOOR_DB`].
pub fn mid_side_rms_db(left: &[f32], right: &[f32]) -> (f32, f32) {
    let n = left.len().min(right.len()).max(1) as f64;
    let mut mid_sum = 0.0_f64;
    let mut side_sum = 0.0_f64;
    let clean = |v: f32| if v.is_finite() { f64::from(v) } else { 0.0 };

    for (&l, &r) in left.iter().zip(right.iter()) {
        let l = clean(l);
        let r = clean(r);
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5;
        mid_sum += mid * mid;
        side_sum += side * side;
    }

    let mid_db = (20.0 * (mid_sum / n).sqrt().log10()).max(FLOOR_DB);
    let side_db = (20.0 * (side_sum / n).sqrt().log10()).max(FLOOR_DB);

    (mid_db as f32, side_db as f32)
}
