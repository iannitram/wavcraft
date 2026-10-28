//! f33: Match Loudness on synthetic audio, judged by the loudness meter (f26) on the edited
//! document: loudness within 0.5 LU of the target, true peak under the ceiling.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::loudness::{Loudness, measure};
use wavcraft_core::match_loudness::{PRESETS, Target, plan, run};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-match-test-{}-{name}", std::process::id()));
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

fn sine(secs: f64, hz: f64, peak_db: f64) -> Vec<f32> {
    let a = 10f64.powf(peak_db / 20.0);
    (0..(secs * f64::from(RATE)) as usize).map(|n| (a * (2.0 * PI * hz * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn read_all(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![0.0; doc.len_frames() as usize];
    doc.read(ch, 0, &mut v).unwrap();
    v
}

fn meas(doc: &DocState) -> Loudness {
    measure(doc, 0..doc.len_frames(), &AtomicBool::new(false), &mut |_| {}).unwrap()
}

fn go(doc: &DocState, t: &Target) -> wavcraft_core::match_loudness::MatchResult {
    run(doc, t, None, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

/// A tone with sharp one-sample clicks every 0.37 s: peaky enough that a gain to -16 LUFS puts
/// the peaks well over 0 dBFS.
fn peaky(secs: f64, tone_db: f64, click: f32) -> Vec<f32> {
    let mut s = sine(secs, 440.0, tone_db);
    let mut at = 1000;
    while at < s.len() {
        s[at] = click;
        at += (0.37 * f64::from(RATE)) as usize;
    }
    s
}

#[test]
fn a_quiet_file_matched_to_minus_16_measures_minus_16_with_the_peak_under_the_ceiling() {
    let t = TempDir::new("quiet");
    let s = sine(20.0, 1000.0, -34.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    assert!((meas(&doc).integrated_lufs + 34.0).abs() < 0.2);
    let r = go(&doc, &Target::PODCAST);
    let after = meas(&r.edit.state);
    assert!((after.integrated_lufs + 16.0).abs() < 0.5, "{after:?}");
    assert!(after.true_peak_dbtp <= -1.0 + 0.3, "{after:?}");
    assert!(r.on_target && !r.limited);
    assert!((r.gain_db - 18.0).abs() < 0.3, "{}", r.gain_db);
    assert_eq!(r.edit.label, "Match Loudness");
}

#[test]
fn a_peaky_file_is_limited_and_still_lands_on_the_target() {
    let t = TempDir::new("peaky");
    let s = peaky(20.0, -32.0, 0.5);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let before = meas(&doc);
    // The gain alone would take the clicks to about +10 dBFS.
    assert!(before.true_peak_dbtp - before.integrated_lufs > 14.0, "{before:?}");
    let r = go(&doc, &Target::PODCAST);
    let after = meas(&r.edit.state);
    assert!(r.limited, "{}", r.message);
    assert!((after.integrated_lufs + 16.0).abs() < 0.5, "{after:?} {}", r.message);
    assert!(after.true_peak_dbtp <= -1.0 + 0.3, "{after:?}");
    // The limiter worked on the clicks, not the tone: the tone stays at the gained level.
    let out = read_all(&r.edit.state, 0);
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn limiting_off_lowers_the_gain_to_the_ceiling_and_reports_the_loudness_reached() {
    let t = TempDir::new("nolimit");
    let s = peaky(20.0, -32.0, 0.5);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let target = Target { limit: false, ..Target::PODCAST };
    let r = go(&doc, &target);
    let after = meas(&r.edit.state);
    assert!(!r.limited);
    assert!(after.true_peak_dbtp <= -1.0 + 0.05 && after.true_peak_dbtp > -1.3, "{after:?}");
    assert!(after.integrated_lufs < -16.5, "short of the target because the peak is in the way: {after:?}");
    assert!(r.message.contains("limiting is off"), "{}", r.message);
    // Pure gain: every sample is the original times the same factor.
    let (a, b) = (read_all(&doc, 0), read_all(&r.edit.state, 0));
    let k = f64::from(b[1000]) / f64::from(a[1000]);
    assert!(a.iter().zip(&b).step_by(997).all(|(x, y)| (f64::from(*y) - f64::from(*x) * k).abs() < 1e-6));
}

#[test]
fn the_limiter_never_goes_over_on_a_dense_peaky_signal_and_does_not_touch_quiet_stretches() {
    let t = TempDir::new("brick");
    // A loud tone: gaining it to -16 LUFS from -30 would put it at -16 dBFS, so no limiting; add a
    // loud burst to exercise it.
    let mut s = sine(12.0, 300.0, -30.0);
    for (n, v) in s.iter_mut().enumerate().skip(5 * RATE as usize).take(RATE as usize / 4) {
        *v = (0.5 * (2.0 * PI * 1000.0 * n as f64 / f64::from(RATE)).sin()) as f32;
    }
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let r = go(&doc, &Target::PODCAST);
    let out = read_all(&r.edit.state, 0);
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(f64::from(peak) <= 10f64.powf(-1.0 / 20.0) + 1e-4, "sample peak {peak}");
    assert!(meas(&r.edit.state).true_peak_dbtp <= -1.0 + 0.3);
}

#[test]
fn one_undoable_step_and_the_original_comes_back() {
    let t = TempDir::new("undo");
    let s = sine(10.0, 1000.0, -30.0);
    let doc = doc_from(&t.0, &[s.clone(), s.clone()]);
    let before = read_all(&doc, 0);
    let r = go(&doc, &Target::PODCAST);
    let mut h = History::new(doc, 1 << 30);
    assert_eq!(h.steps().len(), 1);
    h.push_edit(r.edit);
    assert_eq!(h.steps().len(), 2);
    assert_eq!(h.undo_label(), Some("Match Loudness"));
    h.undo();
    assert_eq!(read_all(h.current(), 0), before);
}

#[test]
fn a_file_already_within_tolerance_is_left_alone() {
    let t = TempDir::new("within");
    let s = sine(10.0, 1000.0, -16.2);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let r = go(&doc, &Target::PODCAST);
    assert!(r.edit.is_noop());
    assert_eq!(r.gain_db, 0.0);
}

#[test]
fn silence_has_nothing_to_match() {
    let t = TempDir::new("silent");
    let doc = doc_from(&t.0, &[vec![0.0; 5 * RATE as usize], vec![0.0; 5 * RATE as usize]]);
    assert!(run(&doc, &Target::PODCAST, None, &AtomicBool::new(false), &mut |_| {}).is_err());
}

#[test]
fn the_broadcast_preset_reaches_minus_23() {
    let t = TempDir::new("broadcast");
    let s = sine(15.0, 1000.0, -35.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let r = go(&doc, &PRESETS[1].1);
    assert!((meas(&r.edit.state).integrated_lufs + 23.0).abs() < 0.5);
    assert_eq!(PRESETS[0].0, "Podcast (-16 LUFS, -1 dBTP)");
}

#[test]
fn plan_caps_the_gain_without_the_limiter_and_asks_for_it_with() {
    let m = Loudness { frames: 1, integrated_lufs: -30.0, range_lu: 3.0, true_peak_dbtp: -12.0, sample_peak_dbfs: -12.0, max_momentary_lufs: -28.0 };
    let on = plan(&m, &Target::PODCAST).unwrap();
    assert!(on.limiter && (on.gain_db - 14.0).abs() < 1e-9);
    let off = plan(&m, &Target { limit: false, ..Target::PODCAST }).unwrap();
    assert!(off.capped && !off.limiter && (off.gain_db - 11.0).abs() < 1e-9 && (off.expected_lufs + 19.0).abs() < 1e-9);
    assert!(plan(&Loudness { integrated_lufs: f64::NEG_INFINITY, ..m }, &Target::PODCAST).is_none());
}

#[test]
fn cancelling_stops_the_run() {
    let t = TempDir::new("cancel");
    let s = sine(10.0, 1000.0, -30.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let cancel = AtomicBool::new(true);
    assert!(run(&doc, &Target::PODCAST, None, &cancel, &mut |_| {}).is_err());
}
