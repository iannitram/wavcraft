//! Locked tests for wav-c-22-a (v1 C3): time-ruler tick step and nearest-tick frame arithmetic.

use wavcraft_core::snap_ruler::{MAJOR_TICK_PX, STEPS, ruler_step, ruler_tick_near};

#[test]
fn c3_ruler_step_below_first_entry_returns_one_millisecond() {
    assert_eq!(MAJOR_TICK_PX, 110.0);
    assert_eq!(STEPS.len(), 19);
    assert_eq!(ruler_step(-5.0), 0.001);
    assert_eq!(ruler_step(0.0), 0.001);
    assert_eq!(ruler_step(0.0004), 0.001);
    assert_eq!(ruler_step(0.001), 0.001);
}

#[test]
fn c3_ruler_step_picks_first_round_step_at_least_min_seconds() {
    assert_eq!(ruler_step(0.0011), 0.002);
    assert_eq!(ruler_step(0.003), 0.005);
    assert_eq!(ruler_step(0.07), 0.1);
    assert_eq!(ruler_step(0.7), 1.0);
    assert_eq!(ruler_step(2.0), 2.0);
    assert_eq!(ruler_step(15.0), 30.0);
    assert_eq!(ruler_step(45.0), 60.0);
    assert_eq!(ruler_step(601.0), 1800.0);
}

#[test]
fn c3_ruler_step_above_half_hour_falls_back_to_one_hour() {
    assert_eq!(ruler_step(1800.0), 1800.0);
    assert_eq!(ruler_step(1800.1), 3600.0);
    assert_eq!(ruler_step(5000.0), 3600.0);
}

#[test]
fn c3_ruler_tick_near_rounds_to_nearest_one_second_tick_at_48khz() {
    // At 48 kHz and 300 frames/px, 300 / 48_000 * 110 = 0.6875 s -> 1.0 s step (48_000 frames).
    assert_eq!(ruler_tick_near(0, 300.0, 48_000), 0);
    assert_eq!(ruler_tick_near(23_999, 300.0, 48_000), 0);
    assert_eq!(ruler_tick_near(24_000, 300.0, 48_000), 48_000);
    assert_eq!(ruler_tick_near(47_500, 300.0, 48_000), 48_000);
    assert_eq!(ruler_tick_near(120_100, 300.0, 48_000), 144_000);
}

#[test]
fn c3_ruler_tick_near_uses_zoom_scale_to_pick_step() {
    // Close zoom: 4 frames/px at 48 kHz -> 4 / 48_000 * 110 = 0.009166... s -> 0.01 s (480 frames).
    assert_eq!(ruler_tick_near(700, 4.0, 48_000), 480);
    assert_eq!(ruler_tick_near(730, 4.0, 48_000), 960);
    // Wide zoom: 4_800 frames/px at 48 kHz -> 4_800 / 48_000 * 110 = 11.0 s -> 30.0 s (1_440_000 frames).
    assert_eq!(ruler_tick_near(2_200_000, 4_800.0, 48_000), 2_880_000);
}

#[test]
fn c3_ruler_tick_near_zero_sample_rate_clamps_to_one_hz() {
    // sample_rate 0 is treated as 1 Hz: 0.005 / 1.0 * 110 = 0.55 s -> 1.0 s step (1 frame).
    assert_eq!(ruler_tick_near(7, 0.005, 0), 7);
}
