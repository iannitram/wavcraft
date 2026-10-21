/// Maximum feedback gain (roadmap row B043: feedback 0..0.95).
pub const MAX_FEEDBACK: f32 = 0.95;

/// Applies a feedback delay to mono samples.
pub fn echo(input: &[f32], delay_samples: usize, feedback: f32, mix: f32) -> Vec<f32> {
    if delay_samples == 0 {
        return input.to_vec();
    }

    let feedback = if feedback.is_nan() { 0.0 } else { feedback.clamp(0.0, MAX_FEEDBACK) };
    let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };
    let mut wet = vec![0.0; input.len()];
    input
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let delayed = index.checked_sub(delay_samples).and_then(|previous| wet.get(previous).copied()).unwrap_or(0.0);
            let wet_sample = sample + feedback * delayed;
            if let Some(slot) = wet.get_mut(index) {
                *slot = wet_sample;
            }
            (1.0 - mix) * sample + mix * wet_sample
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::echo;

    fn assert_close(actual: &[f32], expected: &[f32], tolerance: f32) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            assert!((actual - expected).abs() <= tolerance);
        }
    }

    #[test]
    fn impulse_delay_three() {
        let mut input = vec![0.0; 12];
        input[0] = 1.0;
        let output = echo(&input, 3, 0.5, 1.0);
        assert_eq!(output, vec![1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.25, 0.0, 0.0, 0.125, 0.0, 0.0]);
    }

    #[test]
    fn impulse_delay_two() {
        let mut input = vec![0.0; 9];
        input[0] = 1.0;
        let output = echo(&input, 2, 0.5, 1.0);
        assert_eq!(output, vec![1.0, 0.0, 0.5, 0.0, 0.25, 0.0, 0.125, 0.0, 0.0625]);
    }

    #[test]
    fn mix_zero_is_identity() {
        let input = [0.3, -0.2, 0.9, 0.0, 0.5];
        assert_eq!(echo(&input, 2, 0.7, 0.0), input);
    }

    #[test]
    fn mix_half() {
        let mut input = vec![0.0; 12];
        input[0] = 1.0;
        let output = echo(&input, 3, 0.5, 0.5);
        assert_eq!(output, vec![1.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.125, 0.0, 0.0, 0.0625, 0.0, 0.0]);
    }

    #[test]
    fn feedback_is_clamped() {
        for feedback in [1.0, 2.0] {
            let mut input = vec![0.0; 5];
            input[0] = 1.0;
            let output = echo(&input, 1, feedback, 1.0);
            assert_close(&output, &[1.0, 0.95, 0.9025, 0.857375, 0.81450625], 1e-5);
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn feedback_exactly_max_is_unchanged() {
        let mut input = vec![0.0; 5];
        input[0] = 1.0;
        let output = echo(&input, 1, super::MAX_FEEDBACK, 1.0);
        assert_close(&output, &[1.0, 0.95, 0.9025, 0.857375, 0.81450625], 1e-5);
    }

    #[test]
    fn feedback_above_max_is_clamped_to_max() {
        let mut input = vec![0.0; 5];
        input[0] = 1.0;
        let output = echo(&input, 1, 0.96, 1.0);
        assert_close(&output, &[1.0, 0.95, 0.9025, 0.857375, 0.81450625], 1e-5);
    }

    #[test]
    fn feedback_nan_is_zero() {
        let mut input = vec![0.0; 5];
        input[0] = 1.0;
        assert_eq!(echo(&input, 2, f32::NAN, 1.0), vec![1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn zero_delay_is_identity() {
        let input = [0.3, -0.2, 0.9, 0.0, 0.5];
        assert_eq!(echo(&input, 0, 2.0, 2.0), input);
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(echo(&[], 3, 0.5, 0.5).is_empty());
    }

    #[test]
    fn tail_is_truncated() {
        let mut input = vec![0.0; 3];
        input[0] = 1.0;
        assert_eq!(echo(&input, 4, 0.5, 1.0), vec![1.0, 0.0, 0.0]);
    }

    #[test]
    fn mix_is_clamped() {
        let mut input = vec![0.0; 12];
        input[0] = 1.0;
        let full = echo(&input, 3, 0.5, 1.0);
        let above = echo(&input, 3, 0.5, 2.0);
        let below = echo(&input, 3, 0.5, -1.0);
        assert_eq!(above, full);
        assert_eq!(below, input);
    }
}
