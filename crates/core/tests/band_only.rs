//! A319 core: the band filter for "play only the selected frequencies".
//!
//! Expected gains come from first principles. The band 1000..4000 Hz at 48000 Hz, order 4, is a
//! Butterworth highpass at 1000 Hz followed by a Butterworth lowpass at 4000 Hz. A Butterworth gain is
//! 1/sqrt(1+(f/fc)^(2n)) for the lowpass and 1/sqrt(1+(fc/f)^(2n)) for the highpass.
//! - 2000 Hz: highpass 1/sqrt(1+(1/2)^8) = 0.998, lowpass 0.998, together 0.996 (tests accept 0.97..1.02).
//! - 250 Hz: highpass 1/sqrt(1+4^8) = 0.0039 (tests accept below 0.02).
//! - 12000 Hz: the lowpass is at most 0.0123 (the bilinear transform lowers it further; tests accept below 0.03).
//!
//! Gains are measured as the RMS ratio over the last half of one second of a sine, after the start-up transient.

use wavcraft_core::dsp::band_only::{BandError, BandOnly, band_filter, design_band, validate_band};

const SR: f64 = 48000.0;
const N: usize = 48000;

fn sine(freq: f64, amp: f64) -> Vec<f32> {
    (0..N).map(|i| (amp * (2.0 * std::f64::consts::PI * freq * i as f64 / SR).sin()) as f32).collect()
}

fn tail_rms(x: &[f32]) -> f64 {
    let tail = &x[x.len() / 2..];
    (tail.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / tail.len() as f64).sqrt()
}

/// RMS of the filtered tail divided by the RMS of the input tail.
fn gain(freq: f64, low: f64, high: f64) -> f64 {
    let input = sine(freq, 0.5);
    let out = band_filter(&input, low, high, SR, 4).unwrap_or_default();
    assert_eq!(out.len(), input.len());
    tail_rms(&out) / tail_rms(&input)
}

#[test]
fn in_band_tone_passes_with_gain_near_one() {
    let g = gain(2000.0, 1000.0, 4000.0);
    assert!((0.97..=1.02).contains(&g), "gain {g}");
}

#[test]
fn tone_below_the_band_is_attenuated() {
    let g = gain(250.0, 1000.0, 4000.0);
    assert!(g < 0.02, "gain {g}");
}

#[test]
fn tone_above_the_band_is_attenuated() {
    let g = gain(12000.0, 1000.0, 4000.0);
    assert!(g < 0.03, "gain {g}");
}

#[test]
fn mixed_tones_keep_only_the_in_band_one() {
    let a = sine(250.0, 0.3);
    let b = sine(2000.0, 0.3);
    let c = sine(12000.0, 0.3);
    let mix: Vec<f32> = (0..N).map(|i| a[i] + b[i] + c[i]).collect();
    let out = band_filter(&mix, 1000.0, 4000.0, SR, 4).unwrap_or_default();
    // A 0.3 amplitude sine has RMS 0.2121; times the in-band gain 0.996 is 0.211.
    let rms = tail_rms(&out);
    assert!((0.20..=0.22).contains(&rms), "rms {rms}");
}

#[test]
fn dc_is_removed_when_the_band_starts_above_zero() {
    let input = vec![0.5_f32; N];
    let out = band_filter(&input, 1000.0, 4000.0, SR, 4).unwrap_or_default();
    let worst = out[N / 2..].iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
    assert!(worst < 0.005, "worst {worst}");
}

#[test]
fn zero_low_edge_is_lowpass_only() {
    let low = gain(100.0, 0.0, 4000.0);
    assert!((0.97..=1.02).contains(&low), "gain {low}");
    let high = gain(12000.0, 0.0, 4000.0);
    assert!(high < 0.03, "gain {high}");
}

#[test]
fn nyquist_high_edge_is_highpass_only() {
    let high = gain(12000.0, 1000.0, 24000.0);
    assert!((0.97..=1.02).contains(&high), "gain {high}");
    let low = gain(250.0, 1000.0, 24000.0);
    assert!(low < 0.02, "gain {low}");
}

#[test]
fn full_band_passes_the_input_unchanged() {
    let input = sine(3000.0, 0.5);
    let out = band_filter(&input, 0.0, 24000.0, SR, 4).unwrap_or_default();
    assert_eq!(out, input);
}

#[test]
fn empty_input_gives_empty_output() {
    assert_eq!(band_filter(&[], 1000.0, 4000.0, SR, 4), Ok(Vec::new()));
}

#[test]
fn validate_accepts_good_bands() {
    assert_eq!(validate_band(1000.0, 4000.0, SR), Ok(()));
    assert_eq!(validate_band(0.0, 24000.0, SR), Ok(()));
    assert_eq!(validate_band(100.0, 200.0, SR), Ok(()));
}

#[test]
fn validate_rejects_low_not_below_high() {
    assert_eq!(validate_band(3000.0, 1000.0, SR), Err(BandError::LowNotBelowHigh));
    assert_eq!(validate_band(1000.0, 1000.0, SR), Err(BandError::LowNotBelowHigh));
}

#[test]
fn validate_rejects_out_of_range() {
    assert_eq!(validate_band(-1.0, 1000.0, SR), Err(BandError::OutOfRange));
    assert_eq!(validate_band(1000.0, 30000.0, SR), Err(BandError::OutOfRange));
    assert_eq!(validate_band(1000.0, 24000.5, SR), Err(BandError::OutOfRange));
}

#[test]
fn validate_rejects_non_finite_edges() {
    assert_eq!(validate_band(f64::NAN, 1000.0, SR), Err(BandError::NotFinite));
    assert_eq!(validate_band(100.0, f64::INFINITY, SR), Err(BandError::NotFinite));
    assert_eq!(validate_band(f64::NEG_INFINITY, 1000.0, SR), Err(BandError::NotFinite));
}

