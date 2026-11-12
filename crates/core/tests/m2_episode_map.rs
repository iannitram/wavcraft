//! M2 (issue #1052): the Episode Map's classes and the placing of pins and spans on its strip.

use wavcraft_core::episode_map::{Class, classify, columns, pin_x, span_x};

/// M2.1: one class per peak column; silence under -60 dBFS, loud from -6 dBFS.
#[test]
fn columns_are_classed_by_peak() {
    assert_eq!(columns(&[(0.0, 0.0), (-0.1, 0.1), (-0.9, 0.9)]), vec![Class::Silence, Class::Speech, Class::Loud]);
    assert_eq!(classify(-60.0), Class::Speech);
    assert_eq!(classify(-6.0), Class::Loud);
    assert!(columns(&[]).is_empty());
    // Unreadable values: NaN and minus infinity are silence; a NaN side is ignored.
    assert_eq!(classify(f32::NAN), Class::Silence);
    assert_eq!(classify(f32::NEG_INFINITY), Class::Silence);
    assert_eq!(columns(&[(f32::NAN, f32::NAN), (f32::NAN, 0.1), (-0.9, f32::NAN)]), vec![Class::Silence, Class::Speech, Class::Loud]);
}

/// M2.2: markers and findings land on the columns their frames are in.
#[test]
fn pins_land_on_their_columns() {
    assert_eq!(pin_x(24_000, 48_000, 100.0), 50.0);
    assert_eq!(span_x(0, 480, 48_000, 100.0), (0.0, 1.0));
    assert_eq!(pin_x(24_000, 0, 100.0), 0.0);
    // Past the end lands on the right edge; a reversed span comes back in order; no width, no x.
    assert_eq!(pin_x(96_000, 48_000, 100.0), 100.0);
    assert_eq!(span_x(480, 0, 48_000, 100.0), (0.0, 1.0));
    assert_eq!(pin_x(24_000, 48_000, f32::NAN), 0.0);
    assert_eq!(pin_x(24_000, 48_000, -5.0), 0.0);
}
