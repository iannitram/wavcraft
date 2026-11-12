//! The findings list (K3): one [`Row`] per finding of a [`Report`], and the finding after or
//! before a frame, wrapping at the ends. Pure: no UI and no document.
//!
//! Findings are in frame order, so the comparisons use each finding's `start`.

use std::ops::Range;

use super::{Kind, Report, Severity};

/// One line of the findings list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// What the finding is: "Clipping", "True peak", "DC offset", "Dead air", "Loudness drift",
    /// "Mains hum" or "Plosive".
    pub kind: &'static str,
    /// Where the finding starts, as `m:ss.mmm`.
    pub time: String,
    /// "Warning" or "Error".
    pub severity: &'static str,
    /// The frames of the finding: its start up to, not including, its end.
    pub range: Range<u64>,
}

/// The label of a finding kind, in Audition's words for the Episode Check panel.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Clipping => "Clipping",
        Kind::TruePeak => "True peak",
        Kind::DcOffset => "DC offset",
        Kind::DeadAir => "Dead air",
        Kind::LoudnessDrift => "Loudness drift",
        Kind::MainsHum => "Mains hum",
        Kind::Plosive => "Plosive",
    }
}

/// The label of a severity.
pub fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "Warning",
        Severity::Error => "Error",
    }
}

/// `frame` as `m:ss.mmm` at `rate` Hz, truncated to the millisecond. A rate of 0 reads as 0:00.000.
pub fn time_text(frame: u64, rate: u32) -> String {
    let total_ms = u128::from(frame).saturating_mul(1_000).checked_div(u128::from(rate)).unwrap_or(0);
    let secs = total_ms / 1_000;
    format!("{}:{:02}.{:03}", secs / 60, secs % 60, total_ms % 1_000)
}

/// One [`Row`] per finding of `report`, in its order. `rate` is the sample rate in Hz.
pub fn rows(report: &Report, rate: u32) -> Vec<Row> {
    report
        .findings
        .iter()
        .map(|f| Row { kind: kind_label(f.kind), time: time_text(f.start, rate), severity: severity_label(f.severity), range: f.start..f.end })
        .collect()
}

/// The index of the first finding that starts after `frame`. When none does, it wraps to the
/// first finding. `None` when there are no findings.
pub fn next_after(report: &Report, frame: u64) -> Option<usize> {
    report.findings.iter().position(|f| f.start > frame).or_else(|| (!report.findings.is_empty()).then_some(0))
}

/// The index of the last finding that starts before `frame`. When none does, it wraps to the
/// last finding. `None` when there are no findings.
pub fn prev_before(report: &Report, frame: u64) -> Option<usize> {
    report.findings.iter().rposition(|f| f.start < frame).or_else(|| report.findings.len().checked_sub(1))
}

/// The pixel column of `frame` on a map strip `width_px` wide, for a file of `total_frames`.
/// Frame 0 is column 0, and the last frame is the last column, `width_px - 1`. Frames past the
/// end read as the last frame. Returns 0 when the file or the strip is empty.
pub fn pin_x(frame: u64, total_frames: u64, width_px: u32) -> u32 {
    if total_frames == 0 || width_px == 0 {
        return 0;
    }
    let frame = frame.min(total_frames - 1);
    let x = u128::from(frame) * u128::from(width_px) / u128::from(total_frames);
    u32::try_from(x).unwrap_or(width_px - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Finding;

    #[test]
    fn pin_x_maps_frame_to_pixel() {
        assert_eq!(pin_x(0, 1000, 200), 0);
        assert_eq!(pin_x(500, 1000, 200), 100);
        assert_eq!(pin_x(999, 1000, 200), 199);
        assert_eq!(pin_x(5000, 1000, 200), 199);
        assert_eq!(pin_x(0, 0, 200), 0);
        assert_eq!(pin_x(0, 1000, 0), 0);
    }

    fn finding(kind: Kind, severity: Severity, start: u64, end: u64) -> Finding {
        Finding { kind, severity, channel: Some(0), start, end, value: 0.0 }
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report { findings, truncated: false }
    }

    #[test]
    fn row_range_is_exactly_the_finding() {
        let r = report(vec![finding(Kind::Clipping, Severity::Error, 144_000, 144_480)]);
        let got = rows(&r, 48_000);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].range, 144_000..144_480);
        assert_eq!(got[0].range, r.findings[0].start..r.findings[0].end);
        assert_eq!(got[0].kind, "Clipping");
        assert_eq!(got[0].time, "0:03.000");
        assert_eq!(got[0].severity, "Error");
    }

    #[test]
    fn labels_for_every_kind() {
        assert_eq!(kind_label(Kind::Clipping), "Clipping");
        assert_eq!(kind_label(Kind::TruePeak), "True peak");
        assert_eq!(kind_label(Kind::DcOffset), "DC offset");
        assert_eq!(kind_label(Kind::DeadAir), "Dead air");
        assert_eq!(kind_label(Kind::LoudnessDrift), "Loudness drift");
        assert_eq!(kind_label(Kind::MainsHum), "Mains hum");
        assert_eq!(kind_label(Kind::Plosive), "Plosive");
        assert_eq!(severity_label(Severity::Warning), "Warning");
        assert_eq!(severity_label(Severity::Error), "Error");
    }

    #[test]
    fn time_of_frame_144000_at_48k_is_0_03_000() {
        assert_eq!(time_text(144_000, 48_000), "0:03.000");
    }

    #[test]
    fn next_and_previous_wrap() {
        let r = report(vec![
            finding(Kind::Clipping, Severity::Error, 100, 110),
            finding(Kind::TruePeak, Severity::Error, 200, 210),
            finding(Kind::Plosive, Severity::Warning, 300, 310),
        ]);
        assert_eq!(next_after(&r, 0), Some(0));
        assert_eq!(next_after(&r, 100), Some(1));
        assert_eq!(next_after(&r, 250), Some(2));
        assert_eq!(next_after(&r, 300), Some(0));
        assert_eq!(prev_before(&r, 0), Some(2));
        assert_eq!(prev_before(&r, 100), Some(2));
        assert_eq!(prev_before(&r, 250), Some(1));
        assert_eq!(prev_before(&r, 300), Some(1));
        assert_eq!(prev_before(&r, 301), Some(2));

        let empty = report(Vec::new());
        assert_eq!(next_after(&empty, 0), None);
        assert_eq!(prev_before(&empty, 0), None);
    }
}
