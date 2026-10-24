//! Stereo width control: scales the side signal of a left/right pair.
//!
//! Width 0 is mono (left equals right equals the mid signal), width 1 leaves the signal unchanged,
//! and larger values widen the image. The mid signal is never changed.

use crate::dsp::midside::{lr_to_ms, lr_to_ms_in_place, ms_to_lr, ms_to_lr_in_place};

/// The largest width factor; larger requests are treated as this value.
pub const MAX_WIDTH: f32 = 4.0;

/// Returns a usable width: NaN becomes 1 (unchanged) and the rest is clamped to 0 through [`MAX_WIDTH`].
fn clean_width(width: f32) -> f32 {
    if width.is_nan() {
        return 1.0;
    }
    width.clamp(0.0, MAX_WIDTH)
}

/// Scales the side signal of the shorter common prefix of `left` and `right` by `width`, in place.
/// Samples past the shorter slice are not touched.
pub fn widen(left: &mut [f32], right: &mut [f32], width: f32) {
    let width = clean_width(width);
    let n = left.len().min(right.len());
    let (left, right) = (&mut left[..n], &mut right[..n]);
    lr_to_ms_in_place(left, right);
    for side in right.iter_mut() {
        *side *= width;
    }
    ms_to_lr_in_place(left, right);
}

/// Like [`widen`] for interleaved stereo (`L, R, L, R, ...`). A last odd sample is not touched.
pub fn widen_interleaved(samples: &mut [f32], width: f32) {
    let width = clean_width(width);
    for frame in samples.chunks_exact_mut(2) {
        if let [left, right] = frame {
            let (mid, side) = lr_to_ms(*left, *right);
            (*left, *right) = ms_to_lr(mid, side * width);
        }
    }
}

/// Returns the side RMS divided by the mid RMS over the common prefix of the two slices.
/// Returns 0 when the mid RMS is 0 (silence, empty input, or a pure anti-phase signal).
pub fn measure_width(left: &[f32], right: &[f32]) -> f32 {
    let mut mid_sum = 0.0_f64;
    let mut side_sum = 0.0_f64;
    for (&l, &r) in left.iter().zip(right.iter()) {
        let (mid, side) = lr_to_ms(l, r);
        mid_sum += f64::from(mid) * f64::from(mid);
        side_sum += f64::from(side) * f64::from(side);
    }
    if mid_sum <= 0.0 {
        return 0.0;
    }
    (side_sum / mid_sum).sqrt() as f32
}
