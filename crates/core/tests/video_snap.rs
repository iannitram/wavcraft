//! A324: snap to the attached video's frame rate (core only). Maps a video's frame rate to the
//! nearest SMPTE rate the base offers and snaps a time to that rate's frames.

use wavcraft_core::smpte::SmpteRate;
use wavcraft_core::video_snap::{
    MAX_SECONDS, RATE_TOLERANCE, SNAP_SAMPLE_RATE, VideoSnapError, rate_from_fps, rate_from_ratio, snap_time_to_rate, snap_time_to_video,
    time_format_for,
};
use wavcraft_core::view::TimeFormat;

const GRID: f64 = 96_000.0;

/// A snapped time as a whole number of samples at 96 kHz.
fn grid(x: f64) -> i64 {
    (x * GRID).round() as i64
}

/// The text of a refusal, or "OK" with the value.
fn msg<T: std::fmt::Debug>(r: Result<T, VideoSnapError>) -> String {
    match r {
        Ok(v) => format!("OK {v:?}"),
        Err(e) => e.0,
    }
}

fn too_far(fps: &str) -> String {
    format!("no SMPTE rate is within 0.125 fps of {fps} fps (24, 25, 29.97, 30 and 59.94 are offered)")
}

#[test]
fn constants_are_what_the_docs_say() {
    assert_eq!(RATE_TOLERANCE, 0.125);
    assert_eq!(MAX_SECONDS, 360_000.0);
    assert_eq!(SNAP_SAMPLE_RATE, 96_000);
}

#[test]
fn each_rate_has_its_display_format() {
    assert_eq!(time_format_for(SmpteRate::Fps24), TimeFormat::Smpte24);
    assert_eq!(time_format_for(SmpteRate::Fps25), TimeFormat::Smpte25);
    assert_eq!(time_format_for(SmpteRate::Fps2997Df), TimeFormat::Smpte2997Df);
    assert_eq!(time_format_for(SmpteRate::Fps2997Ndf), TimeFormat::Smpte2997Ndf);
    assert_eq!(time_format_for(SmpteRate::Fps30), TimeFormat::Smpte30);
    assert_eq!(time_format_for(SmpteRate::Fps5994), TimeFormat::Smpte5994);
    for rate in SmpteRate::ALL {
        assert_eq!(time_format_for(rate).smpte(), Some(rate), "{rate:?}");
    }
}

#[test]
fn ratios_map_to_the_nearest_offered_rate() {
    let cases = [
        (24, 1, SmpteRate::Fps24),
        (24_000, 1001, SmpteRate::Fps24),
        (25, 1, SmpteRate::Fps25),
        (30_000, 1001, SmpteRate::Fps2997Df),
        (30, 1, SmpteRate::Fps30),
        (60_000, 1001, SmpteRate::Fps5994),
        (60, 1, SmpteRate::Fps5994),
        (48, 2, SmpteRate::Fps24),
    ];
    for (i, (num, den, want)) in cases.into_iter().enumerate() {
        assert_eq!(rate_from_ratio(num, den), Ok(want), "case {i}: {num}/{den}");
    }
}

#[test]
fn ratios_that_cannot_be_a_rate_are_refused() {
    assert_eq!(msg(rate_from_ratio(24, 0)), "the frame rate denominator is 0");
    assert_eq!(msg(rate_from_ratio(0, 0)), "the frame rate denominator is 0", "the denominator is checked first");
    assert_eq!(msg(rate_from_ratio(0, 1)), "the frame rate is 0");
    assert_eq!(msg(rate_from_ratio(1, 1)), too_far("1"));
    assert_eq!(msg(rate_from_ratio(50, 1)), too_far("50"));
    assert_eq!(msg(rate_from_ratio(120, 1)), too_far("120"));
    assert_eq!(msg(rate_from_ratio(1, 1_000_000)), too_far("0.000001"));
    assert_eq!(msg(rate_from_ratio(u32::MAX, 1)), too_far("4294967295"));
}

