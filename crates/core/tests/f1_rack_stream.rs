//! F1: the Effects Rack runs real effects (F1.1 filters, F1.2 Dynamics; F1.3 adds more).

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::rack::{RackConfig, RackProcessor};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
/// 2 s: longer than one 65,536-frame chunk, so the offline run crosses a chunk join.
const FRAMES: usize = 96_000;

const CASES: [(&str, &str); 4] = [
    ("parametric_eq", "low_gain_db=6 mid1_gain_db=-9 mid2_gain_db=5 high_gain_db=-4"),
    ("high_pass", "cutoff=200 slope=24"),
    ("low_pass", "cutoff=4000 slope=24"),
    ("notch", "freq=200 q=1 depth_db=30"),
];

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-f1-rack-test-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
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

fn offline(id: &str, args: &str, input: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let e = effects::find(id).unwrap();
    let params = effects::parse_params(e, args).unwrap();
    let tmp = TempDir::new(id);
    let d = doc(&tmp.0, input);
    let edit = effects::apply(e, &params, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
    (0..input.len())
        .map(|c| {
            let mut v = vec![0.0f32; input[c].len()];
            assert_eq!(edit.state.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn rack_for(id: &str, args: &str) -> RackConfig {
    let e = effects::find(id).unwrap();
    let mut cfg = RackConfig::new();
    cfg.set_slot(0, id, Some(effects::parse_params(e, args).unwrap())).unwrap();
    cfg
}

fn run_blocks(rack: &mut RackProcessor, data: &mut [Vec<f32>], block: usize) {
    let frames = data[0].len();
    let mut start = 0;
    while start < frames {
        let end = (start + block).min(frames);
        let mut io: Vec<&mut [f32]> = data.iter_mut().map(|c| &mut c[start..end]).collect();
        rack.process_block(&mut io);
        start = end;
    }
}

fn through_rack(id: &str, args: &str, input: &[Vec<f32>], block: usize) -> Vec<Vec<f32>> {
    let mut rack = RackProcessor::new(&rack_for(id, args), input.len(), RATE).unwrap();
    let mut data = input.to_vec();
    run_blocks(&mut rack, &mut data, block);
    data
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[test]
fn f1_rack_filters_match_offline_apply() {
    let input = vec![pink(FRAMES, 1)];
    for (id, args) in CASES {
        let want = offline(id, args, &input);
        for block in [64, 1024] {
            let got = through_rack(id, args, &input, block);
            assert_eq!(got[0].len(), want[0].len(), "{id} at block {block}");
            let d = max_diff(&got[0], &want[0]);
            assert!(d <= 1e-6, "{id} at block {block}: differs from effects::apply by {d}");
            let moved = max_diff(&got[0], &input[0]);
            assert!(moved > 1e-3, "{id} at block {block}: passes audio through (max change {moved})");
        }
    }
}

#[test]
fn f1_rack_filters_reset_matches_a_new_rack() {
    let warm = vec![pink(4096, 2)];
    let probe = vec![pink(FRAMES, 1)[..512].to_vec()];
    for (id, args) in CASES {
        let cfg = rack_for(id, args);
        let mut used = RackProcessor::new(&cfg, 1, RATE).unwrap();
        let mut w = warm.clone();
        run_blocks(&mut used, &mut w, 1024);
        used.reset();
        let mut after = probe.clone();
        run_blocks(&mut used, &mut after, 512);

        let mut fresh = RackProcessor::new(&cfg, 1, RATE).unwrap();
        let mut expected = probe.clone();
        run_blocks(&mut fresh, &mut expected, 512);
        assert_eq!(after, expected, "{id}: reset state differs from a new rack");
    }
}

#[test]
fn f1_rack_filters_keep_channels_apart() {
    let left = pink(FRAMES, 1);
    let stereo = vec![left.clone(), vec![0.0f32; FRAMES]];
    for (id, args) in CASES {
        let got = through_rack(id, args, &stereo, 1024);
        assert!(got[1].iter().all(|&v| v == 0.0), "{id}: the silent channel picked up signal");
        let want = offline(id, args, std::slice::from_ref(&left));
        let d = max_diff(&got[0], &want[0]);
        assert!(d <= 1e-6, "{id}: left channel differs from the mono result by {d}");
    }
}

/// A 440 Hz sine of amplitude `amp`.
fn sine(amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (TAU * 440.0 * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

/// 1 s at -6 dBFS, then 1 s at -40 dBFS.
fn burst() -> Vec<f32> {
    let mut x = sine(10f64.powf(-6.0 / 20.0), 48_000);
    x.extend(sine(10f64.powf(-40.0 / 20.0), 48_000));
    x
}

#[test]
fn f1_rack_dynamics_matches_offline_apply() {
    let input = vec![burst()];
    for args in ["lookahead_ms=0", "lookahead_ms=3 attack_ms=2 release_ms=30 makeup_db=3"] {
        let want = offline("dynamics", args, &input);
        for block in [64, 1024] {
            let got = through_rack("dynamics", args, &input, block);
            assert!(got[0].iter().all(|v| v.is_finite()), "{args}, block {block}: the rack gave a non-finite sample");
            let d = max_diff(&got[0], &want[0]);
            assert!(d <= 1e-6, "{args}, block {block}: worst difference from effects::apply {d}");
            let peak = got[0][24_000..48_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 0.45, "{args}, block {block}: the rack passed the audio through (peak {peak})");
        }
    }
}

#[test]
fn f1_rack_dynamics_reset_matches_a_new_rack() {
    let cfg = rack_for("dynamics", "lookahead_ms=3");
    let quiet = vec![sine(10f64.powf(-40.0 / 20.0), 2_048)];
    let mut used = RackProcessor::new(&cfg, 1, RATE).unwrap();
    let mut loud = vec![sine(10f64.powf(-6.0 / 20.0), 48_000)];
    run_blocks(&mut used, &mut loud, 1024);
    used.reset();
    let mut after = quiet.clone();
    run_blocks(&mut used, &mut after, 1024);
    assert_ne!(after, quiet, "the rack passed the audio through");
    // 3 ms at 48 kHz is 144 frames of delay line, which reset must have cleared.
    assert!(after[0][..144].iter().all(|v| *v == 0.0), "the delay line was not cleared");

    let mut fresh = RackProcessor::new(&cfg, 1, RATE).unwrap();
    let mut expected = quiet.clone();
    run_blocks(&mut fresh, &mut expected, 1024);
    assert_eq!(after, expected, "reset state differs from a new rack");
}
