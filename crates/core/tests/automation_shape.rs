//! Roadmap C179: segment shapes (linear, step, hold) for automation points.

use wavcraft_core::multitrack::automation_shape::{SegmentShape, SegmentShapes, set_segment_shape, shape_value};
use wavcraft_core::multitrack::soundcraft_model::{AutoParam, AutomationLane};

const ALL: [SegmentShape; 3] = [SegmentShape::Linear, SegmentShape::Step, SegmentShape::Hold];

fn lane_of(n: usize) -> AutomationLane {
    let mut lane = AutomationLane::new(AutoParam::Volume);
    for i in 0..n {
        lane.set_point(i as i64 * 48_000, -(i as f32));
    }
    assert_eq!(lane.points.len(), n);
    lane
}

fn v(shape: SegmentShape, t: f64) -> f32 {
    shape_value(shape, 0.0, -12.0, 2.0, 0.0, t).unwrap()
}

#[test]
fn linear_ramps_between_the_two_values() {
    assert_eq!(v(SegmentShape::Linear, 0.0), -12.0);
    assert_eq!(v(SegmentShape::Linear, 0.5), -9.0);
    assert_eq!(v(SegmentShape::Linear, 1.0), -6.0);
    assert_eq!(v(SegmentShape::Linear, 1.5), -3.0);
    assert_eq!(v(SegmentShape::Linear, 2.0), 0.0);
}

#[test]
fn linear_works_downward_and_from_a_later_start() {
    assert_eq!(shape_value(SegmentShape::Linear, 0.0, 0.0, 2.0, -12.0, 0.5).unwrap(), -3.0);
    assert_eq!(shape_value(SegmentShape::Linear, 1.0, 2.0, 3.0, 6.0, 1.5).unwrap(), 3.0);
    assert_eq!(shape_value(SegmentShape::Linear, 1.0, 2.0, 3.0, 6.0, 2.0).unwrap(), 4.0);
}

#[test]
fn step_jumps_at_the_middle_of_the_segment() {
    assert_eq!(v(SegmentShape::Step, 0.0), -12.0);
    assert_eq!(v(SegmentShape::Step, 0.5), -12.0);
    assert_eq!(v(SegmentShape::Step, 0.99), -12.0);
    assert_eq!(v(SegmentShape::Step, 1.0), 0.0);
    assert_eq!(v(SegmentShape::Step, 1.5), 0.0);
    assert_eq!(v(SegmentShape::Step, 2.0), 0.0);
    assert_eq!(shape_value(SegmentShape::Step, 1.0, 2.0, 3.0, 6.0, 1.9).unwrap(), 2.0);
    assert_eq!(shape_value(SegmentShape::Step, 1.0, 2.0, 3.0, 6.0, 2.0).unwrap(), 6.0);
}

#[test]
fn hold_keeps_the_first_value_until_the_next_point() {
    assert_eq!(v(SegmentShape::Hold, 0.0), -12.0);
    assert_eq!(v(SegmentShape::Hold, 1.0), -12.0);
    assert_eq!(v(SegmentShape::Hold, 1.999), -12.0);
    assert_eq!(v(SegmentShape::Hold, 2.0), 0.0);
    assert_eq!(shape_value(SegmentShape::Hold, 1.0, 2.0, 3.0, 6.0, 2.9).unwrap(), 2.0);
    assert_eq!(shape_value(SegmentShape::Hold, 1.0, 2.0, 3.0, 6.0, 3.0).unwrap(), 6.0);
}

#[test]
fn a_time_outside_the_segment_is_clamped_to_its_ends() {
    for shape in ALL {
        assert_eq!(v(shape, -5.0), -12.0, "{shape:?} before");
        assert_eq!(v(shape, 7.0), 0.0, "{shape:?} after");
        assert_eq!(v(shape, f64::MAX), 0.0, "{shape:?} far after");
        assert_eq!(v(shape, -f64::MAX), -12.0, "{shape:?} far before");
    }
}

#[test]
fn a_segment_of_zero_length_gives_the_first_value() {
    for shape in ALL {
        for t in [0.5, 1.0, 2.0] {
            let got = shape_value(shape, 1.0, -12.0, 1.0, 0.0, t).unwrap();
            assert_eq!(got, -12.0, "{shape:?} at {t}");
        }
    }
}

#[test]
fn a_segment_that_ends_before_it_starts_is_refused() {
    for shape in ALL {
        let e = shape_value(shape, 2.0, -12.0, 1.0, 0.0, 1.5).unwrap_err();
        assert!(e.0.contains("before"), "{e}");
    }
}

