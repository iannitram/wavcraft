//! Automatic Phase Correction (B076): lines up two channels recorded with a time offset.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect, RunContext, parse_params};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-phase-test-{}-{name}", std::process::id()));
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

/// Filtered white noise (a one-pole low-pass of a deterministic generator): pink-ish, never periodic.
fn noise(frames: usize, seed: u64) -> Vec<f32> {
    let mut s = seed;
    let mut lp = 0.0f32;
    (0..frames)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((s >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0;
            lp = 0.9 * lp + 0.1 * w;
            lp * 3.0
        })
        .collect()
}

/// `x` delayed by `d` frames (zeros in front), same length.
fn delayed(x: &[f32], d: usize) -> Vec<f32> {
    (0..x.len()).map(|k| if k >= d { x[k - d] } else { 0.0 }).collect()
}

fn corr(a: &[f32], b: &[f32]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        ab += f64::from(*x) * f64::from(*y);
        aa += f64::from(*x) * f64::from(*x);
        bb += f64::from(*y) * f64::from(*y);
    }
    ab / (aa * bb).sqrt()
}

fn read_all(d: &DocState) -> Vec<Vec<f32>> {
    (0..d.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; d.len_frames() as usize];
            assert_eq!(d.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn fx() -> &'static dyn Effect {
    effects::find("phase_correction").expect("phase_correction is registered")
}

fn run(d: &DocState, args: &str) -> Vec<Vec<f32>> {
    let params = parse_params(fx(), args).unwrap();
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(fx(), &params, d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    read_all(&edit.state)
}

const FRAMES: usize = BLOCK_FRAMES * 3 + 7_000;

#[test]
fn global_mode_measures_the_lag_and_lines_the_channels_up() {
    let tmp = TempDir::new("global");
    let l = noise(FRAMES, 1);
    let r = delayed(&l, 37);
    let d = doc(&tmp.0, &[l, r]);
    let m = wavcraft_core::effects::measure_lag(&d, 0..FRAMES as u64, [0, 1], 220).unwrap();
    assert_eq!(m, (37, false));
    let out = run(&d, "mode=global polarity=auto");
    let c = corr(&out[0][200..FRAMES - 200], &out[1][200..FRAMES - 200]);
    assert!(c >= 0.99, "correlation {c}");
}

#[test]
fn a_negative_correlation_is_flipped_and_aligned() {
    let tmp = TempDir::new("polarity");
    let l = noise(FRAMES, 2);
    let r: Vec<f32> = delayed(&l, 12).iter().map(|v| -v).collect();
    let d = doc(&tmp.0, &[l, r]);
    let m = wavcraft_core::effects::measure_lag(&d, 0..FRAMES as u64, [0, 1], 220).unwrap();
    assert_eq!(m, (12, true));
    let out = run(&d, "polarity=auto");
    let c = corr(&out[0][200..FRAMES - 200], &out[1][200..FRAMES - 200]);
    assert!(c >= 0.99, "correlation {c}");
    // With polarity off the channels are aligned but stay opposed.
    let out = run(&d, "polarity=off");
    let c = corr(&out[0][200..FRAMES - 200], &out[1][200..FRAMES - 200]);
    assert!(c <= -0.99, "correlation {c}");
}

#[test]
fn the_earlier_channel_is_the_one_delayed() {
    let tmp = TempDir::new("earlier");
    let r = noise(FRAMES, 3);
    let l = delayed(&r, 20);
    let d = doc(&tmp.0, &[l.clone(), r.clone()]);
    let out = run(&d, "");
    assert_eq!(out[0], l, "the later channel is untouched");
    assert_eq!(&out[1][20..], &r[..FRAMES - 20]);
    assert!(out[1][..20].iter().all(|v| *v == 0.0));
}

#[test]
fn mono_passes_unchanged() {
    let tmp = TempDir::new("mono");
    let x = noise(10_000, 4);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    assert_eq!(run(&d, ""), vec![x]);
}

#[test]
fn track_mode_follows_a_lag_that_steps_halfway() {
    let tmp = TempDir::new("track");
    let l = noise(FRAMES, 5);
    let half = FRAMES / 2;
    let r: Vec<f32> = (0..FRAMES)
        .map(|k| {
            let lag = if k < half { 10 } else { 20 };
            if k >= lag { l[k - lag] } else { 0.0 }
        })
        .collect();
    let d = doc(&tmp.0, &[l.clone(), r]);
    let out = run(&d, "mode=track window_ms=100");
    let win = (RATE as usize) / 10;
    let a = corr(&out[0][200..half - win], &out[1][200..half - win]);
    let b = corr(&out[0][half + 2 * win..FRAMES - 200], &out[1][half + 2 * win..FRAMES - 200]);
    assert!(a >= 0.95 && b >= 0.95, "first half {a}, second half {b}");
    let max_step = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let limit = 2.0 * l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    for (c, ch) in out.iter().enumerate() {
        assert!(max_step(&ch[half - 2 * win..half + 2 * win]) <= limit, "channel {c} steps too far at the change");
    }
}

#[test]
fn chunk_joins_give_the_same_result_as_a_single_pass() {
    let tmp = TempDir::new("join");
    let l = noise(FRAMES, 6);
    let r = delayed(&l, 37);
    let d = doc(&tmp.0, &[l.clone(), r.clone()]);
    for args in ["mode=global", "mode=track window_ms=50"] {
        let chunked = run(&d, args);
        let params = parse_params(fx(), args).unwrap();
        let ctx = RunContext { doc: &d, range: 0..FRAMES as u64, channels: vec![0, 1], sample_rate: RATE };
        let mut r_run = fx().begin(&params, &ctx).unwrap().expect("a run");
        let mut single = vec![vec![0.0f32; FRAMES]; 2];
        {
            let (a, b) = single.split_at_mut(1);
            r_run.process(0, &[&l, &r], &mut [&mut a[0][..], &mut b[0][..]]).unwrap();
        }
        for (c, (x, y)) in chunked.iter().zip(&single).enumerate() {
            for (k, (p, q)) in x.iter().zip(y).enumerate() {
                assert!((p - q).abs() <= 1e-6, "{args}: channel {c} frame {k}");
            }
        }
    }
}

#[test]
fn one_undo_restores_and_is_named_for_the_effect() {
    let tmp = TempDir::new("undo");
    let l = noise(20_000, 7);
    let r = delayed(&l, 30);
    let d = doc(&tmp.0, &[l, r]);
    let before = read_all(&d);
    let mut e = Editor::new(History::new(d, u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = effects::all().iter().position(|x| x.id() == "phase_correction").unwrap() as u16;
    e.run_effect(i, parse_params(fx(), "").unwrap(), 0..0).unwrap();
    assert_eq!(e.history.undo_label(), Some("Automatic Phase Correction"));
    assert_ne!(read_all(e.history.current()), before);
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(read_all(e.history.current()), before);
}

#[test]
fn a_selection_not_at_frame_zero_is_corrected_and_the_rest_is_untouched() {
    let tmp = TempDir::new("selection");
    let l = noise(FRAMES, 8);
    let r = delayed(&l, 25);
    let d = doc(&tmp.0, &[l.clone(), r.clone()]);
    let range = 9_000u64..(FRAMES as u64 - 5_000);
    let (s, e) = (range.start as usize, range.end as usize);
    for args in ["mode=global", "mode=track window_ms=50"] {
        let params = parse_params(fx(), args).unwrap();
        let cancel = AtomicBool::new(false);
        let edit = effects::apply(fx(), &params, &d, range.clone(), ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
        let out = read_all(&edit.state);
        assert_eq!(&out[0][..s], &l[..s], "{args}: before the range");
        assert_eq!(&out[0][e..], &l[e..], "{args}: after the range");
        assert_eq!(&out[1][..s], &r[..s]);
        let skip = s + 5_000;
        let c = corr(&out[0][skip..e - 100], &out[1][skip..e - 100]);
        assert!(c >= 0.99, "{args}: correlation {c}");
    }
}
