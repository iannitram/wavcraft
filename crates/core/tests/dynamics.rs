//! Dynamics (f28, B037 / B034 / B028): Compressor, Hard Limiter and DeEsser through the effect
//! seam, including their state across the 64 K-frame chunks of `replace_range`.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dynamics-test-{}-{name}", std::process::id()));
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

fn read_all(d: &DocState) -> Vec<Vec<f32>> {
    (0..d.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; d.len_frames() as usize];
            assert_eq!(d.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn run(name: &str, args: &str, d: &DocState) -> Vec<Vec<f32>> {
    let e = effects::find(name).unwrap();
    let p = effects::parse_params(e, args).unwrap();
    let out = effects::apply(e, &p, d, 0..0, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
    read_all(&out.state)
}

fn tone(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (TAU * freq * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// Amplitude of `freq` in `x` (a single-bin DFT), 0..1 scale.
fn level_at(x: &[f32], freq: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, v) in x.iter().enumerate() {
        let a = TAU * freq * n as f64 / f64::from(RATE);
        re += f64::from(*v) * a.cos();
        im += f64::from(*v) * a.sin();
    }
    2.0 * (re * re + im * im).sqrt() / x.len() as f64
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

#[test]
fn the_three_are_registered_with_settings() {
    for (id, keys) in [
        ("compressor", &["threshold_db", "ratio", "attack_ms", "release_ms", "output_gain_db", "auto_makeup"][..]),
        ("hard_limiter", &["max_amplitude_db", "input_boost_db", "lookahead_ms", "release_ms", "link_channels"][..]),
        ("deesser", &["frequency_hz", "threshold_db", "amount_db", "mode"][..]),
    ] {
        let e = effects::find(id).unwrap_or_else(|| panic!("{id} is registered"));
        let have: Vec<_> = e.parameters().iter().map(|p| p.id).collect();
        for k in keys {
            assert!(have.contains(k), "{id} lacks {k}");
        }
        let d = effects::Params::defaults(e);
        effects::validate(e, &d).unwrap();
        assert!(e.run_stream(&d, RATE, 2).is_some());
    }
}

#[test]
fn limiter_never_exceeds_its_ceiling_on_a_hot_signal() {
    let t = TempDir::new("limit");
    // 3.5 chunks of a +12 dB over full scale mix of two tones, hotter in the second channel, with
    // bursts placed on the chunk boundaries.
    let frames = BLOCK_FRAMES * 7 / 2;
    let mut l = mix(&tone(220.0, 1.2, frames), &tone(3100.0, 1.0, frames));
    let mut r = mix(&tone(330.0, 2.0, frames), &tone(5300.0, 1.5, frames));
    for edge in [BLOCK_FRAMES, 2 * BLOCK_FRAMES, 3 * BLOCK_FRAMES] {
        for k in 0..6 {
            l[edge - 3 + k] = 3.5;
            r[edge - 3 + k] = -4.0;
        }
    }
    let d = doc(&t.0, &[l, r]);
    let ceiling = 10f64.powf(-3.0 / 20.0) as f32; // -3 dBFS
    for link in ["link_channels=on", "link_channels=off"] {
        let out = run("hard_limiter", &format!("max_amplitude_db=-3 lookahead_ms=3 release_ms=50 {link}"), &d);
        for (c, ch) in out.iter().enumerate() {
            let p = peak(ch);
            assert!(p <= ceiling + 1e-6, "{link}: channel {c} peaks at {p}, ceiling {ceiling}");
            assert!(p > ceiling * 0.5, "{link}: channel {c} was limited to nothing (peak {p})");
            assert!(ch.iter().all(|v| v.is_finite()));
        }
        // The loudest channel is held right at the ceiling.
        assert!(out.iter().map(|c| peak(c)).fold(0.0, f32::max) > ceiling * 0.99, "{link}: nothing reached the ceiling");
    }
}

#[test]
fn limiter_lets_a_quiet_signal_through_unchanged_and_boost_raises_it() {
    let t = TempDir::new("quiet");
    let x = tone(440.0, 0.1, 20_000);
    let d = doc(&t.0, std::slice::from_ref(&x));
    let out = run("hard_limiter", "max_amplitude_db=-0.1", &d);
    let worst = out[0].iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 1e-6, "differs by {worst}");
    let out = run("hard_limiter", "max_amplitude_db=-0.1 input_boost_db=20", &d);
    let p = peak(&out[0]);
    assert!(p > 0.97 && p <= 0.9886, "boosted 0.1 by 20 dB (x10) should sit at the ceiling, peak {p}");
}

/// A run over several chunks equals the same stream fed the whole signal in one piece.
#[test]
fn state_carries_across_chunk_boundaries() {
    let t = TempDir::new("chunks");
    let frames = BLOCK_FRAMES * 5 / 2 + 123;
    // A loud burst every 20000 frames so the envelopes and look-ahead are busy at the boundaries.
    let base = mix(&tone(500.0, 0.3, frames), &tone(7000.0, 0.2, frames));
    let x: Vec<f32> = base.iter().enumerate().map(|(n, v)| if (n % 20_000) < 9_000 { v * 4.0 } else { *v }).collect();
    let d = doc(&t.0, std::slice::from_ref(&x));
    for (name, args) in [
        ("compressor", "threshold_db=-24 ratio=6 attack_ms=2 release_ms=200"),
        ("hard_limiter", "max_amplitude_db=-6 lookahead_ms=5 release_ms=120"),
        ("deesser", "frequency_hz=5000 threshold_db=-30 amount_db=12 mode=broadband"),
        ("deesser", "frequency_hz=5000 threshold_db=-30 amount_db=12 mode=multiband"),
    ] {
        let e = effects::find(name).unwrap();
        let p = effects::parse_params(e, args).unwrap();
        let chunked = run(name, args, &d);
        // The reference: one stream, the whole signal in one call, input padded for the latency.
        let mut s = e.run_stream(&p, RATE, 1).unwrap();
        let lat = s.latency();
        let mut whole = x.clone();
        whole.extend(std::iter::repeat_n(0.0, lat));
        s.process(&mut [&mut whole[..]]);
        let reference = &whole[lat..];
        let worst = chunked[0].iter().zip(reference).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-5, "{name} {args}: chunked run differs from one pass by {worst}");
        for edge in (1..=2).map(|k| k * BLOCK_FRAMES) {
            let step = (chunked[0][edge] - chunked[0][edge - 1]).abs();
            assert!(step < 0.5, "{name}: jump of {step} at the chunk boundary {edge}");
        }
    }
}

#[test]
fn compressor_reduces_a_loud_tone_by_the_expected_amount() {
    let t = TempDir::new("comp");
    // 1 kHz at -6 dBFS peak, threshold -20, 4:1, hard knee: 14 dB over, 10.5 dB of reduction.
    let x = tone(1000.0, 0.5, RATE as usize * 2);
    let d = doc(&t.0, std::slice::from_ref(&x));
    let out = run("compressor", "threshold_db=-20 ratio=4 attack_ms=1 release_ms=50 knee_db=0", &d);
    let tail = RATE as usize; // steady state: the last second
    let reduction = db(f64::from(peak(&x[tail..]))) - db(f64::from(peak(&out[0][tail..])));
    assert!((reduction - 10.5).abs() < 1.5, "reduced by {reduction:.2} dB, expected 10.5 within 1.5");
    // Below the threshold nothing changes.
    let quiet = tone(1000.0, 0.02, RATE as usize);
    let d = doc(&t.0.join("q"), std::slice::from_ref(&quiet));
    let out = run("compressor", "threshold_db=-20 ratio=4 knee_db=0", &d);
    let worst = out[0].iter().zip(&quiet).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 1e-4, "a signal under the threshold changed by {worst}");
    // Output gain and auto make-up add on top.
    let louder = run(
        "compressor",
        "threshold_db=-20 ratio=4 attack_ms=1 release_ms=50 knee_db=0 output_gain_db=6",
        &doc(&t.0.join("g"), std::slice::from_ref(&x)),
    );
    let more = db(f64::from(peak(&louder[0][tail..]))) - db(f64::from(peak(&out_of(&x, &t.0.join("h"))[tail..])));
    assert!((more - 6.0).abs() < 0.1, "output gain added {more:.2} dB");
    let auto = run("compressor", "threshold_db=-20 ratio=4 attack_ms=1 release_ms=50 knee_db=0 auto_makeup=on", &doc(&t.0.join("a"), &[x]));
    let makeup = db(f64::from(peak(&auto[0][tail..]))) - db(f64::from(peak(&out_of(&tone(1000.0, 0.5, RATE as usize * 2), &t.0.join("i"))[tail..])));
    assert!((makeup - 15.0).abs() < 0.1, "auto make-up added {makeup:.2} dB, expected 15");
}

fn out_of(x: &[f32], dir: &Path) -> Vec<f32> {
    run("compressor", "threshold_db=-20 ratio=4 attack_ms=1 release_ms=50 knee_db=0", &doc(dir, &[x.to_vec()])).remove(0)
}

#[test]
fn deesser_reduces_energy_in_its_band() {
    let t = TempDir::new("deess");
    let n = RATE as usize * 2;
    let x = mix(&tone(200.0, 0.3, n), &tone(8000.0, 0.3, n));
    let tail = RATE as usize / 2..n;
    let (low0, high0) = (level_at(&x[tail.clone()], 200.0), level_at(&x[tail.clone()], 8000.0));
    for (i, mode) in ["multiband", "broadband"].iter().enumerate() {
        let d = doc(&t.0.join(format!("m{i}")), std::slice::from_ref(&x));
        let out = run("deesser", &format!("frequency_hz=6000 threshold_db=-30 amount_db=12 mode={mode}"), &d);
        let (low, high) = (level_at(&out[0][tail.clone()], 200.0), level_at(&out[0][tail.clone()], 8000.0));
        let cut = db(high0) - db(high);
        assert!(cut > 6.0, "{mode}: the 8 kHz band fell only {cut:.1} dB");
        if *mode == "multiband" {
            assert!((db(low0) - db(low)).abs() < 1.0, "multiband: the 200 Hz band moved {:.1} dB", db(low0) - db(low));
        } else {
            assert!(db(low0) - db(low) > 6.0, "broadband turns everything down");
        }
    }
    // Under the threshold nothing happens.
    let soft = mix(&tone(200.0, 0.01, n), &tone(8000.0, 0.005, n));
    let out = run("deesser", "frequency_hz=6000 threshold_db=-30 amount_db=12 mode=multiband", &doc(&t.0.join("s"), std::slice::from_ref(&soft)));
    assert!((db(level_at(&soft[tail.clone()], 8000.0)) - db(level_at(&out[0][tail], 8000.0))).abs() < 0.5);
}

#[test]
fn a_selection_is_processed_with_the_audio_after_it_as_look_ahead() {
    let t = TempDir::new("sel");
    let n = RATE as usize;
    let mut x = tone(300.0, 0.2, n);
    for v in &mut x[30_000..30_050] {
        *v = 2.0; // a spike just after the selection's end
    }
    let d = doc(&t.0, std::slice::from_ref(&x));
    let e = effects::find("hard_limiter").unwrap();
    let p = effects::parse_params(e, "max_amplitude_db=-6 lookahead_ms=5").unwrap();
    let out = effects::apply(e, &p, &d, 10_000..30_000, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
    let y = read_all(&out.state);
    assert_eq!(y[0].len(), n);
    assert_eq!(&y[0][..10_000], &x[..10_000]);
    assert_eq!(&y[0][30_000..], &x[30_000..]);
    // The gain was already down for the last frames inside the selection, ahead of the spike.
    let ratio = y[0][29_990..30_000].iter().map(|v| v.abs()).sum::<f32>() / x[29_990..30_000].iter().map(|v| v.abs()).sum::<f32>();
    assert!(ratio < 0.9, "no look-ahead reduction before the end of the selection (ratio {ratio})");
}

#[test]
fn processing_one_chunk_alone_is_refused() {
    let e = effects::find("compressor").unwrap();
    let p = effects::Params::defaults(e);
    let (i, mut o) = (vec![0.0f32; 4], vec![0.0f32; 4]);
    assert!(e.process(&p, 0, &[&i], &mut [&mut o]).is_err());
}
