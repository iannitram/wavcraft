//! The spectral frequency display: energy over time and frequency, rendered for whatever time
//! range and pixel size the screen asks for.
//!
//! Nothing is precomputed. Each pixel column gets one windowed FFT centred on its moment in the
//! file, so a view costs `columns` FFTs whatever the file's length, and memory stays flat. The
//! price: zoomed far out, each column samples only `fft_size` frames of the time it covers, so a
//! click shorter than a column can fall between two of them.

use std::sync::Arc;

use rayon::prelude::*;
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Most columns or rows one render produces; larger requests are clamped.
pub const MAX_SIDE: usize = 8192;

/// How to analyse and scale the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectrogramParams {
    /// FFT length in frames; rounded to a power of two between 64 and 16384. Longer resolves
    /// pitch better, shorter resolves time better.
    pub fft_size: usize,
    /// Lowest frequency shown, in Hz.
    pub min_hz: f32,
    /// Highest frequency shown, in Hz; clamped to half the sample rate.
    pub max_hz: f32,
    /// Logarithmic frequency axis (speech-friendly) instead of linear.
    pub log_freq: bool,
    /// Level drawn as 0, in dBFS.
    pub floor_db: f32,
    /// Level drawn as 255, in dBFS. A full-scale sine reads 0 dBFS.
    pub ceil_db: f32,
}

impl Default for SpectrogramParams {
    fn default() -> Self {
        SpectrogramParams { fft_size: 2048, min_hz: 40.0, max_hz: 24_000.0, log_freq: true, floor_db: -100.0, ceil_db: 0.0 }
    }
}

impl SpectrogramParams {
    fn fft_len(&self) -> usize {
        self.fft_size.clamp(64, 16_384).next_power_of_two().min(16_384)
    }

    /// The usable frequency range for `sample_rate`: `(low, high)` in Hz with `low < high`.
    pub fn range_hz(&self, sample_rate: u32) -> (f32, f32) {
        let nyquist = (sample_rate as f32 / 2.0).max(2.0);
        let hi = if self.max_hz.is_finite() { self.max_hz.clamp(2.0, nyquist) } else { nyquist };
        let lo = if self.min_hz.is_finite() { self.min_hz.clamp(1.0, hi / 2.0) } else { 1.0 };
        (lo, hi)
    }

