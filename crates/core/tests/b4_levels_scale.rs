//! Locked tests for wav-c-118-a (#118, v1 item B4): the Levels meter's 60 dB scale and its colour
//! zones with crossovers at -18 and -6 dBFS. Expected values are fixed here; do not edit them.

use wavcraft_core::meters::levels_scale::{RANGE_DB, RED_FROM_DB, YELLOW_FROM_DB, Zone, bar_fraction, zone};

fn close(got: f32, want: f32) {
    assert!((got - want).abs() <= 1e-6, "got {got}, want {want}");
}

#[test]
fn b4_scale_constants_are_the_audition_values() {
    close(RANGE_DB, 60.0);
    close(YELLOW_FROM_DB, -18.0);
    close(RED_FROM_DB, -6.0);
}

#[test]
fn b4_scale_bar_fraction_is_linear_in_db_over_60_db() {
    close(bar_fraction(-60.0), 0.0);
    close(bar_fraction(-30.0), 0.5);
    close(bar_fraction(-18.0), 0.7);
    close(bar_fraction(-6.0), 0.9);
    close(bar_fraction(0.0), 1.0);
}

#[test]
fn b4_scale_bar_fraction_clamps_outside_the_range() {
    close(bar_fraction(-61.0), 0.0);
    close(bar_fraction(-144.0), 0.0);
    close(bar_fraction(3.0), 1.0);
    close(bar_fraction(f32::NEG_INFINITY), 0.0);
    close(bar_fraction(f32::INFINITY), 1.0);
}

#[test]
fn b4_scale_bar_fraction_of_nan_is_empty() {
    close(bar_fraction(f32::NAN), 0.0);
}

#[test]
fn b4_scale_zone_crossovers_take_the_upper_zone() {
    assert_eq!(zone(-18.0), Zone::Yellow);
    assert_eq!(zone(-18.01), Zone::Green);
    assert_eq!(zone(-6.0), Zone::Red);
    assert_eq!(zone(-6.01), Zone::Yellow);
}

#[test]
fn b4_scale_zone_of_far_and_odd_levels() {
    assert_eq!(zone(-60.0), Zone::Green);
    assert_eq!(zone(-144.0), Zone::Green);
    assert_eq!(zone(-12.0), Zone::Yellow);
    assert_eq!(zone(0.0), Zone::Red);
    assert_eq!(zone(6.0), Zone::Red);
    assert_eq!(zone(f32::NEG_INFINITY), Zone::Green);
    assert_eq!(zone(f32::INFINITY), Zone::Red);
    assert_eq!(zone(f32::NAN), Zone::Green);
}
