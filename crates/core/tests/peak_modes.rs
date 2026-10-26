//! A214: static/dynamic peak mode, settable hold time and valley tracking on `Levels`.

use wavcraft_core::levels::{FLOOR_DB, HOLD_SECS, Levels, MAX_HOLD_SECS, PeakMode, SILENT_DB};

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn quiet(l: &mut Levels, updates: usize) {
    for _ in 0..updates {
        l.update([0.0, 0.0], 0.05);
    }
}

#[test]
fn defaults_leave_the_old_behaviour() {
    let mut l = Levels::new();
    assert_eq!(l.peak_mode(), PeakMode::Dynamic);
    assert_eq!(l.hold_secs(), HOLD_SECS);
    assert_eq!(l.valley_db(0), None);
    assert_eq!(l.valley_db(1), None);
    l.update([0.5, 0.25], 0.05);
    quiet(&mut l, 50);
    for ch in 0..2 {
        assert_eq!(l.shown_hold_db(ch), l.reading(ch).hold_db);
    }
}

#[test]
fn dynamic_mode_lets_the_tick_fall() {
    let mut l = Levels::new();
    l.update([0.5, 0.0], 0.05);
    assert!(near(l.shown_hold_db(0), db(0.5)));
    quiet(&mut l, 400);
    assert!(l.shown_hold_db(0) < -50.0, "{}", l.shown_hold_db(0));
}

#[test]
fn static_mode_keeps_the_highest_peak_per_channel() {
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    assert_eq!(l.peak_mode(), PeakMode::Static);
    l.update([0.2, 0.25], 0.05);
    l.update([0.5, 0.1], 0.05);
    l.update([0.1, 0.05], 0.05);
    quiet(&mut l, 400);
    assert!(near(l.shown_hold_db(0), db(0.5)), "{}", l.shown_hold_db(0));
    assert!(near(l.shown_hold_db(1), db(0.25)), "{}", l.shown_hold_db(1));
    // The meter's own tick still fell: only the shown tick is static.
    assert!(l.reading(0).hold_db < -50.0);
}

#[test]
fn static_peaks_are_tracked_while_dynamic_and_shown_on_switching() {
    let mut l = Levels::new();
    l.update([0.5, 0.0], 0.05);
    quiet(&mut l, 400);
    assert!(l.shown_hold_db(0) < -50.0);
    l.set_peak_mode(PeakMode::Static);
    assert!(near(l.shown_hold_db(0), db(0.5)), "{}", l.shown_hold_db(0));
    l.set_peak_mode(PeakMode::Dynamic);
    assert!(l.shown_hold_db(0) < -50.0);
}

#[test]
fn a_static_peak_starts_silent_and_resets() {
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    assert_eq!(l.shown_hold_db(0), SILENT_DB);
    l.update([0.5, 0.5], 0.05);
    l.reset_static_peaks();
    assert_eq!(l.shown_hold_db(0), SILENT_DB);
    assert_eq!(l.shown_hold_db(1), SILENT_DB);
    l.update([0.25, 0.0], 0.05);
    assert!(near(l.shown_hold_db(0), db(0.25)));
    l.reset();
    assert_eq!(l.shown_hold_db(0), SILENT_DB);
}

#[test]
fn resetting_static_peaks_leaves_valleys_and_resetting_valleys_leaves_peaks() {
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    l.update([0.5, 0.5], 0.05);
    l.update([0.1, 0.1], 0.05);
    l.reset_static_peaks();
    assert!(near(l.valley_db(0).unwrap_or(0.0), db(0.1)));
    l.update([0.5, 0.5], 0.05);
    l.reset_valleys();
    assert_eq!(l.valley_db(0), None);
    assert!(near(l.shown_hold_db(0), db(0.5)));
}

