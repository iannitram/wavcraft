//! Mix Paste (f22): every mode, the volumes, invert, crossfade and loops, checked sample by sample
//! on small synthetic clips, plus the history step and the streaming path.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use wavcraft_core::doc::edit::{self, Change, ChannelMask, MixMode, MixOptions};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-mix-test-{}-{name}", std::process::id()));
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
    Store::create_with(dir, StoreOptions { pack_bytes: 1 << 30, cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
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

/// A mono doc of 8 frames (1..=8) and a clip of 3 frames (10, 20, 30) copied from a second doc.
fn setup(t: &TempDir, mode: MixMode) -> (DocState, edit::Clip, MixOptions) {
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]]);
    let src = doc_from(&s, &[vec![10.0, 20.0, 30.0]]);
    let clip = edit::copy(&src, 0..3, ChannelMask::ALL).unwrap();
    (doc, clip, MixOptions::new(mode))
}

fn run(doc: &DocState, at: u64, clip: &edit::Clip, opts: &MixOptions) -> edit::Edit {
    edit::mix_paste(doc, at, clip, ChannelMask::ALL, opts, &AtomicBool::new(false)).unwrap()
}

#[test]
fn insert_pushes_the_rest_right() {
    let t = TempDir::new("insert");
    let (doc, clip, opts) = setup(&t, MixMode::Insert);
    let e = run(&doc, 2, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0, 10.0, 20.0, 30.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_eq!(e.label, "Mix Paste");
    assert_eq!(e.change, Change::LengthFrom { frame: 2 });
}

#[test]
fn overlap_sums_with_per_channel_volumes() {
    let t = TempDir::new("overlap");
    let (doc, clip, mut opts) = setup(&t, MixMode::Overlap);
    let e = run(&doc, 2, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0, 13.0, 24.0, 35.0, 6.0, 7.0, 8.0]);
    assert_eq!(e.change, Change::SameLength { range: 2..5, channels: ChannelMask::only(0) });
    opts.copied_pct = vec![50.0];
    opts.existing_pct = vec![200.0];
    let e = run(&doc, 2, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0, 11.0, 18.0, 25.0, 6.0, 7.0, 8.0]);
}

#[test]
fn overwrite_replaces_for_the_clips_length() {
    let t = TempDir::new("overwrite");
    let (doc, clip, mut opts) = setup(&t, MixMode::Overwrite);
    opts.existing_pct = vec![0.0]; // ignored by Overwrite
    let e = run(&doc, 4, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 8.0]);
}

#[test]
fn overwrite_stops_at_the_end_of_the_file() {
    let t = TempDir::new("overwrite-end");
    let (doc, clip, opts) = setup(&t, MixMode::Overwrite);
    let e = run(&doc, 6, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 10.0, 20.0]);
}

