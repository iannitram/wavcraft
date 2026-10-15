//! Task 04 acceptance tests on kit files: test 1 on `ramp`, 9b and 9c (one-channel Delete and Cut
//! on `lr-different`), and test 10, the long test (`#[ignore]`: run it explicitly, in release,
//! through the build gate, with the 3-hour kit file present).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{DocState, history::OPEN};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_testkit::*;

const NONE: Crossfades = Crossfades::NONE;

/// The kit, written once per run into the temp folder.
fn kit() -> &'static Path {
    static KIT: OnceLock<PathBuf> = OnceLock::new();
    KIT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("wavcraft-edit-kit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_kit(&dir, &KitOptions::default()).unwrap();
        dir
    })
}

fn sessions() -> PathBuf {
    std::env::temp_dir().join(format!("wavcraft-edit-sessions-{}", std::process::id()))
}

fn load(path: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<u32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v.iter().map(|x| x.to_bits()).collect()
        })
        .collect()
}

/// Test 1 on the `ramp` file: every edit at the start, middle and end, across block edges, a
/// 1-frame range and the whole file, crossfades 0, against the ramp's own formula.
#[test]
fn edits_on_the_ramp_match_the_model() {
    let doc = load(&kit().join(RAMP));
    let len = doc.len_frames();
    let model: Vec<f32> = (0..len).map(|n| ramp_value(n) as f32 / 32_768.0).collect();
    assert_eq!(content(&doc)[0], model.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    let b = 65_536u64;
    let ranges: Vec<Range<u64>> =
        vec![0..10_000, 200_000..260_000, len - 10_000..len, b - 100..b + 100, b..3 * b, 2 * b - 1..2 * b + 1, 12_345..12_346, len - 1..len, 0..len];
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
    let clip = edit::copy(&doc, 1_000..70_000, ChannelMask::ALL).unwrap();
    let clip_model = model[1_000..70_000].to_vec();
    for r in ranges {
        let ru = r.start as usize..r.end as usize;
        let mut m = model.clone();
        m.drain(ru.clone());
        assert_eq!(content(&edit::delete(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap().state)[0], bits(&m), "delete {r:?}");
        let (ed, c) = edit::cut(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap();
        assert_eq!(content(&ed.state)[0], bits(&m), "cut {r:?}");
        let mut cm = vec![f32::NAN; c.len_frames() as usize];
        c.read(0, 0, &mut cm).unwrap();
        assert_eq!(bits(&cm), bits(&model[ru.clone()]), "cut clip {r:?}");

        let mut m = model.clone();
        m[ru.clone()].fill(0.0);
        assert_eq!(content(&edit::silence(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap().state)[0], bits(&m), "silence {r:?}");

        let mut m = model.clone();
        m.splice(ru.clone(), clip_model.iter().copied());
        assert_eq!(content(&edit::paste(&doc, PasteAt::Replace(r.clone()), &clip, NONE).unwrap().state)[0], bits(&m), "paste over {r:?}");

        let mut m = model.clone();
        m.splice(ru.start..ru.start, clip_model.iter().copied());
        assert_eq!(content(&edit::paste(&doc, PasteAt::Insert(r.start), &clip, NONE).unwrap().state)[0], bits(&m), "paste at {r:?}");

        let mut m = model.clone();
        m.splice(ru.end..ru.end, std::iter::repeat_n(0.0, 1_001));
        assert_eq!(content(&edit::insert_silence(&doc, r.end, 1_001, NONE).unwrap().state)[0], bits(&m), "insert silence {r:?}");

        let m = model[ru.clone()].to_vec();
        if !r.is_empty() {
            assert_eq!(content(&edit::crop(&doc, r.clone()).unwrap().state)[0], bits(&m), "crop {r:?}");
        }

        let m: Vec<f32> = model.iter().enumerate().map(|(i, x)| if ru.contains(&i) { x * 0.5 } else { *x }).collect();
        let ed = edit::replace_range(&doc, r.clone(), ChannelMask::ALL, "Gain", NONE, &AtomicBool::new(false), |_, i, o| {
            for (a, b) in i[0].iter().zip(o[0].iter_mut()) {
                *b = a * 0.5;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(content(&ed.state)[0], bits(&m), "replace {r:?}");
    }
}

/// Test 9b: Delete with only the left channel active on `lr-different`, crossfades 0: the length
/// stays, the left range is zeros, the right channel is bit-identical, History says "Delete
/// Audio", and undo is bit-identical to before.
#[test]
fn one_channel_delete_silences_that_channel() {
    let doc = load(&kit().join(LR));
    let before = content(&doc);
    let r = 48_000..96_000u64;
    let mut h = History::new(doc.clone(), u64::MAX);
    let ed = edit::delete(&doc, r.clone(), ChannelMask::only(0), NONE).unwrap();
    assert_eq!(ed.label, "Delete Audio");
    h.push_edit(ed);
    assert_eq!(h.undo_label(), Some("Delete Audio"));
    let after = h.current().clone();
    assert_eq!(after.len_frames(), doc.len_frames());
    let got = content(&after);
    assert!(got[0][r.start as usize..r.end as usize].iter().all(|x| *x == 0));
    assert_eq!(got[0][..r.start as usize], before[0][..r.start as usize]);
    assert_eq!(got[0][r.end as usize..], before[0][r.end as usize..]);
    assert_eq!(got[1], before[1]);
    assert!(Arc::ptr_eq(&after.channels()[1], &doc.channels()[1]), "the right channel is shared");
    h.undo().unwrap();
    assert_eq!(h.steps()[0].label(), OPEN);
    assert_eq!(content(h.current()), before);
}

/// Test 9c: Cut with only the left channel active: the clip holds only the left range, and the
/// document is as in 9b.
#[test]
fn one_channel_cut_copies_that_channel_and_silences_it() {
    let doc = load(&kit().join(LR));
    let before = content(&doc);
    let r = 48_000..96_000u64;
    let (ed, clip) = edit::cut(&doc, r.clone(), ChannelMask::only(0), NONE).unwrap();
    assert_eq!(clip.channel_count(), 1);
    let mut c = vec![f32::NAN; clip.len_frames() as usize];
    clip.read(0, 0, &mut c).unwrap();
    assert_eq!(c.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), before[0][r.start as usize..r.end as usize]);
    let as_9b = edit::delete(&doc, r, ChannelMask::only(0), NONE).unwrap();
    assert_eq!(content(&ed.state), content(&as_9b.state));
    assert_eq!(ed.state.len_frames(), doc.len_frames());
}

/// Where the 3-hour kit file is: `WAVCRAFT_KIT` or Hyperion's kit folder.
fn long_file() -> PathBuf {
    std::env::var_os("WAVCRAFT_KIT").map_or_else(|| PathBuf::from("B:/dev/craft/wavcraft-testkit-out"), PathBuf::from).join(LONG)
}

/// Test 10, the long test: on the 3-hour file, 200 random edits (default crossfades), then undo
/// all: bit-identical to the opened file. Prints the times.
#[test]
#[ignore = "needs the 3-hour kit file; run explicitly in release through the build gate"]
fn long_file_200_random_edits_then_undo_all() {
    let path = long_file();
    let total = Instant::now();
    let t = Instant::now();
    let doc = load(&path);
    println!("open: {:.2} s, {} frames", t.elapsed().as_secs_f64(), doc.len_frames());
    let t = Instant::now();
    let want = content_hash(&doc).unwrap();
    println!("hash of the opened document: {:.2} s", t.elapsed().as_secs_f64());
    let mut h = History::new(doc, History::default_cap(&sessions()));
    let mut rng = Rng::new(DEFAULT_SEED, "task-04-long");
    let mut clipboard = None;
    let mut edits: Vec<RandomEdit> = Vec::new();
    for _ in 0..200 {
        edits.push(random_edit(&mut h, &mut rng, Crossfades::default(), 60 * 48_000, &mut clipboard).unwrap());
    }
    report(&edits);
    println!("steps kept: {}, dropped by the cap: {}, history holds {} beyond the current state", h.steps().len(), h.dropped_total(), h.held_bytes());
    let t = Instant::now();
    let mut undos = Vec::new();
    loop {
        let u = Instant::now();
        if h.undo().is_none() {
            break;
        }
        undos.push(u.elapsed());
    }
    undos.sort();
    println!("undo all ({}): {:.3} ms total, slowest {:?}", undos.len(), t.elapsed().as_secs_f64() * 1e3, undos.last());
    assert_eq!(h.dropped_total(), 0, "the cap dropped steps, so undo can't reach the opened file");
    let t = Instant::now();
    assert_eq!(content_hash(h.current()).unwrap(), want, "undo all is not bit-identical to the opened file");
    println!("hash after undo all: {:.2} s", t.elapsed().as_secs_f64());
    println!("test 10 total: {:.1} s", total.elapsed().as_secs_f64());
}

fn report(edits: &[RandomEdit]) {
    let mut by: std::collections::BTreeMap<&str, Vec<Duration>> = std::collections::BTreeMap::new();
    for e in edits {
        by.entry(e.label.as_str()).or_default().push(e.edit);
    }
    for (label, mut times) in by {
        times.sort();
        println!("  {label:>14}: {:3} edits, median {:?}, slowest {:?}", times.len(), times[times.len() / 2], times.last().unwrap());
    }
    let mut push: Vec<Duration> = edits.iter().map(|e| e.push).collect();
    push.sort();
    println!(
        "  push: median {:?}, slowest {:?}; with 199 steps behind: {:?}",
        push[push.len() / 2],
        push.last().unwrap(),
        edits.last().unwrap().push
    );
}
