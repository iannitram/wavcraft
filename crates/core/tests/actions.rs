//! Task 07, first part: the action table and the Editor that runs it, without a window.
//! Numbers in the test names refer to task 07's acceptance tests.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, TABLE, action, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-actions-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path) -> Store {
    Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

/// Two channels of distinct, exact values: left k, right -k-1.
fn stereo(frames: usize) -> Vec<Vec<f32>> {
    vec![(0..frames).map(|k| k as f32).collect(), (0..frames).map(|k| -(k as f32) - 1.0).collect()]
}

fn doc_from(s: &Store, data: &[Vec<f32>]) -> DocState {
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<f32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// An editor on `data` with crossfades off, so contents can be checked exactly.
fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = store(dir);
    let mut e = Editor::new(History::new(doc_from(&s, data), u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn now(e: &Editor) -> Vec<Vec<f32>> {
    content(e.history.current())
}

#[test]
fn t1_every_action_has_unique_keys_and_a_unique_name() {
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    for a in TABLE {
        assert!(names.insert(a.name), "name {} twice", a.name);
        assert_eq!(by_name(a.name).map(|b| b.id), Some(a.id));
        assert_eq!(action(a.id).name, a.name);
        for k in a.keys {
            assert!(keys.insert(*k), "{k:?} bound twice");
        }
    }
    // Audition's keys for the common ones (task 07's table).
    let text = |id| action(id).keys.iter().map(|k| k.text(false)).collect::<Vec<_>>();
    assert_eq!(text(ActionId::Undo), ["Ctrl+Z"]);
    assert_eq!(text(ActionId::Redo), ["Ctrl+Shift+Z", "Ctrl+Y"]);
    assert_eq!(text(ActionId::Delete), ["Delete", "Backspace"]);
    assert_eq!(text(ActionId::Crop), ["Ctrl+T"]);
    assert_eq!(text(ActionId::ClearTimeSelection), ["G"]);
    assert!(action(ActionId::Silence).keys.is_empty(), "Audition has no key for Silence");
    assert_eq!(action(ActionId::Undo).keys[0].text(true), "⌘Z");
}

#[test]
fn t2_cut_and_copy_with_no_selection_take_the_whole_file() {
    let t = TempDir::new("whole");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    let out = e.run(ActionId::Copy, 10..10).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(e.clipboard.as_ref().map(|c| c.len_frames()), Some(1000));
    // Paste at frame 500 inserts the whole file there.
    let out = e.run(ActionId::Paste, 500..500).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 500..1500);
    let c = now(&e);
    assert_eq!(c[0].len(), 2000);
    assert_eq!(&c[0][500..1500], &data[0][..]);
    // Cut with no selection empties it into the clipboard.
    let mut e = editor(&t.0.join("b"), &data);
    let out = e.run(ActionId::Cut, 0..0).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.current().len_frames(), 0);
    assert_eq!(e.clipboard.as_ref().map(|c| c.len_frames()), Some(1000));
}

#[test]
fn t3_a_rough_cut_through_actions_only_and_undo_to_open() {
    let t = TempDir::new("rough");
    let data = stereo(10_000);
    let mut e = editor(&t.0, &data);
    let mut model = data.clone();
    let cut = |m: &mut Vec<Vec<f32>>, r: std::ops::Range<usize>| -> Vec<Vec<f32>> { m.iter_mut().map(|c| c.drain(r.clone()).collect()).collect() };
    for k in 0..10 {
        let a = 100 + k * 300;
        e.run(ActionId::Delete, a as u64..(a + 50) as u64).unwrap();
        cut(&mut model, a..a + 50);
    }
    for k in 0..3 {
        let a = 200 + k * 1000;
        e.run(ActionId::Cut, a as u64..(a + 100) as u64).unwrap();
        let clip = cut(&mut model, a..a + 100);
        let at = 5000 - k * 700;
        e.run(ActionId::Paste, at as u64..at as u64).unwrap();
        for (c, piece) in model.iter_mut().zip(clip) {
            c.splice(at..at, piece);
        }
    }
    e.run(ActionId::Silence, 1000..1100).unwrap();
    model.iter_mut().for_each(|c| c[1000..1100].fill(0.0));
    e.run(ActionId::Silence, 2000..2010).unwrap();
    model.iter_mut().for_each(|c| c[2000..2010].fill(0.0));
    let out = e.run(ActionId::Crop, 500..9000).unwrap();
    assert_eq!(out.selection, 0..8500);
    model.iter_mut().for_each(|c| {
        c.truncate(9000);
        c.drain(..500);
    });
    assert_eq!(now(&e), model);
    // History: Open plus 10 + 6 + 2 + 1 steps; back to Open is the original, bit for bit.
    assert_eq!(e.history.steps().len(), 20);
    e.history.go_to(0);
    let back: Vec<Vec<u32>> = now(&e).iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect();
    let orig: Vec<Vec<u32>> = data.iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect();
    assert_eq!(back, orig);
}

#[test]
fn t4_up_selects_the_left_channel_and_silence_leaves_the_right_alone() {
    let t = TempDir::new("left");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    let out = e.run(ActionId::LeftChannel, 100..200).unwrap();
    assert_eq!(out.message.as_deref(), Some("Editing the left channel only"));
    e.run(ActionId::Silence, 100..200).unwrap();
    let c = now(&e);
    assert!(c[0][100..200].iter().all(|x| *x == 0.0));
    assert_eq!(c[1], data[1], "right untouched");
    // Up again: both channels.
    let out = e.run(ActionId::LeftChannel, 100..200).unwrap();
    assert_eq!(out.message.as_deref(), Some("Editing both channels"));
}

#[test]
fn t9b_up_then_delete_silences_the_left_range_and_keeps_the_length() {
    let t = TempDir::new("leftdel");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    let out = e.run(ActionId::Delete, 300..400).unwrap();
    assert_eq!(out.message.as_deref(), Some("Delete Audio"));
    let c = now(&e);
    assert_eq!(c[0].len(), 1000);
    assert!(c[0][300..400].iter().all(|x| *x == 0.0));
    assert_eq!(c[1], data[1]);
}

#[test]
fn t8_undo_and_redo_labels_name_the_step() {
    let t = TempDir::new("labels");
    let mut e = editor(&t.0, &stereo(1000));
    assert_eq!(e.label(ActionId::Undo), "Undo");
    assert!(!e.enabled(ActionId::Undo, &(0..0)));
    e.run(ActionId::Cut, 10..20).unwrap();
    assert_eq!(e.label(ActionId::Undo), "Undo Cut");
    let out = e.run(ActionId::Undo, 10..10).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.label(ActionId::Redo), "Redo Cut");
    assert_eq!(e.history.current().len_frames(), 1000);
    e.run(ActionId::Redo, 10..10).unwrap();
    assert_eq!(e.history.current().len_frames(), 990);
}

