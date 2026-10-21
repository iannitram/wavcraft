/// Returns constant-power left and right gains for a clamped pan value.
pub fn pan_gains(mut pan: f32) -> (f32, f32) {
    if !pan.is_finite() {
        pan = 0.0;
    }
    let pan = pan.clamp(-1.0, 1.0);
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
    fn clamps_and_handles_non_finite_input() {
        assert_eq!(pan_gains(5.0), pan_gains(1.0));
        assert_eq!(pan_gains(-5.0), pan_gains(-1.0));
        let (left, right) = pan_gains(f32::NAN);
        approx(left, 0.707_106_77);
        approx(right, 0.707_106_77);
    }
}
