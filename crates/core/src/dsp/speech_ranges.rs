//! Speech versus silence segmentation of a frame level series.

use std::ops::Range;

/// Errors returned by [`speech_ranges`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentError {
    /// The threshold was NaN or infinite.
    NonFiniteThreshold,
    /// `min_speech` was zero.
    ZeroMinSpeech,
}

/// Returns the frame ranges of speech in `levels_db`, in ascending order.
///
/// A frame is speech when its level is `>= threshold_db` (NaN and -inf are
/// silence). A silence run strictly between two speech runs and shorter than
/// `min_silence` is bridged into speech (`min_silence == 0` bridges nothing).
/// Each resulting speech run of at least `min_speech` frames is returned as a
/// half-open range; shorter runs are dropped. A non-finite threshold or a zero
/// `min_speech` is an error.
pub fn speech_ranges(levels_db: &[f64], threshold_db: f64, min_speech: usize, min_silence: usize) -> Result<Vec<Range<usize>>, SegmentError> {
    if !threshold_db.is_finite() {
        return Err(SegmentError::NonFiniteThreshold);
    }
    if min_speech == 0 {
        return Err(SegmentError::ZeroMinSpeech);
    }
    let n = levels_db.len();
    let mut speech: Vec<bool> = levels_db.iter().map(|&v| v >= threshold_db).collect();

    // Bridge short interior silence runs.
    let mut i = 0;
    let mut seen_speech = false;
    while i < n {
        if speech[i] {
            seen_speech = true;
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !speech[i] {
            i += 1;
        }
        if seen_speech && i < n && i - start < min_silence {
            for flag in &mut speech[start..i] {
                *flag = true;
            }
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !speech[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && speech[i] {
            i += 1;
        }
        if i - start >= min_speech {
            out.push(start..i);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    #[test]
    fn finds_speech_runs_at_or_above_threshold() {
        let levels = [-80.0, -40.0, -20.0, -80.0, -30.0, -30.0, -30.0, -80.0];
        assert_eq!(speech_ranges(&levels, -40.0, 1, 0), Ok(vec![1..3, 4..7]));
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn short_speech_runs_are_dropped() {
        let levels = [-20.0, -80.0, -20.0, -20.0, -20.0, -80.0, -20.0, -20.0];
        assert_eq!(speech_ranges(&levels, -40.0, 3, 0), Ok(vec![2..5]));
    }

    #[test]
    fn short_silence_gaps_are_bridged() {
        let levels = [-20.0, -80.0, -20.0, -80.0, -80.0, -20.0, -20.0];
        assert_eq!(speech_ranges(&levels, -40.0, 1, 2), Ok(vec![0..3, 5..7]));
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn leading_and_trailing_silence_is_never_bridged() {
        let levels = [-80.0, -20.0, -80.0, -20.0, -80.0];
        assert_eq!(speech_ranges(&levels, -40.0, 1, 5), Ok(vec![1..4]));
    }

    #[test]
    fn nan_and_negative_infinity_count_as_silence() {
        let levels = [NAN, -20.0, f64::NEG_INFINITY, -20.0, NAN];
        assert_eq!(speech_ranges(&levels, -40.0, 1, 0), Ok(vec![1..2, 3..4]));
    }

    #[test]
    fn non_finite_threshold_is_an_error() {
        let levels = [-20.0];
        assert_eq!(speech_ranges(&levels, NAN, 1, 0), Err(SegmentError::NonFiniteThreshold));
        assert_eq!(speech_ranges(&levels, f64::INFINITY, 1, 0), Err(SegmentError::NonFiniteThreshold));
        assert_eq!(speech_ranges(&levels, f64::NEG_INFINITY, 1, 0), Err(SegmentError::NonFiniteThreshold));
    }

    #[test]
    fn zero_min_speech_is_an_error() {
        assert_eq!(speech_ranges(&[-20.0], -40.0, 0, 0), Err(SegmentError::ZeroMinSpeech));
    }
}
