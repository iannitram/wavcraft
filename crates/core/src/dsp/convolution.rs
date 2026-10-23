//! FFT-based convolution and a convolution reverb for mono samples.
//!
//! The FFT is `rustfft` with `f64` internal buffers.
//!
//! Not in this change: IR file loading; EQ.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex64;

/// Inputs whose direct cost `n * m` is at most this use the direct path.
const DIRECT_LIMIT: usize = 64;

/// Full linear convolution of `input` with `ir`.
///
/// The output length is `input.len() + ir.len() - 1`. An empty `input` or an
/// empty `ir` returns an empty `Vec`. Large inputs use an FFT zero-padded to
/// the next power of two; tiny inputs use a direct O(n*m) sum.
pub fn convolve(input: &[f32], ir: &[f32]) -> Vec<f32> {
    if input.is_empty() || ir.is_empty() {
        return Vec::new();
    }
    let out_len = input.len() + ir.len() - 1;
    if input.len() * ir.len() <= DIRECT_LIMIT {
        return convolve_direct(input, ir, out_len);
    }

    let size = out_len.next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);

    let mut a = to_padded(input, size);
    let mut b = to_padded(ir, size);
    forward.process(&mut a);
    forward.process(&mut b);
    for (x, y) in a.iter_mut().zip(b.iter()) {
        *x *= *y;
    }
    inverse.process(&mut a);

    let scale = 1.0 / size as f64;
    a.iter().take(out_len).map(|c| (c.re * scale) as f32).collect()
}

/// Direct O(n*m) convolution accumulated in `f64`.
fn convolve_direct(input: &[f32], ir: &[f32], out_len: usize) -> Vec<f32> {
    let mut out = vec![0.0f64; out_len];
    for (i, &x) in input.iter().enumerate() {
        for (j, &h) in ir.iter().enumerate() {
            if let Some(slot) = out.get_mut(i + j) {
                *slot += f64::from(x) * f64::from(h);
            }
        }
    }
    out.iter().map(|&v| v as f32).collect()
}

/// Converts samples to complex `f64` and zero-pads to `size`.
fn to_padded(samples: &[f32], size: usize) -> Vec<Complex64> {
    let mut buffer: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(f64::from(s), 0.0)).collect();
    buffer.resize(size, Complex64::new(0.0, 0.0));
    buffer
}

