//! Parametric EQ and the DeEsser (f2, F2.1) against SoundCraft's `soundcraft-dsp` used directly:
//! the same input and settings through `effects::apply`, and through the shared biquad or plugin,
//! must match within 1e-6. High-Pass is compared in filters.rs. Gate, compressor and limiter are
//! the project's own code (Q11), checked by dynamics.rs, so they have no SoundCraft vectors here.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use soundcraft_dsp::biquad::{Biquad, FilterType};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const TOL: f32 = 1.0e-6;
/// The Q of Parametric EQ's shelves, as in eq.rs.
const SHELF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

const EQ_ALL_BANDS: &str = "low_freq=100 low_gain_db=6 mid1_freq=1000 mid1_gain_db=6 mid1_q=1 \
     mid2_freq=2500 mid2_gain_db=-4 mid2_q=2 high_freq=8000 high_gain_db=3";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-f2-soundcraft-test-{}-{name}", std::process::id()));
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

/// Three tones (200, 1000 and 6500 Hz), so the EQ bands and the de-esser band all have energy.
fn mixed_tone(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| {
            let t = n as f64 / f64::from(RATE);
            (0.3 * (TAU * 200.0 * t).sin() + 0.3 * (TAU * 1000.0 * t).sin() + 0.2 * (TAU * 6500.0 * t).sin()) as f32
        })
        .collect()
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
}

/// The shared biquad in the same band order, run over the whole signal in one piece.
fn biquad_reference(x: &[f32], bands: &[(FilterType, f32, f32, f32)]) -> Vec<f32> {
    let mut out = x.to_vec();
    for &(kind, freq, q, gain_db) in bands {
        Biquad::design(kind, freq, q, gain_db, RATE as f32).process_block(&mut out);
    }
    out
}

/// The shared de-esser plugin in Multiband mode at freq 6000, threshold -30, range 10.
fn plugin_reference(channels: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let mut p = soundcraft_dsp::create("de_esser").expect("registered");
    p.prepare(RATE as f32, 65_536, channels.len());
    p.set_param("freq", 6000.0);
    p.set_param("threshold", -30.0);
    p.set_param("range", 10.0);
    let mut bufs = channels.to_vec();
    let n = bufs[0].len();
    p.process(&mut bufs, n);
    bufs
}

#[test]
fn parametric_eq_matches_the_shared_biquad_with_every_band_on() {
    let tmp = TempDir::new("eq-all");
    let x = mixed_tone(3 * BLOCK_FRAMES + 777);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let got = run("parametric_eq", EQ_ALL_BANDS, &d);
    let want = biquad_reference(
        &x,
        &[
            (FilterType::LowShelf, 100.0, SHELF_Q, 6.0),
            (FilterType::Peak, 1000.0, 1.0, 6.0),
            (FilterType::Peak, 2500.0, 2.0, -4.0),
            (FilterType::HighShelf, 8000.0, SHELF_Q, 3.0),
        ],
    );
    let worst = max_diff(&got[0], &want);
    assert!(worst < TOL, "Parametric EQ differs from the shared biquad by {worst}");
    let moved = max_diff(&want, &x);
    assert!(moved > 0.05, "the reference barely changed the input ({moved}), so the test proves nothing");
}

#[test]
fn parametric_eq_leaves_out_a_band_at_zero_db() {
    let tmp = TempDir::new("eq-one");
    let x = mixed_tone(3 * BLOCK_FRAMES + 777);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let got = run("parametric_eq", "mid1_freq=1000 mid1_gain_db=6 mid1_q=1", &d);
    let want = biquad_reference(&x, &[(FilterType::Peak, 1000.0, 1.0, 6.0)]);
    let worst = max_diff(&got[0], &want);
    assert!(worst < TOL, "Parametric EQ with one band differs from the shared biquad by {worst}");
}

#[test]
fn deesser_multiband_matches_the_shared_plugin() {
    let tmp = TempDir::new("deess-mono");
    let x = mixed_tone(40_000);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let got = run("deesser", "frequency_hz=6000 threshold_db=-30 amount_db=10", &d);
    let want = plugin_reference(std::slice::from_ref(&x));
    let worst = max_diff(&got[0], &want[0]);
    assert!(worst < TOL, "DeEsser differs from the shared plugin by {worst}");
    let acted = max_diff(&want[0], &x);
    assert!(acted > 0.01, "the shared plugin barely changed the input ({acted}), so the test proves nothing");
}

#[test]
fn deesser_multiband_matches_the_shared_plugin_on_two_channels() {
    let tmp = TempDir::new("deess-stereo");
    let left = mixed_tone(40_000);
    let right: Vec<f32> = left.iter().map(|s| s * 0.5).collect();
    let d = doc(&tmp.0, &[left.clone(), right.clone()]);
    let got = run("deesser", "frequency_hz=6000 threshold_db=-30 amount_db=10", &d);
    let want = plugin_reference(&[left, right]);
    for (c, (g, w)) in got.iter().zip(&want).enumerate() {
        let worst = max_diff(g, w);
        assert!(worst < TOL, "DeEsser channel {c} differs from the shared plugin by {worst}");
    }
}
