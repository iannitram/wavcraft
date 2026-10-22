//! Piecewise-linear fade envelope implementation.
//!
//! Sanitizes inputs according to these rules:
//! 1. Drop every point whose time is not finite (NaN or infinity).
//! 2. Replace every non-finite gain (NaN or infinity) with 1.0. Keep the point.
//! 3. Keep the order of the remaining points. Drop a point whose time is not strictly greater than
//!    the time of the last kept point.
//! 4. If no points remain, return an empty Vec.
//! 5. Otherwise run the existing algorithm unchanged, on the kept points only.

pub fn piecewise_linear(points: &[(f32, f32)], times: &[f32]) -> Vec<f32> {
    if points.is_empty() {
        return Vec::new();
    }

    // Sanitize input points according to the rules
    let mut sanitized_points = Vec::with_capacity(points.len());
    let mut last_time = f32::NEG_INFINITY;

    for &(time, gain) in points {
        // Rule 1: Drop points with non-finite time
        if !time.is_finite() {
            continue;
        }

        // Rule 2: Replace non-finite gains with 1.0
        let sanitized_gain = if gain.is_finite() { gain } else { 1.0 };

        // Rule 3: Drop points whose time is not strictly greater than the last kept point
        if time <= last_time {
            continue;
        }

        sanitized_points.push((time, sanitized_gain));
        last_time = time;
    }

    // Rule 4: Return empty Vec if no points remain after sanitization
    if sanitized_points.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(times.len());

    for &t in times {
        if t <= sanitized_points[0].0 {
            // Clamp low
            result.push(sanitized_points[0].1);
        } else if t >= sanitized_points[sanitized_points.len() - 1].0 {
            // Clamp high
            result.push(sanitized_points[sanitized_points.len() - 1].1);
        } else {
            // Find the segment and interpolate
            let mut found = false;
            for i in 0..sanitized_points.len() - 1 {
                let (t0, g0) = sanitized_points[i];
                let (t1, g1) = sanitized_points[i + 1];

                if t0 <= t && t <= t1 {
                    // Handle the case where t1 == t0 (avoid division by zero)
                    if t1 == t0 {
                        result.push(g1);
                    } else {
                        let gain = g0 + (t - t0) / (t1 - t0) * (g1 - g0);
                        result.push(gain);
                    }
                    found = true;
                    break;
                }
            }

            // This shouldn't happen due to the earlier clamping, but just in case
            if !found {
                result.push(sanitized_points[sanitized_points.len() - 1].1);
            }
        }
    }

    result
}

/// Interpolation mode for [`gain_envelope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interp {
    Linear,
    Spline,
    /// Monotone cubic Hermite (Fritsch-Butland PCHIP tangents): passes through
    /// every point and never overshoots the values of neighbouring knots.
    Monotone,
}

/// Reasons [`gain_envelope`] rejects its keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GainError {
    TooFewPoints,
    NonFinite,
    NotIncreasing,
}

/// Evaluates a time-varying gain curve through `points` (time, gain) at `times`.
///
/// Checks run in order: at least two points, all times and gains finite, and
/// strictly increasing times; the first failure is returned as a [`GainError`].
/// Times before the first point or after the last point clamp to that point's
/// gain in both modes. `Interp::Linear` reuses [`piecewise_linear`];
/// `Interp::Spline` is a cubic Hermite curve with Catmull-Rom tangents, so it
/// passes through every point and equals the linear result for two points.
pub fn gain_envelope(points: &[(f32, f32)], times: &[f32], interp: Interp) -> Result<Vec<f32>, GainError> {
    if points.len() < 2 {
        return Err(GainError::TooFewPoints);
    }
    if points.iter().any(|&(t, g)| !t.is_finite() || !g.is_finite()) {
        return Err(GainError::NonFinite);
    }
    if points.windows(2).any(|w| w[1].0.partial_cmp(&w[0].0) != Some(std::cmp::Ordering::Greater)) {
        return Err(GainError::NotIncreasing);
    }

    match interp {
        Interp::Linear => Ok(piecewise_linear(points, times)),
        Interp::Spline => Ok(spline_eval(points, times)),
        Interp::Monotone => Ok(monotone_eval(points, times)),
    }
}

