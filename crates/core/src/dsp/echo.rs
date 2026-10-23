/// Maximum feedback gain (roadmap row B043: feedback 0..0.95).
pub const MAX_FEEDBACK: f32 = 0.95;

/// Maximum tail length for `echo_with_tail`: 60 seconds at 48 kHz.
pub const MAX_TAIL_SAMPLES: usize = 2_880_000;

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

/// Converts a delay in milliseconds to a whole number of samples.
///
/// Rounds to the nearest sample (halves round up), computing in f64. Returns 0 when `ms` or
/// `sample_rate` is non-finite, `ms` is negative, or `sample_rate` is not positive; a result
/// above `usize::MAX` saturates to `usize::MAX`.
pub fn ms_to_samples(ms: f32, sample_rate: f32) -> usize {
    if !ms.is_finite() || !sample_rate.is_finite() || ms < 0.0 || sample_rate <= 0.0 {
        return 0;
    }
    let samples = (f64::from(ms) * f64::from(sample_rate) / 1000.0).round();
    samples as usize
}

/// Applies the echo and appends `tail_samples` of silence first, so the last
/// repeats are not cut. Output length is `input.len() + min(tail_samples, MAX_TAIL_SAMPLES)`.
pub fn echo_with_tail(input: &[f32], delay_samples: usize, feedback: f32, mix: f32, tail_samples: usize) -> Vec<f32> {
    let clamped_tail = tail_samples.min(MAX_TAIL_SAMPLES);
    let mut padded = Vec::with_capacity(input.len().saturating_add(clamped_tail));
    padded.extend_from_slice(input);
    padded.resize(input.len().saturating_add(clamped_tail), 0.0);
    echo(&padded, delay_samples, feedback, mix)
}

/// Delay and feedback for one channel of the stereo echo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoChannel {
    pub delay_samples: usize,
    pub feedback: f32,
}

