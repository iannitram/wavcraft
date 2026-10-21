//! Phase correlation for stereo signals is implemented here; a running meter
//! struct and other streaming APIs are deferred.

/// Returns the normalized cross-correlation over the common prefix of `left`
/// and `right`. Empty input or zero energy on either channel returns `0.0`.
pub fn phase_correlation(left: &[f32], right: &[f32]) -> f32 {
    let (cross, left_energy, right_energy) =
        left.iter().zip(right.iter()).fold((0.0_f64, 0.0_f64, 0.0_f64), |(cross, left_energy, right_energy), (&l, &r)| {
            let left_value = f64::from(l);
            let right_value = f64::from(r);
            (cross + left_value * right_value, left_energy + left_value * left_value, right_energy + right_value * right_value)
        });

    let denominator = (left_energy * right_energy).sqrt();
    if denominator == 0.0 { 0.0 } else { (cross / denominator) as f32 }
}

/// Returns one correlation for each full `window` in the common prefix,
/// advancing by `hop` samples. Partial windows are omitted. A zero `window`
/// returns an empty vector; a zero `hop` is treated as a hop of one sample.
pub fn phase_correlation_windowed(left: &[f32], right: &[f32], window: usize, hop: usize) -> Vec<f32> {
    if window == 0 {
        return Vec::new();
    }

    let common_len = left.len().min(right.len());
    if window > common_len {
        return Vec::new();
    }
    let step = hop.max(1);
    let mut correlations = Vec::new();
    let mut start = 0;
    while start <= common_len.saturating_sub(window) {
        let left_window = left.get(start..start + window);
        let right_window = right.get(start..start + window);
        if let (Some(left_window), Some(right_window)) = (left_window, right_window) {
            correlations.push(phase_correlation(left_window, right_window));
        }
        start = start.saturating_add(step);
    }
    correlations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(actual: f32, expected: f32, tolerance: f32) {
        assert!((actual - expected).abs() <= tolerance, "{actual} != {expected}");
    }

    fn sine_samples(length: usize, phase: f32) -> Vec<f32> {
        (0..length)
            .map(|index| {
                let angle = 2.0 * std::f32::consts::PI * index as f32 / 100.0 + phase;
                angle.sin()
            })
            .collect()
    }

    fn noise_samples(length: usize, mut state: u64) -> Vec<f32> {
        (0..length)
            .map(|_| {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let high_bits = (state >> 32) as u32;
                high_bits as f32 / 4_294_967_296.0 * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn identical_signals_correlate_positively() {
        let signal = sine_samples(1000, 0.0);
        approx(phase_correlation(&signal, &signal), 1.0, 1e-6);
    }

    #[test]
    fn inverted_signals_correlate_negatively() {
        let signal = sine_samples(1000, 0.0);
        let inverted: Vec<f32> = signal.iter().map(|sample| -*sample).collect();
        approx(phase_correlation(&signal, &inverted), -1.0, 1e-6);
    }

    #[test]
    fn quarter_period_shift_is_near_zero() {
        let sine = sine_samples(1000, 0.0);
        let cosine = sine_samples(1000, std::f32::consts::FRAC_PI_2);
        approx(phase_correlation(&sine, &cosine), 0.0, 1e-3);
    }

    #[test]
    fn different_deterministic_noise_is_near_zero() {
        let first = noise_samples(10_000, 1);
        let second = noise_samples(10_000, 2);
        assert!(phase_correlation(&first, &second).abs() < 0.05);
    }

    #[test]
    fn silence_returns_zero() {
        let silence = vec![0.0; 100];
        let signal = sine_samples(100, 0.0);
        assert_eq!(phase_correlation(&silence, &signal), 0.0);
        assert_eq!(phase_correlation(&silence, &silence), 0.0);
    }

    #[test]
    fn mismatched_lengths_use_common_prefix() {
        let left = [1.0, 2.0, 3.0];
        let right = [2.0, 4.0, 6.0, 99.0];
        assert_eq!(phase_correlation(&left, &right), phase_correlation(&left, &right[..3]));
    }

    #[test]
    fn empty_inputs_return_zero() {
        assert_eq!(phase_correlation(&[], &[]), 0.0);
        assert_eq!(phase_correlation(&[1.0], &[]), 0.0);
    }

    #[test]
    fn windows_correlate_each_full_window() {
        let first = sine_samples(100, 0.0);
        let second = sine_samples(100, 0.0);
        let mut left = first.clone();
        left.extend_from_slice(&second);
        let mut right = first;
        right.extend(second.iter().map(|sample| -*sample));
        let result = phase_correlation_windowed(&left, &right, 100, 100);
        assert_eq!(result.len(), 2);
        let values: Vec<f32> = result.to_vec();
        approx(values[0], 1.0, 1e-6);
        approx(values[1], -1.0, 1e-6);
    }

    #[test]
    fn zero_window_is_empty_and_zero_hop_is_one() {
        assert!(phase_correlation_windowed(&[1.0], &[1.0], 0, 1).is_empty());
        let result = phase_correlation_windowed(&[1.0, 1.0], &[1.0, 1.0], 1, 0);
        assert_eq!(result, vec![1.0, 1.0]);
    }

    #[test]
    fn windows_use_common_prefix_and_silence_is_zero() {
        let left = [1.0, 1.0, 0.0, 0.0, 2.0, 2.0];
        let right = [1.0, 1.0, 0.0, 0.0];
        let result = phase_correlation_windowed(&left, &right, 2, 2);
        assert_eq!(result.len(), 2);
        let values: Vec<f32> = result.to_vec();
        approx(values[0], 1.0, 1e-6);
        assert_eq!(values[1], 0.0);
    }
}
