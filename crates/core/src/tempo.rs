//! Tempo calculation functions.
//!
//! Calculate BPM from tap measurements.

use std::cmp::Ordering;

pub const TAP_RESET_GAP_MS: f64 = 2000.0;
pub const TAP_WINDOW: usize = 8;

/// Stateful tap tempo calculator.
#[derive(Debug, Clone)]
pub struct TapTempo {
    taps: Vec<f64>,
    window: usize,
    reset_gap_ms: f64,
}

impl TapTempo {
    /// Creates a new TapTempo with default parameters.
    pub fn new() -> Self {
        Self { taps: Vec::new(), window: TAP_WINDOW, reset_gap_ms: TAP_RESET_GAP_MS }
    }

    /// Creates a new TapTempo with custom parameters.
    /// The window is clamped to at least 2.
    pub fn with_params(window: usize, reset_gap_ms: f64) -> Self {
        Self { taps: Vec::new(), window: window.max(2), reset_gap_ms }
    }

    /// Records a tap at the given timestamp.
    /// Returns the current BPM or None if not enough data.
    pub fn tap(&mut self, now_ms: f64) -> Option<f64> {
        // Ignore non-finite timestamps
        if !now_ms.is_finite() {
            return bpm_from_taps(&self.taps);
        }

        // If we have a previous tap and the gap is larger than reset_gap_ms, clear taps
        if self.taps.last().is_some_and(|last_tap| now_ms - last_tap > self.reset_gap_ms) {
            self.taps.clear();
        }

        // Add the new tap
        self.taps.push(now_ms);

        // Keep only the last window + 1 taps (N = window intervals)
        if self.taps.len() > self.window + 1 {
            self.taps.remove(0);
        }

        bpm_from_taps(&self.taps)
    }

    /// Resets the tap history.
    pub fn reset(&mut self) {
        self.taps.clear();
    }

    /// Returns the number of taps recorded.
    pub fn tap_count(&self) -> usize {
        self.taps.len()
    }
}

impl Default for TapTempo {
    fn default() -> Self {
        Self::new()
    }
}

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
    if len % 2 == 1 { values[len / 2] } else { values[len / 2 - 1] + (values[len / 2] - values[len / 2 - 1]) / 2.0 }
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

    #[test]
    fn tap_tempo_basic_functionality() {
        let mut tap_tempo = TapTempo::new();
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1000.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1500.0), Some(120.0));
    }

    #[test]
    fn tap_tempo_resets_after_gap() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        tap_tempo.tap(500.0);
        // Gap of 2100ms > 2000ms reset gap should reset
        assert_eq!(tap_tempo.tap(2600.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
        assert_eq!(tap_tempo.tap(3100.0), Some(120.0));
    }

    #[test]
    fn gap_of_exactly_window_does_not_reset() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        assert_eq!(tap_tempo.tap(2000.0), Some(30.0));
    }

    #[test]
    fn tap_tempo_custom_window() {
        let mut tap_tempo = TapTempo::with_params(3, 2000.0);
        tap_tempo.tap(0.0);
        tap_tempo.tap(1000.0);
        tap_tempo.tap(2000.0);
        tap_tempo.tap(3000.0);
        tap_tempo.tap(3500.0);
        tap_tempo.tap(4000.0);
        tap_tempo.tap(4500.0);
        assert_eq!(tap_tempo.tap(4500.0), Some(120.0));
        assert_eq!(tap_tempo.tap_count(), 4);
    }

    #[test]
    fn tap_tempo_first_tap_returns_none() {
        let mut tap_tempo = TapTempo::new();
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
    }

    #[test]
    fn nan_tap_leaves_count_unchanged() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        let count_before = tap_tempo.tap_count();
        tap_tempo.tap(f64::NAN);
        assert_eq!(tap_tempo.tap_count(), count_before);
    }

    #[test]
    fn reset_then_tap() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        tap_tempo.reset();
        assert_eq!(tap_tempo.tap_count(), 0);
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
    }

    #[test]
    fn tap_tempo_gap_reset() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        tap_tempo.tap(500.0);
        // Gap of 2100ms > 2000ms reset gap should reset
        assert_eq!(tap_tempo.tap(2600.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
        assert_eq!(tap_tempo.tap(3100.0), Some(120.0));
    }

    #[test]
    fn tap_tempo_sequence_0_500_1000_1500() {
        let mut tap_tempo = TapTempo::new();
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1000.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1500.0), Some(120.0));
    }

    #[test]
    fn tap_tempo_extreme_values() {
        let result = bpm_from_taps(&[-f64::MAX, 0.0, f64::MAX]);
        assert!(result.is_some());
        assert!(!result.unwrap().is_nan());
    }
}
