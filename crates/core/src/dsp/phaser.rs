//! A mono phaser with swept cascaded all-pass stages and feedback.
//!
//! Stereo processing, LFO waveforms other than sine, and any other unnamed
//! option are deferred.

/// Applies a mono phaser with 1..=8 swept all-pass stages, sine-LFO rate and
/// depth, feedback, and dry/wet mix. Stereo processing, LFO waveforms other
/// than sine, and any other unnamed option are deferred.
pub fn phaser(input: &[f32], sample_rate: f32, stages: usize, rate_hz: f32, depth: f32, feedback: f32, mix: f32) -> Vec<f32> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return input.to_vec();
    }
    let mix = if mix.is_nan() { 0.0 } else { mix.clamp(0.0, 1.0) };
    if mix == 0.0 || input.is_empty() {
        return input.to_vec();
    }
    let stage_count = stages.clamp(1, 8);
    let rate_hz = if rate_hz.is_finite() && rate_hz > 0.0 { rate_hz } else { 0.0 };
    let depth = if depth.is_nan() { 0.0 } else { depth.clamp(0.0, 1.0) };
    let feedback = if feedback.is_nan() { 0.0 } else { feedback.clamp(0.0, 0.95) };
    let mut previous_input = vec![0.0; stage_count];
    let mut previous_output = vec![0.0; stage_count];
    let mut previous_wet = 0.0;
    let mut output = Vec::with_capacity(input.len());

    for (index, &sample) in input.iter().enumerate() {
        let lfo = if rate_hz > 0.0 { (2.0 * std::f32::consts::PI * rate_hz * index as f32 / sample_rate).sin() } else { 0.0 };
        let centre = (200.0 * 10.0_f32.powf(0.5 * (1.0 + depth * lfo))).min(0.45 * sample_rate);
        let tangent = (std::f32::consts::PI * centre / sample_rate).tan();
        let coefficient = (1.0 - tangent) / (1.0 + tangent);
        let mut stage_input = sample + feedback * previous_wet;
        for stage in 0..stage_count {
            let stage_output = -coefficient * stage_input + previous_input[stage] + coefficient * previous_output[stage];
            previous_input[stage] = stage_input;
            previous_output[stage] = stage_output;
            stage_input = stage_output;
        }
        previous_wet = stage_input;
        output.push((1.0 - mix) * sample + mix * stage_input);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::phaser;

    #[test]
    fn impulse_single_stage_static_chain() {
        let mut input = vec![0.0; 10];
        input[0] = 1.0;
        let got = phaser(&input, 44100.0, 1, 0.0, 0.0, 0.0, 1.0);
        let expected = [-0.913719, 0.165117, 0.150871, 0.137853, 0.125959, 0.115091];
        for (a, e) in got.iter().zip(expected) {
            assert!((*a - e).abs() <= 1e-4);
        }
    }

    #[test]
    fn impulse_two_stage_static_chain() {
        let mut input = vec![0.0; 10];
        input[0] = 1.0;
        let got = phaser(&input, 44100.0, 2, 0.0, 0.0, 0.0, 1.0);
        let expected = [0.834883, -0.301741, -0.248443, -0.202096, -0.161897, -0.12713];
        for (a, e) in got.iter().zip(expected) {
            assert!((*a - e).abs() <= 1e-4);
        }
    }

    #[test]
    fn impulse_single_stage_feedback_half() {
        let mut input = vec![0.0; 10];
        input[0] = 1.0;
        let got = phaser(&input, 44100.0, 1, 0.0, 0.0, 0.5, 1.0);
        let expected = [-0.913719, 0.582558, -0.190712, 0.204151, -0.00208794, 0.101121, 0.0451543, 0.0711899];
        for (a, e) in got.iter().zip(expected) {
            assert!((*a - e).abs() <= 1e-4);
        }
    }

    fn sine_signal(length: usize, frequency: f64) -> Vec<f32> {
        (0..length).map(|n| (2.0 * std::f64::consts::PI * frequency * n as f64 / 44100.0).sin() as f32).collect()
    }

    #[test]
    fn phase_minus_ninety_per_stage_at_centre_one_stage() {
        let input = sine_signal(5000, 632.455532);
        let got = phaser(&input, 44100.0, 1, 0.0, 0.0, 0.0, 1.0);
        for (n, value) in got.iter().enumerate().take(4000).skip(3000) {
            let expected = -(2.0 * std::f64::consts::PI * 632.455532 * n as f64 / 44100.0).cos() as f32;
            assert!((*value - expected).abs() <= 2e-3);
        }
    }

    #[test]
    fn phase_minus_one_eighty_per_two_stages_at_centre() {
        let input = sine_signal(5000, 632.455532);
        let got = phaser(&input, 44100.0, 2, 0.0, 0.0, 0.0, 1.0);
        for (n, value) in got.iter().enumerate().take(4000).skip(3000) {
            let expected = -(2.0 * std::f64::consts::PI * 632.455532 * n as f64 / 44100.0).sin() as f32;
            assert!((*value - expected).abs() <= 2e-3);
        }
    }

    fn gain(frequency: f64, stages: usize, mix: f32) -> f32 {
        let input = sine_signal(20000, frequency);
        let output = phaser(&input, 44100.0, stages, 0.0, 0.0, 0.0, mix);
        let (mut in_sum, mut out_sum) = (0.0_f64, 0.0_f64);
        for n in 10000..20000 {
            in_sum += f64::from(input[n]).powi(2);
            out_sum += f64::from(output[n]).powi(2);
        }
        (out_sum / in_sum).sqrt() as f32
    }

    #[test]
    fn mix_half_single_stage_gain_at_centre() {
        assert!((gain(632.455532, 1, 0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() <= 1e-3);
    }
    #[test]
    fn notch_two_stages_at_centre() {
        assert!(gain(632.455532, 2, 0.5) < 1e-3);
    }
    #[test]
    fn notch_four_stages_at_262_hz() {
        assert!(gain(262.118596, 4, 0.5) < 2e-3);
    }
    #[test]
    fn notch_four_stages_at_1521_hz() {
        assert!(gain(1521.925322, 4, 0.5) < 2e-3);
    }
    #[test]
    fn four_stages_pass_centre_unchanged() {
        assert!((gain(632.455532, 4, 0.5) - 1.0).abs() <= 2e-3);
    }

    fn deterministic_input() -> Vec<f32> {
        (0..500).map(|n| ((n * 37) % 101) as f32 / 100.0 - 0.5).collect()
    }

    #[test]
    fn depth_zero_fixed_notch_positions() {
        let x = deterministic_input();
        assert_eq!(phaser(&x, 44100.0, 4, 0.5, 0.0, 0.3, 0.5), phaser(&x, 44100.0, 4, 7.0, 0.0, 0.3, 0.5));
    }
    #[test]
    fn depth_one_changes_output() {
        let x = deterministic_input();
        let a = phaser(&x, 44100.0, 4, 1.0, 1.0, 0.3, 0.5);
        let b = phaser(&x, 44100.0, 4, 0.0, 0.0, 0.3, 0.5);
        assert!(a.iter().zip(b).any(|(l, r)| (*l - r).abs() > 1e-3));
    }

    #[test]
    fn edge_cases_invalid_inputs_and_clamps() {
        let x = [0.2, -0.4, 0.8];
        assert!(phaser(&[], 44100.0, 1, 0.0, 0.0, 0.0, 1.0).is_empty());
        for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(phaser(&x, rate, 1, 0.0, 0.0, 0.0, 1.0), x);
        }
        assert_eq!(phaser(&x, 44100.0, 0, 0.0, 0.0, 0.0, 1.0), phaser(&x, 44100.0, 1, 0.0, 0.0, 0.0, 1.0));
        assert_eq!(phaser(&x, 44100.0, 1, 0.0, 0.0, 2.0, 1.0), phaser(&x, 44100.0, 1, 0.0, 0.0, 0.95, 1.0));
        assert_eq!(phaser(&x, 44100.0, 1, 0.0, 0.0, f32::NAN, 1.0), phaser(&x, 44100.0, 1, 0.0, 0.0, 0.0, 1.0));
        assert_eq!(phaser(&x, 44100.0, 1, 0.0, 0.0, 0.0, 0.0), x);
    }

    #[test]
    fn bounded_output_for_bounded_input() {
        let x: Vec<f32> = (0..44100).map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 44100.0).sin()).collect();
        let y = phaser(&x, 44100.0, 8, 0.5, 1.0, 0.95, 1.0);
        assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 40.0));
    }
}
