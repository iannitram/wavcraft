//! The test kit's acceptance tests (task 02): the same bytes every run, every file opens through
//! WavCraft's own open path, and the ramp file reads back as its formula. Plus checks that each
//! file holds what its name says.

use std::path::{Path, PathBuf};

use wavcraft_core::Document;
use wavcraft_testkit::*;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-testkit-{}-{name}", std::process::id()));
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

fn kit(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    write_kit(&dir.0, &KitOptions::default()).unwrap();
    dir
}

/// FNV-1a, 64-bit: enough to name a file's contents in a failure message.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01B3))
}

fn open(dir: &Path, name: &str) -> Document {
    Document::open(&dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The whole document as one buffer, to compare its content (the app never does this).
fn audio(doc: &Document) -> wavcraft_core::AudioBuffer {
    doc.audio_buffer().unwrap()
}

/// Acceptance test 1: two runs into two folders give byte-identical files.
#[test]
fn the_kit_is_the_same_bytes_every_time() {
    let (a, b) = (TempDir::new("run-a"), TempDir::new("run-b"));
    let first = write_kit(&a.0, &KitOptions::default()).unwrap();
    let second = write_kit(&b.0, &KitOptions::default()).unwrap();
    let names = |paths: &[PathBuf]| paths.iter().map(|p| p.file_name().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(names(&first), names(&second));
    assert_eq!(first.len(), 10, "8 audio files and 2 sidecars: {:?}", names(&first));
    for (x, y) in first.iter().zip(&second) {
        let (bx, by) = (std::fs::read(x).unwrap(), std::fs::read(y).unwrap());
        assert!(!bx.is_empty(), "{} is empty", x.display());
        assert_eq!((bx.len(), fnv(&bx)), (by.len(), fnv(&by)), "{} differs between runs", x.display());
        assert!(bx == by, "{} differs between runs", x.display());
    }
    // Another seed changes the noise, not the deterministic signals.
    let c = TempDir::new("run-c");
    write_kit(&c.0, &KitOptions { seed: 7, ..KitOptions::default() }).unwrap();
    assert_ne!(std::fs::read(a.0.join(PINK)).unwrap(), std::fs::read(c.0.join(PINK)).unwrap());
    assert_eq!(std::fs::read(a.0.join(RAMP)).unwrap(), std::fs::read(c.0.join(RAMP)).unwrap());
}

/// Acceptance test 2: every kit file opens through `Document::open` with the right shape.
#[test]
fn every_kit_file_opens_in_wavcrafts_open_path() {
    let dir = kit("open");
    for e in expected(false) {
        let doc = open(&dir.0, e.name);
        assert_eq!((doc.channels(), doc.sample_rate(), doc.frames()), (e.channels, e.sample_rate, e.frames), "{}", e.name);
        assert_eq!(doc.peaks.len(), e.channels, "{}: one waveform overview per channel", e.name);
    }
}

/// Acceptance test 3: the ramp file reads back as `(n mod 65536) - 32768` for every `n`. The
/// oracle later tasks check edits against.
#[test]
fn the_ramp_reads_back_as_its_formula() {
    let dir = kit("ramp");
    let doc = open(&dir.0, RAMP);
    let whole = audio(&doc);
    let samples = &whole.channels[0];
    assert_eq!(samples.len(), 480_000);
    for (n, v) in samples.iter().enumerate() {
        let want = ramp_value(n as u64);
        assert_eq!(*v * 32_768.0, want as f32, "sample {n}");
    }
    // What makes it an oracle: no value repeats within 65,536 samples (1.36 s at 48 kHz).
    assert_eq!(ramp_value(0), -32_768);
    assert_eq!(ramp_value(65_535), 32_767);
    assert_eq!(ramp_value(65_536), -32_768);
}

#[test]
fn the_tone_is_1_khz_at_minus_20_dbfs_and_its_copies_hold_the_same_samples() {
    let dir = kit("tone");
    let wav = open(&dir.0, TONE);
    for ch in &audio(&wav).channels {
        let peak = ch.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((20.0 * f64::from(peak).log10() + 20.0).abs() < 0.01, "peak {peak}");
        // 1 kHz at 48 kHz: 48 samples a cycle, so 10 s holds 10,000 cycles: 9,999 upward zero
        // crossings after the one at sample 0.
        let ups = ch.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert_eq!(ups, 9_999);
    }
    assert_eq!(audio(&wav).channels[0], audio(&wav).channels[1]);
    for copy in [TONE_AIFF, TONE_FLAC] {
        assert_eq!(audio(&open(&dir.0, copy)), audio(&wav), "{copy}");
    }
}

#[test]
fn pink_is_at_minus_30_dbfs_and_falls_with_frequency() {
    let dir = kit("pink");
    let doc = open(&dir.0, PINK);
    let x: Vec<f64> = audio(&doc).channels[0].iter().map(|v| f64::from(*v)).collect();
    assert!((rms_dbfs(&x) + 30.0).abs() < 0.05, "{} dBFS", rms_dbfs(&x));
    // Pink: the first difference (a rough high-pass) holds less power than the signal. White
    // noise would hold twice as much (+3 dB).
    let diff: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(rms_dbfs(&diff) < rms_dbfs(&x) - 3.0, "{} vs {}", rms_dbfs(&diff), rms_dbfs(&x));
}

#[test]
fn left_and_right_hold_different_tones() {
    let dir = kit("lr");
    let doc = open(&dir.0, LR);
    let ups = |ch: &[f32]| ch.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    // 30 s of 440 Hz and 660 Hz, give or take the first crossing.
    assert!(ups(&audio(&doc).channels[0]).abs_diff(30 * 440) <= 1);
    assert!(ups(&audio(&doc).channels[1]).abs_diff(30 * 660) <= 1);
    assert_eq!(doc.info.sample_format, wavcraft_core::SampleFormat::Float32);
}

#[test]
fn the_clicks_are_where_the_sidecar_says_and_nowhere_else() {
    let dir = kit("clicks");
    let doc = open(&dir.0, CLICKS);
    let listed: Vec<(usize, i32)> = std::fs::read_to_string(dir.0.join(CLICKS_TXT))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let mut it = l.split_whitespace();
            (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap())
        })
        .collect();
    assert!(listed.len() > 40);
    let found: Vec<(usize, i32)> =
        audio(&doc).channels[0].iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(n, v)| (n, (*v * 32_768.0) as i32)).collect();
    assert_eq!(found, listed);
    // The waveform overview keeps a one-sample click visible when zoomed all the way out.
    let cols = doc.waveform_columns(0, 0.0, doc.frames() as f64, 1000);
    assert!(cols.iter().any(|(_, hi)| *hi == 0.5) && cols.iter().any(|(lo, _)| *lo == -0.5));
}

