//! Frequency Band Splitter, part a (roadmap A166). The splitter copies a selection into up to
//! eight band tracks, each holding one slice of the spectrum, cut at crossover frequencies. This
//! file plans those slices: where the crossovers go (evenly spaced on a logarithmic frequency
//! axis, so each band covers the same number of octaves), which frequency range each band covers,
//! and which band a frequency falls in. No filtering and no tracks are made here.
//!
//! Frequencies are in Hz. Every calculation runs in f64 and is cast to f32 once, at the end.

/// The fewest bands a split can have.
pub const MIN_BANDS: usize = 2;

/// The most bands a split can have.
pub const MAX_BANDS: usize = 8;

/// One band of a split: it covers `low_hz` up to, but not including, `high_hz`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub low_hz: f32,
    pub high_hz: f32,
}

impl Band {
    /// Whether `hz` is in this band: at or above `low_hz` and below `high_hz`.
    pub fn contains(&self, hz: f32) -> bool {
        hz >= self.low_hz && hz < self.high_hz
    }
}

/// The `bands - 1` crossover frequencies of a split into `bands` bands, evenly spaced on a
/// logarithmic axis between `low_hz` and `high_hz`: crossover `k` (1 up to `bands - 1`) is
/// `low_hz * (high_hz / low_hz)^(k / bands)`. The result is in rising order.
///
/// Errors: `bands` outside `MIN_BANDS..=MAX_BANDS` gives `a band split needs 2 to 8 bands`; a
/// `low_hz` or `high_hz` that is not finite, a `low_hz` that is not above 0, or a `high_hz` that
/// is not above `low_hz` gives `the crossover range must be finite, above 0 Hz, with low below high`.
/// The bands check comes first.
pub fn crossover_points(bands: usize, low_hz: f32, high_hz: f32) -> Result<Vec<f32>, String> {
    if !(MIN_BANDS..=MAX_BANDS).contains(&bands) {
        return Err("a band split needs 2 to 8 bands".to_string());
    }
    if !low_hz.is_finite() || !high_hz.is_finite() || low_hz <= 0.0 || high_hz <= low_hz {
        return Err("the crossover range must be finite, above 0 Hz, with low below high".to_string());
    }
    let low = f64::from(low_hz);
    let ratio = f64::from(high_hz) / low;
    Ok((1..bands).map(|k| (low * ratio.powf(k as f64 / bands as f64)) as f32).collect())
}

/// The bands of a split at `sample_rate`: the first starts at 0 Hz, each next band starts where
/// the one before ends (at a crossover from `crossover_points`), and the last ends at half the
/// sample rate. There are `bands` of them.
///
/// Errors are those of `crossover_points`, then `the sample rate must be above 0` for a
/// `sample_rate` of 0, then `the crossover range must end below half the sample rate` when
/// `high_hz` is not below half the sample rate.
pub fn band_plan(bands: usize, low_hz: f32, high_hz: f32, sample_rate: u32) -> Result<Vec<Band>, String> {
    let points = crossover_points(bands, low_hz, high_hz)?;
    if sample_rate == 0 {
        return Err("the sample rate must be above 0".to_string());
    }
    let nyquist = sample_rate as f32 / 2.0;
    if high_hz >= nyquist {
        return Err("the crossover range must end below half the sample rate".to_string());
    }
    let mut edges = Vec::with_capacity(bands + 1);
    edges.push(0.0);
    edges.extend(points);
    edges.push(nyquist);
    Ok(edges.windows(2).map(|w| Band { low_hz: w[0], high_hz: w[1] }).collect())
}

