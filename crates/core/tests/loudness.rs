//! f26: loudness measurement against values the standards fix (BS.1770 / EBU Tech 3342 behaviour,
//! worked out in `loudness.rs`'s header), on synthetic data.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::loudness::{Loudness, measure};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-loud-test-{}-{name}", std::process::id()));
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

fn run(doc: &DocState, r: std::ops::Range<u64>) -> Loudness {
    measure(doc, r, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

#[test]
fn a_1khz_stereo_sine_at_minus_20_dbfs_reads_minus_20_lufs() {
    let t = TempDir::new("sine");
    let s = sine(20.0, 1000.0, -20.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let m = run(&doc, 0..doc.len_frames());
    assert!((m.integrated_lufs + 20.0).abs() < 0.1, "{m:?}");
    assert!((m.sample_peak_dbfs + 20.0).abs() < 0.01, "{m:?}");
    assert!((m.true_peak_dbtp + 20.0).abs() < 0.3, "{m:?}");
    assert!(m.range_lu < 0.2, "a steady tone has no range: {m:?}");
    assert_eq!(m.frames, doc.len_frames());
}

#[test]
fn one_channel_reads_three_db_lower_than_two() {
    let t = TempDir::new("mono");
    let s = sine(10.0, 1000.0, -20.0);
    let doc = doc_from(&t.0, &[s.clone(), vec![0.0; s.len()]]);
    let m = run(&doc, 0..doc.len_frames());
    assert!((m.integrated_lufs + 23.01).abs() < 0.1, "{m:?}");
}

#[test]
fn silence_is_gated_out_and_so_is_a_passage_far_below_the_rest() {
    let t = TempDir::new("gate");
    let loud = sine(10.0, 1000.0, -20.0);
    let quiet = sine(10.0, 1000.0, -45.0);
    let mut l = loud.clone();
    l.extend(vec![0.0; 10 * RATE as usize]);
    l.extend(&quiet);
    let doc = doc_from(&t.0, &[l.clone(), l]);
    let m = run(&doc, 0..doc.len_frames());
    // Absolute gate (-70) drops the silence; relative gate (10 LU under the rest) drops the -45 passage.
    assert!((m.integrated_lufs + 20.0).abs() < 0.15, "{m:?}");
}

#[test]
fn loudness_range_of_two_levels_ten_db_apart_is_about_ten_lu() {
    let t = TempDir::new("lra");
    let mut l = sine(30.0, 1000.0, -30.0);
    l.extend(sine(30.0, 1000.0, -20.0));
    let doc = doc_from(&t.0, &[l.clone(), l]);
    let m = run(&doc, 0..doc.len_frames());
    assert!((m.range_lu - 10.0).abs() < 0.5, "{m:?}");
}

#[test]
fn true_peak_sees_what_the_samples_miss() {
    let t = TempDir::new("tp");
    // A sine at a quarter of the sample rate, 45 degrees in: every sample is +-0.7071 of the
    // peak, but the waveform between them reaches the full peak.
    let a = 0.9f64;
    let s: Vec<f32> = (0..RATE as usize * 5).map(|n| (a * (PI * n as f64 / 2.0 + PI / 4.0).sin()) as f32).collect();
    let doc = doc_from(&t.0, &[s]);
    let m = run(&doc, 0..doc.len_frames());
    let want_sample = 20.0 * (a * 0.5f64.sqrt()).log10();
    let want_true = 20.0 * a.log10();
    assert!((m.sample_peak_dbfs - want_sample).abs() < 0.01, "{m:?}");
    // soundcraft-dsp's 12-tap-a-phase interpolator reads this worst case (a quarter of the sample
    // rate, 45 degrees) about 0.5 dB high; it never reads low here. Measured: -0.40 against -0.92.
    assert!(m.true_peak_dbtp > want_true - 0.1 && m.true_peak_dbtp < want_true + 0.6, "true peak {} vs {want_true}: {m:?}", m.true_peak_dbtp);
    assert!(m.true_peak_dbtp > m.sample_peak_dbfs + 2.5);
}

#[test]
fn a_range_measures_only_the_range_and_silence_reads_minus_infinity() {
    let t = TempDir::new("range");
    let mut l = vec![0.0f32; 5 * RATE as usize];
    l.extend(sine(10.0, 1000.0, -20.0));
    let doc = doc_from(&t.0, &[l.clone(), l]);
    let silent = run(&doc, 0..5 * u64::from(RATE));
    assert_eq!(silent.integrated_lufs, f64::NEG_INFINITY);
    assert_eq!(silent.sample_peak_dbfs, f64::NEG_INFINITY);
    let tone = run(&doc, 5 * u64::from(RATE)..15 * u64::from(RATE));
    assert!((tone.integrated_lufs + 20.0).abs() < 0.1, "{tone:?}");
    // A range past the end is clamped; one under 400 ms has no loudness yet.
    let clamped = run(&doc, 14 * u64::from(RATE)..99 * u64::from(RATE));
    assert_eq!(clamped.frames, u64::from(RATE));
    let short = run(&doc, 6 * u64::from(RATE)..6 * u64::from(RATE) + 4_000);
    assert_eq!(short.integrated_lufs, f64::NEG_INFINITY);
    assert!(short.sample_peak_dbfs > -21.0);
}

#[test]
fn it_can_be_cancelled_and_reports_progress() {
    let t = TempDir::new("cancel");
    let s = sine(30.0, 440.0, -20.0);
    let doc = doc_from(&t.0, &[s]);
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let r = measure(&doc, 0..doc.len_frames(), &cancel, &mut |p| {
        seen.push(p);
        cancel.store(true, Ordering::Relaxed);
    });
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
    assert!(!seen.is_empty());
}
