//! Undo history (task 04): a list of document states with a cursor.
//!
//! Undo is keeping the old [`DocState`]: each step holds an `Arc` of its state, and undo, redo and
//! [`History::go_to`] only move the cursor. The states share blocks, so a step costs RAM only for
//! its piece lists; its **disk** cost is the blocks no other kept state uses.
//!
//! **Disk cap.** The bytes the history holds beyond the current state are the distinct blocks
//! (the zero block aside) that kept steps use and the current state doesn't. When a push takes
//! that over the cap, the oldest steps are dropped until it fits, never the current one, and
//! [`History::push`] says how many went. Dropping a step releases its blocks, and the store
//! deletes packs that empty. To keep a push cheap with hundreds of steps behind it, the history
//! counts, per block, how many kept steps use it, and the total of every block counted: the bytes
//! held beyond the current state are that total less the current state's own bytes.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use super::DocState;
use super::edit::{Change, Edit};
use crate::space::{FreeSpaceFn, format_bytes, free_space};
use crate::store::BlockId;

/// The largest default disk cap: 100 GB.
pub const MAX_DEFAULT_CAP: u64 = 100_000_000_000;
/// A job may not leave less than this free on the session folder's disk: 2 GB.
pub const FREE_RESERVE: u64 = 2_000_000_000;
/// The first step's label.
pub const OPEN: &str = "Open";

/// One state in the history.
#[derive(Debug, Clone)]
pub struct Step {
    label: String,
    state: Arc<DocState>,
    /// Bytes of blocks this step brought into the history (new audio it made).
    made_bytes: u64,
    /// What the step changed from the one before it (for the views, task 05).
    change: Change,
}

impl Step {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn state(&self) -> &Arc<DocState> {
        &self.state
    }

    pub fn made_bytes(&self) -> u64 {
        self.made_bytes
    }

    pub fn change(&self) -> &Change {
        &self.change
    }
}

/// A job would leave too little disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoSpace {
    pub needed: u64,
    pub free: u64,
}

impl fmt::Display for NoSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "needs {}, {} free (WavCraft keeps {} free on the session folder's disk)",
            format_bytes(self.needed),
            format_bytes(self.free),
            format_bytes(FREE_RESERVE)
        )
    }
}

impl std::error::Error for NoSpace {}

/// The undo history of one open document.
pub struct History {
    steps: Vec<Step>,
    cursor: usize,
    cap_disk_bytes: u64,
    /// Per block: how many kept steps use it, and its size.
    counts: HashMap<BlockId, (u32, u64)>,
    /// Bytes of every distinct block a kept step uses.
    held_bytes: u64,
    /// Bytes of the current state's distinct blocks.
    current_bytes: u64,
    /// Steps dropped by the cap since the history began.
    dropped: usize,
    free_space: FreeSpaceFn,
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History").field("steps", &self.steps.len()).field("cursor", &self.cursor).field("cap_disk_bytes", &self.cap_disk_bytes).finish()
    }
}

/// The distinct blocks of a state with their sizes, the zero block left out.
fn blocks(state: &DocState) -> HashMap<BlockId, u64> {
    let mut seen = HashMap::new();
    for ch in state.channels() {
        for p in ch.pieces() {
            let b = p.block().block();
            if !b.is_zero() {
                seen.insert(b.id, b.disk_bytes());
            }
        }
    }
    seen
}

impl History {
    /// A history whose first step ("Open") is `initial`.
    pub fn new(initial: DocState, cap_disk_bytes: u64) -> History {
        let mut h = History {
            steps: Vec::new(),
            cursor: 0,
            cap_disk_bytes,
            counts: HashMap::new(),
            held_bytes: 0,
            current_bytes: 0,
            dropped: 0,
            free_space,
        };
        let state = Arc::new(initial);
        let made = h.add_counts(&state);
        h.current_bytes = state.disk_bytes_referenced();
        h.steps.push(Step { label: OPEN.to_string(), state, made_bytes: made, change: Change::None });
        h
    }

    /// Use another free-space query (tests).
    pub fn with_free_space(mut self, f: FreeSpaceFn) -> History {
        self.free_space = f;
        self
    }

    /// The default cap for a session folder: the smaller of 100 GB and half the free space on
    /// its disk (100 GB when the free space is unknown).
    pub fn default_cap(session_dir: &Path) -> u64 {
        free_space(session_dir).map_or(MAX_DEFAULT_CAP, |f| (f / 2).min(MAX_DEFAULT_CAP))
    }

