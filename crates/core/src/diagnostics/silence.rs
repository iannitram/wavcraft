//! Silence scan and Delete Silence (z13.9, B047).
//!
//! **Level measure: sample peak.** A frame is quiet when the largest absolute sample over the
//! scanned channels is below the threshold (`threshold_db` dBFS, full scale 1.0). A stretch is a
//! run of quiet frames at least `min_len_ms` long. (A short RMS window would ride over single
//! clicks; the peak never hides one, so audio is never cut that the scan could not see.)
//!
//! **Streamed.** The scan reads 64 K-frame chunks, one channel buffer each, and carries only the
//! start of the run it is in, so a stretch that crosses a chunk join is found whole. At most
//! [`MAX_STRETCHES`] are held; more is an error, not a truncated answer.
//!
//! **Delete Silence** shortens every stretch to `keep_ms` (half kept at each end of it) by deleting
//! its middle from all channels, as one history step labelled [`LABEL`]. The edits are pure
//! functions of one state to the next ([`crate::doc::edit`]), so the deletes are applied last to
//! first on a working state and only the final state is pushed: no compound history step is
//! needed, and Undo is the one old state. A document that is silent end to end is refused.
//! Markers are not on this base (a separate change), so none move.

use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doc::DocState;
use crate::doc::edit::{self, Change, ChannelMask, Crossfades, Edit, EditError};
use crate::store::BLOCK_FRAMES;

/// The history label.
pub const LABEL: &str = "Delete Silence";
/// Stretches held at most; a scan that finds more is refused.
pub const MAX_STRETCHES: usize = 10_000;

/// The settings. Defaults: -48 dB, 500 ms, keep 150 ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceParams {
    pub threshold_db: f64,
    pub min_len_ms: u32,
    pub keep_ms: u32,
}

impl Default for SilenceParams {
    fn default() -> Self {
        SilenceParams { threshold_db: -48.0, min_len_ms: 500, keep_ms: 150 }
    }
}

/// A quiet stretch: `len` frames from `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stretch {
    pub start: u64,
    pub len: u64,
}

/// Why a scan or Delete Silence did not run. The document is unchanged.
#[derive(Debug)]
pub enum SilenceError {
    /// The settings are unusable (not a number, above 0 dB, a minimum of 0).
    BadParams(String),
    /// Everything scanned is quiet: nothing would be left.
    AllSilent,
    /// More than [`MAX_STRETCHES`] stretches.
    TooMany,
    Edit(EditError),
}

