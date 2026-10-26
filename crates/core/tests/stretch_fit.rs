//! Stretch to fit a duration (B112): the ratio, its limits, and applying it to a buffer.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::timepitch::fit::{FitError, FitLimits, SUPPORTED, TOLERANCE_FRAMES, Target, fit_ratio, stretch_to_duration, target_frames};

const RATE: u32 = 48_000;
const WIDE: FitLimits = FitLimits { min: 0.01, max: 100.0 };

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-stretchfit-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(dir: &TempDir) -> Store {
    Store::create_with(&dir.0, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

fn sine(hz: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|i| (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / RATE as f64).sin()) as f32).collect()
}

/// Frequency from positive-going zero crossings in the middle half.
fn freq(x: &[f32]) -> f64 {
    let (a, b) = (x.len() / 4, x.len() * 3 / 4);
    let n = (a..b).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).count();
    n as f64 * RATE as f64 / (b - a) as f64
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

// ---- target_frames ----

#[test]
fn frames_target_is_taken_as_is() {
    assert_eq!(target_frames(Target::Frames(24_000), RATE), Ok(24_000));
}

#[test]
fn seconds_target_is_rounded_to_the_nearest_frame() {
    assert_eq!(target_frames(Target::Seconds(1.5), RATE), Ok(72_000));
    assert_eq!(target_frames(Target::Seconds(1.0000105), RATE), Ok(48_001)); // 48000.504
    assert_eq!(target_frames(Target::Seconds(1.00001), RATE), Ok(48_000)); // 48000.48
    assert_eq!(target_frames(Target::Seconds(2.0), 44_100), Ok(88_200));
}

#[test]
fn bad_targets_are_refused() {
    for t in [
        Target::Frames(0),
        Target::Seconds(0.0),
        Target::Seconds(-1.0),
        Target::Seconds(f64::NAN),
        Target::Seconds(f64::INFINITY),
        Target::Seconds(f64::NEG_INFINITY),
    ] {
        assert!(matches!(target_frames(t, RATE), Err(FitError::BadTarget(_))), "{t:?}");
    }
}

#[test]
fn a_target_that_rounds_to_no_frames_is_refused() {
    assert!(matches!(target_frames(Target::Seconds(0.00001), RATE), Err(FitError::BadTarget(_)))); // 0.48 frames
    assert!(matches!(target_frames(Target::Seconds(1.0), 0), Err(FitError::BadTarget(_))));
}

// ---- fit_ratio ----

#[test]
fn ratio_is_target_over_current() {
    assert_eq!(fit_ratio(48_000, Target::Frames(72_000), RATE, SUPPORTED), Ok(1.5));
    assert_eq!(fit_ratio(48_000, Target::Frames(24_000), RATE, SUPPORTED), Ok(0.5));
    assert_eq!(fit_ratio(48_000, Target::Seconds(1.0), RATE, SUPPORTED), Ok(1.0));
    assert_eq!(fit_ratio(22_050, Target::Seconds(2.0), 44_100, SUPPORTED), Ok(4.0));
}

#[test]
fn empty_selection_is_refused() {
    assert_eq!(fit_ratio(0, Target::Frames(100), RATE, SUPPORTED), Err(FitError::ZeroCurrent));
}

#[test]
fn bad_target_is_refused_by_the_ratio() {
    assert!(matches!(fit_ratio(48_000, Target::Frames(0), RATE, SUPPORTED), Err(FitError::BadTarget(_))));
    assert!(matches!(fit_ratio(48_000, Target::Seconds(f64::NAN), RATE, SUPPORTED), Err(FitError::BadTarget(_))));
    assert!(matches!(fit_ratio(48_000, Target::Seconds(-2.0), RATE, SUPPORTED), Err(FitError::BadTarget(_))));
}

#[test]
fn supported_range_edges_are_allowed() {
    assert_eq!(fit_ratio(48_000, Target::Frames(480_000), RATE, SUPPORTED), Ok(10.0));
    assert_eq!(fit_ratio(480_000, Target::Frames(48_000), RATE, SUPPORTED), Ok(0.1));
}