/// Applies the echo to interleaved stereo samples (L,R,L,R,...).
///
/// With `lock_left_right` the `left` settings drive both channels and `right`
/// is ignored; otherwise each channel uses its own settings. For odd-length
/// input the final unpaired sample is treated as a left sample (the right
/// channel simply has one fewer sample). Output length equals input length.
pub fn echo_stereo_interleaved(input: &[f32], left: EchoChannel, right: EchoChannel, mix: f32, lock_left_right: bool) -> Vec<f32> {
    let right = if lock_left_right { left } else { right };
    let left_in: Vec<f32> = input.iter().step_by(2).copied().collect();
    let right_in: Vec<f32> = input.iter().skip(1).step_by(2).copied().collect();
    let left_out = echo(&left_in, left.delay_samples, left.feedback, mix);
    let right_out = echo(&right_in, right.delay_samples, right.feedback, mix);
    let mut output = Vec::with_capacity(input.len());
    for (index, &l) in left_out.iter().enumerate() {
        output.push(l);
        if let Some(&r) = right_out.get(index) {
            output.push(r);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{EchoChannel, echo, echo_stereo_interleaved, echo_with_tail, ms_to_samples};

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

    #[test]
    fn ms_to_samples_exact() {
        assert_eq!(ms_to_samples(5.0, 1000.0), 5);
    }

    #[test]
    fn ms_to_samples_rounds_down_below_half() {
        assert_eq!(ms_to_samples(0.4, 1000.0), 0);
    }

    #[test]
    fn ms_to_samples_rounds_half_up() {
        assert_eq!(ms_to_samples(0.5, 1000.0), 1);
    }

    #[test]
    fn ms_to_samples_negative_is_zero() {
        assert_eq!(ms_to_samples(-1.0, 48000.0), 0);
    }

    #[test]
    fn ms_to_samples_nan_is_zero() {
        assert_eq!(ms_to_samples(f32::NAN, 48000.0), 0);
        assert_eq!(ms_to_samples(5.0, 0.0), 0);
    }

    #[test]
    fn ms_delay_impulse_pattern() {
        let mut input = vec![0.0; 16];
        input[0] = 1.0;
        let output = echo(&input, ms_to_samples(5.0, 1000.0), 0.5, 0.5);
        let mut expected = vec![0.0; 16];
        expected[0] = 1.0;
        expected[5] = 0.25;
        expected[10] = 0.125;
        expected[15] = 0.0625;
        assert_close(&output, &expected, 1e-6);
    }

    #[test]
    fn stereo_locked_matches_mono_per_channel() {
        let input = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let channel = EchoChannel { delay_samples: 2, feedback: 0.5 };
        let ignored = EchoChannel { delay_samples: 1, feedback: 0.9 };
        let output = echo_stereo_interleaved(&input, channel, ignored, 1.0, true);
        assert_close(&output, &[1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0, 0.5, 0.25, 0.0], 1e-6);
    }

    #[test]
    fn stereo_unlocked_is_independent() {
        let input = [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let left = EchoChannel { delay_samples: 1, feedback: 0.5 };
        let right = EchoChannel { delay_samples: 2, feedback: 0.25 };
        let output = echo_stereo_interleaved(&input, left, right, 1.0, false);
        assert_close(&output, &[1.0, 1.0, 0.5, 0.0, 0.25, 0.25, 0.125, 0.0], 1e-6);
    }

    #[test]
    fn stereo_odd_length_last_sample_is_left() {
        let input = [1.0, 2.0, 0.0, 4.0, 0.5];
        let channel = EchoChannel { delay_samples: 1, feedback: 0.5 };
        let output = echo_stereo_interleaved(&input, channel, channel, 1.0, true);
        assert_close(&output, &[1.0, 2.0, 0.5, 5.0, 0.75], 1e-6);
    }

    #[test]
    fn stereo_empty_is_empty() {
        let channel = EchoChannel { delay_samples: 1, feedback: 0.5 };
        assert!(echo_stereo_interleaved(&[], channel, channel, 1.0, false).is_empty());
    }

    #[test]
    fn tail_keeps_last_echoes() {
        let mut input = vec![0.0; 4];
        input[0] = 1.0;
        let output = echo_with_tail(&input, 5, 0.5, 1.0, 15);
        assert_eq!(output.len(), 19);
        assert_eq!(output[0], 1.0);
        assert_eq!(output[5], 0.5);
        assert_eq!(output[10], 0.25);
        assert_eq!(output[15], 0.125);
    }

    #[test]
    fn zero_tail_matches_echo() {
        let input = [0.3, -0.2, 0.9, 0.0, 0.5];
        assert_eq!(echo_with_tail(&input, 2, 0.7, 0.6, 0), echo(&input, 2, 0.7, 0.6));
    }

    #[test]
    fn ms_to_samples_huge_product_saturates() {
        assert_eq!(ms_to_samples(1e30, 1e30), usize::MAX);
    }

    #[test]
    fn ms_to_samples_f32_overflowing_product_saturates() {
        assert_eq!(ms_to_samples(2e19, 2e19), usize::MAX);
    }

    #[test]
    fn ms_to_samples_max_ms_saturates() {
        assert_eq!(ms_to_samples(f32::MAX, 48000.0), usize::MAX);
    }

    #[test]
    fn ms_to_samples_one_second_at_48k() {
        assert_eq!(ms_to_samples(1000.0, 48000.0), 48000);
    }

    #[test]
    fn ms_to_samples_non_finite_inputs_are_zero() {
        assert_eq!(ms_to_samples(5.0, f32::INFINITY), 0);
        assert_eq!(ms_to_samples(5.0, f32::NEG_INFINITY), 0);
        assert_eq!(ms_to_samples(5.0, f32::NAN), 0);
        assert_eq!(ms_to_samples(f32::INFINITY, 48000.0), 0);
        assert_eq!(ms_to_samples(f32::NAN, 48000.0), 0);
    }

    #[test]
    fn tail_above_cap_is_clamped_without_abort() {
        let output = echo_with_tail(&[0.5], 1, 0.5, 0.5, usize::MAX);
        assert_eq!(output.len(), 2_880_001);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn tail_at_cap_is_not_changed() {
        let output = echo_with_tail(&[0.5], 1, 0.5, 0.5, 2_880_000);
        assert_eq!(output.len(), 2_880_001);
    }

    #[test]
    fn tail_below_cap_is_exact() {
        let output = echo_with_tail(&[0.5, 0.25], 1, 0.5, 0.5, 10);
        assert_eq!(output.len(), 12);
    }
}
