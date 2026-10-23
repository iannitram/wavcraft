//! Stretch to fit a duration (roadmap B112): work out the stretch ratio that turns a selection
//! of `current` frames into a chosen target length, check it against the limits
//! [`timepitch::stretch_pitch`](super::stretch_pitch) supports, and apply it to a sample buffer
//! through that function.
//!
//! Ratio = target length / current length. `stretch_pitch` accepts 0.1 to 10 ([`SUPPORTED`]).
//! The stretch rounds its output length to the nearest frame (`offline::scaled_len`), so the
//! result has exactly the target's frame count; [`TOLERANCE_FRAMES`] is the documented slack.
//! Core only: no dialog or menu.

use std::sync::atomic::AtomicBool;

use super::stretch_pitch;
use crate::doc::edit::{Crossfades, EditError};
use crate::doc::{Channel, DocState, Piece, SourceFormat};
use crate::store::BLOCK_FRAMES;
use crate::store::Store;

/// The ratio range `stretch_pitch` accepts (0.1 to 10).
pub const SUPPORTED: FitLimits = FitLimits { min: 0.1, max: 10.0 };

/// How many frames the stretched buffer may differ from the target (the stretch rounds
/// `current x ratio` to the nearest frame, so in practice the difference is 0).
pub const TOLERANCE_FRAMES: u64 = 1;

/// The ratio range a caller allows. It is narrowed to [`SUPPORTED`] when it is wider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitLimits {
    pub min: f64,
    pub max: f64,
}

/// The wanted length of the result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// A length in frames.
    Frames(u64),
    /// A length in seconds (turned into frames at the sample rate, rounded to the nearest).
    Seconds(f64),
}

/// Why a stretch-to-fit was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum FitError {
    /// The selection is empty, so there is nothing to scale.
    ZeroCurrent,
    /// The target is zero, negative, not a number, infinite, or rounds to no frames.
    BadTarget(String),
    /// The limits are not numbers, not above zero, or min is above max.
    InvalidLimits,
    /// The ratio is outside the allowed range (the message names the range, "0.1 to 10" say).
    OutOfRange { ratio: f64, min: f64, max: f64 },
    /// The sample buffer is empty or its channels differ in length.
    BadInput(String),
    /// The stretch itself failed.
    Stretch(String),
    /// Cancelled.
    Cancelled,
}

impl std::fmt::Display for FitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FitError::ZeroCurrent => write!(f, "the selection is empty"),
            FitError::BadTarget(why) => write!(f, "bad target length: {why}"),
            FitError::InvalidLimits => write!(f, "the stretch limits are not valid"),
            FitError::OutOfRange { ratio, min, max } => write!(f, "ratio {ratio} is outside the allowed {min} to {max}"),
            FitError::BadInput(why) => write!(f, "bad audio: {why}"),
            FitError::Stretch(why) => write!(f, "stretch failed: {why}"),
            FitError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for FitError {}

/// The target as a whole frame count at `sample_rate`.
pub fn target_frames(target: Target, sample_rate: u32) -> Result<u64, FitError> {
    match target {
        Target::Frames(frames) => {
            if frames == 0 {
                return Err(FitError::BadTarget("target is zero".to_string()));
            }
            Ok(frames)
        }
        Target::Seconds(seconds) => {
            if seconds.is_nan() || seconds.is_infinite() || seconds <= 0.0 {
                return Err(FitError::BadTarget(format!("invalid seconds: {seconds}")));
            }

            // Check for sample_rate being zero
            if sample_rate == 0 {
                return Err(FitError::BadTarget("sample rate is zero".to_string()));
            }

            // Convert to frames and round to nearest
            let f = (seconds * f64::from(sample_rate)).round();

            // Check for invalid frame count
            if f < 1.0 || f >= u64::MAX as f64 {
                return Err(FitError::BadTarget(format!("target rounds to invalid frames for seconds: {seconds}")));
            }

            Ok(f as u64)
        }
    }
}

/// The stretch ratio (target / current) for a selection of `current_frames`, checked against
/// `limits` (narrowed to [`SUPPORTED`]).
pub fn fit_ratio(current_frames: u64, target: Target, sample_rate: u32, limits: FitLimits) -> Result<f64, FitError> {
    if current_frames == 0 {
        return Err(FitError::ZeroCurrent);
    }

    // Check if the limits are valid
    if !(limits.min.is_finite() && limits.max.is_finite() && limits.min > 0.0 && limits.min <= limits.max) {
        return Err(FitError::InvalidLimits);
    }

    // Get target frames
    let frames = target_frames(target, sample_rate)?;

    // Determine the effective range (narrowed to supported)
    let lo = limits.min.max(SUPPORTED.min);
    let hi = limits.max.min(SUPPORTED.max);

    if lo > hi {
        return Err(FitError::InvalidLimits);
    }

    // Calculate ratio
    let ratio = frames as f64 / current_frames as f64;

    // Check if ratio is within the valid range
    if ratio < lo || ratio > hi {
        return Err(FitError::OutOfRange { ratio, min: lo, max: hi });
    }

    Ok(ratio)
}

/// Stretch `samples` (one `Vec` per channel, all one length) to the target duration through
/// `stretch_pitch` (pitch kept). `store` holds the scratch blocks. The result has the target's
/// frame count within [`TOLERANCE_FRAMES`].
pub fn stretch_to_duration(
    store: &Store,
    sample_rate: u32,
    samples: &[Vec<f32>],
    target: Target,
    limits: FitLimits,
    cancel: &AtomicBool,
) -> Result<Vec<Vec<f32>>, FitError> {
    // Check if there are any channels
    if samples.is_empty() {
        return Err(FitError::BadInput("no channels".to_string()));
    }

    // Check that all channels have the same length
    let len = samples[0].len();
    for channel in samples.iter() {
        if channel.len() != len {
            return Err(FitError::BadInput("channels have different lengths".to_string()));
        }
    }

    // Calculate ratio needed to stretch
    let ratio = fit_ratio(len as u64, target, sample_rate, limits)?;

    // Build channels
    let mut chans: Vec<Channel> = Vec::new();
    for c in samples {
        let mut ch = Channel::new();
        for part in c.chunks(BLOCK_FRAMES) {
            let block = store.append(part).map_err(|e| FitError::Stretch(e.to_string()))?;
            if let Some(p) = Piece::whole(block) {
                ch.push_merged(p);
            }
        }
        chans.push(ch);
    }

    // Create document state
    let n = u16::try_from(samples.len()).unwrap_or(u16::MAX);
    let doc =
        DocState::new(store.clone(), chans, sample_rate, SourceFormat::synthetic(sample_rate, n)).map_err(|e| FitError::Stretch(e.to_string()))?;

    // Apply stretch
    let edit = match stretch_pitch(&doc, 0..len as u64, ratio, 0.0, Crossfades::NONE, cancel) {
        Ok(e) => e,
        Err(EditError::Cancelled) => return Err(FitError::Cancelled),
        Err(e) => return Err(FitError::Stretch(e.to_string())),
    };

    // Read the result
    let mut out = Vec::new();
    for c in 0..edit.state.channel_count() {
        let mut v = vec![0.0f32; edit.state.len_frames() as usize];
        let got = edit.state.read(c, 0, &mut v).map_err(|e| FitError::Stretch(e.to_string()))?;
        v.truncate(got);
        out.push(v);
    }

    Ok(out)
}
