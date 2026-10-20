//! DeReverb (B074): the tail between bursts of sound drops, the bursts barely change, amount 0
//! is a pass-through, and the result does not depend on how the range was cut into chunks.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, RunContext};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const PERIOD: usize = 24_000; // 500 ms
const ON: usize = 4_800; // 100 ms
const FRAMES: usize = 8 * PERIOD + 6_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dereverb-test-{}-{name}", std::process::id()));
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

/// Deterministic uniform noise in [-1, 1).
fn noise(frames: usize, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..frames)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 11) as f64 / (1u64 << 52) as f64 - 1.0) as f32
        })
        .collect()
}

/// Bursts of low-passed noise, 100 ms on and 400 ms off.
fn dry(seed: u64) -> Vec<f32> {
    let n = noise(FRAMES, seed);
    let mut lp = 0.0f32;
    n.iter()
        .enumerate()
        .map(|(i, v)| {
            lp += 0.35 * (v - lp);
            if i % PERIOD < ON { lp * 0.5 } else { 0.0 }
        })
        .collect()
}

/// `x` convolved with an exponentially decaying noise impulse response (60 dB over 600 ms).
fn reverberate(x: &[f32], seed: u64) -> Vec<f32> {
    let ir_len = (0.6 * f64::from(RATE)) as usize;
    let ir: Vec<f32> = noise(ir_len, seed).iter().enumerate().map(|(i, v)| v * 10f32.powf(-3.0 * i as f32 / ir_len as f32) * 0.05).collect();
    let n = (x.len() + ir_len).next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    let spectrum = |s: &[f32]| {
        let mut b: Vec<Complex32> = s.iter().map(|v| Complex32::new(*v, 0.0)).collect();
        b.resize(n, Complex32::new(0.0, 0.0));
        fwd.process(&mut b);
        b
    };
    let (a, b) = (spectrum(x), spectrum(&ir));
    let mut prod: Vec<Complex32> = a.iter().zip(&b).map(|(p, q)| p * q).collect();
    inv.process(&mut prod);
    let mut wet: Vec<f32> = prod[..x.len()].iter().map(|c| c.re / n as f32).collect();
    // Keep the direct sound at its own level: add the dry signal on top of the tail.
    wet.iter_mut().zip(x).for_each(|(w, d)| *w += d);
    wet
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|v| f64::from(*v).powi(2)).sum()
}

fn db(ratio: f64) -> f64 {
    10.0 * ratio.log10()
}

fn read_all(d: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn params(amount: f64) -> Vec<f64> {
    let e = effects::find("dereverb").expect("dereverb is registered");
    effects::parse_params(e, &format!("amount={amount}")).unwrap()
}

fn run(d: &DocState, range: std::ops::Range<u64>, amount: f64) -> DocState {
    let e = effects::find("dereverb").unwrap();
    let cancel = AtomicBool::new(false);
    effects::apply(e, &params(amount), d, range, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state
}

/// Energy of `x` over the tail-only part of each gap and over the bursts, across periods 1..7.
fn gap_and_burst(x: &[f32]) -> (f64, f64) {
    let (mut gap, mut burst) = (0.0, 0.0);
    for p in 1..7 {
        let t = p * PERIOD;
        burst += energy(&x[t + 480..t + ON]);
        gap += energy(&x[t + ON + 7_200..t + PERIOD - 480]);
    }
    (gap, burst)
}

#[test]
fn full_amount_cuts_the_tail_and_keeps_the_bursts() {
    let tmp = TempDir::new("tail");
    let wet = reverberate(&dry(1), 2);
    let d = doc(&tmp.0, &[wet.clone()]);
    let out = read_all(&run(&d, 0..FRAMES as u64, 100.0), 0);
    let ((gw, bw), (go, bo)) = (gap_and_burst(&wet), gap_and_burst(&out));
    let (gap_drop, burst_drop) = (db(gw / go), db(bw / bo));
    println!("gap drop {gap_drop:.2} dB, burst drop {burst_drop:.2} dB");
    assert!(gap_drop >= 6.0, "the tail dropped only {gap_drop:.2} dB");
    assert!(burst_drop <= 2.0, "the bursts dropped {burst_drop:.2} dB");
}

#[test]
fn amount_zero_is_a_pass_through() {
    let tmp = TempDir::new("zero");
    let wet = reverberate(&dry(3), 4);
    let d = doc(&tmp.0, &[wet.clone()]);
    let out = read_all(&run(&d, 7_000..150_000, 0.0), 0);
    let err: f64 = out.iter().zip(&wet).map(|(a, b)| f64::from(a - b).powi(2)).sum();
    assert!((err / out.len() as f64).sqrt() < 1e-4);
}

#[test]
fn a_range_of_three_chunks_from_a_non_zero_frame_equals_one_pass() {
    let tmp = TempDir::new("chunks");
    let wet = reverberate(&dry(5), 6);
    let d = doc(&tmp.0, &[wet]);
    let start = 3_001u64;
    let end = start + 2 * BLOCK_FRAMES as u64 + 9_000;
    // Through the seam: three 64 K chunks.
    let seam = read_all(&run(&d, start..end, 100.0), 0);
    // One pass: a single call of the run over the whole range.
    let e = effects::find("dereverb").unwrap();
    let ctx = RunContext { doc: &d, range: start..end, channels: vec![0], sample_rate: RATE };
    let mut one = e.begin(&params(100.0), &ctx).unwrap().expect("a run");
    let all = {
        let mut v = vec![0.0f32; (end - start) as usize];
        d.read(0, start, &mut v).unwrap();
        v
    };
    let mut whole = vec![0.0f32; all.len()];
    one.process(0, &[&all], &mut [&mut whole]).unwrap();
    let max = whole.iter().zip(&seam[start as usize..end as usize]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(max <= 1e-5, "chunked differs from one pass by {max}");
    // Outside the range nothing changed.
    let orig = read_all(&d, 0);
    assert_eq!(&seam[..start as usize], &orig[..start as usize]);
    assert_eq!(&seam[end as usize..], &orig[end as usize..]);
}

#[test]
fn mono_and_stereo_both_work_and_stereo_channels_match_their_mono() {
    let tmp = TempDir::new("stereo");
    let l = reverberate(&dry(7), 8);
    let r = reverberate(&dry(9), 10);
    let mono_l = read_all(&run(&doc(&tmp.0.join("a"), &[l.clone()]), 0..FRAMES as u64, 100.0), 0);
    let stereo = run(&doc(&tmp.0.join("b"), &[l, r.clone()]), 0..FRAMES as u64, 100.0);
    let mono_r = read_all(&run(&doc(&tmp.0.join("c"), &[r]), 0..FRAMES as u64, 100.0), 0);
    let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
    assert!(diff(&read_all(&stereo, 0), &mono_l) <= 1e-6);
    assert!(diff(&read_all(&stereo, 1), &mono_r) <= 1e-6);
}

#[test]
fn it_is_one_undo_step_named_for_the_effect() {
    let tmp = TempDir::new("undo");
    let wet = reverberate(&dry(11), 12);
    let d = doc(&tmp.0, &[wet.clone()]);
    let mut e = Editor::new(History::new(d, u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = match by_name("effect.dereverb").expect("registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    };
    let out = e.run_effect(i, params(100.0), 0..FRAMES as u64).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("DeReverb"));
    assert_ne!(read_all(e.history.current(), 0), wet);
    e.history.undo().unwrap();
    assert_eq!(read_all(e.history.current(), 0), wet);
}
