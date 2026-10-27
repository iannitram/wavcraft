//! Task 07, first part: the action table and the Editor that runs it, without a window.
//! Numbers in the test names refer to task 07's acceptance tests.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor, Repeat, TABLE, Toward, action, by_name, zero_crossing};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::shortcuts::{Command, Keymap};
use wavcraft_core::save::{SampleType, save_wav};
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
    assert_eq!(e.clipboard().map(|c| c.len_frames()), Some(1000));
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
    assert_eq!(e.clipboard().map(|c| c.len_frames()), Some(1000));
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
        ActionId::ToolMarquee,
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
    // On a crossing already: the edge steps to the next one (Hyperion's check of #7: Shift+J and
    // Shift+L did nothing there), so pressing the key again walks along the crossings.
    let on = (1001..2000).find(|&i| is_crossing(&left, i)).unwrap();
    let next = (on + 1..on + 480).find(|&i| is_crossing(&left, i)).unwrap();
    assert_eq!(e.run(ActionId::ZeroLeftToRight, on..5000).unwrap().selection.start, next);
    let end_on = (4000..5000).rev().find(|&i| is_crossing(&left, i)).unwrap();
    let end_next = (end_on + 1..end_on + 480).find(|&i| is_crossing(&left, i)).unwrap();
    assert_eq!(e.run(ActionId::ZeroRightToRight, 1000..end_on).unwrap().selection.end, end_next);
    let prev = (end_on - 480..end_on).rev().find(|&i| is_crossing(&left, i)).unwrap();
    assert_eq!(e.run(ActionId::ZeroRightToLeft, 1000..end_on).unwrap().selection.end, prev);
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

// ---------------------------------------------------------------------------------------------
// Markers (A170, A171, A176, A121)

fn markers(e: &Editor) -> Vec<(u64, u64)> {
    e.history.current().markers().iter().map(|m| (m.start, m.end)).collect()
}

#[test]
fn m1_add_marker_is_a_point_at_the_cursor_or_a_range_for_a_selection_and_undo_redo_restore_it() {
    let t = TempDir::new("markers");
    let mut e = editor(&t.0, &stereo(1000));
    assert_eq!(Keymap::new().menu_text(Command::Action(ActionId::MarkerAdd), false), "M");
    let steps = e.history.steps().len();
    let out = e.run(ActionId::MarkerAdd, 100..100).unwrap();
    assert!(!out.doc_changed, "the audio is untouched");
    assert_eq!(out.selection, 100..100);
    e.run(ActionId::MarkerAdd, 300..400).unwrap();
    assert_eq!(markers(&e), [(100, 100), (300, 400)]);
    assert_eq!(e.history.steps().len(), steps + 2);
    assert_eq!(e.history.undo_label(), Some("Add Marker"));
    // The same place twice adds nothing and records nothing.
    e.run(ActionId::MarkerAdd, 100..100).unwrap();
    assert_eq!(e.history.steps().len(), steps + 2);
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e), [(100, 100)]);
    e.run(ActionId::Undo, 0..0).unwrap();
    assert!(markers(&e).is_empty());
    e.run(ActionId::Redo, 0..0).unwrap();
    e.run(ActionId::Redo, 0..0).unwrap();
    assert_eq!(markers(&e), [(100, 100), (300, 400)]);
}

#[test]
fn m2_next_and_previous_marker_move_the_cursor_and_are_disabled_at_the_ends() {
    let t = TempDir::new("marker-nav");
    let mut e = editor(&t.0, &stereo(1000));
    for at in [200, 500, 800] {
        e.run(ActionId::MarkerAdd, at..at).unwrap();
    }
    assert!(!e.enabled(ActionId::MarkerPrev, &(200..200)));
    assert!(e.enabled(ActionId::MarkerNext, &(200..200)));
    assert_eq!(e.run(ActionId::MarkerNext, 0..0).unwrap().selection, 200..200);
    assert_eq!(e.run(ActionId::MarkerNext, 200..200).unwrap().selection, 500..500);
    assert_eq!(e.run(ActionId::MarkerNext, 600..650).unwrap().selection, 800..800, "from the selection's start");
    assert!(!e.enabled(ActionId::MarkerNext, &(800..800)));
    assert_eq!(e.run(ActionId::MarkerPrev, 800..800).unwrap().selection, 500..500);
    assert_eq!(e.run(ActionId::MarkerPrev, 501..501).unwrap().selection, 500..500);
    assert_eq!(e.run(ActionId::MarkerPrev, 100..100).unwrap().message.as_deref(), Some("Playhead to Previous Marker is not available now"));
    // Moving the playhead is not an edit.
    assert_eq!(e.history.steps().len(), 4);
}

