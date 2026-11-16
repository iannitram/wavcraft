//! C4.1: blend_in_band adds back only the masked part of what a process changed.

use wavcraft_core::Selection;
use wavcraft_core::stft::{Stft, blend_in_band};
use wavcraft_core::view::{Band, SpectralSelection};

const RATE: u32 = 48_000;
const N: usize = 240_000;

/// Pink noise: xorshift64 white noise through Paul Kellet's filter.
fn pink(frames: usize, seed: u64) -> Vec<f32> {
    let mut x = seed.max(1);
    let (mut b0, mut b1, mut b2) = (0.0f64, 0.0f64, 0.0f64);
    (0..frames)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let w = (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0;
            b0 = 0.99765 * b0 + w * 0.0990460;
            b1 = 0.96300 * b1 + w * 0.2965164;
            b2 = 0.57000 * b2 + w * 1.0526913;
            ((b0 + b1 + b2 + w * 0.1848) * 0.25) as f32
        })
        .collect()
}

fn input(frames: usize) -> Vec<f32> {
    let mut noise = pink(frames, 1);
    let rms = (noise.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / frames as f64).sqrt();
    let k = 0.031623 / rms;
    for (i, v) in noise.iter_mut().enumerate() {
        let sine = 0.251189 * (std::f64::consts::TAU * 1000.0 * i as f64 / f64::from(RATE)).sin();
        *v = (f64::from(*v) * k + sine) as f32;
    }
    noise
}

fn band_power(sig: &[f32], lo_hz: f32, hi_hz: f32) -> f64 {
    let stft = Stft::new(8192, 50).unwrap();
    let mut scratch = Vec::new();
    let mut spec = vec![rustfft::num_complex::Complex32::new(0.0, 0.0); stft.bins()];
    let mut sum = 0.0f64;
    for s in (105_600..134_400).step_by(4096) {
        stft.analyse(sig, 0, s as i64, &mut scratch, &mut spec);
        for (b, c) in spec.iter().enumerate() {
            let hz = b as f32 * 48_000.0 / 8192.0;
            if (lo_hz..=hi_hz).contains(&hz) {
                sum += f64::from(c.norm_sqr());
            }
        }
    }
    sum
}

#[test]
fn the_tone_drops_and_the_noise_stays() {
    let inp = input(N);
    let processed: Vec<f32> = inp.iter().map(|v| v * 0.1).collect();
    let sel = SpectralSelection { time: Selection { anchor: 96_000, head: 144_000 }, band: Band::new(900.0, 1100.0, 24_000.0), rate: RATE };
    let stft = Stft::new(8192, 50).unwrap();
    let out = blend_in_band(&stft, &inp, &processed, 0, RATE, |f, hz| sel.mask(f, hz)).unwrap();
    let tone = 10.0 * (band_power(&out, 900.0, 1100.0) / band_power(&inp, 900.0, 1100.0)).log10();
    let hiss = 10.0 * (band_power(&out, 2000.0, 8000.0) / band_power(&inp, 2000.0, 8000.0)).log10();
    println!("tone change {tone:.3} dB, 2-8 kHz change {hiss:.3} dB");
    assert!((tone + 20.0).abs() <= 1.0, "tone changed {tone} dB");
    assert!(hiss.abs() < 0.5, "noise changed {hiss} dB");
    for i in 0..91_200 {
        assert_eq!(out[i].to_bits(), inp[i].to_bits(), "sample {i}");
    }
}

#[test]
fn unmasked_blend_returns_the_input() {
    let inp = input(48_000);
    let processed: Vec<f32> = inp.iter().map(|v| v * 0.1).collect();
    let stft = Stft::new(8192, 50).unwrap();
    let out = blend_in_band(&stft, &inp, &processed, 0, RATE, |_, _| 0.0).unwrap();
    assert_eq!(out.len(), inp.len());
    assert!(out.iter().zip(&inp).all(|(a, b)| a.to_bits() == b.to_bits()));
}

#[test]
fn a_full_mask_returns_the_processed_audio() {
    let inp = input(48_000);
    let processed: Vec<f32> = inp.iter().map(|v| v * 0.1).collect();
    let stft = Stft::new(8192, 50).unwrap();
    let out = blend_in_band(&stft, &inp, &processed, 0, RATE, |_, _| 1.0).unwrap();
    let worst = out.iter().zip(&processed).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 0.0001, "largest difference {worst}");
}

#[test]
fn a_length_mismatch_is_an_error() {
    let stft = Stft::new(8192, 50).unwrap();
    assert!(blend_in_band(&stft, &[0.0; 100], &[0.0; 99], 0, RATE, |_, _| 1.0).is_err());
}
