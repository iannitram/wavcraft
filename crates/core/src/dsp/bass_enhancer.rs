//! One-pole bass exciter: split off the low band, saturate it, add it back.

/// For each sample: low = alpha * sample + (1 - alpha) * previous_low;
/// saturated = tanh(drive * low); output = sample + amount * (saturated - low).
pub fn enhance(samples: &[f32], alpha: f32, drive: f32, amount: f32) -> Vec<f32> {
    // Guard: if any parameter is NaN or infinite, return input unchanged
    if !alpha.is_finite() || !drive.is_finite() || !amount.is_finite() {
        return samples.to_vec();
    }

    // Clamp alpha to [0.0, 1.0]
    let alpha = alpha.clamp(0.0, 1.0);

    let mut low = 0.0_f32;
    let mut out = Vec::with_capacity(samples.len());
    for &sample in samples {
        // Handle non-finite input samples by treating them as 0.0
        let sample = if sample.is_finite() { sample } else { 0.0 };
        low = alpha * sample + (1.0 - alpha) * low;
        let saturated = (drive * low).tanh();
        out.push(sample + amount * (saturated - low));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::enhance;

    #[test]
    fn one_pole_exciter_has_literal_fixture() {
        let out = enhance(&[0.0, 1.0, 0.0, 0.0], 0.5, 2.0, 1.0);
        let expected = [0.0_f32, 1.261594, 0.212117, 0.119919];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn non_finite_parameters_return_input() {
        let input = [0.0, 1.0, 0.0, 0.0];
        assert_eq!(enhance(&input, f32::NAN, 2.0, 1.0), input);
        assert_eq!(enhance(&input, 0.5, f32::INFINITY, 1.0), input);
        assert_eq!(enhance(&input, 0.5, 2.0, f32::NAN), input);
    }

    #[test]
    fn alpha_above_one_is_clamped() {
        let got = enhance(&[0.0, 1.0, 0.0, 0.0], 2.0, 2.0, 1.0);
        let expected = [0.0_f32, 0.9640276, 0.0, 0.0];
        for (got, want) in got.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
        let reference = enhance(&[0.0, 1.0, 0.0, 0.0], 1.0, 2.0, 1.0);
        for (got, want) in got.iter().zip(reference.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn negative_alpha_is_clamped_to_zero() {
        let got = enhance(&[0.0, 1.0, 0.0, 0.0], -0.5, 2.0, 1.0);
        let expected = [0.0_f32, 1.0, 0.0, 0.0];
        for (got, want) in got.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
        assert_eq!(got, enhance(&[0.0, 1.0, 0.0, 0.0], 0.0, 2.0, 1.0));
    }

    #[test]
    fn non_finite_sample_is_zero() {
        let got = enhance(&[0.0, f32::NAN, 1.0, f32::INFINITY], 0.5, 2.0, 1.0);
        let expected = [0.0_f32, 0.0, 1.2615942, 0.21211717];
        for (got, want) in got.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
        assert!(got.iter().all(|sample| sample.is_finite()));
    }
}
