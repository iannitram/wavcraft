/// Converts left/right samples to mid/side samples.
pub fn lr_to_ms(l: f32, r: f32) -> (f32, f32) {
    ((l + r) * 0.5, (l - r) * 0.5)
}

/// Converts mid/side samples to left/right samples.
pub fn ms_to_lr(m: f32, s: f32) -> (f32, f32) {
    (m + s, m - s)
}

/// Converts the shorter prefix of left/right channels to mid/side in place.
pub fn lr_to_ms_in_place(left: &mut [f32], right: &mut [f32]) {
    for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
        (*left_sample, *right_sample) = lr_to_ms(*left_sample, *right_sample);
    }
}

/// Converts the shorter prefix of mid/side channels to left/right in place.
pub fn ms_to_lr_in_place(mid: &mut [f32], side: &mut [f32]) {
    for (mid_sample, side_sample) in mid.iter_mut().zip(side.iter_mut()) {
        (*mid_sample, *side_sample) = ms_to_lr(*mid_sample, *side_sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-6, "{a} != {b}");
    }

    #[test]
    fn converts_left_right_to_mid_side() {
        assert_eq!(lr_to_ms(1.0, 0.0), (0.5, 0.5));
        assert_eq!(lr_to_ms(0.5, -0.5), (0.0, 0.5));
        for value in [-1.0, -0.25, 0.0, 0.25, 1.0] {
            assert_eq!(lr_to_ms(value, value).1, 0.0);
        }
    }

    #[test]
    fn converts_mid_side_to_left_right() {
        assert_eq!(ms_to_lr(0.5, 0.5), (1.0, 0.0));
    }

    #[test]
    fn round_trip_in_place() {
        let original_left: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.017).sin()).collect();
        let original_right: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.031).sin()).collect();
        let mut left = original_left.clone();
        let mut right = original_right.clone();
        lr_to_ms_in_place(&mut left, &mut right);
        ms_to_lr_in_place(&mut left, &mut right);
        for (actual, expected) in left.iter().zip(&original_left) {
            approx(*actual, *expected);
        }
        for (actual, expected) in right.iter().zip(&original_right) {
            approx(*actual, *expected);
        }
    }

    #[test]
    fn in_place_functions_only_touch_the_shorter_slice() {
        let mut left = [1.0, 2.0, 3.0];
        let mut right = [4.0, 5.0];
        lr_to_ms_in_place(&mut left, &mut right);
        assert_eq!(left[2], 3.0);
        let mut mid = [1.0, 2.0, 3.0];
        let mut side = [4.0, 5.0];
        ms_to_lr_in_place(&mut mid, &mut side);
        assert_eq!(mid[2], 3.0);
    }
}