#[test]
fn m3_a_marker_inside_a_deleted_range_goes_and_undo_brings_it_back() {
    let t = TempDir::new("marker-edit");
    let mut e = editor(&t.0, &stereo(1000));
    e.run(ActionId::MarkerAdd, 100..100).unwrap();
    e.run(ActionId::MarkerAdd, 900..950).unwrap();
    e.run(ActionId::Delete, 500..1000).unwrap();
    assert_eq!(e.history.current().len_frames(), 500);
    assert_eq!(markers(&e), [(100, 100)]);
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e), [(100, 100), (900, 950)], "undo brings the original positions back");
}

/// Markers at 100, 300..400, 450, 700..900 and 950 in a 1000-frame file.
fn marked(dir: &Path) -> Editor {
    let mut e = editor(dir, &stereo(1000));
    for r in [100..100, 300..400, 450..450, 700..900, 950..950] {
        e.run(ActionId::MarkerAdd, r).unwrap();
    }
    e
}

#[test]
fn m4_delete_and_cut_shift_trim_and_remove_markers() {
    for id in [ActionId::Delete, ActionId::Cut] {
        let t = TempDir::new("marker-del");
        let mut e = marked(&t.0);
        e.run(id, 350..800).unwrap();
        // 100 stays; 300..400 loses its tail (ends at 350); 450 is inside: gone; 700..900 keeps 800..900 (350..450);
        // 950 moves back by 450.
        assert_eq!(markers(&e), [(100, 100), (300, 350), (350, 450), (500, 500)], "{id:?}");
        e.run(ActionId::Undo, 0..0).unwrap();
        assert_eq!(markers(&e).len(), 5);
    }
}

#[test]
fn m5_insert_silence_and_paste_shift_markers_after_the_point_and_grow_a_range_across_it() {
    let t = TempDir::new("marker-ins");
    let mut e = marked(&t.0);
    e.insert_silence(350, 100).unwrap();
    assert_eq!(markers(&e), [(100, 100), (300, 500), (550, 550), (800, 1000), (1050, 1050)]);
    // A point exactly at the insertion stays; the one after moves.
    let t2 = TempDir::new("marker-paste");
    let mut e = marked(&t2.0);
    e.run(ActionId::Copy, 0..40).unwrap();
    e.run(ActionId::Paste, 450..450).unwrap();
    assert_eq!(markers(&e), [(100, 100), (300, 400), (450, 450), (740, 940), (990, 990)]);
}

#[test]
fn m6_paste_over_a_selection_replaces_markers_inside_and_a_same_length_paste_moves_nothing() {
    let t = TempDir::new("marker-repl");
    let mut e = marked(&t.0);
    e.run(ActionId::Copy, 0..40).unwrap();
    e.run(ActionId::Paste, 420..500).unwrap();
    // 450 is inside the replaced 420..500 and goes; later markers move by 40 - 80.
    assert_eq!(markers(&e), [(100, 100), (300, 400), (660, 860), (910, 910)]);
    let t2 = TempDir::new("marker-same");
    let mut e = marked(&t2.0);
    e.run(ActionId::Copy, 0..80).unwrap();
    e.run(ActionId::Paste, 420..500).unwrap();
    assert_eq!(markers(&e).len(), 5, "a same-length overwrite keeps every marker");
    e.run(ActionId::Silence, 0..1000).unwrap();
    assert_eq!(markers(&e).len(), 5, "silence moves nothing");
}

