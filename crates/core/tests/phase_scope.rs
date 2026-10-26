//! A040-a: the phase scope numbers (goniometer points, balance, mid/side levels). Core only.

use std::f32::consts::FRAC_1_SQRT_2;
use wavcraft_core::meters::phase_scope::{FLOOR_DB, balance, mid_side_rms_db, scope_points};

fn close(a: f32, b: f32, tolerance: f32) {
    assert!((a - b).abs() <= tolerance, "{a} != {b}");
}

fn close_point(got: (f32, f32), x: f32, y: f32) {
    close(got.0, x, 1e-6);
    close(got.1, y, 1e-6);
}

/// 0.0, 1.0, 2.0 ... as samples.
fn ramp(len: usize) -> Vec<f32> {
    (0..len).map(|i| i as f32).collect()
}

// ---- scope_points ----

#[test]
fn identical_channels_draw_a_vertical_line() {
    let left = [0.5, -0.25, 1.0];
    let points = scope_points(&left, &left, 10);
    assert_eq!(points.len(), 3);
    for (point, y) in points.iter().zip([0.5 * 2.0, -0.25 * 2.0, 2.0]) {
        assert_eq!(point.0, 0.0);
        close(point.1, y * FRAC_1_SQRT_2, 1e-6);
    }
}

#[test]
fn inverted_channels_draw_a_horizontal_line() {
    let left = [0.5, -0.25, 1.0];
    let right = [-0.5, 0.25, -1.0];
    let points = scope_points(&left, &right, 10);
    assert_eq!(points.len(), 3);
    for (point, x) in points.iter().zip([-1.0, 0.5, -2.0]) {
        assert_eq!(point.1, 0.0);
        close(point.0, x * FRAC_1_SQRT_2, 1e-6);
    }
}

