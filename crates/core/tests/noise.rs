//! The STFT engine, noise prints and Noise Reduction (f29, B011 / A337 / B067).
//! The noise print is session-wide, so the tests that touch it hold `LOCK`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::noise;
use wavcraft_core::stft::{Placement, Stft};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-noise-test-{}-{name}", std::process::id()));
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

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The amplitude of a `hz` sinusoid in `x` (a single-bin DFT, which is exact for a steady tone).
fn tone_amplitude(x: &[f32], hz: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let ph = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
        re += f64::from(*v) * ph.cos();
        im += f64::from(*v) * ph.sin();
    }
    2.0 * (re * re + im * im).sqrt() / x.len() as f64
}

fn error_db(a: &[f32], b: &[f32]) -> f64 {
    let err: Vec<f32> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    db(rms(&err) / rms(a))
}

#[test]
fn analysis_then_resynthesis_with_no_change_is_the_input_to_minus_100_db() {
    let x = gaussian(50_000, 0.3, 1);
    for (n, overlap) in [(512, 50), (512, 75), (4096, 75), (2048, 50)] {
        let stft = Stft::new(n, overlap).unwrap();
        let mut out = vec![0.0f32; x.len()];
        stft.process(&x, &mut out, Placement { input_abs: 0, out_abs: 0, grid: 0, warmup: 0 }, |_| {});
        let e = error_db(&x, &out);
        assert!(e < -100.0, "fft {n} overlap {overlap} %: error {e:.1} dB");
    }
}

#[test]
fn the_grid_origin_and_a_chunked_pass_do_not_change_the_result() {
    let x = gaussian(20_000, 0.3, 2);
    let stft = Stft::new(1024, 75).unwrap();
    // One pass over [5000, 15000) with the grid at 5000, against 7 uneven chunks of it.
    let mut whole = vec![0.0f32; 10_000];
    stft.process(&x, &mut whole, Placement { input_abs: 0, out_abs: 5000, grid: 5000, warmup: 0 }, |s| s.iter_mut().for_each(|b| *b *= 0.5));
    let mut chunked = vec![0.0f32; 10_000];
    let mut at = 0;
    for len in [1, 1023, 2000, 1024, 3000, 1, 2951] {
        stft.process(&x, &mut chunked[at..at + len], Placement { input_abs: 0, out_abs: 5000 + at as i64, grid: 5000, warmup: 0 }, |s| {
            s.iter_mut().for_each(|b| *b *= 0.5)
        });
        at += len;
    }
    assert_eq!(at, 10_000);
    let worst = whole.iter().zip(&chunked).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "worst difference {worst}");
    // And a constant gain of 0.5 is exactly half the input.
    let half: Vec<f32> = x[5000..15000].iter().map(|v| v * 0.5).collect();
    assert!(error_db(&half, &whole) < -100.0);
}

/// Noise Reduction with `reduction=0` has a gain of exactly 1 in every bin, so it must give the
/// input back: this drives the engine through `apply`, the run's context reads and the chunk
/// joins, with a selection that starts off zero. (If `apply` handed the run
/// `range.start + at` instead of `at`, the audio it analysed would be 1234 frames off: the
/// identity then fails by a whole sample, not by rounding.)
#[test]
fn identity_through_the_document_is_exact_across_chunk_boundaries() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("identity");
    let frames = BLOCK_FRAMES * 3 + 777;
    let data = vec![gaussian(frames, 0.3, 3), gaussian(frames, 0.2, 4)];
    let mut e = editor(&tmp.0, &data);
    e.run(ActionId::CaptureNoisePrint, 0..RATE as u64).unwrap();
    let doc = e.history.current().clone();
    // A range that starts and ends off every chunk and block boundary and crosses several.
    let range = 1234..(frames as u64 - 4321);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let nr = effects::find("noise_reduction").unwrap();
    let edit = effects::apply(nr, &params("reduction=0"), &doc, range.clone(), wavcraft_core::doc::edit::ChannelMask::ALL, Crossfades::NONE, &cancel)
        .unwrap();
    for (c, original) in data.iter().enumerate() {
        let mut got = vec![0.0f32; frames];
        assert_eq!(edit.state.read(c, 0, &mut got).unwrap(), frames);
        let worst = got.iter().zip(original).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "channel {c}: worst sample difference {worst}");
        assert_eq!(&got[..1234], &original[..1234], "outside the range is untouched");
    }
}

