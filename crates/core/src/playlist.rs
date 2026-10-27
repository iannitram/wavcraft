//! The playlist (A188-A190): marker ranges arranged to play in any order, each with a loop count.
//!
//! Items refer to markers by id, so a rename or a move follows through; [`Playlist::sequence`]
//! turns the list into the frame ranges to play, in order, each repeated for its loops. A marker
//! that is gone, or a point marker (no length), contributes nothing. The playlist belongs to the
//! open session of the window (it is not part of the undo history and is not yet stored in the WAV).

use std::ops::Range;

use crate::markers::Markers;

/// The most times one item may repeat.
pub const MAX_LOOPS: u32 = 999;

/// One entry: a marker (by id) and how many times to play its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub marker: u64,
    pub loops: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Playlist {
    items: Vec<Item>,
}

impl Playlist {
    pub fn new() -> Playlist {
        Playlist::default()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Append `marker` with `loops` repeats (clamped to 1..=[`MAX_LOOPS`]).
    pub fn add(&mut self, marker: u64, loops: u32) {
        self.items.push(Item { marker, loops: loops.clamp(1, MAX_LOOPS) });
    }

    /// Remove item `index`. Whether there was one.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    /// Move item `from` so that it ends up at position `to`. False if either is out of range.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() {
            return false;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        true
    }

    /// Set item `index`'s loop count (clamped to 1..=[`MAX_LOOPS`]). False if there is no such item.
    pub fn set_loops(&mut self, index: usize, loops: u32) -> bool {
        match self.items.get_mut(index) {
            Some(item) => {
                item.loops = loops.clamp(1, MAX_LOOPS);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The frame ranges to play, in order: each item's range once per loop. Ranges are clipped
    /// to `frames`; those left empty are skipped.
    pub fn sequence(&self, markers: &Markers, frames: u64) -> Vec<Range<u64>> {
        let mut out = Vec::new();
        for item in &self.items {
            let Some(m) = markers.iter().find(|m| m.id == item.marker) else { continue };
            let range = m.start.min(frames)..m.end.min(frames);
            if range.start < range.end {
                out.extend(std::iter::repeat_n(range, item.loops as usize));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markers() -> (Markers, Vec<u64>) {
        let mut m = Markers::new();
        for (name, r) in [("a", 100..200), ("b", 300..450), ("p", 500..500), ("c", 600..700)] {
            m = m.with_added(r, Some(name)).expect("added");
        }
        let ids = m.iter().map(|x| x.id).collect();
        (m, ids)
    }

    #[test]
    fn plays_each_range_in_order_with_its_loops() {
        let (m, id) = markers();
        let mut p = Playlist::new();
        p.add(id[1], 2);
        p.add(id[0], 1);
        p.add(id[3], 3);
        assert_eq!(p.sequence(&m, 10_000), vec![300..450, 300..450, 100..200, 600..700, 600..700, 600..700]);
    }

    #[test]
    fn reordering_changes_the_sequence() {
        let (m, id) = markers();
        let mut p = Playlist::new();
        for i in [0, 1, 3] {
            p.add(id[i], 1);
        }
        assert!(p.move_item(2, 0));
        assert_eq!(p.sequence(&m, 10_000), vec![600..700, 100..200, 300..450]);
        assert!(p.move_item(0, 1));
        assert_eq!(p.sequence(&m, 10_000), vec![100..200, 600..700, 300..450]);
        assert!(!p.move_item(0, 3));
        assert!(!p.move_item(3, 0));
    }

    #[test]
    fn loop_counts_are_clamped_and_settable() {
        let (_, id) = markers();
        let mut p = Playlist::new();
        p.add(id[0], 0);
        assert_eq!(p.items()[0].loops, 1);
        assert!(p.set_loops(0, 5000));
        assert_eq!(p.items()[0].loops, MAX_LOOPS);
        assert!(p.set_loops(0, 4));
        assert_eq!(p.items()[0].loops, 4);
        assert!(!p.set_loops(1, 2));
    }

    #[test]
    fn points_missing_markers_and_empty_clips_play_nothing() {
        let (m, id) = markers();
        let mut p = Playlist::new();
        p.add(id[2], 2); // a point
        p.add(9999, 1); // no such marker
        p.add(id[3], 1);
        assert_eq!(p.sequence(&m, 10_000), vec![600..700]);
        // Past the end of a shortened file the range is clipped, then dropped.
        assert_eq!(p.sequence(&m, 650), vec![600..650]);
        assert!(p.sequence(&m, 600).is_empty());
    }

    #[test]
    fn the_same_marker_may_appear_twice_and_items_can_be_removed() {
        let (m, id) = markers();
        let mut p = Playlist::new();
        p.add(id[0], 1);
        p.add(id[1], 1);
        p.add(id[0], 1);
        assert_eq!(p.sequence(&m, 10_000), vec![100..200, 300..450, 100..200]);
        assert!(p.remove(1));
        assert!(!p.remove(5));
        assert_eq!(p.sequence(&m, 10_000), vec![100..200, 100..200]);
    }
}
