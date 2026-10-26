use wavcraft_core::dsp::stereo_width::{measure_width, widen, widen_interleaved};

const TOL: f32 = 1e-6;

fn assert_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "length");
    for (index, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!((a - e).abs() <= TOL, "index {index}: {a} != {e}");
    }
}

// Pairs (L, R) -> (mid, side): (0.75, 0.25) -> (0.5, 0.25); (0.25, 0.25) -> (0.25, 0.0);
// (-0.5, 0.5) -> (0.0, -0.5); (0.0, 0.5) -> (0.25, -0.25).
const LEFT: [f32; 4] = [0.75, 0.25, -0.5, 0.0];
const RIGHT: [f32; 4] = [0.25, 0.25, 0.5, 0.5];

fn run(width: f32) -> (Vec<f32>, Vec<f32>) {
    let mut left = LEFT.to_vec();
    let mut right = RIGHT.to_vec();
    widen(&mut left, &mut right, width);
    (left, right)
}

#[test]
fn width_one_is_identity() {
    let (left, right) = run(1.0);
    assert_close(&left, &LEFT);
    assert_close(&right, &RIGHT);
}

#[test]
fn width_zero_makes_both_channels_the_mid() {
    let (left, right) = run(0.0);
    assert_close(&left, &[0.5, 0.25, 0.0, 0.25]);
    assert_close(&right, &[0.5, 0.25, 0.0, 0.25]);
}

#[test]
fn mono_signal_is_unchanged_at_any_width() {
    let mono = [0.5_f32, -0.25, 0.125, 0.0];
    for width in [0.0, 0.5, 1.0, 2.0, 4.0] {
        let mut left = mono.to_vec();
        let mut right = mono.to_vec();
        widen(&mut left, &mut right, width);
        assert_close(&left, &mono);
        assert_close(&right, &mono);
    }
}

#[test]
fn width_two_doubles_the_side_and_keeps_the_mid() {
    let (left, right) = run(2.0);
    assert_close(&left, &[1.0, 0.25, -1.0, -0.25]);
    assert_close(&right, &[0.0, 0.25, 1.0, 0.75]);
}

#[test]
fn width_half_halves_the_side() {
    let (left, right) = run(0.5);
    assert_close(&left, &[0.625, 0.25, -0.25, 0.125]);
    assert_close(&right, &[0.375, 0.25, 0.25, 0.375]);
}

#[test]
fn width_above_four_acts_as_four() {
    let expected_left = [1.5, 0.25, -2.0, -0.75];
    let expected_right = [-0.5, 0.25, 2.0, 1.25];
    for width in [4.0, 10.0, f32::INFINITY] {
        let (left, right) = run(width);
        assert_close(&left, &expected_left);
        assert_close(&right, &expected_right);
    }
}

#[test]
fn negative_width_acts_as_zero() {
    for width in [-1.0, -4.0, f32::NEG_INFINITY] {
        let (left, right) = run(width);
        assert_close(&left, &[0.5, 0.25, 0.0, 0.25]);
        assert_close(&right, &[0.5, 0.25, 0.0, 0.25]);
    }
}

#[test]
fn nan_width_leaves_the_signal_unchanged() {
    let (left, right) = run(f32::NAN);
    assert_close(&left, &LEFT);
    assert_close(&right, &RIGHT);
}

#[test]
fn unequal_lengths_use_the_shorter_slice() {
    let mut left = vec![0.75_f32, 0.0, 0.9];
    let mut right = vec![0.25_f32, 0.5];
    widen(&mut left, &mut right, 0.0);
    assert_close(&left, &[0.5, 0.25, 0.9]);
    assert_close(&right, &[0.5, 0.25]);
}

#[test]
fn interleaved_matches_the_planar_result() {
    let mut samples = vec![0.75_f32, 0.25, 0.25, 0.25, -0.5, 0.5, 0.0, 0.5];
    widen_interleaved(&mut samples, 2.0);
    assert_close(&samples, &[1.0, 0.0, 0.25, 0.25, -1.0, 1.0, -0.25, 0.75]);
}

#[test]
fn interleaved_odd_length_keeps_the_last_sample() {
    let mut samples = vec![0.75_f32, 0.25, 0.9];
    widen_interleaved(&mut samples, 0.0);
    assert_close(&samples, &[0.5, 0.5, 0.9]);
}

#[test]
fn measure_width_of_a_known_signal() {
    // mid = [0.5, 0.25], side = [0.25, 0.0]: sqrt(0.0625 / 0.3125) = 0.4472136
    let width = measure_width(&[0.75, 0.25], &[0.25, 0.25]);
    assert!((width - 0.4472136).abs() <= TOL, "{width}");
}

#[test]
fn measure_width_doubles_when_the_signal_is_widened() {
    let mut left = vec![0.75_f32, 0.25];
    let mut right = vec![0.25_f32, 0.25];
    widen(&mut left, &mut right, 2.0);
    let width = measure_width(&left, &right);
    assert!((width - 0.8944272).abs() <= TOL, "{width}");
}

#[test]
fn measure_width_of_silence_and_empty_input_is_zero() {
    assert!(measure_width(&[0.0, 0.0], &[0.0, 0.0]).abs() <= TOL);
    assert!(measure_width(&[], &[]).abs() <= TOL);
}

#[test]
fn measure_width_of_a_mono_signal_is_zero() {
    assert!(measure_width(&[0.5, -0.25], &[0.5, -0.25]).abs() <= TOL);
}

#[test]
fn measure_width_uses_the_shorter_slice() {
    let width = measure_width(&[0.75, 0.25, 9.0], &[0.25, 0.25]);
    assert!((width - 0.4472136).abs() <= TOL, "{width}");
}
