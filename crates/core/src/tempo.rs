//! Tempo: tap tempo, and a document's tempo and time signature with Bars and Beats times.
//!
//! Tap tempo calculates BPM from tap measurements. A document's tempo and time signature
//! give its times as `bar:beat.tick`, bars and beats counted from 1, ticks from 0: `1:1.000`
//! is the start, `2:3.480` bar 2, beat 3, 480 ticks in. The mapping is `soundcraft-time`'s
//! `TempoMap` (a constant tempo and meter here; 960 ticks to a quarter note). Roadmap A101.

use soundcraft_time::{BarBeat, SampleRate, TempoMap};
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
    /// The window is clamped to at least 2 and at most 1024.
    pub fn with_params(window: usize, reset_gap_ms: f64) -> Self {
        const MAX_WINDOW: usize = 1024;
        Self { taps: Vec::new(), window: window.clamp(2, MAX_WINDOW), reset_gap_ms }
    }

    /// Records a tap at the given timestamp.
    /// Returns the current BPM or None if not enough data.
    /// 
    /// The new tap is compared against the last retained tap, even if it's a
    /// backward timestamp (which may cause the tap count to decrease).
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
        // Using saturating_add to prevent overflow
        if self.taps.len() > self.window.saturating_add(1) {
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
/// If the median interval is not finite or not strictly positive, return None.
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

    // If the median interval is infinite (overflowed) or not strictly positive, return None
    if !median_interval.is_finite() || median_interval <= 0.0 {
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
        assert!(result.is_some_and(|bpm| bpm.is_finite() && bpm > 0.0));
    }

    #[test]
    fn with_params_zero_or_one_window() {
        let mut tap_tempo = TapTempo::with_params(0, 2000.0);
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1000.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1500.0), Some(120.0));
        assert_eq!(tap_tempo.tap_count(), 3);
    }

    #[test]
    fn with_params_one_window_clamps_to_two() {
        let mut tap_tempo = TapTempo::with_params(1, 2000.0);
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1000.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1500.0), Some(120.0));
        assert_eq!(tap_tempo.tap_count(), 3);
    }

    #[test]
    fn with_params_max_window() {
        let mut tap_tempo = TapTempo::with_params(usize::MAX, 2000.0);
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1000.0), Some(120.0));
        assert_eq!(tap_tempo.tap(1500.0), Some(120.0));
        assert_eq!(tap_tempo.tap(2000.0), Some(120.0));
        assert_eq!(tap_tempo.tap_count(), 5);
    }

    #[test]
    fn tap_infinity_and_neginfinity() {
        let mut tap_tempo = TapTempo::new();
        tap_tempo.tap(0.0);
        let count_before = tap_tempo.tap_count();
        tap_tempo.tap(f64::INFINITY);
        assert_eq!(tap_tempo.tap_count(), count_before);
        tap_tempo.tap(f64::NEG_INFINITY);
        assert_eq!(tap_tempo.tap_count(), count_before);
    }

    #[test]
    fn backwards_timestamps() {
        let mut tap_tempo = TapTempo::new();
        assert_eq!(tap_tempo.tap(0.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
        assert_eq!(tap_tempo.tap(1000.0), Some(60.0));
        assert_eq!(tap_tempo.tap_count(), 2);
        assert_eq!(tap_tempo.tap(500.0), Some(120.0));
        assert_eq!(tap_tempo.tap_count(), 3);
        assert_eq!(tap_tempo.tap(2601.0), None);
        assert_eq!(tap_tempo.tap_count(), 1);
    }

    #[test]
    fn extreme_finite_taps_overflowing_interval_is_none() {
        assert_eq!(bpm_from_taps(&[-f64::MAX, f64::MAX]), None);
    }

    #[test]
    fn overflowing_interval_with_zero_interval_is_none() {
        assert_eq!(bpm_from_taps(&[-f64::MAX, f64::MAX, f64::MAX]), None);
    }

    #[test]
    fn ordinary_taps_still_give_their_bpm_after_overflow_guard() {
        assert_eq!(bpm_from_taps(&[0.0, 500.0, 1000.0]), Some(120.0));
    }
}

/// The slowest and fastest tempo accepted, in beats a minute.
pub const BPM_RANGE: std::ops::RangeInclusive<f64> = 20.0..=400.0;

/// The latest time `parse` accepts, in seconds (1000 hours); anything later is a refusal, not a search.
pub const MAX_PARSE_SECONDS: f64 = 3_600_000.0;

/// Tempo (beats a minute, a beat being the meter's denominator note) and time signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for Tempo {
    fn default() -> Self {
        Tempo { bpm: 120.0, numerator: 4, denominator: 4 }
    }
}

