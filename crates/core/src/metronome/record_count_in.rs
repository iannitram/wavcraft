//! Count-in before a Record (roadmap C202, core only).
//!
//! Given the click grid ([`ClickTrack`]), the count-in setting and the frame a recording is asked
//! to start at, this works out how long the pre-roll is, the frame the recording really begins at
//! (the start plus the pre-roll) and the clicks to sound during the pre-roll. The click maths is
//! the [`ClickTrack`]'s own; nothing here records or plays.

use std::fmt;

use super::{Click, ClickTrack};
use crate::tempo::Tempo;

/// The most bars of count-in accepted (the same 0 to 16 as the Play count-in).
pub const MAX_COUNT_IN_BARS: u32 = 16;

/// The count-in setting for a Record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordCountIn {
    /// Bars of count-in, 0 to [`MAX_COUNT_IN_BARS`]; 0 means none.
    pub bars: u32,
    /// Whether the count-in applies when recording.
    pub on_record: bool,
}

/// Why a count-in plan could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountInError {
    /// The sample rate is 0.
    BadRate,
    /// The tempo or meter is outside what `Tempo::new` accepts.
    BadTempo,
    /// More than [`MAX_COUNT_IN_BARS`] bars.
    TooManyBars,
    /// The start plus the pre-roll does not fit in a frame number.
    Overflow,
}

impl fmt::Display for CountInError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CountInError::BadRate => write!(f, "sample rate must be above 0"),
            CountInError::BadTempo => write!(f, "tempo must be 20 to 400 beats a minute with 1 to 32 beats a bar"),
            CountInError::TooManyBars => write!(f, "count-in must be 0 to 16 bars"),
            CountInError::Overflow => write!(f, "record position plus count-in is past the largest frame"),
        }
    }
}

/// What a Record with a count-in does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPlan {
    /// Frames of pre-roll (0 when no count-in applies).
    pub pre_roll_frames: u64,
    /// The frame the recording begins at: the asked-for start plus the pre-roll.
    pub record_from: u64,
    /// The clicks to sound in `[record_start, record_from)`, frames counted on the same timeline
    /// as `record_from`; beat 0 (the accent of bar 1) falls on the asked-for start.
    pub clicks: Vec<Click>,
}

/// Check the click grid: the rate is above 0 and the tempo is one `Tempo::new` accepts.
pub fn check_track(track: &ClickTrack) -> Result<(), CountInError> {
    if track.rate == 0 {
        return Err(CountInError::BadRate);
    }
    if Tempo::new(track.tempo.bpm, track.tempo.numerator, track.tempo.denominator).is_none() {
        return Err(CountInError::BadTempo);
    }
    Ok(())
}

/// Frames of pre-roll for `settings` on `track`, after checking both.
pub fn pre_roll_frames(track: &ClickTrack, settings: &RecordCountIn) -> Result<u64, CountInError> {
    check_track(track)?;
    if settings.bars > MAX_COUNT_IN_BARS {
        return Err(CountInError::TooManyBars);
    }
    if !settings.on_record || settings.bars == 0 { Ok(0) } else { Ok(track.bars_frames(settings.bars)) }
}

/// The plan for a Record asked to start at frame `record_start`.
pub fn plan_record_count_in(track: &ClickTrack, settings: &RecordCountIn, record_start: u64) -> Result<RecordPlan, CountInError> {
    let pre = pre_roll_frames(track, settings)?;
    let record_from = record_start.checked_add(pre).ok_or(CountInError::Overflow)?;
    let clicks = track.clicks(0, pre).into_iter().map(|c| Click { frame: c.frame + record_start, ..c }).collect();
    Ok(RecordPlan { pre_roll_frames: pre, record_from, clicks })
}