#[test]
fn fps_that_is_not_a_number_or_not_positive_is_refused() {
    let not_finite = "the frame rate is not a finite number";
    assert_eq!(msg(rate_from_fps(f64::NAN)), not_finite);
    assert_eq!(msg(rate_from_fps(f64::INFINITY)), not_finite);
    assert_eq!(msg(rate_from_fps(f64::NEG_INFINITY)), not_finite);
    assert_eq!(msg(rate_from_fps(0.0)), "the frame rate must be above 0 (got 0)");
    assert_eq!(msg(rate_from_fps(-0.0)), "the frame rate must be above 0 (got -0)");
    assert_eq!(msg(rate_from_fps(-24.0)), "the frame rate must be above 0 (got -24)");
    assert_eq!(msg(rate_from_fps(-0.5)), "the frame rate must be above 0 (got -0.5)");
}

#[test]
fn the_tolerance_is_checked_at_n_minus_1_n_and_n_plus_1() {
    // 0.125 fps is exact in binary, so the edge itself is well defined: it is still accepted.
    let cases = [
        (23.874, false),
        (23.875, true),
        (23.876, true),
        (24.124, true),
        (24.125, true),
        (24.126, false),
        (24.874, false),
        (24.875, true),
        (24.876, true),
        (25.124, true),
        (25.125, true),
        (25.126, false),
        (30.124, true),
        (30.125, true),
        (30.126, false),
        (29.845, false),
        (29.846, true),
        (60.064, true),
        (60.065, true),
        (60.066, false),
        (59.815, false),
        (59.816, true),
    ];
    for (i, (fps, ok)) in cases.into_iter().enumerate() {
        let r = rate_from_fps(fps);
        assert_eq!(r.is_ok(), ok, "case {i}: {fps} fps gave {r:?}");
        if !ok {
            assert_eq!(msg(r), too_far(&fps.to_string()), "case {i}");
        }
    }
}

#[test]
fn fps_picks_the_closer_offered_rate() {
    let cases = [
        (23.976, SmpteRate::Fps24),
        (24.0, SmpteRate::Fps24),
        (24.99, SmpteRate::Fps25),
        (25.0, SmpteRate::Fps25),
        (29.97, SmpteRate::Fps2997Df),
        (29.98, SmpteRate::Fps2997Df),
        (29.985, SmpteRate::Fps2997Df),
        (29.986, SmpteRate::Fps30),
        (30.0, SmpteRate::Fps30),
        (59.94, SmpteRate::Fps5994),
        (60.0, SmpteRate::Fps5994),
    ];
    for (i, (fps, want)) in cases.into_iter().enumerate() {
        assert_eq!(rate_from_fps(fps), Ok(want), "case {i}: {fps} fps");
    }
}

#[test]
fn exactly_29_97_goes_to_drop_frame_the_first_of_the_two() {
    assert_eq!(rate_from_fps(30_000.0 / 1001.0), Ok(SmpteRate::Fps2997Df));
    assert_eq!(rate_from_ratio(30_000, 1001), Ok(SmpteRate::Fps2997Df));
}

#[test]
fn the_ratio_and_the_fps_forms_agree() {
    for (i, (num, den)) in
        [(24, 1), (24_000, 1001), (25, 1), (30_000, 1001), (30, 1), (60_000, 1001), (60, 1), (1, 1), (50, 1)].into_iter().enumerate()
    {
        let from_fps = rate_from_fps(f64::from(num) / f64::from(den));
        assert_eq!(rate_from_ratio(num, den), from_fps, "case {i}: {num}/{den}");
    }
}

#[test]
fn snapping_at_25_fps_goes_to_the_nearest_frame_a_tie_to_the_later_one() -> Result<(), VideoSnapError> {
    // 3,840 samples a frame at 96 kHz.
    let cases = [(0.0, 0), (0.019, 0), (0.02, 3840), (0.021, 3840), (0.059, 3840), (0.06, 7680), (0.061, 7680), (0.04, 3840), (10.0, 960_000)];
    for (i, (x, want)) in cases.into_iter().enumerate() {
        assert_eq!(grid(snap_time_to_video(x, 25.0)?), want, "case {i}: {x} s");
    }
    Ok(())
}

