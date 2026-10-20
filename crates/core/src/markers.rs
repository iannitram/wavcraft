//! Markers (A170): point markers and range markers with a name and a position in frames. A
//! [`Markers`] list is part of the document state ([`crate::DocState`]), so undo and redo bring
//! it back with the audio. Saving markers into the WAV file and moving them with edits come
//! later; for now an edit only keeps them inside the new length.

use std::ops::Range;

/// One marker. A point marker has `end == start`; a range marker has `end > start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// Stable within one document: never reused, kept by undo and redo.
    pub id: u64,
    pub name: String,
    /// First frame (the position of a point marker).
    pub start: u64,
    /// Frame just past the range (equal to `start` for a point marker).
    pub end: u64,
}

impl Marker {
    pub fn is_point(&self) -> bool {
        self.end <= self.start
    }

    pub fn range(&self) -> Range<u64> {
        self.start..self.end.max(self.start)
    }
}

/// The markers of one document state, ordered by start (then id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Markers {
    list: Vec<Marker>,
    next_id: u64,
}

impl Markers {
    pub fn new() -> Markers {
        Markers::default()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Marker> {
        self.list.iter()
    }

    pub fn as_slice(&self) -> &[Marker] {
        &self.list
    }

    /// The default name of the next marker: "Marker 01", "Marker 02", … (the first not taken).
    pub fn default_name(&self) -> String {
        (1u64..).map(|n| format!("Marker {n:02}")).find(|name| self.list.iter().all(|m| &m.name != name)).unwrap_or_default()
    }

    /// A copy with a marker added over `range` (a point when empty), named `name` or by default.
    /// None when a marker with exactly this position and extent exists already.
    pub fn with_added(&self, range: Range<u64>, name: Option<&str>) -> Option<Markers> {
        let (start, end) = (range.start, range.end.max(range.start));
        if self.list.iter().any(|m| m.start == start && m.end.max(m.start) == end) {
            return None;
        }
        let mut out = self.clone();
        let name = name.map_or_else(|| self.default_name(), str::to_string);
        out.list.push(Marker { id: out.next_id, name, start, end });
        out.next_id += 1;
        out.list.sort_by_key(|m| (m.start, m.id));
        Some(out)
    }

    /// The start of the first marker strictly after `frame`.
    pub fn next_after(&self, frame: u64) -> Option<u64> {
        self.list.iter().map(|m| m.start).find(|&s| s > frame)
    }

    /// The start of the last marker strictly before `frame`.
    pub fn prev_before(&self, frame: u64) -> Option<u64> {
        self.list.iter().map(|m| m.start).rev().find(|&s| s < frame)
    }

    /// A copy with every position held to `len` frames (after an edit shortened the document).
    /// None when nothing needs it.
    pub fn clamped(&self, len: u64) -> Option<Markers> {
        if self.list.iter().all(|m| m.start <= len && m.end <= len) {
            return None;
        }
        let mut out = self.clone();
        for m in &mut out.list {
            m.start = m.start.min(len);
            m.end = m.end.min(len);
        }
        out.list.sort_by_key(|m| (m.start, m.id));
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_stay_ordered_and_navigation_skips_the_frame_itself() {
        let m = Markers::new().with_added(500..500, None).unwrap().with_added(100..200, None).unwrap().with_added(900..900, Some("End")).unwrap();
        let starts: Vec<u64> = m.iter().map(|x| x.start).collect();
        assert_eq!(starts, [100, 500, 900]);
        assert_eq!(m.as_slice()[0].name, "Marker 02");
        assert_eq!(m.next_after(100), Some(500));
        assert_eq!(m.next_after(900), None);
        assert_eq!(m.prev_before(500), Some(100));
        assert_eq!(m.prev_before(100), None);
        assert!(m.with_added(500..500, None).is_none(), "the same place twice");
        assert!(m.as_slice()[0].range() == (100..200) && !m.as_slice()[0].is_point() && m.as_slice()[1].is_point());
    }

    #[test]
    fn clamping_holds_positions_inside_the_length() {
        let m = Markers::new().with_added(10..10, None).unwrap().with_added(50..90, None).unwrap();
        assert!(m.clamped(100).is_none());
        let c = m.clamped(60).unwrap();
        assert_eq!(c.as_slice()[1].range(), 50..60);
    }
}