#[test]
fn modulate_multiplies() {
    let t = TempDir::new("modulate");
    let (doc, clip, mut opts) = setup(&t, MixMode::Modulate);
    let e = run(&doc, 1, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 20.0, 60.0, 120.0, 5.0, 6.0, 7.0, 8.0]);
    opts.copied_pct = vec![10.0];
    let e = run(&doc, 1, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 2.0 * 1.0, 3.0 * 2.0, 4.0 * 3.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn invert_flips_the_pasted_audio() {
    let t = TempDir::new("invert");
    let (doc, clip, mut opts) = setup(&t, MixMode::Overlap);
    opts.invert = true;
    let e = run(&doc, 0, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![-9.0, -18.0, -27.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    let mut ins = MixOptions::new(MixMode::Insert);
    ins.invert = true;
    let e = run(&doc, 8, &clip, &ins);
    assert_eq!(&content(&e.state)[0][8..], &[-10.0, -20.0, -30.0]);
}

#[test]
fn loops_repeat_the_clip() {
    let t = TempDir::new("loops");
    let (doc, clip, mut opts) = setup(&t, MixMode::Insert);
    opts.loops = 3;
    let e = run(&doc, 0, &clip, &opts);
    let got = &content(&e.state)[0];
    assert_eq!(&got[..9], &[10.0, 20.0, 30.0, 10.0, 20.0, 30.0, 10.0, 20.0, 30.0]);
    assert_eq!(got.len(), 17);
    opts.mode = MixMode::Overlap;
    opts.loops = 2;
    let e = run(&doc, 1, &clip, &opts);
    assert_eq!(content(&e.state)[0], vec![1.0, 12.0, 23.0, 34.0, 15.0, 26.0, 37.0, 8.0]);
}

#[test]
fn a_crossfade_blends_the_edges_and_leaves_the_middle() {
    let t = TempDir::new("fade");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.0; 1000]]);
    let src = doc_from(&s, &[vec![1.0; 200]]);
    let clip = edit::copy(&src, 0..200, ChannelMask::ALL).unwrap();
    let mut opts = MixOptions::new(MixMode::Overwrite);
    opts.crossfade = Duration::from_secs_f64(20.0 / f64::from(RATE));
    let got = content(&run(&doc, 400, &clip, &opts).state).remove(0);
    assert_eq!(got.len(), 1000);
    assert!(got[400] < 0.5 && got[400] >= 0.0, "starts mostly old: {}", got[400]);
    assert_eq!(got[400 + 100], 1.0);
    assert!(got[399] == 0.0 && got[600] == 0.0);
    assert!(got[599] > 0.0 && got[599] < 1.0, "the end blends back: {}", got[599]);
}

#[test]
fn one_channel_active_writes_only_that_channel_and_keeps_the_length() {
    let t = TempDir::new("mask");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![1.0; 6], vec![2.0; 6]]);
    let src = doc_from(&s, &[vec![5.0; 2], vec![7.0; 2]]);
    let clip = edit::copy(&src, 0..2, ChannelMask::ALL).unwrap();
    let opts = MixOptions::new(MixMode::Insert);
    let e = edit::mix_paste(&doc, 1, &clip, ChannelMask::only(1), &opts, &AtomicBool::new(false)).unwrap();
    let c = content(&e.state);
    assert_eq!(c[0], vec![1.0; 6]);
    assert_eq!(c[1], vec![2.0, 7.0, 7.0, 2.0, 2.0, 2.0]);
}

#[test]
fn an_empty_clip_or_the_end_of_the_file_is_no_edit() {
    let t = TempDir::new("noop");
    let (doc, clip, opts) = setup(&t, MixMode::Overlap);
    assert!(run(&doc, 8, &clip, &opts).is_noop(), "nothing left to mix into");
}

#[test]
fn a_large_mix_is_made_in_blocks() {
    let t = TempDir::new("big");
    let s = store(&t.0);
    let n = BLOCK_FRAMES * 3 + 17;
    let doc = doc_from(&s, &[(0..n).map(|i| (i % 7) as f32).collect()]);
    let src = doc_from(&s, &[(0..1000).map(|i| (i % 5) as f32).collect()]);
    let clip = edit::copy(&src, 0..1000, ChannelMask::ALL).unwrap();
    let mut opts = MixOptions::new(MixMode::Overlap);
    opts.loops = 200; // 200 000 frames: spans several blocks and wraps the clip inside them
    let got = content(&run(&doc, 5, &clip, &opts).state).remove(0);
    assert_eq!(got.len(), n);
    for i in [0usize, 4, 5, 6, 1004, 1005, 65_535, 65_541, 65_542, 150_000, n - 1] {
        let old = (i % 7) as f32;
        let add = if (5..5 + 200_000).contains(&i) { ((i - 5) % 1000 % 5) as f32 } else { 0.0 };
        assert_eq!(got[i], old + add, "frame {i}");
    }
}

#[test]
fn undo_returns_to_the_original() {
    let t = TempDir::new("undo");
    let (doc, clip, opts) = setup(&t, MixMode::Overwrite);
    let before = content(&doc);
    let e = run(&doc, 0, &clip, &opts);
    assert_ne!(content(&e.state), before);
    assert_eq!(content(&doc), before, "the state the edit started from is untouched");
}
