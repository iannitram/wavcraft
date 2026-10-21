//! Scientific analogue-prototype filters realised as bilinear-transform biquad cascades.
//!
//! Butterworth, Chebyshev I, Chebyshev II, and Bessel lowpass and highpass filters are
//! implemented. Bessel orders 2..=4 use tabulated normalised poles; Bessel orders above
//! four are deferred. Bandpass, bandstop, and elliptic filters are deferred because they
//! are not part of the feature-log row. An invalid sample rate or cutoff returns an empty
//! design, and `process_filter` consequently returns the dry input. NaN parameters are
//! treated as invalid for the design (ripple and attenuation NaNs use safe defaults).
//! Order zero returns the input unchanged.

use std::f64::consts::PI;

/// The analogue prototype family used by a scientific filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterFamily {
    Butterworth,
    ChebyshevI,
    ChebyshevII,
    Bessel,
}

/// The supported frequency response shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseType {
    Lowpass,
    Highpass,
}

/// One normalised direct-form-II-transposed biquad section.
#[derive(Clone, Copy, Debug)]
pub struct BiquadSection {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    /// Analogue-prototype Q for a second-order section; zero for first order.
    pub q: f64,
}

#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn new(re: f64, im: f64) -> Self { Self { re, im } }
    fn conj(self) -> Self { Self::new(self.re, -self.im) }
}

impl Default for Complex {
    fn default() -> Self { Self::new(0.0, 0.0) }
}

fn bessel_poles(order: usize) -> Option<&'static [Complex]> {
    const N2: [Complex; 1] = [Complex { re: -1.5, im: 0.8660254 }];
    const N3: [Complex; 2] = [Complex { re: -1.838907, im: 1.754381 }, Complex { re: -2.322185, im: 0.0 }];
    const N4: [Complex; 2] = [Complex { re: -2.103789, im: 2.657419 }, Complex { re: -2.896211, im: 0.867235 }];
    match order { 2 => Some(&N2), 3 => Some(&N3), 4 => Some(&N4), _ => None }
}

fn prototype_poles(family: FilterFamily, order: usize, ripple_db: f64, stopband_db: f64) -> Option<(Vec<Complex>, Vec<f64>, f64)> {
    if order == 0 { return Some((Vec::new(), Vec::new(), 1.0)); }
    if family == FilterFamily::Bessel {
        return bessel_poles(order).map(|poles| (poles.to_vec(), Vec::new(), 1.0));
    }
    let mut poles = Vec::new();
    let mut zeros = Vec::new();
    let ripple = if ripple_db.is_finite() { ripple_db.clamp(0.001, 60.0) } else { 1.0 };
    let attenuation = if stopband_db.is_finite() { stopband_db.clamp(0.1, 160.0) } else { 60.0 };
    let eps = (10.0_f64.powf(ripple / 10.0) - 1.0).sqrt();
    let mu = if family == FilterFamily::ChebyshevI { (1.0 / eps).asinh() / order as f64 } else { 0.0 };
    for k in (1..=order.div_ceil(2)).rev() {
        let theta = (2 * k - 1) as f64 * PI / (2.0 * order as f64);
        let (real, imag) = if family == FilterFamily::Butterworth {
            (-theta.sin(), theta.cos())
        } else if family == FilterFamily::ChebyshevI {
            (-mu.sinh() * theta.sin(), mu.cosh() * theta.cos())
        } else {
            let epsilon = 1.0 / (10.0_f64.powf(attenuation / 10.0) - 1.0).sqrt();
            let sinh = (1.0 / epsilon).asinh() / order as f64;
            let (re, im) = (-sinh.sinh() * theta.sin(), sinh.cosh() * theta.cos());
            let norm = re * re + im * im;
            (re / norm, -im / norm)
        };
        poles.push(Complex::new(real, imag));
        if imag.abs() > 1e-10 { poles.push(Complex::new(real, -imag)); }
        if family == FilterFamily::ChebyshevII && imag.abs() > 1e-10 {
            zeros.push(1.0 / theta.cos());
        }
    }
    Some((poles, zeros, if family == FilterFamily::ChebyshevI && order.is_multiple_of(2) { 1.0 / (1.0 + eps * eps).sqrt() } else { 1.0 }))
}

