//! Match Loudness (v1 G3, issue #38): set a file's integrated loudness (ITU-R BS.1770-3 or
//! EBU R128) or its true peak to a target, with optional true-peak limiting.
//!
//! The settings ([`PARAMS`]) and presets ([`PRESETS`]) are data. [`effective_target`],
//! [`match_gain_db`] and [`within_tolerance`] are the arithmetic (card wav-c-38-a); [`measure`]
//! and the effect itself are card wav-c-38-c. The bodies below marked STUB return placeholder
//! values until those cards fill them.

use std::io;
use std::ops::Range;

use super::ParamDesc;
use crate::doc::DocState;

/// The Match To choices in v1, in this order (the `match_to` setting is the index).
pub const MATCH_TO: &[&str] = &["ITU-R BS.1770-3 Loudness", "EBU R128 Loudness", "True Peak Amplitude"];

/// Index of [`MATCH_TO`]: ITU-R BS.1770-3 integrated loudness, to the Target Loudness.
pub const MATCH_BS1770: f64 = 0.0;
/// Index of [`MATCH_TO`]: EBU R128 integrated loudness; the target is always -23 LUFS.
pub const MATCH_EBU_R128: f64 = 1.0;
/// Index of [`MATCH_TO`]: the true peak, to the Target (read as dBTP).
pub const MATCH_TRUE_PEAK: f64 = 2.0;

/// The fixed EBU R128 target, LUFS.
pub const EBU_R128_TARGET: f64 = -23.0;

/// The settings, in order: `match_to` (0), `target` (1), `tolerance` (2), `max_true_peak` (3),
/// `limiting` (4: 0 off, 1 on), `look_ahead` (5, ms), `release` (6, ms). The defaults are the
/// owner's preset, -16 LUFS / -1 dBTP.
pub const PARAMS: &[ParamDesc] = &[
    ParamDesc::choice("match_to", "Match To", MATCH_TO, 0),
    ParamDesc::number("target", "Target Loudness", -60.0, 0.0, -16.0, "LUFS"),
    ParamDesc::number("tolerance", "Tolerance", 0.0, 10.0, 0.5, "LU"),
    ParamDesc::db("max_true_peak", "Max True Peak Level", -20.0, 0.0, -1.0),
    ParamDesc::choice("limiting", "Use True Peak Limiting", &["Off", "On"], 1),
    ParamDesc::time("look_ahead", "Look-Ahead Time", 1.0, 100.0, 12.0, "ms"),
    ParamDesc::time("release", "Release Time", 1.0, 1000.0, 200.0, "ms"),
];

/// A named set of settings, in [`PARAMS`] order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub params: [f64; 7],
}

/// The presets, the default first.
pub const PRESETS: &[Preset] = &[
    Preset { name: "Podcast: -16 LUFS / -1 dBTP", params: [0.0, -16.0, 0.5, -1.0, 1.0, 12.0, 200.0] },
    Preset { name: "ITU-R BS.1770-3: -23 LUFS / -1 dBTP", params: [0.0, -23.0, 0.5, -1.0, 1.0, 12.0, 200.0] },
];

/// What [`measure`] reports for a range: shown before and after a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessReport {
    /// Gated integrated loudness, LUFS (-144 for silence).
    pub integrated_lufs: f64,
    /// Highest true peak (4x oversampled), dBTP (-144 for silence).
    pub true_peak_db: f64,
}

/// The target the settings aim at: -23 for EBU R128 whatever `params[1]` says, otherwise
/// `params[1]` (LUFS, or dBTP for True Peak Amplitude).
pub fn effective_target(_params: &[f64]) -> f64 {
    let params = _params;
    let match_to = params.first().copied().unwrap_or(0.0);
    if match_to == MATCH_EBU_R128 { EBU_R128_TARGET } else { params.get(1).copied().unwrap_or(-16.0) }
}

/// The gain in dB that moves the measured value to the target. See card wav-c-38-a.
pub fn match_gain_db(_params: &[f64], integrated_lufs: f64, true_peak_db: f64) -> f64 {
    let params = _params;
    let match_to = params.first().copied().unwrap_or(0.0);

    // 1. `measured` = `true_peak_db` when `match_to == MATCH_TRUE_PEAK`, else `integrated_lufs`.
    let measured = if match_to == MATCH_TRUE_PEAK { true_peak_db } else { integrated_lufs };

    // 2. If `measured` is not finite, or `measured < -70.0`, return `0.0` (silence or nothing to measure).
    // `-70.0` itself is measurable.
    if !measured.is_finite() || measured < -70.0 {
        return 0.0;
    }

    // 3. `gain = effective_target(params) - measured`.
    let gain = effective_target(params) - measured;

    // 4. If `limiting < 0.5` (off) and `true_peak_db` is finite:
    // `gain = gain.min(max_true_peak - true_peak_db)` (without the limiter the gain may not push the peak over Max True Peak Level).
    let limiting = params.get(4).copied().unwrap_or(1.0);
    if limiting < 0.5 && true_peak_db.is_finite() {
        let max_true_peak = params.get(3).copied().unwrap_or(-1.0);
        return gain.min(max_true_peak - true_peak_db);
    }

    // 5. Return `gain`.
    gain
}

/// Whether a result is within the Tolerance of the target. See card wav-c-38-a.
pub fn within_tolerance(_params: &[f64], integrated_lufs: f64, true_peak_db: f64) -> bool {
    let params = _params;
    let match_to = params.first().copied().unwrap_or(0.0);

    // 1. `measured` = `true_peak_db` when `match_to == MATCH_TRUE_PEAK`, else `integrated_lufs`.
    let measured = if match_to == MATCH_TRUE_PEAK { true_peak_db } else { integrated_lufs };

    // 2. Return `false` if it is not finite.
    if !measured.is_finite() {
        return false;
    }

    // 3. `(measured - effective_target(params)).abs() <= tolerance`.
    let tolerance = params.get(2).copied().unwrap_or(0.5);
    (measured - effective_target(params)).abs() <= tolerance
}