    pub fn current(&self) -> &Arc<DocState> {
        // There is always at least one step and the cursor is on one.
        &self.steps[self.cursor].state
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn cap_disk_bytes(&self) -> u64 {
        self.cap_disk_bytes
    }

    /// A new cap; takes effect at the next push.
    pub fn set_cap_disk_bytes(&mut self, cap: u64) {
        self.cap_disk_bytes = cap;
    }

    /// Bytes the kept steps hold beyond the current state.
    pub fn held_bytes(&self) -> u64 {
        self.held_bytes - self.current_bytes
    }

    /// Steps dropped by the cap since the history began.
    pub fn dropped_total(&self) -> usize {
        self.dropped
    }

    /// Record an edit (nothing for a no-op edit). Returns how many old steps the cap dropped.
    pub fn push_edit(&mut self, edit: Edit) -> usize {
        if edit.is_noop() {
            return 0;
        }
        self.push_with_change(edit.label, edit.state, edit.change)
    }

    /// Make `state` the current step, after the cursor; any redo steps go. A state that is the
    /// current one (it shares every channel) records nothing. Returns how many old steps the cap
    /// dropped.
    pub fn push(&mut self, label: impl Into<String>, state: DocState) -> usize {
        self.push_with_change(label.into(), state, Change::LengthFrom { frame: 0 })
    }

    fn push_with_change(&mut self, label: String, state: DocState, change: Change) -> usize {
        let cur = self.current();
        if cur.channel_count() == state.channel_count() && cur.channels().iter().zip(state.channels()).all(|(a, b)| Arc::ptr_eq(a, b)) {
            return 0;
        }
        while self.steps.len() > self.cursor + 1 {
            if let Some(step) = self.steps.pop() {
                self.remove_counts(&step.state);
            }
        }
        let state = Arc::new(state);
        let made = self.add_counts(&state);
        self.current_bytes = state.disk_bytes_referenced();
        self.steps.push(Step { label, state, made_bytes: made, change });
        self.cursor = self.steps.len() - 1;
        let mut dropped = 0;
        while self.held_bytes() > self.cap_disk_bytes && self.cursor > 0 {
            let step = self.steps.remove(0);
            self.remove_counts(&step.state);
            drop(step);
            self.cursor -= 1;
            dropped += 1;
        }
        self.dropped += dropped;
        dropped
    }

    fn add_counts(&mut self, state: &DocState) -> u64 {
        let mut made = 0;
        for (id, bytes) in blocks(state) {
            let e = self.counts.entry(id).or_insert((0, bytes));
            if e.0 == 0 {
                self.held_bytes += bytes;
                made += bytes;
            }
            e.0 += 1;
        }
        made
    }

    fn remove_counts(&mut self, state: &DocState) {
        for id in blocks(state).into_keys() {
            if let Some(e) = self.counts.get_mut(&id) {
                e.0 -= 1;
                if e.0 == 0 {
                    self.held_bytes -= e.1;
                    self.counts.remove(&id);
                }
            }
        }
    }

    fn move_to(&mut self, index: usize) -> Option<Arc<DocState>> {
        if index >= self.steps.len() {
            return None;
        }
        if index != self.cursor {
            self.cursor = index;
            self.current_bytes = self.current().disk_bytes_referenced();
        }
        Some(self.current().clone())
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.steps.len()
    }

    /// Step back. The new current state, or `None` at the first step.
    pub fn undo(&mut self) -> Option<Arc<DocState>> {
        if !self.can_undo() {
            return None;
        }
        self.move_to(self.cursor - 1)
    }

    /// Step forward. The new current state, or `None` at the last step.
    pub fn redo(&mut self) -> Option<Arc<DocState>> {
        if !self.can_redo() {
            return None;
        }
        self.move_to(self.cursor + 1)
    }

    /// Jump to step `index` (the History panel). `None` if there is no such step.
    pub fn go_to(&mut self, index: usize) -> Option<Arc<DocState>> {
        self.move_to(index)
    }

    /// The label of the step Undo would undo ("Delete", for "Undo Delete").
    pub fn undo_label(&self) -> Option<&str> {
        self.can_undo().then(|| self.steps[self.cursor].label.as_str())
    }

    /// The label of the step Redo would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.steps.get(self.cursor + 1).map(|s| s.label.as_str())
    }

    /// Audition's Clear History: keep only the current state.
    pub fn clear(&mut self) {
        let keep = self.steps.swap_remove(self.cursor);
        for step in std::mem::take(&mut self.steps) {
            self.remove_counts(&step.state);
        }
        self.steps.push(keep);
        self.cursor = 0;
    }

    /// Whether a job that writes `bytes` may start: it must leave at least [`FREE_RESERVE`] free
    /// on the session folder's disk. Allowed when the free space is unknown.
    pub fn can_afford(&self, bytes: u64) -> Result<(), NoSpace> {
        let Some(free) = (self.free_space)(self.current().store().dir()) else { return Ok(()) };
        if free.saturating_sub(bytes) < FREE_RESERVE || bytes > free {
            return Err(NoSpace { needed: bytes, free });
        }
        Ok(())
    }

    /// Every block id some kept step uses (tests and diagnostics).
    pub fn held_blocks(&self) -> HashSet<BlockId> {
        self.counts.keys().copied().collect()
    }
}
