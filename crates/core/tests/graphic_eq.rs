//! Graphic EQ (B055): fixed-band sliders on the chunk-state Run seam.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect, RunContext};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-geq-test-{}-{name}", std::process::id()));
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

fn read(d: &DocState, c: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; d.len_frames() as usize];
    assert_eq!(d.read(c, 0, &mut v).unwrap(), v.len());
    v
}

fn sine(hz: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|k| (0.25 * (2.0 * std::f64::consts::PI * hz * k as f64 / RATE as f64).sin()) as f32).collect()
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

fn geq() -> &'static dyn Effect {
    effects::find("graphic_eq").expect("graphic_eq is registered")
}

/// Settings: `bands` index, then `(band number 1-based, dB)` pairs, master 0.
fn settings(bands: usize, gains: &[(usize, f64)], master: f64) -> Vec<f64> {
    let mut p = effects::Params::defaults(geq());
    p[0] = bands as f64;
    for (n, db) in gains {
        p[*n] = *db;
    }
    *p.last_mut().unwrap() = master;
    p
}

fn run(d: &DocState, params: &[f64]) -> DocState {
    let cancel = AtomicBool::new(false);
    effects::apply(geq(), params, d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state
}

/// Gain in dB of a sine through the effect, measured away from the start-up transient.
fn gain_db(params: &[f64], hz: f64, name: &str) -> f64 {
    let tmp = TempDir::new(name);
    let frames = RATE as usize;
    let input = sine(hz, frames);
    let out = read(&run(&doc(&tmp.0, std::slice::from_ref(&input)), params), 0);
    let (a, b) = (RATE as usize / 4, frames);
    20.0 * (rms(&out[a..b]) / rms(&input[a..b])).log10()
}

#[test]
fn all_zero_is_bit_identical() {
    let tmp = TempDir::new("zero");
    let data = vec![sine(1000.0, BLOCK_FRAMES + 123), sine(300.0, BLOCK_FRAMES + 123)];
    let d = doc(&tmp.0, &data);
    for bands in 0..3 {
        let out = run(&d, &settings(bands, &[], 0.0));
        for (c, want) in data.iter().enumerate() {
            assert_eq!(&read(&out, c), want, "bands choice {bands}, channel {c}");
        }
    }
}

#[test]
fn ten_band_one_khz_boost_is_local() {
    // Band 6 of the 10-band layout is 1 kHz.
    let p = settings(0, &[(6, 12.0)], 0.0);
    let at_1k = gain_db(&p, 1000.0, "g1k");
    assert!((at_1k - 12.0).abs() <= 1.0, "1 kHz moved {at_1k} dB");
    for hz in [125.0, 8000.0] {
        let g = gain_db(&p, hz, &format!("g{hz}"));
        assert!(g.abs() < 1.0, "{hz} Hz moved {g} dB");
    }
}

#[test]
fn thirty_band_all_up_six_has_bounded_ripple() {
    let gains: Vec<(usize, f64)> = (1..=30).map(|n| (n, 6.0)).collect();
    let p = settings(2, &gains, 0.0);
    for hz in [100.0, 180.0, 440.0, 1000.0, 1900.0, 3500.0, 7000.0, 10_000.0] {
        let g = gain_db(&p, hz, &format!("r{hz}"));
        assert!((g - 6.0).abs() <= 1.5, "{hz} Hz moved {g} dB");
    }
}

#[test]
fn bands_beyond_the_chosen_count_are_ignored() {
    // 10 bands chosen: sliders 11..30 do nothing.
    let p = settings(0, &[(20, 24.0), (30, 24.0)], 0.0);
    let tmp = TempDir::new("ignored");
    let data = vec![sine(1000.0, 5000)];
    assert_eq!(read(&run(&doc(&tmp.0, &data), &p), 0), data[0]);
}

#[test]
fn master_gain_applies() {
    let g = gain_db(&settings(0, &[], -6.0), 1000.0, "master");
    assert!((g + 6.0).abs() < 0.05, "master moved {g} dB");
}

#[test]
fn chunked_equals_one_pass() {
    let tmp = TempDir::new("chunk");
    let frames = BLOCK_FRAMES * 2 + 777;
    let (s1, s2) = (sine(700.0, frames), sine(90.0, frames));
    let input: Vec<f32> = s1.iter().zip(&s2).map(|(a, b)| a + b).collect();
    let d = doc(&tmp.0, std::slice::from_ref(&input));
    let p = settings(1, &[(5, 9.0), (8, -7.0), (14, 12.0), (3, 5.0)], 1.5);
    let chunked = read(&run(&d, &p), 0);

    let ctx = RunContext { doc: &d, range: 0..frames as u64, channels: vec![0], sample_rate: RATE };
    let mut r = geq().begin(&p, &ctx).unwrap().expect("graphic_eq has a run");
    let mut whole = vec![0.0f32; frames];
    r.process(0, &[&input], &mut [&mut whole]).unwrap();
    let worst = chunked.iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst <= 1e-6, "worst difference {worst}");
    assert!(chunked.iter().zip(&input).any(|(a, b)| (a - b).abs() > 1e-3), "the filter did something");
}

