//! The spectral frequency display: energy over time and frequency, rendered for whatever time
//! range and pixel size the screen asks for.
//!
//! Nothing is precomputed, so memory stays flat. Zoomed in (a pixel column spans one FFT window
//! or less) each column is one windowed FFT centred on its moment in the file. Zoomed out, a
//! column shows the loudest level in each frequency band across all the audio it spans: FFTs are
//! taken half a window apart on a grid fixed to the file, and each column keeps the per-bin
//! maximum of the ones centred inside it. Nothing falls between two columns, at the price that a
//! view costs time in proportion to the audio on screen.

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

/// Whole picture columns on a grid fixed to the file: the range and column count to hand
/// [`render`] so that a column shows the same audio wherever the view has scrolled to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Columns {
    /// First frame of the first column: at or before the view's start.
    pub start: f64,
    /// One past the last frame of the last column: at or after the view's end.
    pub end: f64,
    /// How many columns that is; never more than [`MAX_SIDE`].
    pub cols: usize,
}

impl Columns {
    /// The columns covering the view `[start, end)` when one column should span about `per_col`
    /// frames. Column edges are multiples of the column's width counted from the start of the
    /// file, not from the view. Zoomed out, the width is rounded to a whole number of FFT hops
    /// (so it can differ from `per_col` by up to a quarter), which gives every column the same
    /// number of FFTs. A request that makes no sense has no columns.
    pub fn covering(start: f64, end: f64, per_col: f64, p: &SpectrogramParams) -> Columns {
        let none = Columns { start, end, cols: 0 };
        if !start.is_finite() || !end.is_finite() || !per_col.is_finite() || end <= start || per_col <= 0.0 {
            return none;
        }
        let n = p.fft_len() as f64;
        let width = if per_col > n { (per_col / (n / 2.0)).round() * (n / 2.0) } else { per_col };
        let first = (start / width).floor() * width;
        let cols = ((end - first) / width).ceil().clamp(1.0, MAX_SIDE as f64);
        let last = first + cols * width;
        if first.is_finite() && last.is_finite() && cols.is_finite() { Columns { start: first, end: last, cols: cols as usize } } else { none }
    }
}

/// One FFT after another over the same scratch memory, keeping the loudest level per bin.
struct Analyser<'a> {
    fft: Arc<dyn Fft<f32>>,
    window: &'a [f32],
    frame: Vec<Complex32>,
    scratch: Vec<Complex32>,
    /// The largest squared magnitude seen in each bin, 0 Hz to Nyquist, since it was last cleared.
    peaks: Vec<f32>,
}

