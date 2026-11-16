//! B4.3: the level tap carries energy (sum of squares and frame count) as well as peaks.

use wavcraft_core::levels::LevelTap;

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
