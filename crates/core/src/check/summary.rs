//! The Episode Check summary (K5): the lines the check panel shows under a run, built from the
//! document's loudness, its channel count and the scan [`Report`].
//!
//! Pure: it reads the numbers it is given and formats them. It measures nothing and changes no
//! document.

use super::Report;
use crate::loudness::Loudness;

/// Integrated loudness target for stereo and wider, LUFS.
pub const TARGET_STEREO_LUFS: f64 = -16.0;
/// Integrated loudness target for mono, LUFS.
pub const TARGET_MONO_LUFS: f64 = -19.0;

/// The integrated loudness target for `channels` channels: -16 LUFS for two or more, -19 LUFS
/// for mono.
pub fn target_lufs(channels: usize) -> f64 {
    if channels >= 2 { TARGET_STEREO_LUFS } else { TARGET_MONO_LUFS }
}

/// What the check panel shows for a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Gated integrated loudness, LUFS.
    pub integrated_lufs: f64,
    /// The target for the channel count, LUFS (see [`target_lufs`]).
    pub target_lufs: f64,
    /// Highest 4x-oversampled peak, dBTP.
    pub true_peak_dbtp: f64,
    /// Findings in the report.
    pub findings_left: usize,
    /// The text lines, in order: integrated loudness against its target, true peak, findings
    /// left.
    pub lines: Vec<String>,
}

impl Summary {
    /// Builds the summary from the document's `loudness`, its `channels` count and the scan
    /// `report`. A report that was truncated counts only the findings it kept.
    pub fn new(loudness: &Loudness, channels: usize, report: &Report) -> Self {
        let target_lufs = target_lufs(channels);
        let findings_left = report.findings.len();
        let lines = vec![
            format!("Integrated loudness: {} LUFS (target {target_lufs:.1})", level(loudness.integrated_lufs)),
            format!("True peak: {} dBTP", level(loudness.true_peak_dbtp)),
            format!("Findings left: {findings_left}"),
        ];
        Self { integrated_lufs: loudness.integrated_lufs, target_lufs, true_peak_dbtp: loudness.true_peak_dbtp, findings_left, lines }
    }
}

/// One decimal place, or `-inf` for silence (and anything else that is not finite).
fn level(v: f64) -> String {
    if v.is_finite() { format!("{v:.1}") } else { "-inf".to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Finding, Kind, Severity};

    fn loudness(integrated_lufs: f64, true_peak_dbtp: f64) -> Loudness {
        Loudness { frames: 48_000, integrated_lufs, range_lu: 4.0, true_peak_dbtp, sample_peak_dbfs: -1.5, max_momentary_lufs: -14.0 }
    }

    fn finding() -> Finding {
        Finding { kind: Kind::Clipping, severity: Severity::Error, channel: Some(0), start: 0, end: 8, value: 1.0 }
    }

    #[test]
    fn target_is_minus_16_stereo_minus_19_mono() {
        assert_eq!(target_lufs(2), -16.0);
        assert_eq!(target_lufs(6), -16.0);
        assert_eq!(target_lufs(1), -19.0);
    }

    #[test]
    fn lines_are_exact() {
        let report = Report { findings: vec![finding(), finding()], truncated: false };
        let summary = Summary::new(&loudness(-16.3, -1.4), 2, &report);
        assert_eq!(
            summary.lines,
            vec!["Integrated loudness: -16.3 LUFS (target -16.0)".to_string(), "True peak: -1.4 dBTP".to_string(), "Findings left: 2".to_string(),]
        );
        assert_eq!(summary.findings_left, 2);
        assert_eq!(summary.target_lufs, -16.0);
    }
}
