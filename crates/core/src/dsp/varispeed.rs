//! A linear-interpolation varispeed resampler (speed changes pitch and length).

const MAX_OUTPUT_LEN: usize = 1 << 30;

pub fn resample_linear(samples: &[f32], ratio: f32) -> Vec<f32> {
    if samples.is_empty() || !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }

    let raw_len = (samples.len() as f32 / ratio).ceil();
    if !raw_len.is_finite() || raw_len > MAX_OUTPUT_LEN as f32 {
        return Vec::new();
    }
    let output_len = raw_len as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let pos = i as f32 * ratio;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f32;

        let sample_a = samples.get(idx).unwrap_or(&samples[samples.len() - 1]);
        let sample_b = samples.get(idx + 1).unwrap_or(&samples[samples.len() - 1]);

        let interpolated = *sample_a + frac * (*sample_b - *sample_a);
        output.push(interpolated);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::resample_linear;

    #[test]
    fn ratio_two_halves_ramp() {
        let out = resample_linear(&[0.0, 1.0, 2.0, 3.0], 2.0);
        let expected = [0.0_f32, 2.0];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn fractional_ratio_half_interpolates_between_samples() {
        let out = resample_linear(&[0.0, 1.0, 4.0, 9.0, 16.0], 0.5);
        let expected = [0.0_f32, 0.5, 1.0, 2.5, 4.0, 6.5, 9.0, 12.5, 16.0, 16.0];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn fractional_ratio_one_and_half_interpolates_between_samples() {
        let out = resample_linear(&[0.0, 1.0, 4.0, 9.0, 16.0], 1.5);
        let expected = [0.0_f32, 2.5, 9.0, 16.0];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn non_integer_position_one_and_quarter_interpolates() {
        let out = resample_linear(&[0.0, 1.0, 4.0, 9.0, 16.0], 1.25);
        let expected = [0.0_f32, 1.75, 6.5, 14.25];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn tiny_ratio_returns_empty_instead_of_panicking() {
        let out = resample_linear(&[0.0], f32::MIN_POSITIVE);
        assert!(out.is_empty());

        // positive control: the same function still produces output for a normal ratio
        let control = resample_linear(&[0.0, 1.0, 4.0, 9.0, 16.0], 1.0);
        let expected = [0.0_f32, 1.0, 4.0, 9.0, 16.0];
        assert_eq!(control.len(), expected.len());
        for (got, want) in control.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }
}