#[test]
fn a_time_between_two_samples_is_rounded_to_the_nearer_sample_before_snapping() -> Result<(), VideoSnapError> {
    // The 25 fps half-way point is sample 1,920 (0.02 s). 1,919.2 samples rounds down to 1,919
    // (below the middle: frame 0); 1,919.9 rounds up to 1,920 (the middle: a tie goes later).
    assert_eq!(grid(snap_time_to_video(1919.2 / GRID, 25.0)?), 0);
    assert_eq!(grid(snap_time_to_video(1919.9 / GRID, 25.0)?), 3840);
    assert_eq!(grid(snap_time_to_video(1920.4 / GRID, 25.0)?), 3840);
    assert_eq!(grid(snap_time_to_video(1920.6 / GRID, 25.0)?), 3840);
    Ok(())
}

#[test]
fn a_23_976_video_snaps_to_24_fps_frames() -> Result<(), VideoSnapError> {
    // 4,000 samples a frame.
    assert_eq!(grid(snap_time_to_video(1.0, 23.976)?), 96_000);
    assert_eq!(grid(snap_time_to_video(1.02, 23.976)?), 96_000);
    assert_eq!(grid(snap_time_to_video(1.021, 23.976)?), 100_000);
    assert_eq!(grid(snap_time_to_video(1.021, 24.0)?), 100_000);
    assert_eq!(grid(snap_time_to_video(1.02, 24_000.0 / 1001.0)?), 96_000);
    Ok(())
}

#[test]
fn snapping_at_30_fps() -> Result<(), VideoSnapError> {
    // 3,200 samples a frame, half is 1,600.
    assert_eq!(grid(snap_time_to_video(0.016, 30.0)?), 0);
    assert_eq!(grid(snap_time_to_video(0.0166, 30.0)?), 0);
    assert_eq!(grid(snap_time_to_video(0.0167, 30.0)?), 3200);
    Ok(())
}

#[test]
fn snapping_at_29_97_lands_on_true_frame_starts_not_whole_samples_of_30() -> Result<(), VideoSnapError> {
    // Frame 30 starts at 1.001 s; frame 1,798 at 5,759,354 samples (just under a minute).
    assert_eq!(grid(snap_time_to_video(1.0, 30_000.0 / 1001.0)?), 96_096);
    assert_eq!(grid(snap_time_to_video(1.0, 29.97)?), 96_096);
    assert_eq!(grid(snap_time_to_video(60.0, 29.97)?), 5_759_354);
    assert_eq!(grid(snap_time_to_video(0.001, 29.97)?), 0);
    assert_eq!(grid(snap_time_to_video(0.0005, 29.97)?), 0);
    assert_eq!(grid(snap_time_to_video(3600.0, 29.97)?), 345_599_655);
    Ok(())
}

#[test]
fn snapping_at_59_94() -> Result<(), VideoSnapError> {
    assert_eq!(grid(snap_time_to_video(1.0, 59.94)?), 96_096);
    assert_eq!(grid(snap_time_to_video(1.0, 60.0)?), 96_096, "a 60 fps video uses the 59.94 frames");
    Ok(())
}

#[test]
fn drop_and_non_drop_snap_alike_because_the_pictures_are_the_same() -> Result<(), VideoSnapError> {
    for (i, x) in [0.0, 0.016, 0.5, 1.0, 59.99, 60.0, 600.0, 3600.0, 12_345.678].into_iter().enumerate() {
        let df = snap_time_to_rate(x, SmpteRate::Fps2997Df)?;
        let ndf = snap_time_to_rate(x, SmpteRate::Fps2997Ndf)?;
        assert_eq!(df, ndf, "case {i}: {x} s");
    }
    Ok(())
}

#[test]
fn the_snap_is_the_bases_own_snap() -> Result<(), VideoSnapError> {
    for rate in SmpteRate::ALL {
        for (i, x) in [0.0, 0.013, 0.5, 1.0, 7.77, 60.0, 599.99, 3600.5].into_iter().enumerate() {
            let samples = (x * GRID).round() as usize;
            let want = rate.snap(samples, SNAP_SAMPLE_RATE, usize::MAX) as f64 / GRID;
            assert_eq!(snap_time_to_rate(x, rate)?, want, "{rate:?} case {i}: {x} s");
        }
    }
    Ok(())
}

