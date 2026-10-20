//! FFT Filter (B054): a drawn frequency response applied on the STFT engine.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, RunContext};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-fftfilter-test-{}-{name}", std::process::id()));
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

fn channel(e: &Editor, c: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
    v
}

fn index() -> u16 {
    effects::all().iter().position(|e| e.id() == "fft_filter").expect("registered") as u16
}

/// Settings: size index, scale index, then (Hz, dB) pairs; unused points stay at 0 Hz.
fn settings(size: f64, log: bool, pts: &[(f64, f64)]) -> Vec<f64> {
    let mut p = vec![size, if log { 0.0 } else { 1.0 }];
    for i in 0..8 {
        let (f, g) = pts.get(i).copied().unwrap_or((0.0, 0.0));
        p.extend([f, g]);
    }
    p
}

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((s >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.8
        })
        .collect()
}

fn sine(n: usize, hz: f64) -> Vec<f32> {
    (0..n).map(|k| (0.5 * (std::f64::consts::TAU * hz * k as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

#[test]
fn a_flat_curve_reproduces_the_input() {
    let tmp = TempDir::new("flat");
    let data = noise(BLOCK_FRAMES * 2 + 777, 1);
    let mut e = editor(&tmp.0, std::slice::from_ref(&data));
    e.run_effect(index(), settings(2.0, true, &[]), 0..0).unwrap();
    let out = channel(&e, 0);
    let diff: Vec<f32> = out.iter().zip(&data).map(|(a, b)| a - b).collect();
    assert!(rms(&diff) < 1e-5, "rms error {}", rms(&diff));
}

#[test]
fn a_low_pass_curve_passes_500_hz_and_removes_6_khz() {
    let curve = [(1000.0, 0.0), (2000.0, -60.0)];
    let n = 100_000;
    for (hz, lo, hi) in [(500.0, -0.2, 0.2), (6000.0, -200.0, -50.0)] {
        let tmp = TempDir::new(&format!("lp{hz}"));
        let data = sine(n, hz);
        let mut e = editor(&tmp.0, std::slice::from_ref(&data));
        e.run_effect(index(), settings(2.0, true, &curve), 0..0).unwrap();
        let out = channel(&e, 0);
        let db = 20.0 * (rms(&out[10_000..n - 10_000]) / rms(&data[10_000..n - 10_000])).log10();
        assert!((lo..=hi).contains(&db), "{hz} Hz changed by {db} dB");
    }
}

#[test]
fn chunked_output_equals_a_single_pass() {
    let tmp = TempDir::new("join");
    let n = BLOCK_FRAMES * 2 + 5000;
    let data = noise(n, 7);
    let mut e = editor(&tmp.0, std::slice::from_ref(&data));
    let before = e.history.current();
    let params = settings(1.0, true, &[(300.0, 6.0), (3000.0, -20.0), (9000.0, 0.0)]);
    let range = 1234..(n as u64 - 900);
    // One pass over the whole range, by hand.
    let fx = effects::all()[index() as usize];
    let ctx = RunContext { doc: &before, range: range.clone(), channels: vec![0], sample_rate: RATE };
    let mut run = fx.begin(&params, &ctx).unwrap().expect("a run");
    let input = &data[range.start as usize..range.end as usize];
    let mut want = vec![0.0f32; input.len()];
    run.process(range.start, &[input], &mut [&mut want[..]]).unwrap();
    // The same range through the streamed 64 K-frame chunks.
    e.run_effect(index(), params, range.clone()).unwrap();
    let got = channel(&e, 0);
    for (k, w) in want.iter().enumerate() {
        let g = got[range.start as usize + k];
        assert!((g - w).abs() <= 1e-6, "frame {k}: {g} vs {w}");
    }
    assert!(got[..range.start as usize] == data[..range.start as usize], "before the selection is untouched");
    assert!(got[range.end as usize..] == data[range.end as usize..], "after the selection is untouched");
    assert_eq!(e.history.undo_label(), Some("FFT Filter"));
    e.history.undo().unwrap();
    assert_eq!(channel(&e, 0), data, "one undo restores");
}