/// Integrated loudness and true peak of `range` of `doc` over `channels` (document channel
/// indices), measured with `soundcraft_dsp::meter::LoudnessMeter`, read in 64 K-frame chunks.
/// An empty range means the whole file. See card wav-c-38-c.
pub fn measure(_doc: &DocState, _range: Range<u64>, _channels: &[usize]) -> io::Result<LoudnessReport> {
    // STUB: card wav-c-38-c fills this.
    Ok(LoudnessReport { integrated_lufs: f64::NAN, true_peak_db: f64::NAN })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's preset: BS.1770-3, -16 LUFS, 0.5 LU, -1 dBTP, limiting on, 12 ms, 200 ms.
    const OWNER: [f64; 7] = [0.0, -16.0, 0.5, -1.0, 1.0, 12.0, 200.0];
    /// Audition's default: as OWNER but -23 LUFS.
    const AUDITION: [f64; 7] = [0.0, -23.0, 0.5, -1.0, 1.0, 12.0, 200.0];
    /// EBU R128 chosen, target field left at -16 (ignored: R128 is always -23).
    const EBU: [f64; 7] = [1.0, -16.0, 0.5, -1.0, 1.0, 12.0, 200.0];
    /// True Peak Amplitude to -3 dBTP.
    const PEAK: [f64; 7] = [2.0, -3.0, 0.5, -1.0, 1.0, 12.0, 200.0];
    /// The owner's preset with limiting off.
    const NO_LIMIT: [f64; 7] = [0.0, -16.0, 0.5, -1.0, 0.0, 12.0, 200.0];

    fn near(got: f64, want: f64) {
        assert!((got - want).abs() <= 1e-9, "got {got}, want {want}");
    }

    #[test]
    fn presets_are_the_owner_default_and_the_audition_default() {
        assert_eq!(PRESETS.len(), 2);
        assert_eq!(PRESETS[0].params, OWNER);
        assert_eq!(PRESETS[1].params, AUDITION);
        let defaults: Vec<f64> = PARAMS.iter().map(|p| p.default).collect();
        assert_eq!(defaults, OWNER.to_vec());
    }

    #[test]
    fn effective_target_is_minus_23_for_ebu_r128() {
        near(effective_target(&OWNER), -16.0);
        near(effective_target(&AUDITION), -23.0);
        near(effective_target(&EBU), -23.0);
        near(effective_target(&PEAK), -3.0);
    }

    #[test]
    fn gain_raises_a_quiet_file_to_the_target() {
        near(match_gain_db(&OWNER, -20.0, -6.0), 4.0);
        near(match_gain_db(&AUDITION, -30.0, -12.0), 7.0);
    }

    #[test]
    fn gain_lowers_a_loud_file_to_the_target() {
        near(match_gain_db(&OWNER, -12.5, -0.5), -3.5);
    }

    #[test]
    fn ebu_r128_matches_to_minus_23() {
        near(match_gain_db(&EBU, -20.0, -9.0), -3.0);
    }

    #[test]
    fn true_peak_mode_matches_the_peak_not_the_loudness() {
        near(match_gain_db(&PEAK, -30.0, -9.0), 6.0);
        near(match_gain_db(&PEAK, -10.0, -1.0), -2.0);
    }

    #[test]
    fn limiting_off_caps_the_gain_at_max_true_peak() {
        near(match_gain_db(&NO_LIMIT, -20.0, -3.0), 2.0);
        near(match_gain_db(&NO_LIMIT, -20.0, -8.0), 4.0);
        near(match_gain_db(&NO_LIMIT, -12.0, -3.0), -4.0);
    }

    #[test]
    fn limiting_on_does_not_cap_the_gain() {
        near(match_gain_db(&OWNER, -20.0, -3.0), 4.0);
    }

    #[test]
    fn silence_and_bad_measurements_get_no_gain() {
        near(match_gain_db(&OWNER, -144.0, -144.0), 0.0);
        near(match_gain_db(&OWNER, -70.5, -50.0), 0.0);
        near(match_gain_db(&OWNER, f64::NAN, -6.0), 0.0);
        near(match_gain_db(&OWNER, f64::NEG_INFINITY, -6.0), 0.0);
        near(match_gain_db(&PEAK, -30.0, -144.0), 0.0);
        near(match_gain_db(&PEAK, -30.0, f64::NAN), 0.0);
        // -70 itself is measurable: 54 dB of gain, limiting on so no cap
        near(match_gain_db(&OWNER, -70.0, -50.0), 54.0);
    }

    #[test]
    fn within_tolerance_uses_the_tolerance_around_the_target() {
        assert!(within_tolerance(&OWNER, -16.4, -3.0));
        assert!(within_tolerance(&OWNER, -16.5, -3.0));
        assert!(within_tolerance(&OWNER, -15.6, -3.0));
        assert!(!within_tolerance(&OWNER, -16.6, -3.0));
        assert!(!within_tolerance(&OWNER, -15.4, -3.0));
        assert!(within_tolerance(&EBU, -23.3, -3.0));
        assert!(!within_tolerance(&EBU, -16.0, -3.0));
        assert!(within_tolerance(&PEAK, -40.0, -3.2));
        assert!(!within_tolerance(&PEAK, -40.0, -3.6));
        assert!(!within_tolerance(&OWNER, f64::NAN, -3.0));
    }
}