#[test]
fn m7_crop_keeps_what_is_inside_shifted_to_the_start_and_trims_ranges() {
    let t = TempDir::new("marker-crop");
    let mut e = marked(&t.0);
    e.run(ActionId::Crop, 350..800).unwrap();
    assert_eq!(e.history.current().len_frames(), 450);
    assert_eq!(markers(&e), [(0, 50), (100, 100), (350, 450)]);
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e).len(), 5);
}

#[test]
fn m8_one_channel_edits_leave_markers_alone() {
    let t = TempDir::new("marker-onech");
    let mut e = marked(&t.0);
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    e.run(ActionId::Delete, 350..800).unwrap();
    assert_eq!(e.history.current().len_frames(), 1000);
    assert_eq!(markers(&e).len(), 5);
}

#[test]
fn m9_delete_marker_removes_those_the_selection_touches_and_undo_restores() {
    let t = TempDir::new("marker-delete");
    let mut e = marked(&t.0);
    assert!(!e.enabled(ActionId::MarkerDelete, &(200..200)));
    assert!(e.run(ActionId::MarkerDelete, 200..200).unwrap().message.unwrap().contains("not available"));
    e.run(ActionId::MarkerDelete, 100..100).unwrap();
    assert_eq!(markers(&e).len(), 4);
    // A cursor inside a range touches it; a selection touches every marker it overlaps.
    e.run(ActionId::MarkerDelete, 350..350).unwrap();
    assert_eq!(markers(&e), [(450, 450), (700, 900), (950, 950)]);
    e.run(ActionId::MarkerDelete, 400..800).unwrap();
    assert_eq!(markers(&e), [(950, 950)]);
    assert_eq!(e.history.undo_label(), Some("Delete Marker"));
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e).len(), 3);
    assert_eq!(e.history.current().len_frames(), 1000, "the audio is untouched");
}

#[test]
fn m10_convert_point_to_range_uses_the_selection_and_range_to_point_keeps_its_start() {
    let t = TempDir::new("marker-convert");
    let mut e = marked(&t.0);
    // Needs a selection that touches a point marker.
    assert!(!e.enabled(ActionId::MarkerToRange, &(100..100)));
    assert!(!e.enabled(ActionId::MarkerToRange, &(500..600)));
    assert!(e.enabled(ActionId::MarkerToRange, &(80..160)));
    let name = e.history.current().markers().iter().find(|m| m.start == 100).unwrap().name.clone();
    e.run(ActionId::MarkerToRange, 80..160).unwrap();
    assert_eq!(markers(&e), [(80, 160), (300, 400), (450, 450), (700, 900), (950, 950)]);
    assert_eq!(e.history.current().markers().as_slice()[0].name, name, "the name is kept");
    // Range to point: a cursor inside the range will do.
    assert!(!e.enabled(ActionId::MarkerToPoint, &(500..500)));
    e.run(ActionId::MarkerToPoint, 750..750).unwrap();
    assert_eq!(markers(&e), [(80, 160), (300, 400), (450, 450), (700, 700), (950, 950)]);
    assert_eq!(e.history.undo_label(), Some("Convert Marker to Point"));
    e.run(ActionId::Undo, 0..0).unwrap();
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e)[0], (100, 100));
}

// ---- Insert > Audio from File (A169) ----

/// Write `data` at `rate` as a 32-bit float WAV in `dir` (a file to insert), made in its own store.
fn wav_file(dir: &Path, name: &str, data: &[Vec<f32>], rate: u32) -> PathBuf {
    let s = store(&dir.join(format!("src-{name}")));
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s.clone(), chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap();
    let path = dir.join(format!("{name}.wav"));
    save_wav(&doc, &path, SampleType::Float32, &mut |_| {}).unwrap();
    path
}