impl Tempo {
    /// `Some` when the tempo is in [`BPM_RANGE`], the numerator 1 to 32 and the denominator 1, 2, 4, 8, 16 or 32.
    pub fn new(bpm: f64, numerator: u32, denominator: u32) -> Option<Tempo> {
        let ok = BPM_RANGE.contains(&bpm) && (1..=32).contains(&numerator) && [1, 2, 4, 8, 16, 32].contains(&denominator);
        ok.then_some(Tempo { bpm, numerator, denominator })
    }

    /// A time signature written `3/4`.
    pub fn parse_meter(text: &str) -> Option<(u32, u32)> {
        let (n, d) = text.trim().split_once('/')?;
        let (n, d) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
        Tempo::new(120.0, n, d).map(|t| (t.numerator, t.denominator))
    }

    /// The meter as `4/4`.
    pub fn meter_text(&self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    /// The tempo as `120` or `93.5`.
    pub fn bpm_text(&self) -> String {
        let t = format!("{:.3}", self.bpm);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    fn map(&self) -> Option<TempoMap> {
        TempoMap::new(self.bpm, self.numerator, self.denominator).ok()
    }

    /// Seconds in one beat.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm * 4.0 / f64::from(self.denominator)
    }

    /// Seconds in one bar.
    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * f64::from(self.numerator)
    }

    /// `frame` at `rate` as `bar:beat.tick`.
    pub fn format(&self, frame: usize, rate: u32) -> String {
        match (self.map(), SampleRate::new(rate)) {
            (Some(map), Ok(sr)) => Self::text(&map, map.samples_to_ticks(i64::try_from(frame).unwrap_or(i64::MAX), sr)),
            _ => "1:1.000".to_string(),
        }
    }

    /// The ruler label at `seconds`.
    pub fn format_tick(&self, seconds: f64, rate: u32) -> String {
        self.format((seconds.max(0.0) * f64::from(rate)).round() as usize, rate)
    }

    fn text(map: &TempoMap, tick: i64) -> String {
        let bb = map.bar_beat_at_tick(tick.max(0));
        format!("{}:{}.{:03}", bb.bar, bb.beat, bb.tick)
    }

    /// The first frame whose time reads as `text` (`bar:beat.tick`; `bar:beat` and `bar` also
    /// read, with the missing parts at their start), or `None` when `text` is not a time or the
    /// beat or tick is past the end of its bar or beat.
    pub fn parse(&self, text: &str, rate: u32) -> Option<usize> {
        let map = self.map()?;
        let sr = SampleRate::new(rate).ok()?;
        let (bar_beat, tick) = match text.trim().split_once('.') {
            Some((b, t)) if !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit()) => (b, t.parse::<i64>().ok()?),
            Some(_) => return None,
            None => (text.trim(), 0),
        };
        let (bar, beat) = match bar_beat.split_once(':') {
            Some((a, b)) => (a, b),
            None => (bar_beat, "1"),
        };
        let digits = |s: &str| (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse::<i64>().ok()).flatten();
        let (bar, beat) = (digits(bar)?, digits(beat)?);
        let meter = map.meter_at_tick(0);
        if bar < 1 || beat < 1 || beat > i64::from(self.numerator) || tick >= meter.ticks_per_beat() {
            return None;
        }
        // Refuse a time past the longest a document can be, before any arithmetic can overflow.
        if (bar - 1) as f64 * self.bar_seconds() > MAX_PARSE_SECONDS {
            return None;
        }
        let want = map.tick_at_bar_beat(BarBeat { bar, beat, tick });
        let mut f = map.tick_to_samples(want, sr).max(0);
        while f > 0 && map.samples_to_ticks(f - 1, sr) >= want {
            f -= 1;
        }
        while map.samples_to_ticks(f, sr) < want {
            f += 1;
        }
        usize::try_from(f).ok()
    }
}

#[cfg(test)]
mod doc_tempo_tests {
    use super::*;

    #[test]
    fn the_start_and_known_positions_read_as_expected() {
        let t = Tempo::default(); // 120 bpm, 4/4: a beat is 0.5 s, a bar 2 s
        assert_eq!(t.format(0, 48_000), "1:1.000");
        assert_eq!(t.format(24_000, 48_000), "1:2.000");
        assert_eq!(t.format(96_000, 48_000), "2:1.000");
        assert_eq!(t.format(96_000 + 48_000 + 12_000, 48_000), "2:3.480");
        let waltz = Tempo::new(60.0, 3, 4).unwrap(); // a beat is 1 s, a bar 3 s
        assert_eq!(waltz.format(48_000 * 7, 48_000), "3:2.000");
        let eighths = Tempo::new(120.0, 6, 8).unwrap(); // a beat is 0.25 s
        assert_eq!(eighths.format(12_000 * 7, 48_000), "2:2.000");
    }