#[test]
fn delete_needs_a_selection_and_says_so() {
    let t = TempDir::new("nosel");
    let mut e = editor(&t.0, &stereo(100));
    assert!(!e.enabled(ActionId::Delete, &(5..5)));
    let out = e.run(ActionId::Delete, 5..5).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(out.message.as_deref(), Some("Delete is not available now"));
    assert_eq!(e.history.steps().len(), 1);
}

#[test]
fn select_all_and_clear_time_selection() {
    let t = TempDir::new("select");
    let mut e = editor(&t.0, &stereo(777));
    assert_eq!(e.run(ActionId::SelectAll, 3..3).unwrap().selection, 0..777);
    assert_eq!(e.run(ActionId::ClearTimeSelection, 10..50).unwrap().selection, 10..10);
    assert_eq!(e.run(ActionId::DeselectAll, 10..50).unwrap().selection, 10..10);
}

#[test]
fn home_and_end_move_the_cursor_and_insert_silence_selects_what_it_inserted() {
    let t = TempDir::new("nav");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    assert_eq!(e.run(ActionId::CursorToEnd, 10..20).unwrap().selection, 1000..1000);
    assert_eq!(e.run(ActionId::CursorToStart, 10..20).unwrap().selection, 0..0);
    let out = e.insert_silence(400, 250).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 400..650);
    assert_eq!(out.message.as_deref(), Some("Insert Silence"));
    let c = now(&e);
    assert_eq!(c[0].len(), 1250);
    assert!(c[0][400..650].iter().chain(&c[1][400..650]).all(|x| *x == 0.0));
    assert_eq!(&c[0][650..], &data[0][400..]);
    assert_eq!(e.label(ActionId::Undo), "Undo Insert Silence");
}

#[test]
fn window_actions_are_left_to_the_window() {
    let t = TempDir::new("window");
    let mut e = editor(&t.0, &stereo(100));
    for id in [ActionId::ZoomIn, ActionId::ZoomOut, ActionId::SetIn, ActionId::SetOut, ActionId::InsertSilence, ActionId::ZoomOutFull] {
        assert_eq!(action(id).kind, wavcraft_core::actions::Kind::Window, "{id:?}");
        let out = e.run(id, 5..9).unwrap();
        assert!(!out.doc_changed && out.selection == (5..9), "{id:?}: the editor leaves it alone");
    }
    assert_eq!(e.history.steps().len(), 1);
    // Audition's keys for them.
    let text = |id| action(id).keys.iter().map(|k| k.text(false)).collect::<Vec<_>>();
    assert_eq!(text(ActionId::ZoomToSelection), ["Shift+S"]);
    assert_eq!(text(ActionId::ZoomOutFull), ["Ctrl+\\"]);
    assert_eq!(text(ActionId::SetIn), ["I"]);
    assert_eq!(text(ActionId::ZoomIn), ["="]);
}
