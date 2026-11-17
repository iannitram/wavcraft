//! L4.3 (#1060): `transcript::delete_ranges` cuts several ranges as one edit, last to first, the
//! way Delete Silence does, so removing every filler is one History step.

use std::ops::Range;
use std::path::{Path, PathBuf};

use wavcraft_core::doc::edit::{self, Change, ChannelMask, Crossfades, EditError};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::transcript::delete_ranges;

const RATE: u32 = 48_000;
const LABEL: &str = "Remove Fillers";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-l4-3-test-{}-{name}", std::process::id()));
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

fn bits(data: &[Vec<f32>]) -> Vec<Vec<u32>> {
    data.iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect()
}

/// Stereo, 2 s: left a ramp, right the ramp negated, so every frame is different.
fn ramp_stereo() -> Vec<Vec<f32>> {
    let left: Vec<f32> = (0..2 * RATE as usize).map(|i| i as f32 / 96_000.0).collect();
    let right: Vec<f32> = left.iter().map(|x| -x).collect();
    vec![left, right]
}

/// Mono, 2 s, full-scale 1 kHz sine.
fn sine_1k() -> Vec<Vec<f32>> {
    vec![(0..2 * RATE as usize).map(|i| (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / f64::from(RATE)).sin() as f32).collect()]
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// The cuts done one by one with `edit::delete` on every channel, last to first.
fn by_hand(doc: &DocState, ranges: &[Range<u64>], xf: Crossfades) -> DocState {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut state = doc.clone();
    for r in sorted.iter().rev() {
        state = edit::delete(&state, r.clone(), ChannelMask::ALL, xf).unwrap().state;
    }
    state
}

fn fillers() -> Vec<Range<u64>> {
    vec![1_000..2_000, 10_000..12_000, 30_000..30_500]
}

// Catches: each range pushed as its own step, or the label not passed through.
#[test]
fn removing_all_fillers_is_one_history_step() {
    let dir = TempDir::new("one-step");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let mut h = History::new(doc, u64::MAX);
    let before = h.steps().len();
    let e = delete_ranges(h.current(), &fillers(), Crossfades::NONE, LABEL).unwrap();
    assert_eq!(e.label, LABEL);
    h.push_edit(e);
    assert_eq!(h.steps().len(), before + 1);
    assert_eq!(h.steps()[before].label(), LABEL);
}

// Catches: cuts applied first to last (later ranges then cut the wrong frames), or not through
// `edit::delete` on every channel.
#[test]
fn the_result_equals_three_deletes_last_to_first() {
    let dir = TempDir::new("equal");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let e = delete_ranges(&doc, &fillers(), Crossfades::NONE, LABEL).unwrap();
    assert_eq!(e.state.len_frames(), 96_000 - 3_500);
    assert_eq!(bits(&content(&e.state)), bits(&content(&by_hand(&doc, &fillers(), Crossfades::NONE))));
}

// Catches: the crossfades argument ignored (Crossfades::NONE used inside).
#[test]
fn the_crossfades_given_are_used() {
    let dir = TempDir::new("xf");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let xf = Crossfades::default();
    let e = delete_ranges(&doc, &fillers(), xf, LABEL).unwrap();
    assert_eq!(bits(&content(&e.state)), bits(&content(&by_hand(&doc, &fillers(), xf))));
    assert_ne!(bits(&content(&e.state)), bits(&content(&by_hand(&doc, &fillers(), Crossfades::NONE))));
}

// Catches: ranges used in the order given instead of sorted by start first.
#[test]
fn ranges_in_any_order_give_the_same_result() {
    let dir = TempDir::new("order");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let shuffled = [30_000..30_500, 1_000..2_000, 10_000..12_000];
    let e = delete_ranges(&doc, &shuffled, Crossfades::NONE, LABEL).unwrap();
    assert_eq!(bits(&content(&e.state)), bits(&content(&by_hand(&doc, &fillers(), Crossfades::NONE))));
}

// Catches: a click at a splice (the default 2 ms join crossfade not applied).
#[test]
fn no_click_at_any_filler_splice() {
    let dir = TempDir::new("click");
    let data = sine_1k();
    let doc = doc_from(&store(&dir.0), &data);
    let ranges = [24_000..26_400, 48_000..49_000, 72_013..73_500];
    let e = delete_ranges(&doc, &ranges, Crossfades::default(), LABEL).unwrap();
    let got = max_step(&content(&e.state)[0]);
    let clean = max_step(&data[0]);
    assert!(got <= 1.035 * clean, "max step {got} against {clean} for the untouched sine");
}

// Catches: the change not starting at the lowest range, so the views redraw too little.
#[test]
fn the_change_starts_at_the_first_range() {
    let dir = TempDir::new("change");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let shuffled = [30_000..30_500, 10_000..12_000, 1_000..2_000];
    let e = delete_ranges(&doc, &shuffled, Crossfades::NONE, LABEL).unwrap();
    assert_eq!(e.change, Change::LengthFrom { frame: 1_000 });
}

// Catches: an empty list (or only empty ranges) making a step that changes something.
#[test]
fn no_ranges_is_no_edit() {
    let dir = TempDir::new("none");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let e = delete_ranges(&doc, &[], Crossfades::NONE, LABEL).unwrap();
    assert!(e.is_noop());
    assert_eq!(e.label, LABEL);
    assert_eq!(e.state.len_frames(), 96_000);
    let e = delete_ranges(&doc, &[5_000..5_000, 7_000..7_000], Crossfades::NONE, LABEL).unwrap();
    assert!(e.is_noop());
    assert_eq!(e.state.len_frames(), 96_000);
}

// Catches: an empty range among real ones changing the result or the change frame.
#[test]
fn empty_ranges_are_skipped() {
    let dir = TempDir::new("skip");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let with_empty = [500..500, 1_000..2_000, 10_000..12_000, 30_000..30_500];
    let e = delete_ranges(&doc, &with_empty, Crossfades::NONE, LABEL).unwrap();
    assert_eq!(e.change, Change::LengthFrom { frame: 1_000 });
    assert_eq!(bits(&content(&e.state)), bits(&content(&by_hand(&doc, &fillers(), Crossfades::NONE))));
}

// Catches: overlapping ranges cut twice (removing audio outside both), or touching ranges refused.
#[test]
fn overlapping_ranges_are_refused() {
    let dir = TempDir::new("overlap");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let r = delete_ranges(&doc, &[2_000..4_000, 1_000..3_000], Crossfades::NONE, LABEL);
    assert!(matches!(r, Err(EditError::Unsupported(_))), "{r:?}");
    let e = delete_ranges(&doc, &[1_000..2_000, 2_000..3_000], Crossfades::NONE, LABEL).unwrap();
    assert_eq!(e.state.len_frames(), 96_000 - 2_000);
}

// Catches: a range past the end being clamped or ignored instead of refused.
#[test]
fn a_range_past_the_end_is_refused() {
    let dir = TempDir::new("past-end");
    let doc = doc_from(&store(&dir.0), &ramp_stereo());
    let r = delete_ranges(&doc, &[1_000..2_000, 95_000..97_000], Crossfades::NONE, LABEL);
    assert!(matches!(r, Err(EditError::OutOfRange { .. })), "{r:?}");
}

// Catches: undo of the step not giving back the original samples exactly.
#[test]
fn undo_is_bit_identical() {
    let dir = TempDir::new("undo");
    let data = ramp_stereo();
    let doc = doc_from(&store(&dir.0), &data);
    let mut h = History::new(doc, u64::MAX);
    let e = delete_ranges(h.current(), &fillers(), Crossfades::default(), LABEL).unwrap();
    h.push_edit(e);
    let undone = h.undo().unwrap();
    assert_eq!(bits(&content(&undone)), bits(&data));
}
