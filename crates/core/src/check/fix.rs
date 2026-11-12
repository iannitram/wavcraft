//! The suggested fix for a finding (K4): one [`Fix`] that an existing tool can run on the finding's
//! range and channels. Pure: [`suggest`] reads a [`Finding`] and changes no document. Applying a
//! fix is another step (K4.4).
//!
//! - Loudness drift: Amplify by the opposite of the finding's value, so a stretch 8 LU louder than
//!   the file goes down 8 dB and one 8 LU quieter goes up 8 dB.
//! - Dead air: Delete the range.
//! - Mains hum: Noise Reduction with its default settings (K4.5 captures the noise print first).
//! - Plosive: High-Pass Filter at [`PLOSIVE_CUTOFF_HZ`] and 24 dB per octave.
//! - DC offset: Remove DC Offset.
//! - Clipping and true peak: no fix yet (K4.6 adds them), so [`suggest`] gives `None`.

use std::ops::Range;

use super::{Finding, Kind};
use crate::effects::{AMPLIFY_MAX_GAIN_DB, MIN_GAIN_DB};

/// Every channel: the mask of a finding that covers all of them.
pub const ALL_CHANNELS: ChannelMask = u64::MAX;

/// The channels a fix covers, one bit per channel (bit 0 is channel 0). See [`ALL_CHANNELS`].
pub type ChannelMask = u64;

/// Cutoff of the plosive fix, Hz: the band the plosive detector measures ([`super::Config`]'s
/// `plosive_band_hz`).
pub const PLOSIVE_CUTOFF_HZ: f64 = 120.0;

/// The plosive fix's `slope` setting: the index of High-Pass Filter's slope choice, which is 1 for
/// 24 dB per octave (choices `12`, `24`).
pub const PLOSIVE_SLOPE_CHOICE: f64 = 1.0;

/// A fix a finding suggests.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    /// The fix's name, in Audition's words.
    pub label: &'static str,
    /// The frames it covers: the finding's start up to, not including, its end.
    pub range: Range<u64>,
    /// The channels it covers.
    pub channels: ChannelMask,
    /// What it does to them.
    pub operation: Operation,
}

/// What a fix does.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Run the effect with this id (its `Effect::id`). Each setting is keyed by its
    /// `ParamDesc::id` and given in the effect's own units (a choice is its index). A setting left
    /// out takes the effect's default.
    Effect { id: &'static str, settings: Vec<(&'static str, f64)> },
    /// Delete the range.
    Delete,
}

/// The fix for `finding`, or `None` when no existing tool answers it (clipping, true peak), when
/// its range is empty, or when its level is outside what the tool accepts.
pub fn suggest(finding: &Finding) -> Option<Fix> {
    if finding.start >= finding.end {
        return None;
    }
    let channels = match finding.channel {
        None => ALL_CHANNELS,
        Some(channel) => channel_bit(channel)?,
    };
    let (label, operation) = match finding.kind {
        Kind::LoudnessDrift => {
            let gain = -finding.value;
            if !(MIN_GAIN_DB..=AMPLIFY_MAX_GAIN_DB).contains(&gain) {
                return None;
            }
            ("Amplify", Operation::Effect { id: "amplify", settings: vec![("left_db", gain), ("right_db", gain)] })
        }
        Kind::DeadAir => ("Delete", Operation::Delete),
        Kind::MainsHum => ("Noise Reduction", Operation::Effect { id: "noise_reduction", settings: Vec::new() }),
        Kind::Plosive => (
            "High-Pass Filter",
            Operation::Effect { id: "high_pass", settings: vec![("cutoff", PLOSIVE_CUTOFF_HZ), ("slope", PLOSIVE_SLOPE_CHOICE)] },
        ),
        Kind::DcOffset => ("Remove DC Offset", Operation::Effect { id: "dc_offset", settings: Vec::new() }),
        Kind::Clipping | Kind::TruePeak => return None,
    };
    Some(Fix { label, range: finding.start..finding.end, channels, operation })
}