impl<'a> Analyser<'a> {
    fn new(fft: Arc<dyn Fft<f32>>, window: &'a [f32]) -> Analyser<'a> {
        let (n, scratch) = (window.len(), fft.get_inplace_scratch_len());
        Analyser { fft, window, frame: vec![Complex32::default(); n], scratch: vec![Complex32::default(); scratch], peaks: vec![0.0; n / 2 + 1] }
    }

    /// Analyse the window of frames that begins at `first` and fold it into `peaks`. Frames
    /// outside the file, and samples that are not numbers, count as silence.
    fn add(&mut self, samples: &[f32], first: i64) {
        let n = self.window.len();
        let whole = usize::try_from(first).ok().and_then(|at| samples.get(at..at.checked_add(n)?));
        if let Some(whole) = whole {
            for ((slot, w), v) in self.frame.iter_mut().zip(self.window).zip(whole) {
                *slot = Complex32::new(if v.is_finite() { v * w } else { 0.0 }, 0.0);
            }
        } else {
            for (i, (slot, w)) in self.frame.iter_mut().zip(self.window).enumerate() {
                let at = first.saturating_add(i as i64);
                let v = usize::try_from(at).ok().and_then(|at| samples.get(at)).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                *slot = Complex32::new(v * w, 0.0);
            }
        }
        self.fft.process_with_scratch(&mut self.frame, &mut self.scratch);
        for (peak, bin) in self.peaks.iter_mut().zip(&self.frame) {
            *peak = peak.max(bin.norm_sqr());
        }
    }
}

/// Render `cols` x `rows` levels (0..=255) for channel data `samples` over the frame range
/// `[start, end)`. The result is row-major with row 0 at the top (highest frequency), ready to
/// become an image. Anything that cannot be analysed (no samples, a broken range) renders as 0.
///
/// A column spanning more than one FFT window shows the loudest level across its whole span;
/// take the range from [`Columns::covering`] to keep those columns still while scrolling.
pub fn render(samples: &[f32], sample_rate: u32, start: f64, end: f64, cols: usize, rows: usize, p: &SpectrogramParams) -> Vec<u8> {
    let (cols, rows) = (cols.min(MAX_SIDE), rows.min(MAX_SIDE));
    let mut image = vec![0u8; cols * rows];
    let per_col = (end - start) / cols.max(1) as f64;
    if cols == 0
        || rows == 0
        || samples.is_empty()
        || sample_rate == 0
        || !start.is_finite()
        || !end.is_finite()
        || !per_col.is_finite()
        || end <= start
    {
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
    // Zoomed out: the grid's FFTs are centred on the odd multiples of half a hop, so a column
    // that starts and ends on a hop (see `Columns`) holds a whole number of them, evenly inside
    // it. Only those whose window reaches the file are computed, however wide the request.
    let hop = (n / 2) as i64;
    let grid_end = (samples.len() / (n / 2)) as i64 + 2;
    let grid_index = |frame: f64| (frame / hop as f64 - 0.5).ceil() as i64;

    // Column-major while computing (each column is its own FFTs), transposed to rows at the end.
    let mut by_column = vec![0u8; cols * rows];
    by_column.par_chunks_mut(rows).enumerate().for_each_init(
        || Analyser::new(fft.clone(), &window),
        |analyser, (c, column)| {
            analyser.peaks.fill(0.0);
            if per_col > n as f64 {
                let (from, to) = (grid_index(start + c as f64 * per_col), grid_index(start + (c + 1) as f64 * per_col));
                for g in from.max(-1)..to.min(grid_end) {
                    analyser.add(samples, g * hop - hop / 2);
                }
            } else {
                let centre = start + (c as f64 + 0.5) * per_col;
                analyser.add(samples, (centre.floor() as i64).saturating_sub(hop));
            }
            for (level, (lo, hi)) in column.iter_mut().zip(&bands) {
                let peak = analyser.peaks.get(*lo..*hi).unwrap_or(&[]).iter().copied().fold(0.0f32, f32::max);
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

    /// Steady hiss between `-amp` and `amp`, the same every run.
    fn noise(frames: usize, amp: f32) -> Vec<f32> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..frames)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 2.0 * amp
            })
            .collect()
    }

    #[test]
    fn a_short_tone_is_never_lost_by_zooming_out() {
        // Forty minutes at 8 kHz with a 32 ms window, fitted to the window: a column spans 37 to
        // 117 FFT windows and 1.2 to 3.8 seconds, the proportions of three hours at 48 kHz.
        let rate = 8_000;
        let frames = 40 * 60 * rate as usize;
        let p = SpectrogramParams { fft_size: 256, ..SpectrogramParams::default() };
        // One second of full-scale 3 kHz in quiet hiss. It sits where one FFT at the centre of
        // each column (the first version of this display) missed it at all four widths.
        let tone_at = 8_752_700;
        let mut x = noise(frames, 0.001);
        x[tone_at..tone_at + rate as usize].copy_from_slice(&sine(3_000.0, 1.0, rate as usize, rate));
        let rows = 128;
        let tone_row = (0..rows).find(|r| p.row_edge_hz(r + 1, rows, rate) <= 3_000.0).unwrap();
        for cols in [640, 960, 1280, 2048] {
            let image = render(&x, rate, 0.0, frames as f64, cols, rows, &p);
            let row = &image[tone_row * cols..(tone_row + 1) * cols];
            let tone_col = (tone_at + rate as usize / 2) * cols / frames;
            let hiss = row.iter().enumerate().filter(|(c, _)| c.abs_diff(tone_col) > 1).map(|(_, v)| *v).max().unwrap();
            assert!(row[tone_col] >= 250, "{cols} columns: the tone reads {} in column {tone_col}, the hiss up to {hiss}", row[tone_col]);
            assert!(hiss < 130, "{cols} columns: the hiss reads up to {hiss}, too close to the tone");
        }
    }

    #[test]
    fn scrolling_moves_the_picture_without_changing_it() {
        // Hiss that swells and fades, so no two stretches of the file look alike.
        let rate = 8_000;
        let p = SpectrogramParams { fft_size: 256, ..SpectrogramParams::default() };
        let x: Vec<f32> = noise(2_000_000, 0.5).into_iter().enumerate().map(|(i, v)| v * (0.55 + 0.45 * (i as f32 / 9_000.0).sin())).collect();
        // A 1000 px window at 1200 frames per pixel, drawn at half resolution as the app does:
        // one pixel is half a column, and a column spans many FFT windows.
        let (per_px, width, rows) = (1_200.0, 1_000.0, 64);
        let picture = |start: f64| {
            let c = Columns::covering(start, start + per_px * width, per_px * 2.0, &p);
            assert!(c.start <= start && c.end >= start + per_px * width, "{c:?} covers the view from {start}");
            (c, render(&x, rate, c.start, c.end, c.cols, rows, &p))
        };
        let (a, before) = picture(300_000.0);
        let column = (a.end - a.start) / a.cols as f64;
        assert!(before.iter().any(|v| *v > 100), "there is something to see");
        for px in [1.0, 2.0, 3.0, 7.0, 40.0] {
            let (b, after) = picture(300_000.0 + px * per_px);
            assert_eq!((b.end - b.start) / b.cols as f64, column, "the same zoom has the same columns");
            // Column `c` of the scrolled picture and column `c + shift` of the first cover the
            // same frames, so they are the same column.
            let shift = (b.start - a.start) / column;
            assert_eq!(shift, shift.round(), "scrolled {px} px: columns stay on the file's grid");
            let shift = shift as usize;
            for c in 0..(a.cols - shift).min(b.cols) {
                for r in 0..rows {
                    assert_eq!(after[r * b.cols + c], before[r * a.cols + c + shift], "scrolled {px} px: column {c}, row {r}");
                }
            }
        }
    }

    #[test]
    fn columns_sit_on_a_grid_fixed_to_the_file() {
        let p = SpectrogramParams::default();
        // Zoomed out: 5000 frames per column becomes 5 hops of 1024, and the view is widened to whole columns.
        let c = Columns::covering(1_000_000.0, 2_000_000.0, 5_000.0, &p);
        assert_eq!(c, Columns { start: 998_400.0, end: 2_001_920.0, cols: 196 });
        assert_eq!((c.start % 5_120.0, (c.end - c.start) / c.cols as f64), (0.0, 5_120.0));
        // Zoomed in: the width asked for, counted from the start of the file.
        let c = Columns::covering(1_000.0, 2_000.0, 300.0, &p);
        assert_eq!(c, Columns { start: 900.0, end: 2_100.0, cols: 4 });
        assert_eq!(Columns::covering(0.0, 10.0, 0.25, &p), Columns { start: 0.0, end: 10.0, cols: 40 });
        // Nonsense has no columns, and nothing asks for more than `MAX_SIDE`.
        for (start, end, per_col) in
            [(0.0, 0.0, 1.0), (5.0, 1.0, 1.0), (0.0, 9.0, 0.0), (0.0, 9.0, -2.0), (f64::NAN, 9.0, 1.0), (0.0, f64::INFINITY, 1.0)]
        {
            assert_eq!(Columns::covering(start, end, per_col, &p).cols, 0, "{start} {end} {per_col}");
        }
        assert_eq!(Columns::covering(0.0, 9.0, f64::NAN, &p).cols, 0);
        assert_eq!(Columns::covering(0.0, 1e9, 1e-300, &p).cols, MAX_SIDE);
        assert_eq!(Columns::covering(1e300, 1e301, 1e-300, &p).cols, 0);
        assert_eq!(Columns::covering(0.0, 1e12, 1.0, &p).cols, MAX_SIDE);
    }

    #[test]
    fn a_view_far_wider_than_the_file_costs_no_more_than_the_file() {
        // Each column spans 5e14 frames; only the FFTs that touch the tenth of a second of audio run.
        let x = sine(440.0, 0.5, 4_800, 48_000);
        let image = render(&x, 48_000, -1e15, 1e15, 4, 4, &SpectrogramParams::default());
        let column_max = |c: usize| (0..4).map(|r| image[r * 4 + c]).max().unwrap();
        assert_eq!((column_max(0), column_max(3)), (0, 0));
        assert!(column_max(2) > 200, "the audio is in the column that starts at frame 0");
        assert!(column_max(1) < column_max(2), "the column before it holds only the one FFT that leans into the file");
        assert_eq!(render(&x, 48_000, -f64::MAX, f64::MAX, 4, 4, &SpectrogramParams::default()), vec![0; 16]);
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