fn invert(pole: Complex) -> Complex {
    let norm = pole.re * pole.re + pole.im * pole.im;
    Complex::new(pole.re / norm, -pole.im / norm)
}

/// `k` is the bilinear constant relative to the prewarped cutoff: 1 / tan(pi fc / fs).
fn section_from_poles(pole: Complex, k: f64, response: ResponseType, zero: Option<f64>) -> BiquadSection {
    let highpass = response == ResponseType::Highpass;
    let q = (pole.re * pole.re + pole.im * pole.im).sqrt() / (-2.0 * pole.re);
    let pole = if highpass { invert(pole) } else { pole };
    let zero = zero.map(|w| if highpass { 1.0 / w } else { w });
    let m = pole.re * pole.re + pole.im * pole.im;
    let a0 = k * k - pole.re * 2.0 * k + m;
    let a1 = -2.0 * k * k + 2.0 * m;
    let a2 = k * k + pole.re * 2.0 * k + m;
    let (n0, n1, n2) = match (response, zero) {
        (_, Some(w)) => (k * k + w * w, 2.0 * (w * w - k * k), k * k + w * w),
        (ResponseType::Highpass, None) => (k * k, -2.0 * k * k, k * k),
        (ResponseType::Lowpass, None) => (m, 2.0 * m, m),
    };
    BiquadSection { b0: n0 / a0, b1: n1 / a0, b2: n2 / a0, a1: a1 / a0, a2: a2 / a0, q }
}

/// Designs a bilinear-transform cascade. `ripple_db` applies to Chebyshev I and
/// `stopband_db` applies to Chebyshev II; the other families ignore those values.
pub fn design_filter(family: FilterFamily, response: ResponseType, order: usize, cutoff_hz: f64, sample_rate: f64, ripple_db: f64, stopband_db: f64) -> Vec<BiquadSection> {
    if order == 0 { return Vec::new(); }
    if !sample_rate.is_finite() || sample_rate <= 0.0 || !cutoff_hz.is_finite() || cutoff_hz <= 0.0 || cutoff_hz >= sample_rate / 2.0 { return Vec::new(); }
    let cutoff = 1.0 / (PI * cutoff_hz / sample_rate).tan();
    let Some((poles, prototype_zeros, prototype_gain)) = prototype_poles(family, order, ripple_db, stopband_db) else { return Vec::new(); };
    let mut zeros = prototype_zeros.into_iter();
    let mut sections = Vec::new();
    let mut used = vec![false; poles.len()];
    for index in 0..poles.len() {
        if used[index] { continue; }
        let pole = poles[index];
        if pole.im.abs() > 1e-10 {
            let pair = pole.conj();
            for other in (index + 1)..poles.len() {
                if !used[other] && (poles[other].re - pair.re).abs() < 1e-5 && (poles[other].im - pair.im).abs() < 1e-5 { used[other] = true; break; }
            }
            let zero = zeros.next();
            sections.push(section_from_poles(pole, cutoff, response, zero));
        } else {
            used[index] = true;
            let p = if response == ResponseType::Highpass { 1.0 / pole.re } else { pole.re };
            let a0 = cutoff - p;
            let a1 = -cutoff - p;
            let (n0, n1) = if response == ResponseType::Highpass { (cutoff, -cutoff) } else { (1.0, 1.0) };
            sections.push(BiquadSection { b0: n0 / a0, b1: n1 / a0, b2: 0.0, a1: a1 / a0, a2: 0.0, q: 0.0 });
        }
    }
    let target = prototype_gain;
    let frequency = if response == ResponseType::Highpass { sample_rate * 0.5 } else { 0.0 };
    let current = magnitude_linear(&sections, frequency, sample_rate);
    if current.is_finite() && current > 0.0 {
        let scale = target / current;
        if let Some(first) = sections.first_mut() {
            first.b0 *= scale;
            first.b1 *= scale;
            first.b2 *= scale;
        }
    }
    sections
}

