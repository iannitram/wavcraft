/// The waveshaper that follows the drive stage.
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Smooth tanh-style saturation.
    SoftClip,
    /// Hard clipping to the range [-1.0, 1.0].
    HardClip,
}

/// Drives one sample into the waveshaper, then applies the output gain.
///
/// A drive of exactly 0.0 bypasses the stage: the input comes back unchanged.
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
}
