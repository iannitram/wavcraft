//! A077-a: fade curve shapes (linear, logarithmic, cosine, exponential), the in-place fades and the
//! drag-to-curve pick. Core maths only.

use wavcraft_core::dsp::fade_curve::{curve_from_drag, fade_gain, fade_in, fade_out, CurveKind};

fn close(a: f32, b: f32) {
    assert!((a - b).abs() <= 1e-6, "{a} != {b}");
}

fn close_all(got: &[f32], want: &[f32]) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (a, b) in got.iter().zip(want) {
        close(*a, *b);
    }
}

const KINDS: [CurveKind; 4] = [CurveKind::Linear, CurveKind::Logarithmic, CurveKind::Cosine, CurveKind::Exponential];

#[test]
fn all_lists_the_four_shapes_in_tie_order() {
    assert_eq!(CurveKind::ALL, KINDS);
}

#[test]
fn every_curve_is_exactly_zero_at_the_start_and_one_at_the_end() {
    for kind in KINDS {
        assert_eq!(fade_gain(kind, 0.0), 0.0, "{kind:?} start");
        assert_eq!(fade_gain(kind, 1.0), 1.0, "{kind:?} end");
    }
}

#[test]
fn every_curve_never_decreases_and_stays_in_range() {
    for kind in KINDS {
        let mut last = 0.0_f32;
        for step in 0..=1000 {
            let g = fade_gain(kind, step as f32 / 1000.0);
            assert!(g >= last, "{kind:?} fell at step {step}: {g} < {last}");
            assert!((0.0..=1.0).contains(&g), "{kind:?} left 0..1 at step {step}: {g}");
            last = g;
        }
    }
}

#[test]
fn linear_is_the_identity() {
    for t in [0.0_f32, 0.125, 0.25, 0.5, 0.75, 0.875, 1.0] {
        assert_eq!(fade_gain(CurveKind::Linear, t), t);
    }
}

#[test]
fn the_middle_values_order_log_above_linear_above_exponential() {
    let log = fade_gain(CurveKind::Logarithmic, 0.5);
    let cos = fade_gain(CurveKind::Cosine, 0.5);
    let exp = fade_gain(CurveKind::Exponential, 0.5);
    assert_eq!(cos, 0.5);
    assert!(log > 0.5, "{log}");
    assert!(exp < 0.5, "{exp}");
    close(log, 0.740_362_7);
    close(exp, 0.240_253_07);
}

#[test]
fn curve_values_at_a_quarter_and_three_quarters() {
    close(fade_gain(CurveKind::Logarithmic, 0.25), 0.511_883_4);
    close(fade_gain(CurveKind::Cosine, 0.25), 0.146_446_61);
    close(fade_gain(CurveKind::Exponential, 0.25), 0.086_475_49);
    close(fade_gain(CurveKind::Logarithmic, 0.75), 0.889_301_7);
    close(fade_gain(CurveKind::Cosine, 0.75), 0.853_553_4);
    close(fade_gain(CurveKind::Exponential, 0.75), 0.513_712_6);
}

#[test]
fn t_is_clamped_and_nan_is_zero() {
    for kind in KINDS {
        assert_eq!(fade_gain(kind, -0.5), 0.0, "{kind:?} below");
        assert_eq!(fade_gain(kind, f32::NEG_INFINITY), 0.0, "{kind:?} -inf");
        assert_eq!(fade_gain(kind, 1.5), 1.0, "{kind:?} above");
        assert_eq!(fade_gain(kind, f32::INFINITY), 1.0, "{kind:?} +inf");
        assert_eq!(fade_gain(kind, f32::NAN), 0.0, "{kind:?} nan");
    }
}

