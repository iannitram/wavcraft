//! Noise print files (G1.1): a print saved with NoisePrint::save and read back with
//! NoisePrint::load is bit-equal, and a file with a bad magic, version or length is refused.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::noise::{self, NoisePrint};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-g1-print-test-{}-{name}", std::process::id()));
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

#[test]
fn g1_print_from_half_second_round_trips() {
    let dir = TempDir::new("round-trip");
    let e = editor(&dir.0.join("doc"), &[gaussian(24_000, 0.1, 7)]);
    let doc = e.history.current();
    let print = noise::capture(doc, 0..24_000).unwrap();

    let file = dir.0.join("print.wcnp");
    print.save(&file).unwrap();
    let loaded = NoisePrint::load(&file).unwrap();

    assert_eq!(loaded.sample_rate, 48_000);
    assert_eq!(loaded.frames, print.frames);
    assert_eq!(loaded.density.len(), 1);
    assert_eq!(loaded.density[0].len(), 2049);
    for (a, b) in loaded.density[0].iter().zip(&print.density[0]) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    assert_eq!(loaded, print);
}

#[test]
fn g1_load_rejects_bad_magic_and_version() {
    let dir = TempDir::new("bad-header");
    let e = editor(&dir.0.join("doc"), &[gaussian(24_000, 0.1, 7)]);
    let doc = e.history.current();
    let print = noise::capture(doc, 0..24_000).unwrap();

    let file = dir.0.join("print.wcnp");
    print.save(&file).unwrap();
    let valid = std::fs::read(&file).unwrap();

    // (a) a wrong first byte.
    let mut bad_magic = valid.clone();
    bad_magic[0] = b'X';
    std::fs::write(&file, &bad_magic).unwrap();
    assert!(NoisePrint::load(&file).is_err());

    // (b) a version this build does not read.
    let mut bad_version = valid.clone();
    bad_version[8..12].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(&file, &bad_version).unwrap();
    assert!(NoisePrint::load(&file).is_err());

    // (c) a missing file, and a valid file cut short.
    assert!(NoisePrint::load(&dir.0.join("missing.wcnp")).is_err());
    std::fs::write(&file, &valid[..100]).unwrap();
    assert!(NoisePrint::load(&file).is_err());
}