/// Applies a convolution reverb using the impulse response `ir`.
///
/// The wet signal is `convolve(input, ir_used)`, where `ir_used` is `ir`
/// scaled to unit energy (sum of squares of 1) when `normalize_ir` is true,
/// else `ir` unchanged. A zero-energy IR is not divided, so it stays zero and
/// produces no NaN. The dry signal is zero-padded to
/// `input.len() + ir.len() - 1` so the reverb tail is kept. `mix` is clamped
/// to `0..=1` and a NaN `mix` is treated as 0. The output is
/// `(1 - mix) * dry + mix * wet`. An empty `input` or `ir` returns an empty
/// `Vec`.
pub fn convolution_reverb(input: &[f32], ir: &[f32], mix: f32, normalize_ir: bool) -> Vec<f32> {
    if input.is_empty() || ir.is_empty() {
        return Vec::new();
    }
    let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };

    let energy: f64 = ir.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let ir_used: Vec<f32> = if normalize_ir && energy > 0.0 {
        let gain = 1.0 / energy.sqrt();
        ir.iter().map(|&s| (f64::from(s) * gain) as f32).collect()
    } else {
        ir.to_vec()
    };

    let wet = convolve(input, &ir_used);
    wet.iter()
        .enumerate()
        .map(|(index, &w)| {
            let dry = input.get(index).copied().unwrap_or(0.0);
            (1.0 - mix) * dry + mix * w
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{convolution_reverb, convolve};

    fn assert_close(actual: &[f32], expected: &[f32], tolerance: f32) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            assert!((actual - expected).abs() <= tolerance, "{actual} vs {expected}");
        }
    }

    fn lcg(seed: u64, count: usize) -> Vec<f32> {
        let modulus = 1u64 << 31;
        let mut state = seed;
        (0..count)
            .map(|_| {
                state = (state * 1_103_515_245 + 12_345) % modulus;
                (state as f64 / modulus as f64 * 2.0 - 1.0) as f32
            })
            .collect()
    }

    fn direct(a: &[f32], b: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f64; a.len() + b.len() - 1];
        for (i, &x) in a.iter().enumerate() {
            for (j, &h) in b.iter().enumerate() {
                out[i + j] += f64::from(x) * f64::from(h);
            }
        }
        out.iter().map(|&v| v as f32).collect()
    }

    const IR5: [f32; 5] = [0.5, -0.25, 0.125, 0.0625, -0.03125];

    #[test]
    fn convolve_delta_ir_returns_input() {
        assert_close(&convolve(&[1.0, 2.0, 3.0], &[1.0]), &[1.0, 2.0, 3.0], 1e-6);
    }

    #[test]
    fn convolve_delay_ir_shifts_by_two() {
        assert_close(&convolve(&[1.0, 2.0, 3.0], &[0.0, 0.0, 1.0]), &[0.0, 0.0, 1.0, 2.0, 3.0], 1e-6);
    }

    #[test]
    fn convolve_known_small_example() {
        assert_close(&convolve(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5]), &[0.0, 1.0, 2.5, 4.0, 1.5], 1e-6);
    }

    #[test]
    fn convolve_input_shorter_than_ir() {
        assert_close(&convolve(&[1.0, 2.0], &[1.0, 2.0, 3.0, 4.0]), &[1.0, 4.0, 7.0, 10.0, 8.0], 1e-6);
    }

    #[test]
    fn convolve_lcg_known_values() {
        let x = lcg(1, 12);
        let expected = [
            0.0138701, -0.331194, -0.0257516, 0.0508772, 0.341125, -0.547097, 0.494546, -0.402956, 0.113106, -0.395263, -0.276604, 0.0203226,
            -0.0954322, -0.0561459, 0.0122122, 0.00689705,
        ];
        assert_close(&convolve(&x, &IR5), &expected, 1e-5);
    }

    #[test]
    fn convolve_matches_direct_on_1000_samples() {
        let x = lcg(7, 1000);
        let ir = lcg(99, 9);
        assert_close(&convolve(&x, &ir), &direct(&x, &ir), 1e-5);
    }

    #[test]
    fn convolve_doubling_ir_doubles_output() {
        let x = lcg(1, 12);
        let doubled: Vec<f32> = IR5.iter().map(|s| s * 2.0).collect();
        let single: Vec<f32> = convolve(&x, &IR5).iter().map(|s| s * 2.0).collect();
        assert_close(&convolve(&x, &doubled), &single, 1e-5);
    }

    #[test]
    fn convolve_commutes() {
        let x = lcg(1, 12);
        assert_close(&convolve(&x, &IR5), &convolve(&IR5, &x), 1e-5);
    }

    #[test]
    fn convolve_output_length_is_n_plus_m_minus_1() {
        for (n, m) in [(1, 1), (3, 5), (12, 5), (100, 33), (257, 300), (300, 300)] {
            let out = convolve(&lcg(3, n), &lcg(4, m));
            assert_eq!(out.len(), n + m - 1);
        }
    }

    #[test]
    fn convolve_crosses_power_of_two_257_300() {
        let x = lcg(5, 257);
        let ir = lcg(6, 300);
        assert_close(&convolve(&x, &ir), &direct(&x, &ir), 1e-5);
    }

    #[test]
    fn convolve_empty_input_returns_empty() {
        assert!(convolve(&[], &[1.0, 2.0]).is_empty());
        assert!(convolve(&[], &[]).is_empty());
    }

    #[test]
    fn convolve_empty_ir_returns_empty() {
        assert!(convolve(&[1.0, 2.0], &[]).is_empty());
    }

    #[test]
    fn reverb_mix_zero_returns_dry_padded() {
        let out = convolution_reverb(&[1.0, 2.0, 3.0], &[0.0, 0.0, 1.0], 0.0, false);
        assert_close(&out, &[1.0, 2.0, 3.0, 0.0, 0.0], 1e-6);
    }

    #[test]
    fn reverb_mix_one_returns_wet_only() {
        let out = convolution_reverb(&[1.0, 2.0, 3.0], &[0.0, 0.0, 1.0], 1.0, false);
        assert_close(&out, &[0.0, 0.0, 1.0, 2.0, 3.0], 1e-6);
    }

    #[test]
    fn reverb_mix_half_blends() {
        let out = convolution_reverb(&[1.0, 2.0, 3.0], &[0.0, 0.0, 1.0], 0.5, false);
        assert_close(&out, &[0.5, 1.0, 2.0, 1.0, 1.5], 1e-6);
    }

    #[test]
    fn reverb_mix_clamped_and_nan_is_zero() {
        let x = [1.0, 2.0, 3.0];
        let ir = [0.0, 0.0, 1.0];
        let wet = convolution_reverb(&x, &ir, 1.0, false);
        let dry = convolution_reverb(&x, &ir, 0.0, false);
        assert_close(&convolution_reverb(&x, &ir, 2.0, false), &wet, 1e-6);
        assert_close(&convolution_reverb(&x, &ir, -1.0, false), &dry, 1e-6);
        assert_close(&convolution_reverb(&x, &ir, f32::NAN, false), &dry, 1e-6);
    }

    #[test]
    fn reverb_normalize_ir_scales_to_unit_energy() {
        let normalized = convolution_reverb(&[1.0], &[3.0, 4.0], 1.0, true);
        assert_close(&normalized, &[0.6, 0.8], 1e-6);
        let energy: f32 = normalized.iter().map(|s| s * s).sum();
        assert!((energy - 1.0).abs() <= 1e-6);
        assert_close(&convolution_reverb(&[1.0], &[3.0, 4.0], 1.0, false), &[3.0, 4.0], 1e-6);
    }

    #[test]
    fn reverb_empty_cases_return_empty() {
        assert!(convolution_reverb(&[], &[1.0], 0.5, true).is_empty());
        assert!(convolution_reverb(&[1.0], &[], 0.5, true).is_empty());
    }

    #[test]
    fn reverb_zero_ir_normalize_has_no_nan() {
        let out = convolution_reverb(&[1.0, 2.0], &[0.0, 0.0], 1.0, true);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|s| s.is_finite() && *s == 0.0));
    }
}
