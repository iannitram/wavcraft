//! Tempo calculation functions.
//!
//! Calculate BPM from tap measurements.

use std::cmp::Ordering;

/// Calculates BPM from a series of tap measurements.
///
/// Non-finite taps (NaN, +inf, -inf) are dropped first.
/// The remaining taps are sorted ascending first.
/// Intervals are the differences between consecutive sorted taps.
/// If fewer than 2 taps remain after dropping non-finite values, return None.
/// The median of the intervals is taken (for an even count, the mean of the two middle values).
/// If the median interval is not strictly positive, return None.
/// Otherwise return Some(60000.0 / median_interval_ms).
///
/// # Arguments
///
/// * `taps_ms` - A slice of tap measurements in milliseconds
///
/// # Returns
///
/// * `Some(f64)` - The calculated BPM
/// * `None` - If there are fewer than 2 valid taps or the median interval is not positive
pub fn bpm_from_taps(taps_ms: &[f64]) -> Option<f64> {
    // Drop non-finite values first
    let mut valid_taps: Vec<f64> = taps_ms.iter().copied().filter(|v| v.is_finite()).collect();

    // Sort ascending
    valid_taps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    // If fewer than 2 taps remain, return None
    if valid_taps.len() < 2 {
        return None;
    }

    // Calculate intervals between consecutive taps
    let intervals: Vec<f64> = valid_taps.windows(2).map(|w| w[1] - w[0]).collect();

    // Sort intervals before calculating median
    let mut sorted_intervals = intervals;
    sorted_intervals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    // Calculate median interval
    let median_interval = median(&sorted_intervals);

    // If median interval is not strictly positive, return None
    if median_interval <= 0.0 {
        return None;
    }

    // Return BPM
    Some(60000.0 / median_interval)
}

/// Calculates the median of a sorted slice of f64 values.
fn median(values: &[f64]) -> f64 {
    let len = values.len();
    if len % 2 == 1 { values[len / 2] } else { (values[len / 2 - 1] + values[len / 2]) / 2.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_taps_500ms_apart_is_120() {
        assert_eq!(bpm_from_taps(&[0.0, 500.0]), Some(120.0));
    }

    #[test]
    fn fewer_than_two_taps_is_none() {
        assert_eq!(bpm_from_taps(&[]), None);
        assert_eq!(bpm_from_taps(&[100.0]), None);
    }

    #[test]
    fn outlier_interval_does_not_move_median() {
        assert_eq!(bpm_from_taps(&[0.0, 500.0, 1000.0, 1500.0, 2500.0]), Some(120.0));
    }

    #[test]
    fn even_count_uses_mean_of_middle_pair() {
        let result = bpm_from_taps(&[0.0, 400.0, 900.0, 1500.0, 2300.0]);
        assert!(result.is_some_and(|b| (b - 60000.0 / 550.0).abs() < 1e-9));
    }

    #[test]
    fn unsorted_taps_are_sorted_first() {
        assert_eq!(bpm_from_taps(&[1500.0, 0.0, 1000.0, 500.0]), Some(120.0));
    }

    #[test]
    fn non_finite_taps_are_dropped() {
        assert_eq!(bpm_from_taps(&[0.0, f64::NAN, 500.0, f64::INFINITY, 1000.0]), Some(120.0));
    }

    #[test]
    fn zero_median_interval_is_none() {
        assert_eq!(bpm_from_taps(&[100.0, 100.0, 100.0]), None);
    }

    #[test]
    fn unsorted_intervals_use_true_median() {
        assert_eq!(bpm_from_taps(&[0.0, 1000.0, 1100.0, 1600.0]), Some(120.0));
    }
}