/// Monotone PCHIP evaluation. Expects validated points.
fn monotone_eval(points: &[(f32, f32)], times: &[f32]) -> Vec<f32> {
    let n = points.len();
    let ts = |k: usize| f64::from(points[k].0);
    let gs = |k: usize| f64::from(points[k].1);

    // Compute segment widths and slopes
    let mut h = vec![0.0_f64; n - 1];
    let mut d = vec![0.0_f64; n - 1];
    for k in 0..n - 1 {
        h[k] = ts(k + 1) - ts(k);
        d[k] = (gs(k + 1) - gs(k)) / h[k];
    }

    // Compute tangents
    let mut tangents = vec![0.0_f64; n];
    tangents[0] = d[0];
    tangents[n - 1] = d[n - 2];

    for k in 1..n - 1 {
        if d[k - 1] * d[k] <= 0.0 {
            // Flat or extremum knot
            tangents[k] = 0.0;
        } else {
            // Weighted harmonic mean of slopes
            let w1 = 2.0 * h[k] + h[k - 1];
            let w2 = h[k] + 2.0 * h[k - 1];
            tangents[k] = (w1 + w2) / (w1 / d[k - 1] + w2 / d[k]);
        }
    }

    let first = points[0];
    let last = points[n - 1];
    let mut result = Vec::with_capacity(times.len());
    for &t in times {
        if t <= first.0 {
            result.push(first.1);
        } else if t >= last.0 {
            result.push(last.1);
        } else {
            // Find the segment
            let k = points.partition_point(|p| p.0 <= t).saturating_sub(1).min(n - 2);

            let s = (f64::from(t) - ts(k)) / h[k];
            let s2 = s * s;
            let s3 = s2 * s;

            // Hermite basis functions
            let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
            let h10 = s3 - 2.0 * s2 + s;
            let h01 = -2.0 * s3 + 3.0 * s2;
            let h11 = s3 - s2;

            // Evaluate
            let value = h00 * gs(k) + h10 * h[k] * tangents[k] + h01 * gs(k + 1) + h11 * h[k] * tangents[k + 1];
            result.push(value as f32);
        }
    }
    result
}

