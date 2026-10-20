//! Reverb and Delay (f124): measured behaviour through the effect seam and the stateful Run.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-revdelay-test-{}-{name}", std::process::id()));
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

fn doc(dir: &std::path::Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn run(effect: &str, args: &str, d: &DocState, range: std::ops::Range<u64>) -> DocState {
    let e = effects::find(effect).unwrap();
    let p = effects::parse_params(e, args).unwrap();
    let cancel = AtomicBool::new(false);
    effects::apply(e, &p, d, range, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state
}

fn read(d: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![0.0; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

/// Sum of samples in a small window around `at` (a low-pass smears an impulse over a few frames;
/// its area is kept).
fn area(v: &[f32], at: usize) -> f32 {
    v[at - 3..at + 5].iter().sum()
}

fn energy(v: &[f32], from: f64, to: f64) -> f64 {
    v[(from * RATE as f64) as usize..(to * RATE as f64) as usize].iter().map(|x| (*x as f64).powi(2)).sum::<f64>()
}

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

#[test]
fn both_are_registered_with_settings() {
    for id in ["reverb", "delay"] {
        let e = effects::find(id).unwrap();
        assert!(!e.parameters().is_empty());
        effects::validate(e, &effects::Params::defaults(e)).unwrap();
    }
}

#[test]
fn delay_places_an_echo_at_the_set_time_with_the_set_feedback() {
    let tmp = TempDir::new("delay");
    let at = 1000usize;
    let mut x = vec![0.0f32; RATE as usize * 2];
    x[at] = 1.0;
    let d = doc(&tmp.0, &[x]);
    // 100 ms = 4800 frames; wet 50 %, feedback 50 %.
    let out = read(&run("delay", "time=100ms feedback=50% wet=50%", &d, 0..0), 0);
    let (dry, e1, e2, e3) = (area(&out, at), area(&out, at + 4800), area(&out, at + 9600), area(&out, at + 14400));
    assert!((dry - 0.5).abs() < 0.03, "dry part {dry}");
    assert!((e1 - 0.5).abs() < 0.03, "first echo {e1} at +100 ms");
    assert!((e2 / e1 - 0.5).abs() < 0.04, "second echo is the feedback times the first: {e2} / {e1}");
    assert!((e3 / e2 - 0.5).abs() < 0.04, "third echo: {e3} / {e2}");
    // Nothing between the echoes.
    assert!(out[at + 600..at + 4000].iter().all(|v| v.abs() < 1e-4), "silence between dry and first echo");
}

#[test]
fn delay_echo_crossing_a_chunk_boundary_is_where_it_should_be() {
    let tmp = TempDir::new("delay-chunk");
    let at = BLOCK_FRAMES - 2000;
    let mut x = vec![0.0f32; BLOCK_FRAMES * 2];
    x[at] = 1.0;
    let d = doc(&tmp.0, &[x]);
    let out = read(&run("delay", "time=100ms feedback=0% wet=100%", &d, 0..0), 0);
    assert!(at + 4800 > BLOCK_FRAMES, "the echo lands in the second chunk");
    assert!((area(&out, at + 4800) - 1.0).abs() < 0.03, "echo after the boundary: {}", area(&out, at + 4800));
}

#[test]
fn reverb_tail_decays_over_the_set_time_and_longer_decay_rings_longer() {
    let tmp = TempDir::new("reverb");
    let mut x = noise(RATE as usize / 10, 7);
    x.resize(RATE as usize * 6, 0.0);
    let d = doc(&tmp.0, &[x]);
    let short = read(&run("reverb", "decay=1s wet=100%", &d, 0..0), 0);
    let long = read(&run("reverb", "decay=4s wet=100%", &d, 0..0), 0);
    let early = energy(&short, 0.3, 0.4);
    assert!(early > 1e-3, "there is a tail right after the burst: {early}");
    // RT60 = 1 s means -60 dB one second later; require at least 30 dB of the 60.
    let drop_db = 10.0 * (early / energy(&short, 1.3, 1.4).max(1e-30)).log10();
    assert!(drop_db > 30.0, "1 s decay fell only {drop_db} dB in one second");
    assert!(energy(&long, 2.3, 2.4) > 50.0 * energy(&short, 2.3, 2.4), "a 4 s decay still rings when a 1 s one is gone");
    // The tail that falls past the selection's end is cut there.
    let part = read(&run("reverb", "decay=4s wet=100%", &d, 0..(RATE as u64 / 2)), 0);
    assert_eq!(part.len(), short.len(), "length kept");
    assert!(part[RATE as usize / 2..].iter().all(|v| *v == 0.0), "nothing written past the selection");
}

#[test]
fn reverb_in_chunks_matches_one_pass_through_the_plugin() {
    let tmp = TempDir::new("reverb-chunk");
    let n = BLOCK_FRAMES * 2 + 5000;
    let x = noise(n, 99);
    let d = doc(&tmp.0, &[x.clone(), noise(n, 5)]);
    let out = read(&run("reverb", "wet=60%", &d, 0..0), 0);
    let mut p = soundcraft_dsp::create("room_reverb").unwrap();
    p.prepare(RATE as f32, 4096, 2);
    for (k, v) in [("size", 50.0), ("decay", 1.2), ("damping", 6000.0), ("pre_delay", 10.0), ("mix", 60.0)] {
        assert!(p.set_param(k, v));
    }
    p.reset();
    let mut io = vec![x, noise(n, 5)];
    p.process(&mut io, n);
    let worst = out.iter().zip(&io[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "chunked run differs from one pass by {worst}: state was lost at a chunk join");
}
