//! Task 07, first part: the action table and the Editor that runs it, without a window.
//! Numbers in the test names refer to task 07's acceptance tests.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, Repeat, TABLE, Toward, action, by_name, zero_crossing};
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
fn t1_every_action_has_a_unique_name() {
    let mut names = HashSet::new();
    for a in TABLE {
        assert!(names.insert(a.name), "name {} twice", a.name);
        assert_eq!(by_name(a.name).map(|b| b.id), Some(a.id));
        assert_eq!(action(a.id).name, a.name);
    }
    // The keys are in `shortcuts` now, one test each (tests/shortcuts.rs).
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
    for id in [
        ActionId::ZoomIn,
        ActionId::ZoomOut,
        ActionId::SetIn,
        ActionId::SetOut,
        ActionId::InsertSilence,
        ActionId::ZoomOutFull,
        ActionId::ToolSelect,
        ActionId::ToolHand,
        ActionId::ToolZoom,
    ] {
        assert_eq!(action(id).kind, wavcraft_core::actions::Kind::Window, "{id:?}");
        let out = e.run(id, 5..9).unwrap();
        assert!(!out.doc_changed && out.selection == (5..9), "{id:?}: the editor leaves it alone");
    }
    assert_eq!(e.history.steps().len(), 1);
    assert_eq!(wavcraft_core::actions::by_name("tool.hand").map(|a| a.id), Some(ActionId::ToolHand));
    assert_eq!(wavcraft_core::actions::by_name("tool.zoom").map(|a| a.id), Some(ActionId::ToolZoom));
}

/// A 1 kHz sine at 48 kHz (48 frames a cycle), phase-shifted so no sample is exactly 0.
fn tone(frames: usize, phase: f64) -> Vec<f32> {
    (0..frames).map(|k| (std::f64::consts::TAU * 1000.0 * k as f64 / f64::from(RATE) + phase).sin() as f32).collect()
}

/// Whether the signal changes sign between frames `i - 1` and `i`, or frame `i` is 0.
fn is_crossing(x: &[f32], i: u64) -> bool {
    let i = i as usize;
    i > 0 && (x[i] == 0.0 || (x[i - 1] < 0.0) != (x[i] < 0.0))
}

#[test]
fn t6_zero_crossing_adjusts_land_on_sign_changes_of_the_1khz_tone() {
    let t = TempDir::new("zero");
    let left = tone(48_000, 0.3);
    let mut e = editor(&t.0, std::slice::from_ref(&left));
    let sel = 1000..5000;
    let crossings_between = |a: u64, b: u64| (a.min(b) + 1..a.max(b)).filter(|&i| is_crossing(&left, i)).count();
    let check = |id, out: std::ops::Range<u64>, moved_start: Option<bool>, moved_end: Option<bool>| {
        for (edge, was, side) in [(out.start, sel.start, moved_start), (out.end, sel.end, moved_end)] {
            match side {
                None => assert_eq!(edge, was, "{id:?}: this edge stays"),
                Some(right) => {
                    assert!(is_crossing(&left, edge), "{id:?}: {edge} is a sign change");
                    assert!(if right { edge >= was } else { edge <= was }, "{id:?}: {was} -> {edge} moves the right way");
                    assert!(edge.abs_diff(was) <= 480, "{id:?}: within 10 ms");
                    assert_eq!(crossings_between(was, edge), 0, "{id:?}: the nearest one");
                }
            }
        }
    };
    for (id, s, en) in [
        (ActionId::ZeroInward, Some(true), Some(false)),
        (ActionId::ZeroOutward, Some(false), Some(true)),
        (ActionId::ZeroLeftToLeft, Some(false), None),
        (ActionId::ZeroLeftToRight, Some(true), None),
        (ActionId::ZeroRightToLeft, None, Some(false)),
        (ActionId::ZeroRightToRight, None, Some(true)),
    ] {
        let out = e.run(id, sel.clone()).unwrap();
        assert!(!out.doc_changed, "{id:?} only moves the selection");
        assert_ne!(out.selection, sel, "{id:?} moved something");
        check(id, out.selection, s, en);
    }
    assert_eq!(e.history.steps().len(), 1, "no history step for a selection change");
    // On a crossing already: nothing moves.
    let on = (1001..2000).find(|&i| is_crossing(&left, i)).unwrap();
    let out = e.run(ActionId::ZeroLeftToRight, on..5000).unwrap();
    assert_eq!(out.selection.start, on);
}

