//! G3.1: the Match To choice (BS.1770-3, EBU R128, True Peak) in Match Loudness.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::loudness::{Loudness, measure};
use wavcraft_core::match_loudness::{MATCH_TO, MatchTo, Target, run};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-g3-match-to-{}-{name}", std::process::id()));
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

#[test]
fn g3_true_peak_amplitude_mode_lands_peak_on_target() {
    let t = TempDir::new("true-peak");
    let s = sine(10.0, 1000.0, -12.0);
    let doc = doc_from(&t.0, &[s.clone(), s]);
    let target = Target { match_to: MatchTo::TruePeak, max_true_peak_dbtp: -3.0, ..Target::PODCAST };
    let r = go(&doc, &target);
    let after = meas(&r.edit.state);
    assert!((after.true_peak_dbtp + 3.0).abs() <= 0.1, "{after:?}");
    assert!((r.gain_db - 9.0).abs() <= 0.1, "gain {}", r.gain_db);
    assert!(!r.limited);
}

#[test]
fn g3_ebu_r128_matches_bs1770_result() {
    let ta = TempDir::new("r128-bs");
    let tb = TempDir::new("r128-ebu");
    let s = sine(15.0, 1000.0, -35.0);
    let a_doc = doc_from(&ta.0, &[s.clone(), s.clone()]);
    let b_doc = doc_from(&tb.0, &[s.clone(), s]);
    let bs = Target { match_to: MatchTo::Bs1770, lufs: -23.0, ..Target::PODCAST };
    // The -16 is ignored: EBU R128 fixes the target at -23 LUFS.
    let ebu = Target { match_to: MatchTo::EbuR128, lufs: -16.0, ..Target::PODCAST };
    let a = go(&a_doc, &bs);
    let b = go(&b_doc, &ebu);
    assert!((a.gain_db - b.gain_db).abs() <= 1e-9, "{} vs {}", a.gain_db, b.gain_db);
    assert_eq!(read_all(&a.edit.state, 0), read_all(&b.edit.state, 0));
    assert_eq!(read_all(&a.edit.state, 1), read_all(&b.edit.state, 1));
    assert!((meas(&b.edit.state).integrated_lufs + 23.0).abs() <= 0.5);
}

#[test]
fn g3_default_match_to_is_bs1770() {
    assert_eq!(Target::default().match_to, MatchTo::Bs1770);
    let names: Vec<&str> = MATCH_TO.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["ITU-R BS.1770-3 Loudness", "EBU R128 Loudness", "True Peak Amplitude"]);
}
