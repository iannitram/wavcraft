//! K1.3: Episode Check on a document read from its store (`check::scan_doc`) gives the same
//! report as the scan of the same samples held in slices.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::check::{self, Config};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::spectrogram::Samples;
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;
const PI_F: f64 = std::f64::consts::PI;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-check-doc-test-{}-{name}", std::process::id()));
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

fn frame(seconds: f64) -> usize {
    (seconds * f64::from(RATE)).round() as usize
}

/// The speech-like channel of crates/core/tests/episode_check.rs: a harmonic voice in 1.5 s
/// phrases with 0.5 s gaps over a -50 dBFS noise floor.
fn speechlike(seed: u32, f0: f64, seconds: f64) -> Vec<f32> {
    let mut lcg = seed;
    (0..frame(seconds))
        .map(|n| {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(lcg >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * 0.003;
            let t = n as f64 / f64::from(RATE);
            let phrase = t % 2.0;
            if phrase >= 1.5 {
                return noise as f32;
            }
            let envelope = 0.5 - 0.5 * (2.0 * PI_F * phrase / 1.5).cos();
            let phase = 2.0 * PI_F * f0 * t - 15.0 / 0.7 * (2.0 * PI_F * 0.7 * t).cos();
            let voice: f64 = (1..=8).map(|k| (f64::from(k) * phase).sin() / f64::from(k)).sum();
            (envelope * 0.12 * voice + noise) as f32
        })
        .collect()
}

/// The 20 s planted-fault signal of episode_check.rs: clipping at 3.0 s, a true peak at 7.0 s,
/// DC at 12.0 s and dead air at 16.0 s.
fn planted() -> Vec<Vec<f32>> {
    let (mut ch0, mut ch1) = (speechlike(1, 120.0, 20.0), speechlike(2, 150.0, 20.0));
    ch0[frame(3.0)..frame(3.02)].fill(1.0);
    let tp = frame(7.0);
    for (i, x) in ch1[tp..frame(8.0)].iter_mut().enumerate() {
        let phase = std::f64::consts::FRAC_PI_4 + ((tp + i) as f64) * std::f64::consts::FRAC_PI_2;
        *x = (0.95 * phase.sin()) as f32;
    }
    for x in &mut ch0[frame(12.0)..frame(14.0)] {
        *x += 0.1;
    }
    ch0[frame(16.0)..frame(19.5)].fill(0.0);
    ch1[frame(16.0)..frame(19.5)].fill(0.0);
    vec![ch0, ch1]
}

/// `data` written to a new store in blocks of `block` frames (so reads cross piece joins).
fn doc(dir: &TempDir, data: &[Vec<f32>], block: usize) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(block).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

#[test]
fn scan_doc_matches_the_slice_scan() {
    let data = planted();
    let (a, b): (&[f32], &[f32]) = (&data[0], &data[1]);
    let sources: [&dyn Samples; 2] = [&a, &b];
    let want = check::scan(&sources, &Config::new(RATE));
    assert!(!want.findings.is_empty(), "the planted faults should be found");

    let dir = TempDir::new("planted");
    let state = doc(&dir, &data, 40_009);
    let mut last = 0.0;
    let got = check::scan_doc(&state, &AtomicBool::new(false), &mut |p| last = p).unwrap();
    assert_eq!(got, Some(want));
    assert_eq!(last, 1.0);
}