#[test]
fn ratio_past_the_supported_range_is_refused_with_the_range() {
    for (cur, tgt) in [(48_000u64, 480_048u64), (480_000, 47_999)] {
        let e = fit_ratio(cur, Target::Frames(tgt), RATE, SUPPORTED).unwrap_err();
        assert!(matches!(e, FitError::OutOfRange { .. }), "{cur} -> {tgt}: {e:?}");
        assert!(e.to_string().contains("0.1 to 10"), "{e}");
    }
}

#[test]
fn caller_limits_narrow_the_range() {
    let narrow = FitLimits { min: 0.5, max: 2.0 };
    assert_eq!(fit_ratio(48_000, Target::Frames(96_000), RATE, narrow), Ok(2.0));
    assert_eq!(fit_ratio(48_000, Target::Frames(24_000), RATE, narrow), Ok(0.5));
    let e = fit_ratio(48_000, Target::Frames(120_000), RATE, narrow).unwrap_err();
    assert!(matches!(e, FitError::OutOfRange { .. }), "{e:?}");
    assert!(e.to_string().contains("0.5 to 2"), "{e}");
    assert!(matches!(fit_ratio(48_000, Target::Frames(23_000), RATE, narrow), Err(FitError::OutOfRange { .. })));
}

#[test]
fn limits_wider_than_supported_are_narrowed_to_supported() {
    assert_eq!(fit_ratio(48_000, Target::Frames(480_000), RATE, WIDE), Ok(10.0));
    let e = fit_ratio(48_000, Target::Frames(960_000), RATE, WIDE).unwrap_err();
    assert!(matches!(e, FitError::OutOfRange { .. }), "{e:?}");
    assert!(e.to_string().contains("0.1 to 10"), "{e}");
    assert!(matches!(fit_ratio(480_000, Target::Frames(24_000), RATE, WIDE), Err(FitError::OutOfRange { .. })));
}

#[test]
fn invalid_limits_are_refused() {
    for l in [
        FitLimits { min: f64::NAN, max: 2.0 },
        FitLimits { min: 0.5, max: f64::NAN },
        FitLimits { min: 0.5, max: f64::INFINITY },
        FitLimits { min: 0.0, max: 2.0 },
        FitLimits { min: -1.0, max: 2.0 },
        FitLimits { min: 3.0, max: 2.0 },
    ] {
        assert_eq!(fit_ratio(48_000, Target::Frames(48_000), RATE, l), Err(FitError::InvalidLimits), "{l:?}");
    }
}

#[test]
fn limits_that_miss_the_supported_range_entirely_are_refused() {
    assert_eq!(fit_ratio(48_000, Target::Frames(48_000), RATE, FitLimits { min: 20.0, max: 30.0 }), Err(FitError::InvalidLimits));
    assert_eq!(fit_ratio(48_000, Target::Frames(48_000), RATE, FitLimits { min: 0.01, max: 0.05 }), Err(FitError::InvalidLimits));
}

// ---- stretch_to_duration ----

#[test]
fn stretching_longer_hits_the_target_length_and_keeps_pitch() {
    let dir = TempDir::new("longer");
    let st = store(&dir);
    let input = vec![sine(220.0, 48_000), sine(220.0, 48_000)];
    let out = stretch_to_duration(&st, RATE, &input, Target::Seconds(1.5), SUPPORTED, &no_cancel()).unwrap();
    assert_eq!(out.len(), 2);
    for ch in &out {
        assert!((ch.len() as i64 - 72_000).unsigned_abs() <= TOLERANCE_FRAMES, "{}", ch.len());
        assert!((freq(ch) - 220.0).abs() < 4.0, "{}", freq(ch));
    }
}

