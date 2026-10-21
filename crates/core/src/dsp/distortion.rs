/// The waveshaper that follows the drive stage.
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Smooth tanh-style saturation.
    SoftClip,
    /// Hard clipping to the range [-1.0, 1.0].
    HardClip,
}

/// A simple first-order low-pass filter.
pub struct ToneLp {
    alpha: f32,
    y: f32,
}

impl ToneLp {
    /// Creates a new low-pass filter with the given cutoff frequency and sample rate.
    ///
    /// If `cutoff_hz >= sample_rate / 2.0`, the filter is bypassed (alpha = 1.0).
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let alpha = if cutoff_hz >= sample_rate / 2.0 {
            1.0
        } else {
            1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate).exp()
        };
        Self { alpha, y: 0.0 }
    }

    /// Processes a sample through the low-pass filter.
    ///
    /// Returns the filtered output and updates the internal state.
    pub fn process(&mut self, x: f32) -> f32 {
        self.y += self.alpha * (x - self.y);
        self.y
    }
}

/// Drives one sample into the waveshaper, then applies the output gain.
///
/// A drive of exactly 0.0 bypasses the stage: the input comes back unchanged.
/// `output_gain` is a linear trim, and `|out| <= |output_gain|`.
pub fn distort(sample: f32, drive: f32, mode: Mode, output_gain: f32) -> f32 {
    if drive == 0.0 {
        return sample;
    }
    let driven = drive * sample;
    let shaped = match mode {
        Mode::SoftClip => driven.tanh(),
        Mode::HardClip => driven.clamp(-1.0, 1.0),
    };
    shaped * output_gain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-6, "{a} != {b}");
    }

    #[test]
    fn tone_lp_impulse_response() {
        let mut lp = ToneLp::new(1000.0, 48000.0);
        let alpha = 0.12269423;
        let output = lp.process(1.0);
        approx(output, alpha);
    }

    #[test]
    fn tone_lp_step_response() {
        let mut lp = ToneLp::new(1000.0, 48000.0);
        // Process a constant 1.0 input for 20000 samples to let it settle
        for _ in 0..20000 {
            lp.process(1.0);
        }
        // Should be very close to 1.0 (the filter will have settled)
        assert!((lp.process(1.0) - 1.0).abs() <= 1e-4);
    }

    #[test]
    fn tone_lp_bypass_at_nyquist() {
        // At cutoff 24000 Hz with 48000 Hz sample rate, should be bypassed
        let mut lp = ToneLp::new(24000.0, 48000.0);
        let input = 0.5;
        let output = lp.process(input);
        assert_eq!(output, input);
    }

    #[test]
    fn soft_clip_is_tanh_of_driven_sample() {
        approx(distort(0.5, 1.0, Mode::SoftClip, 1.0), 0.4621172);
    }

    #[test]
    fn hard_clip_clamps_driven_sample() {
        approx(distort(0.75, 2.0, Mode::HardClip, 1.0), 1.0);
        approx(distort(0.25, 2.0, Mode::HardClip, 1.0), 0.5);
        approx(distort(-0.25, 2.0, Mode::HardClip, 1.0), -0.5);
    }

    #[test]
    fn zero_drive_is_bypass_in_both_modes() {
        for mode in [Mode::SoftClip, Mode::HardClip] {
            for x in [0.0, 0.3, -2.0] {
                assert_eq!(distort(x, 0.0, mode, 1.0), x);
                assert_eq!(distort(x, 0.0, mode, 0.5), x);
            }
        }
    }

    #[test]
    fn output_stays_within_unit_range_at_unity_gain() {
        for mode in [Mode::SoftClip, Mode::HardClip] {
            for step in -200..=200 {
                let x = step as f32 / 20.0;
                let out = distort(x, 10.0, mode, 1.0);
                assert!(out.abs() <= 1.0, "{x} -> {out}");
            }
        }
    }

    #[test]
    fn output_gain_halves_the_result() {
        for mode in [Mode::SoftClip, Mode::HardClip] {
            for x in [0.1, 0.5, -0.7] {
                let full = distort(x, 2.0, mode, 1.0);
                let half = distort(x, 2.0, mode, 0.5);
                approx(half, full * 0.5);
            }
        }
    }

    #[test]
    fn soft_clip_tanh_result() {
        // tanh(1.0) = 0.7615942
        approx(distort(0.5, 2.0, Mode::SoftClip, 1.0), 0.7615942);
    }

    #[test]
    fn hard_clip_result() {
        approx(distort(1.0, 1.0, Mode::HardClip, 2.0), 2.0);
    }

    #[test]
    fn negative_output_gain_flips_sign() {
        approx(distort(0.5, 1.0, Mode::HardClip, -1.0), -0.5);
    }
}
