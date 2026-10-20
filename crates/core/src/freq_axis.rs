//! The spectral display's frequency axis: which band is shown (`min_hz`..`max_hz`), on a linear
//! or a logarithmic scale, and the maths to scroll it, zoom it, map a frequency to a picture row
//! and place the ruler's ticks. Pure arithmetic, no UI.
//!
//! The picture itself is drawn by [`crate::spectrogram`] from the same band (it reads only the
//! FFT bins the band covers, and its overview cache does not depend on the band), so moving or
//! zooming the axis never re-reads the file's audio beyond the columns on screen.

use crate::spectrogram::SpectrogramParams;

/// Lowest frequency of a logarithmic axis, in Hz.
pub const MIN_LOG_HZ: f32 = 1.0;
/// The log axis can't be zoomed to a band narrower than this ratio (high / low).
pub const MIN_LOG_RATIO: f32 = 1.05;
/// The linear axis can't be zoomed to a band narrower than this, in Hz.
pub const MIN_LINEAR_SPAN_HZ: f32 = 10.0;
/// Lowest frequency of the default (reset) logarithmic view, in Hz.
pub const DEFAULT_LOG_MIN_HZ: f32 = 40.0;

/// The band shown and the scale it is drawn on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreqAxis {
    pub min_hz: f32,
    pub max_hz: f32,
    /// Logarithmic scale (equal ratios are equal heights) instead of linear.
    pub log: bool,
}

/// One mark on the frequency ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub hz: f32,
    /// Position in the band: 0 at the bottom edge, 1 at the top.
    pub t: f32,
    /// "440 Hz", "1.5 kHz".
    pub label: String,
}

impl FreqAxis {
    pub fn from_params(p: &SpectrogramParams) -> FreqAxis {
        FreqAxis { min_hz: p.min_hz, max_hz: p.max_hz, log: p.log_freq }
    }

    /// Put this band and scale into `p`.
    pub fn apply_to(&self, p: &mut SpectrogramParams) {
        p.min_hz = self.min_hz;
        p.max_hz = self.max_hz;
        p.log_freq = self.log;
    }

    /// The whole file's range for `sample_rate`: from the default low end (0 Hz when linear) to
    /// half the sample rate.
    pub fn full(log: bool, sample_rate: u32) -> FreqAxis {
        FreqAxis { min_hz: if log { DEFAULT_LOG_MIN_HZ } else { 0.0 }, max_hz: nyquist(sample_rate), log }.clamped(sample_rate)
    }

    fn floor_hz(&self) -> f32 {
        if self.log { MIN_LOG_HZ } else { 0.0 }
    }

    /// This axis made usable for `sample_rate`: finite, inside `floor..Nyquist`, and not
    /// narrower than the smallest band. This is the range the picture is drawn for.
    pub fn clamped(self, sample_rate: u32) -> FreqAxis {
        let nyq = nyquist(sample_rate);
        let floor = self.floor_hz();
        let narrowest_hi = if self.log { MIN_LOG_HZ * MIN_LOG_RATIO } else { MIN_LINEAR_SPAN_HZ };
        let hi = if self.max_hz.is_finite() { self.max_hz.clamp(narrowest_hi.min(nyq), nyq) } else { nyq };
        let highest_lo = if self.log { hi / MIN_LOG_RATIO } else { hi - MIN_LINEAR_SPAN_HZ };
        let lo = if self.min_hz.is_finite() { self.min_hz.clamp(floor, highest_lo.max(floor)) } else { floor };
        FreqAxis { min_hz: lo, max_hz: hi, log: self.log }
    }

    /// The same band on the other scale (or this one), made usable for `sample_rate`.
    pub fn with_scale(self, log: bool, sample_rate: u32) -> FreqAxis {
        FreqAxis { log, ..self }.clamped(sample_rate)
    }

    /// Position on the scale: ln(Hz) when logarithmic, Hz when linear.
    fn u(&self, hz: f64) -> f64 {
        if self.log { hz.max(1e-9).ln() } else { hz }
    }

