//! DeEsser (f28, B028) through the effect seam, including its state across the 64 K-frame chunks
//! of `replace_range`. (f28's Compressor and Hard Limiter are covered by B029's Dynamics, ruling 5.)

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
        let dir = std::env::temp_dir().join(format!("wavcraft-deesser-test-{}-{name}", std::process::id()));
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

#[test]
fn the_deesser_is_registered_with_settings() {
    let e = effects::find("deesser").expect("deesser is registered");
    let have: Vec<_> = e.parameters().iter().map(|p| p.id).collect();
    for k in ["frequency_hz", "threshold_db", "amount_db", "mode"] {
        assert!(have.contains(&k), "deesser lacks {k}");
    }
    let d = effects::Params::defaults(e);
    effects::validate(e, &d).unwrap();
    assert!(e.stream(&d, 2, RATE).is_some());
}

#[test]
fn state_carries_across_chunk_boundaries() {
    let t = TempDir::new("chunks");
    let frames = BLOCK_FRAMES * 5 / 2 + 123;
    // A loud burst every 20000 frames so the envelope is busy at the boundaries.
    let base = mix(&tone(500.0, 0.3, frames), &tone(7000.0, 0.2, frames));
    let x: Vec<f32> = base.iter().enumerate().map(|(n, v)| if (n % 20_000) < 9_000 { v * 4.0 } else { *v }).collect();
    let d = doc(&t.0, std::slice::from_ref(&x));
    for args in ["frequency_hz=5000 threshold_db=-30 amount_db=12 mode=broadband", "frequency_hz=5000 threshold_db=-30 amount_db=12 mode=multiband"] {
        let e = effects::find("deesser").unwrap();
        let p = effects::parse_params(e, args).unwrap();
        let chunked = run("deesser", args, &d);
        // The reference: one stream, the whole signal in one call.
        let mut s = e.stream(&p, 1, RATE).unwrap();
        let mut whole = vec![0.0f32; x.len()];
        s.process_block(&[&x[..]], &mut [&mut whole[..]]);
        let worst = chunked[0].iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-5, "{args}: chunked run differs from one pass by {worst}");
        for edge in (1..=2).map(|k| k * BLOCK_FRAMES) {
            let step = (chunked[0][edge] - chunked[0][edge - 1]).abs();
            assert!(step < 0.5, "jump of {step} at the chunk boundary {edge}");
        }
    }
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
fn processing_one_chunk_alone_is_refused() {
    let e = effects::find("deesser").unwrap();
    let p = effects::Params::defaults(e);
    let (i, mut o) = (vec![0.0f32; 4], vec![0.0f32; 4]);
    assert!(e.process(&p, 0, &[&i], &mut [&mut o]).is_err());
}
