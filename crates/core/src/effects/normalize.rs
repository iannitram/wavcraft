//! Normalize (E3, #29): scale a range so its sample peak lands on a target, given in % of full
//! scale or in dB (default 100 %). "Normalize All Channels Equally" (on by default) uses one gain
//! for every processed channel, set by the loudest; off, each channel gets its own. "DC Bias
//! Adjust" (off by default) removes each channel's mean over the range before the peak is
//! measured and the gain applied. As in Audition 26.5.0.82 (checked 2026-10-15).
//!
//! [`target_peak`] and [`normalize_gains`] are the pure arithmetic; [`Normalize`] measures the
//! range in one streamed pass in [`Effect::begin`] and applies the gains chunk by chunk.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

/// The linear peak a target means: `value` percent of full scale (`in_db` false; 100 is 1.0),
/// or `value` dBFS (`in_db` true; 0 is 1.0, -6 is about 0.501).
pub fn target_peak(value: f64, in_db: bool) -> f64 {
    if !in_db { value / 100.0 } else { 10f64.powf(value / 20.0) }
}

/// One linear gain per channel that brings the measured `peaks` (linear, one per processed
/// channel, each >= 0) to `target`. `equally`: every channel gets `target / (largest peak)`;
/// otherwise channel `c` gets `target / peaks[c]`. A peak of 0 (a silent channel) gets gain 1.0,
/// and so does every channel when `equally` and all peaks are 0. Same length as `peaks`.
pub fn normalize_gains(peaks: &[f64], target: f64, equally: bool) -> Vec<f64> {
    if equally {
        let loudest = peaks.iter().fold(0.0_f64, |acc, &peak| acc.max(peak));
        peaks.iter().map(|&_peak| if loudest > 0.0 { target / loudest } else { 1.0 }).collect()
    } else {
        peaks.iter().map(|&peak| if peak > 0.0 { target / peak } else { 1.0 }).collect()
    }
}

/// The Normalize effect. Settings, in this order: `percent` (target in %, used when `unit` is
/// `percent`), `db` (target in dBFS, used when `unit` is `dB`), `unit`, `equally` (Normalize
/// All Channels Equally), `dc_bias` (DC Bias Adjust).
pub struct Normalize;

impl Effect for Normalize {
    fn id(&self) -> &'static str {
        "normalize"
    }
    fn name(&self) -> &'static str {
        "Normalize"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::percent("percent", "Normalize to (%)", 0.1, 100.0, 100.0),
            ParamDesc::db("db", "Normalize to (dB)", -60.0, 0.0, 0.0),
            ParamDesc::choice("unit", "Units", &["percent", "dB"], 0),
            ParamDesc::choice("equally", "Normalize All Channels Equally", &["off", "on"], 1),
            ParamDesc::choice("dc_bias", "DC Bias Adjust", &["off", "on"], 0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let _ = (params, ctx);
        Err(io::Error::new(io::ErrorKind::Unsupported, "Normalize is not built yet"))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Normalize measures the range first: run it through effects::apply"))
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_gains, target_peak};

    fn close(got: f64, want: f64) -> bool {
        (got - want).abs() <= 1e-12
    }

    fn all_close(got: &[f64], want: &[f64]) {
        assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");
        assert!(got.iter().zip(want).all(|(g, w)| close(*g, *w)), "got {got:?}, want {want:?}");
    }

    #[test]
    fn target_peak_percent() {
        assert!(close(target_peak(100.0, false), 1.0));
        assert!(close(target_peak(50.0, false), 0.5));
        assert!(close(target_peak(0.1, false), 0.001));
    }

    #[test]
    fn target_peak_db() {
        assert!(close(target_peak(0.0, true), 1.0));
        assert!(close(target_peak(-6.0, true), 0.501_187_233_627_272_2));
        assert!(close(target_peak(-20.0, true), 0.1));
        assert!(close(target_peak(-60.0, true), 0.001));
    }

    #[test]
    fn gains_equally_use_the_loudest_channel() {
        all_close(&normalize_gains(&[0.5, 0.25], 1.0, true), &[2.0, 2.0]);
        all_close(&normalize_gains(&[0.25, 0.5, 0.125], 0.5, true), &[1.0, 1.0, 1.0]);
    }

    #[test]
    fn gains_separately_per_channel() {
        all_close(&normalize_gains(&[0.5, 0.25], 1.0, false), &[2.0, 4.0]);
        all_close(&normalize_gains(&[0.8], 0.4, false), &[0.5]);
        all_close(&normalize_gains(&[2.0], 1.0, false), &[0.5]);
    }

    #[test]
    fn silent_channels_keep_gain_one() {
        all_close(&normalize_gains(&[0.0, 0.5], 1.0, false), &[1.0, 2.0]);
        all_close(&normalize_gains(&[0.0, 0.0], 1.0, true), &[1.0, 1.0]);
        all_close(&normalize_gains(&[0.0, 0.25], 1.0, true), &[4.0, 4.0]);
        all_close(&normalize_gains(&[], 1.0, true), &[]);
    }
}
