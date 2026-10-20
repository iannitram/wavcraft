//! z13.9 (B047): the silence scan and Delete Silence, on synthetic files: five 1 s tone bursts
//! with gaps of 2 s, 0.3 s, 2 s and 0.3 s of noise at -60 dB. The first gap crosses the 64 K-frame
//! chunk joins of the scan (frames 65,536 and 131,072).

#![allow(clippy::needless_range_loop)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::diagnostics::silence::{SilenceError, SilenceParams, Stretch, delete_silence, scan_silence};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const BURST: usize = RATE as usize;
const GAPS: [usize; 4] = [2 * RATE as usize, 14_400, 2 * RATE as usize, 14_400];
const KEEP: usize = 7_200; // 150 ms
static NEVER: AtomicBool = AtomicBool::new(false);

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dsil-test-{}-{name}", std::process::id()));
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

fn doc_from(s: &Store, data: &[Vec<f32>], block: usize) -> DocState {
    let chans = data
        .iter()
        .map(|c| Channel::from_pieces(c.chunks(block.clamp(1, BLOCK_FRAMES)).map(|b| Piece::whole(s.append(b).unwrap()).unwrap())))
        .collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
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

/// One channel: tone bursts (amplitude 0.5, loud at both ends) and -60 dB noise gaps.
/// Returns the samples and the gaps as stretches.
fn channel(phase: f64, seed: u32) -> (Vec<f32>, Vec<Stretch>) {
    let mut v = Vec::new();
    let mut gaps = Vec::new();
    let mut state = seed;
    for b in 0..5 {
        v.extend((0..BURST).map(|n| (0.5 * (std::f64::consts::TAU * 440.0 * n as f64 / f64::from(RATE) + phase).sin()) as f32));
        if b < 4 {
            gaps.push(Stretch { start: v.len() as u64, len: GAPS[b] as u64 });
            v.extend((0..GAPS[b]).map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(state >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) as f32 * 0.001 // -60 dB peak
            }));
        }
    }
    (v, gaps)
}

fn file(channels: usize) -> (Vec<Vec<f32>>, Vec<Stretch>) {
    let mut data = Vec::new();
    let mut gaps = Vec::new();
    for c in 0..channels {
        let (v, g) = channel(1.0 + 0.3 * c as f64, 7 + c as u32);
        data.push(v);
        gaps = g;
    }
    (data, gaps)
}

/// `data` with the middle of each long gap removed, keeping KEEP frames of it.
fn reference(data: &[Vec<f32>], gaps: &[Stretch]) -> Vec<Vec<u32>> {
    data.iter()
        .map(|ch| {
            let mut out = Vec::new();
            let mut at = 0usize;
            for g in gaps.iter().filter(|g| g.len >= 24_000) {
                out.extend(ch[at..g.start as usize + KEEP / 2].iter().map(|x| x.to_bits()));
                at = (g.start + g.len) as usize - KEEP / 2;
            }
            out.extend(ch[at..].iter().map(|x| x.to_bits()));
            out
        })
        .collect()
}

#[test]
fn the_scan_finds_exactly_the_long_gaps_mono_and_stereo() {
    for channels in [1, 2] {
        let t = TempDir::new(&format!("scan{channels}"));
        let (data, gaps) = file(channels);
        let doc = doc_from(&store(&t.0), &data, 4_000);
        let found = scan_silence(&doc, 0..0, ChannelMask::ALL, &SilenceParams::default(), &NEVER).unwrap();
        let long: Vec<Stretch> = gaps.iter().copied().filter(|g| g.len >= 24_000).collect();
        assert_eq!(found, long, "{channels} channel(s): the 0.3 s gaps are shorter than 500 ms and stay");
        // The first gap runs across two 64 K chunk joins and is one stretch.
        assert!(found[0].start < 65_536 && found[0].start + found[0].len > 131_072);
        // A shorter minimum finds the 0.3 s gaps too.
        let all = scan_silence(&doc, 0..0, ChannelMask::ALL, &SilenceParams { min_len_ms: 250, ..SilenceParams::default() }, &NEVER).unwrap();
        assert_eq!(all, gaps);
    }
}