#[test]
fn the_ten_minute_file_holds_its_four_tones() {
    let dir = kit("tones");
    let doc = open(&dir.0, TONES);
    let whole = audio(&doc);
    let x = &whole.channels[0];
    for (start, len, _) in TONE_LIST {
        let mid = ((start + len / 2.0) * 48_000.0) as usize;
        let around = &x[mid - 100..mid + 100];
        assert!(around.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.4, "tone at {start} s");
    }
    let quiet = &x[10 * 48_000..11 * 48_000];
    assert!(quiet.iter().all(|v| v.abs() <= 0.0011));
}

#[test]
fn speech_and_noise_mix_at_the_snr_asked_for() {
    let dir = TempDir::new("speech");
    let inputs = TempDir::new("speech-in");
    // A stand-in "speech" (a tone that comes and goes) and a shorter noise file that has to loop.
    let speech = inputs.0.join("speech.wav");
    let noise = inputs.0.join("noise.wav");
    let mut w = WavWriter::create(&speech, 1, 16_000, Encoding::Float32, 32_000).unwrap();
    w.write(&(0..32_000).map(|n| if (n / 4000) % 2 == 0 { 0.3 * (n as f64 * 0.2).sin() } else { 0.0 }).collect::<Vec<_>>()).unwrap();
    w.finish().unwrap();
    let mut rng = Rng::new(1, "noise");
    let mut w = WavWriter::create(&noise, 1, 16_000, Encoding::Float32, 5_000).unwrap();
    w.write(&(0..5_000).map(|_| rng.signed() * 0.1).collect::<Vec<_>>()).unwrap();
    w.finish().unwrap();
    let paths = write_kit(&dir.0, &KitOptions { speech: Some((speech, noise)), ..KitOptions::default() }).unwrap();
    for name in [SPEECH_CLEAN, SPEECH_SNR10, SPEECH_SNR20, SPEECH_TXT] {
        assert!(paths.contains(&dir.0.join(name)), "{name}");
    }
    let clean = open(&dir.0, SPEECH_CLEAN);
    let power = |x: &[f32]| x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    for (name, snr) in [(SPEECH_SNR10, 10.0), (SPEECH_SNR20, 20.0)] {
        let mixed = open(&dir.0, name);
        assert_eq!(mixed.frames(), 32_000);
        let noise: Vec<f32> = audio(&mixed).channels[0].iter().zip(&audio(&clean).channels[0]).map(|(m, c)| m - c).collect();
        let got = 10.0 * (power(&audio(&clean).channels[0]) / power(&noise)).log10();
        assert!((got - snr).abs() < 0.05, "{name}: {got} dB");
    }
    // Without both inputs nothing is mixed.
    assert!(!kit("no-speech").0.join(SPEECH_CLEAN).exists());
}