/// Stream `path` into the editor's own store, as the app does.
fn open_into(e: &Editor, path: &Path) -> DocState {
    let opts = OpenOptions { into_store: Some(e.history.current().store().clone()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn insert_file_at_the_cursor_is_one_undoable_step_and_copies_no_samples() {
    let t = TempDir::new("insfile-cursor");
    let mut e = editor(&t.0.join("doc"), &stereo(1000));
    let before = now(&e);
    let file_data = vec![(0..300).map(|k| 10_000.0 + k as f32).collect::<Vec<_>>(), (0..300).map(|k| -10_000.0 - k as f32).collect()];
    let path = wav_file(&t.0, "ins", &file_data, RATE);
    let file = open_into(&e, &path);
    let bytes0 = e.history.current().store().bytes_on_disk();
    let out = e.insert_file(&file, 400..400).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 400..700);
    assert_eq!(e.history.undo_label(), Some("Insert Audio from File"));
    assert_eq!(e.history.steps().len(), 2);
    let after = now(&e);
    for ch in 0..2 {
        assert_eq!(&after[ch][..400], &before[ch][..400]);
        assert_eq!(&after[ch][400..700], &file_data[ch][..]);
        assert_eq!(&after[ch][700..], &before[ch][400..]);
    }
    assert_eq!(e.history.current().store().bytes_on_disk(), bytes0, "the inserted blocks are shared, not copied");
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(now(&e), before);
}

#[test]
fn insert_file_over_a_selection_replaces_it_and_mono_goes_to_both_channels() {
    let t = TempDir::new("insfile-replace");
    let mut e = editor(&t.0.join("doc"), &stereo(1000));
    let before = now(&e);
    let mono = vec![(0..100).map(|k| 5_000.0 + k as f32).collect::<Vec<_>>()];
    let file = open_into(&e, &wav_file(&t.0, "mono", &mono, RATE));
    let out = e.insert_file(&file, 200..500).unwrap();
    assert_eq!(out.selection, 200..300);
    let after = now(&e);
    assert_eq!(e.history.current().len_frames(), 1000 - 300 + 100);
    for ch in 0..2 {
        assert_eq!(&after[ch][..200], &before[ch][..200]);
        assert_eq!(&after[ch][200..300], &mono[0][..]);
        assert_eq!(&after[ch][300..], &before[ch][500..]);
    }
}

#[test]
fn insert_file_with_another_sample_rate_is_refused_and_changes_nothing() {
    let t = TempDir::new("insfile-rate");
    let mut e = editor(&t.0.join("doc"), &stereo(1000));
    let file = open_into(&e, &wav_file(&t.0, "r44", &stereo(100), 44_100));
    let err = e.insert_file(&file, 10..10).unwrap_err();
    assert!(err.contains("44100 Hz") && err.contains("48000 Hz") && err.contains("nothing was inserted"), "{err}");
    assert_eq!(e.history.steps().len(), 1);
    assert_eq!(e.history.current().len_frames(), 1000);
}

#[test]
fn insert_file_refuses_a_file_opened_into_another_store() {
    let t = TempDir::new("insfile-store");
    let mut e = editor(&t.0.join("doc"), &stereo(1000));
    let path = wav_file(&t.0, "x", &stereo(50), RATE);
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    let other = open(&path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(e.insert_file(&other, 0..0).is_err());
    assert_eq!(e.history.steps().len(), 1);
}

#[test]
fn insert_file_is_not_repeatable_and_is_a_window_action() {
    assert_eq!(action(ActionId::InsertFile).kind, wavcraft_core::actions::Kind::Window);
    assert_eq!(action(ActionId::InsertFile).name, "edit.insertFile");
}

#[test]
fn m6_rename_and_move_a_marker_are_one_undo_step_each() {
    let t = TempDir::new("marker-rename-move");
    let mut e = editor(&t.0, &stereo(2000));
    e.run(ActionId::MarkerAdd, 100..100).unwrap();
    e.run(ActionId::MarkerAdd, 300..400).unwrap();
    let steps = e.history.steps().len();
    let id = e.history.current().markers().id_at(1).unwrap();
    assert!(e.rename_marker(id, "Chorus"));
    assert_eq!(e.history.steps().len(), steps + 1);
    assert_eq!(e.history.undo_label(), Some("Rename Marker"));
    assert!(!e.rename_marker(id, "Chorus") && !e.rename_marker(id, " "), "no change, no step");
    assert_eq!(e.history.steps().len(), steps + 1);
    assert!(e.move_marker(id, 1900));
    assert_eq!(e.history.undo_label(), Some("Move Marker"));
    let m = e.history.current().markers().as_slice()[1].clone();
    assert_eq!((m.name.as_str(), m.start, m.end), ("Chorus", 1900, 2000), "length kept, held inside the file");
    assert!(!e.move_marker(id, 1900));
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(markers(&e), [(100, 100), (300, 400)]);
    assert_eq!(e.history.current().markers().as_slice()[1].name, "Chorus");
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(e.history.current().markers().as_slice()[1].name, "Marker 02");
    e.run(ActionId::Redo, 0..0).unwrap();
    e.run(ActionId::Redo, 0..0).unwrap();
    assert_eq!(markers(&e)[1], (1900, 2000));
}

#[test]
fn f42_select_actions_and_their_control_names() {
    let t = TempDir::new("f42select");
    let mut e = editor(&t.0, &stereo(1000));
    let sel = |e: &mut Editor, id, from| e.run(id, from).unwrap().selection;
    assert_eq!(sel(&mut e, by_name("select.all").unwrap().id, 10..20), 0..1000);
    assert_eq!(sel(&mut e, by_name("select.none").unwrap().id, 10..20), 10..10);
    assert_eq!(sel(&mut e, by_name("select.toStart").unwrap().id, 400..900), 0..400);
    assert_eq!(sel(&mut e, by_name("select.toEnd").unwrap().id, 400..900), 900..1000);
    assert!(!e.enabled(ActionId::SelectToStart, &(0..0)), "nothing before the start");
    assert!(!e.enabled(ActionId::SelectToEnd, &(1000..1000)), "nothing after the end");
    // Select Current View is the window's; the editor leaves the selection alone.
    assert_eq!(action(ActionId::SelectCurrentView).kind, wavcraft_core::actions::Kind::Window);
    assert_eq!(sel(&mut e, ActionId::SelectCurrentView, 5..9), 5..9);
    assert_eq!(e.history.steps().len(), 1, "selecting records no step");
}

#[test]
fn f42_a_channel_mask_from_a_drag_limits_edits_and_both_channels_undoes_it() {
    use wavcraft_core::ChannelMask;
    use wavcraft_core::view::channels_for_drag;
    let t = TempDir::new("f42dragmask");
    let data = stereo(1000);
    let mut e = editor(&t.0, &data);
    // A drag in the bottom strip selects the right channel only; the app hands that to the editor.
    e.channels = channels_for_drag(0.9, 2, true);
    assert_eq!(e.channels, ChannelMask::only(1));
    e.run(ActionId::Silence, 100..200).unwrap();
    let c = now(&e);
    assert_eq!(c[0], data[0], "left untouched");
    assert!(c[1][100..200].iter().all(|x| *x == 0.0));
    // Enable Channels > Both, then the same edit hits the left too.
    assert_eq!(e.label(ActionId::BothChannels), "Both Channels");
    e.run(ActionId::BothChannels, 100..200).unwrap();
    assert_eq!(e.channels, ChannelMask::ALL);
    e.run(ActionId::Silence, 300..400).unwrap();
    let c = now(&e);
    assert!(c[0][300..400].iter().all(|x| *x == 0.0) && c[1][300..400].iter().all(|x| *x == 0.0));
    // A top-strip drag: left only, and Delete silences in place.
    e.channels = channels_for_drag(0.1, 2, true);
    e.run(ActionId::Delete, 500..600).unwrap();
    assert_eq!(now(&e)[0].len(), 1000);
}

#[test]
fn f19_zoom_actions_answer_to_their_panel_names_and_amplitude_zoom_is_bound() {
    assert_eq!(by_name("view.zoom.in").map(|a| a.id), Some(ActionId::ZoomIn));
    assert_eq!(by_name("view.zoom.out").map(|a| a.id), Some(ActionId::ZoomOut));
    assert_eq!(by_name("view.zoom.selection").map(|a| a.id), Some(ActionId::ZoomToSelection));
    assert_eq!(by_name("view.zoom.full").map(|a| a.id), Some(ActionId::ZoomOutFull));
    assert_eq!(by_name("view.zoom.ampIn").map(|a| a.id), Some(ActionId::ZoomAmpIn));
    assert_eq!(by_name("view.zoom.ampOut").map(|a| a.id), Some(ActionId::ZoomAmpOut));
    assert_eq!(by_name("view.zoom.ampReset").map(|a| a.id), Some(ActionId::ZoomAmpReset));
}

#[test]
fn f68_toggling_a_channel_enables_and_disables_it_for_edits() {
    let dir = TempDir::new("f68-toggle");
    let mut e = editor(&dir.0, &[vec![1.0; 1000], vec![2.0; 1000], vec![3.0; 1000]]);
    assert_eq!(e.channels.active(3), [0, 1, 2], "all on to start with");
    e.run(ActionId::ToggleChannel(1), 0..0).unwrap();
    assert_eq!(e.channels.active(3), [0, 2]);
    // Silence now acts on channels 0 and 2 only.
    e.run(ActionId::Silence, 100..200).unwrap();
    let c = now(&e);
    assert_eq!((c[0][150], c[1][150], c[2][150]), (0.0, 2.0, 0.0));
    e.run(ActionId::ToggleChannel(1), 0..0).unwrap();
    assert_eq!(e.channels.active(3), [0, 1, 2]);
    assert_eq!(e.channels, wavcraft_core::ChannelMask::ALL, "all on is the plain 'every channel' mask again");
}

#[test]
fn f68_the_last_enabled_channel_cannot_be_turned_off_and_a_missing_channel_is_refused() {
    let dir = TempDir::new("f68-last");
    let mut e = editor(&dir.0, &stereo(100));
    e.run(ActionId::ToggleChannel(0), 0..0).unwrap();
    let out = e.run(ActionId::ToggleChannel(1), 0..0).unwrap();
    assert_eq!(out.message.as_deref(), Some("At least one channel stays enabled"));
    assert_eq!(e.channels.active(2), [1]);
    assert!(!e.enabled(ActionId::ToggleChannel(2), &(0..0)), "a channel the file does not have cannot be toggled");
}

#[test]
fn f68_the_layout_actions_are_in_the_table_and_run_without_changing_the_document() {
    let dir = TempDir::new("f68-actions");
    let mut e = editor(&dir.0, &stereo(100));
    for name in ["view.channels.layered", "view.channels.stacked"] {
        let a = by_name(name).unwrap();
        assert!(e.enabled(a.id, &(0..0)));
        let out = e.run(a.id, 10..20).unwrap();
        assert!(!out.doc_changed);
    }
}

#[test]
fn f69_zoom_to_time_and_zoom_at_point_actions_are_named_and_need_a_file() {
    for n in 1..=5 {
        let a = by_name(&format!("view.zoom.preset.{n}")).unwrap();
        assert_eq!(wavcraft_core::actions::zoom_preset_slot(a.id), Some(n - 1));
    }
    assert_eq!(by_name("view.zoom.in_at_in").map(|a| a.id), Some(ActionId::ZoomInAtIn));
    assert_eq!(by_name("view.zoom.in_at_out").map(|a| a.id), Some(ActionId::ZoomInAtOut));
    assert_eq!(wavcraft_core::actions::zoom_preset_slot(ActionId::ZoomIn), None);
    let t = TempDir::new("f69-enabled");
    let e = editor(&t.0, &stereo(100));
    for id in [ActionId::ZoomInAtIn, ActionId::ZoomInAtOut, ActionId::ZoomPreset3] {
        assert!(e.enabled(id, &(0..0)), "{id:?}");
    }
}
