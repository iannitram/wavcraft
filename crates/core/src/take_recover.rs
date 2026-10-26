//! J3 (a crash keeps the take): finding and repairing, on disk, takes left by a killed app.
//!
//! The next start lists the takes in the recordings folder whose header is out of date
//! ([`takes_needing_repair`]) so the app can offer them, and repairs one in place
//! ([`repair_take_file`]): only the 44 header bytes are rewritten, then the file is synced. No
//! sample is moved and no file is deleted, made or truncated.

// Stub for wav-c-116-b: the builder replaces the bodies and deletes these two lines.
#![allow(clippy::todo)]

use std::io;
use std::path::{Path, PathBuf};

use crate::take_repair::HeaderRepair;

/// Repair the take at `path` in place and say what was found. A file shorter than a header is
/// `NotATake`. A missing file is an error.
pub fn repair_take_file(path: &Path) -> io::Result<HeaderRepair> {
    let _ = path;
    todo!("wav-c-116-b: repair_take_file")
}

/// The takes directly in `dir` (`Take *.wav`) whose header needs a repair, sorted by path.
/// Changes no file. A missing folder lists nothing.
pub fn takes_needing_repair(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let _ = dir;
    todo!("wav-c-116-b: takes_needing_repair")
}
