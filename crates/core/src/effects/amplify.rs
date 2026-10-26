//! Amplify (E2, issue #28): per-channel gain in decibels with linked sliders on by default.
//!
//! Range `-80.0` to `+40.0` dB, default `0.0` dB, `Link Sliders` (`link`) on by default.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

/// Lowest gain setting in decibels.
pub const MIN_GAIN_DB: f64 = -80.0;

/// Highest gain setting in decibels.
pub const MAX_GAIN_DB: f64 = 40.0;

/// Default gain setting in decibels.
pub const DEFAULT_GAIN_DB: f64 = 0.0;

/// Convert a decibel gain into a linear amplitude multiplier: `10^(db / 20)`.
pub fn db_to_linear(db: f64) -> f64 {
    let _ = db;
    0.0
}

/// Linear amplitude multipliers for the active document `channels` (in slice order).
///
/// When `link` is `true`, every active channel uses the same decibel gain: `left_db` if
/// `left_db != DEFAULT_GAIN_DB`, otherwise `right_db`. When `link` is `false`, document channel
/// `0` uses `left_db` and any document channel with index `>= 1` uses `right_db`.
pub fn amplify_gains(left_db: f64, right_db: f64, link: bool, channels: &[usize]) -> Vec<f64> {
    let _ = (left_db, right_db, link, channels);
    Vec::new()
}

/// Amplify effect (`effect.amplify`).
pub struct Amplify;

impl Effect for Amplify {
    fn id(&self) -> &'static str {
        "amplify"
    }

    fn name(&self) -> &'static str {
        "Amplify"
    }

    fn parameters(&self) -> &'static [ParamDesc] {
        &[]
    }

    fn begin(&self, _params: &[f64], _ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        Ok(None)
    }

    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Amplify is not built yet"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_to_linear_at_zero_and_bounds() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-12);
        assert!((db_to_linear(20.0) - 10.0).abs() < 1e-12);
        assert!((db_to_linear(MAX_GAIN_DB) - 100.0).abs() < 1e-10);
        assert!((db_to_linear(-20.0) - 0.1).abs() < 1e-12);
        assert!((db_to_linear(-40.0) - 0.01).abs() < 1e-12);
        assert!((db_to_linear(MIN_GAIN_DB) - 0.0001).abs() < 1e-15);
    }

    #[test]
    fn db_to_linear_matches_formula_within_thousandth_db() {
        for (db, expected) in [(-6.0, 0.5011872336272722), (6.0, 1.9952623149688795), (-12.0, 0.251188643150958), (12.0, 3.9810717055349722)] {
            let got = db_to_linear(db);
            assert!((got - expected).abs() < 1e-12, "db={db}: got {got}, expected {expected}");
            let round_trip_db = 20.0 * got.log10();
            assert!((round_trip_db - db).abs() <= 0.001, "db={db}: round_trip_db={round_trip_db}");
        }
    }

    #[test]
    fn gains_linked_apply_shared_db_to_every_channel() {
        let g = amplify_gains(6.0, 0.0, true, &[0, 1]);
        assert_eq!(g.len(), 2);
        assert!((g[0] - 1.9952623149688795).abs() < 1e-12);
        assert!((g[1] - 1.9952623149688795).abs() < 1e-12);

        let g_right = amplify_gains(0.0, -20.0, true, &[0, 1]);
        assert_eq!(g_right.len(), 2);
        assert!((g_right[0] - 0.1).abs() < 1e-12);
        assert!((g_right[1] - 0.1).abs() < 1e-12);

        let g_both_nonzero = amplify_gains(-20.0, 20.0, true, &[0, 1]);
        assert_eq!(g_both_nonzero.len(), 2);
        assert!((g_both_nonzero[0] - 0.1).abs() < 1e-12);
        assert!((g_both_nonzero[1] - 0.1).abs() < 1e-12);

        let g_single = amplify_gains(20.0, 0.0, true, &[1]);
        assert_eq!(g_single.len(), 1);
        assert!((g_single[0] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn gains_unlinked_use_left_for_channel_zero_and_right_for_others() {
        let g = amplify_gains(-20.0, 20.0, false, &[0, 1]);
        assert_eq!(g.len(), 2);
        assert!((g[0] - 0.1).abs() < 1e-12);
        assert!((g[1] - 10.0).abs() < 1e-12);

        let g_left_only = amplify_gains(-20.0, 20.0, false, &[0]);
        assert_eq!(g_left_only.len(), 1);
        assert!((g_left_only[0] - 0.1).abs() < 1e-12);

        let g_right_only = amplify_gains(-20.0, 20.0, false, &[1]);
        assert_eq!(g_right_only.len(), 1);
        assert!((g_right_only[0] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn gains_for_empty_channel_list() {
        assert!(amplify_gains(6.0, -6.0, true, &[]).is_empty());
        assert!(amplify_gains(6.0, -6.0, false, &[]).is_empty());
    }
}
