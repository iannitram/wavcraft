//! Card wav-c-86-a (#86, v1 item I4): `History::restore`, the undo history rebuilt from the steps
//! a crash-recovery journal recorded. Locked tests: every expected value is written out here.
//!
//! The three states share a store: block A holds 4 frames (16 bytes on disk), block B 3 frames
//! (12 bytes). "Open" is A whole, "Delete Audio" is A's first 2 frames, "Insert Silence" is B.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BlockId, Store, StoreOptions};

const RATE: u32 = 48_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-restore-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mono(store: &Store, piece: Piece) -> DocState {
    DocState::new(store.clone(), vec![Channel::from_pieces([piece])], RATE, SourceFormat::synthetic(RATE, 1)).unwrap()
}

/// The store, the three states (oldest first, labelled), and the ids of blocks A and B.
fn three_steps(dir: &Path) -> (Store, Vec<(String, DocState)>, BlockId, BlockId) {
    let store = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let a = store.append(&[1.0, 2.0, 3.0, 4.0]).unwrap();
    let b = store.append(&[9.0, 9.0, 9.0]).unwrap();
    let (a_id, b_id) = (a.id(), b.id());
    let open = mono(&store, Piece::whole(a.clone()).unwrap());
    let deleted = mono(&store, Piece::new(a, 0, 2).unwrap());
    let silence = mono(&store, Piece::whole(b).unwrap());
    let steps = vec![("Open".to_string(), open), ("Delete Audio".to_string(), deleted), ("Insert Silence".to_string(), silence)];
    (store, steps, a_id, b_id)
}

fn labels(h: &History) -> Vec<String> {
    h.steps().iter().map(|s| s.label().to_string()).collect()
}

#[test]
fn c86a_restore_keeps_every_step_and_the_cursor() {
    let t = TempDir::new("keeps");
    let (_store, steps, _, _) = three_steps(&t.0);
    let h = History::restore(steps, 1, 1 << 30).unwrap();
    assert_eq!(labels(&h), ["Open", "Delete Audio", "Insert Silence"]);
    assert_eq!(h.cursor(), 1);
    assert_eq!(h.current().len_frames(), 2);
    assert_eq!(h.cap_disk_bytes(), 1 << 30);
    assert_eq!(h.dropped_total(), 0);
    assert_eq!((h.can_undo(), h.can_redo()), (true, true));
    assert_eq!((h.undo_label(), h.redo_label()), (Some("Delete Audio"), Some("Insert Silence")));
    // Bytes each step brought in: A with "Open", nothing new with "Delete Audio", B with "Insert Silence".
    assert_eq!(h.steps().iter().map(|s| s.made_bytes()).collect::<Vec<_>>(), [16, 0, 12]);
}

#[test]
fn c86a_restore_counts_the_disk_the_steps_hold() {
    let t = TempDir::new("bytes");
    let (_store, steps, a_id, b_id) = three_steps(&t.0);
    let mut h = History::restore(steps, 1, 1 << 30).unwrap();
    // The current state uses A (16 bytes); the kept steps also hold B (12 bytes).
    assert_eq!(h.held_bytes(), 12);
    assert_eq!(h.held_blocks(), HashSet::from([a_id, b_id]));
    // Moving the cursor keeps the counts right.
    assert_eq!(h.redo().unwrap().len_frames(), 3);
    assert_eq!(h.held_bytes(), 16);
    assert_eq!(h.go_to(0).unwrap().len_frames(), 4);
    assert_eq!(h.held_bytes(), 12);
}

#[test]
fn c86a_a_new_edit_after_restore_drops_the_redo_steps() {
    let t = TempDir::new("push");
    let (store, steps, a_id, _) = three_steps(&t.0);
    let mut h = History::restore(steps, 1, 1 << 30).unwrap();
    let c = store.append(&[0.5, 0.5]).unwrap();
    let c_id = c.id();
    assert_eq!(h.push("Silence", mono(&store, Piece::whole(c).unwrap())), 0);
    assert_eq!(labels(&h), ["Open", "Delete Audio", "Silence"]);
    assert_eq!(h.cursor(), 2);
    // B was only in the dropped redo step: no longer held.
    assert_eq!(h.held_blocks(), HashSet::from([a_id, c_id]));
    assert_eq!(h.held_bytes(), 16);
}

#[test]
fn c86a_restore_with_the_cursor_on_the_last_step() {
    let t = TempDir::new("last");
    let (_store, steps, _, _) = three_steps(&t.0);
    let h = History::restore(steps, 2, 1 << 30).unwrap();
    assert_eq!(h.cursor(), 2);
    assert_eq!(h.current().len_frames(), 3);
    assert_eq!((h.can_undo(), h.can_redo()), (true, false));
    assert_eq!(h.held_bytes(), 16);
}

#[test]
fn c86a_restore_drops_nothing_even_over_the_cap() {
    let t = TempDir::new("cap");
    let (_store, steps, _, _) = three_steps(&t.0);
    // Recovery gives back what the journal holds; the cap applies again at the next push.
    let h = History::restore(steps, 2, 0).unwrap();
    assert_eq!(labels(&h), ["Open", "Delete Audio", "Insert Silence"]);
    assert_eq!(h.cursor(), 2);
    assert_eq!(h.dropped_total(), 0);
    assert_eq!(h.held_bytes(), 16);
}

#[test]
fn c86a_restore_refuses_no_steps_or_a_cursor_past_them() {
    let t = TempDir::new("none");
    let (_store, steps, _, _) = three_steps(&t.0);
    assert!(History::restore(steps, 3, 1 << 30).is_none());
    assert!(History::restore(Vec::new(), 0, 1 << 30).is_none());
}
