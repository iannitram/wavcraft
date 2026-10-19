//! Repeat collapse and the ring of recent lines.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

/// Lines kept in memory for crash files and failure reports.
pub const RING_LINES: usize = 200;

/// Tests that flood the ring, or need a line of theirs to stay in it, hold this so they never
/// run at the same time (the ring is one per process, and tests run in parallel).
#[cfg(test)]
pub(crate) static RING_TEST_LOCK: Mutex<()> = Mutex::new(());
/// A line repeated this many times gets a "so far" note, so a flood still shows it is going on.
pub const REPEAT_NOTE_EVERY: u64 = 10_000;

/// What to do with a record, decided by [`Collapser::observe`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// Write this note first: (times repeated, its level, its target, "so far" when still going).
    pub note: Option<(u64, log::Level, String, bool)>,
    /// The record repeats the previous one: do not write it.
    pub swallow: bool,
}

/// Remembers the previous record so a repeat is written once, then counted.
#[derive(Default)]
pub struct Collapser {
    last: Option<(log::Level, String, String)>,
    repeats: u64,
}

impl Collapser {
    pub fn observe(&mut self, level: log::Level, target: &str, message: &str) -> Action {
        if let Some((l, t, m)) = &self.last
            && *l == level
            && t == target
            && m == message
        {
            self.repeats = self.repeats.saturating_add(1);
            let note = self.repeats.is_multiple_of(REPEAT_NOTE_EVERY).then(|| (self.repeats, level, target.to_owned(), true));
            return Action { note, swallow: true };
        }
        let note = self.pending();
        self.last = Some((level, target.to_owned(), message.to_owned()));
        Action { note, swallow: false }
    }

    /// The note owed for repeats not yet reported (at a new line, or when flushing).
    pub fn pending(&mut self) -> Option<(u64, log::Level, String, bool)> {
        let n = std::mem::take(&mut self.repeats);
        let (l, t, _) = self.last.as_ref()?;
        (!n.is_multiple_of(REPEAT_NOTE_EVERY)).then(|| (n, *l, t.clone(), false))
    }
}

/// The text of a repeat note.
pub fn note_text(times: u64, so_far: bool) -> String {
    if so_far { format!("previous line repeated {times} times so far") } else { format!("previous line repeated {times} times") }
}

static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Keep `line` (already formatted) among the last [`RING_LINES`].
pub fn remember(line: &str) {
    let mut ring = RING.lock().unwrap_or_else(PoisonError::into_inner);
    if ring.len() == RING_LINES {
        ring.pop_front();
    }
    ring.push_back(line.strip_suffix('\n').unwrap_or(line).to_owned());
}

/// The last `n` lines logged in this process (oldest first).
pub fn recent(n: usize) -> Vec<String> {
    let ring = RING.lock().unwrap_or_else(PoisonError::into_inner);
    ring.iter().skip(ring.len().saturating_sub(n)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level::{Info, Warn};

    #[test]
    fn a_repeat_is_written_once_then_counted() {
        let mut c = Collapser::default();
        assert_eq!(c.observe(Warn, "t", "a"), Action { note: None, swallow: false });
        assert!(c.observe(Warn, "t", "a").swallow);
        assert!(c.observe(Warn, "t", "a").swallow);
        assert_eq!(c.observe(Warn, "t", "b"), Action { note: Some((2, Warn, "t".into(), false)), swallow: false });
        // same text at another level or target is not a repeat
        assert!(!c.observe(Info, "t", "b").swallow);
        assert!(!c.observe(Info, "u", "b").swallow);
        assert_eq!(c.pending(), None);
    }

    #[test]
    fn a_flood_notes_its_count_as_it_goes() {
        let mut c = Collapser::default();
        c.observe(Warn, "t", "x");
        let notes: Vec<_> = (0..25_000).filter_map(|_| c.observe(Warn, "t", "x").note).collect();
        assert_eq!(notes, vec![(10_000, Warn, "t".into(), true), (20_000, Warn, "t".into(), true)]);
        assert_eq!(c.pending(), Some((25_000, Warn, "t".into(), false)));
        assert_eq!(note_text(3, false), "previous line repeated 3 times");
    }

    #[test]
    fn the_ring_keeps_the_newest_lines() {
        let _turn = RING_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        for i in 0..(RING_LINES + 5) {
            remember(&format!("ring-test {i}\n"));
        }
        // other tests log in parallel into the same ring: look for ours among the newest
        let all = recent(10_000);
        assert_eq!(all.len(), RING_LINES);
        assert!(all.iter().any(|l| l.ends_with(&format!("ring-test {}", RING_LINES + 4))));
        assert!(!all.iter().any(|l| l.ends_with("ring-test 0")));
        assert_eq!(recent(3).len(), 3);
    }
}
