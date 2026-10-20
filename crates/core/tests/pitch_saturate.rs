//! f125: the Pitch Shifter and the Saturator through the effect seam.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use soundcraft_dsp::Plugin;
use soundcraft_dsp::plugins::pitch::PitchShifter;
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, parse_params};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wavcraft-f125-{}-{name}-{k}", std::process::id()));
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

fn sine(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|i| (amp * (TAU * freq * i as f64 / RATE as f64).sin()) as f32).collect()
}

fn run(name: &str, args: &str, input: &[f32]) -> Vec<f32> {
    let tmp = TempDir::new(name);
    let d = doc(&tmp.0, &[input.to_vec()]);
    let e = effects::find(name).unwrap();
    let p = parse_params(e, args).unwrap();
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(e, &p, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let mut out = vec![0.0f32; input.len()];
    assert_eq!(edit.state.read(0, 0, &mut out).unwrap(), input.len());
    assert_eq!(edit.state.len_frames(), input.len() as u64, "length is kept");
    out
}

/// Strongest frequency in `lo..=hi` Hz over `x[from..to]`, 1 Hz steps (Goertzel).
fn peak_hz(x: &[f32], from: usize, to: usize, lo: u32, hi: u32) -> u32 {
    let seg = &x[from..to];
    let mut best = (0.0f64, 0u32);
    for f in lo..=hi {
        let w = TAU * f as f64 / RATE as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in seg.iter().enumerate() {
            re += *s as f64 * (w * i as f64).cos();
            im += *s as f64 * (w * i as f64).sin();
        }
        let m = re * re + im * im;
        if m > best.0 {
            best = (m, f);
        }
    }
    best.1
}

#[test]
fn a_440_tone_up_an_octave_measures_880_hz_with_the_same_length() {
    let input = sine(440.0, 0.5, 3 * RATE as usize);
    let out = run("pitch_shift", "semitones=12", &input);
    let f = peak_hz(&out, RATE as usize / 2, 5 * RATE as usize / 2, 800, 960);
    assert!((878..=882).contains(&f), "measured {f} Hz");
    let down = run("pitch_shift", "semitones=-12", &input);
    let f = peak_hz(&down, RATE as usize / 2, 5 * RATE as usize / 2, 150, 300);
    assert!((218..=222).contains(&f), "measured {f} Hz");
}

#[test]
fn cents_add_to_the_semitones() {
    let input = sine(440.0, 0.5, 2 * RATE as usize);
    let out = run("pitch_shift", "semitones=11 cents=100", &input);
    let f = peak_hz(&out, RATE as usize / 2, 3 * RATE as usize / 2, 800, 960);
    assert!((878..=882).contains(&f), "measured {f} Hz");
}

#[test]
fn the_chunks_join_as_one_stream() {
    // Longer than two 64 K chunks: the result must equal the plugin run once over the whole signal
    // (its delay taken out), which only holds if state and lookahead cross the chunk boundaries.
    let n = BLOCK_FRAMES * 2 + 3000;
    let input = sine(440.0, 0.5, n);
    let out = run("pitch_shift", "semitones=7", &input);
    let mut plugin = PitchShifter::new();
    plugin.prepare(RATE as f32, 1 << 20, 1);
    plugin.set_param("semitones", 7.0);
    plugin.reset();
    let l = plugin.latency();
    let mut io = vec![input.iter().copied().chain(std::iter::repeat_n(0.0, l)).collect::<Vec<f32>>()];
    let total = io[0].len();
    plugin.process(&mut io, total);
    let worst = out.iter().zip(&io[0][l..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "differs from one pass by {worst}");
}

#[test]
fn the_saturator_stays_under_its_ceiling_and_adds_harmonics() {
    let input = sine(1000.0, 0.9, RATE as usize);
    for curve in ["soft", "hard"] {
        let out = run("saturator", &format!("drive=12 curve={curve} output=-6"), &input);
        let ceiling = 10f32.powf(-6.0 / 20.0);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= ceiling + 1e-6, "{curve}: peak {peak} over the ceiling {ceiling}");
        assert!(peak > ceiling * 0.9, "{curve}: peak {peak} should be near the ceiling");
        // The input has nothing at 3 kHz; the saturated sine does.
        let mag = |x: &[f32]| {
            let w = TAU * 3000.0 / RATE as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in x.iter().take(RATE as usize / 2).enumerate() {
                re += *s as f64 * (w * i as f64).cos();
                im += *s as f64 * (w * i as f64).sin();
            }
            (re * re + im * im).sqrt() / (RATE as f64 / 4.0)
        };
        assert!(mag(&input) < 0.01, "the input has no 3rd harmonic");
        assert!(mag(&out) > 0.02, "{curve}: 3rd harmonic {}", mag(&out));
    }
}

#[test]
fn the_saturator_mix_zero_is_the_dry_signal_scaled_by_the_output() {
    let input = sine(1000.0, 0.5, 20_000);
    let out = run("saturator", "drive=24 mix=0 output=0", &input);
    let worst = out.iter().zip(&input).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "mix 0 differs from dry by {worst}");
}

#[test]
fn both_are_registered_with_settings() {
    for id in ["pitch_shift", "saturator"] {
        let e = effects::find(id).unwrap();
        assert!(!e.parameters().is_empty());
        assert!(effects::describe(e).contains('='));
    }
}
