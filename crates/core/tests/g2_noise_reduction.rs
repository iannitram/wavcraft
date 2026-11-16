//! Noise Reduction (G2): the threshold curve the processor uses (G2.5).

use wavcraft_core::effects;
use wavcraft_core::noise::NoisePrint;

/// A flat print at -60 dB (magnitude 0.001), one channel, 2049 bins (PRINT_FFT / 2 + 1).
fn flat_print_minus_60_db() -> NoisePrint {
    NoisePrint { sample_rate: 48_000, density: vec![vec![0.001f32; 2049]], frames: 1 }
}

fn noise_reduction_params(text: &str) -> Vec<f64> {
    let effect = *effects::all().iter().find(|e| e.id() == "noise_reduction").unwrap();
    effects::parse_params(effect, text).unwrap()
}

#[test]
fn g2_threshold_curve_matches_processor() {
    let print = flat_print_minus_60_db();
    let params = noise_reduction_params("fft_size=4096");
    let curve = effects::threshold_curve_db(&print, 0, &params);

    assert_eq!(curve.len(), 2049);
    let first = curve[0];
    for (k, &v) in curve.iter().enumerate() {
        assert!((v - first).abs() <= 1e-6, "bin {k}: {v} differs from {first}");
    }
    // 20 * log10(0.001 * sqrt(0.375 * 4096) * 3.5)
    assert!((first - -17.2547).abs() < 1e-3, "threshold {first} dB");

    let magnitudes = print.magnitudes(0, 4096);
    for (k, &v) in curve.iter().enumerate() {
        let processor = 20.0 * f64::from(magnitudes[k] * 3.5f32).log10();
        assert!((v - processor).abs() <= 1e-6, "bin {k}: curve {v} processor {processor}");
    }
}

#[test]
fn g2_threshold_curve_length_follows_fft_size() {
    let print = flat_print_minus_60_db();

    let small = effects::threshold_curve_db(&print, 0, &noise_reduction_params("fft_size=1024"));
    assert_eq!(small.len(), 513);
    assert!(small.iter().all(|v| (v - -23.2753).abs() < 1e-3), "1024: {small:?}");

    let large = effects::threshold_curve_db(&print, 0, &noise_reduction_params("fft_size=8192"));
    assert_eq!(large.len(), 4097);
    assert!(large.iter().all(|v| (v - -14.2444).abs() < 1e-3), "8192 first {}", large[0]);
}
