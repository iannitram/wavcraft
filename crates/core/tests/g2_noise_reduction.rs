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