    fn hz_of_u(&self, u: f64) -> f64 {
        if self.log { u.exp() } else { u }
    }

    /// Where `hz` sits in the band: 0 at `min_hz`, 1 at `max_hz`; outside the band it is below 0
    /// or above 1. Assumes a usable axis (see [`clamped`](Self::clamped)).
    pub fn fraction_of_hz(&self, hz: f32) -> f32 {
        let (lo, hi) = (self.u(f64::from(self.min_hz)), self.u(f64::from(self.max_hz)));
        ((self.u(f64::from(hz)) - lo) / (hi - lo)) as f32
    }

    /// The frequency at `t` in the band (0 bottom, 1 top): the inverse of
    /// [`fraction_of_hz`](Self::fraction_of_hz).
    pub fn hz_at_fraction(&self, t: f32) -> f32 {
        let (lo, hi) = (self.u(f64::from(self.min_hz)), self.u(f64::from(self.max_hz)));
        self.hz_of_u(lo + (hi - lo) * f64::from(t)) as f32
    }

    /// The row of a `rows`-tall picture that shows `hz` (row 0 is the top, the highest
    /// frequency), or `None` when `hz` is outside the band. Agrees with
    /// [`SpectrogramParams::row_edge_hz`]: row `r` shows `row_edge_hz(r + 1) <= hz < row_edge_hz(r)`.
    pub fn row_of_hz(&self, hz: f32, rows: usize) -> Option<usize> {
        if rows == 0 || !hz.is_finite() {
            return None;
        }
        let t = self.fraction_of_hz(hz);
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some((((1.0 - f64::from(t)) * rows as f64).floor() as usize).min(rows - 1))
    }

    /// The scale position of the whole range this axis can move in, `(low, high)`.
    fn limits_u(&self, sample_rate: u32) -> (f64, f64) {
        (self.u(f64::from(self.floor_hz())), self.u(f64::from(nyquist(sample_rate))))
    }

    fn min_span_u(&self) -> f64 {
        if self.log { f64::from(MIN_LOG_RATIO).ln() } else { f64::from(MIN_LINEAR_SPAN_HZ) }
    }

    /// A band of `span` (scale units) starting at `lo`, moved to lie inside the limits.
    fn placed(&self, lo: f64, span: f64, sample_rate: u32) -> FreqAxis {
        let (floor, ceil) = self.limits_u(sample_rate);
        let lo = lo.min(ceil - span).max(floor);
        FreqAxis { min_hz: self.hz_of_u(lo) as f32, max_hz: self.hz_of_u(lo + span) as f32, log: self.log }.clamped(sample_rate)
    }

    /// Zoom by `factor` (below 1 zooms in, above 1 out) keeping the frequency at `anchor_t`
    /// (0 bottom, 1 top) where it is on screen. The band stays inside `floor..Nyquist` and above
    /// the narrowest band.
    pub fn zoom_at(self, factor: f32, anchor_t: f32, sample_rate: u32) -> FreqAxis {
        let a = self.clamped(sample_rate);
        if !factor.is_finite() || factor <= 0.0 {
            return a;
        }
        let t = f64::from(anchor_t.clamp(0.0, 1.0));
        let (lo, hi) = (a.u(f64::from(a.min_hz)), a.u(f64::from(a.max_hz)));
        let span = hi - lo;
        let (floor, ceil) = a.limits_u(sample_rate);
        let new_span = (span * f64::from(factor)).clamp(a.min_span_u(), (ceil - floor).max(a.min_span_u()));
        let anchor = lo + t * span;
        a.placed(anchor - t * new_span, new_span, sample_rate)
    }

    /// Scroll the band by `fraction` of its own height (positive: towards higher frequencies).
    pub fn pan_by(self, fraction: f32, sample_rate: u32) -> FreqAxis {
        let a = self.clamped(sample_rate);
        if !fraction.is_finite() {
            return a;
        }
        let (lo, hi) = (a.u(f64::from(a.min_hz)), a.u(f64::from(a.max_hz)));
        a.placed(lo + (hi - lo) * f64::from(fraction), hi - lo, sample_rate)
    }