fn magnitude_linear(sections: &[BiquadSection], frequency_hz: f64, sample_rate: f64) -> f64 {
    let angle = 2.0 * PI * frequency_hz / sample_rate;
    let (sin, cos) = angle.sin_cos();
    let mut magnitude = 1.0;
    for section in sections {
        let num_re = section.b0 + section.b1 * cos + section.b2 * (2.0 * cos * cos - 1.0);
        let num_im = -section.b1 * sin - section.b2 * 2.0 * sin * cos;
        let den_re = 1.0 + section.a1 * cos + section.a2 * (2.0 * cos * cos - 1.0);
        let den_im = -section.a1 * sin - section.a2 * 2.0 * sin * cos;
        magnitude *= (num_re * num_re + num_im * num_im).sqrt() / (den_re * den_re + den_im * den_im).sqrt();
    }
    magnitude
}

/// Processes samples through the sections using f64 state and returns f32 samples.
pub fn process_filter(input: &[f32], sections: &[BiquadSection]) -> Vec<f32> {
    if sections.is_empty() { return input.to_vec(); }
    let mut states = vec![[0.0_f64; 2]; sections.len()];
    input.iter().map(|&sample| {
        let mut value = if sample.is_finite() { sample as f64 } else { 0.0 };
        for (section, state) in sections.iter().zip(states.iter_mut()) {
            let output = section.b0 * value + state[0];
            state[0] = section.b1 * value - section.a1 * output + state[1];
            state[1] = section.b2 * value - section.a2 * output;
            value = if output.is_finite() { output } else { 0.0 };
        }
        if value.is_finite() { value as f32 } else { 0.0 }
    }).collect()
}

/// Returns the cascade magnitude in dB at `frequency_hz`.
pub fn magnitude_response_db(sections: &[BiquadSection], frequency_hz: f64, sample_rate: f64) -> f64 {
    if !sample_rate.is_finite() || sample_rate <= 0.0 || !frequency_hz.is_finite() { return f64::NEG_INFINITY; }
    let magnitude = magnitude_linear(sections, frequency_hz.clamp(0.0, sample_rate / 2.0), sample_rate);
    if magnitude > 0.0 && magnitude.is_finite() { 20.0 * magnitude.log10() } else { f64::NEG_INFINITY }
}

#[cfg(test)]
mod tests {
    use super::{bessel_poles, design_filter, magnitude_response_db, process_filter, BiquadSection, FilterFamily, ResponseType};

    fn assert_close(actual: f64, expected: f64, tolerance: f64) { assert!((actual - expected).abs() <= tolerance, "{actual} != {expected}"); }
    fn filter(family: FilterFamily, response: ResponseType, order: usize) -> Vec<BiquadSection> { design_filter(family, response, order, 1000.0, 48000.0, 1.0, 40.0) }

