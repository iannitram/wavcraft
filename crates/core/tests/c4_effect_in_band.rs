//! C4.3: an effect run in a spectral band is one undo step.

use std::path::{Path, PathBuf};

use wavcraft_core::Selection;
use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::stft::Stft;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::view::{Band, SpectralSelection};

const RATE: u32 = 48_000;
const N: usize = 240_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-c4-effect-in-band-test-{}-{name}", std::process::id()));
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

fn samples(e: &Editor) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(0, 0, &mut v).unwrap(), v.len());
    v
}

/// Pink noise: xorshift64 white noise through Paul Kellet's filter.
fn pink(frames: usize, seed: u64) -> Vec<f32> {
    let mut x = seed.max(1);
    let (mut b0, mut b1, mut b2) = (0.0f64, 0.0f64, 0.0f64);
    (0..frames)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let w = (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0;
            b0 = 0.99765 * b0 + w * 0.0990460;
            b1 = 0.96300 * b1 + w * 0.2965164;
            b2 = 0.57000 * b2 + w * 1.0526913;
            ((b0 + b1 + b2 + w * 0.1848) * 0.25) as f32
        })
        .collect()
}

fn input(frames: usize) -> Vec<f32> {
    let mut noise = pink(frames, 1);
    let rms = (noise.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / frames as f64).sqrt();
    let k = 0.031623 / rms;
    for (i, v) in noise.iter_mut().enumerate() {
        let sine = 0.251189 * (std::f64::consts::TAU * 1000.0 * i as f64 / f64::from(RATE)).sin();
        *v = (f64::from(*v) * k + sine) as f32;
    }
    noise
}

fn band_power(sig: &[f32], lo_hz: f32, hi_hz: f32) -> f64 {
    let stft = Stft::new(8192, 50).unwrap();
    let mut scratch = Vec::new();
    let mut spec = vec![rustfft::num_complex::Complex32::new(0.0, 0.0); stft.bins()];
    let mut sum = 0.0f64;
    for s in (105_600..134_400).step_by(4096) {
        stft.analyse(sig, 0, s as i64, &mut scratch, &mut spec);
        for (b, c) in spec.iter().enumerate() {
            let hz = b as f32 * 48_000.0 / 8192.0;
            if (lo_hz..=hi_hz).contains(&hz) {
                sum += f64::from(c.norm_sqr());
            }
        }
    }
    sum
}

#[test]
fn amplify_in_a_band_is_one_undo_step() {
    let t = TempDir::new("amplify");
    let mut e = editor(&t.0, &[input(N)]);

    let amp = u16::try_from(effects::all().iter().position(|e| e.id() == "amplify").unwrap()).unwrap();
    let params = vec![-20.0, 0.0, 1.0];
    let mut band = Band::new(900.0, 1100.0, 24_000.0);
    band.feather_hz = 50.0;
    band.feather_ms = 20.0;
    let sel = SpectralSelection { time: Selection { anchor: 96_000, head: 144_000 }, band, rate: RATE };

    let before = samples(&e);
    let steps = e.history.steps().len();

    let out = e.run_effect_in_band(amp, params, &sel).unwrap();
    assert!(out.doc_changed);

    let after = samples(&e);
    let drop = 10.0 * (band_power(&after, 900.0, 1100.0) / band_power(&before, 900.0, 1100.0)).log10();
    assert!((drop + 20.0).abs() <= 1.0, "1 kHz changed {drop} dB");

    assert_eq!(e.history.steps().len(), steps + 1);
    assert_eq!(e.history.steps().last().unwrap().label(), "Amplify");

    e.run(ActionId::Undo, 0..0).unwrap();
    let undone = samples(&e);
    assert!(undone.iter().zip(&before).all(|(a, b)| a.to_bits() == b.to_bits()));
}