#[test]
fn nan_and_infinity_are_refused_in_every_argument() {
    for shape in ALL {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let cases = [
                shape_value(shape, bad, -12.0, 2.0, 0.0, 1.0),
                shape_value(shape, 0.0, -12.0, bad, 0.0, 1.0),
                shape_value(shape, 0.0, -12.0, 2.0, 0.0, bad),
            ];
            for r in cases {
                let e = r.unwrap_err();
                assert!(e.0.contains("number"), "{shape:?} {bad}: {e}");
            }
        }
        for bad in [f32::NAN, f32::INFINITY] {
            let e = shape_value(shape, 0.0, bad, 2.0, 0.0, 1.0).unwrap_err();
            assert!(e.0.contains("number"), "{e}");
            let e = shape_value(shape, 0.0, -12.0, 2.0, bad, 1.0).unwrap_err();
            assert!(e.0.contains("number"), "{e}");
        }
    }
}

#[test]
fn nan_is_refused_even_for_a_segment_of_zero_length() {
    let e = shape_value(SegmentShape::Linear, 1.0, f32::NAN, 1.0, 0.0, 1.0).unwrap_err();
    assert!(e.0.contains("number"), "{e}");
    let e = shape_value(SegmentShape::Linear, 1.0, 0.0, 1.0, 0.0, f64::NAN).unwrap_err();
    assert!(e.0.contains("number"), "{e}");
}

#[test]
fn a_fresh_set_is_all_linear() {
    let s = SegmentShapes::new();
    assert_eq!(s, SegmentShapes::default());
    assert_eq!(SegmentShape::default(), SegmentShape::Linear);
    for i in [0, 1, 7, usize::MAX] {
        assert_eq!(s.get(i), SegmentShape::Linear);
    }
}

#[test]
fn setting_a_shape_stores_it_for_that_segment_only() {
    let lane = lane_of(4);
    let mut s = SegmentShapes::new();
    set_segment_shape(&lane, &mut s, 2, SegmentShape::Hold).unwrap();
    assert_eq!(s.get(0), SegmentShape::Linear);
    assert_eq!(s.get(1), SegmentShape::Linear);
    assert_eq!(s.get(2), SegmentShape::Hold);
    set_segment_shape(&lane, &mut s, 0, SegmentShape::Step).unwrap();
    assert_eq!(s.get(0), SegmentShape::Step);
    assert_eq!(s.get(1), SegmentShape::Linear);
    assert_eq!(s.get(2), SegmentShape::Hold, "an earlier shape stays");
    set_segment_shape(&lane, &mut s, 2, SegmentShape::Linear).unwrap();
    assert_eq!(s.get(2), SegmentShape::Linear, "a shape can be set back");
    assert_eq!(s.get(0), SegmentShape::Step);
    assert_eq!(s.get(3), SegmentShape::Linear);
}

#[test]
fn the_last_valid_segment_is_one_before_the_last_point() {
    let lane = lane_of(4);
    let mut s = SegmentShapes::new();
    for i in 0..3 {
        set_segment_shape(&lane, &mut s, i, SegmentShape::Hold).unwrap();
    }
    let before = s.clone();
    for i in [3, 4, 5, 1000, usize::MAX] {
        let e = set_segment_shape(&lane, &mut s, i, SegmentShape::Step).unwrap_err();
        assert!(e.0.contains("0 to 2"), "index {i}: {e}");
        assert_eq!(s, before, "a refused set changes nothing");
    }
}

#[test]
fn a_lane_of_two_points_has_one_segment() {
    let lane = lane_of(2);
    let mut s = SegmentShapes::new();
    set_segment_shape(&lane, &mut s, 0, SegmentShape::Step).unwrap();
    assert_eq!(s.get(0), SegmentShape::Step);
    for i in [1, 2, 3] {
        let e = set_segment_shape(&lane, &mut s, i, SegmentShape::Hold).unwrap_err();
        assert!(e.0.contains("0 to 0"), "index {i}: {e}");
    }
    assert_eq!(s.get(1), SegmentShape::Linear);
}

#[test]
fn a_lane_with_fewer_than_two_points_has_no_segment() {
    for n in [0, 1] {
        let lane = lane_of(n);
        let mut s = SegmentShapes::new();
        for i in [0, 1] {
            let e = set_segment_shape(&lane, &mut s, i, SegmentShape::Hold).unwrap_err();
            assert!(e.0.contains("2 points"), "{n} points, index {i}: {e}");
        }
        assert_eq!(s, SegmentShapes::new());
    }
}

#[test]
fn a_lane_read_with_its_shapes_follows_each_segment() {
    // Points at 0 s, 1 s, 2 s (48 kHz) holding 0, -1, -2 dB.
    let lane = lane_of(3);
    let mut s = SegmentShapes::new();
    set_segment_shape(&lane, &mut s, 0, SegmentShape::Hold).unwrap();
    set_segment_shape(&lane, &mut s, 1, SegmentShape::Step).unwrap();
    let at = |i: usize, t: f64| {
        let (a, b) = (lane.points[i], lane.points[i + 1]);
        shape_value(s.get(i), a.at as f64, a.value, b.at as f64, b.value, t).unwrap()
    };
    assert_eq!(at(0, 36_000.0), 0.0, "held");
    assert_eq!(at(0, 48_000.0), -1.0);
    assert_eq!(at(1, 60_000.0), -1.0, "first half of the step");
    assert_eq!(at(1, 72_000.0), -2.0, "second half of the step");
}
