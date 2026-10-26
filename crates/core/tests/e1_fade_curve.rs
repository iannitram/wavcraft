//! Locked tests for v1 E1 (issue #27, card wav-c-27-a): linear, logarithmic and cosine (S-curve)
//! fade gain formulas and the top-corner handle drag-to-curve rule.

use std::f64::consts::PI;

use wavcraft_core::dsp::fade::{DRAG_LOG_THRESHOLD_PX, FadeCurve, curve_from_drag, fade_in_gain, fade_out_gain};

fn near(got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-12, "got {got}, want {want}");
}

#[test]
fn e1_curve_from_drag_picks_linear_logarithmic_or_cosine() {
    assert_eq!(DRAG_LOG_THRESHOLD_PX, 8.0);
    assert_eq!(FadeCurve::default(), FadeCurve::Linear);
    assert_eq!(curve_from_drag(0.0, false), FadeCurve::Linear);
    assert_eq!(curve_from_drag(7.9, false), FadeCurve::Linear);
    assert_eq!(curve_from_drag(-7.9, false), FadeCurve::Linear);
    assert_eq!(curve_from_drag(8.0, false), FadeCurve::Logarithmic);
    assert_eq!(curve_from_drag(-8.0, false), FadeCurve::Logarithmic);
    assert_eq!(curve_from_drag(25.0, false), FadeCurve::Logarithmic);
    assert_eq!(curve_from_drag(-40.0, false), FadeCurve::Logarithmic);
    assert_eq!(curve_from_drag(0.0, true), FadeCurve::Cosine);
    assert_eq!(curve_from_drag(30.0, true), FadeCurve::Cosine);
    assert_eq!(curve_from_drag(-30.0, true), FadeCurve::Cosine);
    assert_eq!(curve_from_drag(f32::NAN, false), FadeCurve::Linear);
    assert_eq!(curve_from_drag(f32::NAN, true), FadeCurve::Cosine);
}

#[test]
fn e1_curve_fade_in_and_out_endpoints_and_midpoints() {
    for c in [FadeCurve::Linear, FadeCurve::Logarithmic, FadeCurve::Cosine] {
        near(fade_in_gain(0.0, c), 0.0);
        near(fade_in_gain(1.0, c), 1.0);
        near(fade_out_gain(0.0, c), 1.0);
        near(fade_out_gain(1.0, c), 0.0);
    }
    near(fade_in_gain(0.5, FadeCurve::Linear), 0.5);
    near(fade_out_gain(0.5, FadeCurve::Linear), 0.5);
    near(fade_in_gain(0.5, FadeCurve::Cosine), 0.5);
    near(fade_out_gain(0.5, FadeCurve::Cosine), 0.5);
    near(fade_in_gain(0.5, FadeCurve::Logarithmic), 0.740_362_689_494_243_9);
    near(fade_out_gain(0.5, FadeCurve::Logarithmic), 0.740_362_689_494_243_9);
    near(fade_in_gain(0.25, FadeCurve::Linear), 0.25);
    near(fade_out_gain(0.25, FadeCurve::Linear), 0.75);
    near(fade_in_gain(0.25, FadeCurve::Logarithmic), 0.511_883_360_978_874_4);
    near(fade_out_gain(0.25, FadeCurve::Logarithmic), 0.889_301_702_506_31);
    near(fade_in_gain(0.25, FadeCurve::Cosine), 0.146_446_609_406_726_2);
    near(fade_out_gain(0.25, FadeCurve::Cosine), 0.853_553_390_593_273_7);
}

#[test]
fn e1_curve_gain_matches_formula_to_hundredth_db_at_100_points() {
    for k in 1..=100 {
        let t = f64::from(k) / 100.0;
        let cases = [(FadeCurve::Linear, t), (FadeCurve::Logarithmic, (1.0 + 9.0 * t).log10()), (FadeCurve::Cosine, 0.5 * (1.0 - (PI * t).cos()))];
        for (curve, want) in cases {
            let got_in = fade_in_gain(t, curve);
            let got_out = fade_out_gain(1.0 - t, curve);
            assert!(got_in > 0.0 && got_out > 0.0, "{curve:?} point {k}");
            let db_in = (20.0 * (got_in / want).log10()).abs();
            let db_out = (20.0 * (got_out / want).log10()).abs();
            assert!(db_in <= 0.01, "{curve:?} fade_in at {t}: {db_in} dB");
            assert!(db_out <= 0.01, "{curve:?} fade_out at {}: {db_out} dB", 1.0 - t);
        }
    }
}

#[test]
fn e1_curve_gain_clamps_out_of_range_and_rejects_non_finite() {
    for c in [FadeCurve::Linear, FadeCurve::Logarithmic, FadeCurve::Cosine] {
        near(fade_in_gain(-0.5, c), 0.0);
        near(fade_in_gain(1.5, c), 1.0);
        near(fade_out_gain(-0.5, c), 1.0);
        near(fade_out_gain(1.5, c), 0.0);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(fade_in_gain(bad, c), 0.0);
            assert_eq!(fade_out_gain(bad, c), 0.0);
        }
    }
}