    #[test]
    fn parse_and_format_round_trip() {
        for t in [Tempo::default(), Tempo::new(93.5, 3, 4).unwrap(), Tempo::new(200.0, 7, 8).unwrap(), Tempo::new(47.0, 5, 4).unwrap()] {
            for rate in [8000u32, 22_050, 44_100, 48_000, 96_000] {
                for frame in [0usize, 1, 7, 441, 4410, rate as usize - 1, rate as usize, 12_345_678, 3600 * rate as usize + 17] {
                    let text = t.format(frame, rate);
                    let back = t.parse(&text, rate).unwrap_or_else(|| panic!("{text:?} did not parse"));
                    assert_eq!(t.format(back, rate), text, "{t:?} at {rate} Hz, frame {frame}");
                    assert!(back <= frame, "the parsed frame is the first with that text");
                }
            }
        }
    }

    #[test]
    fn typed_forms_and_refusals() {
        let t = Tempo::default();
        // The first frame that reads so: up to half a tick (about 10 frames here) before the exact one.
        let near = |text: &str, exact: usize| {
            let f = t.parse(text, 48_000).unwrap();
            assert!(f <= exact && exact - f <= 12, "{text}: {f} vs {exact}");
            assert_eq!(t.format(f, 48_000), t.format(exact, 48_000));
        };
        near("2:1.000", 96_000);
        near("2:1", 96_000);
        near("2", 96_000);
        near(" 1:3.480 ", 48_000 + 12_000);
        for bad in ["", "x", "0:1.000", "1:0.000", "1:5.000", "1:1.960", "1:1.", "1:1.x", "-1:1.000", "1:1:1"] {
            assert_eq!(t.parse(bad, 48_000), None, "{bad:?}");
        }
        assert_eq!(t.parse("1:1.000", 0), None);
    }

    #[test]
    fn huge_bar_numbers_are_refused_quickly() {
        let t = Tempo::default();
        let started = std::time::Instant::now();
        for huge in ["9223372036854775807", "9223372036854775807:1.000", "4611686018427387904:4.959", "99999999999", "1800002"] {
            assert_eq!(t.parse(huge, 48_000), None, "{huge}");
            assert_eq!(Tempo::new(20.0, 32, 1).unwrap().parse(huge, 192_000), None, "{huge}");
        }
        assert!(started.elapsed().as_secs() < 2, "refusal must not scale with the input");
    }

    #[test]
    fn boundary_bars_parse() {
        let t = Tempo::default(); // a bar is 2 s: bar 1_800_001 starts at exactly the cap
        let f = t.parse("1800001:1.000", 48_000).expect("the last accepted bar");
        assert_eq!(t.format(f, 48_000), "1800001:1.000");
        assert_eq!(t.parse("1800002", 48_000), None);
        assert!(t.parse("1000000:4.959", 48_000).is_some());
    }

    #[test]
    fn tempo_and_meter_are_checked() {
        assert!(Tempo::new(19.9, 4, 4).is_none());
        assert!(Tempo::new(400.1, 4, 4).is_none());
        assert!(Tempo::new(f64::NAN, 4, 4).is_none());
        assert!(Tempo::new(120.0, 0, 4).is_none());
        assert!(Tempo::new(120.0, 4, 3).is_none());
        assert_eq!(Tempo::parse_meter("3/4"), Some((3, 4)));
        assert_eq!(Tempo::parse_meter("6 / 8"), Some((6, 8)));
        assert_eq!(Tempo::parse_meter("4/5"), None);
        assert_eq!(Tempo::parse_meter("44"), None);
        assert_eq!(Tempo::new(93.5, 4, 4).unwrap().bpm_text(), "93.5");
        assert_eq!(Tempo::default().bpm_text(), "120");
    }

    #[test]
    fn ruler_labels_follow_the_tempo() {
        let t = Tempo::default();
        assert_eq!(t.format_tick(2.0, 44_100), "2:1.000");
        assert_eq!(t.format_tick(0.5, 44_100), "1:2.000");
        assert!((t.bar_seconds() - 2.0).abs() < 1e-12 && (t.beat_seconds() - 0.5).abs() < 1e-12);
    }
}
