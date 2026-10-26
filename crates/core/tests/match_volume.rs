//! B041: Match Volume (gain from a selection's integrated loudness to a reference's), on synthetic
//! data. A 1 kHz stereo sine reads its peak level in LUFS (see `loudness.rs`), so gains are known.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::loudness::{Loudness, MATCH_VOLUME_LABEL, MatchVolume, MatchVolumeError, match_gain_db, match_volume, measure};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-matchvol-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans =
        data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES.min(30_000)).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn sine(secs: f64, peak_db: f64) -> Vec<f32> {
    let a = 10f64.powf(peak_db / 20.0);
    (0..(secs * f64::from(RATE)) as usize).map(|n| (a * (2.0 * PI * 1000.0 * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn stereo(parts: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let all: Vec<f32> = parts.concat();
    vec![all.clone(), all]
}

fn silent(secs: u64) -> Vec<Vec<f32>> {
    vec![vec![0.0; (secs * u64::from(RATE)) as usize]; 2]
}

fn frames(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; doc.len_frames() as usize];
    let got = doc.read(ch, 0, &mut v).unwrap();
    assert_eq!(got, v.len());
    v
}

fn lufs(doc: &DocState, r: std::ops::Range<u64>) -> f64 {
    measure(doc, r, &AtomicBool::new(false), &mut |_| {}).unwrap().integrated_lufs
}

fn secs(s: u64) -> u64 {
    s * u64::from(RATE)
}

fn loud(integrated: f64) -> Loudness {
    Loudness { frames: 1, integrated_lufs: integrated, range_lu: 0.0, true_peak_dbtp: -1.0, sample_peak_dbfs: -1.0, max_momentary_lufs: integrated }
}

fn go(doc: &DocState, sel: std::ops::Range<u64>, rdoc: &DocState, rr: std::ops::Range<u64>) -> Result<MatchVolume, MatchVolumeError> {
    match_volume(doc, sel, rdoc, rr, &AtomicBool::new(false), &mut |_| {})
}

#[test]
fn the_gain_is_reference_minus_selection() {
    assert_eq!(match_gain_db(&loud(-30.0), &loud(-20.0)), Some(10.0));
    assert_eq!(match_gain_db(&loud(-14.5), &loud(-23.0)), Some(-8.5));
    assert_eq!(match_gain_db(&loud(-20.0), &loud(-20.0)), Some(0.0));
}

#[test]
fn no_gain_when_either_side_has_no_loudness() {
    assert_eq!(match_gain_db(&loud(f64::NEG_INFINITY), &loud(-20.0)), None);
    assert_eq!(match_gain_db(&loud(-20.0), &loud(f64::NEG_INFINITY)), None);
}

#[test]
fn a_quiet_selection_is_raised_to_the_reference() {
    let (a, b) = (TempDir::new("q-doc"), TempDir::new("q-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(6.0, -30.0)]));
    let rdoc = doc_from(&b.0, &stereo(&[sine(6.0, -20.0)]));
    let r = go(&doc, 0..doc.len_frames(), &rdoc, 0..rdoc.len_frames()).unwrap();
    assert!((r.gain_db - 10.0).abs() < 0.05, "{}", r.gain_db);
    assert!((r.selection.integrated_lufs + 30.0).abs() < 0.1, "{:?}", r.selection);
    assert!((r.reference.integrated_lufs + 20.0).abs() < 0.1, "{:?}", r.reference);
    assert_eq!(r.edit.label, MATCH_VOLUME_LABEL);
    assert!(!r.edit.is_noop());
    assert_eq!(r.edit.state.len_frames(), doc.len_frames());
    let after = lufs(&r.edit.state, 0..r.edit.state.len_frames());
    assert!((after + 20.0).abs() < 0.1, "{after}");
    // A sample is the old one times the gain (10 dB is a factor of 3.1622777).
    let (old, new) = (frames(&doc, 1), frames(&r.edit.state, 1));
    let k = 12_345;
    assert!((f64::from(new[k]) - f64::from(old[k]) * 10f64.powf(r.gain_db / 20.0)).abs() < 1e-5, "{} {}", old[k], new[k]);
}

#[test]
fn a_loud_selection_is_lowered() {
    let (a, b) = (TempDir::new("l-doc"), TempDir::new("l-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(6.0, -12.0)]));
    let rdoc = doc_from(&b.0, &stereo(&[sine(6.0, -23.0)]));
    let r = go(&doc, 0..doc.len_frames(), &rdoc, 0..rdoc.len_frames()).unwrap();
    assert!((r.gain_db + 11.0).abs() < 0.05, "{}", r.gain_db);
    let after = lufs(&r.edit.state, 0..r.edit.state.len_frames());
    assert!((after + 23.0).abs() < 0.1, "{after}");
}

#[test]
fn only_the_selection_changes() {
    let (a, b) = (TempDir::new("s-doc"), TempDir::new("s-ref"));
    // 2 s at -40, 6 s at -30 (the selection), 2 s at -40.
    let doc = doc_from(&a.0, &stereo(&[sine(2.0, -40.0), sine(6.0, -30.0), sine(2.0, -40.0)]));
    let rdoc = doc_from(&b.0, &stereo(&[sine(6.0, -20.0)]));
    let r = go(&doc, secs(2)..secs(8), &rdoc, 0..rdoc.len_frames()).unwrap();
    assert!((r.gain_db - 10.0).abs() < 0.05, "{}", r.gain_db);
    assert_eq!(r.edit.state.len_frames(), doc.len_frames());
    for ch in 0..2 {
        let (old, new) = (frames(&doc, ch), frames(&r.edit.state, ch));
        assert_eq!(old[..secs(2) as usize], new[..secs(2) as usize], "before the selection, channel {ch}");
        assert_eq!(old[secs(8) as usize..], new[secs(8) as usize..], "after the selection, channel {ch}");
        assert_ne!(old[secs(3) as usize..secs(4) as usize], new[secs(3) as usize..secs(4) as usize], "inside, channel {ch}");
    }
    let inside = lufs(&r.edit.state, secs(2)..secs(8));
    assert!((inside + 20.0).abs() < 0.1, "{inside}");
}

#[test]
fn only_the_reference_range_is_measured() {
    let (a, b) = (TempDir::new("r-doc"), TempDir::new("r-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(6.0, -30.0)]));
    // The reference file is 4 s at -50 then 6 s at -20: the range picks the second part.
    let rdoc = doc_from(&b.0, &stereo(&[sine(4.0, -50.0), sine(6.0, -20.0)]));
    let r = go(&doc, 0..doc.len_frames(), &rdoc, secs(4)..secs(10)).unwrap();
    assert!((r.gain_db - 10.0).abs() < 0.05, "{}", r.gain_db);
    assert!((r.reference.integrated_lufs + 20.0).abs() < 0.1, "{:?}", r.reference);
}

#[test]
fn a_silent_or_too_short_selection_makes_no_edit() {
    let (a, b, c) = (TempDir::new("z-doc"), TempDir::new("z-ref"), TempDir::new("z-short"));
    let rdoc = doc_from(&b.0, &stereo(&[sine(6.0, -20.0)]));
    let quiet = doc_from(&a.0, &silent(6));
    let e = go(&quiet, 0..quiet.len_frames(), &rdoc, 0..rdoc.len_frames()).unwrap_err();
    assert!(matches!(e, MatchVolumeError::SelectionSilent), "{e:?}");
    let short = doc_from(&c.0, &stereo(&[sine(6.0, -30.0)]));
    let e = go(&short, 0..secs(1) / 4, &rdoc, 0..rdoc.len_frames()).unwrap_err();
    assert!(matches!(e, MatchVolumeError::SelectionSilent), "{e:?}");
}

#[test]
fn a_silent_reference_makes_no_edit() {
    let (a, b) = (TempDir::new("y-doc"), TempDir::new("y-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(6.0, -30.0)]));
    let quiet = doc_from(&b.0, &silent(6));
    let e = go(&doc, 0..doc.len_frames(), &quiet, 0..quiet.len_frames()).unwrap_err();
    assert!(matches!(e, MatchVolumeError::ReferenceSilent), "{e:?}");
}

#[test]
fn cancel_stops_it() {
    let (a, b) = (TempDir::new("c-doc"), TempDir::new("c-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(6.0, -30.0)]));
    let rdoc = doc_from(&b.0, &stereo(&[sine(6.0, -20.0)]));
    let cancel = AtomicBool::new(true);
    let e = match_volume(&doc, 0..doc.len_frames(), &rdoc, 0..rdoc.len_frames(), &cancel, &mut |_| {}).unwrap_err();
    assert!(matches!(e, MatchVolumeError::Edit(_)), "{e:?}");
    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn progress_runs_from_zero_to_one_in_order() {
    let (a, b) = (TempDir::new("p-doc"), TempDir::new("p-ref"));
    let doc = doc_from(&a.0, &stereo(&[sine(8.0, -30.0)]));
    let rdoc = doc_from(&b.0, &stereo(&[sine(8.0, -20.0)]));
    let mut seen: Vec<f64> = Vec::new();
    match_volume(&doc, 0..doc.len_frames(), &rdoc, 0..rdoc.len_frames(), &AtomicBool::new(false), &mut |p| seen.push(p)).unwrap();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|p| (0.0..=1.0).contains(p)), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert_eq!(seen.last().copied(), Some(1.0));
}
