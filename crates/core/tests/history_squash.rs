//! Card N4.3 (v1 step N4.3): `History::squash_since`, which replaces the steps after a cursor with
//! one step holding the current state, so a run of edits undoes in one step. Locked tests: every
//! expected value is written out here.
//!
//! The four states share a store: block A holds 4 frames (16 bytes on disk), B 2 frames (8 bytes),
//! C 3 frames (12 bytes), D 3 frames (12 bytes). "Open" is A, then E1 is B, E2 is C, E3 is D.

use std::path::PathBuf;

use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-squash-test-{}-{name}", std::process::id()));
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

fn content(doc: &DocState) -> Vec<f32> {
    let mut v = vec![0.0f32; doc.len_frames() as usize];
    assert_eq!(doc.read(0, 0, &mut v).unwrap(), v.len());
    v
}

/// A history of four steps: "Open" (A), then E1 (B), E2 (C), E3 (D), cursor on E3.
fn four_steps(dir: &std::path::Path) -> (Store, History) {
    let store = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let a = store.append(&[1.0, 2.0, 3.0, 4.0]).unwrap();
    let b = store.append(&[5.0, 6.0]).unwrap();
    let c = store.append(&[7.0, 8.0, 9.0]).unwrap();
    let d = store.append(&[9.0, 9.0, 9.0]).unwrap();
    let mut h = History::new(mono(&store, Piece::whole(a).unwrap()), u64::MAX);
    h.push("E1", mono(&store, Piece::whole(b).unwrap()));
    h.push("E2", mono(&store, Piece::whole(c).unwrap()));
    h.push("E3", mono(&store, Piece::whole(d).unwrap()));
    (store, h)
}

fn labels(h: &History) -> Vec<String> {
    h.steps().iter().map(|s| s.label().to_string()).collect()
}

#[test]
fn squash_makes_one_step() {
    let t = TempDir::new("one");
    let (_store, mut h) = four_steps(&t.0);
    assert_eq!(h.steps().len(), 4);
    assert_eq!(h.held_bytes(), 36);
    let before = h.held_bytes();

    assert_eq!(h.squash_since(0, "Run Script"), Some(3));
    assert_eq!(h.steps().len(), 2);
    assert_eq!(h.cursor(), 1);
    assert_eq!(h.undo_label(), Some("Run Script"));
    assert!(h.held_bytes() <= before);
    assert_eq!(h.held_bytes(), 16);
    assert_eq!(content(h.current()), [9.0, 9.0, 9.0]);

    let undone = h.undo().unwrap();
    assert_eq!(content(&undone), [1.0, 2.0, 3.0, 4.0]);
    let redone = h.redo().unwrap();
    assert_eq!(content(&redone), [9.0, 9.0, 9.0]);
    assert!(h.redo().is_none());
}

#[test]
fn squash_keeps_the_steps_up_to_cursor() {
    let t = TempDir::new("keeps");
    let (_store, mut h) = four_steps(&t.0);

    assert_eq!(h.squash_since(1, "Run Script"), Some(2));
    assert_eq!(labels(&h), ["Open", "E1", "Run Script"]);
    assert_eq!(h.cursor(), 2);
    let undone = h.undo().unwrap();
    assert_eq!(content(&undone), [5.0, 6.0]);
}

#[test]
fn squash_with_nothing_after_cursor_is_none() {
    let t = TempDir::new("none");
    let (_store, mut h) = four_steps(&t.0);

    assert!(h.squash_since(3, "x").is_none());
    assert_eq!(h.steps().len(), 4);
    assert!(h.squash_since(9, "x").is_none());
    assert_eq!(h.steps().len(), 4);

    h.undo().unwrap();
    assert_eq!(h.cursor(), 2);
    assert!(h.squash_since(2, "x").is_none());
    assert_eq!(h.steps().len(), 4);
}
