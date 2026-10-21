/// Returns constant-power gains, treating NaN as center and clamping pan.
pub fn pan_gains(mut pan: f32) -> (f32, f32) {
    if pan.is_nan() {
        pan = 0.0;
    }
    let pan = pan.clamp(-1.0, 1.0);
    if pan <= -1.0 {
        return (1.0, 0.0);
    }
    if pan >= 1.0 {
        return (0.0, 1.0);
    }
    let t = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    (t.cos(), t.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-6, "{a} != {b}");
    }

    #[test]
    fn endpoints_and_center() {
        let (left, right) = pan_gains(-1.0);
        approx(left, 1.0);
        approx(right, 0.0);
        let (left, right) = pan_gains(0.0);
        approx(left, 0.707_106_77);
        approx(right, 0.707_106_77);
        let (left, right) = pan_gains(1.0);
        approx(left, 0.0);
        approx(right, 1.0);
    }

    #[test]
    fn gains_have_constant_power() {
        for step in -10..=10 {
            let (left, right) = pan_gains(step as f32 / 10.0);
            approx(left * left + right * right, 1.0);
        }
    }

    #[test]
    fn handles_infinite_input() {
        assert_eq!(pan_gains(f32::INFINITY), (0.0, 1.0));
        assert_eq!(pan_gains(f32::NEG_INFINITY), (1.0, 0.0));
    }

    #[test]
    fn handles_nan_input() {
        let (left, right) = pan_gains(f32::NAN);
        approx(left, 0.707_106_77);
        approx(right, 0.707_106_77);
        assert_eq!(left, right);
    }

    #[test]
    fn clamps_finite_input() {
        assert_eq!(pan_gains(1.0).0, 0.0);
        assert_eq!(pan_gains(-1.0).1, 0.0);
        assert_eq!(pan_gains(5.0), pan_gains(1.0));
        assert_eq!(pan_gains(-5.0), pan_gains(-1.0));
    }

    #[test]
    fn uses_constant_power_angles() {
        let (left, right) = pan_gains(-0.5);
        approx(left, (std::f32::consts::PI / 8.0).cos());
        approx(right, (std::f32::consts::PI / 8.0).sin());
        approx(left * left + right * right, 1.0);

        let (left, right) = pan_gains(0.5);
        approx(left, (3.0 * std::f32::consts::PI / 8.0).cos());
        approx(right, (3.0 * std::f32::consts::PI / 8.0).sin());
        approx(left * left + right * right, 1.0);
    }
}
