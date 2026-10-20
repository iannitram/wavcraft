//! Sample-rate conversion for playback: a windowed-sinc (Kaiser) interpolator that can be asked
//! for the file's signal at any fractional frame.
//!
//! It is stateless: output frame `n` of a run that starts at document frame `start` is the
//! signal at `start + n * in_rate / out_rate`, worked out from the integer `n` each time, so
//! nothing accumulates and a run of any length cannot drift. A player renders it on a worker
//! thread into a ring buffer; the audio callback only copies. Audition is a behaviour reference
//! only; the maths is the textbook windowed sinc (own code, no dependency).
//!
//! When the rates match the resampler is the identity and copies frames exactly.

use std::f64::consts::PI;

/// How much work the interpolator does per output frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    /// 16 taps, cut-off at 90 % of the lower Nyquist: for slow machines.
    Fast,
    /// 64 taps, cut-off at 95 % of the lower Nyquist.
    #[default]
    High,
}

impl Quality {
    /// The word used in the settings file and on the control channel.
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Fast => "fast",
            Quality::High => "high",
        }
    }

    /// `fast` or `high` (any case), else `None`.
    pub fn parse(s: &str) -> Option<Quality> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fast" => Some(Quality::Fast),
            "high" => Some(Quality::High),
            _ => None,
        }
    }

    /// (taps each side at unity ratio, Kaiser beta, cut-off as a fraction of Nyquist, table steps per frame)
    fn design(self) -> (f64, f64, f64, usize) {
        match self {
            Quality::Fast => (8.0, 5.0, 0.90, 128),
            Quality::High => (32.0, 9.0, 0.95, 1024),
        }
    }
}

/// Modified Bessel function of the first kind, order 0 (the Kaiser window's shape).
fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..60 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-14 * sum {
            break;
        }
    }
    sum
}

/// Converts between two sample rates by windowed-sinc interpolation.
#[derive(Debug, Clone)]
pub struct Resampler {
    in_rate: u32,
    out_rate: u32,
    quality: Quality,
    /// Input frames per output frame (`in_rate / out_rate`).
    ratio: f64,
    /// Kernel half-width in input frames.
    width: f64,
    /// Kernel samples per input frame.
    steps: f64,
    /// The kernel from 0 to `width`, one extra zero at the end for interpolation.
    table: Vec<f32>,
}

impl Resampler {
    /// A resampler from `in_rate` to `out_rate` (both above 0; a 0 is taken as 1).
    pub fn new(in_rate: u32, out_rate: u32, quality: Quality) -> Resampler {
        let (in_rate, out_rate) = (in_rate.max(1), out_rate.max(1));
        let ratio = f64::from(in_rate) / f64::from(out_rate);
        let (half, beta, cutoff, os) = quality.design();
        // Going down in rate the kernel is stretched so everything above the new Nyquist is cut.
        let scale = (1.0 / ratio).min(1.0);
        let gain = cutoff * scale;
        let width = half / scale;
        let n = (width * os as f64).ceil() as usize;
        let i0_beta = bessel_i0(beta);
        let mut table = Vec::with_capacity(n + 2);
        for i in 0..=n {
            let t = i as f64 / os as f64;
            let x = PI * gain * t;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
            let r = (t / width).min(1.0);
            let window = bessel_i0(beta * (1.0 - r * r).sqrt()) / i0_beta;
            table.push((gain * sinc * window) as f32);
        }
        table.push(0.0);
        Resampler { in_rate, out_rate, quality, ratio, width, steps: os as f64, table }
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// True when the rates are equal and frames are copied unchanged.
    pub fn is_identity(&self) -> bool {
        self.in_rate == self.out_rate
    }

    /// Input frames per output frame.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// The input (document) frame that output frame `n` of a run starting at `start` falls on.
    pub fn doc_position(&self, start: f64, n: u64) -> f64 {
        start + n as f64 * self.ratio
    }

    /// How many output frames cover the input frames `[start, end)`.
    pub fn out_frames(&self, start: f64, end: f64) -> u64 {
        if end <= start { 0 } else { ((end - start) / self.ratio).ceil() as u64 }
    }

    fn kernel(&self, t: f64) -> f32 {
        let u = t.abs() * self.steps;
        let i = u as usize;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * (u - i as f64) as f32,
            _ => 0.0,
        }
    }

