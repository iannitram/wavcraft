//! "Play only the selected frequencies": a band filter for a spectral selection (A319, core only).
//!
//! The band `[low_hz, high_hz]` comes from a spectral selection. Only that band passes: a Butterworth
//! highpass at `low_hz` followed by a Butterworth lowpass at `high_hz`, both built with
//! [`design_filter`]. A low edge of 0 Hz needs no highpass and a high edge at Nyquist needs no lowpass.

use super::sci_filter::{BiquadSection, FilterFamily, ResponseType, design_filter, process_filter};

/// Why a band could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandError {
    /// The sample rate was NaN, infinite, zero or negative.
    InvalidSampleRate,
    /// `low_hz` or `high_hz` was NaN or infinite.
    NotFinite,
    /// `low_hz` was below 0 Hz or `high_hz` was above the Nyquist frequency.
    OutOfRange,
    /// `low_hz` was not strictly below `high_hz`.
    LowNotBelowHigh,
    /// The filter order was 0.
    ZeroOrder,
    /// `design_filter` returned an error.
    DesignFailed,
}

/// Checks a band against a sample rate. Checks in this order: sample rate, finiteness, range, order of edges.
pub fn validate_band(low_hz: f64, high_hz: f64, sample_rate: f64) -> Result<(), BandError> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(BandError::InvalidSampleRate);
    }
    if !low_hz.is_finite() || !high_hz.is_finite() {
        return Err(BandError::NotFinite);
    }
    if low_hz < 0.0 || high_hz > sample_rate / 2.0 {
        return Err(BandError::OutOfRange);
    }
    if low_hz >= high_hz {
        return Err(BandError::LowNotBelowHigh);
    }
    Ok(())
}

/// Designs the band filter sections: highpass sections first (only when `low_hz > 0`), then lowpass
/// sections (only when `high_hz` is below Nyquist). The band is validated first, then the order.
pub fn design_band(low_hz: f64, high_hz: f64, sample_rate: f64, order: usize) -> Result<Vec<BiquadSection>, BandError> {
    validate_band(low_hz, high_hz, sample_rate)?;
    if order == 0 {
        return Err(BandError::ZeroOrder);
    }
    let mut sections = Vec::new();
    if low_hz > 0.0 {
        sections.extend(
            design_filter(FilterFamily::Butterworth, ResponseType::Highpass, order, low_hz, sample_rate, 1.0, 40.0)
                .map_err(|_| BandError::DesignFailed)?,
        );
    }
    if high_hz < sample_rate / 2.0 {
        sections.extend(
            design_filter(FilterFamily::Butterworth, ResponseType::Lowpass, order, high_hz, sample_rate, 1.0, 40.0)
                .map_err(|_| BandError::DesignFailed)?,
        );
    }
    Ok(sections)
}

/// Filters `input` so that only `[low_hz, high_hz]` passes. The output has the same length as `input`.
pub fn band_filter(input: &[f32], low_hz: f64, high_hz: f64, sample_rate: f64, order: usize) -> Result<Vec<f32>, BandError> {
    let sections = design_band(low_hz, high_hz, sample_rate, order)?;
    Ok(process_filter(input, &sections))
}

/// The "play only selected frequencies" toggle state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandOnly {
    pub enabled: bool,
    pub low_hz: f64,
    pub high_hz: f64,
}

impl BandOnly {
    /// A disabled toggle holding the band. The band is not validated here.
    pub fn new(low_hz: f64, high_hz: f64) -> BandOnly {
        BandOnly { enabled: false, low_hz, high_hz }
    }

    /// Replaces the band after validating it. On an error the stored band does not change.
    pub fn set_band(&mut self, low_hz: f64, high_hz: f64, sample_rate: f64) -> Result<(), BandError> {
        validate_band(low_hz, high_hz, sample_rate)?;
        self.low_hz = low_hz;
        self.high_hz = high_hz;
        Ok(())
    }

    /// Disabled: returns a copy of `input`. Enabled: returns [`band_filter`] of `input` for the stored band.
    pub fn process(&self, input: &[f32], sample_rate: f64, order: usize) -> Result<Vec<f32>, BandError> {
        if !self.enabled {
            return Ok(input.to_vec());
        }
        band_filter(input, self.low_hz, self.high_hz, sample_rate, order)
    }
}