#[test]
fn mask_is_respected() {
    let tmp = TempDir::new("mask");
    let data = vec![sine(1000.0, 20_000), sine(1000.0, 20_000)];
    let d = doc(&tmp.0, &data);
    let cancel = AtomicBool::new(false);
    let p = settings(0, &[(6, 12.0)], 0.0);
    let out = effects::apply(geq(), &p, &d, 0..0, ChannelMask::only(0), Crossfades::NONE, &cancel).unwrap().state;
    assert_ne!(read(&out, 0), data[0]);
    assert_eq!(read(&out, 1), data[1]);
}

#[test]
fn one_undo_restores_and_the_step_is_named() {
    let tmp = TempDir::new("undo");
    let data = vec![sine(1000.0, 30_000)];
    let s = doc(&tmp.0, &data);
    let mut e = Editor::new(History::new(s, u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = effects::all().iter().position(|x| x.id() == "graphic_eq").unwrap() as u16;
    e.run_effect(i, settings(0, &[(6, 12.0)], 0.0), 0..0).unwrap();
    assert_eq!(e.history.undo_label(), Some("Graphic EQ"));
    assert_ne!(read(e.history.current(), 0), data[0]);
    e.run(wavcraft_core::actions::ActionId::Undo, 0..0).unwrap();
    assert_eq!(read(e.history.current(), 0), data[0]);
}

#[test]
fn thirty_db_is_refused() {
    let p = settings(0, &[(1, 30.0)], 0.0);
    assert!(effects::validate(geq(), &p).is_err());
    let tmp = TempDir::new("refuse");
    let d = doc(&tmp.0, &[sine(1000.0, 1000)]);
    let cancel = AtomicBool::new(false);
    assert!(effects::apply(geq(), &p, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).is_err());
}

#[test]
fn a_selection_not_at_the_start_filters_from_its_own_first_frame() {
    let tmp = TempDir::new("offset");
    let frames = BLOCK_FRAMES + 20_000;
    let input = sine(900.0, frames);
    let d = doc(&tmp.0, std::slice::from_ref(&input));
    let p = settings(0, &[(6, 12.0), (7, -9.0)], 0.0);
    let range = 5_000u64..(frames as u64 - 3_000);
    let cancel = AtomicBool::new(false);
    let out = read(&effects::apply(geq(), &p, &d, range.clone(), ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state, 0);

    let (s, e) = (range.start as usize, range.end as usize);
    let ctx = RunContext { doc: &d, range: range.clone(), channels: vec![0], sample_rate: RATE };
    let mut r = geq().begin(&p, &ctx).unwrap().unwrap();
    let mut want = vec![0.0f32; e - s];
    r.process(range.start, &[&input[s..e]], &mut [&mut want]).unwrap();
    assert_eq!(&out[..s], &input[..s], "before the selection is untouched");
    assert_eq!(&out[e..], &input[e..], "after the selection is untouched");
    let worst = out[s..e].iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst <= 1e-6, "worst difference {worst}");
}
