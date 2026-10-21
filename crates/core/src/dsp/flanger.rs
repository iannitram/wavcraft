//! A mono flanger with a short sine-modulated delay and feedback. Delay stays within 0.1..10 ms, depth is 0..1,
//! feedback is 0..=0.95, rate is positive Hz, and mix is a 0..1 dry/wet blend; NaN values for depth, feedback,
//! and mix are treated as zero, while unusable rates disable modulation. Stereo and phase inversion deferred.

/// The minimum and maximum modulated delay limits, in milliseconds.
pub const MIN_DELAY_MS: f32 = 0.1;
pub const MAX_DELAY_MS: f32 = 10.0;

const BASE_DELAY_MS: f32 = 5.0;
const MAX_FEEDBACK: f32 = 0.95;

fn sample_at(input: &[f32], position: f32) -> f32 {
    if !position.is_finite() {
        return 0.0;
    }
    let lower = position.floor() as i64;
    let fraction = position - lower as f32;
    let first = if lower >= 0 {
        match input.get(lower as usize) {
            Some(&sample) => sample,
            None => 0.0,
        }
    } else {
        0.0
    };
    let upper = lower.saturating_add(1);
    let second = if upper >= 0 {
        match input.get(upper as usize) {
            Some(&sample) => sample,
            None => 0.0,
        }
    } else {
        0.0
    };
    first + fraction * (second - first)
}