    #[test] fn butterworth_lowpass_minus_3db_at_cutoff_n2() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2), 1000.0, 48000.0), -3.0103, 0.02); }
    #[test] fn butterworth_lowpass_minus_3db_at_cutoff_n4() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Lowpass, 4), 1000.0, 48000.0), -3.0103, 0.02); }
    #[test] fn butterworth_lowpass_n2_at_twice_cutoff() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2), 2000.0, 48000.0), -12.304, 0.15); }
    #[test] fn butterworth_lowpass_n4_at_twice_cutoff() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Lowpass, 4), 2000.0, 48000.0), -24.099, 0.20); }
    #[test] fn butterworth_n2_section_q() { let s = filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2); assert_close(s[0].q, 0.70711, 1e-4); }
    #[test] fn butterworth_n4_section_qs() { let s = filter(FilterFamily::Butterworth, ResponseType::Lowpass, 4); assert_close(s[0].q, 0.54120, 1e-4); assert_close(s[1].q, 1.30656, 1e-4); }
    #[test] fn butterworth_lowpass_dc_gain_unity() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2), 0.0, 48000.0), 0.0, 0.02); }
    #[test] fn butterworth_highpass_minus_3db_at_cutoff_n2() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Highpass, 2), 1000.0, 48000.0), -3.0103, 0.02); }
    #[test] fn butterworth_highpass_nyquist_gain_near_zero_db() { assert_close(magnitude_response_db(&filter(FilterFamily::Butterworth, ResponseType::Highpass, 2), 23999.0, 48000.0), 0.0, 0.05); }
    #[test] fn chebyshev1_gain_at_cutoff_minus_ripple() { assert_close(magnitude_response_db(&filter(FilterFamily::ChebyshevI, ResponseType::Lowpass, 2), 1000.0, 48000.0), -1.0, 0.02); }
    #[test] fn chebyshev1_even_order_dc_gain() { assert_close(magnitude_response_db(&filter(FilterFamily::ChebyshevI, ResponseType::Lowpass, 2), 0.0, 48000.0), -1.0, 0.02); }
    #[test] fn chebyshev1_odd_order_dc_gain() { assert_close(magnitude_response_db(&filter(FilterFamily::ChebyshevI, ResponseType::Lowpass, 3), 0.0, 48000.0), 0.0, 0.02); }
    #[test] fn chebyshev1_passband_ripple_bounded() { let s = filter(FilterFamily::ChebyshevI, ResponseType::Lowpass, 4); for i in 0..=100 { let db = magnitude_response_db(&s, i as f64 * 10.0, 48000.0); assert!(db <= 0.02 && db >= -1.05); } }
    #[test] fn chebyshev2_stopband_ripple_bounded() { let s = filter(FilterFamily::ChebyshevII, ResponseType::Lowpass, 4); assert!(magnitude_response_db(&s, 5000.0, 48000.0) < -20.0); }
    #[test] fn bessel_n2_prototype_denominator() { let poles = bessel_poles(2).unwrap_or(&[]); let pole = poles[0]; assert_close(-2.0 * pole.re, 3.0, 0.02); assert_close(pole.re * pole.re + pole.im * pole.im, 3.0, 0.02); }
    #[test] fn invalid_sample_rate_does_not_panic() { assert!(design_filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2, 1000.0, f64::NAN, 1.0, 40.0).is_empty()); }
    #[test] fn invalid_cutoff_does_not_panic() { assert!(design_filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2, 0.0, 48000.0, 1.0, 40.0).is_empty()); }
    #[test] fn order_zero_returns_input() { let input = [0.2, -0.4]; assert_eq!(process_filter(&input, &filter(FilterFamily::Butterworth, ResponseType::Lowpass, 0)), input); }
    #[test] fn impulse_response_decays() { let mut input = vec![0.0; 256]; input[0] = 1.0; let output = process_filter(&input, &filter(FilterFamily::Butterworth, ResponseType::Lowpass, 4)); assert!(output[255].abs() < output[0].abs()); }
    #[test] fn zero_length_input_is_empty() { assert!(process_filter(&[], &filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2)).is_empty()); }
    #[test] fn bounded_input_bounded_output() { let input = vec![1.0; 256]; let output = process_filter(&input, &filter(FilterFamily::Butterworth, ResponseType::Lowpass, 4)); assert!(output.iter().all(|sample| sample.abs() <= 2.0)); }
    #[test] fn output_has_no_nan() { let output = process_filter(&[f32::NAN, 0.5, f32::INFINITY], &filter(FilterFamily::Butterworth, ResponseType::Lowpass, 2)); assert!(output.iter().all(|sample| sample.is_finite())); }
}
