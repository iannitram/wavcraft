//! G2.1: the Noise Reduction defaults match Audition's (50 %, 16 dB, 65 % decay, 4096 FFT), and
//! the FFT size list runs from 512 to 16384. The noise print is session-wide, so the tests that
//! touch it hold `LOCK`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::noise::NoisePrint;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-g2-test-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn now(e: &Editor) -> Vec<Vec<f32>> {
    let doc = e.history.current();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// Deterministic Gaussian noise (xorshift + Box-Muller), standard deviation `sigma`.
fn gaussian(frames: usize, sigma: f64, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        ((x >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    (0..frames)
        .map(|_| {
            let (u, v) = (next(), next());
            (sigma * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
        })
        .collect()
}

fn nr_index() -> u16 {
    effects::all().iter().position(|e| e.id() == "noise_reduction").unwrap() as u16
}

fn params(text: &str) -> Vec<f64> {
    effects::parse_params(effects::all()[usize::from(nr_index())], text).unwrap()
}

#[test]
fn g2_defaults_match_audition() {
    let nr = effects::find("noise_reduction").unwrap();
    let d = effects::Params::defaults(nr);
    let p = nr.parameters();
    let at = |id: &str| {
        let i = p.iter().position(|x| x.id == id).unwrap();
        d[i]
    };
    assert_eq!(at("reduction"), 50.0);
    assert_eq!(at("reduce_db"), 16.0);
    assert_eq!(at("decay"), 65.0);
    assert_eq!(at("noise_only"), 0.0);
    assert_eq!(at("fft_size"), 3.0);
    let fft = p.iter().find(|x| x.id == "fft_size").unwrap();
    assert_eq!(fft.choices, ["512", "1024", "2048", "4096", "8192", "16384"].as_slice());
    assert_eq!(fft.choices[3], "4096");
    assert!(effects::validate(nr, &d).is_ok(), "the defaults are valid");
}

#[test]
fn g2_smallest_and_largest_fft_sizes_run() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("fft");
    let frames = 4 * RATE as usize;
    let x = gaussian(frames, 0.02, 3);
    let mut e = editor(&tmp.0, std::slice::from_ref(&x));
    e.run(ActionId::CaptureNoisePrint, 0..2 * RATE as u64).unwrap();
    for (i, text) in ["fft_size=512", "fft_size=16384"].iter().enumerate() {
        let mut e2 = editor(&tmp.0.join(i.to_string()), std::slice::from_ref(&x));
        e2.run_effect(nr_index(), params(text), 0..0).unwrap();
        let y = &now(&e2)[0];
        assert_eq!(y.len(), 4 * RATE as usize, "{text}");
        assert!(y.iter().all(|v| v.is_finite()), "{text}: every sample is finite");
    }
}

/// A flat print at -60 dB (magnitude 0.001), one channel, 2049 bins (PRINT_FFT / 2 + 1).
fn flat_print_minus_60_db() -> NoisePrint {
    NoisePrint { sample_rate: 48_000, density: vec![vec![0.001f32; 2049]], frames: 1 }
}

fn noise_reduction_params(text: &str) -> Vec<f64> {
    let effect = *effects::all().iter().find(|e| e.id() == "noise_reduction").unwrap();
    effects::parse_params(effect, text).unwrap()
}

#[test]
fn g2_threshold_curve_matches_processor() {
    let print = flat_print_minus_60_db();
    let params = noise_reduction_params("fft_size=4096");
    let curve = effects::threshold_curve_db(&print, 0, &params);

    assert_eq!(curve.len(), 2049);
    let first = curve[0];
    for (k, &v) in curve.iter().enumerate() {
        assert!((v - first).abs() <= 1e-6, "bin {k}: {v} differs from {first}");
    }
    // 20 * log10(0.001 * sqrt(0.375 * 4096) * 3.5)
    assert!((first - -17.2547).abs() < 1e-3, "threshold {first} dB");

    let magnitudes = print.magnitudes(0, 4096);
    for (k, &v) in curve.iter().enumerate() {
        let processor = 20.0 * f64::from(magnitudes[k] * 3.5f32).log10();
        assert!((v - processor).abs() <= 1e-6, "bin {k}: curve {v} processor {processor}");
    }
}

#[test]
fn g2_threshold_curve_length_follows_fft_size() {
    let print = flat_print_minus_60_db();

    let small = effects::threshold_curve_db(&print, 0, &noise_reduction_params("fft_size=1024"));
    assert_eq!(small.len(), 513);
    assert!(small.iter().all(|v| (v - -23.2753).abs() < 1e-3), "1024: {small:?}");

    let large = effects::threshold_curve_db(&print, 0, &noise_reduction_params("fft_size=8192"));
    assert_eq!(large.len(), 4097);
    assert!(large.iter().all(|v| (v - -14.2444).abs() < 1e-3), "8192 first {}", large[0]);
}
