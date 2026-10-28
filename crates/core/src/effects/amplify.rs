//! Amplify (E2, issue #28): per-channel gain in decibels with linked sliders on by default.
//!
//! Range `-80.0` to `+40.0` dB, default `0.0` dB, `Link Sliders` (`link`) on by default.

use std::io;

use super::{Effect, EffectStream, ParamDesc, Run, RunContext};

/// Lowest gain setting in decibels.
pub const MIN_GAIN_DB: f64 = -80.0;

/// Highest gain setting in decibels.
pub const MAX_GAIN_DB: f64 = 40.0;

/// Default gain setting in decibels.
pub const DEFAULT_GAIN_DB: f64 = 0.0;

/// Convert a decibel gain into a linear amplitude multiplier: `10^(db / 20)`.
pub fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Linear amplitude multipliers for the active document `channels` (in slice order).
///
/// When `link` is `true`, every active channel uses the same decibel gain: `left_db` if
/// `left_db != DEFAULT_GAIN_DB`, otherwise `right_db`. When `link` is `false`, document channel
/// `0` uses `left_db` and any document channel with index `>= 1` uses `right_db`.
pub fn amplify_gains(left_db: f64, right_db: f64, link: bool, channels: &[usize]) -> Vec<f64> {
    channels
        .iter()
        .map(|&ch| {
            let db = if link {
                if left_db != DEFAULT_GAIN_DB { left_db } else { right_db }
            } else if ch == 0 {
                left_db
            } else {
                right_db
            };
            db_to_linear(db)
        })
        .collect()
}

/// The settings as `(left_db, right_db, link)`, read from `params` in parameter order. A missing
/// value takes its default, so a short list cannot panic.
fn settings(params: &[f64]) -> (f64, f64, bool) {
    let at = |i: usize, default: f64| params.get(i).copied().unwrap_or(default);
    (at(0, DEFAULT_GAIN_DB), at(1, DEFAULT_GAIN_DB), at(2, 1.0) >= 0.5)
}

/// Scale each channel by its gain, in f64, writing f32. No clamping: the document keeps headroom
/// above 0 dBFS until save.
fn scale(gains: &[f64], input: &[&[f32]], output: &mut [&mut [f32]]) {
    for ((i, o), g) in input.iter().zip(output.iter_mut()).zip(gains) {
        for (a, b) in i.iter().zip(o.iter_mut()) {
            *b = (f64::from(*a) * g) as f32;
        }
    }
}

/// The linear gains for a run or a stream, fixed when it starts (so the audio path allocates
/// nothing).
struct Gains(Vec<f64>);

impl Run for Gains {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        scale(&self.0, input, output);
        Ok(())
    }
}

impl EffectStream for Gains {
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        scale(&self.0, input, output);
    }
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
        const P: &[ParamDesc] = &[
            ParamDesc::db("left_db", "Left", MIN_GAIN_DB, MAX_GAIN_DB, DEFAULT_GAIN_DB),
            ParamDesc::db("right_db", "Right", MIN_GAIN_DB, MAX_GAIN_DB, DEFAULT_GAIN_DB),
            ParamDesc::choice("link", "Link Sliders", &["off", "on"], 1),
        ];
        P
    }

    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let (left, right, link) = settings(params);
        Ok(Some(Box::new(Gains(amplify_gains(left, right, link, &ctx.channels)))))
    }

    fn process(&self, params: &[f64], _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let (left, right, link) = settings(params);
        let channels: Vec<usize> = (0..input.len()).collect();
        scale(&amplify_gains(left, right, link, &channels), input, output);
        Ok(())
    }

    fn stream(&self, params: &[f64], channels: usize, _rate: u32) -> Option<Box<dyn EffectStream>> {
        let (left, right, link) = settings(params);
        let channels: Vec<usize> = (0..channels).collect();
        Some(Box::new(Gains(amplify_gains(left, right, link, &channels))))
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
