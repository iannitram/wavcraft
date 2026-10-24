//! Spectral phase display, part a (roadmap A068). The display shows, for every frequency bin, how
//! far the right channel runs ahead of or behind the left channel. This file holds the numbers
//! behind it and draws nothing: the phase difference per bin, its mapping onto 0..1 for a colour
//! ramp, and the share of bins that are in phase.
//!
//! A bin is a complex number `(re, im)` as `(f32, f32)`, as it comes out of a short-time Fourier
//! transform. All maths run in f64 and are cast to f32 once, at the end.

use std::f64::consts::PI;

/// The phase of each `right` bin relative to the matching `left` bin, in radians in `(-pi, pi]`,
/// over the common prefix of the two slices. A positive value means the right channel is ahead.
/// A pair gives 0.0 when either bin is exactly `(0.0, 0.0)`, when any of the four parts is not
/// finite, or when the result is not finite.
pub fn phase_difference(left: &[(f32, f32)], right: &[(f32, f32)]) -> Vec<f32> {
    left.iter().zip(right.iter()).map(|(&l, &r)| bin_difference(l, r)).collect()
}

fn bin_difference(left: (f32, f32), right: (f32, f32)) -> f32 {
    let (lr, li) = (f64::from(left.0), f64::from(left.1));
    let (rr, ri) = (f64::from(right.0), f64::from(right.1));
    if !(lr.is_finite() && li.is_finite() && rr.is_finite() && ri.is_finite()) {
        return 0.0;
    }
    let re = lr * rr + li * ri;
    let im = lr * ri - li * rr;
    if re == 0.0 && im == 0.0 {
        return 0.0;
    }
    let angle = im.atan2(re);
    if !angle.is_finite() {
        return 0.0;
    }
    (if angle <= -PI { PI } else { angle }) as f32
}

/// Maps a phase difference in radians onto `0.0..=1.0` for a colour ramp: -pi gives 0.0, 0 gives
/// 0.5 and pi gives 1.0. A value outside `-pi..=pi` is clamped; NaN gives 0.5.
pub fn phase_to_unit(phase: f32) -> f32 {
    if phase.is_nan() {
        return 0.5;
    }
    let t = (f64::from(phase) / PI + 1.0) / 2.0;
    t.clamp(0.0, 1.0) as f32
}

/// One display row: `phase_difference` of the two slices, each value passed through
/// `phase_to_unit`.
pub fn phase_row(left: &[(f32, f32)], right: &[(f32, f32)]) -> Vec<f32> {
    phase_difference(left, right).into_iter().map(phase_to_unit).collect()
}

/// The share of `differences` (0.0 to 1.0) whose absolute value is at most `tolerance` radians.
/// A value that is NaN is never counted, and a negative or NaN `tolerance` counts nothing. Returns
/// 0.0 for an empty slice.
pub fn in_phase_fraction(differences: &[f32], tolerance: f32) -> f32 {
    if differences.is_empty() {
        return 0.0;
    }
    let inside = differences.iter().filter(|d| d.abs() <= tolerance).count();
    (inside as f64 / differences.len() as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn approx(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn equal_channels_have_zero_difference() {
        let bins = [(1.0, 0.0), (0.0, 2.0), (-3.0, 4.0)];
        assert_eq!(phase_difference(&bins, &bins), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn quarter_turns_are_plus_and_minus_half_pi() {
        let out = phase_difference(&[(1.0, 0.0), (1.0, 0.0), (0.0, 1.0)], &[(0.0, 1.0), (0.0, -1.0), (-1.0, 0.0)]);
        approx(out[0], FRAC_PI_2);
        approx(out[1], -FRAC_PI_2);
        approx(out[2], FRAC_PI_2);
    }

    #[test]
    fn opposite_channels_give_plus_pi() {
        let out = phase_difference(&[(1.0, 0.0), (0.5, 0.5), (-1.0, 0.0)], &[(-1.0, 0.0), (-0.5, -0.5), (1.0, 0.0)]);
        approx(out[0], PI);
        approx(out[1], PI);
        approx(out[2], PI);
    }

    #[test]
    fn the_difference_wraps_into_minus_pi_to_pi() {
        let out = phase_difference(&[(-0.984_807_7, 0.173_648_18)], &[(-0.984_807_7, -0.173_648_18)]);
        approx(out[0], 0.349_065_87);
        let out = phase_difference(&[(3.0, 4.0)], &[(4.0, 3.0)]);
        approx(out[0], -0.283_794_1);
    }

    #[test]
    fn silent_or_broken_bins_and_short_slices_are_safe() {
        assert_eq!(
            phase_difference(
                &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
                &[(1.0, 1.0), (f32::NAN, 0.0), (0.0, f32::INFINITY), (f32::INFINITY, 0.0)]
            ),
            vec![0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(phase_difference(&[(1.0, 0.0), (1.0, 0.0), (1.0, 0.0)], &[(0.0, 1.0), (0.0, 1.0)]).len(), 2);
        assert!(phase_difference(&[], &[(1.0, 0.0)]).is_empty());
    }

    #[test]
    fn phase_to_unit_maps_the_circle_onto_zero_to_one() {
        approx(phase_to_unit(-PI), 0.0);
        approx(phase_to_unit(0.0), 0.5);
        approx(phase_to_unit(PI), 1.0);
        approx(phase_to_unit(FRAC_PI_2), 0.75);
        approx(phase_to_unit(-FRAC_PI_2), 0.25);
        approx(phase_to_unit(1.0), 0.659_154_95);
        approx(phase_to_unit(-1.0), 0.340_845_05);
    }

    #[test]
    fn phase_to_unit_clamps_and_survives_nan() {
        approx(phase_to_unit(10.0), 1.0);
        approx(phase_to_unit(-10.0), 0.0);
        approx(phase_to_unit(f32::NAN), 0.5);
    }

    #[test]
    fn phase_row_converts_each_bin() {
        let row = phase_row(&[(1.0, 0.0), (1.0, 0.0), (0.0, 0.0)], &[(1.0, 0.0), (-1.0, 0.0), (5.0, 5.0)]);
        assert_eq!(row.len(), 3);
        approx(row[0], 0.5);
        approx(row[1], 1.0);
        approx(row[2], 0.5);
    }

    #[test]
    fn in_phase_fraction_counts_bins_within_the_tolerance() {
        approx(in_phase_fraction(&[0.0, 0.1, -0.1, 1.0], 0.1), 0.75);
        approx(in_phase_fraction(&[0.0, 0.1, -0.1, 1.0], 4.0), 1.0);
        approx(in_phase_fraction(&[f32::NAN, 0.0], 0.5), 0.5);
        approx(in_phase_fraction(&[0.0, 0.0], 0.0), 1.0);
    }

    #[test]
    fn in_phase_fraction_refuses_bad_input() {
        approx(in_phase_fraction(&[], 1.0), 0.0);
        approx(in_phase_fraction(&[0.0], -1.0), 0.0);
        approx(in_phase_fraction(&[0.0], f32::NAN), 0.0);
    }
}