#[test]
fn zero_crossing_finds_none_beyond_10_ms_and_never_turns_the_selection_inside_out() {
    let t = TempDir::new("zero-none");
    // DC for 2,000 frames, then a step down: the only crossing is at 2000.
    let x: Vec<f32> = (0..4000).map(|k| if k < 2000 { 0.5 } else { -0.5 }).collect();
    let mut e = editor(&t.0, &[x]);
    // 900 frames away (more than 480): the edge stays.
    assert_eq!(e.run(ActionId::ZeroRightToRight, 100..1100).unwrap().selection, 100..1100);
    // 300 frames away: it moves there.
    assert_eq!(e.run(ActionId::ZeroRightToRight, 100..1700).unwrap().selection, 100..2000);
    // Inward with both edges finding the same crossing would leave nothing: unchanged.
    let out = e.run(ActionId::ZeroInward, 1900..2100).unwrap();
    assert_eq!(out.selection, 1900..2100);
    assert!(out.message.unwrap().contains("No zero crossings"));
    // It needs a selection.
    assert!(!e.enabled(ActionId::ZeroInward, &(50..50)));
}

#[test]
fn zero_crossing_on_stereo_uses_the_active_channels() {
    let t = TempDir::new("zero-stereo");
    // Left crosses at 1000, right at 1200 (both 0.5 -> -0.5).
    let step = |at: usize| (0..3000).map(move |k| if k < at { 0.5f32 } else { -0.5 }).collect::<Vec<f32>>();
    let mut e = editor(&t.0, &[step(1000), step(1200)]);
    let doc = e.history.current().clone();
    use wavcraft_core::doc::edit::ChannelMask;
    assert_eq!(zero_crossing(&doc, ChannelMask::only(0), 900, Toward::Right, 480).unwrap(), Some(1000));
    assert_eq!(zero_crossing(&doc, ChannelMask::only(1), 900, Toward::Right, 480).unwrap(), Some(1200));
    // Both: the mix is 0 from 1000 to 1199, and 0 counts as a crossing.
    assert_eq!(zero_crossing(&doc, ChannelMask::ALL, 900, Toward::Right, 480).unwrap(), Some(1000));
    e.run(ActionId::RightChannel, 0..0).unwrap();
    assert_eq!(e.run(ActionId::ZeroRightToRight, 0..900).unwrap().selection, 0..1200, "Down: the right channel only");
    // Ends of the file are safe.
    assert_eq!(zero_crossing(&doc, ChannelMask::ALL, 0, Toward::Left, 480).unwrap(), None);
    assert_eq!(zero_crossing(&doc, ChannelMask::ALL, 3000, Toward::Right, 480).unwrap(), None);
}

#[test]
fn t7_repeat_last_command_after_insert_silence_inserts_the_same_length_at_the_cursor() {
    let t = TempDir::new("repeat-insert");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    assert!(!e.enabled(ActionId::RepeatLast, &(0..0)), "nothing to repeat yet");
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Last Command");
    e.insert_silence(100, 250).unwrap();
    assert_eq!(e.last, Some(Repeat::InsertSilence(250)));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Insert Silence");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Insert Silence…");
    // The cursor at 800 (in the new length): Shift+R inserts 250 more there.
    let out = e.run(ActionId::RepeatLast, 800..800).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 800..1050);
    let c = now(&e);
    assert_eq!(c[0].len(), 1500);
    assert!(c[0][800..1050].iter().chain(&c[1][800..1050]).all(|x| *x == 0.0));
    assert_eq!(&c[0][1050..], &data[0][550..]);
    assert_eq!(e.history.steps().len(), 3, "Open + two inserts");
    assert_eq!(e.label(ActionId::Undo), "Undo Insert Silence");
}

#[test]
fn repeat_last_command_runs_silence_again_on_the_new_selection_but_never_delete() {
    let t = TempDir::new("repeat-silence");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    e.run(ActionId::Silence, 10..20).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Silence");
    e.run(ActionId::RepeatLast, 500..600).unwrap();
    let c = now(&e);
    assert!(c[0][500..600].iter().all(|x| *x == 0.0) && c[0][600] == 600.0);
    // Delete is not repeatable: Repeat still means Silence.
    e.run(ActionId::Delete, 700..710).unwrap();
    assert_eq!(e.last, Some(Repeat::Action(ActionId::Silence)));
    e.run(ActionId::RepeatLast, 0..5).unwrap();
    assert_eq!(e.history.undo_label(), Some("Silence"));
    // Repeat follows the repeated command's enabled rule: Silence needs a selection.
    assert!(!e.enabled(ActionId::RepeatLast, &(3..3)));
    assert_eq!(by_name("edit.zeroCrossings.outward").map(|a| a.id), Some(ActionId::ZeroOutward));
}
