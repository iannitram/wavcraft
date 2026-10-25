//! Row C083: the multitrack time ruler as a pure model (`wavcraft_core::ruler`). Every expected value
//! below was worked out by hand and checked against an independent python reference.

use wavcraft_core::ruler::{MAX_SECONDS, MAX_TICKS, Ruler, RulerError, RulerFormat, format_label, nice_step, ruler_ticks, validate_ruler_input};

use RulerFormat::{Decimal, MinSec};

fn good<T>(r: Result<T, RulerError>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("unexpected error: {e}"),
    }
}

fn ruler(start: f64, end: f64, width: f64, spacing: f64, format: RulerFormat) -> Ruler {
    good(ruler_ticks(start, end, width, spacing, format))
}

fn times(r: &Ruler) -> Vec<f64> {
    r.ticks.iter().map(|t| t.seconds).collect()
}

fn majors(r: &Ruler) -> Vec<bool> {
    r.ticks.iter().map(|t| t.major).collect()
}

fn labels(r: &Ruler) -> Vec<String> {
    r.ticks.iter().map(|t| t.label.clone()).collect()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// `true` for T and `false` for any other letter.
fn flags(s: &str) -> Vec<bool> {
    s.chars().map(|c| c == 'T').collect()
}

#[test]
fn the_limits_are_the_stated_ones() {
    assert_eq!(MAX_TICKS, 2000);
    assert_eq!(MAX_SECONDS, 1.0e9);
}

#[test]
fn nice_step_decimal_is_one_two_or_five_times_a_power_of_ten() {
    let cases = [
        (0.0004, 0.001),
        (0.001, 0.001),
        (0.0011, 0.002),
        (0.003, 0.005),
        (0.05, 0.05),
        (0.06, 0.1),
        (0.2, 0.2),
        (0.21, 0.5),
        (1.0, 1.0),
        (1.0001, 2.0),
        (4.9, 5.0),
        (5.0, 5.0),
        (5.1, 10.0),
        (999.0, 1000.0),
        (4_000_000.0, 5_000_000.0),
    ];
    for (i, (min, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(nice_step(min, Decimal)), want, "case {i}: minimum {min}");
    }
}

#[test]
fn nice_step_decimal_tops_out_at_five_million_seconds() {
    assert_eq!(good(nice_step(5_000_000.0, Decimal)), 5_000_000.0);
    assert_eq!(nice_step(5_000_001.0, Decimal), Err(RulerError::StepOutOfRange));
    assert_eq!(nice_step(4_999_999.0, Decimal), Ok(5_000_000.0));
}

#[test]
fn nice_step_minsec_uses_the_clock_friendly_table() {
    let cases = [
        (0.0004, 0.001),
        (0.05, 0.05),
        (0.06, 0.1),
        (0.5, 0.5),
        (0.7, 1.0),
        (1.0, 1.0),
        (1.5, 2.0),
        (3.0, 5.0),
        (5.5, 10.0),
        (10.0, 10.0),
        (11.0, 15.0),
        (15.0, 15.0),
        (16.0, 30.0),
        (31.0, 60.0),
        (61.0, 120.0),
        (121.0, 300.0),
        (301.0, 600.0),
        (601.0, 900.0),
        (901.0, 1800.0),
        (1801.0, 3600.0),
        (3600.0, 3600.0),
    ];
    for (i, (min, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(nice_step(min, MinSec)), want, "case {i}: minimum {min}");
    }
}

#[test]
fn nice_step_minsec_tops_out_at_one_hour() {
    assert_eq!(nice_step(3600.5, MinSec), Err(RulerError::StepOutOfRange));
    assert_eq!(nice_step(3599.5, MinSec), Ok(3600.0));
    assert_eq!(nice_step(3601.0, MinSec), Err(RulerError::StepOutOfRange));
}

#[test]
fn nice_step_refuses_a_minimum_it_cannot_use() {
    for format in [Decimal, MinSec] {
        assert_eq!(nice_step(f64::NAN, format), Err(RulerError::NotFinite));
        assert_eq!(nice_step(f64::INFINITY, format), Err(RulerError::NotFinite));
        assert_eq!(nice_step(f64::NEG_INFINITY, format), Err(RulerError::NotFinite));
        assert_eq!(nice_step(0.0, format), Err(RulerError::NonPositiveStep));
        assert_eq!(nice_step(-1.0, format), Err(RulerError::NonPositiveStep));
    }
}

#[test]
fn decimal_labels_for_whole_second_steps() {
    let cases = [(12.0, "12s"), (0.0, "0s"), (2.6, "3s"), (2.4, "2s"), (123_456.0, "123456s")];
    for (i, (seconds, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(format_label(seconds, 1.0, Decimal)), want, "case {i}: {seconds}");
    }
    assert_eq!(good(format_label(123_456.0, 1000.0, Decimal)), "123456s");
}

#[test]
fn decimal_labels_show_as_many_digits_as_the_step_needs() {
    let cases = [
        (1.5, 0.5, "1.5s"),
        (0.7, 0.1, "0.7s"),
        (0.25, 0.05, "0.25s"),
        (0.05, 0.05, "0.05s"),
        (0.07, 0.01, "0.07s"),
        (3.002, 0.002, "3.002s"),
        (1.2346, 0.001, "1.235s"),
        (1.26, 0.1, "1.3s"),
        (0.0, 0.2, "0.0s"),
    ];
    for (i, (seconds, step, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(format_label(seconds, step, Decimal)), want, "case {i}: {seconds} at {step}");
    }
}

#[test]
fn decimal_digit_count_changes_exactly_at_1_s_100_ms_and_10_ms() {
    assert_eq!(good(format_label(2.0, 1.0, Decimal)), "2s");
    assert_eq!(good(format_label(2.0, 0.999, Decimal)), "2.0s");
    assert_eq!(good(format_label(1.0, 0.1, Decimal)), "1.0s");
    assert_eq!(good(format_label(1.0, 0.099, Decimal)), "1.00s");
    assert_eq!(good(format_label(1.0, 0.01, Decimal)), "1.00s");
    assert_eq!(good(format_label(1.0, 0.009, Decimal)), "1.000s");
    // The step is rounded to the nearest millisecond, not cut off or rounded up.
    assert_eq!(good(format_label(1.0, 0.0999996, Decimal)), "1.0s");
    assert_eq!(good(format_label(1.0, 0.0991, Decimal)), "1.00s");
}

#[test]
fn minsec_labels_for_steps_of_a_second_or_more() {
    let cases = [
        (0.0, 1.0, "0:00"),
        (5.0, 5.0, "0:05"),
        (65.0, 5.0, "1:05"),
        (90.0, 30.0, "1:30"),
        (59.6, 1.0, "1:00"),
        (3599.0, 1.0, "59:59"),
        (3600.0, 60.0, "1:00:00"),
        (3661.0, 1.0, "1:01:01"),
        (7325.0, 5.0, "2:02:05"),
        (36_000.0, 3600.0, "10:00:00"),
    ];
    for (i, (seconds, step, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(format_label(seconds, step, MinSec)), want, "case {i}: {seconds} at {step}");
    }
}

#[test]
fn minsec_labels_show_milliseconds_only_below_a_one_second_step() {
    let cases = [
        (0.0, 0.001, "0:00.000"),
        (0.5, 0.5, "0:00.500"),
        (61.25, 0.25, "1:01.250"),
        (3600.002, 0.002, "1:00:00.002"),
        (1.2346, 0.001, "0:01.235"),
        (59.9996, 0.001, "1:00.000"),
    ];
    for (i, (seconds, step, want)) in cases.into_iter().enumerate() {
        assert_eq!(good(format_label(seconds, step, MinSec)), want, "case {i}: {seconds} at {step}");
    }
    assert_eq!(good(format_label(2.0, 1.0, MinSec)), "0:02");
    assert_eq!(good(format_label(2.0, 0.999, MinSec)), "0:02.000");
}

#[test]
fn format_label_refuses_what_it_cannot_write() {
    assert_eq!(format_label(f64::NAN, 1.0, Decimal), Err(RulerError::NotFinite));
    assert_eq!(format_label(f64::INFINITY, 1.0, MinSec), Err(RulerError::NotFinite));
    assert_eq!(format_label(1.0, f64::NAN, Decimal), Err(RulerError::NotFinite));
    assert_eq!(format_label(1.0, f64::INFINITY, MinSec), Err(RulerError::NotFinite));
    assert_eq!(format_label(-0.001, 1.0, Decimal), Err(RulerError::NegativeTime));
    assert_eq!(format_label(1.0, 0.0, Decimal), Err(RulerError::NonPositiveStep));
    assert_eq!(format_label(1.0, -1.0, MinSec), Err(RulerError::NonPositiveStep));
    assert_eq!(format_label(999_999_999.0, 1.0, Decimal), Ok("999999999s".to_string()));
    assert_eq!(format_label(1.0e9, 1.0, Decimal), Ok("1000000000s".to_string()));
    assert_eq!(format_label(1_000_000_001.0, 1.0, Decimal), Err(RulerError::TimeTooLarge));
}

#[test]
fn validate_accepts_a_usable_range() {
    assert_eq!(validate_ruler_input(0.0, 10.0, 800.0, 80.0), Ok(()));
    assert_eq!(validate_ruler_input(0.0, 1.0e9, 0.5, 0.25), Ok(()));
    assert_eq!(validate_ruler_input(-0.0, 1.0, 1.0, 1.0), Ok(()));
}

#[test]
fn validate_refuses_numbers_that_are_not_finite() {
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let cases = [
        (nan, 10.0, 800.0, 80.0),
        (0.0, nan, 800.0, 80.0),
        (0.0, 10.0, nan, 80.0),
        (0.0, 10.0, 800.0, nan),
        (f64::NEG_INFINITY, 10.0, 800.0, 80.0),
        (0.0, inf, 800.0, 80.0),
        (0.0, 10.0, inf, 80.0),
        (0.0, 10.0, 800.0, inf),
    ];
    for (i, (start, end, width, spacing)) in cases.into_iter().enumerate() {
        assert_eq!(validate_ruler_input(start, end, width, spacing), Err(RulerError::NotFinite), "case {i}");
    }
}

#[test]
fn validate_refuses_a_negative_start() {
    assert_eq!(validate_ruler_input(-0.001, 10.0, 800.0, 80.0), Err(RulerError::NegativeTime));
}

#[test]
fn validate_refuses_an_empty_or_reversed_range() {
    assert_eq!(validate_ruler_input(5.0, 5.0, 800.0, 80.0), Err(RulerError::EmptyRange));
    assert_eq!(validate_ruler_input(5.0, 4.0, 800.0, 80.0), Err(RulerError::EmptyRange));
    assert_eq!(validate_ruler_input(5.0, 5.001, 800.0, 80.0), Ok(()));
}

#[test]
fn validate_refuses_times_past_the_limit() {
    assert_eq!(validate_ruler_input(0.0, 999_999_999.0, 800.0, 80.0), Ok(()));
    assert_eq!(validate_ruler_input(0.0, 1.0e9, 800.0, 80.0), Ok(()));
    assert_eq!(validate_ruler_input(0.0, 1_000_000_001.0, 800.0, 80.0), Err(RulerError::TimeTooLarge));
}

#[test]
fn validate_refuses_a_width_or_spacing_of_zero_or_less() {
    assert_eq!(validate_ruler_input(0.0, 10.0, 0.0, 80.0), Err(RulerError::ZeroWidth));
    assert_eq!(validate_ruler_input(0.0, 10.0, -1.0, 80.0), Err(RulerError::ZeroWidth));
    assert_eq!(validate_ruler_input(0.0, 10.0, 800.0, 0.0), Err(RulerError::ZeroSpacing));
    assert_eq!(validate_ruler_input(0.0, 10.0, 800.0, -1.0), Err(RulerError::ZeroSpacing));
}

#[test]
fn every_error_says_what_is_wrong() {
    let cases = [
        (RulerError::NotFinite, "ruler input is not a finite number"),
        (RulerError::NegativeTime, "ruler time is negative"),
        (RulerError::EmptyRange, "ruler range is empty or reversed: the end must be after the start"),
        (RulerError::ZeroWidth, "ruler width must be more than 0 pixels"),
        (RulerError::ZeroSpacing, "ruler label spacing must be more than 0 pixels"),
        (RulerError::TimeTooLarge, "ruler time is beyond 1000000000 seconds"),
        (RulerError::NonPositiveStep, "ruler step must be more than 0 seconds"),
        (RulerError::StepOutOfRange, "no ruler step is wide enough for that label spacing"),
        (RulerError::TooManyTicks, "that range needs more than 2000 ruler ticks"),
    ];
    for (i, (error, want)) in cases.into_iter().enumerate() {
        assert_eq!(error.to_string(), want, "case {i}");
    }
}

#[test]
fn ruler_ticks_checks_its_own_input() {
    let cases = [
        (f64::NAN, 10.0, 800.0, 80.0, RulerError::NotFinite),
        (0.0, f64::INFINITY, 800.0, 80.0, RulerError::NotFinite),
        (-1.0, 10.0, 800.0, 80.0, RulerError::NegativeTime),
        (5.0, 5.0, 800.0, 80.0, RulerError::EmptyRange),
        (6.0, 5.0, 800.0, 80.0, RulerError::EmptyRange),
        (0.0, 10.0, 0.0, 80.0, RulerError::ZeroWidth),
        (0.0, 10.0, 800.0, 0.0, RulerError::ZeroSpacing),
        (0.0, 1_000_000_001.0, 800.0, 80.0, RulerError::TimeTooLarge),
    ];
    for (i, (start, end, width, spacing, want)) in cases.into_iter().enumerate() {
        for format in [Decimal, MinSec] {
            assert_eq!(ruler_ticks(start, end, width, spacing, format), Err(want), "case {i}");
        }
    }
}

#[test]
fn decimal_ruler_one_second_steps() {
    // 8 s over 1024 px with 128 px per label: the minimum step is exactly 1 s.
    let r = ruler(0.0, 8.0, 1024.0, 128.0, Decimal);
    assert_eq!(r.step, 1.0);
    assert_eq!(times(&r), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_eq!(majors(&r), flags("TFTFTFTFT"));
    assert_eq!(labels(&r), strs(&["0s", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s"]));
}

#[test]
fn decimal_ruler_a_hair_more_spacing_doubles_the_step() {
    let r = ruler(0.0, 8.0, 1024.0, 129.0, Decimal);
    assert_eq!(r.step, 2.0);
    assert_eq!(times(&r), vec![0.0, 2.0, 4.0, 6.0, 8.0]);
    // The next coarser step is 5 s, so only 0 is on it.
    assert_eq!(majors(&r), flags("TFFFF"));
    assert_eq!(labels(&r), strs(&["0s", "2s", "4s", "6s", "8s"]));
}

#[test]
fn decimal_ruler_starts_on_the_first_tick_inside_the_range() {
    let r = ruler(3.0, 11.0, 1024.0, 128.0, Decimal);
    assert_eq!(times(&r), vec![3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
    assert_eq!(majors(&r), flags("FTFTFTFTF"));
    assert_eq!(labels(&r), strs(&["3s", "4s", "5s", "6s", "7s", "8s", "9s", "10s", "11s"]));
    // Half-way between two ticks at each end: the first tick after the start, the last before the end.
    let r = ruler(3.5, 11.5, 1024.0, 128.0, Decimal);
    assert_eq!(times(&r), vec![4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
    assert_eq!(majors(&r), flags("TFTFTFTF"));
}

#[test]
fn decimal_ruler_tenths_and_hundredths() {
    let r = ruler(0.0, 1.0, 1024.0, 128.0, Decimal);
    assert_eq!(r.step, 0.2);
    assert_eq!(times(&r), vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    assert_eq!(majors(&r), flags("TFFFFT"));
    assert_eq!(labels(&r), strs(&["0.0s", "0.2s", "0.4s", "0.6s", "0.8s", "1.0s"]));
    let r = ruler(0.0, 0.125, 1024.0, 128.0, Decimal);
    assert_eq!(r.step, 0.02);
    assert_eq!(times(&r), vec![0.0, 0.02, 0.04, 0.06, 0.08, 0.1, 0.12]);
    assert_eq!(majors(&r), flags("TFFFFTF"));
    assert_eq!(labels(&r), strs(&["0.00s", "0.02s", "0.04s", "0.06s", "0.08s", "0.10s", "0.12s"]));
}

#[test]
fn decimal_ruler_keeps_a_tick_that_sits_a_rounding_error_inside_the_range() {
    // 16.1 s * 1000 / 100 ms is 161.00000000000003: the tick at 16.1 must still be the first.
    let r = ruler(16.1, 16.5, 1024.0, 200.0, Decimal);
    assert_eq!(r.step, 0.1);
    assert_eq!(times(&r), vec![16.1, 16.2, 16.3, 16.4, 16.5]);
    assert_eq!(majors(&r), flags("FTFTF"));
    assert_eq!(labels(&r), strs(&["16.1s", "16.2s", "16.3s", "16.4s", "16.5s"]));
    // 32.3 s * 1000 / 100 ms is 322.99999999999994: the tick at 32.3 must still be the last.
    let r = ruler(32.0, 32.3, 1024.0, 200.0, Decimal);
    assert_eq!(times(&r), vec![32.0, 32.1, 32.2, 32.3]);
    assert_eq!(majors(&r), flags("TFTF"));
    assert_eq!(labels(&r), strs(&["32.0s", "32.1s", "32.2s", "32.3s"]));
}

#[test]
fn a_range_with_one_tick_has_that_tick() {
    // 0.5 s to 1.5 s with a 1 s minimum step: only 1 s is inside.
    let r = ruler(0.5, 1.5, 1024.0, 1024.0, Decimal);
    assert_eq!(r.step, 1.0);
    assert_eq!(times(&r), vec![1.0]);
    assert_eq!(majors(&r), flags("F"));
    assert_eq!(labels(&r), strs(&["1s"]));
}

#[test]
fn a_range_between_two_ticks_has_no_ticks() {
    let r = ruler(0.25, 0.75, 128.0, 256.0, Decimal);
    assert_eq!(r.step, 1.0);
    assert!(r.ticks.is_empty());
}

#[test]
fn minsec_ruler_fifteen_second_steps() {
    let r = ruler(0.0, 120.0, 1024.0, 128.0, MinSec);
    assert_eq!(r.step, 15.0);
    assert_eq!(times(&r), vec![0.0, 15.0, 30.0, 45.0, 60.0, 75.0, 90.0, 105.0, 120.0]);
    assert_eq!(majors(&r), flags("TFTFTFTFT"));
    assert_eq!(labels(&r), strs(&["0:00", "0:15", "0:30", "0:45", "1:00", "1:15", "1:30", "1:45", "2:00"]));
}

#[test]
fn minsec_ruler_fifteen_minute_steps_cross_the_hour() {
    let r = ruler(0.0, 7200.0, 1024.0, 128.0, MinSec);
    assert_eq!(r.step, 900.0);
    assert_eq!(times(&r), vec![0.0, 900.0, 1800.0, 2700.0, 3600.0, 4500.0, 5400.0, 6300.0, 7200.0]);
    assert_eq!(majors(&r), flags("TFTFTFTFT"));
    assert_eq!(labels(&r), strs(&["0:00", "15:00", "30:00", "45:00", "1:00:00", "1:15:00", "1:30:00", "1:45:00", "2:00:00"]));
}

#[test]
fn minsec_ruler_shows_milliseconds_below_a_one_second_step() {
    let r = ruler(0.0, 0.5, 1024.0, 128.0, MinSec);
    assert_eq!(r.step, 0.1);
    assert_eq!(times(&r), vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5]);
    assert_eq!(majors(&r), flags("TFTFTF"));
    assert_eq!(labels(&r), strs(&["0:00.000", "0:00.100", "0:00.200", "0:00.300", "0:00.400", "0:00.500"]));
}

#[test]
fn minsec_ruler_between_ticks_off_the_second() {
    let r = ruler(5.5, 6.5, 500.0, 60.0, MinSec);
    assert_eq!(r.step, 0.2);
    assert_eq!(times(&r), vec![5.6, 5.8, 6.0, 6.2, 6.4]);
    assert_eq!(majors(&r), flags("FFTFF"));
    assert_eq!(labels(&r), strs(&["0:05.600", "0:05.800", "0:06.000", "0:06.200", "0:06.400"]));
}

#[test]
fn the_largest_step_has_no_coarser_one_so_every_tick_is_major() {
    // 40,000,000 s over 1024 px at 128 px per label: the minimum step is exactly 5,000,000 s.
    let r = ruler(0.0, 40_000_000.0, 1024.0, 128.0, Decimal);
    assert_eq!(r.step, 5_000_000.0);
    assert_eq!(times(&r).len(), 9);
    assert_eq!(majors(&r), flags("TTTTTTTTT"));
    assert_eq!(labels(&r)[0], "0s");
    assert_eq!(labels(&r)[1], "5000000s");
    assert_eq!(labels(&r)[8], "40000000s");
    // 28,800 s (8 h): the minimum step is exactly 3600 s, the top of the clock table.
    let r = ruler(0.0, 28_800.0, 1024.0, 128.0, MinSec);
    assert_eq!(r.step, 3600.0);
    assert_eq!(majors(&r), flags("TTTTTTTTT"));
    assert_eq!(labels(&r), strs(&["0:00", "1:00:00", "2:00:00", "3:00:00", "4:00:00", "5:00:00", "6:00:00", "7:00:00", "8:00:00"]));
}

#[test]
fn a_range_too_wide_for_the_biggest_step_is_refused() {
    assert_eq!(ruler_ticks(0.0, 40_000_008.0, 1024.0, 128.0, Decimal), Err(RulerError::StepOutOfRange));
    assert_eq!(ruler_ticks(0.0, 28_808.0, 1024.0, 128.0, MinSec), Err(RulerError::StepOutOfRange));
    assert_eq!(ruler_ticks(0.0, 1.0e9, 1024.0, 128.0, Decimal), Err(RulerError::StepOutOfRange));
}

#[test]
fn the_tick_cap_is_2000_and_the_error_comes_before_any_allocation() {
    // One tick per second, minimum step exactly 1 s: n seconds make n + 1 ticks.
    let at = |n: f64| ruler_ticks(0.0, n, n, 1.0, Decimal);
    assert_eq!(good(at(1998.0)).ticks.len(), 1999);
    assert_eq!(good(at(1999.0)).ticks.len(), 2000);
    assert_eq!(at(2000.0), Err(RulerError::TooManyTicks));
    // A very wide screen or a very long range cannot slip past the cap.
    assert_eq!(ruler_ticks(0.0, 10.0, 1.0e12, 1.0, Decimal), Err(RulerError::TooManyTicks));
    assert_eq!(ruler_ticks(0.0, 1.0e9, 1.0e9, 1.0, MinSec), Err(RulerError::TooManyTicks));
}

#[test]
fn labels_are_never_closer_than_the_spacing_and_ticks_stay_in_range() {
    let cases = [
        (0.0, 10.0, 800.0, 80.0, Decimal),
        (0.0, 3600.0, 1200.0, 90.0, MinSec),
        (5.5, 6.5, 500.0, 60.0, MinSec),
        (100.0, 100.01, 900.0, 70.0, Decimal),
        (0.0, 20_000.0, 1500.0, 100.0, MinSec),
        (12.0, 4000.0, 777.0, 55.0, Decimal),
    ];
    for (i, (start, end, width, spacing, format)) in cases.into_iter().enumerate() {
        let r = ruler(start, end, width, spacing, format);
        let px_per_step = r.step / (end - start) * width;
        assert!(px_per_step >= spacing * (1.0 - 1.0 / 1_073_741_824.0), "case {i}: {px_per_step} px per step");
        assert!(!r.ticks.is_empty() && r.ticks.len() <= MAX_TICKS, "case {i}");
        for (j, tick) in r.ticks.iter().enumerate() {
            assert!(tick.seconds >= start - 1.0e-6 && tick.seconds <= end + 1.0e-6, "case {i} tick {j}");
            assert!(!tick.label.is_empty(), "case {i} tick {j}");
        }
        for (j, pair) in r.ticks.windows(2).enumerate() {
            assert!(pair[1].seconds > pair[0].seconds, "case {i} pair {j}");
        }
    }
}