#[test]
fn stretching_shorter_hits_the_target_length_and_keeps_pitch() {
    let dir = TempDir::new("shorter");
    let st = store(&dir);
    let out = stretch_to_duration(&st, RATE, &[sine(220.0, 48_000)], Target::Frames(24_000), SUPPORTED, &no_cancel()).unwrap();
    assert_eq!(out.len(), 1);
    assert!((out[0].len() as i64 - 24_000).unsigned_abs() <= TOLERANCE_FRAMES, "{}", out[0].len());
    assert!((freq(&out[0]) - 220.0).abs() < 6.0, "{}", freq(&out[0]));
}

#[test]
fn the_length_is_exact_for_an_awkward_ratio() {
    let dir = TempDir::new("awkward");
    let st = store(&dir);
    let input = vec![sine(300.0, 7_001)];
    let out = stretch_to_duration(&st, RATE, &input, Target::Frames(10_007), SUPPORTED, &no_cancel()).unwrap();
    assert_eq!(out[0].len(), 10_007);
}

#[test]
fn a_target_equal_to_the_length_returns_the_audio_unchanged() {
    let dir = TempDir::new("same");
    let st = store(&dir);
    let input = vec![sine(220.0, 4_800), sine(330.0, 4_800)];
    let out = stretch_to_duration(&st, RATE, &input, Target::Frames(4_800), SUPPORTED, &no_cancel()).unwrap();
    assert_eq!(out, input);
}

#[test]
fn ratio_out_of_range_is_refused_before_any_work() {
    let dir = TempDir::new("range");
    let st = store(&dir);
    let input = vec![sine(220.0, 4_800)];
    let r = stretch_to_duration(&st, RATE, &input, Target::Frames(4_800 * 11), SUPPORTED, &no_cancel());
    assert!(matches!(r, Err(FitError::OutOfRange { .. })), "{r:?}");
    let r = stretch_to_duration(&st, RATE, &input, Target::Frames(9_600), FitLimits { min: 0.5, max: 1.5 }, &no_cancel());
    assert!(matches!(r, Err(FitError::OutOfRange { .. })), "{r:?}");
}

#[test]
fn bad_target_and_bad_limits_are_refused_for_a_buffer() {
    let dir = TempDir::new("bad");
    let st = store(&dir);
    let input = vec![sine(220.0, 4_800)];
    let r = stretch_to_duration(&st, RATE, &input, Target::Seconds(f64::NAN), SUPPORTED, &no_cancel());
    assert!(matches!(r, Err(FitError::BadTarget(_))), "{r:?}");
    let r = stretch_to_duration(&st, RATE, &input, Target::Frames(100), FitLimits { min: 2.0, max: 1.0 }, &no_cancel());
    assert_eq!(r, Err(FitError::InvalidLimits));
}

#[test]
fn bad_buffers_are_refused() {
    let dir = TempDir::new("buffers");
    let st = store(&dir);
    let t = Target::Frames(100);
    let r = stretch_to_duration(&st, RATE, &[], t, SUPPORTED, &no_cancel());
    assert!(matches!(r, Err(FitError::BadInput(_))), "{r:?}");
    let r = stretch_to_duration(&st, RATE, &[vec![0.0; 100], vec![0.0; 99]], t, SUPPORTED, &no_cancel());
    assert!(matches!(r, Err(FitError::BadInput(_))), "{r:?}");
    let r = stretch_to_duration(&st, RATE, &[Vec::new()], t, SUPPORTED, &no_cancel());
    assert_eq!(r, Err(FitError::ZeroCurrent));
}

#[test]
fn a_cancelled_stretch_reports_cancelled() {
    let dir = TempDir::new("cancel");
    let st = store(&dir);
    let cancel = AtomicBool::new(true);
    let r = stretch_to_duration(&st, RATE, &[sine(220.0, 4_800)], Target::Frames(9_600), SUPPORTED, &cancel);
    assert_eq!(r, Err(FitError::Cancelled));
}

#[test]
fn a_huge_target_is_refused() {
    assert!(matches!(target_frames(Target::Seconds(1e300), RATE), Err(FitError::BadTarget(_))));
    assert!(matches!(target_frames(Target::Seconds(f64::MAX), RATE), Err(FitError::BadTarget(_))));
}
