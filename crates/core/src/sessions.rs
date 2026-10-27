//! Disk used by sessions, and "Clean up old sessions" (A281, f70).
//!
//! Every rule of the start-up sweep (#8) holds here, because this is built on the same probe
//! ([`stale_candidates`](crate::store)): a session is removed only when
//! - it was made by WavCraft on this machine and no running program holds its lock (a live
//!   session is never touched, nor is one this machine cannot vouch for),
//! - it holds **no journal**, not even a half-written one (a session that can be recovered is the
//!   only copy of unsaved work; only the owner, asked in the recovery window, discards it),
//! - nothing in it has changed for the number of days asked (a clock that cannot be read keeps
//!   the session), and
//! - nothing about it is uncertain; the journal check is made again just before removal.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::store::{StaleSession, has_journal, stale_candidates};

/// What lies in a session folder root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// Session folders (16 hex digits) found.
    pub sessions: usize,
    /// Bytes they hold.
    pub bytes: u64,
    /// Sessions a recovery is offered for (they hold a journal): never removed by a clean-up.
    pub recoverable: usize,
    /// Sessions left by a run that ended, with nothing to recover.
    pub stale: usize,
    /// Of those, how many (and how many bytes) a clean-up for the asked age would remove.
    pub removable: usize,
    pub removable_bytes: u64,
}

/// What a clean-up did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleanup {
    pub removed: usize,
    pub freed: u64,
}

fn is_session_name(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |d| d.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum())
}

/// How long since anything in the session folder changed: from the newest modification time of
/// the folder and its entries. `None` when that can't be told (then the session is kept).
pub fn idle_for(dir: &Path, now: SystemTime) -> Option<Duration> {
    let mut newest = std::fs::metadata(dir).ok()?.modified().ok()?;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    }
    // A time in the future (clock set back) counts as no age at all.
    Some(now.duration_since(newest).unwrap_or(Duration::ZERO))
}

fn days(d: u32) -> Duration {
    Duration::from_secs(u64::from(d) * 86_400)
}

/// The sessions a clean-up for `older_than_days` removes. Pure listing, nothing is touched.
pub fn removable(root: &Path, older_than_days: u32, now: SystemTime) -> Vec<StaleSession> {
    stale_candidates(root)
        .into_iter()
        .filter(|(_, journaled)| !journaled)
        .map(|(s, _)| s)
        .filter(|s| idle_for(&s.dir, now).is_some_and(|age| age >= days(older_than_days)))
        .collect()
}

/// What `root` holds and what a clean-up for `older_than_days` would remove.
pub fn usage(root: &Path, older_than_days: u32, now: SystemTime) -> Usage {
    let mut u = Usage::default();
    let Ok(entries) = std::fs::read_dir(root) else { return u };
    for e in entries.flatten() {
        if is_session_name(&e.file_name().to_string_lossy()) && e.path().is_dir() {
            u.sessions += 1;
            u.bytes += dir_bytes(&e.path());
        }
    }
    for (_, journaled) in stale_candidates(root) {
        if journaled {
            u.recoverable += 1;
        } else {
            u.stale += 1;
        }
    }
    let r = removable(root, older_than_days, now);
    u.removable = r.len();
    u.removable_bytes = r.iter().map(|s| s.bytes).sum();
    u
}

/// Remove the [`removable`] sessions under `root`, logging each. The owner has confirmed before
/// this is called; the safety rules in the module notes are checked again here, not trusted.
pub fn cleanup(root: &Path, older_than_days: u32, now: SystemTime) -> Cleanup {
    let mut out = Cleanup::default();
    for s in removable(root, older_than_days, now) {
        let dir: PathBuf = s.dir.clone();
        // A journal may have appeared since the list was made: then it is not ours to remove.
        if has_journal(&dir) {
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                out.removed += 1;
                out.freed += s.bytes;
                log::info!(target: "wavcraft", "decision: sessions: clean-up removed an old session folder (pid {}, idle {} days or more): {} MB in {}", s.pid, older_than_days, s.bytes / 1_000_000, dir.display());
            }
            Err(e) => log::warn!(target: "wavcraft", "decision: sessions: clean-up could not remove {}: {e}", dir.display()),
        }
    }
    out
}