    /// The signal in `src` at fractional frame `x`; frames outside `src` are silence.
    pub fn sample_at(&self, src: &[f32], x: f64) -> f32 {
        if self.is_identity() {
            let i = x.round();
            return if i >= 0.0 { src.get(i as usize).copied().unwrap_or(0.0) } else { 0.0 };
        }
        let first = (x - self.width).ceil() as i64;
        let last = (x + self.width).floor() as i64;
        let (mut acc, mut weights) = (0.0f32, 0.0f32);
        for k in first..=last {
            let w = self.kernel(x - k as f64);
            weights += w;
            if k >= 0
                && let Some(s) = src.get(k as usize)
            {
                acc += w * s;
            }
        }
        if weights.abs() > 1e-6 { acc / weights } else { 0.0 }
    }

    /// Fill `out` with output frames `first..first + out.len()` of the run that starts at input
    /// frame `start`. Calling it in pieces gives the same samples as one call.
    pub fn render(&self, src: &[f32], start: f64, first: u64, out: &mut [f32]) {
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.sample_at(src, self.doc_position(start, first + i as u64));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32).collect()
    }

    /// Frequency from the spacing of rising zero crossings (linear interpolation between samples).
    fn measure_hz(x: &[f32], rate: u32) -> f64 {
        let mut crossings = Vec::new();
        for i in 1..x.len() {
            if x[i - 1] < 0.0 && x[i] >= 0.0 {
                let frac = f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]);
                crossings.push(i as f64 - 1.0 + frac);
            }
        }
        let n = crossings.len();
        assert!(n > 10, "too few crossings: {n}");
        (n - 1) as f64 * f64::from(rate) / (crossings[n - 1] - crossings[0])
    }

    #[test]
    fn a_sine_keeps_its_frequency_and_level() {
        // Measured: 1 kHz and 10 kHz, 44.1 -> 48 kHz and 48 -> 44.1 kHz, both qualities.
        for (q, tol_hz, tol_amp) in [(Quality::High, 0.05, 1e-3), (Quality::Fast, 0.1, 2e-2)] {
            for (a, b) in [(44_100u32, 48_000u32), (48_000, 44_100), (22_050, 48_000)] {
                for f in [1_000.0, 10_000.0f64.min(0.3 * f64::from(a.min(b)))] {
                    let input = sine(f, a, a as usize);
                    let r = Resampler::new(a, b, q);
                    let n = r.out_frames(0.0, input.len() as f64) as usize;
                    let mut out = vec![0.0f32; n];
                    r.render(&input, 0.0, 0, &mut out);
                    let hz = measure_hz(&out[2_000..n - 2_000], b);
                    assert!((hz - f).abs() < tol_hz, "{q:?} {a}->{b} {f} Hz measured {hz}");
                    let worst = out[2_000..n - 2_000]
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (f64::from(*v) - (2.0 * PI * f * (i + 2_000) as f64 / f64::from(b)).sin()).abs())
                        .fold(0.0, f64::max);
                    assert!(worst < tol_amp, "{q:?} {a}->{b} {f} Hz worst error {worst}");
                }
            }
        }
    }

    #[test]
    fn above_the_new_nyquist_is_removed_when_going_down() {
        let input = sine(20_000.0, 48_000, 48_000);
        let r = Resampler::new(48_000, 32_000, Quality::High);
        let n = r.out_frames(0.0, input.len() as f64) as usize;
        let mut out = vec![0.0f32; n];
        r.render(&input, 0.0, 0, &mut out);
        let peak = out[2_000..n - 2_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.01, "a 20 kHz tone must not alias into 32 kHz output: peak {peak}");
    }

    #[test]
    fn dc_passes_at_unity_everywhere() {
        let input = vec![0.5f32; 4_000];
        for q in [Quality::Fast, Quality::High] {
            let r = Resampler::new(44_100, 48_000, q);
            let mut out = vec![0.0f32; 3_000];
            r.render(&input, 100.0, 0, &mut out);
            assert!(out[100..2_800].iter().all(|v| (v - 0.5).abs() < 1e-4), "{q:?}");
        }
    }

    #[test]
    fn identical_rates_copy_exactly() {
        let input: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        let r = Resampler::new(48_000, 48_000, Quality::High);
        assert!(r.is_identity());
        let mut out = vec![9.0f32; 10];
        r.render(&input, 20.0, 0, &mut out);
        assert_eq!(out, input[20..30]);
        // Past the end is silence.
        let mut tail = vec![9.0f32; 4];
        r.render(&input, 98.0, 0, &mut tail);
        assert_eq!(tail, [input[98], input[99], 0.0, 0.0]);
    }

    #[test]
    fn position_maps_between_rates_and_back() {
        let r = Resampler::new(44_100, 48_000, Quality::High);
        // One second of output is 44,100 input frames: exactly.
        assert_eq!(r.doc_position(0.0, 48_000), 44_100.0);
        assert_eq!(r.doc_position(1_000.0, 96_000), 1_000.0 + 88_200.0);
        // And the count of output frames covering a range.
        assert_eq!(r.out_frames(0.0, 44_100.0), 48_000);
        assert_eq!(r.out_frames(10.0, 10.0), 0);
        let down = Resampler::new(48_000, 44_100, Quality::Fast);
        assert_eq!(down.out_frames(0.0, 48_000.0), 44_100);
        assert!((down.doc_position(5.0, 44_100) - 48_005.0).abs() < 1e-6);
    }

    #[test]
    fn no_drift_over_a_long_run() {
        // Position maths for ten hours of output: the mapped position equals the exact rational
        // value to far better than a thousandth of a frame (measured against integer arithmetic).
        let r = Resampler::new(44_100, 48_000, Quality::High);
        for hours in [1u64, 10] {
            let n = hours * 3_600 * 48_000;
            let exact = n as f64 * 44_100.0 / 48_000.0; // n * 441 / 480, n a multiple of 480
            assert_eq!(r.doc_position(0.0, n), exact);
            assert_eq!(r.doc_position(0.0, n), (n / 480 * 441) as f64);
        }
        // Rendered in 1,000-frame chunks for a minute, the last sample still sits on the sine's
        // phase and the chunks equal a single render, bit for bit.
        let input = sine(1_000.0, 44_100, 44_100 * 61);
        let total = 48_000 * 60;
        let mut whole = vec![0.0f32; total];
        r.render(&input, 0.0, 0, &mut whole);
        let mut chunked = vec![0.0f32; total];
        for (i, c) in chunked.chunks_mut(1_000).enumerate() {
            r.render(&input, 0.0, (i * 1_000) as u64, c);
        }
        assert_eq!(whole, chunked);
        let k = total - 1;
        let want = (2.0 * PI * 1_000.0 * k as f64 / 48_000.0).sin();
        assert!((f64::from(whole[k]) - want).abs() < 1e-3, "phase after a minute: {} vs {want}", whole[k]);
    }

    #[test]
    fn quality_words_round_trip() {
        assert_eq!(Quality::parse(" HIGH "), Some(Quality::High));
        assert_eq!(Quality::parse("fast"), Some(Quality::Fast));
        assert_eq!(Quality::parse("best"), None);
        assert_eq!(Quality::parse(Quality::Fast.as_str()), Some(Quality::Fast));
    }
}
