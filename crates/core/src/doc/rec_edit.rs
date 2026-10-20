//! Recording into an open document (roadmap C026 record at the cursor or over the selection,
//! C027 punch-in, C030 record target channel).
//!
//! The take is recorded to its own file first (`record`), crash-safe and never touched by this
//! module. When it is done and has been opened through the normal streamed open, [`apply_take`]
//! turns it into one [`Edit`] on the document: pure, like every edit in [`edit`], so it is one
//! history step and the document is unchanged if it fails.

use std::ops::Range;

use super::DocState;
use super::edit::{self, Change, ChannelMask, Crossfades, Edit, EditError, PasteAt};

/// History labels.
pub const RECORD: &str = "Record";
pub const RECORD_OVER: &str = "Record Over";
pub const PUNCH_IN: &str = "Punch In";

/// Where the take goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecMode {
    /// Put the take in at the cursor; what follows moves along (the file grows).
    Insert,
    /// Put the take in place of the selection. With only a cursor, it overwrites from the
    /// cursor for the take's length (the file never grows).
    Replace,
    /// Play from a preroll, record only inside the selection and replace that range (same
    /// length). The take stops by itself at the selection's end.
    Punch,
}

impl RecMode {
    pub fn parse(text: &str) -> Option<RecMode> {
        match text {
            "insert" => Some(RecMode::Insert),
            "replace" => Some(RecMode::Replace),
            "punch" => Some(RecMode::Punch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RecMode::Insert => "insert",
            RecMode::Replace => "replace",
            RecMode::Punch => "punch",
        }
    }
}

/// Which channels of the document the take goes into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecTarget {
    #[default]
    Both,
    Left,
    Right,
}

impl RecTarget {
    pub fn parse(text: &str) -> Option<RecTarget> {
        match text {
            "both" | "B" | "b" => Some(RecTarget::Both),
            "L" | "l" | "left" => Some(RecTarget::Left),
            "R" | "r" | "right" => Some(RecTarget::Right),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RecTarget::Both => "both",
            RecTarget::Left => "L",
            RecTarget::Right => "R",
        }
    }
}

/// What a recording into the document was asked to do, fixed when it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecPlan {
    pub mode: RecMode,
    pub target: RecTarget,
    /// The cursor (frames) when the recording started.
    pub cursor: u64,
    /// The selection (frames) when it started; empty is just the cursor.
    pub selection: Range<u64>,
}

impl RecPlan {
    /// Whether this plan can be carried out on `doc`. Called before anything is recorded, so a
    /// plan that cannot work never gets as far as a take.
    pub fn check(&self, doc: &DocState) -> Result<(), String> {
        let n = doc.channel_count();
        match self.target {
            RecTarget::Right if n < 2 => return Err("the file has no right channel".to_string()),
            RecTarget::Both if n > 2 => return Err("recording into both channels needs a mono or stereo file; choose left or right".to_string()),
            _ => {}
        }
        if self.mode == RecMode::Punch && self.selection.start >= self.selection.end {
            return Err("punch-in records inside a selection: select the range to replace first".to_string());
        }
        if self.mode == RecMode::Insert && !self.mask(n).covers_all(n) {
            return Err("insert records into both channels (one channel cannot grow alone): use replace or punch-in for one channel".to_string());
        }
        Ok(())
    }

    /// How many channels the take is recorded with: both channels of a stereo file, else one.
    pub fn take_channels(&self, doc: &DocState) -> u16 {
        if self.target == RecTarget::Both { doc.channel_count().clamp(1, 2) as u16 } else { 1 }
    }

    /// The most frames the take may hold: a punch stops at the selection's end.
    pub fn limit_frames(&self) -> Option<u64> {
        self.limit_frames_with_offset(0)
    }