    /// The ruler's marks for a band drawn `extent_px` tall, no two closer than `min_gap_px`.
    /// Linear: multiples of a round step (1, 2 or 5 times a power of ten). Logarithmic: 1, 2, 3,
    /// 5 and so on times a power of ten, as many as fit.
    pub fn ticks(&self, extent_px: f32, min_gap_px: f32) -> Vec<Tick> {
        let (lo, hi) = (self.min_hz, self.max_hz);
        if !(hi.is_finite() && lo.is_finite() && hi > lo && extent_px > 0.0 && min_gap_px > 0.0) {
            return Vec::new();
        }
        let mut hz_values: Vec<f32> = Vec::new();
        if self.log {
            let per_decade = f64::from(extent_px) / (f64::from(hi) / f64::from(lo.max(1e-3))).log10().max(1e-6);
            // The first set of marks per decade whose closest pair (in decades) still has room.
            let sets: [(&[f32], f64); 4] = [
                (&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0], 10f64.log10() - 9f64.log10()),
                (&[1.0, 2.0, 5.0], 2f64.log10()),
                (&[1.0, 3.0], 3f64.log10()),
                (&[1.0], 1.0),
            ];
            let mantissas = sets.iter().find(|(_, closest)| closest * per_decade >= f64::from(min_gap_px)).map_or(&[1.0f32][..], |(m, _)| *m);
            let first = f64::from(lo.max(1e-3)).log10().floor() as i32;
            let last = f64::from(hi).log10().ceil() as i32;
            for d in first..=last {
                for m in mantissas {
                    let hz = m * 10f32.powi(d);
                    if hz >= lo * 0.999_999 && hz <= hi * 1.000_001 {
                        hz_values.push(hz);
                    }
                }
            }
        } else {
            let raw = f64::from(hi - lo) * f64::from(min_gap_px) / f64::from(extent_px);
            let decade = 10f64.powf(raw.log10().floor());
            let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * decade).find(|s| *s >= raw).unwrap_or(10.0 * decade);
            let mut k = (f64::from(lo) / step).ceil();
            while k * step <= f64::from(hi) * 1.000_001 && hz_values.len() < 512 {
                hz_values.push((k * step) as f32);
                k += 1.0;
            }
        }
        hz_values.into_iter().map(|hz| Tick { hz, t: self.fraction_of_hz(hz).clamp(0.0, 1.0), label: format_hz(hz) }).collect()
    }
}

fn nyquist(sample_rate: u32) -> f32 {
    (sample_rate as f32 / 2.0).max(2.0)
}