/// The mask with only `channel`'s bit set; `None` past the 64 channels a mask holds.
fn channel_bit(channel: usize) -> Option<ChannelMask> {
    u32::try_from(channel).ok().and_then(|shift| 1u64.checked_shl(shift))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Severity;
    use crate::effects::{Effect, HighPass};

    fn finding(kind: Kind, channel: Option<usize>, start: u64, end: u64, value: f64) -> Finding {
        Finding { kind, severity: Severity::Warning, channel, start, end, value }
    }

    fn operation_of(finding: &Finding) -> Option<Operation> {
        suggest(finding).map(|fix| fix.operation)
    }

    fn effect(id: &'static str, settings: Vec<(&'static str, f64)>) -> Option<Operation> {
        Some(Operation::Effect { id, settings })
    }

    #[test]
    fn every_named_kind_has_one_fix() {
        let drift_up = finding(Kind::LoudnessDrift, None, 0, 480_000, 8.0);
        assert_eq!(operation_of(&drift_up), effect("amplify", vec![("left_db", -8.0), ("right_db", -8.0)]));
        assert_eq!(suggest(&drift_up).map(|f| (f.label, f.channels)), Some(("Amplify", ALL_CHANNELS)));

        let drift_down = finding(Kind::LoudnessDrift, None, 0, 480_000, -8.0);
        assert_eq!(operation_of(&drift_down), effect("amplify", vec![("left_db", 8.0), ("right_db", 8.0)]));

        let dead_air = finding(Kind::DeadAir, None, 768_000, 936_000, -70.0);
        let fix = suggest(&dead_air);
        assert_eq!(fix.as_ref().map(|f| f.range.clone()), Some(768_000..936_000));
        assert_eq!(fix.map(|f| f.operation), Some(Operation::Delete));

        let hum = finding(Kind::MainsHum, Some(1), 0, 480_000, 60.0);
        assert_eq!(operation_of(&hum), effect("noise_reduction", Vec::new()));
        assert_eq!(suggest(&hum).map(|f| f.channels), Some(0b10));

        let plosive = finding(Kind::Plosive, Some(0), 48_000, 96_000, -20.0);
        assert_eq!(operation_of(&plosive), effect("high_pass", vec![("cutoff", 120.0), ("slope", 1.0)]));
        // Slope choice 1 is the second of High-Pass Filter's choices: 24 dB per octave.
        let slope_desc = HighPass.parameters().iter().find(|p| p.id == "slope");
        assert_eq!(slope_desc.and_then(|p| p.choices.get(1).copied()), Some("24"));
        assert_eq!(suggest(&plosive).map(|f| f.channels), Some(0b1));

        let dc = finding(Kind::DcOffset, Some(0), 0, 480_000, 0.03);
        assert_eq!(operation_of(&dc), effect("dc_offset", Vec::new()));

        let clipping = finding(Kind::Clipping, Some(0), 0, 480_000, 1.0);
        assert_eq!(suggest(&clipping), None);
        assert_eq!(operation_of(&finding(Kind::TruePeak, Some(0), 0, 480_000, 0.5)), None);
    }

    #[test]
    fn no_fix_for_an_empty_range_a_gain_out_of_reach_or_a_channel_past_the_mask() {
        assert_eq!(suggest(&finding(Kind::DeadAir, None, 100, 100, -70.0)), None);
        assert_eq!(suggest(&finding(Kind::LoudnessDrift, None, 0, 480_000, -45.0)), None);
        assert_eq!(suggest(&finding(Kind::LoudnessDrift, None, 0, 480_000, f64::NAN)), None);
        assert_eq!(suggest(&finding(Kind::DcOffset, Some(64), 0, 480_000, 0.03)), None);
        assert_eq!(suggest(&finding(Kind::DcOffset, Some(63), 0, 480_000, 0.03)).map(|f| f.channels), Some(1u64 << 63));
    }
}
