//! DeHummer (B073): mains hum and harmonics are cut, other tones stay, the filter state runs
//! across chunk joins, and the whole thing is one undo step.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, DeHummer, Effect, RunContext};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dehum-test-{}-{name}", std::process::id()));
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

fn doc(dir: &Path, rate: u32, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap()
}

/// Sum of sines, each at 0.1 (-20 dB).
fn tones(freqs: &[f64], rate: u32, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| freqs.iter().map(|f| 0.1 * (2.0 * PI * f * n as f64 / f64::from(rate)).sin()).sum::<f64>() as f32).collect()
}

/// Level in dB of the component at `freq` over `x` (Goertzel).
fn level_db(x: &[f32], freq: f64, rate: u32) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, v) in x.iter().enumerate() {
        let ph = 2.0 * PI * freq * n as f64 / f64::from(rate);
        re += f64::from(*v) * ph.cos();
        im += f64::from(*v) * ph.sin();
    }
    20.0 * (2.0 * (re * re + im * im).sqrt() / x.len() as f64).log10()
}

fn run(input: &[f32], rate: u32, params: &[f64]) -> Vec<f32> {
    let tmp = TempDir::new(&format!("run{}", params.iter().map(|p| p.to_string()).collect::<String>()));
    let d = doc(&tmp.0, rate, std::slice::from_ref(&input.to_vec()));
    let edit = effects::apply(&DeHummer, params, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
    let mut out = vec![f32::NAN; input.len()];
    assert_eq!(edit.state.read(0, 0, &mut out).unwrap(), out.len());
    out
}

fn defaults() -> Vec<f64> {
    effects::Params::defaults(&DeHummer)
}

/// Change in dB at `freq` between input and output, measured on the last second (settled).
fn moved(input: &[f32], output: &[f32], freq: f64, rate: u32) -> f64 {
    let tail = input.len() - rate as usize..;
    level_db(&output[tail.clone()], freq, rate) - level_db(&input[tail], freq, rate)
}

#[test]
fn the_default_cuts_60_hz_and_its_harmonics_and_leaves_the_rest() {
    let hum = [60.0, 120.0, 180.0, 240.0, 300.0];
    let mut all = hum.to_vec();
    all.extend([1000.0, 90.0]);
    let input = tones(&all, RATE, RATE as usize * 3);
    let out = run(&input, RATE, &defaults());
    for f in hum {
        let d = moved(&input, &out, f, RATE);
        assert!(d <= -30.0, "{f} Hz moved {d} dB");
    }
    assert!(moved(&input, &out, 1000.0, RATE).abs() < 0.5);
    assert!(moved(&input, &out, 90.0, RATE).abs() < 3.0);
}

#[test]
fn fifty_hertz_mode_cuts_the_multiples_of_50() {
    let hum = [50.0, 100.0, 150.0, 200.0, 250.0];
    let mut all = hum.to_vec();
    all.extend([1000.0, 75.0]);
    let input = tones(&all, RATE, RATE as usize * 3);
    let mut p = defaults();
    p[0] = 50.0;
    let out = run(&input, RATE, &p);
    for f in hum {
        let d = moved(&input, &out, f, RATE);
        assert!(d <= -30.0, "{f} Hz moved {d} dB");
    }
    assert!(moved(&input, &out, 1000.0, RATE).abs() < 0.5);
    assert!(moved(&input, &out, 75.0, RATE).abs() < 3.0);
}

#[test]
fn skip_low_leaves_the_fundamental_and_still_cuts_the_next_harmonic() {
    let input = tones(&[60.0, 120.0, 1000.0], RATE, RATE as usize * 3);
    let mut p = defaults();
    p[4] = 1.0;
    let out = run(&input, RATE, &p);
    assert!(moved(&input, &out, 60.0, RATE).abs() < 0.5);
    assert!(moved(&input, &out, 120.0, RATE) <= -30.0);
}

#[test]
fn a_harmonic_above_045_of_the_sample_rate_is_skipped_without_nan() {
    // At 2 kHz the limit is 900 Hz: harmonics 15 to 20 of 60 Hz are out of reach.
    let rate = 2000;
    let input = tones(&[60.0, 120.0, 840.0, 960.0, 1020.0], rate, rate as usize * 3);
    let mut p = defaults();
    p[1] = 20.0;
    let out = run(&input, rate, &p);
    assert!(out.iter().all(|v| v.is_finite()));
    assert!(moved(&input, &out, 120.0, rate) <= -30.0);
    assert!(moved(&input, &out, 840.0, rate) <= -30.0, "14th harmonic, below the limit");
    assert!(moved(&input, &out, 960.0, rate).abs() < 0.5, "16th harmonic, above the limit: skipped");
    assert!(moved(&input, &out, 1020.0, rate).abs() < 0.5, "17th harmonic, above the limit: skipped");
}

#[test]
fn chunked_output_equals_one_pass_for_a_selection_starting_off_zero() {
    let frames = BLOCK_FRAMES * 2 + 5000;
    let input = tones(&[60.0, 120.0, 1000.0], RATE, frames);
    let tmp = TempDir::new("chunks");
    let d = doc(&tmp.0, RATE, std::slice::from_ref(&input));
    let range = 1234..(frames as u64 - 777);
    let params = defaults();
    let edit = effects::apply(&DeHummer, &params, &d, range.clone(), ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
    let mut got = vec![f32::NAN; frames];
    assert_eq!(edit.state.read(0, 0, &mut got).unwrap(), frames);

    let ctx = RunContext { doc: &d, range: range.clone(), channels: vec![0], sample_rate: RATE };
    let mut one = DeHummer.begin(&params, &ctx).unwrap().unwrap();
    let src = &input[range.start as usize..range.end as usize];
    let mut want = vec![0.0f32; src.len()];
    one.process(range.start, &[src], &mut [&mut want[..]]).unwrap();

    assert_eq!(&got[..range.start as usize], &input[..range.start as usize], "before the selection untouched");
    assert_eq!(&got[range.end as usize..], &input[range.end as usize..], "after the selection untouched");
    let mut worst = 0.0f32;
    for (a, b) in got[range.start as usize..range.end as usize].iter().zip(&want) {
        worst = worst.max((a - b).abs());
    }
    assert!(worst <= 1e-6, "chunked vs one pass differ by {worst}");
    assert!(want.iter().any(|v| v.abs() > 0.01), "the comparison is not all silence");
}

#[test]
fn it_runs_from_the_editor_as_one_undo_step_and_refuses_q_500() {
    let tmp = TempDir::new("undo");
    let input = tones(&[60.0, 1000.0], RATE, RATE as usize);
    let mut e = Editor::new(History::new(doc(&tmp.0, RATE, std::slice::from_ref(&input)), u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = effects::all().iter().position(|x| x.id() == "dehummer").unwrap() as u16;

    let mut bad = defaults();
    bad[2] = 500.0;
    assert!(e.run_effect(i, bad.clone(), 0..0).is_err(), "q = 500 is refused");
    assert!(effects::validate(&DeHummer, &bad).is_err());
    assert!(!e.history.can_undo(), "a refused run changes nothing");

    e.run_effect(i, defaults(), 0..0).unwrap();
    assert_eq!(e.history.undo_label(), Some("DeHummer"));
    e.history.undo().unwrap();
    assert!(!e.history.can_undo(), "one undo step takes it all back");
    let mut back = vec![f32::NAN; input.len()];
    e.history.current().read(0, 0, &mut back).unwrap();
    assert_eq!(back, input);
}