#[test]
fn fade_in_scales_each_sample_by_the_curve() {
    let mut linear = [2.0_f32; 5];
    fade_in(&mut linear, CurveKind::Linear);
    assert_eq!(linear, [0.0, 0.5, 1.0, 1.5, 2.0]);

    let mut log = [1.0_f32; 5];
    fade_in(&mut log, CurveKind::Logarithmic);
    close_all(&log, &[0.0, 0.511_883_4, 0.740_362_7, 0.889_301_7, 1.0]);

    let mut cos = [1.0_f32; 5];
    fade_in(&mut cos, CurveKind::Cosine);
    close_all(&cos, &[0.0, 0.146_446_61, 0.5, 0.853_553_4, 1.0]);

    let mut exp = [1.0_f32; 5];
    fade_in(&mut exp, CurveKind::Exponential);
    close_all(&exp, &[0.0, 0.086_475_49, 0.240_253_07, 0.513_712_6, 1.0]);
}

#[test]
fn fade_in_keeps_the_sign_and_shape_of_the_audio() {
    let mut samples = [1.0_f32, -1.0, 1.0, -1.0, 1.0];
    fade_in(&mut samples, CurveKind::Linear);
    assert_eq!(samples, [0.0, -0.25, 0.5, -0.75, 1.0]);
}

#[test]
fn fade_out_scales_each_sample_by_the_mirrored_curve() {
    let mut linear = [2.0_f32; 5];
    fade_out(&mut linear, CurveKind::Linear);
    assert_eq!(linear, [2.0, 1.5, 1.0, 0.5, 0.0]);

    let mut cos = [1.0_f32; 5];
    fade_out(&mut cos, CurveKind::Cosine);
    close_all(&cos, &[1.0, 0.853_553_4, 0.5, 0.146_446_61, 0.0]);
}

#[test]
fn fade_out_is_fade_in_reversed_for_every_shape() {
    for kind in KINDS {
        for n in [2_usize, 3, 9, 100] {
            let mut up = vec![1.0_f32; n];
            let mut down = vec![1.0_f32; n];
            fade_in(&mut up, kind);
            fade_out(&mut down, kind);
            up.reverse();
            assert_eq!(up, down, "{kind:?} n={n}");
        }
    }
}

#[test]
fn the_fades_start_and_end_exactly() {
    for kind in KINDS {
        let mut up = vec![1.0_f32; 64];
        fade_in(&mut up, kind);
        assert_eq!(up.first(), Some(&0.0), "{kind:?} fade-in start");
        assert_eq!(up.last(), Some(&1.0), "{kind:?} fade-in end");
        let mut down = vec![1.0_f32; 64];
        fade_out(&mut down, kind);
        assert_eq!(down.first(), Some(&1.0), "{kind:?} fade-out start");
        assert_eq!(down.last(), Some(&0.0), "{kind:?} fade-out end");
    }
}

#[test]
fn two_samples_get_zero_and_one() {
    let mut up = [3.0_f32, 3.0];
    fade_in(&mut up, CurveKind::Logarithmic);
    assert_eq!(up, [0.0, 3.0]);
    let mut down = [3.0_f32, 3.0];
    fade_out(&mut down, CurveKind::Logarithmic);
    assert_eq!(down, [3.0, 0.0]);
}

#[test]
fn an_empty_slice_is_untouched() {
    let mut empty: [f32; 0] = [];
    fade_in(&mut empty, CurveKind::Cosine);
    fade_out(&mut empty, CurveKind::Cosine);
    assert!(empty.is_empty());
}

#[test]
fn a_single_sample_is_left_unchanged() {
    for kind in KINDS {
        let mut one = [0.7_f32];
        fade_in(&mut one, kind);
        assert_eq!(one, [0.7]);
        fade_out(&mut one, kind);
        assert_eq!(one, [0.7]);
    }
}

#[test]
fn a_drag_to_each_curves_own_value_picks_that_curve() {
    for kind in KINDS {
        let own = fade_gain(kind, 0.25);
        assert_eq!(curve_from_drag(0.25, own), kind, "{kind:?}");
    }
    // The same values typed by hand.
    assert_eq!(curve_from_drag(0.25, 0.086_475_49), CurveKind::Exponential);
    assert_eq!(curve_from_drag(0.25, 0.146_446_61), CurveKind::Cosine);
    assert_eq!(curve_from_drag(0.25, 0.25), CurveKind::Linear);
    assert_eq!(curve_from_drag(0.25, 0.511_883_4), CurveKind::Logarithmic);
}

