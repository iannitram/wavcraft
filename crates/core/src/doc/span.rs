//! What an edit replaced, found by comparing two states, and where a play position goes across
//! it (edit while playing, f48).
//!
//! The comparison walks the piece tables of the two states from the front and from the back
//! while the pieces are the same part of the same block, so it costs the number of pieces, not
//! the number of frames, and it works for any change between two states: an edit, an undo, a
//! jump in the History panel.

use super::{Channel, DocState};
use std::sync::Arc;

/// The old frames `[at, at + removed)` became the new frames `[at, at + inserted)`; everything
/// before `at` and everything after is the same audio, shifted by `inserted - removed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditSpan {
    pub at: u64,
    pub removed: u64,
    pub inserted: u64,
}

impl EditSpan {
    /// The smallest span that turns `old` into `new`, over all channels (a change in any
    /// channel counts). If the channel counts differ, the whole of both.
    pub fn between(old: &DocState, new: &DocState) -> EditSpan {
        let (old_len, new_len) = (old.len_frames(), new.len_frames());
        if old.channel_count() != new.channel_count() {
            return EditSpan { at: 0, removed: old_len, inserted: new_len };
        }
        let shortest = old_len.min(new_len);
        let mut prefix = shortest;
        for (a, b) in old.channels().iter().zip(new.channels()) {
            prefix = prefix.min(common_prefix(a, b));
        }
        let mut suffix = shortest - prefix;
        for (a, b) in old.channels().iter().zip(new.channels()) {
            suffix = suffix.min(common_suffix(a, b));
        }
        EditSpan { at: prefix, removed: old_len - prefix - suffix, inserted: new_len - prefix - suffix }
    }

    /// Where a play position (in old frames, with a fraction) is in the new document: unchanged
    /// before the span, shifted by the length change after it, and inside a replaced range at
    /// the same offset into the new audio or its end. Inside a deleted range, the cut point.
    pub fn map_position(&self, pos: f64) -> f64 {
        if !pos.is_finite() {
            return if pos.is_nan() { 0.0 } else { pos };
        }
        let (at, removed, inserted) = (self.at as f64, self.removed as f64, self.inserted as f64);
        if pos <= at {
            pos.max(0.0)
        } else if pos >= at + removed {
            pos - removed + inserted
        } else {
            at + (pos - at).min(inserted)
        }
    }
}

/// Frames at the start where `a` and `b` are the same parts of the same blocks.
fn common_prefix(a: &Channel, b: &Channel) -> u64 {
    let (pa, pb) = (a.pieces(), b.pieces());
    let (mut i, mut j, mut used_a, mut used_b, mut n) = (0, 0, 0u32, 0u32, 0u64);
    while let (Some(x), Some(y)) = (pa.get(i), pb.get(j)) {
        if !Arc::ptr_eq(x.block(), y.block()) || x.offset() + used_a != y.offset() + used_b {
            break;
        }
        let m = (x.len() - used_a).min(y.len() - used_b);
        n += u64::from(m);
        used_a += m;
        used_b += m;
        if used_a == x.len() {
            i += 1;
            used_a = 0;
        }
        if used_b == y.len() {
            j += 1;
            used_b = 0;
        }
    }
    n
}

/// Frames at the end where `a` and `b` are the same parts of the same blocks.
fn common_suffix(a: &Channel, b: &Channel) -> u64 {
    let (pa, pb) = (a.pieces(), b.pieces());
    let (mut i, mut j, mut used_a, mut used_b, mut n) = (pa.len(), pb.len(), 0u32, 0u32, 0u64);
    while i > 0 && j > 0 {
        let (x, y) = (&pa[i - 1], &pb[j - 1]);
        if !Arc::ptr_eq(x.block(), y.block()) || x.offset() + x.len() - used_a != y.offset() + y.len() - used_b {
            break;
        }
        let m = (x.len() - used_a).min(y.len() - used_b);
        n += u64::from(m);
        used_a += m;
        used_b += m;
        if used_a == x.len() {
            i -= 1;
            used_a = 0;
        }
        if used_b == y.len() {
            j -= 1;
            used_b = 0;
        }
    }
    n
}
