//! Open Append (f57): files added to the end of a document, or put back to back in a new one.
//!
//! Each file is streamed into the document's own session store (`io::open_into`, a block at a
//! time, so no file is ever held whole in RAM), then joined on with the edit machinery's paste:
//! the channels map as Paste maps them (equal counts, mono into any, any into mono as the mean)
//! and a different sample rate is refused before anything is written. All the files that went in
//! make one [`Edit`], so the history gets one step. A file that cannot be added is refused with
//! its reason and the others go on.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::DocState;
use super::edit::{self, Change, ChannelMask, Crossfades, Edit, PasteAt};
use crate::io::{self, OpenError, OpenOptions};
use crate::space::FreeSpaceFn;

/// The history label of an append.
pub const APPEND: &str = "Append";

/// What an append did.
#[derive(Debug, Default)]
pub struct AppendReport {
    /// The one edit that adds every file that went in (`None` when none did).
    pub edit: Option<Edit>,
    /// The files added, in order, with their lengths in frames.
    pub added: Vec<(PathBuf, u64)>,
    /// The files refused and why.
    pub refused: Vec<(PathBuf, String)>,
    /// The cancel flag stopped it: nothing is changed.
    pub cancelled: bool,
}

/// A new document made of files back to back.
#[derive(Debug, Default)]
pub struct NewReport {
    pub state: Option<DocState>,
    pub added: Vec<(PathBuf, u64)>,
    pub refused: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

/// Append each of `paths` to the end of `base`. `progress` gets 0 to 1 over all the files.
pub fn append_files(
    base: &DocState,
    paths: &[PathBuf],
    free_space: Option<FreeSpaceFn>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> AppendReport {
    let mut report = AppendReport::default();
    let mut cur = base.clone();
    let n = paths.len().max(1) as f64;
    for (i, path) in paths.iter().enumerate() {
        let mut each = |p: f64| progress((i as f64 + p) / n);
        match append_one(&cur, path, free_space, &mut each, cancel) {
            Ok((next, frames)) => {
                report.added.push((path.clone(), frames));
                cur = next;
            }
            Err(OpenError::Cancelled) => {
                report.cancelled = true;
                report.added.clear();
                return report;
            }
            Err(e) => report.refused.push((path.clone(), e.to_string())),
        }
    }
    progress(1.0);
    if !report.added.is_empty() {
        report.edit = Some(Edit { state: cur, label: APPEND.to_string(), change: Change::LengthFrom { frame: base.len_frames() } });
    }
    report
}

/// Stream `path` into `cur`'s store and join it on at the end. Returns the new state and the
/// file's length in frames.
fn append_one(
    cur: &DocState,
    path: &Path,
    free_space: Option<FreeSpaceFn>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<(DocState, u64), OpenError> {
    let file = io::open_into(path, cur.store(), cur.sample_rate(), free_space, progress, cancel)?;
    let frames = file.len_frames();
    let clip = edit::copy(&file, 0..frames, ChannelMask::ALL).map_err(|e| OpenError::Io(e.to_string()))?;
    // The channel counts are checked by the paste: a stereo document and a six-channel file do
    // not map onto each other.
    let joined =
        edit::paste(cur, PasteAt::Insert(cur.len_frames()), &clip, ChannelMask::ALL, Crossfades::NONE).map_err(|e| OpenError::Io(e.to_string()))?;
    Ok((joined.state, frames))
}

/// A new document holding `paths` back to back. The first file that opens sets the sample rate,
/// the channel count and the sample type; the later ones are appended as by [`append_files`].
pub fn new_from_files(opts: &OpenOptions, paths: &[PathBuf], progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> NewReport {
    let mut report = NewReport::default();
    let n = paths.len().max(1) as f64;
    for (i, path) in paths.iter().enumerate() {
        let mut each = |p: f64| progress((i as f64 + p) / n);
        match io::open(path, opts, &mut each, cancel) {
            Ok(base) => {
                report.added.push((path.clone(), base.len_frames()));
                let mut rest = |p: f64| progress((i as f64 + 1.0 + p * (paths.len() - i - 1) as f64) / n);
                let tail = append_files(&base, &paths[i + 1..], opts.free_space, &mut rest, cancel);
                if tail.cancelled {
                    report.cancelled = true;
                    report.added.clear();
                    return report;
                }
                report.added.extend(tail.added);
                report.refused.extend(tail.refused);
                report.state = Some(tail.edit.map_or(base, |e| e.state));
                progress(1.0);
                return report;
            }
            Err(OpenError::Cancelled) => {
                report.cancelled = true;
                return report;
            }
            Err(e) => report.refused.push((path.clone(), e.to_string())),
        }
    }
    report
}