#[test]
fn left_only_leans_left_and_right_only_leans_right() {
    let points = scope_points(&[1.0, -1.0], &[0.0, 0.0], 10);
    assert_eq!(points.len(), 2);
    close_point(points[0], -FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    close_point(points[1], FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
    let points = scope_points(&[0.0, 0.0], &[1.0, -1.0], 10);
    assert_eq!(points.len(), 2);
    close_point(points[0], FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    close_point(points[1], -FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
}

#[test]
fn a_general_pair_uses_both_formulas() {
    // l = 3, r = 1: x = (1 - 3) / sqrt2 = -sqrt2, y = (3 + 1) / sqrt2 = 2 sqrt2.
    let points = scope_points(&[3.0], &[1.0], 4);
    assert_eq!(points.len(), 1);
    close_point(points[0], -2.0 * FRAC_1_SQRT_2, 4.0 * FRAC_1_SQRT_2);
}

#[test]
fn thinning_picks_evenly_spaced_pairs_rounded_down_and_keeps_order() {
    // n = 10, max = 4: pairs 0, 2, 5, 7 (2.5 and 7.5 round down).
    let left = ramp(10);
    let right = vec![0.0; 10];
    let points = scope_points(&left, &right, 4);
    assert_eq!(points.len(), 4);
    for (point, index) in points.iter().zip([0.0_f32, 2.0, 5.0, 7.0]) {
        close_point(*point, -index * FRAC_1_SQRT_2, index * FRAC_1_SQRT_2);
    }
}

#[test]
fn thinning_reads_the_right_channel_at_the_same_pairs() {
    // Right carries the ramp, left is silent: x = y = index / sqrt2 at pairs 0, 2, 5, 7.
    let right = ramp(10);
    let points = scope_points(&[0.0; 10], &right, 4);
    assert_eq!(points.len(), 4);
    for (point, index) in points.iter().zip([0.0_f32, 2.0, 5.0, 7.0]) {
        close_point(*point, index * FRAC_1_SQRT_2, index * FRAC_1_SQRT_2);
    }
}

#[test]
fn thinning_a_long_signal_never_exceeds_the_limit() {
    // n = 1000, max = 7: pairs 0, 142, 285, 428, 571, 714, 857.
    let left = ramp(1000);
    let right = vec![0.0; 1000];
    let points = scope_points(&left, &right, 7);
    assert_eq!(points.len(), 7);
    for (point, index) in points.iter().zip([0.0_f32, 142.0, 285.0, 428.0, 571.0, 714.0, 857.0]) {
        close(point.1, index * FRAC_1_SQRT_2, 1e-3);
        close(point.0, -index * FRAC_1_SQRT_2, 1e-3);
    }
}

#[test]
fn a_signal_at_or_under_the_limit_keeps_every_pair() {
    let left = ramp(4);
    let right = vec![0.0; 4];
    for max_points in [4, 5, 100] {
        let points = scope_points(&left, &right, max_points);
        assert_eq!(points.len(), 4, "max {max_points}");
        for (point, index) in points.iter().zip([0.0_f32, 1.0, 2.0, 3.0]) {
            close(point.1, index * FRAC_1_SQRT_2, 1e-6);
        }
    }
}

#[test]
fn a_limit_of_one_keeps_the_first_pair_and_zero_keeps_nothing() {
    let left = [5.0, 6.0, 7.0];
    let right = [0.0, 0.0, 0.0];
    let points = scope_points(&left, &right, 1);
    assert_eq!(points.len(), 1);
    close_point(points[0], -5.0 * FRAC_1_SQRT_2, 5.0 * FRAC_1_SQRT_2);
    assert!(scope_points(&left, &right, 0).is_empty());
}

#[test]
fn empty_input_gives_no_points() {
    assert!(scope_points(&[], &[], 8).is_empty());
    assert!(scope_points(&[1.0], &[], 8).is_empty());
    assert!(scope_points(&[], &[1.0], 8).is_empty());
}

#[test]
fn silence_still_draws_its_points_at_the_centre() {
    let points = scope_points(&[0.0; 3], &[0.0; 3], 10);
    assert_eq!(points, vec![(0.0, 0.0); 3]);
}

#[test]
fn mismatched_lengths_use_the_common_prefix() {
    // Right is shorter: 4 pairs.
    let points = scope_points(&ramp(10), &[0.0; 4], 10);
    assert_eq!(points.len(), 4);
    close(points[3].1, 3.0 * FRAC_1_SQRT_2, 1e-6);
    // Left is shorter: 4 pairs.
    let points = scope_points(&[0.0; 4], &ramp(10), 10);
    assert_eq!(points.len(), 4);
    close(points[3].1, 3.0 * FRAC_1_SQRT_2, 1e-6);
}

#[test]
fn thinning_counts_the_common_prefix_not_the_longer_channel() {
    // n = 4 (right is short), max = 2: pairs 0 and 2, not 0 and 50.
    let left = ramp(100);
    let points = scope_points(&left, &[0.0; 4], 2);
    assert_eq!(points.len(), 2);
    close(points[0].1, 0.0, 1e-6);
    close(points[1].1, 2.0 * FRAC_1_SQRT_2, 1e-6);
}

#[test]
fn samples_that_are_not_finite_count_as_zero() {
    let points = scope_points(&[f32::NAN, 1.0], &[1.0, f32::INFINITY], 10);
    assert_eq!(points.len(), 2);
    close_point(points[0], FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    close_point(points[1], -FRAC_1_SQRT_2, FRAC_1_SQRT_2);
}

// ---- balance ----

#[test]
fn left_only_is_minus_one_and_right_only_is_plus_one() {
    assert_eq!(balance(&[0.5, -0.5], &[0.0, 0.0]), -1.0);
    assert_eq!(balance(&[0.0, 0.0], &[0.5, -0.5]), 1.0);
}

#[test]
fn equal_levels_are_centred_whatever_the_phase() {
    assert_eq!(balance(&[0.3, -0.2], &[0.3, -0.2]), 0.0);
    assert_eq!(balance(&[1.0, -1.0], &[-1.0, 1.0]), 0.0);
}

#[test]
fn balance_follows_the_rms_ratio() {
    // rms 1 and 3: (3 - 1) / (3 + 1) = 0.5.
    assert_eq!(balance(&[1.0, -1.0], &[3.0, -3.0]), 0.5);
    assert_eq!(balance(&[3.0, -3.0], &[1.0, -1.0]), -0.5);
}

#[test]
fn balance_uses_rms_not_peak() {
    // Left peaks at 2 but its rms is sqrt(2); right rms is 1: (1 - sqrt2) / (1 + sqrt2).
    let got = balance(&[2.0, 0.0], &[1.0, -1.0]);
    close(got, (1.0 - 2.0_f32.sqrt()) / (1.0 + 2.0_f32.sqrt()), 1e-6);
}

#[test]
fn silence_and_empty_input_are_centred() {
    assert_eq!(balance(&[0.0; 4], &[0.0; 4]), 0.0);
    assert_eq!(balance(&[], &[]), 0.0);
    assert_eq!(balance(&[1.0], &[]), 0.0);
    assert_eq!(balance(&[], &[1.0]), 0.0);
}

#[test]
fn balance_uses_the_common_prefix() {
    assert_eq!(balance(&[1.0, -1.0, 100.0], &[3.0, -3.0]), 0.5);
    assert_eq!(balance(&[1.0, -1.0], &[3.0, -3.0, 100.0]), 0.5);
}

#[test]
fn balance_ignores_samples_that_are_not_finite() {
    // Counted as 0, the channels are [0, 3] and [3, 0]: equal rms.
    assert_eq!(balance(&[f32::INFINITY, 3.0], &[3.0, f32::NAN]), 0.0);
    // A lone bad sample in an otherwise silent pair is silence, not a level.
    assert_eq!(balance(&[f32::NAN, 0.0], &[0.0, 0.0]), 0.0);
    assert_eq!(balance(&[0.0, 0.0], &[f32::NEG_INFINITY, 0.0]), 0.0);
}

// ---- mid_side_rms_db ----

#[test]
fn identical_channels_are_all_mid() {
    // mid = [1, -1] (rms 1 = 0 dB), side = 0 (floor).
    assert_eq!(mid_side_rms_db(&[1.0, -1.0], &[1.0, -1.0]), (0.0, -120.0));
}

#[test]
fn inverted_channels_are_all_side() {
    assert_eq!(mid_side_rms_db(&[1.0, -1.0], &[-1.0, 1.0]), (-120.0, 0.0));
}

#[test]
fn one_channel_only_splits_half_and_half() {
    // mid = side = [0.5, -0.5]: 20 log10(0.5) = -6.0206 dB each.
    for (left, right) in [([1.0_f32, -1.0], [0.0_f32, 0.0]), ([0.0, 0.0], [1.0, -1.0])] {
        let (mid, side) = mid_side_rms_db(&left, &right);
        close(mid, -6.0206, 1e-4);
        close(side, -6.0206, 1e-4);
    }
}

#[test]
fn mid_and_side_levels_are_told_apart() {
    // l = [3, 1], r = [1, -1]: mid = [2, 0] (rms sqrt2 = +3.0103 dB), side = [1, 1] (rms 1 = 0 dB).
    let (mid, side) = mid_side_rms_db(&[3.0, 1.0], &[1.0, -1.0]);
    close(mid, 3.0103, 1e-4);
    close(side, 0.0, 1e-4);
}

#[test]
fn the_level_follows_the_amplitude() {
    let (mid, side) = mid_side_rms_db(&[0.1, -0.1], &[0.1, -0.1]);
    close(mid, -20.0, 1e-4);
    assert_eq!(side, -120.0);
}

#[test]
fn levels_never_go_below_the_floor() {
    assert_eq!(FLOOR_DB, -120.0);
    // 1e-9 would be -180 dB; 1e-5 is -100 dB, above the floor.
    assert_eq!(mid_side_rms_db(&[1e-9, -1e-9], &[1e-9, -1e-9]), (-120.0, -120.0));
    let (mid, side) = mid_side_rms_db(&[1e-5, -1e-5], &[1e-5, -1e-5]);
    close(mid, -100.0, 1e-3);
    assert_eq!(side, -120.0);
}

#[test]
fn silence_and_empty_input_sit_on_the_floor() {
    assert_eq!(mid_side_rms_db(&[0.0; 4], &[0.0; 4]), (-120.0, -120.0));
    assert_eq!(mid_side_rms_db(&[], &[]), (-120.0, -120.0));
    assert_eq!(mid_side_rms_db(&[1.0], &[]), (-120.0, -120.0));
}

#[test]
fn mid_side_uses_the_common_prefix() {
    assert_eq!(mid_side_rms_db(&[1.0, -1.0, 100.0], &[1.0, -1.0]), (0.0, -120.0));
    assert_eq!(mid_side_rms_db(&[1.0, -1.0], &[1.0, -1.0, 100.0]), (0.0, -120.0));
}

#[test]
fn mid_side_ignores_samples_that_are_not_finite() {
    // Counted as 0: left = right = [0, 1]: mid = [0, 1] (rms sqrt(0.5) = -3.0103 dB), side = 0.
    let (mid, side) = mid_side_rms_db(&[f32::INFINITY, 1.0], &[f32::INFINITY, 1.0]);
    close(mid, -3.0103, 1e-4);
    assert_eq!(side, -120.0);
}
