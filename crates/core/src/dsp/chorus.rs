//! A bounded, mono chorus made from several sine-modulated delay voices.

/// The lower and upper delay limits, in milliseconds.
pub const MIN_DELAY_MS: f32 = 10.0;
pub const MAX_DELAY_MS: f32 = 30.0;

const BASE_DELAY_MS: f32 = 20.0;

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

/// Applies a 2- or 3-voice sine-LFO chorus to mono samples.
///
/// `depth` is a 0..1 amount of the 10..30 ms delay range: zero is the fixed
/// 20 ms base delay and one reaches the two range limits. `mix` is the dry/wet
/// blend, where zero is exactly the input and one is fully wet.
/// If `sample_rate` is non-finite or not positive, the dry input is returned
/// unchanged.
pub fn chorus(input: &[f32], sample_rate: f32, voices: usize, rate_hz: f32, depth: f32, mix: f32) -> Vec<f32> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return input.to_vec();
    }
    let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };
    if mix == 0.0 || input.is_empty() {
        return input.to_vec();
    }
    let rate_hz = if rate_hz.is_finite() && rate_hz > 0.0 { rate_hz } else { 0.0 };
    let depth = if depth.is_nan() { 0.0 } else { depth.clamp(0.0, 1.0) };
    let voice_count = voices.clamp(2, 3);
    let base_delay = BASE_DELAY_MS * sample_rate / 1000.0;
    let modulation = (MAX_DELAY_MS - BASE_DELAY_MS) * sample_rate / 1000.0 * depth;
    let period = if rate_hz > 0.0 { sample_rate / rate_hz } else { f32::INFINITY };
    let voice_count_f = voice_count as f32;

    input
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let mut wet = 0.0;
            for voice in 0..voice_count {
                let phase = voice as f32 / voice_count_f * std::f32::consts::TAU;
                let lfo = if period.is_finite() { ((index as f32 / period) * std::f32::consts::TAU + phase).sin() } else { 0.0 };
                let delay = (base_delay + modulation * lfo).clamp(MIN_DELAY_MS * sample_rate / 1000.0, MAX_DELAY_MS * sample_rate / 1000.0);
                wet += sample_at(input, index as f32 - delay);
            }
            let wet = wet / voice_count_f;
            (1.0 - mix) * sample + mix * wet
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{chorus, MAX_DELAY_MS, MIN_DELAY_MS};

    #[test]
    fn mix_zero_is_identity() {
        let input = [0.3, -0.2, 0.9];
        assert_eq!(chorus(&input, 1000.0, 2, 2.0, 1.0, 0.0), input);
    }

    #[test]
    fn depth_zero_has_fixed_base_delay() {
        let mut input = vec![0.0; 30];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 2.0, 0.0, 1.0);
        assert_eq!(output[20], 1.0);
        assert_eq!(output[19], 0.0);
        assert_eq!(output[21], 0.0);
    }

    #[test]
    fn lfo_period_and_peak_delay_are_predictable() {
        let mut input = vec![0.0; 80];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 50.0, 1.0, 1.0);
        let period_samples = 1000.0 / 50.0;
        assert_eq!(period_samples, 20.0);
        let peak = output.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
        assert_eq!(peak, Some(20));
        assert!((output[20] - 1.0).abs() <= 1e-6);
    }

    #[test]
    fn voices_have_expected_impulse_positions() {
        let mut input = vec![0.0; 40];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 25.0, 0.5, 1.0);
        let peak = output.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
        assert_eq!(peak, Some(20));
        assert!((output[20] - 1.0).abs() <= 1e-6);

        let output = chorus(&input, 1000.0, 3, 25.0, 0.5, 1.0);
        let peak = output.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
        assert_eq!(peak, Some(20));
        assert!((output[20] - (1.0 / 3.0)).abs() <= 1e-6);
    }

    #[test]
    fn output_stays_bounded() {
        let input: Vec<f32> = (0..20_000).map(|n| (n as f32 * 0.07).sin()).collect();
        let output = chorus(&input, 48_000.0, 3, 0.8, 1.0, 1.0);
        assert!(output.iter().all(|sample| sample.is_finite() && sample.abs() <= 1.0));
    }

    #[test]
    fn empty_zero_and_huge_delays_do_not_panic() {
        assert!(chorus(&[], 48_000.0, 2, 1.0, 0.0, 1.0).is_empty());
        let input = [1.0, 0.0];
        assert_eq!(chorus(&input, 0.0, 2, 0.0, 0.0, 1.0), input);
        assert!(chorus(&input, f32::MAX, 3, f32::MAX, 1.0, 1.0).iter().all(|x| x.is_finite()));
    }

    #[test]
    fn fractional_delay_uses_linear_interpolation() {
        let input = [0.0, 0.0, 1.0, 0.0, 0.0];
        let output = chorus(&input, 75.0, 2, 1.0, 0.0, 1.0);
        assert_eq!(output[3], 0.5);
        assert_eq!(output[4], 0.5);
        assert_eq!(MIN_DELAY_MS, 10.0);
        assert_eq!(MAX_DELAY_MS, 30.0);
    }
}
