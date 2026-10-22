//! A linear-interpolation varispeed resampler (speed changes pitch and length).

pub fn resample_linear(samples: &[f32], ratio: f32) -> Vec<f32> {
    if samples.is_empty() || !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }

    let output_len = (samples.len() as f32 / ratio).ceil() as usize;
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
}