#[test]
fn a_snapped_time_stays_put_and_is_within_half_a_frame() -> Result<(), VideoSnapError> {
    for (fi, fps) in [23.976, 24.0, 25.0, 29.97, 30.0, 59.94].into_iter().enumerate() {
        let rate = rate_from_fps(fps)?;
        for (i, x) in [0.0, 0.001, 0.0123, 0.5, 1.0, 3.15, 60.0, 61.0, 599.999, 3600.25].into_iter().enumerate() {
            let s = snap_time_to_video(x, fps)?;
            assert_eq!(snap_time_to_video(s, fps)?, s, "fps case {fi}, time case {i}: snapping twice changes nothing");
            assert!((s - x).abs() <= rate.frame_seconds() / 2.0 + 2.0 / GRID, "fps case {fi}, time case {i}: {x} -> {s}");
            assert!(s >= 0.0 && s.is_finite(), "fps case {fi}, time case {i}");
        }
    }
    Ok(())
}

#[test]
fn times_are_checked_at_both_ends_n_minus_1_n_and_n_plus_1() -> Result<(), VideoSnapError> {
    assert_eq!(grid(snap_time_to_video(0.0, 25.0)?), 0);
    assert_eq!(grid(snap_time_to_video(-0.0, 25.0)?), 0, "minus zero is zero");
    assert_eq!(msg(snap_time_to_video(-0.5, 25.0)), "the time must be 0 or more (got -0.5)");
    assert_eq!(msg(snap_time_to_video(-1.0, 25.0)), "the time must be 0 or more (got -1)");
    assert_eq!(grid(snap_time_to_video(359_999.0, 25.0)?), 34_559_904_000);
    assert_eq!(grid(snap_time_to_video(360_000.0, 25.0)?), 34_560_000_000);
    assert_eq!(grid(snap_time_to_video(360_000.0, 29.97)?), 34_560_000_676);
    assert_eq!(grid(snap_time_to_video(359_999.9, 29.97)?), 34_559_991_066);
    assert_eq!(msg(snap_time_to_video(360_000.5, 25.0)), "the time must be at most 360000 seconds (got 360000.5)");
    assert_eq!(msg(snap_time_to_video(1e12, 25.0)), "the time must be at most 360000 seconds (got 1000000000000)");
    Ok(())
}

#[test]
fn a_time_that_is_not_a_number_is_refused() {
    let want = "the time is not a finite number";
    assert_eq!(msg(snap_time_to_video(f64::NAN, 25.0)), want);
    assert_eq!(msg(snap_time_to_video(f64::INFINITY, 25.0)), want);
    assert_eq!(msg(snap_time_to_video(f64::NEG_INFINITY, 25.0)), want);
    assert_eq!(msg(snap_time_to_rate(f64::NAN, SmpteRate::Fps25)), want);
    assert_eq!(msg(snap_time_to_rate(f64::INFINITY, SmpteRate::Fps25)), want);
    assert_eq!(msg(snap_time_to_rate(-2.0, SmpteRate::Fps25)), "the time must be 0 or more (got -2)");
    assert_eq!(msg(snap_time_to_rate(360_001.0, SmpteRate::Fps25)), "the time must be at most 360000 seconds (got 360001)");
}

#[test]
fn the_video_form_checks_the_rate_first_and_then_the_time() {
    assert_eq!(msg(snap_time_to_video(f64::NAN, f64::NAN)), "the frame rate is not a finite number");
    assert_eq!(msg(snap_time_to_video(-1.0, 0.0)), "the frame rate must be above 0 (got 0)");
    assert_eq!(msg(snap_time_to_video(1.0, 50.0)), too_far("50"));
    assert_eq!(msg(snap_time_to_video(1.0, f64::INFINITY)), "the frame rate is not a finite number");
    assert_eq!(msg(snap_time_to_video(f64::NAN, 25.0)), "the time is not a finite number");
}
