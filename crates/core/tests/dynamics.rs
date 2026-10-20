//! Dynamics (B029): gate/expander, compressor and limiter in one stateful effect.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{
    self, Effect, Run, RunContext,
    dynamics::{Dynamics, static_curve},
};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dyn-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path) -> Store {
    Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

fn doc(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = store(dir);
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn params(args: &str) -> Vec<f64> {
    effects::parse_params(&Dynamics, args).unwrap()
}

/// Run Dynamics through the seam over `range` and return the whole document afterwards.
fn run(name: &str, args: &str, data: &[Vec<f32>], range: std::ops::Range<u64>) -> Vec<Vec<f32>> {
    let tmp = TempDir::new(name);
    let d = doc(&tmp.0, data);
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(&Dynamics, &params(args), &d, range, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    (0..data.len())
        .map(|c| {
            let mut v = vec![f32::NAN; data[c].len()];
            assert_eq!(edit.state.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

fn amp(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

fn sine(level_db: f64, hz: f64, frames: usize) -> Vec<f32> {
    let a = amp(level_db) as f64;
    (0..frames).map(|n| (a * (2.0 * std::f64::consts::PI * hz * n as f64 / RATE as f64).sin()) as f32).collect()
}

fn peak(x: &[f32]) -> f64 {
    x.iter().fold(0.0f64, |m, v| m.max(v.abs() as f64))
}

#[test]
fn static_curve_follows_the_settings() {
    let p = params("knee_db=0");
    assert!((static_curve(&p, -40.0) - -40.0).abs() < 1e-9, "unity between the gate and the compressor");
    assert!((static_curve(&p, -20.0) - -20.0).abs() < 1e-9);
    let out = static_curve(&p, -10.0);
    assert!((out - -16.666_666).abs() < 1e-4, "3:1 above -20 dB: -10 in gives {out}");
    assert!((static_curve(&p, 0.0) - -13.333_333).abs() < 1e-4);
    let lim = params("knee_db=0 comp_ratio=1 limit_db=-1");
    assert!((static_curve(&lim, 6.0) - -1.0).abs() < 1e-9, "the limiter caps at -1 dB");
    let exp = params("expand_ratio=2");
    assert!((static_curve(&exp, -70.0) - -80.0).abs() < 1e-9, "2:1 expander below -60: -70 in gives -80");
    assert!((static_curve(&exp, -60.0) - -60.0).abs() < 1e-9);
    let mk = params("makeup_db=6");
    assert!((static_curve(&mk, -40.0) - -34.0).abs() < 1e-9);
    // the soft knee is continuous and between the straight lines
    let soft = params("knee_db=6");
    let a = static_curve(&soft, -20.0);
    assert!(a < -20.0 && a > -21.0, "{a}");
}

#[test]
fn steady_sines_reach_the_static_curve_level() {
    let frames = RATE as usize * 4;
    let p = params("");
    for level in [-40.0, -10.0, 0.0] {
        let out = run("sines", "", &[sine(level, 1000.0, frames)], 0..0);
        let tail = &out[0][frames - RATE as usize / 2..];
        let got = db(peak(tail));
        let want = static_curve(&p, level);
        assert!((got - want).abs() < 0.5, "sine at {level} dBFS: got {got:.2} dB, curve says {want:.2} dB");
    }
}

/// A constant input makes the detector's rise and fall easy to read: with the threshold at the
/// floor and a ratio of 20, the gain in dB is a straight line of the smoothed level.
fn gain_trace(args: &str, steps: &[(f64, usize)]) -> Vec<f64> {
    let mut input = Vec::new();
    for (level, n) in steps {
        input.extend(std::iter::repeat_n(amp(*level), *n));
    }
    let out = run("trace", args, &[input.clone()], 0..0);
    out[0].iter().zip(&input).map(|(o, i)| db((*o / *i) as f64)).collect()
}

/// Frames until `trace` has covered 63.2 % of the way from `from` to `to`.
fn time_constant_ms(trace: &[f64], start: usize, from: f64, to: f64) -> f64 {
    let target = from + (to - from) * 0.632_120_6;
    let hit = trace[start..].iter().position(|g| if to < from { *g <= target } else { *g >= target }).unwrap();
    hit as f64 * 1000.0 / RATE as f64
}

#[test]
fn attack_and_release_follow_their_time_constants() {
    let base = "comp_db=-60 comp_ratio=20 knee_db=0 limit_db=0 attack_ms=20 release_ms=200";
    let n1 = RATE as usize / 2;
    let n2 = RATE as usize * 3;
    let trace = gain_trace(base, &[(-30.0, n1), (-10.0, n2), (-30.0, n2)]);
    let settled = |at: usize| trace[at];
    let g_lo = settled(n1 - 1);
    let g_hi = settled(n1 + n2 - 1);
    assert!(g_hi < g_lo - 5.0, "more reduction at the louder level: {g_lo} -> {g_hi}");
    let attack = time_constant_ms(&trace, n1, g_lo, g_hi);
    assert!((attack - 20.0).abs() < 6.0, "attack time constant {attack} ms, wanted 20 ms within 30 %");
    let release = time_constant_ms(&trace, n1 + n2, g_hi, trace[n1 + 2 * n2 - 1]);
    assert!((release - 200.0).abs() < 60.0, "release time constant {release} ms, wanted 200 ms within 30 %");
}

#[test]
fn the_limiter_holds_a_full_scale_burst_with_lookahead() {
    let mut x = vec![0.0f32; RATE as usize / 10];
    x.extend(sine(0.0, 1000.0, RATE as usize / 20));
    x.extend(vec![0.0f32; RATE as usize / 10]);
    let out = run("limiter", "comp_ratio=1 limit_db=-1 lookahead_ms=5", std::slice::from_ref(&x), 0..0);
    let top = db(peak(&out[0]));
    assert!(top <= -1.0 + 0.1, "peak {top} dB above the -1 dB ceiling");
    assert!(top > -3.0, "the burst is limited, not removed: {top} dB");
}

#[test]
fn linked_gain_follows_the_loud_channel_and_per_channel_does_not() {
    let frames = RATE as usize * 2;
    let loud = sine(-6.0, 700.0, frames);
    let quiet = sine(-40.0, 1100.0, frames);
    let args = "comp_db=-20 comp_ratio=4 knee_db=0 limit_db=0";
    let tail = frames - RATE as usize / 4;
    let linked = run("linked", &format!("{args} link=linked"), &[loud.clone(), quiet.clone()], 0..0);
    let split = run("split", &format!("{args} link=1"), &[loud.clone(), quiet.clone()], 0..0);
    let drop_db = |out: &[f32], inp: &[f32]| db(peak(&out[tail..])) - db(peak(&inp[tail..]));
    let l1 = drop_db(&linked[1], &quiet);
    assert!(l1 < -5.0, "linked: the quiet channel is turned down with the loud one ({l1} dB)");
    assert!((drop_db(&linked[0], &loud) - l1).abs() < 0.5, "linked: both channels get the same gain");
    assert!(drop_db(&split[1], &quiet).abs() < 0.1, "per channel: the quiet channel is untouched");
    assert!(drop_db(&split[0], &loud) < -5.0, "per channel: the loud channel is compressed");
}

#[test]
fn all_ratios_one_and_no_makeup_is_the_identity() {
    let frames = 20_000;
    let x: Vec<f32> = (0..frames).map(|n| 0.8 * ((n * 7919 % 2001) as f32 / 1000.0 - 1.0)).collect();
    let out = run("identity", "expand_ratio=1 comp_ratio=1 limit_db=0 makeup_db=0", std::slice::from_ref(&x), 0..0);
    let worst = x.iter().zip(&out[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst <= 1e-6, "worst difference {worst}");
}

/// Reference: the same run fed the whole selection in one piece.
fn one_piece(args: &str, data: &[Vec<f32>], range: std::ops::Range<u64>) -> Vec<Vec<f32>> {
    let tmp = TempDir::new("onepiece");
    let d = doc(&tmp.0, data);
    let ctx = RunContext { doc: &d, range: range.clone(), channels: (0..data.len()).collect(), sample_rate: RATE };
    let mut r: Box<dyn Run> = Dynamics.begin(&params(args), &ctx).unwrap().expect("Dynamics is stateful");
    let input: Vec<&[f32]> = data.iter().map(|c| &c[range.start as usize..range.end as usize]).collect();
    let mut out: Vec<Vec<f32>> = input.iter().map(|c| vec![0.0; c.len()]).collect();
    let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|c| c.as_mut_slice()).collect();
    r.process(range.start, &input, &mut outs).unwrap();
    out
}

#[test]
fn chunked_equals_unchunked_across_a_join_with_a_selection_that_starts_late() {
    let frames = BLOCK_FRAMES * 3 + 777;
    let mk = |seed: usize| -> Vec<f32> {
        (0..frames).map(|n| 0.9 * (((n * 31 + seed) % 977) as f32 / 488.5 - 1.0) * (0.3 + 0.7 * ((n as f32 / 9000.0).sin()).abs())).collect()
    };
    let data = vec![mk(3), mk(500)];
    let range = 1234..(1234 + 2 * BLOCK_FRAMES as u64 + 300);
    for args in ["lookahead_ms=0", "lookahead_ms=3 attack_ms=2 release_ms=30 makeup_db=3"] {
        let got = run("chunked", args, &data, range.clone());
        let want = one_piece(args, &data, range.clone());
        for c in 0..2 {
            let s = range.start as usize;
            let worst = want[c].iter().enumerate().map(|(i, w)| (got[c][s + i] - w).abs()).fold(0.0f32, f32::max);
            assert!(worst <= 1e-6, "{args}: channel {c} differs by {worst}");
            assert_eq!(got[c][s - 1], data[c][s - 1], "before the selection is untouched");
            assert_eq!(got[c][range.end as usize], data[c][range.end as usize], "after the selection is untouched");
        }
    }
}

#[test]
fn one_undo_restores_and_the_step_is_named() {
    let tmp = TempDir::new("undo");
    let x = sine(-3.0, 440.0, 5000);
    let mut e = Editor::new(History::new(doc(&tmp.0, std::slice::from_ref(&x)), u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = match by_name("effect.dynamics").expect("registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    };
    e.run_effect(i, params(""), 0..5000).unwrap();
    assert_eq!(e.history.undo_label(), Some("Dynamics"));
    let read = |e: &Editor| {
        let d = e.history.current();
        let mut v = vec![0.0f32; 5000];
        d.read(0, 0, &mut v).unwrap();
        v
    };
    assert_ne!(read(&e), x);
    e.history.undo().unwrap();
    assert_eq!(read(&e), x);
}

#[test]
fn out_of_range_settings_are_refused() {
    for bad in [
        "gate_db=5",
        "expand_ratio=0.5",
        "comp_ratio=21",
        "knee_db=13",
        "limit_db=-30",
        "attack_ms=0",
        "release_ms=4000",
        "lookahead_ms=11",
        "makeup_db=30",
    ] {
        assert!(effects::parse_params(&Dynamics, bad).is_err(), "{bad} should be refused");
    }
    let tmp = TempDir::new("refuse");
    let d = doc(&tmp.0, &[vec![0.1; 100]]);
    let mut p = params("");
    p[3] = 99.0;
    let cancel = AtomicBool::new(false);
    assert!(effects::apply(&Dynamics, &p, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).is_err());
    assert!(p.len() == Dynamics.parameters().len());
}
