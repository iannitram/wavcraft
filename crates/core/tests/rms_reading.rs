//! A217 (core): the RMS level reading beside the peak meter.

use wavcraft_core::levels::FLOOR_DB;
use wavcraft_core::levels::rms::{MAX_WINDOW, RMS_WINDOW_MS, RmsLevels, RmsMeter, mean_square_to_db, window_samples};

fn sine(amp: f32, period: usize, n: usize) -> Vec<f32> {
    (0..n).map(|i| amp * (std::f32::consts::TAU * i as f32 / period as f32).sin()).collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn mean_square_converts_to_ten_log() {
    assert!(near(mean_square_to_db(1.0), 0.0));
    assert!(near(mean_square_to_db(0.5), -3.0103));
    assert!(near(mean_square_to_db(0.25), -6.0206));
    assert!(near(mean_square_to_db(0.01), -20.0));
}

#[test]
fn nothing_computable_or_too_quiet_reads_the_floor() {
    assert_eq!(mean_square_to_db(0.0), FLOOR_DB);
    assert_eq!(mean_square_to_db(-1.0), FLOOR_DB);
    assert_eq!(mean_square_to_db(f64::NAN), FLOOR_DB);
    assert_eq!(mean_square_to_db(f64::INFINITY), FLOOR_DB);
    assert_eq!(mean_square_to_db(1e-9), FLOOR_DB);
}

#[test]
fn window_samples_is_rate_times_milliseconds() {
    assert_eq!(window_samples(48_000.0, 300.0), 14_400);
    assert_eq!(window_samples(44_100.0, 10.0), 441);
    assert_eq!(window_samples(48_000.0, 0.0), 1);
    assert_eq!(window_samples(48_000.0, -5.0), 1);
    assert_eq!(window_samples(0.0, 300.0), 1);
    assert_eq!(window_samples(f32::NAN, 300.0), 1);
    assert_eq!(window_samples(48_000.0, f32::NAN), 1);
    assert_eq!(window_samples(44_100.0, 1.0), 44, "44.1 rounds to 44");
    assert_eq!(window_samples(44_100.0, 1.4), 62, "61.74 rounds to 62");
    assert_eq!(window_samples(48_000.0, 1.0e9), MAX_WINDOW);
}

#[test]
fn the_default_window_is_three_hundred_ms() {
    assert_eq!(RMS_WINDOW_MS, 300.0);
    assert_eq!(RmsLevels::default().window(), 14_400);
}

#[test]
fn a_new_meter_is_silent_and_the_window_is_limited() {
    assert_eq!(RmsMeter::new(480).window(), 480);
    assert_eq!(RmsMeter::new(480).rms_db(), FLOOR_DB);
    assert_eq!(RmsMeter::new(0).window(), 1);
    assert_eq!(RmsMeter::new(usize::MAX).window(), MAX_WINDOW);
}

#[test]
fn a_full_scale_sine_reads_minus_three_db() {
    let mut m = RmsMeter::new(480);
    m.feed(&sine(1.0, 48, 480));
    assert!(near(m.rms_db(), -3.0103), "{}", m.rms_db());
}

#[test]
fn a_constant_reads_its_level() {
    let mut m = RmsMeter::new(100);
    m.feed(&[0.5; 100]);
    assert!(near(m.rms_db(), -6.0206), "{}", m.rms_db());
    let mut n = RmsMeter::new(100);
    n.feed(&[-0.5; 100]);
    assert!(near(n.rms_db(), -6.0206), "sign does not matter: {}", n.rms_db());
}

#[test]
fn samples_not_fed_yet_count_as_silence() {
    let mut m = RmsMeter::new(100);
    m.feed(&[1.0; 50]);
    assert!(near(m.rms_db(), -3.0103), "{}", m.rms_db());
}

#[test]
fn the_window_slides_and_old_samples_leave() {
    let mut m = RmsMeter::new(100);
    m.feed(&[0.5; 100]);
    m.feed(&[0.0; 50]);
    assert!(near(m.rms_db(), -9.0309), "half the window is silent: {}", m.rms_db());
    m.feed(&[0.0; 50]);
    assert_eq!(m.rms_db(), FLOOR_DB, "all of the loud part has left");
    let mut k = RmsMeter::new(10);
    k.feed(&[1.0; 10]);
    k.feed(&[0.1; 10]);
    assert!(near(k.rms_db(), -20.0), "only the last 10 count: {}", k.rms_db());
}

#[test]
fn feeding_in_pieces_equals_feeding_at_once() {
    let x = sine(0.8, 37, 1000);
    let mut whole = RmsMeter::new(300);
    whole.feed(&x);
    let mut parts = RmsMeter::new(300);
    for chunk in x.chunks(77) {
        parts.feed(chunk);
    }
    assert!((whole.rms_db() - parts.rms_db()).abs() < 1e-4);
    let mut one_by_one = RmsMeter::new(300);
    for s in &x {
        one_by_one.feed(std::slice::from_ref(s));
    }
    assert!((whole.rms_db() - one_by_one.rms_db()).abs() < 1e-4);
}

#[test]
fn a_run_longer_than_the_window_only_keeps_the_end() {
    let mut m = RmsMeter::new(10);
    let mut x = vec![1.0; 25];
    x.extend([0.1; 10]);
    m.feed(&x);
    assert!(near(m.rms_db(), -20.0), "{}", m.rms_db());
}

#[test]
fn nan_and_infinite_samples_count_as_silence() {
    let mut m = RmsMeter::new(4);
    m.feed(&[1.0, f32::NAN, f32::INFINITY, 1.0]);
    assert!(near(m.rms_db(), -3.0103), "{}", m.rms_db());
    m.feed(&[f32::NEG_INFINITY; 4]);
    assert_eq!(m.rms_db(), FLOOR_DB);
}

#[test]
fn a_quiet_signal_reads_the_floor_not_below() {
    let mut m = RmsMeter::new(100);
    m.feed(&[0.0005; 100]);
    assert_eq!(m.rms_db(), FLOOR_DB);
}

#[test]
fn reset_and_set_window_start_over() {
    let mut m = RmsMeter::new(100);
    m.feed(&[0.5; 100]);
    m.reset();
    assert_eq!(m.rms_db(), FLOOR_DB);
    assert_eq!(m.window(), 100);
    m.feed(&[0.5; 100]);
    m.set_window(20);
    assert_eq!(m.window(), 20);
    assert_eq!(m.rms_db(), FLOOR_DB, "a new window starts silent");
    m.feed(&[0.5; 20]);
    assert!(near(m.rms_db(), -6.0206), "{}", m.rms_db());
    m.set_window(0);
    assert_eq!(m.window(), 1);
    m.set_window(usize::MAX);
    assert_eq!(m.window(), MAX_WINDOW);
}

#[test]
fn the_pair_keeps_the_channels_apart() {
    let mut l = RmsLevels::new(48_000.0, 300.0);
    assert_eq!(l.window(), 14_400);
    l.feed(0, &[0.5; 14_400]);
    assert!(near(l.reading(0).rms_db, -6.0206), "{:?}", l.reading(0));
    assert_eq!(l.reading(1).rms_db, FLOOR_DB);
    l.feed(1, &[1.0; 14_400]);
    assert!(near(l.reading(1).rms_db, 0.0), "{:?}", l.reading(1));
    assert!(near(l.reading(0).rms_db, -6.0206), "channel 0 unchanged");
}

#[test]
fn a_channel_number_past_one_means_channel_one() {
    let mut l = RmsLevels::new(48_000.0, 10.0);
    assert_eq!(l.window(), 480);
    l.feed(7, &[0.5; 480]);
    assert!(near(l.reading(1).rms_db, -6.0206));
    assert!(near(l.reading(9).rms_db, -6.0206));
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
}

#[test]
fn the_pair_sets_one_window_for_both_and_starts_over() {
    let mut l = RmsLevels::new(44_100.0, 10.0);
    assert_eq!(l.window(), 441);
    l.feed(0, &[0.5; 441]);
    l.feed(1, &[0.5; 441]);
    l.set_window_ms(20.0);
    assert_eq!(l.window(), 882, "44100 Hz keeps its own rate");
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
    assert_eq!(l.reading(1).rms_db, FLOOR_DB);
    l.feed(1, &[0.5; 882]);
    assert!(near(l.reading(1).rms_db, -6.0206));
}

#[test]
fn the_pair_resets_both_channels_and_keeps_the_window() {
    let mut l = RmsLevels::new(48_000.0, 10.0);
    l.feed(0, &[0.5; 480]);
    l.feed(1, &[0.5; 480]);
    l.reset();
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
    assert_eq!(l.reading(1).rms_db, FLOOR_DB);
    assert_eq!(l.window(), 480);
}

#[test]
fn reset_forgets_the_old_squares() {
    let mut m = RmsMeter::new(100);
    m.feed(&[0.5; 100]);
    m.reset();
    m.feed(&[0.5; 100]);
    assert!(near(m.rms_db(), -6.0206), "{}", m.rms_db());
    let mut l = RmsLevels::new(48_000.0, 10.0);
    l.feed(0, &[0.5; 480]);
    l.feed(1, &[0.5; 480]);
    l.reset();
    l.feed(0, &[0.5; 480]);
    l.feed(1, &[0.5; 480]);
    assert!(near(l.reading(0).rms_db, -6.0206));
    assert!(near(l.reading(1).rms_db, -6.0206));
}

#[test]
fn channel_two_and_up_is_channel_one_and_both_share_the_window() {
    let mut l = RmsLevels::new(1000.0, 4.0);
    assert_eq!(l.window(), 4);
    l.feed(1, &[0.5; 4]);
    assert!(near(l.reading(2).rms_db, -6.0206), "{:?}", l.reading(2));
    assert!(near(l.reading(4).rms_db, -6.0206));
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
    l.reset();
    l.feed(1, &[1.0; 4]);
    assert!(near(l.reading(1).rms_db, 0.0), "channel 1 has the same 4-sample window: {:?}", l.reading(1));
    l.feed(0, &[1.0; 4]);
    assert!(near(l.reading(0).rms_db, 0.0), "{:?}", l.reading(0));
}
