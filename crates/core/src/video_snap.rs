//! A324 (core only): tie the frame snap to an attached video's frame rate.
//!
//! The base (f47) snaps to the frames of a SMPTE display format (`TimeFormat::snap_to_frame`,
//! which calls `SmpteRate::snap`). Nothing says which of its six rates a given video is. This
//! module maps a video's frame rate (a ratio like 24000/1001, or an `f64` like 23.976) to the
//! nearest rate the base offers, refusing anything farther than [`RATE_TOLERANCE`] fps from every
//! one of them, and snaps a time in seconds to the nearest frame of that rate by calling the
//! base's snap (the maths is not repeated here).
//!
//! The rates the base offers are 24, 25, 29.97 (drop and non-drop: the same pictures, only the
//! labels differ, so the snap is the same), 30 and 59.94 fps. A 29.97 video maps to drop-frame
//! (the first of the two in `SmpteRate::ALL`). The snap uses the offered rate, not the video's
//! exact one, so 23.976 fps snaps to 24 fps frames.
//!
//! Times are seconds. The base snaps sample positions, so a time is rounded to a sample position
//! at [`SNAP_SAMPLE_RATE`] Hz, snapped, and turned back into seconds; the result is on that
//! sample grid. Not wired into the app: no attached video is read here.

use crate::smpte::SmpteRate;
use crate::view::TimeFormat;

/// The farthest a video's rate may be from the nearest offered rate, in frames a second.
pub const RATE_TOLERANCE: f64 = 0.125;
/// The longest time that can be snapped, in seconds (100 hours).
pub const MAX_SECONDS: f64 = 360_000.0;
/// The sample rate a time is snapped at.
pub const SNAP_SAMPLE_RATE: u32 = 96_000;

/// Why a rate or a time was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSnapError(pub String);

impl std::fmt::Display for VideoSnapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(s: impl Into<String>) -> Result<T, VideoSnapError> {
    Err(VideoSnapError(s.into()))
}

/// The display format of a SMPTE rate (the inverse of `TimeFormat::smpte`).
pub fn time_format_for(rate: SmpteRate) -> TimeFormat {
    match rate {
        SmpteRate::Fps24 => TimeFormat::Smpte24,
        SmpteRate::Fps25 => TimeFormat::Smpte25,
        SmpteRate::Fps2997Df => TimeFormat::Smpte2997Df,
        SmpteRate::Fps2997Ndf => TimeFormat::Smpte2997Ndf,
        SmpteRate::Fps30 => TimeFormat::Smpte30,
        SmpteRate::Fps5994 => TimeFormat::Smpte5994,
    }
}

/// The offered rate nearest a frame rate given as `num / den` frames a second.
pub fn rate_from_ratio(num: u32, den: u32) -> Result<SmpteRate, VideoSnapError> {
    if den == 0 {
        return err("the frame rate denominator is 0");
    }
    if num == 0 {
        return err("the frame rate is 0");
    }
    rate_from_fps(f64::from(num) / f64::from(den))
}

/// The offered rate nearest `fps` frames a second.
pub fn rate_from_fps(fps: f64) -> Result<SmpteRate, VideoSnapError> {
    if !fps.is_finite() {
        return err("the frame rate is not a finite number");
    }
    if fps <= 0.0 {
        return err(format!("the frame rate must be above 0 (got {fps})"));
    }
    let mut best = SmpteRate::Fps24;
    let mut best_gap = f64::INFINITY;
    for rate in SmpteRate::ALL {
        let (n, d) = rate.ratio();
        let gap = (n as f64 / d as f64 - fps).abs();
        if gap < best_gap {
            best = rate;
            best_gap = gap;
        }
    }
    if best_gap > RATE_TOLERANCE {
        return err(format!("no SMPTE rate is within 0.125 fps of {fps} fps (24, 25, 29.97, 30 and 59.94 are offered)"));
    }
    Ok(best)
}

/// `seconds` snapped to the nearest frame boundary of `rate`.
pub fn snap_time_to_rate(seconds: f64, rate: SmpteRate) -> Result<f64, VideoSnapError> {
    if !seconds.is_finite() {
        return err("the time is not a finite number");
    }
    if seconds < 0.0 {
        return err(format!("the time must be 0 or more (got {seconds})"));
    }
    if seconds > MAX_SECONDS {
        return err(format!("the time must be at most 360000 seconds (got {seconds})"));
    }
    let samples = (seconds * f64::from(SNAP_SAMPLE_RATE)).round() as usize;
    let snapped = time_format_for(rate).snap_to_frame(samples, SNAP_SAMPLE_RATE, usize::MAX);
    Ok(snapped as f64 / f64::from(SNAP_SAMPLE_RATE))
}

/// `seconds` snapped to the nearest frame boundary of a video running at `fps`.
pub fn snap_time_to_video(seconds: f64, fps: f64) -> Result<f64, VideoSnapError> {
    let rate = rate_from_fps(fps)?;
    snap_time_to_rate(seconds, rate)
}
