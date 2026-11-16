//! F1: the Effects Rack streams equal the offline effects (roadmap F1.2: Dynamics).

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, dynamics::Dynamics};
use wavcraft_core::rack::{RackConfig, RackProcessor};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-f1-test-{}-{name}", std::process::id()));
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

fn doc(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn sine(amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (TAU * 440.0 * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

/// A 440 Hz tone: 1 s at -6 dBFS, then 1 s at -40 dBFS.
fn burst() -> Vec<f32> {
    let mut x = sine(10f64.powf(-6.0 / 20.0), 48_000);
    x.extend(sine(10f64.powf(-40.0 / 20.0), 48_000));
    x
}

fn dynamics_rack(args: &str) -> RackConfig {
    let p = effects::parse_params(&Dynamics, args).unwrap();
    let mut c = RackConfig::new();
    c.set_slot(0, "dynamics", Some(p)).unwrap();
    c
}

fn through(rack: &mut RackProcessor, input: &[f32], bs: usize) -> Vec<f32> {
    let mut out = input.to_vec();
    for chunk in out.chunks_mut(bs) {
        rack.process_block(&mut [&mut chunk[..]]);
    }
    out
}

#[test]
fn f1_rack_dynamics_matches_offline_apply() {
    let x = burst();
    for args in ["lookahead_ms=0", "lookahead_ms=3 attack_ms=2 release_ms=30 makeup_db=3"] {
        let p = effects::parse_params(&Dynamics, args).unwrap();
        let tmp = TempDir::new("dyn");
        let d = doc(&tmp.0, std::slice::from_ref(&x));
        let edit = effects::apply(&Dynamics, &p, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
        let mut offline = vec![f32::NAN; x.len()];
        assert_eq!(edit.state.read(0, 0, &mut offline).unwrap(), x.len());

        for bs in [64usize, 1024] {
            let mut rack = RackProcessor::new(&dynamics_rack(args), 1, RATE).unwrap();
            let out = through(&mut rack, &x, bs);
            let worst = out.iter().zip(&offline).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst <= 1e-6, "{args}, block {bs}: worst difference {worst}");
            let peak = out[24_000..48_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 0.45, "{args}, block {bs}: the rack passed the audio through (peak {peak})");
        }
    }
}

#[test]
fn f1_rack_dynamics_reset_matches_a_new_rack() {
    let cfg = dynamics_rack("lookahead_ms=3");
    let quiet = sine(10f64.powf(-40.0 / 20.0), 2_048);
    let mut used = RackProcessor::new(&cfg, 1, RATE).unwrap();
    through(&mut used, &sine(10f64.powf(-6.0 / 20.0), 48_000), 1024);
    used.reset();
    let after = through(&mut used, &quiet, 1024);
    let mut fresh = RackProcessor::new(&cfg, 1, RATE).unwrap();
    assert_eq!(after, through(&mut fresh, &quiet, 1024));
}
