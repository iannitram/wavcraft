//! C4.5: the lasso outline and its mask.

use wavcraft_core::Selection;
use wavcraft_core::view::{Band, LassoSelection, SpectralSelection};

const RATE: u32 = 48_000;
const NYQ: f32 = 24_000.0;

fn rectangle(feather_hz: f32, feather_ms: f32) -> LassoSelection {
    LassoSelection::new(&[(1000, 900.0), (2000, 900.0), (2000, 1100.0), (1000, 1100.0)], NYQ, feather_hz, feather_ms, RATE)
}

#[test]
fn a_rectangle_lasso_equals_the_marquee() {
    let lasso = rectangle(0.0, 0.0);
    assert_eq!(lasso.mask(1500, 1000.0), 1.0);
    assert_eq!(lasso.mask(500, 1000.0), 0.0);
    assert_eq!(lasso.mask(1500, 2000.0), 0.0);

    let marquee = SpectralSelection { time: Selection { anchor: 1000, head: 2000 }, band: Band::new(900.0, 1100.0, NYQ), rate: RATE };
    for (frame, hz) in [(1500, 1000.0), (500, 1000.0), (1500, 2000.0), (1500, 950.0)] {
        assert_eq!(lasso.mask(frame, hz), marquee.mask(frame, hz), "at {frame},{hz}");
    }
}

#[test]
fn a_triangle_knows_inside_from_outside() {
    let triangle = LassoSelection::new(&[(0, 0.0), (1000, 0.0), (0, 1000.0)], NYQ, 0.0, 0.0, RATE);
    assert_eq!(triangle.mask(100, 100.0), 1.0);
    assert_eq!(triangle.mask(900, 900.0), 0.0);
}

#[test]
fn a_feather_fades_outside_the_outline() {
    let lasso = rectangle(100.0, 0.0);
    assert_eq!(lasso.mask(1500, 1000.0), 1.0);
    assert!((lasso.mask(1500, 850.0) - 0.5).abs() < 1e-6, "middle of the feather");
    assert_eq!(lasso.mask(1500, 800.0), 0.0, "the feather ends");
    let near = lasso.mask(1500, 880.0);
    assert!(near > 0.5 && near < 1.0, "between the middle and the edge: {near}");
}

#[test]
fn too_few_points_select_nothing() {
    let two = LassoSelection::new(&[(0, 0.0), (10, 10.0)], NYQ, 0.0, 0.0, RATE);
    assert_eq!(two.mask(5, 5.0), 0.0);
    let none = LassoSelection::new(&[], NYQ, 0.0, 0.0, RATE);
    assert_eq!(none.mask(5, 5.0), 0.0);
}
