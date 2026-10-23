//! The "recent sessions" list (roadmap C080): an ordered, newest-first list of multitrack session
//! file paths (`.wcsession`, see [`crate::session_file`]) with a maximum length. A session goes to
//! the front when it is opened or saved ([`RecentSessions::promote`]); the oldest fall off past the
//! maximum. Entries whose file is gone can be pruned, and the list round-trips as a JSON array of
//! strings. Pure logic: no disk, no settings file, no menu.
//!
//! Paths are compared by their [`normalise_path`] form (so `C:\m\a.wcsession` and `C:/m/a.wcsession/`
//! are one entry), case-sensitively, like the Open Recent file list. The path is kept as given
//! (trimmed), so what the menu shows is what the user opened.

use serde_json::Value;

/// How many sessions the list keeps unless another maximum is asked for.
pub const DEFAULT_MAX_RECENT_SESSIONS: usize = 10;

/// The comparison form of `path`: trimmed, every `\` turned into `/`, trailing `/` removed
/// (but a lone `/` stays). Case is kept.
pub fn normalise_path(path: &str) -> String {
    let t = path.trim().replace('\\', "/");
    let mut s = t;
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// The recent sessions, newest first, at most `max` of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSessions {
    max: usize,
    paths: Vec<String>,
}

impl Default for RecentSessions {
    fn default() -> RecentSessions {
        RecentSessions::new()
    }
}

impl RecentSessions {
    /// An empty list with the default maximum, [`DEFAULT_MAX_RECENT_SESSIONS`].
    pub fn new() -> RecentSessions {
        RecentSessions::with_max(DEFAULT_MAX_RECENT_SESSIONS)
    }

    /// An empty list that keeps at most `max` sessions (0 keeps none).
    pub fn with_max(max: usize) -> RecentSessions {
        RecentSessions { max, paths: Vec::new() }
    }

    /// The most sessions the list keeps.
    pub fn max(&self) -> usize {
        self.max
    }

    /// The sessions, newest first.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Change the maximum; a smaller one drops the oldest entries now.
    pub fn set_max(&mut self, max: usize) {
        self.max = max;
        self.paths.truncate(max);
    }

    /// `path` was opened or saved: it goes to the front (once; an entry with the same normalised
    /// path is replaced), and the oldest fall off past the maximum. The path is stored trimmed.
    /// A blank path, or one with a line break in it, is not listed. Returns true when the path is
    /// in the list afterwards (false for a rejected path, and always false when the maximum is 0).
    pub fn promote(&mut self, path: &str) -> bool {
        let p = path.trim();
        if p.is_empty() || p.contains(['\n', '\r']) {
            return false;
        }
        let key = normalise_path(p);
        self.paths.retain(|q| normalise_path(q) != key);
        self.paths.insert(0, p.to_owned());
        self.paths.truncate(self.max);
        self.max > 0
    }

    /// Take the entry with the same normalised path as `path` out of the list. Returns whether
    /// there was one. The others keep their order.
    pub fn remove(&mut self, path: &str) -> bool {
        let key = normalise_path(path);
        let old_len = self.paths.len();
        self.paths.retain(|q| normalise_path(q) != key);
        self.paths.len() < old_len
    }

    /// Empty the list (the maximum stays).
    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// Drop every entry for which `exists` (given the stored path) says the file is gone. The
    /// others keep their order. Returns how many were dropped.
    pub fn prune(&mut self, exists: impl Fn(&str) -> bool) -> usize {
        let old_len = self.paths.len();
        self.paths.retain(|q| exists(q));
        old_len - self.paths.len()
    }

    /// The list as a JSON array of strings, newest first (`["b","a"]`; empty is `[]`).
    pub fn to_json(&self) -> String {
        Value::Array(self.paths.iter().map(|p| Value::String(p.clone())).collect()).to_string()
    }

    /// A list with maximum `max` read from `text` (as [`RecentSessions::to_json`] writes it).
    /// Never fails: text that is not a JSON array gives an empty list, and array elements that are
    /// not usable paths (not a string, blank, a line break, a repeat) are skipped. The first
    /// `max` usable ones are kept, in order.
    pub fn from_json(text: &str, max: usize) -> RecentSessions {
        let mut r = RecentSessions::with_max(max);
        let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(text) else {
            return r;
        };

        for elem in arr {
            if let Some(s) = elem.as_str() {
                let p = s.trim();
                if p.is_empty() || p.contains(['\n', '\r']) {
                    continue;
                }

                // Check if a path with the same normalised form is already in the list
                if r.paths.iter().any(|existing| normalise_path(existing) == normalise_path(p)) {
                    continue;
                }

                // Only add if max > 0, otherwise we skip adding anything (but still process all elements)
                if max > 0 {
                    r.paths.push(p.to_owned());
                    if r.paths.len() >= max {
                        break;
                    }
                }
            }
        }

        r
    }
}