fn tone_and_noise(sigma: f64) -> (Vec<f32>, usize, usize) {
    // 12 s: noise alone for the first 5 s, then a 1 kHz tone (amplitude 0.25) over it.
    let frames = 12 * RATE as usize;
    let mut x = gaussian(frames, sigma, 7);
    let tone_from = 5 * RATE as usize;
    for (i, v) in x.iter_mut().enumerate().skip(tone_from) {
        *v += 0.25 * (std::f64::consts::TAU * 1000.0 * i as f64 / f64::from(RATE)).sin() as f32;
    }
    (x, tone_from, frames)
}

fn nr_index() -> u16 {
    effects::all().iter().position(|e| e.id() == "noise_reduction").unwrap() as u16
}

fn params(text: &str) -> Vec<f64> {
    effects::parse_params(effects::all()[usize::from(nr_index())], text).unwrap()
}

#[test]
fn noise_reduction_lowers_the_noise_floor_by_the_set_amount_and_keeps_the_tone() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("nr");
    let (x, tone_from, frames) = tone_and_noise(0.02);
    let mut e = editor(&tmp.0, std::slice::from_ref(&x));
    // The print comes from the first 2 s (noise only), through the action.
    let a = by_name("nr.capture_print").unwrap();
    assert_eq!(a.keys[0].key, "P");
    assert!(a.keys[0].shift);
    let out = e.run(a.id, 0..2 * RATE as u64).unwrap();
    assert!(!out.doc_changed);
    assert!(out.message.unwrap().contains("Noise print captured"));
    assert_eq!(e.history.undo_label(), None, "capturing is not an edit");
    for reduce in [12.0, 24.0] {
        let mut e2 = editor(&tmp.0.join("x"), std::slice::from_ref(&x));
        e2.run_effect(nr_index(), params(&format!("reduce_db={reduce}")), 0..0).unwrap();
        assert_eq!(e2.history.undo_label(), Some("Noise Reduction"));
        let y = &now(&e2)[0];
        assert_eq!(y.len(), frames);
        // Noise-only region: skip the first/last 0.25 s.
        let region = (RATE as usize / 4)..(5 * RATE as usize - RATE as usize / 4);
        let drop = db(rms(&x[region.clone()]) / rms(&y[region]));
        assert!(drop >= reduce, "reduce {reduce} dB: the noise floor dropped {drop:.2} dB");
        assert!(drop <= reduce + 3.0, "reduce {reduce} dB: dropped {drop:.2} dB, far more than asked");
        // The tone, 6 s to 11 s.
        let span = (tone_from + RATE as usize)..(frames - RATE as usize);
        let change = db(tone_amplitude(&y[span.clone()], 1000.0) / tone_amplitude(&x[span], 1000.0));
        assert!(change.abs() < 1.0, "reduce {reduce} dB: the tone moved {change:.2} dB");
        eprintln!("reduce {reduce}: noise drop {drop:.2} dB, tone change {change:.3} dB");
    }
}

#[test]
fn noise_reduction_does_not_depend_on_how_the_range_is_cut_into_chunks() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("nr-chunks");
    let (x, _, frames) = tone_and_noise(0.02);
    let mut e = editor(&tmp.0, std::slice::from_ref(&x));
    e.run(ActionId::CaptureNoisePrint, 0..2 * RATE as u64).unwrap();
    // The whole file in one run (about 9 chunks), against the same audio as a selection that
    // starts mid-file with its grid there: both must leave the audio outside alone and be sane.
    let mut e1 = editor(&tmp.0.join("a"), std::slice::from_ref(&x));
    e1.run_effect(nr_index(), params("reduce_db=12"), 0..0).unwrap();
    let mut e2 = editor(&tmp.0.join("b"), std::slice::from_ref(&x));
    let sel = 100_003..(frames as u64 - 99_991);
    e2.run_effect(nr_index(), params("reduce_db=12"), sel.clone()).unwrap();
    let (a, b) = (&now(&e1)[0], &now(&e2)[0]);
    assert_eq!(&b[..100_003], &x[..100_003]);
    assert_eq!(&b[frames - 99_991..], &x[frames - 99_991..]);
    // Different grids give slightly different frames but the same smooth result: compare levels.
    let mid = 200_000..400_000;
    let d = db(rms(&a[mid.clone()]) / rms(&b[mid]));
    assert!(d.abs() < 0.5, "levels differ by {d:.2} dB");
}

