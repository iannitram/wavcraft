//! Speech Volume Leveler (B038): even out loud and quiet talkers in one pass. A stateful effect
//! on the Run seam; the level detector and the gain carry across 64 K-frame chunks.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-leveler-test-{}-{name}", std::process::id()));
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

fn read(d: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn rms_db(x: &[f32]) -> f64 {
    let p = x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64;
    10.0 * p.log10()
}

fn scaled_to(mut x: Vec<f32>, db: f64) -> Vec<f32> {
    let k = 10f64.powf((db - rms_db(&x)) / 20.0);
    for v in &mut x {
        *v = (*v as f64 * k) as f32;
    }
    x
}

/// A 300 Hz + 1.2 kHz two-tone with a 4 Hz syllable envelope, `secs` long, at `db` RMS.
fn speechlike(secs: usize, db: f64) -> Vec<f32> {
    let x = (0..secs * RATE as usize)
        .map(|k| {
            let t = k as f64 / RATE as f64;
            let env = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * 4.0 * t).sin();
            let tones = (2.0 * std::f64::consts::PI * 300.0 * t).sin() + 0.5 * (2.0 * std::f64::consts::PI * 1200.0 * t).sin();
            (env * tones) as f32
        })
        .collect();
    scaled_to(x, db)
}

/// Deterministic white-ish noise at `db` RMS.
fn noise(secs: usize, db: f64) -> Vec<f32> {
    let mut s = 0x2545_F491u32;
    let x = (0..secs * RATE as usize)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f64 / u32::MAX as f64 - 0.5) as f32
        })
        .collect();
    scaled_to(x, db)
}

fn leveler() -> &'static dyn Effect {
    effects::find("speech_leveler").expect("the Speech Volume Leveler is registered")
}

/// target, amount, max gain, max cut, gate, attack, release (the order of the effect's settings).
fn params(target: f64, amount: f64, max_gain: f64, max_cut: f64, gate: f64) -> Vec<f64> {
    vec![target, amount, max_gain, max_cut, gate, 100.0, 300.0]
}

