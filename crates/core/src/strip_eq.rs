//! A mixer strip's quick EQ (C164): three knobs that map onto the existing Parametric EQ.
//!
//! `StripEq` holds a low shelf gain (shelf at 100 Hz), one bell (gain, frequency and Q) and a
//! high shelf gain (shelf at 8 kHz). [`StripEq::to_params`] gives the 10 settings of
//! [`ParametricEq`] in its own order, with its second bell left at 0 dB. Nothing is processed
//! here; the effect does that.

use crate::effects::{Effect, ParametricEq};

/// The low shelf's frequency, in Hz.
const LOW_HZ: f64 = 100.0;
/// The high shelf's frequency, in Hz.
const HIGH_HZ: f64 = 8000.0;
/// The Parametric EQ's second bell, left as it is by default: frequency, gain (dB) and Q.
const BELL2_HZ: f64 = 2500.0;
const BELL2_DB: f64 = 0.0;
const BELL2_Q: f64 = 1.0;
/// How many settings the Parametric EQ takes.
const PARAM_COUNT: usize = 10;

/// The strip's EQ knobs. The fields are plain numbers; [`StripEq::clamped`] and
/// [`StripEq::to_params`] bring them into the effect's ranges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripEq {
    pub low_db: f64,
    pub mid_db: f64,
    pub mid_hz: f64,
    pub mid_q: f64,
    pub high_db: f64,
}

impl Default for StripEq {
    /// Flat: every gain 0 dB, the bell at 500 Hz with Q 1 (the effect's own defaults).
    fn default() -> Self {
        StripEq { low_db: 0.0, mid_db: 0.0, mid_hz: 500.0, mid_q: 1.0, high_db: 0.0 }
    }
}

/// `v` clamped to the range of the Parametric EQ setting `key`; NaN becomes its default.
fn limit(key: &str, v: f64) -> f64 {
    match ParametricEq.parameters().iter().find(|d| d.id == key) {
        Some(d) if v.is_nan() => d.default,
        Some(d) => v.clamp(d.min, d.max),
        None => v,
    }
}

impl StripEq {
    /// The same knobs with every value inside the effect's range (NaN becomes the default).
    pub fn clamped(&self) -> StripEq {
        StripEq {
            low_db: limit("low_gain_db", self.low_db),
            mid_db: limit("mid1_gain_db", self.mid_db),
            mid_hz: limit("mid1_freq", self.mid_hz),
            mid_q: limit("mid1_q", self.mid_q),
            high_db: limit("high_gain_db", self.high_db),
        }
    }

    /// The 10 Parametric EQ settings, in the effect's order: low shelf frequency and gain,
    /// bell 1 frequency, gain and Q, bell 2 frequency, gain and Q, high shelf frequency and gain.
    /// The knobs are clamped first, so the result always passes `effects::validate`.
    pub fn to_params(&self) -> Vec<f64> {
        let s = self.clamped();
        vec![LOW_HZ, s.low_db, s.mid_hz, s.mid_db, s.mid_q, BELL2_HZ, BELL2_DB, BELL2_Q, HIGH_HZ, s.high_db]
    }

    /// The knobs behind a Parametric EQ setting list, or `None` when the list is not one this
    /// strip can write: wrong length, a value out of range or NaN, or a shelf frequency or bell 2
    /// setting other than the ones `to_params` writes.
    pub fn from_params(params: &[f64]) -> Option<StripEq> {
        if params.len() != PARAM_COUNT {
            return None;
        }
        let strip = StripEq { low_db: params[1], mid_db: params[3], mid_hz: params[2], mid_q: params[4], high_db: params[9] };
        (strip.to_params() == params).then_some(strip)
    }

    /// Whether all three gains are 0 dB (the bell's frequency and Q do not matter then).
    pub fn is_flat(&self) -> bool {
        let s = self.clamped();
        s.low_db == 0.0 && s.mid_db == 0.0 && s.high_db == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::validate;

    fn knobs() -> StripEq {
        StripEq { low_db: 3.0, mid_db: -4.5, mid_hz: 1200.0, mid_q: 2.5, high_db: 6.0 }
    }

    #[test]
    fn default_is_flat_and_its_params_equal_the_effect_defaults() {
        let strip = StripEq::default();
        assert!(strip.is_flat());
        let defaults: Vec<f64> = ParametricEq.parameters().iter().map(|d| d.default).collect();
        assert_eq!(strip.to_params(), defaults);
    }

    #[test]
    fn to_params_passes_the_effect_validation() {
        assert_eq!(validate(&ParametricEq, &knobs().to_params()), Ok(()));
        assert_eq!(knobs().to_params(), vec![100.0, 3.0, 1200.0, -4.5, 2.5, 2500.0, 0.0, 1.0, 8000.0, 6.0]);
    }

    #[test]
    fn params_round_trip_through_from_params() {
        assert_eq!(StripEq::from_params(&knobs().to_params()), Some(knobs()));
        assert_eq!(StripEq::from_params(&StripEq::default().to_params()), Some(StripEq::default()));
    }

    #[test]
    fn values_are_clamped_to_the_effect_ranges() {
        let wild = StripEq { low_db: 99.0, mid_db: -99.0, mid_hz: 5.0, mid_q: 100.0, high_db: f64::NAN };
        assert_eq!(wild.to_params(), vec![100.0, 24.0, 20.0, -24.0, 20.0, 2500.0, 0.0, 1.0, 8000.0, 0.0]);
        let other = StripEq { low_db: -99.0, mid_db: 99.0, mid_hz: 99999.0, mid_q: 0.01, high_db: 30.0 };
        assert_eq!(other.to_params(), vec![100.0, -24.0, 20000.0, 24.0, 0.1, 2500.0, 0.0, 1.0, 8000.0, 24.0]);
        assert_eq!(validate(&ParametricEq, &wild.to_params()), Ok(()));
        assert_eq!(wild.clamped().mid_q, 20.0);
    }

    #[test]
    fn from_params_of_the_wrong_length_is_none() {
        let params = knobs().to_params();
        assert_eq!(StripEq::from_params(&params[..9]), None);
        let mut longer = params.clone();
        longer.push(0.0);
        assert_eq!(StripEq::from_params(&longer), None);
        assert_eq!(StripEq::from_params(&[]), None);
    }

    #[test]
    fn from_params_refuses_settings_the_strip_cannot_write() {
        let mut bell2 = knobs().to_params();
        bell2[6] = 3.0;
        assert_eq!(StripEq::from_params(&bell2), None);
        let mut shelf = knobs().to_params();
        shelf[0] = 200.0;
        assert_eq!(StripEq::from_params(&shelf), None);
        let mut out_of_range = knobs().to_params();
        out_of_range[1] = 40.0;
        assert_eq!(StripEq::from_params(&out_of_range), None);
    }

    #[test]
    fn is_flat_looks_at_the_three_gains_only() {
        let flat_moved = StripEq { mid_hz: 3000.0, mid_q: 5.0, ..StripEq::default() };
        assert!(flat_moved.is_flat());
        assert!(!StripEq { low_db: 1.0, ..StripEq::default() }.is_flat());
        assert!(!StripEq { mid_db: -1.0, ..StripEq::default() }.is_flat());
        assert!(!StripEq { high_db: 0.5, ..StripEq::default() }.is_flat());
    }
}
