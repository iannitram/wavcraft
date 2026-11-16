//! Item A4 (issue #12): dither when reducing bit depth, and remembering the choice.
//! Locked tests for cards wav-c-12-a (`a4a_`), wav-c-12-b (`a4b_`) and wav-c-12-c (`a4c_`).
//! Every expected value is hard-coded; it was computed outside the code under test.
//! The dither is the document's `Ditherer` (crates/core/src/dither.rs), the one the app uses.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::dither::{DitherSettings, DitherType, Ditherer, Shaping};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::settings::Settings;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

/// Triangular dither, no noise shaping, the seed the settings use by default.
fn on() -> DitherSettings {
    DitherSettings { dither: DitherType::Triangular, shaping: Shaping::None, seed: wavcraft_core::dither::DEFAULT_SEED }
}

/// No dither and no shaping: plain rounding.
fn off() -> DitherSettings {
    DitherSettings::default()
}

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-a4-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>], dither: DitherSettings) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let mut src = SourceFormat::synthetic(RATE, data.len() as u16);
    src.dither = dither;
    DocState::new(s.clone(), chans, RATE, src).unwrap()
}

/// Save `data` (a fresh document with `dither` as its source's dither) to `name` in `dir`;
/// returns the file's bytes.
fn save_bytes(dir: &Path, name: &str, data: &[Vec<f32>], ty: SampleType, dither: DitherSettings) -> Vec<u8> {
    let doc = doc_from(&dir.join(format!("{name}-store")), data, dither);
    let target = dir.join(name);
    save_wav(&doc, &target, ty, &mut |_| {}).unwrap();
    std::fs::read(&target).unwrap()
}

/// The saved file `name` in `dir`, reopened, as integer sample values at `bits`.
fn reopen_ints(dir: &Path, name: &str, bits: u32) -> Vec<Vec<i32>> {
    let opts = OpenOptions { sessions_root: Some(dir.join(format!("{name}-sessions"))), ..OpenOptions::default() };
    let doc = open(&dir.join(name), &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let scale = (1u64 << (bits - 1)) as f64;
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v.iter().map(|x| (f64::from(*x) * scale).round() as i32).collect()
        })
        .collect()
}

/// Values between the 16-bit steps, longer than two blocks so a save crosses block edges.
fn off_grid(frames: usize) -> Vec<f32> {
    (0..frames).map(|k| ((k * 7_919 % 20_001) as f32 - 10_000.0) / 40_000.0).collect()
}

/// A −90 dBFS 1 kHz sine at 48 kHz, one second (exactly 1000 cycles).
fn sine_minus_90() -> Vec<f32> {
    let amp = 10f64.powf(-90.0 / 20.0);
    (0..48_000).map(|i| (amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 48_000.0).sin()) as f32).collect()
}

/// Amplitude in LSBs of harmonic `n` of 1 kHz in one second of 48 kHz samples (one DFT bin).
fn harmonic(q: &[i32], n: usize) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, v) in q.iter().enumerate() {
        let ph = 2.0 * std::f64::consts::PI * (n * 1000) as f64 * i as f64 / 48_000.0;
        re += f64::from(*v) * ph.cos();
        im += f64::from(*v) * ph.sin();
    }
    2.0 / q.len() as f64 * re.hypot(im)
}

// ---- wav-c-12-a: the Ditherer quantises one channel ----

#[test]
fn a4a_a_third_of_an_lsb_comes_through_on_average() {
    // Plain rounding turns 0.3 LSB into 0 every time; with dither the average is 0.3.
    let x = 0.3f32 / 32_768.0;
    let mut d = Ditherer::new(&on(), 0);
    let q: Vec<i32> = (0..100_000).map(|_| d.quantize(x, 16)).collect();
    assert!(q.iter().all(|v| (-1..=1).contains(v)));
    let mean = q.iter().map(|v| f64::from(*v)).sum::<f64>() / q.len() as f64;
    assert!((mean - 0.3).abs() < 0.01, "mean {mean}");
}

#[test]
fn a4a_full_scale_clamps() {
    let mut d = Ditherer::new(&on(), 0);
    for _ in 0..1000 {
        assert_eq!(d.quantize(1.0, 16), 32_767);
    }
    for _ in 0..1000 {
        let v = d.quantize(-1.0, 16);
        assert!(v == -32_768 || v == -32_767, "got {v}");
    }
}

#[test]
fn a4a_twenty_four_bits_stay_within_one_step() {
    let (mut a, mut b) = (Ditherer::new(&on(), 0), Ditherer::new(&on(), 0));
    let qa: Vec<i32> = (0..1000).map(|_| a.quantize(0.25, 24)).collect();
    let qb: Vec<i32> = (0..1000).map(|_| b.quantize(0.25, 24)).collect();
    assert_eq!(qa, qb, "the same settings and channel give the same values");
    assert!(qa.iter().all(|v| (2_097_151..=2_097_153).contains(v)));
    assert!(qa.contains(&2_097_151) && qa.contains(&2_097_153), "dither moves some values");
}

#[test]
fn a4a_nan_is_zero() {
    assert_eq!(Ditherer::new(&on(), 0).quantize(f32::NAN, 16), 0);
    assert_eq!(Ditherer::new(&off(), 0).quantize(f32::NAN, 16), 0);
}

// ---- wav-c-12-b: the dithered WAV save ----

