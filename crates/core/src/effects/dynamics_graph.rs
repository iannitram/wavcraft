//! The data behind the Dynamics transfer-curve graph (B030): sampled points for the graph to
//! draw. No drawing and no UI here.
//!
//! A point is `(input level in dB, value in dB)`. The output levels come from [`static_curve`]
//! in `effects/dynamics.rs`, so the graph always shows what the effect does.

use super::Effect;
use super::dynamics::{Dynamics, static_curve};

/// The `n` input levels from `lo_db` to `hi_db`, both ends included when `n` is 2 or more.
/// Empty when `n` is 0, when a level is not finite, or when `lo_db >= hi_db`; just `lo_db`
/// when `n` is 1.
fn levels(lo_db: f64, hi_db: f64, n: usize) -> Vec<f64> {
    if n == 0 || !lo_db.is_finite() || !hi_db.is_finite() || lo_db >= hi_db {
        return Vec::new();
    }
    if n == 1 {
        return vec![lo_db];
    }
    let last = (n - 1) as f64;
    (0..n).map(|i| if i + 1 == n { hi_db } else { lo_db + (hi_db - lo_db) * (i as f64) / last }).collect()
}

/// Whether `params` holds one value per Dynamics parameter ([`static_curve`] indexes them).
fn fits(params: &[f64]) -> bool {
    params.len() == Dynamics.parameters().len()
}

/// `n` points of the transfer curve: (input dB, output dB), from `lo_db` to `hi_db`.
/// Empty for a `params` of the wrong length and for the cases `levels` refuses.
pub fn curve_points(params: &[f64], lo_db: f64, hi_db: f64, n: usize) -> Vec<(f64, f64)> {
    if !fits(params) {
        return Vec::new();
    }
    levels(lo_db, hi_db, n).into_iter().map(|x| (x, static_curve(params, x))).collect()
}

/// `n` points of the gain change: (input dB, output dB minus input dB). Zero or negative
/// for a compressor, negative below the gate threshold. Empty in the same cases as
/// [`curve_points`].
pub fn reduction_points(params: &[f64], lo_db: f64, hi_db: f64, n: usize) -> Vec<(f64, f64)> {
    if !fits(params) {
        return Vec::new();
    }
    levels(lo_db, hi_db, n).into_iter().map(|x| (x, static_curve(params, x) - x)).collect()
}

/// The straight line where output equals input, from `lo_db` to `hi_db`.
pub fn unity_line(lo_db: f64, hi_db: f64) -> [(f64, f64); 2] {
    [(lo_db, lo_db), (hi_db, hi_db)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<f64> {
        Dynamics.parameters().iter().map(|d| d.default).collect()
    }

    fn set(params: &mut [f64], key: &str, value: f64) {
        if let Some(i) = Dynamics.parameters().iter().position(|d| d.id == key) {
            params[i] = value;
        }
    }

    fn assert_close(got: &[(f64, f64)], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6, "got {g:?}, wanted {w:?}");
        }
    }

    #[test]
    fn default_params_give_the_unity_line_below_the_thresholds() {
        let pts = curve_points(&defaults(), -50.0, -30.0, 5);
        assert_eq!(pts, vec![(-50.0, -50.0), (-45.0, -45.0), (-40.0, -40.0), (-35.0, -35.0), (-30.0, -30.0)]);
    }

    #[test]
    fn compressor_points_match_the_formula_by_hand() {
        let pts = curve_points(&defaults(), -20.0, 0.0, 3);
        assert_close(&pts, &[(-20.0, -20.5), (-10.0, -16.666667), (0.0, -13.333333)]);
    }

    #[test]
    fn limiter_is_flat_at_the_ceiling() {
        let mut p = defaults();
        set(&mut p, "comp_ratio", 1.0);
        set(&mut p, "makeup_db", 24.0);
        let pts = curve_points(&p, -10.0, 0.0, 3);
        assert_eq!(pts, vec![(-10.0, -1.0), (-5.0, -1.0), (0.0, -1.0)]);
    }

    #[test]
    fn n_of_zero_one_and_two() {
        let p = defaults();
        assert!(curve_points(&p, -80.0, 0.0, 0).is_empty());
        assert_eq!(curve_points(&p, -80.0, 0.0, 1), vec![(-80.0, -100.0)]);
        assert_close(&curve_points(&p, -80.0, 0.0, 2), &[(-80.0, -100.0), (0.0, -13.333333)]);
    }

    #[test]
    fn lo_not_below_hi_gives_no_points() {
        let p = defaults();
        assert!(curve_points(&p, -20.0, -20.0, 3).is_empty());
        assert!(curve_points(&p, 0.0, -60.0, 3).is_empty());
        assert!(curve_points(&p, f64::NAN, 0.0, 3).is_empty());
        assert!(reduction_points(&p, -20.0, -20.0, 3).is_empty());
        assert!(reduction_points(&p, 0.0, f64::INFINITY, 3).is_empty());
    }

    #[test]
    fn reduction_is_at_most_zero_for_a_compressor() {
        let pts = reduction_points(&defaults(), -20.0, 0.0, 3);
        assert_close(&pts, &[(-20.0, -0.5), (-10.0, -6.666667), (0.0, -13.333333)]);
        assert!(pts.iter().all(|p| p.1 <= 0.0));
    }

    #[test]
    fn unity_line_joins_the_two_ends() {
        assert_eq!(unity_line(-60.0, 0.0), [(-60.0, -60.0), (0.0, 0.0)]);
    }

    #[test]
    fn wrong_parameter_count_gives_no_points() {
        assert!(curve_points(&[0.0, 1.0, 2.0], -60.0, 0.0, 3).is_empty());
        assert!(reduction_points(&[], -60.0, 0.0, 3).is_empty());
    }
}