/// Cubic Hermite evaluation with Catmull-Rom tangents. Expects validated points.
fn spline_eval(points: &[(f32, f32)], times: &[f32]) -> Vec<f32> {
    let n = points.len();
    let ts = |k: usize| f64::from(points[k].0);
    let gs = |k: usize| f64::from(points[k].1);

    let mut tangents = vec![0.0_f64; n];
    tangents[0] = (gs(1) - gs(0)) / (ts(1) - ts(0));
    tangents[n - 1] = (gs(n - 1) - gs(n - 2)) / (ts(n - 1) - ts(n - 2));
    for (k, m) in tangents.iter_mut().enumerate().take(n - 1).skip(1) {
        *m = (gs(k + 1) - gs(k - 1)) / (ts(k + 1) - ts(k - 1));
    }

    let first = points[0];
    let last = points[n - 1];
    let mut result = Vec::with_capacity(times.len());
    for &t in times {
        if t <= first.0 {
            result.push(first.1);
        } else if t >= last.0 {
            result.push(last.1);
        } else {
            let k = points.partition_point(|p| p.0 <= t).saturating_sub(1).min(n - 2);
            let h = ts(k + 1) - ts(k);
            let s = (f64::from(t) - ts(k)) / h;
            let s2 = s * s;
            let s3 = s2 * s;
            let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
            let h10 = s3 - 2.0 * s2 + s;
            let h01 = -2.0 * s3 + 3.0 * s2;
            let h11 = s3 - s2;
            let value = h00 * gs(k) + h10 * h * tangents[k] + h01 * gs(k + 1) + h11 * h * tangents[k + 1];
            result.push(value as f32);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piecewise_linear_ramp_is_exact() {
        let points = [(0.0, 0.0), (1.0, 1.0)];
        let times = [0.0, 0.25, 0.5, 0.75, 1.0];
        let expected = [0.0, 0.25, 0.5, 0.75, 1.0];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result, expected);
    }

    #[test]
    fn piecewise_linear_multi_segment_shape() {
        let points = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.2)];
        let times = [-0.1, 0.25, 0.5, 0.75, 1.0, 1.5];
        let expected = [0.0, 0.5, 1.0, 0.6, 0.2, 0.2];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result.len(), expected.len());
        for (a, b) in result.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn piecewise_linear_mid_ramp_scaled() {
        let points = [(0.0, 0.0), (2.0, 0.5)];
        let times = [0.5, 1.0, 3.0];
        let expected = [0.125, 0.25, 0.5];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result, expected);
    }

    #[test]
    fn piecewise_linear_empty_points_returns_empty() {
        let points: [(f32, f32); 0] = [];
        let times = [0.0, 0.5];
        let result = piecewise_linear(&points, &times);
        assert!(result.is_empty());
    }

    #[test]
    fn non_finite_gain_becomes_unity() {
        let points = [(0.0, 0.0), (0.5, f32::NAN), (1.0, 1.0)];
        let times = [0.25, 0.5, 0.75];
        let expected = [0.5, 1.0, 1.0];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    #[test]
    fn infinite_gain_becomes_unity() {
        let points = [(0.0, f32::INFINITY), (1.0, 0.5)];
        let times = [0.0, 0.5, 1.0];
        let expected = [1.0, 0.75, 0.5];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    #[test]
    fn non_increasing_time_point_is_dropped() {
        let points = [(0.0, 0.0), (1.0, 1.0), (0.5, 0.25), (2.0, 0.0)];
        let times = [0.25, 1.25, 2.5];
        let expected = [0.25, 0.75, 0.0];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    #[test]
    fn equal_time_point_is_dropped() {
        let points = [(0.0, 0.0), (1.0, 1.0), (1.0, 0.5), (2.0, 0.0)];
        let times = [0.5, 1.5];
        let expected = [0.5, 0.5];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    #[test]
    fn non_finite_time_point_is_dropped() {
        let points = [(f32::NAN, 0.3), (0.0, 0.0), (1.0, 1.0)];
        let times = [-1.0, 0.5];
        let expected = [0.0, 0.5];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    #[test]
    fn all_points_dropped_gives_empty() {
        let points = [(f32::NAN, 0.3)];
        let times = [0.5];
        let expected: [f32; 0] = [];
        assert_eq!(piecewise_linear(&points, &times), expected);
    }

    fn assert_close(got: Result<Vec<f32>, GainError>, want: &[f32]) {
        let got = match got {
            Ok(values) => values,
            Err(e) => panic!("unexpected error: {e:?}"),
        };
        assert_eq!(got.len(), want.len());
        for (a, b) in got.iter().zip(want.iter()) {
            assert!((a - b).abs() < 1e-5, "got {a}, want {b}");
        }
    }

    #[test]
    fn linear_interpolates_and_clamps() {
        let points = [(0.0, 0.0), (1.0, 1.0)];
        let times = [-1.0, 0.0, 0.25, 1.0, 2.0];
        assert_close(gain_envelope(&points, &times, Interp::Linear), &[0.0, 0.0, 0.25, 1.0, 1.0]);
    }

    #[test]
    fn linear_multi_segment_values() {
        let points = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.2)];
        assert_close(gain_envelope(&points, &[0.25, 0.75], Interp::Linear), &[0.5, 0.6]);
    }

    #[test]
    fn spline_passes_through_every_knot() {
        let points = [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        assert_close(gain_envelope(&points, &[0.0, 1.0, 2.0], Interp::Spline), &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn spline_midpoints_match_hermite() {
        let points = [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        assert_close(gain_envelope(&points, &[0.5, 1.25], Interp::Spline), &[0.625, 0.890625]);
    }

    #[test]
    fn spline_reproduces_straight_line_data() {
        let points = [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0)];
        assert_close(gain_envelope(&points, &[0.3, 1.7], Interp::Spline), &[0.15, 0.85]);
    }

    #[test]
    fn non_finite_points_are_an_error() {
        assert_eq!(gain_envelope(&[(0.0, 0.0), (f32::NAN, 1.0)], &[0.5], Interp::Linear), Err(GainError::NonFinite));
        assert_eq!(gain_envelope(&[(0.0, 0.0), (1.0, f32::INFINITY)], &[0.5], Interp::Spline), Err(GainError::NonFinite));
    }

    #[test]
    fn times_must_strictly_increase() {
        assert_eq!(gain_envelope(&[(0.0, 0.0), (0.0, 1.0)], &[0.5], Interp::Linear), Err(GainError::NotIncreasing));
        assert_eq!(gain_envelope(&[(0.0, 0.0), (1.0, 1.0), (0.5, 0.0)], &[0.5], Interp::Spline), Err(GainError::NotIncreasing));
    }

    #[test]
    fn fewer_than_two_points_is_an_error() {
        assert_eq!(gain_envelope(&[(0.0, 0.0)], &[0.5], Interp::Linear), Err(GainError::TooFewPoints));
    }

    #[test]
    fn monotone_passes_through_every_knot() {
        let points = [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        assert_close(gain_envelope(&points, &[0.0, 1.0, 2.0], Interp::Monotone), &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn monotone_flat_segments_stay_flat() {
        let points = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)];
        assert_close(gain_envelope(&points, &[0.5, 1.5], Interp::Monotone), &[0.0, 0.0]);
    }

    #[test]
    fn monotone_step_has_no_overshoot() {
        let points = [(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (3.0, 1.0)];
        assert_close(gain_envelope(&points, &[0.5, 1.5, 2.5], Interp::Monotone), &[0.0, 0.5, 1.0]);
    }

    #[test]
    fn monotone_unequal_spacing_uses_weighted_tangent() {
        let points = [(0.0, 0.0), (1.0, 1.0), (3.0, 1.5), (4.0, 1.5)];
        assert_close(gain_envelope(&points, &[0.5, 2.0, 3.5], Interp::Monotone), &[0.5714286, 1.3571428, 1.5]);
    }

    #[test]
    fn monotone_zeroes_tangent_at_extremum() {
        let points = [(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        assert_close(gain_envelope(&points, &[0.25, 1.75], Interp::Monotone), &[0.296875, 0.296875]);
    }

    #[test]
    fn monotone_never_overshoots_neighbouring_knots() {
        let sequences: [&[(f32, f32)]; 4] = [
            &[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (3.0, 1.0)],
            &[(0.0, 1.0), (1.0, 0.0), (2.0, 1.0), (3.0, 0.5), (4.0, 0.5)],
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)],
            &[(0.0, 0.2), (1.0, 0.2), (2.0, 0.9), (3.0, 0.2)],
        ];
        for points in sequences {
            for pair in points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let lo = a.1.min(b.1);
                let hi = a.1.max(b.1);
                let times: Vec<f32> = (0..=100).map(|j| (f64::from(a.0) + (f64::from(b.0) - f64::from(a.0)) * f64::from(j) / 100.0) as f32).collect();
                let Ok(values) = gain_envelope(points, &times, Interp::Monotone) else {
                    panic!("valid points rejected");
                };
                for v in values {
                    assert!(v >= lo - 1e-6 && v <= hi + 1e-6, "value {v} outside [{lo}, {hi}]");
                }
            }
        }
    }

    #[test]
    fn monotone_rejects_bad_points_like_linear() {
        assert_eq!(gain_envelope(&[(0.0, 0.0), (f32::NAN, 1.0)], &[0.5], Interp::Monotone), Err(GainError::NonFinite));
        assert_eq!(gain_envelope(&[(0.0, 0.0), (1.0, f32::INFINITY)], &[0.5], Interp::Monotone), Err(GainError::NonFinite));
        assert_eq!(gain_envelope(&[(0.0, 0.0), (0.0, 1.0)], &[0.5], Interp::Monotone), Err(GainError::NotIncreasing));
        assert_eq!(gain_envelope(&[(0.0, 0.0)], &[0.5], Interp::Monotone), Err(GainError::TooFewPoints));
    }
}