#[test]
fn output_noise_only_is_what_reduction_removed() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("nr-noiseonly");
    let (x, tone_from, _) = tone_and_noise(0.02);
    let mut e = editor(&tmp.0, std::slice::from_ref(&x));
    e.run(ActionId::CaptureNoisePrint, 0..2 * RATE as u64).unwrap();
    e.run_effect(nr_index(), params("reduce_db=12 noise_only=On"), 0..0).unwrap();
    let y = &now(&e)[0];
    // Over the tone almost nothing is removed; over the noise almost all of it is.
    let tone_part = (tone_from + RATE as usize)..(tone_from + 4 * RATE as usize);
    assert!(db(tone_amplitude(&y[tone_part], 1000.0) / 0.25) < -30.0);
    let noise_part = RATE as usize / 4..4 * RATE as usize;
    let kept = db(rms(&y[noise_part.clone()]) / rms(&x[noise_part]));
    // 12 dB down leaves a quarter of the amplitude, so the removed part is three quarters: -2.5 dB.
    assert!((-3.5..-1.5).contains(&kept), "the removed noise is {kept:.2} dB of the input");
}

#[test]
fn without_a_print_or_with_a_too_short_selection_it_says_so() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("nr-errors");
    let x = gaussian(RATE as usize, 0.05, 9);
    let mut e = editor(&tmp.0, std::slice::from_ref(&x));
    noise::clear();
    let err = e.run_effect(nr_index(), params("reduce_db=12"), 0..0).unwrap_err().to_string();
    assert!(err.contains("Capture a noise print"), "{err}");
    assert_eq!(e.history.undo_label(), None);
    let err = e.run(ActionId::CaptureNoisePrint, 100..1000).unwrap_err().to_string();
    assert!(err.contains("too short"), "{err}");
    assert!(noise::current().is_none());
    assert!(!e.enabled(ActionId::CaptureNoisePrint, &(5..5)), "needs a selection");
    // A short but long-enough selection (0.5 s) works.
    let out = e.run(ActionId::CaptureNoisePrint, 0..RATE as u64 / 2).unwrap();
    assert!(out.message.is_some());
    let p = noise::current().unwrap();
    assert_eq!((p.density.len(), p.sample_rate), (1, RATE));
    assert!(p.frames >= 10);
}

#[test]
fn stereo_gets_a_print_per_channel() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new("nr-stereo");
    // Left is quiet noise, right is 10 times louder: each channel is judged against its own print.
    let frames = 4 * RATE as usize;
    let data = vec![gaussian(frames, 0.01, 11), gaussian(frames, 0.1, 12)];
    let mut e = editor(&tmp.0, &data);
    e.run(ActionId::CaptureNoisePrint, 0..2 * RATE as u64).unwrap();
    let p = noise::current().unwrap();
    assert_eq!(p.density.len(), 2);
    let ratio = p.density[1].iter().sum::<f32>() / p.density[0].iter().sum::<f32>();
    assert!((8.0..12.5).contains(&ratio), "print ratio {ratio}");
    e.run_effect(nr_index(), params("reduce_db=12"), 0..0).unwrap();
    let y = now(&e);
    for c in 0..2 {
        let region = (RATE as usize / 4)..(frames - RATE as usize / 4);
        let drop = db(rms(&data[c][region.clone()]) / rms(&y[c][region]));
        assert!(drop >= 12.0, "channel {c}: dropped {drop:.2} dB");
    }
}