/// "440 Hz", "12.5 Hz", "1 kHz", "1.5 kHz", "22.05 kHz".
pub fn format_hz(hz: f32) -> String {
    let trim = |s: String| s.trim_end_matches('0').trim_end_matches('.').to_string();
    if hz.abs() < 1000.0 { format!("{} Hz", trim(format!("{hz:.1}"))) } else { format!("{} kHz", trim(format!("{:.2}", hz / 1000.0))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn params(axis: FreqAxis) -> SpectrogramParams {
        let mut p = SpectrogramParams::default();
        axis.apply_to(&mut p);
        p
    }

    #[test]
    fn a_row_holds_the_frequencies_between_its_edges_on_both_scales() {
        for axis in [
            FreqAxis { min_hz: 40.0, max_hz: 24_000.0, log: true },
            FreqAxis { min_hz: 0.0, max_hz: 24_000.0, log: false },
            FreqAxis { min_hz: 1_000.0, max_hz: 1_500.0, log: false },
            FreqAxis { min_hz: 200.0, max_hz: 800.0, log: true },
        ] {
            let p = params(axis);
            let rows = 97;
            for r in 0..rows {
                let (top, bottom) = (p.row_edge_hz(r, rows, RATE), p.row_edge_hz(r + 1, rows, RATE));
                let mid = if axis.log { (top * bottom).sqrt() } else { (top + bottom) / 2.0 };
                assert_eq!(axis.row_of_hz(mid, rows), Some(r), "{axis:?} row {r} between {bottom} and {top}");
            }
        }
    }

    #[test]
    fn frequencies_outside_the_band_have_no_row() {
        let axis = FreqAxis { min_hz: 100.0, max_hz: 1_000.0, log: true };
        assert_eq!(axis.row_of_hz(99.0, 50), None);
        assert_eq!(axis.row_of_hz(1_001.0, 50), None);
        assert_eq!(axis.row_of_hz(1_000.0, 50), Some(0));
        assert_eq!(axis.row_of_hz(100.0, 50), Some(49));
        assert_eq!(axis.row_of_hz(f32::NAN, 50), None);
        assert_eq!(axis.row_of_hz(500.0, 0), None);
    }

    #[test]
    fn the_scales_place_the_middle_differently() {
        let lin = FreqAxis { min_hz: 0.0, max_hz: 20_000.0, log: false };
        assert!((lin.hz_at_fraction(0.5) - 10_000.0).abs() < 1.0);
        let log = FreqAxis { min_hz: 20.0, max_hz: 20_000.0, log: true };
        assert!((log.hz_at_fraction(0.5) - 632.46).abs() < 1.0, "geometric middle");
        for axis in [lin, log] {
            for t in [0.0, 0.1, 0.5, 0.97, 1.0] {
                assert!((axis.fraction_of_hz(axis.hz_at_fraction(t)) - t).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn zooming_keeps_the_frequency_under_the_pointer_in_place() {
        for log in [true, false] {
            let axis = FreqAxis { min_hz: if log { 100.0 } else { 1_000.0 }, max_hz: if log { 12_000.0 } else { 9_000.0 }, log };
            for t in [0.0, 0.3, 0.5, 1.0] {
                let before = axis.hz_at_fraction(t);
                let zoomed = axis.zoom_at(0.5, t, RATE);
                let after = zoomed.hz_at_fraction(t);
                assert!((after - before).abs() <= before * 1e-3 + 0.5, "log={log} t={t}: {before} -> {after}");
                assert!((zoomed.max_hz - zoomed.min_hz) < (axis.max_hz - axis.min_hz));
            }
        }
    }

    #[test]
    fn zooming_out_stops_at_the_whole_range_and_in_at_the_narrowest_band() {
        let axis = FreqAxis { min_hz: 500.0, max_hz: 2_000.0, log: true };
        let out = axis.zoom_at(1e6, 0.5, RATE);
        assert_eq!((out.min_hz, out.max_hz), (MIN_LOG_HZ, 24_000.0));
        let lin = FreqAxis { min_hz: 500.0, max_hz: 2_000.0, log: false }.zoom_at(1e6, 0.5, RATE);
        assert_eq!((lin.min_hz, lin.max_hz), (0.0, 24_000.0));
        let tight = axis.zoom_at(1e-9, 0.5, RATE);
        assert!((tight.max_hz / tight.min_hz - MIN_LOG_RATIO).abs() < 1e-3, "{tight:?}");
        let tight = FreqAxis { min_hz: 500.0, max_hz: 2_000.0, log: false }.zoom_at(1e-9, 0.5, RATE);
        assert!((tight.max_hz - tight.min_hz - MIN_LINEAR_SPAN_HZ).abs() < 0.01, "{tight:?}");
    }

    #[test]
    fn panning_keeps_the_size_and_stops_at_the_ends() {
        let axis = FreqAxis { min_hz: 1_000.0, max_hz: 3_000.0, log: false };
        let up = axis.pan_by(0.5, RATE);
        assert_eq!((up.min_hz, up.max_hz), (2_000.0, 4_000.0));
        let down = axis.pan_by(-10.0, RATE);
        assert_eq!((down.min_hz, down.max_hz), (0.0, 2_000.0));
        let top = axis.pan_by(1e6, RATE);
        assert_eq!((top.min_hz, top.max_hz), (22_000.0, 24_000.0));
        let log = FreqAxis { min_hz: 100.0, max_hz: 400.0, log: true };
        let up = log.pan_by(1.0, RATE);
        assert!((up.min_hz - 400.0).abs() < 0.5 && (up.max_hz - 1_600.0).abs() < 2.0, "a log pan moves by ratio: {up:?}");
        assert_eq!(log.pan_by(f32::NAN, RATE), log);
    }

    #[test]
    fn a_band_is_made_usable_for_the_sample_rate() {
        let wild = FreqAxis { min_hz: f32::NAN, max_hz: f32::INFINITY, log: true }.clamped(16_000);
        assert_eq!((wild.min_hz, wild.max_hz), (1.0, 8_000.0));
        let backwards = FreqAxis { min_hz: 9_000.0, max_hz: 100.0, log: false }.clamped(48_000);
        assert!(backwards.min_hz < backwards.max_hz);
        let linear_low = FreqAxis { min_hz: 0.0, max_hz: 5_000.0, log: false }.clamped(48_000);
        assert_eq!(linear_low.min_hz, 0.0, "a linear axis may start at 0 Hz");
        let to_log = linear_low.with_scale(true, 48_000);
        assert_eq!((to_log.min_hz, to_log.max_hz, to_log.log), (1.0, 5_000.0, true));
        assert_eq!(FreqAxis::full(true, 44_100).min_hz, 40.0);
        assert_eq!(FreqAxis::full(false, 44_100).max_hz, 22_050.0);
    }

    #[test]
    fn linear_ticks_are_round_multiples_with_room_between() {
        let axis = FreqAxis { min_hz: 0.0, max_hz: 24_000.0, log: false };
        let ticks = axis.ticks(300.0, 30.0);
        let hz: Vec<f32> = ticks.iter().map(|t| t.hz).collect();
        assert_eq!(hz.first(), Some(&0.0));
        assert!(hz.windows(2).all(|w| (w[1] - w[0] - 5_000.0).abs() < 1.0), "{hz:?}");
        assert!(ticks.iter().any(|t| t.label == "5 kHz") && ticks.iter().any(|t| t.label == "0 Hz"));
        let narrow = FreqAxis { min_hz: 1_000.0, max_hz: 1_100.0, log: false }.ticks(200.0, 25.0);
        assert!(narrow.iter().all(|t| t.hz >= 1_000.0 && t.hz <= 1_100.0) && narrow.len() >= 3, "{narrow:?}");
        assert!(ticks.windows(2).all(|w| (w[1].t - w[0].t) * 300.0 >= 29.9));
    }

    #[test]
    fn log_ticks_follow_the_decades_and_thin_out_when_short() {
        let axis = FreqAxis { min_hz: 20.0, max_hz: 20_000.0, log: true };
        let tall: Vec<String> = axis.ticks(900.0, 22.0).into_iter().map(|t| t.label).collect();
        assert!(tall.contains(&"100 Hz".to_string()) && tall.contains(&"1 kHz".to_string()) && tall.contains(&"20 kHz".to_string()), "{tall:?}");
        let short = axis.ticks(120.0, 22.0);
        assert!(short.len() < tall.len());
        assert!(short.windows(2).all(|w| (w[1].t - w[0].t) * 120.0 >= 21.9), "{short:?}");
        assert!(FreqAxis { min_hz: 1.0, max_hz: 2.0, log: true }.ticks(0.0, 10.0).is_empty());
    }

    #[test]
    fn hertz_read_as_hz_or_khz() {
        assert_eq!(format_hz(440.0), "440 Hz");
        assert_eq!(format_hz(12.5), "12.5 Hz");
        assert_eq!(format_hz(0.0), "0 Hz");
        assert_eq!(format_hz(1_000.0), "1 kHz");
        assert_eq!(format_hz(1_500.0), "1.5 kHz");
        assert_eq!(format_hz(22_050.0), "22.05 kHz");
        assert_eq!(format_hz(20_000.0), "20 kHz");
    }
}
