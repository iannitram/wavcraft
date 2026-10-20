//! Dither and noise shaping (roadmap A163): measured on spectra and correlations.

use std::f64::consts::PI;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use wavcraft_core::convert::{self, ConvertSettings};
use wavcraft_core::dither::{DitherSettings, DitherType, Ditherer, Shaping};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::save::{SampleType, quantize, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: f64 = 48_000.0;

fn settings(dither: DitherType, shaping: Shaping) -> DitherSettings {
    DitherSettings { dither, shaping, ..DitherSettings::default() }
}

/// A sine of `amp` (full scale 1) at a bin-centred frequency near 1 kHz, `n` samples (n = 65536).
fn tone(n: usize, amp: f64) -> (Vec<f32>, usize) {
    let bin = 1365; // of 65536: 999.7 Hz at 48 kHz
    let w = 2.0 * PI * bin as f64 / n as f64;
    ((0..n).map(|i| (amp * (w * i as f64).sin()) as f32).collect(), bin)
}

fn run(s: &DitherSettings, x: &[f32], bits: u32) -> Vec<i32> {
    let mut out = vec![0; x.len()];
    Ditherer::new(s, 0).process(x, bits, &mut out);
    out
}

/// Power spectrum of `x` (Hann window), bins 0..=n/2.
fn power(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    let mut buf: Vec<Complex<f64>> =
        x.iter().enumerate().map(|(i, v)| Complex::new(v * (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()), 0.0)).collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    buf[..=n / 2].iter().map(|c| c.norm_sqr()).collect()
}

/// Average power spectrum over 4096-sample segments of `x`.
fn welch(x: &[f64]) -> Vec<f64> {
    let seg = 4096;
    let mut acc = vec![0.0; seg / 2 + 1];
    let mut count = 0.0;
    for c in x.chunks_exact(seg) {
        for (a, p) in acc.iter_mut().zip(power(c)) {
            *a += p;
        }
        count += 1.0;
    }
    acc.iter().map(|a| a / count).collect()
}

/// Sum of `spec` between two frequencies (Hz) for a transform of `2 * (spec.len() - 1)` points.
fn band(spec: &[f64], lo: f64, hi: f64) -> f64 {
    let n = 2.0 * (spec.len() - 1) as f64;
    let (a, b) = ((lo / RATE * n) as usize, (hi / RATE * n) as usize);
    spec[a..b].iter().sum()
}

fn db(ratio: f64) -> f64 {
    10.0 * ratio.log10()
}

fn error_lsb(x: &[f32], q: &[i32], bits: u32) -> Vec<f64> {
    let scale = (1u64 << (bits - 1)) as f64;
    x.iter().zip(q).map(|(&x, &q)| f64::from(q) - f64::from(x) * scale).collect()
}

fn as_f64(x: &[f32]) -> Vec<f64> {
    x.iter().map(|&v| f64::from(v)).collect()
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    sab / (saa * sbb).sqrt()
}

#[test]
fn with_dither_off_conversion_is_exact_rounding() {
    let x: Vec<f32> =
        (0..70_000).map(|i| ((i as f32 * 0.37).sin() * 1.2) + (i % 7) as f32 * 1e-5).chain([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]).collect();
    for bits in [16, 24] {
        let got = run(&DitherSettings::default(), &x, bits);
        let want: Vec<i32> = x.iter().map(|&v| quantize(v, bits)).collect();
        assert_eq!(got, want, "{bits}-bit");
    }
}

#[test]
fn triangular_dither_keeps_a_100_db_tone_that_plain_rounding_loses() {
    let n = 1 << 16;
    let (x, bin) = tone(n, 1e-5); // -100 dBFS: 0.33 LSB at 16 bits, below the rounding threshold
    let plain = run(&DitherSettings::default(), &x, 16);
    assert!(plain.iter().all(|&q| q == 0), "plain rounding makes silence of it");

    let q = run(&settings(DitherType::Triangular, Shaping::None), &x, 16);
    let y: Vec<f64> = q.iter().map(|&v| f64::from(v) / 32768.0).collect();
    let spec = power(&y);
    let tone_power: f64 = spec[bin - 1..=bin + 1].iter().sum();
    // Noise floor: the mean power per bin well away from the tone, scaled to the tone's 3 bins.
    let floor = spec[3000..20000].iter().sum::<f64>() / 17_000.0 * 3.0;
    let margin = db(tone_power / floor);
    assert!(margin > 25.0, "the tone stands {margin:.1} dB above the noise");

    // The error is uncorrelated with the input; without dither it is the input, negated.
    let corr = correlation(&error_lsb(&x, &q, 16), &as_f64(&x));
    assert!(corr.abs() < 0.03, "TPDF error/input correlation {corr:.4}");
    let plain_corr = correlation(&error_lsb(&x, &plain, 16), &as_f64(&x));
    assert!(plain_corr < -0.99, "plain rounding error is the signal: {plain_corr:.4}");
}

#[test]
fn rectangular_dither_also_decorrelates_and_is_quieter_than_triangular() {
    let (x, _) = tone(1 << 16, 1e-5);
    let rect = error_lsb(&x, &run(&settings(DitherType::Rectangular, Shaping::None), &x, 16), 16);
    let tri = error_lsb(&x, &run(&settings(DitherType::Triangular, Shaping::None), &x, 16), 16);
    assert!(correlation(&rect, &as_f64(&x)).abs() < 0.03);
    let ms = |e: &[f64]| e.iter().map(|v| v * v).sum::<f64>() / e.len() as f64;
    // Theory: triangular 1/4 LSB^2 whatever the signal; rectangular at most 1/6, and it varies with
    // the signal (noise modulation: a quiet input gets less), which is why triangular is the default.
    assert!(ms(&rect) < 1.0 / 6.0 + 0.01 && ms(&rect) < ms(&tri) - 0.05, "rectangular error power {}", ms(&rect));
    assert!((ms(&tri) - 0.25).abs() < 0.01, "triangular error power {}", ms(&tri));
}

#[test]
fn noise_shaping_moves_noise_energy_up_in_frequency() {
    let n = 1 << 18;
    let x: Vec<f32> = (0..n).map(|i| (0.1 * (2.0 * PI * 1000.0 * i as f64 / RATE).sin()) as f32).collect();
    let spectrum = |sh| {
        let e = error_lsb(&x, &run(&settings(DitherType::Triangular, sh), &x, 16), 16);
        let w = welch(&e);
        (band(&w, 20.0, 6000.0), band(&w, 18_000.0, 24_000.0))
    };
    let (flat_lo, flat_hi) = spectrum(Shaping::None);
    let (light_lo, light_hi) = spectrum(Shaping::Light);
    let (strong_lo, strong_hi) = spectrum(Shaping::Strong);
    let light_cut = db(flat_lo / light_lo);
    let strong_cut = db(flat_lo / strong_lo);
    assert!(light_cut > 4.0, "light shaping cuts 20 Hz-6 kHz by {light_cut:.1} dB");
    assert!(strong_cut > light_cut + 3.0 && strong_cut > 9.0, "strong shaping cuts it by {strong_cut:.1} dB");
    let (light_up, strong_up) = (db(light_hi / flat_hi), db(strong_hi / flat_hi));
    assert!(light_up > 3.0, "light shaping raises 18-24 kHz by {light_up:.1} dB");
    assert!(strong_up > light_up + 2.0, "strong shaping raises it by {strong_up:.1} dB");
}

#[test]
fn shaped_output_does_not_wind_up_when_it_clips() {
    let x: Vec<f32> = (0..100_000).map(|i| if (i / 50) % 2 == 0 { 3.0 } else { -3.0 }).collect();
    for sh in [Shaping::Light, Shaping::Strong] {
        let mut d = Ditherer::new(&settings(DitherType::Triangular, sh), 0);
        let mut sink = vec![0; x.len()];
        d.process(&x, 16, &mut sink);
        assert!(sink.iter().all(|&v| (-32768..=32767).contains(&v)));
        // Right after the overload ends the output is silence again, give or take a few LSB.
        let mut out = vec![0; 1000];
        d.process(&vec![0.0; 1000], 16, &mut out);
        assert!(out.iter().all(|v| v.abs() <= 8), "{sh:?}: the error feedback stayed bounded");
    }
}

#[test]
fn output_is_deterministic_per_seed_and_the_same_in_chunks() {
    let (x, _) = tone(1 << 14, 0.25);
    let s = settings(DitherType::Triangular, Shaping::Strong);
    let whole = run(&s, &x, 16);
    assert_eq!(whole, run(&s, &x, 16), "same seed, same output");
    let other = run(&DitherSettings { seed: 7, ..s }, &x, 16);
    assert_ne!(whole, other, "another seed, another noise");
    let mut d = Ditherer::new(&s, 0);
    let mut chunked = vec![0; x.len()];
    let mut at = 0usize;
    for len in [1, 100, 4096, 333, 5000, usize::MAX] {
        let end = at.saturating_add(len).min(x.len());
        d.process(&x[at..end], 16, &mut chunked[at..end]);
        at = end;
    }
    assert_eq!(whole, chunked, "filter and noise state carry across chunks");
    let mut ch1 = vec![0; x.len()];
    Ditherer::new(&s, 1).process(&x, 16, &mut ch1);
    assert_ne!(whole, ch1, "channels have their own noise");
}

// ---- through Convert Sample Type and a save -------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-dither-test-{}-{name}", std::process::id()));
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

fn doc_of(dir: &std::path::Path, data: &[f32]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let ch = Channel::from_pieces(data.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()));
    DocState::new(s.clone(), vec![ch], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap()
}

fn read_i16(path: &std::path::Path) -> Vec<i16> {
    let b = std::fs::read(path).unwrap();
    b[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
}

fn converted(doc: &DocState, text: &str) -> DocState {
    let s = ConvertSettings::parse(text).unwrap();
    convert::convert(doc, &s, &AtomicBool::new(false), &mut |_| {}).unwrap().state
}

#[test]
fn convert_settings_carry_dither_to_the_save_and_it_spans_blocks() {
    let t = TempDir::new("save");
    let n = BLOCK_FRAMES * 3 + 123; // several blocks, so the state must carry across them
    let (x, _) = tone(1 << 16, 1e-5);
    let x: Vec<f32> = x.iter().cycle().take(n).copied().collect();
    let doc = doc_of(&t.0.join("s"), &x);

    // Off: exact rounding.
    let plain = converted(&doc, "bits=16");
    assert!(plain.source().dither.is_off());
    let (p_off, p_on) = (t.0.join("off.wav"), t.0.join("on.wav"));
    save_wav(&plain, &p_off, SampleType::Int16, &mut |_| {}).unwrap();
    assert!(read_i16(&p_off).iter().all(|&v| v == 0));

    // On: the saved file equals quantising the whole channel in one go with the same settings.
    let on = converted(&doc, "bits=16 dither=tpdf shaping=light");
    assert_eq!(on.source().dither, settings(DitherType::Triangular, Shaping::Light));
    save_wav(&on, &p_on, SampleType::for_source(on.source()), &mut |_| {}).unwrap();
    let want = run(&settings(DitherType::Triangular, Shaping::Light), &x, 16);
    let got: Vec<i32> = read_i16(&p_on).iter().map(|&v| i32::from(v)).collect();
    assert_eq!(got, want);

    // 32-bit float is not a reduction: no dither touches it.
    let p_f = t.0.join("f.wav");
    save_wav(&on, &p_f, SampleType::Float32, &mut |_| {}).unwrap();
    let b = std::fs::read(&p_f).unwrap();
    let first = f32::from_le_bytes(b[b.len() - 4 * n..][..4].try_into().unwrap());
    assert_eq!(first, x[0]);
}

#[test]
fn dither_settings_text_round_trips_and_rejects_nonsense() {
    let s = ConvertSettings::parse("bits=16 dither=tpdf shaping=strong").unwrap();
    assert_eq!(s.dither, Some(settings(DitherType::Triangular, Shaping::Strong)));
    assert_eq!(ConvertSettings::parse(&s.to_text()).unwrap(), s);
    assert_eq!(ConvertSettings::parse("dither=rectangular").unwrap().dither, Some(settings(DitherType::Rectangular, Shaping::None)));
    assert_eq!(ConvertSettings::parse("bits=24").unwrap().dither, None);
    for bad in ["dither=gaussian", "shaping=extreme", "seed=x"] {
        assert!(ConvertSettings::parse(bad).is_err(), "{bad}");
    }
}
