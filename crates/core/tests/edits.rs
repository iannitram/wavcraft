//! Task 04 acceptance tests on synthetic data: the edits against a reference model (tests 1, 2),
//! undo and redo (3-5), the disk cap (6), silence writes nothing (7), `replace_range` streams and
//! cancels (7b), crossfades (8, 9d), paste channel rules (9), one-channel crop and paste, the empty-range
//! regression (W1), and the history's own behaviour.
//! The kit-file tests (1 on `ramp`, 9b, 9c, 10) are in `crates/testkit/tests/edits.rs`.

use std::collections::HashSet;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use proptest::prelude::*;
use wavcraft_core::doc::edit::{self, Change, ChannelMask, Clip, Crossfades, EditError, PasteAt};
use wavcraft_core::doc::history::{History, NoSpace};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, BlockId, Store, StoreOptions};

const RATE: u32 = 48_000;
const NONE: Crossfades = Crossfades::NONE;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-edit-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path, pack_bytes: u64) -> Store {
    Store::create_with(dir, StoreOptions { pack_bytes, cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

/// A document holding `data` (one `Vec` per channel), written in blocks of `block` frames.
fn doc_from(s: &Store, data: &[Vec<f32>], block: usize) -> DocState {
    let chans = data
        .iter()
        .map(|c| Channel::from_pieces(c.chunks(block.clamp(1, BLOCK_FRAMES)).map(|b| Piece::whole(s.append(b).unwrap()).unwrap())))
        .collect();
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

fn clip_content(clip: &Clip) -> Vec<Vec<f32>> {
    (0..clip.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; clip.len_frames() as usize];
            assert_eq!(clip.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn bits(v: &[Vec<f32>]) -> Vec<Vec<u32>> {
    v.iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect()
}

/// Distinct test values, exact in f32.
fn data(channels: usize, frames: usize, seed: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| (0..frames).map(|i| (((i * 31 + c * 7_919 + seed * 104_729) % 1_000_003) as f32 - 500_000.0) / 524_288.0).collect())
        .collect()
}

fn sine(frames: usize, phase: f64) -> Vec<f32> {
    (0..frames).map(|n| (std::f64::consts::TAU * 1_000.0 * n as f64 / f64::from(RATE) + phase).sin() as f32).collect()
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// The reference model: plain vectors, the same operations written the obvious way.
#[derive(Clone, Debug, PartialEq)]
struct Model(Vec<Vec<f32>>);

impl Model {
    fn len(&self) -> usize {
        self.0[0].len()
    }

    fn delete(&mut self, r: Range<usize>, mask: ChannelMask) {
        if mask.covers_all(self.0.len()) {
            for c in &mut self.0 {
                c.drain(r.clone());
            }
        } else {
            self.silence(r, mask);
        }
    }

    fn copy(&self, r: Range<usize>, mask: ChannelMask) -> Vec<Vec<f32>> {
        self.0.iter().enumerate().filter(|(c, _)| mask.contains(*c)).map(|(_, v)| v[r.clone()].to_vec()).collect()
    }

    /// Every channel in `mask`: insert (or replace `r`). Only some: overwrite those channels from
    /// `r.start` for the clip's length, stopping at the end; the length stays the same.
    fn paste(&mut self, r: Range<usize>, clip: &[Vec<f32>], mask: ChannelMask) {
        if clip.is_empty() || clip[0].is_empty() {
            return;
        }
        let n = self.0.len();
        if !mask.covers_all(n) {
            let len = self.len();
            let written = clip[0].len().min(len - r.start);
            for (c, ch) in self.0.iter_mut().enumerate() {
                if !mask.contains(c) {
                    continue;
                }
                let src = if clip.len() == n { &clip[c] } else { &clip[0] };
                ch[r.start..r.start + written].copy_from_slice(&src[..written]);
            }
            return;
        }
        let src: Vec<Vec<f32>> = if clip.len() == n {
            clip.to_vec()
        } else if clip.len() == 1 {
            vec![clip[0].clone(); n]
        } else {
            let scale = 1.0 / clip.len() as f32;
            let mut acc = clip[0].clone();
            for c in &clip[1..] {
                for (a, b) in acc.iter_mut().zip(c) {
                    *a += *b;
                }
            }
            vec![acc.iter().map(|a| a * scale).collect()]
        };
        for (c, s) in self.0.iter_mut().zip(src) {
            c.splice(r.clone(), s);
        }
    }

    fn silence(&mut self, r: Range<usize>, mask: ChannelMask) {
        for (i, c) in self.0.iter_mut().enumerate() {
            if mask.contains(i) {
                c[r.clone()].fill(0.0);
            }
        }
    }

    fn insert_silence(&mut self, at: usize, frames: usize) {
        for c in &mut self.0 {
            c.splice(at..at, std::iter::repeat_n(0.0, frames));
        }
    }

    /// Crop of an empty range is no edit: Audition greys Crop out with nothing selected and Ctrl+T
    /// leaves the file as it is (`plan/tasks/audition-check.md`, 2026-10-16 Result, item 6).
    /// Every channel in `mask`: keep only `r`. Only some (Audition's one-channel Crop, the same
    /// Result, item 4a): silence everything outside `r` in those channels, same length.
    fn crop(&mut self, r: Range<usize>, mask: ChannelMask) {
        if r.is_empty() {
            return;
        }
        if !mask.covers_all(self.0.len()) {
            for (i, c) in self.0.iter_mut().enumerate() {
                if mask.contains(i) {
                    c[..r.start].fill(0.0);
                    c[r.end..].fill(0.0);
                }
            }
            return;
        }
        for c in &mut self.0 {
            *c = c[r.clone()].to_vec();
        }
    }

    fn replace(&mut self, r: Range<usize>, mask: ChannelMask) {
        for (i, c) in self.0.iter_mut().enumerate() {
            if mask.contains(i) {
                for x in &mut c[r.clone()] {
                    *x = effect(*x);
                }
            }
        }
    }
}

/// The test effect: exact in f32.
fn effect(x: f32) -> f32 {
    x * -0.5 + 0.25
}

fn effect_producer(_at: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
    for (i, o) in input.iter().zip(output.iter_mut()) {
        for (a, b) in i.iter().zip(o.iter_mut()) {
            *b = effect(*a);
        }
    }
    Ok(())
}

fn replace(doc: &DocState, r: Range<u64>, mask: ChannelMask, xf: Crossfades) -> edit::Edit {
    edit::replace_range(doc, r, mask, "Effect", xf, &AtomicBool::new(false), effect_producer).unwrap()
}

/// One random edit, its numbers scaled to the document when applied.
#[derive(Debug, Clone)]
struct Op {
    kind: u8,
    a: u32,
    b: u32,
    mask: u8,
}

fn op() -> impl Strategy<Value = Op> {
    (0u8..8, any::<u32>(), any::<u32>(), 1u8..4).prop_map(|(kind, a, b, mask)| Op { kind, a, b, mask })
}

/// Apply `op` to the document (pushing it on the history) and to the model.
fn apply(op: &Op, h: &mut History, model: &mut Model, clipboard: &mut Option<(Clip, Vec<Vec<f32>>)>) {
    let doc = h.current().clone();
    let len = doc.len_frames();
    let s = u64::from(op.a) % (len + 1);
    let e = s + u64::from(op.b) % (len - s + 1);
    let r = s..e;
    let ru = s as usize..e as usize;
    let mask = ChannelMask::from_bits(u64::from(op.mask));
    let ed = match op.kind {
        0 => {
            model.delete(ru, mask);
            edit::delete(&doc, r, mask, NONE).unwrap()
        }
        1 => {
            let want = model.copy(ru.clone(), mask);
            let (ed, clip) = edit::cut(&doc, r, mask, NONE).unwrap();
            assert_eq!(bits(&clip_content(&clip)), bits(&want), "cut clip");
            model.delete(ru, mask);
            *clipboard = Some((clip, want));
            ed
        }
        2 => {
            let clip = edit::copy(&doc, r, mask).unwrap();
            let want = model.copy(ru, mask);
            assert_eq!(bits(&clip_content(&clip)), bits(&want), "copy");
            *clipboard = Some((clip, want));
            return;
        }
        3 => {
            let Some((clip, want)) = clipboard.as_ref() else { return };
            // The top bit of `b` picks the op's own mask (one channel or both) over all channels;
            // `op.mask` still picks insert (1) or replace (2, 3).
            let pmask = if op.b & 0x8000_0000 != 0 { mask } else { ChannelMask::ALL };
            if op.mask == 1 {
                model.paste(s as usize..s as usize, want, pmask);
                edit::paste(&doc, PasteAt::Insert(s), clip, pmask, NONE).unwrap()
            } else {
                model.paste(ru, want, pmask);
                edit::paste(&doc, PasteAt::Replace(r), clip, pmask, NONE).unwrap()
            }
        }
        4 => {
            model.silence(ru, mask);
            edit::silence(&doc, r, mask, NONE).unwrap()
        }
        5 => {
            let frames = u64::from(op.b % 5_000);
            model.insert_silence(s as usize, frames as usize);
            edit::insert_silence(&doc, s, frames, NONE).unwrap()
        }
        6 => {
            // Keep crops from emptying the document too often: at least half of it.
            let e = (s + len / 2 + u64::from(op.b) % (len / 2 + 1)).min(len);
            // Mask 3 is both channels (the length changes); 1 or 2 one channel (silences, same length).
            model.crop(s as usize..e as usize, mask);
            edit::crop(&doc, s..e, mask, NONE).unwrap()
        }
        _ => {
            model.replace(ru, mask);
            replace(&doc, r, mask, NONE)
        }
    };
    h.push_edit(ed);
}

fn check(doc: &DocState, model: &Model, what: &str) {
    assert_eq!(doc.len_frames(), model.len() as u64, "{what}: length");
    assert!(bits(&content(doc)) == bits(&model.0), "{what}: content differs from the model");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Tests 2, 3 and 4: random sequences of 1-50 edits of every kind with random channel masks
    /// (crossfades 0) equal the model after every step; undo all is bit-identical to the start
    /// (pieces and length too); undo half then redo all is the state before the undos.
    #[test]
    fn random_edits_match_the_model_and_undo_redo(ops in prop::collection::vec(op(), 1..50), block in 700usize..9_000, frames in 1usize..40_000) {
        let dir = TempDir::new(&format!("prop-{block}-{frames}-{}", ops.len()));
        let s = store(&dir.0.join("s"), 1 << 20);
        let start = data(2, frames, block);
        let doc = doc_from(&s, &start, block);
        let pieces = (doc.piece_count(0), doc.piece_count(1));
        let mut h = History::new(doc, u64::MAX);
        let mut model = Model(start.clone());
        let mut clipboard = None;
        let mut models = vec![model.clone()];
        for (i, o) in ops.iter().enumerate() {
            if model.len() == 0 {
                break;
            }
            apply(o, &mut h, &mut model, &mut clipboard);
            check(h.current(), &model, &format!("step {i} {o:?}"));
            if h.steps().len() > models.len() {
                models.push(model.clone());
            }
        }
        prop_assert_eq!(h.steps().len(), models.len());
        let last = h.current().clone();
        // Test 4: undo half, redo all.
        let n = h.cursor();
        for _ in 0..n / 2 {
            h.undo().unwrap();
        }
        check(h.current(), &models[h.cursor()], "after undoing half");
        while h.redo().is_some() {}
        prop_assert!(std::sync::Arc::ptr_eq(h.current(), &last));
        check(h.current(), &model, "after redo all");
        // Test 3: undo all.
        while h.undo().is_some() {}
        let first = h.current();
        check(first, &Model(start.clone()), "after undo all");
        prop_assert_eq!((first.piece_count(0), first.piece_count(1)), pieces);
        prop_assert_eq!(first.len_frames(), frames as u64);
    }
}

/// W1 (the Linux handheld's verification, 2026-10-15): a Crop of an empty range, at the end of the file
/// (`s == len`, what the property test kept drawing) and in the middle (`s == e`), leaves a
/// non-empty document unchanged and adds no history step, in the edit and in the model; so do
/// Delete and Cut of an empty range. Fixed inputs, no random draw (`plan/tasks/audition-check.md`,
/// 2026-10-16 Result, item 6).
#[test]
fn crop_of_an_empty_range_changes_nothing() {
    let dir = TempDir::new("w1");
    let s = store(&dir.0.join("s"), 1 << 20);
    let frames = 10_000usize;
    let start = data(2, frames, 21);
    let doc = doc_from(&s, &start, 3_000);
    let len = frames as u64;
    for r in [len..len, 4_321..4_321, 0..0] {
        let ru = r.start as usize..r.end as usize;
        let mut h = History::new(doc.clone(), u64::MAX);
        let mut m = Model(start.clone());
        m.crop(ru.clone(), ChannelMask::ALL);
        assert_eq!(m, Model(start.clone()), "model crop {r:?}");
        m.delete(ru.clone(), ChannelMask::ALL);
        assert_eq!(m, Model(start.clone()), "model delete {r:?}");
        let eds = [
            edit::crop(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap(),
            edit::delete(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap(),
            edit::delete(&doc, r.clone(), ChannelMask::only(0), NONE).unwrap(),
            edit::cut(&doc, r.clone(), ChannelMask::ALL, NONE).unwrap().0,
            edit::cut(&doc, r.clone(), ChannelMask::only(1), NONE).unwrap().0,
        ];
        for ed in eds {
            assert!(ed.is_noop(), "{} {r:?}", ed.label);
            assert_eq!(ed.state.len_frames(), len);
            assert!(ed.state.channels().iter().zip(doc.channels()).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)));
            check(&ed.state, &m, &format!("{} {r:?}", ed.label));
            assert_eq!(h.push_edit(ed), 0);
        }
        assert_eq!(h.steps().len(), 1, "no history step for {r:?}");
    }
    // The property test's own path, with the draw that failed on main: kind 6 (Crop), `a == len`
    // so `s == len`, both channels.
    let mut h = History::new(doc.clone(), u64::MAX);
    let mut model = Model(start.clone());
    let mut clipboard = None;
    apply(&Op { kind: 6, a: frames as u32, b: 0, mask: 3 }, &mut h, &mut model, &mut clipboard);
    check(h.current(), &model, "kind 6 at s == len");
    assert_eq!(model.len(), frames);
    assert_eq!(h.steps().len(), 1);
}

/// Test 1 (synthetic part): each edit at the start, middle and end, across block edges, a
/// 1-frame range and the whole file, crossfades 0, against the model.
#[test]
fn each_edit_at_every_kind_of_place_matches_the_model() {
    let dir = TempDir::new("t1");
    let s = store(&dir.0.join("s"), 1 << 20);
    let frames = 20_000usize;
    let block = 4_096u64;
    let start = data(2, frames, 3);
    let doc = doc_from(&s, &start, block as usize);
    let len = frames as u64;
    let ranges: Vec<Range<u64>> =
        vec![0..1_000, 9_000..11_000, len - 1_000..len, block - 10..block + 10, block..2 * block, block - 1..block, 7..8, len - 1..len, 0..1, 0..len];
    let masks = [ChannelMask::ALL, ChannelMask::only(0), ChannelMask::only(1)];
    let clip_src = edit::copy(&doc, 100..1_300, ChannelMask::ALL).unwrap();
    let clip_want = Model(start.clone()).copy(100..1_300, ChannelMask::ALL);
    for r in &ranges {
        let ru = r.start as usize..r.end as usize;
        for &mask in &masks {
            let mut m = Model(start.clone());
            m.delete(ru.clone(), mask);
            check(&edit::delete(&doc, r.clone(), mask, NONE).unwrap().state, &m, &format!("delete {r:?} {mask:?}"));

            let (ed, clip) = edit::cut(&doc, r.clone(), mask, NONE).unwrap();
            check(&ed.state, &m, &format!("cut {r:?}"));
            assert_eq!(bits(&clip_content(&clip)), bits(&Model(start.clone()).copy(ru.clone(), mask)));

            let mut m = Model(start.clone());
            m.silence(ru.clone(), mask);
            check(&edit::silence(&doc, r.clone(), mask, NONE).unwrap().state, &m, &format!("silence {r:?} {mask:?}"));

            let mut m = Model(start.clone());
            m.replace(ru.clone(), mask);
            check(&replace(&doc, r.clone(), mask, NONE).state, &m, &format!("replace {r:?} {mask:?}"));

            let mut m = Model(start.clone());
            m.crop(ru.clone(), mask);
            check(&edit::crop(&doc, r.clone(), mask, NONE).unwrap().state, &m, &format!("crop {r:?} {mask:?}"));

            let mut m = Model(start.clone());
            m.paste(ru.clone(), &clip_want, mask);
            let ed = edit::paste(&doc, PasteAt::Replace(r.clone()), &clip_src, mask, NONE).unwrap();
            check(&ed.state, &m, &format!("paste over {r:?} {mask:?}"));

            for at in [r.start, r.end] {
                let mut m = Model(start.clone());
                m.paste(at as usize..at as usize, &clip_want, mask);
                check(&edit::paste(&doc, PasteAt::Insert(at), &clip_src, mask, NONE).unwrap().state, &m, &format!("paste at {at} {mask:?}"));
            }
        }
        for at in [r.start, r.end] {
            let mut m = Model(start.clone());
            m.insert_silence(at as usize, 777);
            check(&edit::insert_silence(&doc, at, 777, NONE).unwrap().state, &m, &format!("insert silence at {at}"));
        }
    }
    // Empty ranges are no-ops returning the same state, and the history records nothing.
    let mut h = History::new(doc.clone(), u64::MAX);
    for ed in [
        edit::delete(&doc, 5..5, ChannelMask::ALL, NONE).unwrap(),
        edit::silence(&doc, 5..5, ChannelMask::ALL, NONE).unwrap(),
        edit::crop(&doc, 5..5, ChannelMask::ALL, NONE).unwrap(),
        edit::crop(&doc, len..len, ChannelMask::ALL, NONE).unwrap(),
        edit::crop(&doc, 0..0, ChannelMask::ALL, NONE).unwrap(),
        edit::crop(&doc, 5..5, ChannelMask::only(0), NONE).unwrap(),
        edit::crop(&doc, 0..len, ChannelMask::only(1), NONE).unwrap(),
        edit::crop(&doc, 5..50, ChannelMask::NONE, NONE).unwrap(),
        edit::paste(&doc, PasteAt::Insert(len), &clip_src, ChannelMask::only(0), NONE).unwrap(),
        edit::paste(&doc, PasteAt::Insert(5), &clip_src, ChannelMask::NONE, NONE).unwrap(),
        edit::cut(&doc, 5..5, ChannelMask::ALL, NONE).unwrap().0,
        edit::cut(&doc, len..len, ChannelMask::only(1), NONE).unwrap().0,
        edit::delete(&doc, len..len, ChannelMask::ALL, NONE).unwrap(),
        edit::insert_silence(&doc, 5, 0, NONE).unwrap(),
        replace(&doc, 5..5, ChannelMask::ALL, NONE),
        edit::delete(&doc, 5..50, ChannelMask::NONE, NONE).unwrap(),
    ] {
        assert!(ed.is_noop());
        assert!(ed.state.channels().iter().zip(doc.channels()).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)));
        assert_eq!(h.push_edit(ed.clone()), 0);
        h.push(ed.label, ed.state);
    }
    assert_eq!(h.steps().len(), 1);
    // Out of range is an error, not a panic.
    assert!(matches!(edit::delete(&doc, 10..len + 1, ChannelMask::ALL, NONE), Err(EditError::OutOfRange { .. })));
    assert!(matches!(edit::insert_silence(&doc, len + 1, 5, NONE), Err(EditError::OutOfRange { .. })));
}

/// Test 5: a push after an undo drops the redo steps.
#[test]
fn push_after_undo_drops_redo_steps() {
    let dir = TempDir::new("t5");
    let s = store(&dir.0.join("s"), 1 << 20);
    let doc = doc_from(&s, &data(1, 10_000, 1), 1_000);
    let mut h = History::new(doc, u64::MAX);
    for i in 0..4u64 {
        let ed = edit::delete(h.current(), i * 10..i * 10 + 5, ChannelMask::ALL, NONE).unwrap();
        h.push_edit(ed);
    }
    assert_eq!(h.steps().len(), 5);
    h.undo().unwrap();
    h.undo().unwrap();
    assert!(h.can_redo());
    assert_eq!(h.redo_label(), Some(edit::DELETE));
    let ed = edit::silence(h.current(), 0..100, ChannelMask::ALL, NONE).unwrap();
    h.push_edit(ed);
    assert!(!h.can_redo());
    assert_eq!(h.steps().len(), 4);
    assert_eq!(h.undo_label(), Some(edit::SILENCE));
    assert_eq!(h.steps().iter().map(|s| s.label()).collect::<Vec<_>>(), ["Open", "Delete", "Delete", "Silence"]);
}

/// Test 6: cap 10 MiB, 20 `replace_range` steps of 1 MiB of new audio each, packs of 1 MiB.
#[test]
fn disk_cap_drops_the_oldest_steps_and_their_packs() {
    let dir = TempDir::new("t6");
    let mib = 1u64 << 20;
    let s = store(&dir.0.join("s"), mib);
    let frames = (mib / 4) as usize; // 1 MiB of f32, mono
    let doc = doc_from(&s, &data(1, frames, 6), BLOCK_FRAMES);
    let mut h = History::new(doc, 10 * mib);
    let mut dropped = 0;
    for k in 0..20 {
        let ed = edit::replace_range(h.current(), 0..frames as u64, ChannelMask::ALL, "Gain", NONE, &AtomicBool::new(false), |_, i, o| {
            for (a, b) in i[0].iter().zip(o[0].iter_mut()) {
                *b = *a + 1.0;
            }
            Ok(())
        })
        .unwrap();
        let d = h.push_edit(ed);
        // Each state holds 1 MiB of its own; 10 MiB beyond the current one fit, so 11 steps stay.
        let expect = usize::from(k + 2 > 11);
        assert_eq!(d, expect, "push {k}");
        dropped += d;
        assert!(h.held_bytes() <= 10 * mib, "push {k}: {} held", h.held_bytes());
    }
    assert_eq!(dropped, 10);
    assert_eq!(h.dropped_total(), 10);
    assert_eq!(h.steps().len(), 11);
    assert_eq!(h.held_bytes(), 10 * mib);
    // The packs of dropped steps are gone: 11 states of 1 MiB live, each in its own pack.
    assert_eq!(s.bytes_live(), 11 * mib);
    assert_eq!(s.packs().len(), 11, "one pack per kept state (the last one still being written)");
    assert_eq!(s.bytes_on_disk(), 11 * mib);
    // Undo works back to the oldest step kept (the 10th edit: every sample + 10).
    while h.undo().is_some() {}
    assert_eq!(h.cursor(), 0);
    let first = content(h.current());
    let want: Vec<f32> = data(1, frames, 6)[0].iter().map(|x| (1..=10).fold(*x, |v, _| v + 1.0)).collect();
    assert_eq!(bits(&first), bits(&[want]));
}

/// Test 7: Silence and Insert Silence write nothing (crossfades 0) and give exact zeros.
#[test]
fn silence_writes_nothing_and_is_exact_zeros() {
    let dir = TempDir::new("t7");
    let s = store(&dir.0.join("s"), 1 << 20);
    let doc = doc_from(&s, &data(2, 50_000, 7), 5_000);
    s.flush().unwrap();
    let before = (s.bytes_on_disk(), s.bytes_live());
    let a = edit::silence(&doc, 1_234..40_000, ChannelMask::ALL, NONE).unwrap().state;
    let b = edit::insert_silence(&a, 10_000, 100_000, NONE).unwrap().state;
    let c = edit::delete(&b, 3..30_000, ChannelMask::only(1), NONE).unwrap().state;
    assert_eq!((s.bytes_on_disk(), s.bytes_live()), before);
    let got = content(&b);
    assert!(got.iter().all(|ch| ch[1_234..10_000].iter().chain(&ch[10_000..140_000]).all(|x| x.to_bits() == 0)));
    assert_eq!(b.len_frames(), 150_000);
    assert!(content(&c)[1][3..30_000].iter().all(|x| x.to_bits() == 0));
}

/// Test 7b: `replace_range` over 10 minutes reuses one set of buffers (two chunks per channel,
/// never more), and a cancel halfway leaves the document unchanged and its new blocks freed.
#[test]
fn replace_range_streams_and_cancels() {
    let dir = TempDir::new("t7b");
    let s = store(&dir.0.join("s"), 64 << 20);
    let one = s.append(&data(1, BLOCK_FRAMES, 77)[0]).unwrap();
    let frames = 10 * 60 * u64::from(RATE);
    let mut ch = Channel::new();
    while ch.len() < frames {
        ch.push(Piece::new(one.clone(), 0, (frames - ch.len()).min(BLOCK_FRAMES as u64) as u32).unwrap());
    }
    let doc = DocState::new(s.clone(), vec![ch.clone(), ch], RATE, SourceFormat::synthetic(RATE, 2)).unwrap();
    let live = s.bytes_live();
    let mut buffers: HashSet<usize> = HashSet::new();
    let mut calls = 0u64;
    let mut longest = 0usize;
    let ed = edit::replace_range(&doc, 0..frames, ChannelMask::ALL, "Gain", NONE, &AtomicBool::new(false), |_, i, o| {
        calls += 1;
        for b in i {
            buffers.insert(b.as_ptr() as usize);
            longest = longest.max(b.len());
        }
        for b in o.iter_mut() {
            buffers.insert(b.as_ptr() as usize);
        }
        for (a, b) in i.iter().zip(o.iter_mut()) {
            for (x, y) in a.iter().zip(b.iter_mut()) {
                *y = x * 0.5;
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, frames.div_ceil(BLOCK_FRAMES as u64));
    assert_eq!(buffers.len(), 4, "two input and two output buffers, reused for every chunk");
    assert_eq!(longest, BLOCK_FRAMES);
    assert_eq!(s.bytes_live(), live + frames * 2 * 4);
    let mut x = vec![0.0f32; 1_000];
    ed.state.read(1, frames - 1_000, &mut x).unwrap();
    let mut y = vec![0.0f32; 1_000];
    doc.read(1, frames - 1_000, &mut y).unwrap();
    assert!(x.iter().zip(&y).all(|(a, b)| a.to_bits() == (b * 0.5).to_bits()));
    drop(ed);
    assert_eq!(s.bytes_live(), live);

    // Cancel halfway.
    let cancel = AtomicBool::new(false);
    let before = content_hash(&doc);
    let r = edit::replace_range(&doc, 0..frames, ChannelMask::ALL, "Gain", NONE, &cancel, |at, i, o| {
        if at >= frames / 2 {
            cancel.store(true, Ordering::Relaxed);
        }
        for (a, b) in i.iter().zip(o.iter_mut()) {
            b.copy_from_slice(a);
        }
        Ok(())
    });
    assert!(matches!(r, Err(EditError::Cancelled)));
    assert_eq!(s.bytes_live(), live, "the blocks written before the cancel are dead");
    assert_eq!(content_hash(&doc), before);
}

fn content_hash(doc: &DocState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    for c in 0..doc.channel_count() {
        let mut at = 0;
        while at < doc.len_frames() {
            let n = doc.read(c, at, &mut buf).unwrap();
            for x in &buf[..n] {
                x.to_bits().hash(&mut h);
            }
            at += n as u64;
        }
    }
    h.finish()
}

fn frames_dur(frames: u64) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(RATE))
}

/// The most a crossfade's steps may exceed the sine's own largest step. No crossfade between two
/// sines that meet out of phase can stay strictly inside the sine's step: the phase has to move
/// across the fade, which briefly raises the frequency. Measured worst cases over every phase for
/// this curve (raised cosine, gains summing to 1): 6.6 % at 64 frames, 3.1 % at 96 (2 ms), 0.5 %
/// at 240 (5 ms); an equal-power curve: 41-43 %; a hard splice: up to 864 %.
///
/// **These hold for a full-scale sine at 1 kHz and above only** (the tests use 1 kHz), not for
/// any audio. Lower down, the sine's own largest step shrinks while the 2 ms fade's slope does
/// not, so the ratio grows without any discontinuity: a Linux handheld measured Delete at 1.43 × at
/// 220 Hz and 4.12 × at 50 Hz, Paste at 1.10 × and 1.43 × (the Linux handheld review's W6,
/// 2026-10-16). At 50 Hz that step is about 0.027 of full scale, which is the fade's own slope,
/// not a click. A test of "no click" at low frequencies needs another measure (for example the
/// step against the fade's slope); none is written yet.
const STEP_TOLERANCE_64: f32 = 1.07;
const STEP_TOLERANCE_96: f32 = 1.035;

/// Test 8: a 64-frame crossfade where the middle of a full-scale 1 kHz sine is deleted leaves no
/// step bigger than the sine's own (within [`STEP_TOLERANCE_64`]), whatever phase the two sides
/// meet at, and the same deletes without a crossfade do click.
#[test]
fn crossfaded_delete_join_has_no_click() {
    let dir = TempDir::new("t8");
    let s = store(&dir.0.join("s"), 64 << 20);
    let x = sine(2 * RATE as usize, 0.0);
    let limit = max_step(&x);
    let doc = doc_from(&s, &[x], BLOCK_FRAMES);
    let xf = Crossfades { cut_join: frames_dur(64), edge: Duration::ZERO };
    let mut worst = 0.0f32;
    let mut worst_hard = 0.0f32;
    // The sine's period is 48 frames: these 48 lengths meet it at every phase.
    for k in 0..48u64 {
        let r = 30_000..30_000 + 20_000 + k;
        let out = edit::delete(&doc, r.clone(), ChannelMask::ALL, xf).unwrap().state;
        let step = max_step(&content(&out)[0]);
        worst = worst.max(step / limit);
        assert!(step <= limit * STEP_TOLERANCE_64, "delete of {r:?}: step {step} > the sine's {limit}");
        assert_eq!(fade_spans(&doc, &out), vec![(30_000 - 32, 64)], "delete of {r:?}");
        let hard = edit::delete(&doc, r, ChannelMask::ALL, NONE).unwrap().state;
        worst_hard = worst_hard.max(max_step(&content(&hard)[0]) / limit);
    }
    println!("64-frame delete join: worst step {worst:.4} x the sine's own; without a crossfade {worst_hard:.4} x");
    assert!(worst_hard > 5.0);
}

/// Where `after` holds blocks `before` doesn't (merged where they touch), as (start, frames).
fn fade_spans(before: &DocState, after: &DocState) -> Vec<(u64, u64)> {
    let old: HashSet<BlockId> = before.channels().iter().flat_map(|c| c.pieces().iter().map(|p| p.block().id())).collect();
    let ch = after.channel(0).unwrap();
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for (i, p) in ch.pieces().iter().enumerate() {
        if p.block().is_zero() || old.contains(&p.block().id()) {
            continue;
        }
        let start = ch.piece_start(i).unwrap();
        match spans.last_mut() {
            Some(last) if last.0 + last.1 == start => last.1 += u64::from(p.len()),
            _ => spans.push((start, u64::from(p.len()))),
        }
    }
    spans
}

/// Test 9: paste channel rules.
#[test]
fn paste_maps_channels_as_specified() {
    let dir = TempDir::new("t9");
    let s = store(&dir.0.join("s"), 1 << 20);
    let st = data(2, 10_000, 9);
    let stereo = doc_from(&s, &st, 3_000);
    let mo = data(1, 8_000, 10);
    let mono = doc_from(&s, &mo, 3_000);
    // mono -> stereo: both channels get it.
    let clip = edit::copy(&mono, 100..600, ChannelMask::ALL).unwrap();
    let out = edit::paste(&stereo, PasteAt::Insert(5_000), &clip, ChannelMask::ALL, NONE).unwrap();
    let mut m = Model(st.clone());
    m.paste(5_000..5_000, &[mo[0][100..600].to_vec()], ChannelMask::ALL);
    check(&out.state, &m, "mono into stereo");
    assert_eq!(out.change, Change::LengthFrom { frame: 5_000 });
    // stereo -> mono: mixed (the mean), as new blocks.
    let clip = edit::copy(&stereo, 2_000..2_500, ChannelMask::ALL).unwrap();
    let out = edit::paste(&mono, PasteAt::Replace(10..510), &clip, ChannelMask::ALL, NONE).unwrap();
    let mix: Vec<f32> = (2_000..2_500).map(|i| (st[0][i] + st[1][i]) * 0.5).collect();
    let mut m = Model(mo.clone());
    m.paste(10..510, &[mix], ChannelMask::ALL);
    check(&out.state, &m, "stereo into mono");
    assert_eq!(out.change, Change::SameLength { range: 10..510, channels: ChannelMask::only(0) });
    // stereo -> stereo: channel to channel.
    let out = edit::paste(&stereo, PasteAt::Insert(0), &clip, ChannelMask::ALL, NONE).unwrap();
    let mut m = Model(st.clone());
    m.paste(0..0, &Model(st.clone()).copy(2_000..2_500, ChannelMask::ALL), ChannelMask::ALL);
    check(&out.state, &m, "stereo into stereo");
    // A one-channel clip (right only) into stereo goes to both.
    let right = edit::copy(&stereo, 0..100, ChannelMask::only(1)).unwrap();
    let out = edit::paste(&stereo, PasteAt::Insert(0), &right, ChannelMask::ALL, NONE).unwrap();
    assert_eq!(content(&out.state)[0][..100], st[1][..100]);
    // A clip from another document's store is copied in.
    let dir2 = TempDir::new("t9-other");
    let s2 = store(&dir2.0.join("s"), 1 << 20);
    let other = doc_from(&s2, &data(2, 1_000, 11), 1_000);
    let foreign = edit::copy(&other, 0..1_000, ChannelMask::ALL).unwrap();
    let out = edit::paste(&stereo, PasteAt::Insert(0), &foreign, ChannelMask::ALL, NONE).unwrap();
    assert!(out.state.channels().iter().all(|c| c.pieces().iter().all(|p| p.block().belongs_to(&s))));
    let mut m = Model(st.clone());
    m.paste(0..0, &data(2, 1_000, 11), ChannelMask::ALL);
    check(&out.state, &m, "clip from another store");
    // Other sample rates and channel layouts are refused.
    let other_rate =
        DocState::new(s.clone(), vec![Channel::from_pieces([Piece::zeros(100).unwrap()])], 44_100, SourceFormat::synthetic(44_100, 1)).unwrap();
    let c44 = edit::copy(&other_rate, 0..100, ChannelMask::ALL).unwrap();
    assert!(matches!(
        edit::paste(&stereo, PasteAt::Insert(0), &c44, ChannelMask::ALL, NONE),
        Err(EditError::SampleRate { clip: 44_100, doc: 48_000 })
    ));
    let three = DocState::new(
        s.clone(),
        (0..3).map(|_| Channel::from_pieces([Piece::zeros(100).unwrap()])).collect(),
        RATE,
        SourceFormat::synthetic(RATE, 3),
    )
    .unwrap();
    let c3 = edit::copy(&three, 0..100, ChannelMask::ALL).unwrap();
    assert!(matches!(edit::paste(&stereo, PasteAt::Insert(0), &c3, ChannelMask::ALL, NONE), Err(EditError::Channels { clip: 3, doc: 2 })));
}

/// One channel active (Audition 26.5.0.82, `plan/tasks/audition-check.md`, 2026-10-16 Result,
/// item 4a): Crop of one channel of a stereo document, at the start, middle and end, silences
/// everything outside the range in that channel, keeps the length, and leaves the other channel
/// byte-identical (the same `Arc`).
#[test]
fn one_channel_crop_silences_outside_and_keeps_the_length() {
    let dir = TempDir::new("crop1");
    let s = store(&dir.0.join("s"), 1 << 20);
    let frames = 20_000usize;
    let len = frames as u64;
    let start = data(2, frames, 31);
    let doc = doc_from(&s, &start, 3_000);
    for ch in 0..2 {
        let other = 1 - ch;
        for r in [0..3_000u64, 8_000..12_000, 17_000..len, 0..1, len - 1..len] {
            let ru = r.start as usize..r.end as usize;
            let what = format!("crop {r:?} channel {ch}");
            let ed = edit::crop(&doc, r.clone(), ChannelMask::only(ch), NONE).unwrap();
            assert_eq!(ed.label, edit::CROP);
            assert_eq!(ed.change, Change::SameLength { range: 0..len, channels: ChannelMask::only(ch) }, "{what}");
            assert_eq!(ed.state.len_frames(), len, "{what}");
            assert!(std::sync::Arc::ptr_eq(&ed.state.channels()[other], &doc.channels()[other]), "{what}");
            let got = content(&ed.state);
            assert_eq!(bits(&got[other..other + 1]), bits(&start[other..other + 1]), "{what}");
            assert!(got[ch][..ru.start].iter().chain(&got[ch][ru.end..]).all(|x| x.to_bits() == 0), "{what}");
            assert_eq!(bits(&[got[ch][ru.clone()].to_vec()]), bits(&[start[ch][ru.clone()].to_vec()]), "{what}");
            let mut m = Model(start.clone());
            m.crop(ru, ChannelMask::only(ch));
            check(&ed.state, &m, &what);
        }
    }
    // Default crossfades: the fades sit inside the silences, so the kept range is exact.
    let ed = edit::crop(&doc, 8_000..12_000, ChannelMask::only(0), Crossfades::default()).unwrap();
    assert_eq!(fade_spans(&doc, &ed.state), vec![(8_000 - 240, 240), (12_000, 240)]);
    let got = content(&ed.state);
    assert_eq!(bits(&[got[0][8_000..12_000].to_vec()]), bits(&[start[0][8_000..12_000].to_vec()]));
    assert!(got[0][..8_000 - 240].iter().chain(&got[0][12_000 + 240..]).all(|x| x.to_bits() == 0));
    assert!(std::sync::Arc::ptr_eq(&ed.state.channels()[1], &doc.channels()[1]));
}

/// One channel active (the same Result, item 4c): Paste into one channel of a stereo document
/// overwrites that channel from the cursor (or a selection's start, whatever the selection's
/// length) for the clip's length, keeps the length, leaves the other channel byte-identical,
/// and reports the pasted range (what is selected afterwards). Near the end of the file the clip
/// is cut short (our choice: Audition was only seen mid-file).
#[test]
fn one_channel_paste_overwrites_and_keeps_the_length() {
    let dir = TempDir::new("paste1");
    let s = store(&dir.0.join("s"), 1 << 20);
    let frames = 20_000usize;
    let len = frames as u64;
    let start = data(2, frames, 41);
    let doc = doc_from(&s, &start, 3_000);
    let mono = edit::copy(&doc, 1_000..2_000, ChannelMask::only(1)).unwrap();
    let mono_want = Model(start.clone()).copy(1_000..2_000, ChannelMask::only(1));
    let stereo = edit::copy(&doc, 5_000..6_500, ChannelMask::ALL).unwrap();
    let stereo_want = Model(start.clone()).copy(5_000..6_500, ChannelMask::ALL);
    let targets =
        [PasteAt::Insert(0), PasteAt::Insert(9_500), PasteAt::Insert(len - 300), PasteAt::Replace(8_000..8_010), PasteAt::Replace(3_000..12_000)];
    for ch in 0..2 {
        let other = 1 - ch;
        let mask = ChannelMask::only(ch);
        for (clip, want) in [(&mono, &mono_want), (&stereo, &stereo_want)] {
            for at in &targets {
                let r = match at {
                    PasteAt::Insert(f) => *f as usize..*f as usize,
                    PasteAt::Replace(r) => r.start as usize..r.end as usize,
                };
                let (f, t) = (r.start, (r.start + clip.len_frames() as usize).min(frames));
                let what = format!("paste {at:?} channel {ch}, a {}-channel clip", clip.channel_count());
                let ed = edit::paste(&doc, at.clone(), clip, mask, NONE).unwrap();
                assert_eq!(ed.label, edit::PASTE);
                assert_eq!(ed.change, Change::SameLength { range: f as u64..t as u64, channels: mask }, "{what}");
                assert_eq!(ed.state.len_frames(), len, "{what}");
                assert!(std::sync::Arc::ptr_eq(&ed.state.channels()[other], &doc.channels()[other]), "{what}");
                let got = content(&ed.state);
                let src = if want.len() == 2 { &want[ch] } else { &want[0] };
                assert_eq!(bits(&[got[ch][f..t].to_vec()]), bits(&[src[..t - f].to_vec()]), "{what}");
                assert_eq!(
                    bits(&[got[ch][..f].to_vec(), got[ch][t..].to_vec()]),
                    bits(&[start[ch][..f].to_vec(), start[ch][t..].to_vec()]),
                    "{what}"
                );
                let mut m = Model(start.clone());
                m.paste(r, want, mask);
                check(&ed.state, &m, &what);
            }
        }
        // At the very end there is nothing to overwrite.
        assert!(edit::paste(&doc, PasteAt::Insert(len), &mono, mask, NONE).unwrap().is_noop());
    }
}

/// Test 9d: default crossfades (2 ms / 5 ms) inside a full-scale 1 kHz sine: a Delete, a paste, a
/// Silence and a `replace_range` at -6 dB leave no click, and each fade is its stated length to
/// the frame.
#[test]
fn default_crossfades_leave_no_click_and_have_their_lengths() {
    let dir = TempDir::new("t9d");
    let s = store(&dir.0.join("s"), 64 << 20);
    let x = sine(3 * RATE as usize, 0.3);
    let limit = max_step(&x);
    let doc = doc_from(&s, &[x], BLOCK_FRAMES);
    let xf = Crossfades::default();
    let (join, edge) = (Crossfades::frames(xf.cut_join, RATE), Crossfades::frames(xf.edge, RATE));
    assert_eq!((join, edge), (96, 240));
    let no_click = |d: &DocState, what: &str| {
        let step = max_step(&content(d)[0]);
        println!("{what}: worst step {:.4} x the sine's own", step / limit);
        assert!(step <= limit * STEP_TOLERANCE_96, "{what}: step {step} > the sine's {limit}");
    };

    let del = edit::delete(&doc, 50_017..90_000, ChannelMask::ALL, xf).unwrap().state;
    no_click(&del, "delete");
    assert_eq!(fade_spans(&doc, &del), vec![(50_017 - 48, 96)]);
    // Every phase the two sides can meet at.
    for k in 0..48u64 {
        let d = edit::delete(&doc, 50_000..80_000 + k, ChannelMask::ALL, xf).unwrap().state;
        let step = max_step(&content(&d)[0]);
        assert!(step <= limit * STEP_TOLERANCE_96, "delete 50000..{}: step {step} > the sine's {limit}", 80_000 + k);
    }

    // A clip of the sine at another phase, pasted in and over a range.
    let clip = edit::copy(&doc, 1_011..20_000, ChannelMask::ALL).unwrap();
    let ins = edit::paste(&doc, PasteAt::Insert(70_005), &clip, ChannelMask::ALL, xf).unwrap().state;
    no_click(&ins, "paste insert");
    let l = 20_000 - 1_011;
    assert_eq!(fade_spans(&doc, &ins), vec![(70_005, 240), (70_005 + l - 240, 240)]);
    let over = edit::paste(&doc, PasteAt::Replace(70_005..80_000), &clip, ChannelMask::ALL, xf).unwrap().state;
    no_click(&over, "paste over");
    assert_eq!(fade_spans(&doc, &over), vec![(70_005, 240), (70_005 + l - 240, 240)]);

    let sil = edit::silence(&doc, 40_003..60_000, ChannelMask::ALL, xf).unwrap().state;
    no_click(&sil, "silence");
    assert_eq!(fade_spans(&doc, &sil), vec![(40_003, 240), (60_000 - 240, 240)]);
    assert!(content(&sil)[0][40_003 + 240..60_000 - 240].iter().all(|v| *v == 0.0));

    let ins_sil = edit::insert_silence(&doc, 40_003, 10_000, xf).unwrap().state;
    no_click(&ins_sil, "insert silence");
    assert_eq!(fade_spans(&doc, &ins_sil), vec![(40_003, 240), (50_003 - 240, 240)]);

    let gain = 10f32.powf(-6.0 / 20.0);
    let fx = |_: u64, i: &[&[f32]], o: &mut [&mut [f32]]| -> io::Result<()> {
        for (a, b) in i[0].iter().zip(o[0].iter_mut()) {
            *b = a * gain;
        }
        Ok(())
    };
    let r = 30_001..100_000u64;
    let quiet = edit::replace_range(&doc, r.clone(), ChannelMask::ALL, "Amplify", xf, &AtomicBool::new(false), fx).unwrap().state;
    no_click(&quiet, "replace_range -6 dB");
    let hard = edit::replace_range(&doc, r.clone(), ChannelMask::ALL, "Amplify", NONE, &AtomicBool::new(false), fx).unwrap().state;
    let (a, b) = (&content(&quiet)[0], &content(&hard)[0]);
    let differs: Vec<usize> = (0..a.len()).filter(|&i| a[i].to_bits() != b[i].to_bits()).collect();
    assert!(differs.iter().all(|&i| (30_001..30_001 + 240).contains(&(i as u64)) || (100_000 - 240..100_000).contains(&(i as u64))));
    let ch = quiet.channel(0).unwrap();
    for at in [30_001, 30_001 + 240, 100_000 - 240, 100_000] {
        assert_eq!(ch.locate(at).unwrap().1, 0, "a piece starts at {at}");
    }
}

/// A one-channel Delete at default crossfades fades inside the range and leaves the other
/// channel shared; a fade shortens at the ends of the file.
#[test]
fn fades_shorten_at_the_file_ends() {
    let dir = TempDir::new("ends");
    let s = store(&dir.0.join("s"), 64 << 20);
    let doc = doc_from(&s, &[sine(10_000, 0.0), sine(10_000, 1.0)], 4_000);
    let xf = Crossfades::default();
    // Delete from the very start: there is nothing before to join, so no crossfade.
    let d = edit::delete(&doc, 0..1_000, ChannelMask::ALL, xf).unwrap().state;
    assert!(fade_spans(&doc, &d).is_empty());
    // Delete near the start: only 10 frames before the join.
    let d = edit::delete(&doc, 10..1_000, ChannelMask::ALL, xf).unwrap().state;
    assert_eq!(fade_spans(&doc, &d), vec![(0, 96)]);
    // Paste a short clip: the two fades share it.
    let clip = edit::copy(&doc, 500..600, ChannelMask::ALL).unwrap();
    let p = edit::paste(&doc, PasteAt::Insert(5_000), &clip, ChannelMask::ALL, xf).unwrap().state;
    assert_eq!(fade_spans(&doc, &p), vec![(5_000, 100)]);
    // One-channel delete: the right channel is the same Arc.
    let one = edit::delete(&doc, 2_000..3_000, ChannelMask::only(0), xf).unwrap();
    assert_eq!(one.label, edit::DELETE_AUDIO);
    assert!(std::sync::Arc::ptr_eq(&one.state.channels()[1], &doc.channels()[1]));
    assert_eq!(fade_spans(&doc, &one.state), vec![(2_000, 240), (3_000 - 240, 240)]);
}

/// Clear History, go_to, labels, made bytes, and the space check.
#[test]
fn history_clear_go_to_labels_and_space() {
    let dir = TempDir::new("hist");
    let s = store(&dir.0.join("s"), 1 << 20);
    let doc = doc_from(&s, &data(2, 10_000, 12), 2_000);
    let mut h = History::new(doc.clone(), u64::MAX);
    assert_eq!(h.steps()[0].made_bytes(), 2 * 10_000 * 4);
    assert_eq!(h.undo_label(), None);
    let (ed, clip) = edit::cut(&doc, 100..200, ChannelMask::ALL, NONE).unwrap();
    h.push_edit(ed);
    assert_eq!(h.undo_label(), Some("Cut"));
    let ed = edit::paste(h.current(), PasteAt::Insert(0), &clip, ChannelMask::ALL, NONE).unwrap();
    h.push_edit(ed);
    assert_eq!(h.steps()[2].made_bytes(), 0, "a paste shares blocks");
    let ed = replace(h.current(), 0..1_000, ChannelMask::only(1), NONE);
    assert_eq!(ed.change, Change::SameLength { range: 0..1_000, channels: ChannelMask::only(1) });
    h.push_edit(ed);
    assert_eq!(h.steps()[3].made_bytes(), 1_000 * 4);
    assert_eq!(h.held_bytes(), 0, "the replaced blocks are still used by the current state");
    h.go_to(1).unwrap();
    assert_eq!(h.undo_label(), Some("Cut"));
    assert_eq!(h.redo_label(), Some("Paste"));
    assert!(h.go_to(9).is_none());
    h.undo().unwrap();
    assert!(std::sync::Arc::ptr_eq(&h.current().channels()[0], &doc.channels()[0]));
    h.redo().unwrap();
    let cur = h.current().clone();
    h.clear();
    assert_eq!(h.steps().len(), 1);
    assert!(std::sync::Arc::ptr_eq(h.current(), &cur));
    assert!(!h.can_undo() && !h.can_redo());
    assert_eq!(h.held_bytes(), 0);
    // Space: 5 GB free; a job may use all but 2 GB of it.
    let h = h.with_free_space(|_| Some(5_000_000_000));
    assert!(h.can_afford(3_000_000_000).is_ok());
    let e = h.can_afford(3_000_000_001).unwrap_err();
    assert_eq!(e, NoSpace { needed: 3_000_000_001, free: 5_000_000_000 });
    assert!(e.to_string().starts_with("needs 3.0 GB, 5.0 GB free"));
    let h = h.with_free_space(|_| None);
    assert!(h.can_afford(u64::MAX).is_ok());
}