    /// [`limit_frames`](Self::limit_frames) for a recording that will have `offset_frames`
    /// dropped from the front of the take (C010): the take runs that much longer, so the
    /// range still gets filled to its end.
    pub fn limit_frames_with_offset(&self, offset_frames: u64) -> Option<u64> {
        (self.mode == RecMode::Punch).then(|| self.selection.end.saturating_sub(self.selection.start).saturating_add(offset_frames))
    }

    fn mask(&self, n: usize) -> ChannelMask {
        match self.target {
            RecTarget::Both => ChannelMask::ALL,
            RecTarget::Left => ChannelMask::only(0),
            RecTarget::Right => ChannelMask::only(1),
        }
        .within(n)
    }
}

/// The one edit that brings the finished `take` (opened from its file, so in a store of its own)
/// into `doc` as `plan` says. The document is not touched; the caller pushes the [`Edit`] as a
/// single history step. A take at another sample rate is refused ([`EditError::SampleRate`]).
pub fn apply_take(doc: &DocState, take: &DocState, plan: &RecPlan, xf: Crossfades) -> Result<Edit, EditError> {
    apply_take_with_offset(doc, take, plan, xf, 0)
}

/// The largest record offset accepted, in milliseconds.
pub const MAX_OFFSET_MS: u32 = 2000;

/// How many frames at `rate` a record offset of `ms` milliseconds is (rounded to the nearest).
pub fn offset_frames(ms: u32, rate: u32) -> u64 {
    (u64::from(ms) * u64::from(rate) + 500) / 1000
}

/// The record offset to use: the preference when the owner set one, else what the device
/// reported (microseconds, rounded to milliseconds), else 0. Never above [`MAX_OFFSET_MS`].
pub fn resolve_offset_ms(preference: Option<u32>, reported_us: Option<u64>) -> u32 {
    let ms = preference.unwrap_or_else(|| reported_us.map_or(0, |us| ((us + 500) / 1000).min(u64::from(MAX_OFFSET_MS)) as u32));
    ms.min(MAX_OFFSET_MS)
}

/// [`apply_take`] with latency compensation (C010, C050): what the input delivered arrived
/// `offset_frames` later than what was heard, so the first `offset_frames` of the take are left
/// out and the rest is placed where the take began. The take file itself is not changed. A take
/// no longer than the offset places nothing (the edit is a no-op). A punch-in's take should
/// have been recorded `offset_frames` longer ([`RecPlan::limit_frames_with_offset`]).
pub fn apply_take_with_offset(doc: &DocState, take: &DocState, plan: &RecPlan, xf: Crossfades, offset_frames: u64) -> Result<Edit, EditError> {
    let skip = offset_frames.min(take.len_frames());
    let len = doc.len_frames();
    let mask = plan.mask(doc.channel_count());
    // One channel takes the take's first channel; both take the take as it is.
    let source_channels = if plan.target == RecTarget::Both { ChannelMask::ALL } else { ChannelMask::only(0) };
    let mut n = take.len_frames() - skip;
    let (cursor, sel) = (plan.cursor.min(len), plan.selection.start.min(len)..plan.selection.end.min(len));
    let (at, label) = match plan.mode {
        RecMode::Insert => (PasteAt::Insert(cursor), RECORD),
        RecMode::Replace if sel.start < sel.end => (PasteAt::Replace(sel), RECORD_OVER),
        RecMode::Replace => {
            // Over a cursor the file never grows: the take is cut where the file ends.
            n = n.min(len - cursor);
            (PasteAt::Replace(cursor..cursor + n), RECORD_OVER)
        }
        RecMode::Punch => {
            n = n.min(sel.end.saturating_sub(sel.start));
            (PasteAt::Replace(sel.start..sel.start.saturating_add(n).min(sel.end)), PUNCH_IN)
        }
    };
    let clip = edit::copy(take, skip..skip + n, source_channels)?;
    let mut out = edit::paste(doc, at, &clip, mask, xf)?;
    if out.change != Change::None {
        out.label = label.to_string();
    }
    Ok(out)
}
