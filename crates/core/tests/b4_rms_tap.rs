//! B4.3: the level tap carries energy (sum of squares and frame count) as well as peaks.
//! B4.4: `RmsLevels::feed_energy` feeds the 300 ms window from block totals.

use wavcraft_core::levels::rms::RmsLevels;
use wavcraft_core::levels::{FLOOR_DB, LevelTap};

#[test]
fn energy_adds_up_and_take_starts_over() {
    let tap = LevelTap::default();
    tap.record_energy(0, 120.0, 480);
    tap.record_energy(0, 120.0, 480);
    let first = tap.take_energy();
    assert_eq!(first[0], (240.0, 960));
    assert_eq!(first[1], (0.0, 0));
    assert_eq!(tap.take_energy()[0], (0.0, 0));
}

#[test]
fn bad_energy_is_ignored_and_peaks_are_separate() {
    let tap = LevelTap::default();
    tap.record_energy(2, 1.0, 10);
    tap.record_energy(0, f32::NAN, 10);
    tap.record_energy(0, -1.0, 10);
    tap.record_energy(0, f32::INFINITY, 10);
    tap.record_energy(1, 5.0, 0);
    tap.record(0, 0.5);
    assert_eq!(tap.take_energy(), [(0.0, 0), (0.0, 0)]);
    assert_eq!(tap.take(), [0.5, 0.0]);
}

#[test]
fn an_overflowing_sum_counts_no_frames() {
    let tap = LevelTap::default();
    tap.record_energy(0, f32::MAX, 10);
    // The second sum would overflow to infinity, so the tap refuses it: its frames are not counted.
    tap.record_energy(0, f32::MAX, 10);
    assert_eq!(tap.take_energy()[0], (f32::MAX, 10));
}

#[test]
fn a_block_of_half_power_reads_minus_three_db() {
    let mut l = RmsLevels::new(48_000.0, 300.0);
    assert_eq!(l.window(), 14_400);
    l.feed_energy(0, 7_200.0, 14_400);
    assert!((l.reading(0).rms_db - (-3.01)).abs() < 0.01, "{:?}", l.reading(0));
    l.feed_energy(0, 0.0, 14_400);
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
}

#[test]
fn a_block_longer_than_the_window_and_bad_numbers_are_safe() {
    let mut l = RmsLevels::new(48_000.0, 300.0);
    l.feed_energy(1, f64::NAN, 100);
    l.feed_energy(1, -5.0, 100);
    assert_eq!(l.reading(1).rms_db, FLOOR_DB);
    l.feed_energy(0, 5.0, 0);
    assert_eq!(l.reading(0).rms_db, FLOOR_DB);
    let mut huge = RmsLevels::new(48_000.0, 300.0);
    huge.feed_energy(0, 1.0e300, 1);
    assert!(huge.reading(0).rms_db.is_finite());
    // 14 400 total energy over a block of a million frames: only the newest window counts, so the
    // mean square is 14 400 / 1 000 000 = 0.0144, which is -18.42 dB.
    let mut long = RmsLevels::new(48_000.0, 300.0);
    long.feed_energy(0, 14_400.0, 1_000_000);
    assert!((long.reading(0).rms_db - (-18.42)).abs() < 0.01, "{:?}", long.reading(0));
}