#[test]
fn delete_silence_removes_the_excess_and_keeps_every_tone_sample() {
    for channels in [1, 2] {
        let t = TempDir::new(&format!("del{channels}"));
        let (data, gaps) = file(channels);
        let doc = doc_from(&store(&t.0), &data, 4_000);
        // Exact splices: the whole content equals the reference.
        let d = delete_silence(&doc, 0..0, ChannelMask::ALL, &SilenceParams::default(), Crossfades::NONE, &NEVER).unwrap();
        let excess = 2 * (2 * BURST - KEEP);
        assert_eq!(d.stretches, 2);
        assert_eq!(d.removed_frames as usize, excess);
        assert_eq!(d.edit.state.len_frames() as usize, data[0].len() - excess);
        assert_eq!(d.edit.label, "Delete Silence");
        assert_eq!(content(&d.edit.state), reference(&data, &gaps), "{channels} channel(s)");
        // The source state is untouched.
        assert_eq!(content(&doc).iter().map(Vec::len).max(), Some(data[0].len()));

        // With the default crossfades the length is the same, each long gap is KEEP long, and the
        // tone bursts are bit-identical, in order.
        let d = delete_silence(&doc, 0..0, ChannelMask::ALL, &SilenceParams::default(), Crossfades::default(), &NEVER).unwrap();
        assert_eq!(d.edit.state.len_frames() as usize, data[0].len() - excess);
        let after = content(&d.edit.state);
        let r = reference(&data, &gaps);
        let mut at = 0usize;
        for b in 0..5 {
            for c in 0..channels {
                assert_eq!(after[c][at..at + BURST], r[c][at..at + BURST], "burst {b} channel {c}");
            }
            at += BURST + if b < 4 { if GAPS[b] >= 24_000 { KEEP } else { GAPS[b] } } else { 0 };
        }
        let rescan =
            scan_silence(&d.edit.state, 0..0, ChannelMask::ALL, &SilenceParams { min_len_ms: 100, ..SilenceParams::default() }, &NEVER).unwrap();
        assert_eq!(rescan.iter().map(|s| s.len as usize).collect::<Vec<_>>(), vec![KEEP, 14_400, KEEP, 14_400]);
    }
}

#[test]
fn one_undo_restores_bit_exactly_and_redo_reapplies() {
    let t = TempDir::new("undo");
    let (data, _) = file(2);
    let doc = doc_from(&store(&t.0), &data, 50_000);
    let original = content(&doc);
    let mut ed = Editor::new(History::new(doc, u64::MAX));
    let steps_before = ed.history.steps().len();
    let done = ed.delete_silence(0..0, &SilenceParams::default()).unwrap();
    assert!(done.outcome.doc_changed);
    assert_eq!(done.stretches, 2);
    assert_eq!(ed.history.steps().len(), steps_before + 1, "one step");
    assert_eq!(ed.history.undo_label(), Some("Delete Silence"));
    let edited = content(ed.history.current());
    assert_ne!(edited, original);
    ed.history.undo().unwrap();
    assert_eq!(content(ed.history.current()), original, "one undo restores the original");
    assert!(!ed.history.can_undo());
    ed.history.redo().unwrap();
    assert_eq!(content(ed.history.current()), edited, "redo re-applies");
    // The Edit-menu action runs the defaults: a second pass finds nothing left to shorten.
    let out = ed.run(ActionId::DeleteSilence, 0..0).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(ed.history.steps().len(), steps_before + 1, "a no-op records no step");
}

#[test]
fn a_threshold_above_the_tone_refuses_to_delete_everything() {
    let t = TempDir::new("all");
    let (data, _) = file(1);
    let doc = doc_from(&store(&t.0), &data, 50_000);
    // 0 dB: every sample is below full scale, so the whole file is one stretch.
    let p = SilenceParams { threshold_db: 0.0, ..SilenceParams::default() };
    let found = scan_silence(&doc, 0..0, ChannelMask::ALL, &p, &NEVER).unwrap();
    assert_eq!(found, vec![Stretch { start: 0, len: data[0].len() as u64 }]);
    assert!(matches!(delete_silence(&doc, 0..0, ChannelMask::ALL, &p, Crossfades::NONE, &NEVER), Err(SilenceError::AllSilent)));
    let mut ed = Editor::new(History::new(doc, u64::MAX));
    assert!(matches!(ed.delete_silence(0..0, &p), Err(SilenceError::AllSilent)));
    assert_eq!(ed.history.steps().len(), 1, "nothing recorded");
}

#[test]
fn bad_settings_are_refused_and_a_selection_limits_the_scan() {
    let t = TempDir::new("params");
    let (data, gaps) = file(1);
    let doc = doc_from(&store(&t.0), &data, 50_000);
    for p in [
        SilenceParams { threshold_db: f64::NAN, ..SilenceParams::default() },
        SilenceParams { threshold_db: 3.0, ..SilenceParams::default() },
        SilenceParams { min_len_ms: 0, ..SilenceParams::default() },
    ] {
        assert!(matches!(scan_silence(&doc, 0..0, ChannelMask::ALL, &p, &NEVER), Err(SilenceError::BadParams(_))));
    }
    // Only the third gap's neighbourhood: the first gap is not touched.
    let sel = gaps[1].start..data[0].len() as u64;
    let d = delete_silence(&doc, sel, ChannelMask::ALL, &SilenceParams::default(), Crossfades::NONE, &NEVER).unwrap();
    assert_eq!(d.stretches, 1);
    // A cancelled scan changes nothing.
    let stop = AtomicBool::new(true);
    assert!(scan_silence(&doc, 0..0, ChannelMask::ALL, &SilenceParams::default(), &stop).is_err());
}