#[test]
fn a4b_dither_off_is_plain_rounding() {
    let dir = TempDir::new("b-off");
    let frames = 2 * BLOCK_FRAMES + 123;
    let first = off_grid(frames);
    let second: Vec<f32> = first.iter().map(|x| -x).collect();
    let data = vec![first.clone(), second.clone()];
    save_bytes(&dir.0, "off.wav", &data, SampleType::Int16, off());
    let got = reopen_ints(&dir.0, "off.wav", 16);
    for (ch, samples) in [first, second].iter().enumerate() {
        for (i, x) in samples.iter().enumerate() {
            let want = (f64::from(*x) * 32768.0).round().clamp(-32768.0, 32767.0) as i32;
            assert_eq!(got[ch][i], want, "channel {ch} frame {i}");
        }
    }
}

#[test]
fn a4b_minus_90_dbfs_sine_gives_noise_not_steps() {
    let dir = TempDir::new("b-sine");
    let data = vec![sine_minus_90()];
    save_bytes(&dir.0, "off.wav", &data, SampleType::Int16, off());
    save_bytes(&dir.0, "on.wav", &data, SampleType::Int16, on());
    let off = &reopen_ints(&dir.0, "off.wav", 16)[0];
    let on = &reopen_ints(&dir.0, "on.wav", 16)[0];
    // Off: plain rounding, a stepped wave of -1, 0, 1 with strong odd harmonics
    // (computed: 5th 0.1709 LSB, 7th 0.188 LSB).
    assert!(off.iter().all(|v| (-1..=1).contains(v)));
    assert!(harmonic(off, 5) > 0.15, "off 5th {}", harmonic(off, 5));
    assert!(harmonic(off, 7) > 0.15, "off 7th {}", harmonic(off, 7));
    // On: the 1.036 LSB fundamental survives and the harmonics sink into the noise.
    let fundamental = harmonic(on, 1);
    assert!((fundamental - 1.036).abs() < 0.05, "on fundamental {fundamental}");
    for n in [3, 5, 7] {
        assert!(harmonic(on, n) < 0.03, "on harmonic {n}: {}", harmonic(on, n));
    }
}

#[test]
fn a4b_channel_k_uses_its_own_ditherer_for_the_whole_save() {
    let dir = TempDir::new("b-chan");
    let frames = 2 * BLOCK_FRAMES + 123;
    let data = vec![off_grid(frames), off_grid(frames)];
    save_bytes(&dir.0, "on.wav", &data, SampleType::Int16, on());
    let got = reopen_ints(&dir.0, "on.wav", 16);
    for (ch, samples) in data.iter().enumerate() {
        let mut d = Ditherer::new(&on(), ch);
        let want: Vec<i32> = samples.iter().map(|x| d.quantize(*x, 16)).collect();
        assert!(got[ch] == want, "channel {ch}");
    }
    assert!(got[0] != got[1], "identical channels get independent noise");
}

#[test]
fn a4b_dither_on_is_repeatable_and_changes_reduced_samples() {
    let dir = TempDir::new("b-repeat");
    let data = vec![off_grid(2 * BLOCK_FRAMES + 123)];
    let first = save_bytes(&dir.0, "a.wav", &data, SampleType::Int24, on());
    let second = save_bytes(&dir.0, "b.wav", &data, SampleType::Int24, on());
    let off = save_bytes(&dir.0, "c.wav", &data, SampleType::Int24, off());
    assert!(first == second, "two dithered saves of the same audio are identical");
    assert!(first != off, "dither on changes some 24-bit samples");
    assert_eq!(first.len(), off.len());
}

#[test]
fn a4b_dither_never_touches_float() {
    let dir = TempDir::new("b-float");
    let data = vec![off_grid(2 * BLOCK_FRAMES + 123)];
    let on = save_bytes(&dir.0, "on-f.wav", &data, SampleType::Float32, on());
    let off = save_bytes(&dir.0, "off-f.wav", &data, SampleType::Float32, off());
    assert!(on == off, "no dither at float sample type");
}

// ---- wav-c-12-c: Settings remember dither ----

const HEAD: &str = concat!(
    "# WavCraft settings. One `key = value` per line.\n",
    "# session_folder: where opened files are kept while you work on them (empty: the default).\n",
    "session_folder = \n",
);
const HIDDEN_LINES: &str = "# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\nhidden_panels = history\n";
const DITHER_LINES: &str = "# dither: dither when reducing bit depth, on or off (off by default, as in Audition).\ndither = on\n";

#[test]
fn a4c_dither_on_is_written_last() {
    let on = Settings { dither: true, ..Settings::default() };
    assert_eq!(on.to_text(), format!("{HEAD}{DITHER_LINES}"));
    let both = Settings { dither: true, hidden_panels: vec!["history".into()], ..Settings::default() };
    assert_eq!(both.to_text(), format!("{HEAD}{HIDDEN_LINES}{DITHER_LINES}"));
}

#[test]
fn a4c_dither_off_keeps_the_old_file_format() {
    assert!(!Settings::default().dither);
    assert_eq!(Settings::default().to_text(), HEAD);
    assert_eq!(Settings::parse("dither = on\n").to_text().matches("dither = on").count(), 1);
}

#[test]
fn a4c_only_on_turns_dither_on() {
    assert!(Settings::parse("dither = on\n").dither);
    assert!(Settings::parse("dither=on").dither);
    assert!(Settings::parse("  dither   =   on  \n").dither);
    assert!(!Settings::parse("dither = off\n").dither);
    assert!(!Settings::parse("dither = yes\n").dither);
    assert!(!Settings::parse("dither = ON\n").dither);
    assert!(!Settings::parse("dither = on\ndither = off\n").dither, "the last line wins");
    assert!(!Settings::parse("").dither);
}

#[test]
fn a4c_dither_survives_save_and_load() {
    let dir = TempDir::new("c-file");
    let file = dir.0.join("settings.txt");
    let s = Settings { dither: true, session_folder: Some(PathBuf::from("/w-new")), ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file), s);
}