#[test]
fn a_drag_in_between_picks_the_nearest_curve() {
    // The four values at t = 0.25 are 0.0865, 0.1464, 0.25 and 0.5119; the halfway points are
    // 0.1165, 0.1982 and 0.3819.
    assert_eq!(curve_from_drag(0.25, 0.11), CurveKind::Exponential);
    assert_eq!(curve_from_drag(0.25, 0.12), CurveKind::Cosine);
    assert_eq!(curve_from_drag(0.25, 0.19), CurveKind::Cosine);
    assert_eq!(curve_from_drag(0.25, 0.20), CurveKind::Linear);
    assert_eq!(curve_from_drag(0.25, 0.38), CurveKind::Linear);
    assert_eq!(curve_from_drag(0.25, 0.39), CurveKind::Logarithmic);
}

#[test]
fn a_drag_at_the_middle_tells_three_curves_apart_and_gives_linear_for_the_tie() {
    assert_eq!(curve_from_drag(0.5, 0.74), CurveKind::Logarithmic);
    assert_eq!(curve_from_drag(0.5, 0.24), CurveKind::Exponential);
    assert_eq!(curve_from_drag(0.5, 0.6), CurveKind::Linear);
    // Linear and Cosine are both 0.5 here: the earlier one in ALL wins.
    assert_eq!(curve_from_drag(0.5, 0.5), CurveKind::Linear);
}

#[test]
fn a_drag_gain_outside_0_to_1_is_clamped() {
    assert_eq!(curve_from_drag(0.25, -5.0), CurveKind::Exponential);
    assert_eq!(curve_from_drag(0.25, f32::NEG_INFINITY), CurveKind::Exponential);
    assert_eq!(curve_from_drag(0.25, 5.0), CurveKind::Logarithmic);
    assert_eq!(curve_from_drag(0.25, f32::INFINITY), CurveKind::Logarithmic);
}

#[test]
fn a_drag_with_t_outside_0_to_1_is_clamped() {
    // t = 3 acts as t = 1, where every shape is 1: Linear. t = -3 acts as 0: Linear.
    assert_eq!(curve_from_drag(3.0, 0.5), CurveKind::Linear);
    assert_eq!(curve_from_drag(-3.0, 0.5), CurveKind::Linear);
    // t = 1.5 acts as 1.
    assert_eq!(curve_from_drag(1.5, 0.99), CurveKind::Linear);
}

#[test]
fn nan_and_the_ends_give_linear() {
    assert_eq!(curve_from_drag(f32::NAN, 0.9), CurveKind::Linear);
    assert_eq!(curve_from_drag(0.25, f32::NAN), CurveKind::Linear);
    assert_eq!(curve_from_drag(0.0, 0.3), CurveKind::Linear);
    assert_eq!(curve_from_drag(1.0, 0.3), CurveKind::Linear);
}

#[test]
fn a_drag_gain_is_clamped_to_exactly_0_and_1_not_nearer_values() {
    // At t = 0.1 the values are Exponential 0.0288, Cosine 0.0245, Linear 0.1, Logarithmic 0.2788, so a
    // gain of 0 (or below) is nearest to Cosine; a clamp that stopped at 0.1 would pick Linear.
    assert_eq!(curve_from_drag(0.1, 0.0), CurveKind::Cosine);
    assert_eq!(curve_from_drag(0.1, -3.0), CurveKind::Cosine);
    // At t = 0.9 the values are 0.771, 0.9756, 0.9 and 0.959, so a gain of 1 (or above) is nearest to
    // Cosine; a clamp that stopped at 0.9 would pick Linear.
    assert_eq!(curve_from_drag(0.9, 1.0), CurveKind::Cosine);
    assert_eq!(curve_from_drag(0.9, 7.0), CurveKind::Cosine);
}
