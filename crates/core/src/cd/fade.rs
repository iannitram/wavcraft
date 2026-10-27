//! CD Layout track fades (roadmap C245, core only): a fade-in and a fade-out length for each
//! track, how a track's fades are applied to its samples, and where each track starts once the
//! pauses (gaps) are counted.
//!
//! **This touches exported audio, but nothing calls it yet**: the CUE sheet and the export in
//! [`super`] are unchanged, so a disc exports exactly as before. The default fade ([`TrackFade::NONE`],
//! 0 s in and 0 s out) leaves every sample bit-identical.
//!
//! - A fade is 0 to [`MAX_FADE_SECS`] seconds. The curve is linear (the crossfades of
//!   `doc::edit` are a raised-cosine pair for splices and are not reused here).
//! - Each fade may be at most half the track's length, so the fade-in and the fade-out never
//!   overlap.
//! - Fades happen inside the track's own audio: they never move a track's start.
//! - Buffers are per channel, one `Vec<f32>` for each channel, as the export reads them.

use super::{CD_RATE, CdError, CdLayout, SAMPLES_PER_FRAME, err};

/// The longest fade-in or fade-out a track may be given, in seconds.
pub const MAX_FADE_SECS: f64 = 30.0;

/// A track's fade-in and fade-out lengths in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackFade {
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

impl TrackFade {
    /// No fades: the audio is left exactly as it is.
    pub const NONE: TrackFade = TrackFade { fade_in_secs: 0.0, fade_out_secs: 0.0 };

    /// A fade pair, each length 0 to [`MAX_FADE_SECS`] seconds and finite.
    pub fn new(fade_in_secs: f64, fade_out_secs: f64) -> Result<TrackFade, CdError> {
        if !fade_in_secs.is_finite() || !(0.0..=MAX_FADE_SECS).contains(&fade_in_secs) {
            return err(format!("fade-in must be 0 to {MAX_FADE_SECS} seconds"));
        }
        if !fade_out_secs.is_finite() || !(0.0..=MAX_FADE_SECS).contains(&fade_out_secs) {
            return err(format!("fade-out must be 0 to {MAX_FADE_SECS} seconds"));
        }
        Ok(TrackFade { fade_in_secs, fade_out_secs })
    }

    /// True when both lengths are zero.
    pub fn is_none(&self) -> bool {
        self.fade_in_secs == 0.0 && self.fade_out_secs == 0.0
    }
}

/// A fade length in seconds as whole sample frames at `rate`, rounded to the nearest frame.
pub fn fade_frames(secs: f64, rate: u32) -> u64 {
    (secs * f64::from(rate)).round() as u64
}

/// The linear gain of frame `i` (counting from 0 at the quiet end) of a fade `n` frames long:
/// `i / n`, never above 1. A fade of 0 frames has gain 1.
pub fn fade_gain(i: u64, n: u64) -> f32 {
    if n == 0 { 1.0 } else { (i as f64 / n as f64).min(1.0) as f32 }
}

/// Whether `fade` fits a track of `track_frames` frames at `rate`: the rate is above 0 and each
/// fade is at most half the track (rounded down).
pub fn check_fit(fade: &TrackFade, track_frames: u64, rate: u32) -> Result<(), CdError> {
    if rate == 0 {
        return err("sample rate must be above 0");
    }
    let half = track_frames / 2;
    if fade_frames(fade.fade_in_secs, rate) > half {
        return err(format!("fade-in is longer than half the track ({half} frames)"));
    }
    if fade_frames(fade.fade_out_secs, rate) > half {
        return err(format!("fade-out is longer than half the track ({half} frames)"));
    }
    Ok(())
}

/// Apply `fade` in place to a track's per-channel samples at `rate`: the fade-in scales the first
/// frames by [`fade_gain`] and the fade-out scales the last frames by it, mirrored. Refused (and
/// nothing changed) for an invalid fade, channels of different lengths, a zero rate, or a fade
/// longer than half the track.
pub fn apply_fades(channels: &mut [Vec<f32>], rate: u32, fade: &TrackFade) -> Result<(), CdError> {
    TrackFade::new(fade.fade_in_secs, fade.fade_out_secs)?;
    if channels.is_empty() {
        return Ok(());
    }
    let len = channels.first().map_or(0, Vec::len);
    if channels.iter().any(|c| c.len() != len) {
        return err("all channels must have the same length");
    }
    check_fit(fade, len as u64, rate)?;
    let n_in = fade_frames(fade.fade_in_secs, rate) as usize;
    let n_out = fade_frames(fade.fade_out_secs, rate) as usize;

    for ch in channels.iter_mut() {
        for (i, s) in ch.iter_mut().take(n_in).enumerate() {
            *s *= fade_gain(i as u64, n_in as u64);
        }
        for (d, s) in ch.iter_mut().rev().take(n_out).enumerate() {
            *s *= fade_gain(d as u64, n_out as u64);
        }
    }
    Ok(())
}

/// The start of each track's audio, in sample frames at 44.1 kHz from the start of the disc
/// (`INDEX 01` times its 588 samples), given `fades` (one per track, in order). Refused when the
/// count is wrong or a fade is invalid or longer than half its track (measured on the track's
/// audio at 44.1 kHz).
pub fn track_starts(layout: &CdLayout, fades: &[TrackFade]) -> Result<Vec<u64>, CdError> {
    if fades.len() != layout.tracks.len() {
        return err(format!("{} fades for {} tracks", fades.len(), layout.tracks.len()));
    }
    for (i, (t, f)) in layout.tracks.iter().zip(fades).enumerate() {
        TrackFade::new(f.fade_in_secs, f.fade_out_secs)
            .and_then(|_| check_fit(f, t.cd_samples(), CD_RATE))
            .map_err(|e| CdError(format!("track {}: {e}", i + 1)))?;
    }
    Ok(layout.timeline().iter().map(|p| p.index01 * SAMPLES_PER_FRAME).collect())
}