impl fmt::Display for SilenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SilenceError::BadParams(s) => write!(f, "{s}"),
            SilenceError::AllSilent => write!(f, "everything is below the threshold; nothing would be left (lower the threshold)"),
            SilenceError::TooMany => write!(f, "more than {MAX_STRETCHES} silent stretches; raise the minimum length or select a part"),
            SilenceError::Edit(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SilenceError {}

impl From<EditError> for SilenceError {
    fn from(e: EditError) -> Self {
        SilenceError::Edit(e)
    }
}

impl From<std::io::Error> for SilenceError {
    fn from(e: std::io::Error) -> Self {
        SilenceError::Edit(e.into())
    }
}

impl SilenceParams {
    fn check(&self) -> Result<(), SilenceError> {
        if !self.threshold_db.is_finite() || self.threshold_db > 0.0 {
            return Err(SilenceError::BadParams("threshold_db must be a number, 0 or below".to_string()));
        }
        if self.min_len_ms == 0 {
            return Err(SilenceError::BadParams("min_len_ms must be above 0".to_string()));
        }
        Ok(())
    }
}

fn ms_to_frames(ms: u32, rate: u32) -> u64 {
    (u64::from(ms) * u64::from(rate)).div_ceil(1000)
}

/// The quiet stretches of `range` (the whole file when empty), judged on the peak over the
/// channels in `channels`. Frames are in the document's own numbering.
pub fn scan_silence(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    p: &SilenceParams,
    cancel: &AtomicBool,
) -> Result<Vec<Stretch>, SilenceError> {
    p.check()?;
    let len = doc.len_frames();
    let range = if range.start < range.end { range.start.min(len)..range.end.min(len) } else { 0..len };
    let active = channels.active(doc.channel_count());
    if active.is_empty() || range.is_empty() {
        return Ok(Vec::new());
    }
    let limit = 10f64.powf(p.threshold_db / 20.0) as f32;
    let min_frames = ms_to_frames(p.min_len_ms, doc.sample_rate()).max(1);
    let mut peak = vec![0.0f32; BLOCK_FRAMES];
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut out = Vec::new();
    let mut run: Option<u64> = None;
    let close = |out: &mut Vec<Stretch>, start: u64, end: u64| -> Result<(), SilenceError> {
        if end - start >= min_frames {
            if out.len() >= MAX_STRETCHES {
                return Err(SilenceError::TooMany);
            }
            out.push(Stretch { start, len: end - start });
        }
        Ok(())
    };
    let mut at = range.start;
    while at < range.end {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled.into());
        }
        let m = (range.end - at).min(BLOCK_FRAMES as u64) as usize;
        peak[..m].fill(0.0);
        for &c in &active {
            let got = doc.read(c, at, &mut buf[..m])?;
            if got != m {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
            for (pk, v) in peak[..m].iter_mut().zip(&buf[..m]) {
                // A NaN sample counts as loud: it is not silence.
                let a = if v.is_nan() { f32::INFINITY } else { v.abs() };
                if a > *pk {
                    *pk = a;
                }
            }
        }
        for (i, &pk) in peak[..m].iter().enumerate() {
            let quiet = pk < limit;
            match (quiet, run) {
                (true, None) => run = Some(at + i as u64),
                (false, Some(s)) => {
                    close(&mut out, s, at + i as u64)?;
                    run = None;
                }
                _ => {}
            }
        }
        at += m as u64;
    }
    if let Some(s) = run {
        close(&mut out, s, range.end)?;
    }
    Ok(out)
}

/// What Delete Silence did.
#[derive(Debug, Clone)]
pub struct Deleted {
    pub edit: Edit,
    /// Stretches that were shortened.
    pub stretches: usize,
    /// Frames removed in all.
    pub removed_frames: u64,
}

/// The frames to delete from each stretch: all but `keep` of it, half of `keep` at each end.
pub fn excess(stretches: &[Stretch], keep: u64) -> Vec<Range<u64>> {
    stretches
        .iter()
        .filter(|s| s.len > keep)
        .map(|s| {
            let head = keep / 2;
            let tail = keep - head;
            s.start + head..s.start + s.len - tail
        })
        .collect()
}

/// Shorten every silent stretch of `range` (the whole file when empty) to `keep_ms`, on all
/// channels. The scan reads the channels in `scan_channels`. Nothing to remove gives a no-op edit.
pub fn delete_silence(
    doc: &DocState,
    range: Range<u64>,
    scan_channels: ChannelMask,
    p: &SilenceParams,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Deleted, SilenceError> {
    let len = doc.len_frames();
    let stretches = scan_silence(doc, range.clone(), scan_channels, p, cancel)?;
    // One stretch that is the whole file: nothing but silence, so nothing to keep.
    if len > 0 && stretches.iter().any(|s| s.start == 0 && s.len == len) {
        return Err(SilenceError::AllSilent);
    }
    let cuts = excess(&stretches, ms_to_frames(p.keep_ms, doc.sample_rate()));
    let mut state = doc.clone();
    let mut removed = 0;
    // Last to first, so the frame numbers of the cuts still to come stay true.
    for r in cuts.iter().rev() {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled.into());
        }
        removed += r.end - r.start;
        state = edit::delete(&state, r.clone(), ChannelMask::ALL, xf)?.state;
    }
    let change = cuts.first().map_or(Change::None, |r| Change::LengthFrom { frame: r.start });
    Ok(Deleted { edit: Edit { state, label: LABEL.to_string(), change }, stretches: cuts.len(), removed_frames: removed })
}
