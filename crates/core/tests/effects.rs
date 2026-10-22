//! The effect seam (f01, B001/B166/B167): the registry, the action table entries, undo labels,
//! and processing on the selection, the whole file or only the enabled channels. Invert is the
//! reference effect.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, TABLE, action, all_actions, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-effects-test-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn now(e: &Editor) -> Vec<Vec<f32>> {
    let doc = e.history.current();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// Left k + 1, right -(k + 1): never zero, so an inverted sample always differs.
fn stereo(frames: usize) -> Vec<Vec<f32>> {
    vec![(0..frames).map(|k| k as f32 + 1.0).collect(), (0..frames).map(|k| -(k as f32) - 1.0).collect()]
}

fn invert() -> ActionId {
    by_name("effect.invert").expect("invert is in the action table").id
}

#[test]
fn the_registry_lists_invert_and_the_action_table_finds_it() {
    assert!(effects::all().iter().any(|e| e.id() == "invert" && e.name() == "Invert"));
    let a = by_name("effect.invert").unwrap();
    assert_eq!(a.menu, "Invert");
    assert_eq!(action(a.id).name, "effect.invert");
    assert!(all_actions().any(|x| x.id == a.id));
    // The fixed table is untouched; effects come through all_actions() and by_name().
    assert!(TABLE.iter().all(|t| !t.name.starts_with("effect.")));
    let mut names: Vec<_> = all_actions().map(|a| a.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all_actions().count(), "action names are unique");
}

#[test]
fn invert_flips_the_selection_only_and_the_undo_step_is_named_for_the_effect() {
    let tmp = TempDir::new("sel");
    let mut e = editor(&tmp.0, &stereo(1000));
    let before = now(&e);
    let out = e.run(invert(), 100..200).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 100..200);
    assert_eq!(e.history.undo_label(), Some("Invert"));
    assert_eq!(e.label(ActionId::Undo), "Undo Invert");
    let after = now(&e);
    for c in 0..2 {
        for k in 0..1000 {
            let want = if (100..200).contains(&k) { -before[c][k] } else { before[c][k] };
            assert_eq!(after[c][k], want, "ch {c} frame {k}");
        }
    }
    e.run(ActionId::Undo, 100..200).unwrap();
    assert_eq!(now(&e), before);
}

#[test]
fn with_nothing_selected_the_whole_file_is_processed() {
    let tmp = TempDir::new("whole");
    let mut e = editor(&tmp.0, &stereo(3000));
    let before = now(&e);
    let out = e.run(invert(), 500..500).unwrap();
    assert!(out.doc_changed);
    let after = now(&e);
    for c in 0..2 {
        assert!(after[c].iter().zip(&before[c]).all(|(a, b)| *a == -*b));
    }
}

#[test]
fn only_the_enabled_channels_are_processed() {
    let tmp = TempDir::new("chan");
    let mut e = editor(&tmp.0, &stereo(2000));
    let before = now(&e);
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    e.run(invert(), 0..2000).unwrap();
    let after = now(&e);
    assert!(after[0].iter().zip(&before[0]).all(|(a, b)| *a == -*b), "left inverted");
    assert_eq!(after[1], before[1], "right untouched");
}

#[test]
fn invert_twice_is_the_original_and_works_across_blocks() {
    let tmp = TempDir::new("twice");
    let frames = BLOCK_FRAMES * 2 + 17;
    let mut e = editor(&tmp.0, &stereo(frames));
    let before = now(&e);
    e.run(invert(), 10..frames as u64 - 10).unwrap();
    e.run(invert(), 10..frames as u64 - 10).unwrap();
    assert_eq!(now(&e), before);
    assert_eq!(e.history.current().len_frames(), frames as u64);
}

#[test]
fn repeat_last_command_repeats_an_effect() {
    let tmp = TempDir::new("repeat");
    let mut e = editor(&tmp.0, &stereo(1000));
    let before = now(&e);
    e.run(invert(), 0..100).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Invert");
    e.run(ActionId::RepeatLast, 0..100).unwrap();
    assert_eq!(now(&e), before, "the repeat inverted the same range back");
}

#[test]
fn a_cancelled_run_changes_nothing() {
    let tmp = TempDir::new("cancel");
    let e = editor(&tmp.0, &stereo(1000));
    let cancel = std::sync::atomic::AtomicBool::new(true);
    let inv = effects::find("invert").unwrap();
    let r = effects::apply(
        inv,
        &effects::Params::defaults(inv),
        e.history.current(),
        0..1000,
        wavcraft_core::ChannelMask::ALL,
        Crossfades::NONE,
        &cancel,
    );
    assert!(matches!(r, Err(wavcraft_core::EditError::Cancelled)));
}

#[test]
fn an_empty_document_is_refused_by_enabled() {
    let tmp = TempDir::new("empty");
    let e = editor(&tmp.0, &[vec![], vec![]]);
    assert!(!e.enabled(invert(), &(0..0)));
}

/// Copies its input and records the `first_frame` of every chunk it is given, in order.
struct Recorder(std::sync::Mutex<Vec<u64>>);

impl effects::Effect for Recorder {
    fn id(&self) -> &'static str {
        "recorder"
    }
    fn name(&self) -> &'static str {
        "Recorder"
    }
    fn process(&self, _params: &[f64], first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> std::io::Result<()> {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(first_frame);
        }
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = *a;
            }
        }
        Ok(())
    }
}

/// Runs [`Recorder`] over `range` of a stereo document of `frames` frames and returns the
/// `first_frame` it was given for each chunk.
fn recorded_first_frames(name: &str, range: std::ops::Range<u64>, frames: usize) -> Vec<u64> {
    let tmp = TempDir::new(name);
    let e = editor(&tmp.0, &stereo(frames));
    let rec = Recorder(std::sync::Mutex::new(Vec::new()));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    effects::apply(&rec, &[], e.history.current(), range, wavcraft_core::ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    rec.0.into_inner().unwrap()
}

#[test]
fn a_chunk_gets_its_absolute_first_frame_inside_a_selection() {
    // Chunks start at start, start + BLOCK_FRAMES and start + 2 * BLOCK_FRAMES (the last one 17 frames).
    let start = 1_000u64;
    let block = BLOCK_FRAMES as u64;
    let range = start..start + 2 * block + 17;
    let seen = recorded_first_frames("first-frame-sel", range, (start + 2 * block + 17 + 500) as usize);
    assert_eq!(seen, vec![start, start + block, start + 2 * block]);
}

#[test]
fn a_selection_from_frame_zero_gets_frames_from_zero() {
    let block = BLOCK_FRAMES as u64;
    let seen = recorded_first_frames("first-frame-zero", 0..2 * block + 17, (2 * block + 17) as usize);
    assert_eq!(seen, vec![0, block, 2 * block]);
}