/// Applies a mono sine-modulated short-delay flanger.
///
/// `sample_rate` is the sample rate in Hz; an invalid or non-positive value returns the input unchanged.
/// `rate_hz` is the LFO rate, with non-finite or non-positive values disabling modulation.
/// `depth` is clamped to 0..=1 (NaN becomes zero), from the fixed 5 ms base delay toward the delay limits.
/// `feedback` is clamped to 0..=0.95 (NaN becomes zero) and is taken from the wet line.
/// `mix` is clamped to 0..=1 (NaN becomes zero), blending dry and wet output; zero returns the input exactly.
pub fn flanger(input: &[f32], sample_rate: f32, rate_hz: f32, depth: f32, feedback: f32, mix: f32) -> Vec<f32> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return input.to_vec();
    }
    let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };
    if input.is_empty() || mix == 0.0 {
        return input.to_vec();
    }
    let rate_hz = if rate_hz.is_finite() && rate_hz > 0.0 { rate_hz } else { 0.0 };
    let depth = if depth.is_nan() { 0.0 } else { depth.clamp(0.0, 1.0) };
    let feedback = if feedback.is_nan() { 0.0 } else { feedback.clamp(0.0, MAX_FEEDBACK) };
    let base_delay = BASE_DELAY_MS * sample_rate / 1000.0;
    let modulation = (MAX_DELAY_MS - BASE_DELAY_MS) * sample_rate / 1000.0 * depth;
    let minimum_delay = (MIN_DELAY_MS * sample_rate / 1000.0).max(1.0);
    let maximum_delay = (MAX_DELAY_MS * sample_rate / 1000.0).max(1.0);
    let mut wet_line = vec![0.0; input.len()];
    let mut output = vec![0.0; input.len()];

    for (index, &dry) in input.iter().enumerate() {
        let lfo = if rate_hz > 0.0 { (std::f32::consts::TAU * index as f32 * rate_hz / sample_rate).sin() } else { 0.0 };
        let unclamped_delay = base_delay + modulation * lfo;
        let delay = if unclamped_delay.is_finite() { unclamped_delay.clamp(minimum_delay, maximum_delay).max(1.0) } else { maximum_delay.max(1.0) };
        let position = index as f32 - delay;
        let wet = sample_at(input, position) + feedback * sample_at(&wet_line, position);
        if let Some(slot) = wet_line.get_mut(index) {
            *slot = wet;
        }
        if let Some(slot) = output.get_mut(index) {
            *slot = (1.0 - mix) * dry + mix * wet;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::flanger;
    use std::f32::consts::PI;

    #[test]
    fn fixed_delay_feedback_impulse() {
        let mut input = vec![0.0; 30];
        input[0] = 1.0;
        let output = flanger(&input, 1000.0, 1.0, 0.0, 0.5, 0.5);
        assert_eq!(output[0], 0.5);
        assert_eq!(output[5], 0.5);
        assert_eq!(output[10], 0.25);
        assert_eq!(output[15], 0.125);
        assert_eq!(output[20], 0.0625);
        assert_eq!(output[25], 0.03125);
        assert!(output.iter().enumerate().all(|(index, &sample)| { matches!(index, 0 | 5 | 10 | 15 | 20 | 25) || sample == 0.0 }));
    }

    #[test]
    fn mix_zero_and_invalid_sample_rate_are_identity() {
        let input = [0.3, -0.2, 0.9];
        assert_eq!(flanger(&input, 1000.0, 1.0, 1.0, 0.5, 0.0), input);
        assert_eq!(flanger(&input, 0.0, 1.0, 1.0, 0.5, 1.0), input);
        assert_eq!(flanger(&[], 1000.0, 1.0, 1.0, 0.5, 1.0), Vec::<f32>::new());
    }

    #[test]
    fn comb_notch_at_100_hz() {
        let input: Vec<f32> = (0..41).map(|index| (2.0 * PI * index as f32 / 10.0).cos()).collect();
        let output = flanger(&input, 1000.0, 1.0, 0.0, 0.0, 0.5);
        for sample in output.iter().skip(5) {
            assert!(sample.abs() <= 1e-6);
        }
    }

    #[test]
    fn feedback_clamps_to_0_95() {
        let mut input = vec![0.0; 60];
        input[0] = 1.0;
        let output = flanger(&input, 1000.0, 1.0, 0.0, 2.0, 1.0);
        assert!((output[5] - 1.0).abs() <= 1e-6);
        assert!((output[10] - 0.95).abs() <= 1e-6);
        assert!((output[15] - 0.9025).abs() <= 1e-6);
        assert!((output[20] - 0.857375).abs() <= 1e-6);
        assert!((output[25] - 0.814506).abs() <= 1e-6);
        let output = flanger(&input, 1000.0, 1.0, 0.0, -1.0, 1.0);
        assert_eq!(output[5], 1.0);
        assert!(output.iter().enumerate().all(|(index, &sample)| index == 5 || sample == 0.0));
    }

    #[test]
    fn fractional_delay_reads_out_of_range_as_zero() {
        let mut input = vec![0.0; 30];
        input[0] = 1.0;
        let output = flanger(&input, 1500.0, 1.0, 0.0, 0.0, 1.0);
        assert_eq!(output[7], 0.5);
        assert_eq!(output[8], 0.5);
        assert!(output.iter().enumerate().all(|(index, &sample)| (index == 7 || index == 8) || sample == 0.0));
    }

    #[test]
    fn bounded_feedback_output() {
        let input: Vec<f32> = (0..20000).map(|index| (index as f32 * 0.01).sin()).collect();
        let output = flanger(&input, 48000.0, 0.5, 1.0, 0.95, 1.0);
        assert!(output.iter().all(|sample| sample.is_finite() && sample.abs() <= 20.0));
    }

    #[test]
    fn invalid_rate_and_nan_depth_use_fixed_delay() {
        let input: Vec<f32> = (0..30).map(|index| index as f32 * 0.1).collect();
        let fixed = flanger(&input, 1000.0, 1.0, 0.0, 0.25, 1.0);
        assert_eq!(flanger(&input, 1000.0, 0.0, 1.0, 0.25, 1.0), fixed);
        assert_eq!(flanger(&input, 1000.0, -2.0, 1.0, 0.25, 1.0), fixed);
        assert_eq!(flanger(&input, 1000.0, f32::NAN, 1.0, 0.25, 1.0), fixed);
        assert_eq!(flanger(&input, 1000.0, 1.0, f32::NAN, 0.25, 1.0), fixed);
        assert_eq!(flanger(&input, 1000.0, 1.0, 0.0, 0.25, f32::NAN), input);
    }

    #[test]
    fn invalid_sample_rate_returns_input() {
        let input = [0.3, -0.2, 0.9];
        assert_eq!(flanger(&input, 0.0, 1.0, 1.0, 0.5, 1.0), input);
        assert_eq!(flanger(&input, -1.0, 1.0, 1.0, 0.5, 1.0), input);
        assert_eq!(flanger(&input, f32::NAN, 1.0, 1.0, 0.5, 1.0), input);
        assert_eq!(flanger(&input, f32::INFINITY, 1.0, 1.0, 0.5, 1.0), input);
    }

    #[test]
    fn huge_values_do_not_panic() {
        let output = flanger(&[1.0, -1.0, 0.5], f32::MAX, f32::MAX, 1.0, 0.95, 1.0);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn mix_zero_returns_input_exactly() {
        let input = [0.5, 0.25, -0.75];
        assert_eq!(flanger(&input, 1000.0, 1.0, 1.0, 0.5, 0.0), input);
        assert_eq!(flanger(&input, 1000.0, 1.0, 1.0, 0.5, f32::NAN), input);
        assert_eq!(flanger(&[], 1000.0, 1.0, 1.0, 0.5, 0.0), Vec::<f32>::new());
    }
}