#[test]
fn validate_rejects_bad_sample_rate() {
    assert_eq!(validate_band(100.0, 200.0, 0.0), Err(BandError::InvalidSampleRate));
    assert_eq!(validate_band(100.0, 200.0, -48000.0), Err(BandError::InvalidSampleRate));
    assert_eq!(validate_band(100.0, 200.0, f64::NAN), Err(BandError::InvalidSampleRate));
}

#[test]
fn design_counts_sections_by_order_and_open_edges() {
    // Butterworth order 4 is two sections; order 2 is one; order 3 is a first-order plus a second-order section.
    assert_eq!(design_band(1000.0, 4000.0, SR, 4).map(|s| s.len()), Ok(4));
    assert_eq!(design_band(1000.0, 4000.0, SR, 2).map(|s| s.len()), Ok(2));
    assert_eq!(design_band(1000.0, 4000.0, SR, 3).map(|s| s.len()), Ok(4));
    assert_eq!(design_band(0.0, 4000.0, SR, 4).map(|s| s.len()), Ok(2));
    assert_eq!(design_band(1000.0, 24000.0, SR, 4).map(|s| s.len()), Ok(2));
    assert_eq!(design_band(0.0, 24000.0, SR, 4).map(|s| s.len()), Ok(0));
}

#[test]
fn design_puts_highpass_sections_before_lowpass_sections() {
    // The first section of a highpass has a zero at DC (b0 + b1 + b2 = 0); a lowpass section does not.
    let sections = design_band(1000.0, 4000.0, SR, 2).unwrap_or_default();
    assert_eq!(sections.len(), 2);
    assert!((sections[0].b0 + sections[0].b1 + sections[0].b2).abs() < 1e-9);
    assert!((sections[1].b0 + sections[1].b1 + sections[1].b2).abs() > 1e-4);
}

#[test]
fn zero_order_is_an_error() {
    assert_eq!(design_band(1000.0, 4000.0, SR, 0).map(|s| s.len()), Err(BandError::ZeroOrder));
    assert_eq!(band_filter(&[0.1, 0.2], 1000.0, 4000.0, SR, 0), Err(BandError::ZeroOrder));
}

#[test]
fn bad_band_is_an_error_from_design_and_filter() {
    assert_eq!(design_band(3000.0, 1000.0, SR, 4).map(|s| s.len()), Err(BandError::LowNotBelowHigh));
    assert_eq!(band_filter(&[0.1], 3000.0, 1000.0, SR, 4), Err(BandError::LowNotBelowHigh));
    assert_eq!(band_filter(&[0.1], 100.0, 200.0, 0.0, 4), Err(BandError::InvalidSampleRate));
}

#[test]
fn new_toggle_is_disabled_and_keeps_the_band() {
    let t = BandOnly::new(1000.0, 4000.0);
    assert!(!t.enabled);
    assert_eq!(t.low_hz, 1000.0);
    assert_eq!(t.high_hz, 4000.0);
}

#[test]
fn disabled_toggle_returns_the_input_even_for_a_bad_band() {
    let input = sine(250.0, 0.5);
    let t = BandOnly::new(1000.0, 4000.0);
    assert_eq!(t.process(&input, SR, 4), Ok(input.clone()));
    let bad = BandOnly::new(3000.0, 1000.0);
    assert_eq!(bad.process(&input, SR, 4), Ok(input));
}

#[test]
fn enabled_toggle_matches_band_filter() {
    let input = sine(2000.0, 0.4);
    let mut t = BandOnly::new(1000.0, 4000.0);
    t.enabled = true;
    assert_eq!(t.process(&input, SR, 4), band_filter(&input, 1000.0, 4000.0, SR, 4));
    let quiet = sine(250.0, 0.4);
    let out = t.process(&quiet, SR, 4).unwrap_or_default();
    assert!(tail_rms(&out) / tail_rms(&quiet) < 0.02);
}

#[test]
fn enabled_toggle_with_a_bad_band_is_an_error() {
    let mut t = BandOnly::new(3000.0, 1000.0);
    t.enabled = true;
    assert_eq!(t.process(&[0.1, 0.2], SR, 4), Err(BandError::LowNotBelowHigh));
}

#[test]
fn set_band_stores_a_valid_band() {
    let mut t = BandOnly::new(1000.0, 4000.0);
    assert_eq!(t.set_band(200.0, 800.0, SR), Ok(()));
    assert_eq!(t.low_hz, 200.0);
    assert_eq!(t.high_hz, 800.0);
    assert!(!t.enabled);
}

#[test]
fn set_band_with_an_invalid_band_changes_nothing() {
    let mut t = BandOnly::new(1000.0, 4000.0);
    t.enabled = true;
    assert_eq!(t.set_band(900.0, 100.0, SR), Err(BandError::LowNotBelowHigh));
    assert_eq!(t.set_band(100.0, 30000.0, SR), Err(BandError::OutOfRange));
    assert_eq!(t, BandOnly { enabled: true, low_hz: 1000.0, high_hz: 4000.0 });
}

#[test]
fn lowpass_edge_above_half_nyquist_is_applied() {
    // Lowpass order 4 at 15000 Hz: a 22000 Hz tone is far above it (analogue gain 1/sqrt(1+(tan ratio)^8) < 0.002).
    let above = gain(22000.0, 1000.0, 15000.0);
    assert!(above < 0.02, "gain {above}");
    let inside = gain(5000.0, 1000.0, 15000.0);
    assert!((0.97..=1.02).contains(&inside), "gain {inside}");
}