#[test]
fn a_longer_hold_time_keeps_the_dynamic_tick_up() {
    let mut l = Levels::new();
    l.set_hold_secs(3.0);
    assert_eq!(l.hold_secs(), 3.0);
    l.update([0.5, 0.0], 0.05);
    quiet(&mut l, 40);
    assert!(near(l.shown_hold_db(0), db(0.5)), "{}", l.shown_hold_db(0));
}

#[test]
fn a_shorter_hold_time_lets_it_fall_sooner() {
    let mut l = Levels::new();
    l.set_hold_secs(0.5);
    l.update([0.5, 0.0], 0.05);
    quiet(&mut l, 40);
    assert!(l.shown_hold_db(0) < -20.0, "{}", l.shown_hold_db(0));
}

#[test]
fn hold_time_is_clamped_and_bad_values_are_ignored() {
    let mut l = Levels::new();
    l.set_hold_secs(1000.0);
    assert_eq!(l.hold_secs(), MAX_HOLD_SECS);
    l.set_hold_secs(-2.0);
    assert_eq!(l.hold_secs(), 0.0);
    l.set_hold_secs(2.5);
    l.set_hold_secs(f32::NAN);
    assert_eq!(l.hold_secs(), 2.5);
    l.set_hold_secs(f32::INFINITY);
    assert_eq!(l.hold_secs(), 2.5);
}

#[test]
fn a_zero_hold_time_never_holds() {
    let mut l = Levels::new();
    l.set_hold_secs(0.0);
    l.update([0.5, 0.0], 0.05);
    quiet(&mut l, 40);
    assert!(l.shown_hold_db(0) < -20.0, "{}", l.shown_hold_db(0));
}

#[test]
fn valleys_are_the_lowest_level_seen_per_channel() {
    let mut l = Levels::new();
    assert_eq!(l.valley_db(0), None);
    l.update([0.5, 0.5], 0.05);
    assert!(near(l.valley_db(0).unwrap_or(0.0), db(0.5)));
    l.update([0.1, 0.4], 0.05);
    l.update([0.3, 0.2], 0.05);
    l.update([0.9, 0.9], 0.05);
    assert!(near(l.valley_db(0).unwrap_or(0.0), db(0.1)), "{:?}", l.valley_db(0));
    assert!(near(l.valley_db(1).unwrap_or(0.0), db(0.2)), "{:?}", l.valley_db(1));
}

#[test]
fn a_valley_never_goes_below_the_floor_and_silence_reads_the_floor() {
    let mut l = Levels::new();
    l.update([0.5, 0.001], 0.05);
    l.update([0.0, 0.5], 0.05);
    assert_eq!(l.valley_db(0), Some(FLOOR_DB));
    assert_eq!(l.valley_db(1), Some(FLOOR_DB));
}

#[test]
fn valleys_reset_by_themselves_and_with_reset() {
    let mut l = Levels::new();
    l.update([0.1, 0.1], 0.05);
    l.reset_valleys();
    assert_eq!(l.valley_db(0), None);
    assert_eq!(l.valley_db(1), None);
    l.update([0.3, 0.3], 0.05);
    assert!(near(l.valley_db(1).unwrap_or(0.0), db(0.3)));
    l.reset();
    assert_eq!(l.valley_db(0), None);
}

#[test]
fn bad_peaks_count_as_silence_and_channels_clamp() {
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    l.update([f32::NAN, -1.0], 0.05);
    assert_eq!(l.shown_hold_db(0), SILENT_DB);
    assert_eq!(l.shown_hold_db(1), SILENT_DB);
    assert_eq!(l.valley_db(0), Some(FLOOR_DB));
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    l.update([0.1, 0.5], 0.05);
    assert_eq!(l.shown_hold_db(9), l.shown_hold_db(1));
    assert_eq!(l.valley_db(9), l.valley_db(1));
}

#[test]
fn a_static_tick_does_not_keep_the_window_repainting() {
    let mut l = Levels::new();
    l.set_peak_mode(PeakMode::Static);
    l.update([0.5, 0.5], 0.05);
    assert!(l.active());
    quiet(&mut l, 400);
    assert!(!l.active());
}
