//! K5.2: the Episode Check summary agrees with `loudness::measure` on the same document, and the
//! TruePeak finding agrees with `measure`'s true peak over the finding's range. Both within 0.1.
//! The signals are synthetic: a 1 kHz sine and a quarter-rate sine at 0.95 (as in episode_check.rs).

use std::f64::consts::{FRAC_PI_4, PI};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::check::summary::Summary;
use wavcraft_core::check::{self, Kind};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::loudness::measure;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const TOLERANCE_DB: f64 = 0.1;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-summary-test-{}-{name}", std::process::id()));
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

#[test]
fn summary_matches_loudness_measure_to_a_tenth() {
    let t = TempDir::new("sine");
    let s = sine(20.0, 1000.0, -20.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);

    let cancel = AtomicBool::new(false);
    let m = measure(&doc, 0..doc.len_frames(), &cancel, &mut |_| {}).unwrap();
    let report = check::scan_doc(&doc, &cancel, &mut |_| {}).unwrap().unwrap();
    let summary = Summary::new(&m, doc.channel_count(), &report);

    assert!((summary.integrated_lufs + 20.0).abs() <= TOLERANCE_DB, "summary integrated {} LUFS, want -20.0", summary.integrated_lufs);
    assert!(
        (summary.integrated_lufs - m.integrated_lufs).abs() <= TOLERANCE_DB,
        "summary integrated {} vs measure {}",
        summary.integrated_lufs,
        m.integrated_lufs
    );
    assert!(
        (summary.true_peak_dbtp - m.true_peak_dbtp).abs() <= TOLERANCE_DB,
        "summary true peak {} vs measure {}",
        summary.true_peak_dbtp,
        m.true_peak_dbtp
    );
    assert_eq!(summary.target_lufs, -16.0);
}

#[test]
fn true_peak_finding_agrees_with_measure() {
    // Channel 0: a quiet 1 kHz sine, so it has no finding of its own. Channel 1: silence, then
    // one second of a quarter-rate sine at 0.95 with a 45 degree phase from 7.0 s, which is over
    // the -1 dBTP ceiling.
    let t = TempDir::new("true-peak");
    let frames = 10 * RATE as usize;
    let ch0 = sine(10.0, 1000.0, -20.0);
    let mut ch1 = vec![0.0f32; frames];
    let tp_start = 7 * RATE as usize;
    for (i, x) in ch1[tp_start..tp_start + RATE as usize].iter_mut().enumerate() {
        let phase = FRAC_PI_4 + ((tp_start + i) as f64) * std::f64::consts::FRAC_PI_2;
        *x = (0.95 * phase.sin()) as f32;
    }
    let doc = doc_from(&t.0, &[ch0, ch1]);

    let cancel = AtomicBool::new(false);
    let report = check::scan_doc(&doc, &cancel, &mut |_| {}).unwrap().unwrap();
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == Kind::TruePeak && f.channel == Some(1))
        .copied()
        .expect("the quarter-rate sine should give a TruePeak finding on channel 1");

    let m = measure(&doc, finding.start..finding.end, &cancel, &mut |_| {}).unwrap();
    assert!(
        (finding.value - m.true_peak_dbtp).abs() <= TOLERANCE_DB,
        "TruePeak finding {} dBTP vs measure {} dBTP over {}..{}",
        finding.value,
        m.true_peak_dbtp,
        finding.start,
        finding.end
    );
}