    /// The frequency at the top edge of `row` of `rows` (row 0 is the top of the picture, the
    /// highest frequency; `row == rows` is the bottom edge).
    pub fn row_edge_hz(&self, row: usize, rows: usize, sample_rate: u32) -> f32 {
        let (lo, hi) = self.range_hz(sample_rate);
        let t = if rows == 0 { 0.0 } else { 1.0 - (row.min(rows) as f32 / rows as f32) };
        if self.log_freq { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
    }
}

/// Render `cols` x `rows` levels (0..=255) for channel data `samples` over the frame range
/// `[start, end)`. The result is row-major with row 0 at the top (highest frequency), ready to
/// become an image. Anything that cannot be analysed (no samples, a broken range) renders as 0.
pub fn render(samples: &[f32], sample_rate: u32, start: f64, end: f64, cols: usize, rows: usize, p: &SpectrogramParams) -> Vec<u8> {
    let (cols, rows) = (cols.min(MAX_SIDE), rows.min(MAX_SIDE));
    let mut image = vec![0u8; cols * rows];
    if cols == 0 || rows == 0 || samples.is_empty() || sample_rate == 0 || !start.is_finite() || !end.is_finite() || end <= start {
        return image;
    }
    let n = p.fft_len();
    let fft: Arc<dyn Fft<f32>> = FftPlanner::new().plan_fft_forward(n);
    // Hann window, scaled so a full-scale sine reads 0 dBFS.
    let window: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect();
    let gain = 2.0 / window.iter().sum::<f32>().max(f32::MIN_POSITIVE);
    // Which FFT bins each row covers: [first, last), at least one bin.
    let bin_hz = sample_rate as f32 / n as f32;
    let half = n / 2;
    let bands: Vec<(usize, usize)> = (0..rows)
        .map(|r| {
            let top = p.row_edge_hz(r, rows, sample_rate) / bin_hz;
            let bottom = p.row_edge_hz(r + 1, rows, sample_rate) / bin_hz;
            let first = (bottom.floor().max(0.0) as usize).min(half);
            let last = (top.ceil().max(0.0) as usize).clamp(first + 1, half + 1);
            (first, last)
        })
        .collect();
    let span = (p.ceil_db - p.floor_db).max(1e-3);
    let per_col = (end - start) / cols as f64;

    // Column-major while computing (each column is one FFT), transposed to rows at the end.
    let mut by_column = vec![0u8; cols * rows];
    by_column.par_chunks_mut(rows).enumerate().for_each_init(
        || (vec![Complex32::default(); n], vec![Complex32::default(); fft.get_inplace_scratch_len()]),
        |(frame, scratch), (c, column)| {
            let centre = start + (c as f64 + 0.5) * per_col;
            let first = centre.floor() as i64 - (n / 2) as i64;
            for (i, (slot, w)) in frame.iter_mut().zip(&window).enumerate() {
                let at = first + i as i64;
                let v = usize::try_from(at).ok().and_then(|at| samples.get(at)).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                *slot = Complex32::new(v * w, 0.0);
            }
            fft.process_with_scratch(frame, scratch);
            for (level, (lo, hi)) in column.iter_mut().zip(&bands) {
                let peak = frame.get(*lo..*hi).unwrap_or(&[]).iter().map(|b| b.norm_sqr()).fold(0.0f32, f32::max);
                let db = 10.0 * (peak * gain * gain).max(1e-20).log10();
                *level = (((db - p.floor_db) / span).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        },
    );
    for (r, row) in image.chunks_mut(cols).enumerate() {
        for (c, px) in row.iter_mut().enumerate() {
            *px = by_column.get(c * rows + r).copied().unwrap_or(0);
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amp: f32, frames: usize, rate: u32) -> Vec<f32> {
        (0..frames).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect()
    }

    /// The row of column `c` with the highest level.
    fn brightest_row(image: &[u8], cols: usize, rows: usize, c: usize) -> usize {
        (0..rows).max_by_key(|r| image[r * cols + c]).unwrap()
    }

    #[test]
    fn a_full_scale_sine_is_a_bright_line_at_its_frequency() {
        let rate = 48_000;
        let x = sine(1_000.0, 1.0, 48_000, rate);
        for log_freq in [true, false] {
            let p = SpectrogramParams { log_freq, ..SpectrogramParams::default() };
            let (cols, rows) = (32, 256);
            let image = render(&x, rate, 0.0, 48_000.0, cols, rows, &p);
            assert_eq!(image.len(), cols * rows);
            let r = brightest_row(&image, cols, rows, 16);
            let (top, bottom) = (p.row_edge_hz(r, rows, rate), p.row_edge_hz(r + 1, rows, rate));
            // The row holding the peak contains 1 kHz, give or take one FFT bin (23 Hz).
            assert!(bottom - 24.0 <= 1_000.0 && 1_000.0 <= top + 24.0, "log {log_freq}: row {r} covers {bottom}..{top} Hz");
            assert!(image[r * cols + 16] >= 252, "0 dBFS should be near white, got {}", image[r * cols + 16]);
            // Far from the tone it is dark.
            assert!(image[cols + 16] < 40, "top rows should be quiet, got {}", image[cols + 16]);
        }
    }

    #[test]
    fn levels_follow_decibels() {
        let rate = 48_000;
        let p = SpectrogramParams::default();
        let image = render(&sine(2_000.0, 0.1, 48_000, rate), rate, 0.0, 48_000.0, 8, 128, &p);
        let r = brightest_row(&image, 8, 128, 4);
        // -20 dBFS on a -100..0 dB scale is 80% of 255 = 204.
        assert!((i32::from(image[r * 8 + 4]) - 204).abs() <= 3, "{}", image[r * 8 + 4]);
        assert!(render(&vec![0.0; 48_000], rate, 0.0, 48_000.0, 8, 128, &p).iter().all(|v| *v == 0), "silence is black");
    }

    #[test]
    fn columns_show_their_own_moment_in_time() {
        let rate = 48_000;
        let mut x = vec![0.0; 48_000];
        x.extend(sine(4_000.0, 0.5, 48_000, rate));
        let (cols, rows) = (10, 64);
        let image = render(&x, rate, 0.0, 96_000.0, cols, rows, &SpectrogramParams::default());
        let column_max = |c: usize| (0..rows).map(|r| image[r * cols + c]).max().unwrap();
        assert!(column_max(1) == 0 && column_max(3) == 0, "the first second is silent");
        assert!(column_max(6) > 200 && column_max(8) > 200, "the second holds the tone");
        // Zooming into the tone alone shows it in every column, including the edges that lean
        // on audio outside the view.
        let zoomed = render(&x, rate, 60_000.0, 61_000.0, 4, rows, &SpectrogramParams::default());
        assert!((0..4).all(|c| (0..rows).map(|r| zoomed[r * 4 + c]).max().unwrap() > 200));
    }

    #[test]
    fn odd_requests_render_blank_instead_of_failing() {
        let x = sine(440.0, 0.5, 4_800, 48_000);
        let p = SpectrogramParams::default();
        assert!(render(&x, 48_000, 0.0, 4_800.0, 0, 16, &p).is_empty());
        assert!(render(&x, 48_000, 0.0, 4_800.0, 16, 0, &p).is_empty());
        assert_eq!(render(&[], 48_000, 0.0, 4_800.0, 4, 4, &p), vec![0; 16]);
        assert_eq!(render(&x, 0, 0.0, 4_800.0, 4, 4, &p), vec![0; 16]);
        assert_eq!(render(&x, 48_000, f64::NAN, 4_800.0, 4, 4, &p), vec![0; 16]);
        assert_eq!(render(&x, 48_000, 100.0, 100.0, 4, 4, &p), vec![0; 16]);
        // Past the end of the file and before its start: silence.
        assert_eq!(render(&x, 48_000, 1e9, 1e9 + 100.0, 4, 4, &p), vec![0; 16]);
        assert_eq!(render(&x, 48_000, -1e9, -1e9 + 100.0, 4, 4, &p), vec![0; 16]);
        // NaN samples, absurd parameters and oversized requests are clamped, not trusted.
        let wild = SpectrogramParams { fft_size: usize::MAX, min_hz: f32::NAN, max_hz: f32::INFINITY, floor_db: 0.0, ceil_db: 0.0, log_freq: true };
        let nan = vec![f32::NAN; 4_800];
        assert_eq!(render(&nan, 48_000, 0.0, 4_800.0, 4, 4, &wild), vec![0; 16]);
        assert_eq!(render(&x, 48_000, 0.0, 4_800.0, usize::MAX, 2, &p).len(), MAX_SIDE * 2);
    }

    #[test]
    fn the_frequency_axis_runs_high_to_low_inside_the_range() {
        let p = SpectrogramParams::default();
        assert_eq!(p.range_hz(48_000), (40.0, 24_000.0));
        assert_eq!(p.range_hz(16_000), (40.0, 8_000.0), "clamped to Nyquist");
        assert!((p.row_edge_hz(0, 100, 48_000) - 24_000.0).abs() < 1.0);
        assert!((p.row_edge_hz(100, 100, 48_000) - 40.0).abs() < 0.01);
        let edges: Vec<f32> = (0..=100).map(|r| p.row_edge_hz(r, 100, 48_000)).collect();
        assert!(edges.windows(2).all(|w| w[0] > w[1]));
    }
}
