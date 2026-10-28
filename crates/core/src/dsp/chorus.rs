//! A bounded, mono chorus made from several sine-modulated delay voices.

/// The lower and upper delay limits, in milliseconds.
pub const MIN_DELAY_MS: f32 = 10.0;
pub const MAX_DELAY_MS: f32 = 30.0;

const BASE_DELAY_MS: f32 = 20.0;
/// Relative LFO-rate spread between neighbouring voices, from the B065 "detuned" intent.
const VOICE_DETUNE: f32 = 0.04;

fn voice_rate_factor(voice: usize, voices: usize) -> f32 {
    1.0 + VOICE_DETUNE * (voice as f32 - (voices as f32 - 1.0) / 2.0)
}

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
/// `voices` is clamped to 2..=3 (0 and 1 become 2; 4 and above become 3).
/// A non-finite or non-positive `rate_hz` gives a fixed 20 ms delay. Invalid
/// `depth` and `mix` values are clamped, with NaN treated as zero.
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
    let voice_count_f = voice_count as f32;

    input
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let mut wet = 0.0;
            for voice in 0..voice_count {
                let phase = voice as f32 / voice_count_f * std::f32::consts::TAU;
                let period = if rate_hz > 0.0 { sample_rate / (rate_hz * voice_rate_factor(voice, voice_count)) } else { f32::INFINITY };
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
    use super::{MAX_DELAY_MS, MIN_DELAY_MS, chorus, voice_rate_factor};

    #[test]
    fn voice_rate_factors_are_detuned() {
        assert!((voice_rate_factor(0, 2) - 0.98).abs() <= 1e-6);
        assert!((voice_rate_factor(1, 2) - 1.02).abs() <= 1e-6);
        assert!((voice_rate_factor(0, 3) - 0.96).abs() <= 1e-6);
        assert!((voice_rate_factor(1, 3) - 1.0).abs() <= 1e-6);
        assert!((voice_rate_factor(2, 3) - 1.04).abs() <= 1e-6);
    }

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
        let sample_rate = 1000.0_f32;
        let voices = 2;
        let rate_hz = 50.0_f32;
        let depth = 1.0_f32;
        let mix = 1.0_f32;
        let output = chorus(&input, sample_rate, voices, rate_hz, depth, mix);
        let period_samples = sample_rate / rate_hz;
        assert_eq!(period_samples, 20.0);
        let base_delay = 20.0_f32 * sample_rate / 1000.0;
        let modulation = 10.0_f32 * sample_rate / 1000.0 * depth;
        let expected: Vec<f32> = (0..input.len())
            .map(|index| {
                let mut wet = 0.0_f32;
                for voice in 0..voices {
                    let factor = voice_rate_factor(voice, voices);
                    let phase = voice as f32 / voices as f32 * std::f32::consts::TAU;
                    let lfo = (index as f32 * rate_hz * factor / sample_rate * std::f32::consts::TAU + phase).sin();
                    let delay = (base_delay + modulation * lfo).clamp(MIN_DELAY_MS * sample_rate / 1000.0, MAX_DELAY_MS * sample_rate / 1000.0);
                    let position = index as f32 - delay;
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
                    wet += first + fraction * (second - first);
                }
                (1.0 - mix) * input[index] + mix * (wet / voices as f32)
            })
            .collect();
        let expected_peak = expected.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
        let actual_peak = output.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
        assert_eq!(actual_peak, expected_peak);
        for (actual, expected) in output.iter().zip(expected.iter()) {
            assert!((actual - expected).abs() <= 1e-6);
        }
    }

    #[test]
    fn voices_have_expected_impulse_positions() {
        let mut input = vec![0.0; 40];
        input[0] = 1.0;
        let sample_rate = 1000.0_f32;
        let rate_hz = 25.0_f32;
        let depth = 0.5_f32;
        let mix = 1.0_f32;
        for voices in [2, 3] {
            let output = chorus(&input, sample_rate, voices, rate_hz, depth, mix);
            let base_delay = 20.0_f32 * sample_rate / 1000.0;
            let modulation = 10.0_f32 * sample_rate / 1000.0 * depth;
            let expected: Vec<f32> = (0..input.len())
                .map(|index| {
                    let mut wet = 0.0_f32;
                    for voice in 0..voices {
                        let factor = voice_rate_factor(voice, voices);
                        let phase = voice as f32 / voices as f32 * std::f32::consts::TAU;
                        let lfo = (index as f32 * rate_hz * factor / sample_rate * std::f32::consts::TAU + phase).sin();
                        let delay = (base_delay + modulation * lfo).clamp(MIN_DELAY_MS * sample_rate / 1000.0, MAX_DELAY_MS * sample_rate / 1000.0);
                        let position = index as f32 - delay;
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
                        wet += first + fraction * (second - first);
                    }
                    (1.0 - mix) * input[index] + mix * (wet / voices as f32)
                })
                .collect();
            let expected_peak = expected.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
            let actual_peak = output.iter().enumerate().max_by(|(_, left), (_, right)| left.total_cmp(right)).map(|(index, _)| index);
            assert_eq!(actual_peak, expected_peak);
            for (actual, expected) in output.iter().zip(expected.iter()) {
                assert!((actual - expected).abs() <= 1e-6);
            }
        }
    }

    #[test]
    fn independent_peak_two_voices_50hz_full_depth() {
        let mut input = vec![0.0_f32; 80];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 50.0, 1.0, 1.0);
        let peak = output.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(index, _)| index);
        assert_eq!(peak, Some(13));
        assert!((output[13] - 0.20819).abs() <= 1e-4);
    }

    #[test]
    fn independent_peak_two_voices_25hz_half_depth() {
        let mut input = vec![0.0_f32; 40];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 25.0, 0.5, 1.0);
        let peak = output.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(index, _)| index);
        assert_eq!(peak, Some(20));
        assert!((output[20] - 0.686047).abs() <= 1e-4);
    }

    #[test]
    fn independent_peak_three_voices_25hz_half_depth() {
        let mut input = vec![0.0_f32; 40];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 3, 25.0, 0.5, 1.0);
        let peak = output.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(index, _)| index);
        assert_eq!(peak, Some(15));
        assert!((output[15] - 0.276543).abs() <= 1e-4);
    }

    #[test]
    fn detuned_voices_have_different_late_impulse_positions() {
        let sample_rate = 1000.0;
        let rate_hz = 3.0;
        let impulse_index = 1000;
        let mut input = vec![0.0; 1060];
        input[impulse_index] = 1.0;
        let output = chorus(&input, sample_rate, 2, rate_hz, 1.0, 1.0);

        let expected_positions: Vec<usize> = (0..2)
            .map(|voice| {
                let factor = voice_rate_factor(voice, 2);
                let phase = voice as f32 / 2.0 * std::f32::consts::TAU;
                let mut position = impulse_index as f32 + 20.0;
                for _ in 0..8 {
                    let delay = 20.0 + 10.0 * (phase + std::f32::consts::TAU * position * rate_hz * factor / sample_rate).sin();
                    position = impulse_index as f32 + delay;
                }
                position.round() as usize
            })
            .collect();

        assert!(expected_positions[0].abs_diff(expected_positions[1]) >= 5);
        assert!(output[expected_positions[0]] > 0.35);
        assert!(output[expected_positions[1]] > 0.35);
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

    #[test]
    fn invalid_sample_rates_return_input() {
        let input = [0.3_f32, -0.2, 0.9];
        assert_eq!(chorus(&input, -1.0, 2, 1.0, 1.0, 1.0), input);
        assert_eq!(chorus(&input, f32::NAN, 2, 1.0, 1.0, 1.0), input);
        assert_eq!(chorus(&input, f32::INFINITY, 2, 1.0, 1.0, 1.0), input);
    }

    #[test]
    fn nan_mix_returns_input_exactly() {
        let input = [0.3_f32, -0.2, 0.9];
        assert_eq!(chorus(&input, 1000.0, 2, 1.0, 1.0, f32::NAN), input);
    }

    #[test]
    fn mix_above_one_is_clamped() {
        let mut input = vec![0.0_f32; 26];
        input[0] = 1.0;
        let output = chorus(&input, 1000.0, 2, 0.0, 0.0, 2.0);
        assert_eq!(output[20], 1.0);
        assert!(output.iter().enumerate().all(|(i, &v)| i == 20 || v == 0.0));
    }

    #[test]
    fn invalid_rate_and_nan_depth_use_fixed_delay() {
        let input: Vec<f32> = (0..30).map(|i| i as f32 * 0.1).collect();
        let fixed = chorus(&input, 1000.0, 2, 0.0, 0.0, 1.0);
        assert_eq!(chorus(&input, 1000.0, 2, -2.0, 1.0, 1.0), fixed);
        assert_eq!(chorus(&input, 1000.0, 2, f32::INFINITY, 1.0, 1.0), fixed);
        assert_eq!(chorus(&input, 1000.0, 2, 1.0, f32::NAN, 1.0), fixed);
    }

    #[test]
    fn voice_count_is_clamped_at_both_boundaries() {
        let mut input = vec![0.0_f32; 40];
        input[0] = 1.0;
        assert!((chorus(&input, 1000.0, 0, 25.0, 0.5, 1.0)[20] - 0.6860474).abs() <= 1e-6);
        assert!((chorus(&input, 1000.0, 4, 25.0, 0.5, 1.0)[15] - 0.27654305).abs() <= 1e-6);
    }
}