fn level(d: &DocState, p: &[f64], doc_range: std::ops::Range<u64>) -> DocState {
    let cancel = AtomicBool::new(false);
    effects::apply(leveler(), p, d, doc_range, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state
}

#[test]
fn quiet_and_loud_talkers_both_land_near_the_target() {
    let tmp = TempDir::new("levels");
    let mut x = speechlike(3, -30.0);
    x.extend(speechlike(3, -10.0));
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let out = read(&level(&d, &params(-18.0, 100.0, 30.0, 30.0, -45.0), 0..0), 0);
    let n = RATE as usize;
    let before = (rms_db(&x[n..3 * n]), rms_db(&x[4 * n..]));
    let (a, b) = (rms_db(&out[n..3 * n]), rms_db(&out[4 * n..]));
    assert!((before.0 + 30.0).abs() < 0.1 && (before.1 + 10.0).abs() < 0.1, "test signal levels {before:?}");
    assert!((a + 18.0).abs() <= 3.0, "quiet half ends at {a:.2} dB");
    assert!((b + 18.0).abs() <= 3.0, "loud half ends at {b:.2} dB");
}

#[test]
fn amount_zero_is_bit_identical() {
    let tmp = TempDir::new("zero");
    let mut x = speechlike(2, -30.0);
    x.extend(speechlike(2, -10.0));
    let d = doc(&tmp.0, &[x.clone(), x.iter().map(|v| v * 0.5).collect()]);
    let out = level(&d, &params(-18.0, 0.0, 12.0, 12.0, -45.0), 0..0);
    for (c, src) in [(0, x.clone()), (1, x.iter().map(|v| v * 0.5).collect::<Vec<f32>>())] {
        let got = read(&out, c);
        assert!(got.iter().zip(&src).all(|(a, b)| a.to_bits() == b.to_bits()), "channel {c} changed");
    }
}

#[test]
fn a_gap_of_noise_is_not_raised() {
    let tmp = TempDir::new("gate");
    let mut x = speechlike(3, -30.0);
    x.extend(noise(3, -60.0));
    x.extend(speechlike(3, -10.0));
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let out = read(&level(&d, &params(-18.0, 100.0, 12.0, 12.0, -45.0), 0..0), 0);
    let n = RATE as usize;
    let (from, to) = (5 * n, 6 * n); // the gap's last second
    let raised = rms_db(&out[from..to]) - rms_db(&x[from..to]);
    assert!(raised <= 1.0, "the gap was raised by {raised:.2} dB");
}

#[test]
fn gain_never_exceeds_the_ceiling_or_the_cut_floor() {
    let tmp = TempDir::new("limits");
    let mut x = speechlike(2, -50.0);
    x.extend(speechlike(2, 0.0));
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let out = read(&level(&d, &params(-18.0, 100.0, 6.0, 4.0, -80.0), 0..0), 0);
    let (up, down) = (10f64.powf(6.0 / 20.0), 10f64.powf(-4.0 / 20.0));
    let mut seen_up = 0.0f64;
    let mut seen_down = f64::MAX;
    for (a, b) in out.iter().zip(&x) {
        if b.abs() > 1e-4 {
            let g = (*a as f64 / *b as f64).abs();
            assert!(g <= up * 1.0001 && g >= down * 0.9999, "gain {g} outside {down}..{up}");
            seen_up = seen_up.max(g);
            seen_down = seen_down.min(g);
        }
    }
    assert!(seen_up > up * 0.9, "the quiet part should reach the ceiling, got {seen_up}");
    assert!(seen_down < down * 1.1, "the loud part should reach the floor, got {seen_down}");
}

#[test]
fn chunked_equals_unchunked() {
    let tmp = TempDir::new("chunks");
    let mut x = speechlike(3, -30.0);
    x.extend(speechlike(3, -10.0));
    let y: Vec<f32> = x.iter().rev().map(|v| v * 0.7).collect();
    assert!(x.len() > 4 * BLOCK_FRAMES);
    let d = doc(&tmp.0, &[x.clone(), y.clone()]);
    let p = params(-18.0, 80.0, 12.0, 12.0, -45.0);
    let chunked = level(&d, &p, 0..0);
    // The same run fed the whole file in one call.
    let ctx = effects::RunContext { doc: &d, range: 0..d.len_frames(), channels: vec![0, 1], sample_rate: RATE };
    let mut run = leveler().begin(&p, &ctx).unwrap().expect("the leveler keeps state in a run");
    let (mut o0, mut o1) = (vec![0.0f32; x.len()], vec![0.0f32; x.len()]);
    run.process(0, &[&x, &y], &mut [&mut o0, &mut o1]).unwrap();
    for (c, want) in [(0, o0), (1, o1)] {
        let got = read(&chunked, c);
        let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst <= 1e-5, "channel {c}: chunked differs by {worst}");
    }
}

#[test]
fn one_undo_restores_and_the_step_is_named() {
    let tmp = TempDir::new("undo");
    let x = speechlike(2, -30.0);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let mut e = Editor::new(History::new(d, u64::MAX));
    e.crossfades = Crossfades::NONE;
    let i = effects::all().iter().position(|f| f.id() == "speech_leveler").unwrap() as u16;
    let out = e.run_effect(i, params(-18.0, 100.0, 12.0, 12.0, -45.0), 0..0).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("Speech Volume Leveler"));
    assert_ne!(read(e.history.current(), 0), x);
    e.run(wavcraft_core::actions::ActionId::Undo, 0..0).unwrap();
    assert_eq!(read(e.history.current(), 0), x);
}

#[test]
fn a_selection_not_at_frame_zero_is_levelled_and_the_rest_untouched() {
    let tmp = TempDir::new("offset");
    let x = speechlike(4, -30.0);
    let d = doc(&tmp.0, std::slice::from_ref(&x));
    let n = RATE as u64;
    let out = read(&level(&d, &params(-18.0, 100.0, 30.0, 30.0, -45.0), n..4 * n), 0);
    assert!(out[..n as usize].iter().zip(&x).all(|(a, b)| a.to_bits() == b.to_bits()), "before the selection is untouched");
    let tail = rms_db(&out[3 * n as usize..]);
    assert!((tail + 18.0).abs() <= 3.0, "the selection ends at {tail:.2} dB");
}