/// The index of the band of `plan` that holds `hz`, or `None`. The last band also holds its own
/// upper edge, so half the sample rate is in the last band. NaN and frequencies outside the plan
/// give `None`.
pub fn band_index(plan: &[Band], hz: f32) -> Option<usize> {
    if let Some(i) = plan.iter().position(|b| b.contains(hz)) {
        return Some(i);
    }
    match plan.last() {
        Some(band) if band.high_hz == hz => Some(plan.len() - 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= 0.01, "{actual} != {expected}");
    }

    fn approx_all(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            approx(*a, *e);
        }
    }

    #[test]
    fn crossovers_are_evenly_spaced_on_a_log_axis() {
        approx_all(&crossover_points(4, 100.0, 6400.0).unwrap_or_default(), &[282.8427, 800.0, 2262.7417]);
        approx_all(&crossover_points(2, 100.0, 10_000.0).unwrap_or_default(), &[1000.0]);
        approx_all(&crossover_points(3, 20.0, 20_000.0).unwrap_or_default(), &[200.0, 2000.0]);
    }

    #[test]
    fn eight_bands_give_seven_octave_spaced_crossovers() {
        approx_all(&crossover_points(8, 50.0, 12_800.0).unwrap_or_default(), &[100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0]);
    }

    #[test]
    fn crossovers_refuse_a_bad_band_count() {
        let msg = Err("a band split needs 2 to 8 bands".to_string());
        assert_eq!(crossover_points(0, 100.0, 1000.0), msg);
        assert_eq!(crossover_points(1, 100.0, 1000.0), msg);
        assert_eq!(crossover_points(9, 100.0, 1000.0), msg);
        assert_eq!(crossover_points(1, 0.0, 0.0), msg);
    }

    #[test]
    fn crossovers_refuse_a_bad_range() {
        let msg = Err("the crossover range must be finite, above 0 Hz, with low below high".to_string());
        assert_eq!(crossover_points(4, 0.0, 1000.0), msg);
        assert_eq!(crossover_points(4, -5.0, 1000.0), msg);
        assert_eq!(crossover_points(4, 1000.0, 1000.0), msg);
        assert_eq!(crossover_points(4, 1000.0, 100.0), msg);
        assert_eq!(crossover_points(4, f32::NAN, 1000.0), msg);
        assert_eq!(crossover_points(4, 100.0, f32::INFINITY), msg);
    }

    #[test]
    fn the_plan_runs_from_zero_to_half_the_sample_rate() {
        let plan = band_plan(4, 100.0, 6400.0, 48_000).unwrap_or_default();
        assert_eq!(plan.len(), 4);
        approx(plan[0].low_hz, 0.0);
        approx(plan[0].high_hz, 282.8427);
        approx(plan[1].low_hz, 282.8427);
        approx(plan[1].high_hz, 800.0);
        approx(plan[2].low_hz, 800.0);
        approx(plan[2].high_hz, 2262.7417);
        approx(plan[3].low_hz, 2262.7417);
        approx(plan[3].high_hz, 24_000.0);
    }

    #[test]
    fn every_split_has_touching_bands() {
        for bands in MIN_BANDS..=MAX_BANDS {
            let plan = band_plan(bands, 80.0, 8000.0, 44_100).unwrap_or_default();
            assert_eq!(plan.len(), bands);
            assert_eq!(plan[0].low_hz, 0.0);
            assert_eq!(plan[bands - 1].high_hz, 22_050.0);
            for pair in plan.windows(2) {
                assert_eq!(pair[0].high_hz, pair[1].low_hz);
            }
        }
    }

    #[test]
    fn the_plan_refuses_a_bad_rate_or_a_range_above_half_of_it() {
        assert_eq!(band_plan(4, 100.0, 6400.0, 0), Err("the sample rate must be above 0".to_string()));
        let msg = Err("the crossover range must end below half the sample rate".to_string());
        assert_eq!(band_plan(4, 100.0, 24_000.0, 48_000), msg);
        assert_eq!(band_plan(4, 100.0, 30_000.0, 48_000), msg);
        assert!(band_plan(4, 100.0, 23_999.0, 48_000).is_ok());
        assert_eq!(band_plan(9, 100.0, 6400.0, 0), Err("a band split needs 2 to 8 bands".to_string()));
        assert_eq!(band_plan(4, 0.0, 6400.0, 0), Err("the crossover range must be finite, above 0 Hz, with low below high".to_string()));
    }

    #[test]
    fn a_band_holds_its_low_edge_but_not_its_high_edge() {
        let band = Band { low_hz: 100.0, high_hz: 200.0 };
        assert!(band.contains(100.0));
        assert!(band.contains(199.99));
        assert!(!band.contains(200.0));
        assert!(!band.contains(99.99));
        assert!(!band.contains(f32::NAN));
    }

    #[test]
    fn band_index_finds_the_band_of_a_frequency() {
        let plan = band_plan(4, 100.0, 6400.0, 48_000).unwrap_or_default();
        assert_eq!(band_index(&plan, 0.0), Some(0));
        assert_eq!(band_index(&plan, plan[1].low_hz), Some(1));
        assert_eq!(band_index(&plan, 799.0), Some(1));
        assert_eq!(band_index(&plan, 800.0), Some(2));
        assert_eq!(band_index(&plan, 5000.0), Some(3));
    }

    #[test]
    fn band_index_includes_half_the_sample_rate_and_refuses_the_rest() {
        let plan = band_plan(4, 100.0, 6400.0, 48_000).unwrap_or_default();
        assert_eq!(band_index(&plan, 24_000.0), Some(3));
        assert_eq!(band_index(&plan, 24_000.5), None);
        assert_eq!(band_index(&plan, -1.0), None);
        assert_eq!(band_index(&plan, f32::NAN), None);
        assert_eq!(band_index(&[], 100.0), None);
    }
}
