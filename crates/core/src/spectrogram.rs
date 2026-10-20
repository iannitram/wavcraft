//! The spectral frequency display: energy over time and frequency, rendered for whatever time
//! range and pixel size the screen asks for.
//!
//! Zoomed in (a pixel column spans one FFT window or less) each column is one windowed FFT
//! centred on its moment in the file. Zoomed out, a column shows the loudest level in each
//! frequency band across all the audio it spans: FFTs are taken half a window apart on a grid
//! fixed to the file, and each column keeps the per-bin maximum of the ones centred inside it.
//! Nothing falls between two columns, at the price that a view costs time in proportion to the
//! audio on screen.
//!
//! An [`Overview`] pays that price once per file: it keeps the per-bin maximum of every
//! [`BLOCK_HOPS`] FFTs of the grid, as levels. A column that holds whole blocks reads them and
//! runs FFTs only for the ends that stick out, and comes to the same picture.

use std::ops::Range;
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
    /// The window each FFT is taken through.
    pub window: WindowKind,
}

impl Default for SpectrogramParams {
    fn default() -> Self {
        SpectrogramParams { fft_size: 2048, min_hz: 40.0, max_hz: 24_000.0, log_freq: true, floor_db: -100.0, ceil_db: 0.0, window: WindowKind::Hann }
    }
}

/// The window function an FFT is taken through: it trades how sharp a tone looks against how
/// far the leakage from loud tones spreads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowKind {
    #[default]
    Hann,
    Hamming,
    Blackman,
    BlackmanHarris,
    /// No shaping: the sharpest peaks and the most leakage.
    Rectangular,
}

impl WindowKind {
    /// Every window, in the order a menu lists them.
    pub const ALL: [WindowKind; 5] =
        [WindowKind::Hann, WindowKind::Hamming, WindowKind::Blackman, WindowKind::BlackmanHarris, WindowKind::Rectangular];

    /// The lower-case name used in settings and on the control channel.
    pub fn name(self) -> &'static str {
        match self {
            WindowKind::Hann => "hann",
            WindowKind::Hamming => "hamming",
            WindowKind::Blackman => "blackman",
            WindowKind::BlackmanHarris => "blackman-harris",
            WindowKind::Rectangular => "rectangular",
        }
    }

    /// The name as a person reads it.
    pub fn label(self) -> &'static str {
        match self {
            WindowKind::Hann => "Hann",
            WindowKind::Hamming => "Hamming",
            WindowKind::Blackman => "Blackman",
            WindowKind::BlackmanHarris => "Blackman-Harris",
            WindowKind::Rectangular => "Rectangular",
        }
    }

    /// The window called `text` (case and `-`/`_`/space ignored; `rect` and `boxcar` too).
    pub fn parse(text: &str) -> Option<WindowKind> {
        let key: String = text.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
        Some(match key.as_str() {
            "hann" | "hanning" => WindowKind::Hann,
            "hamming" => WindowKind::Hamming,
            "blackman" => WindowKind::Blackman,
            "blackmanharris" => WindowKind::BlackmanHarris,
            "rectangular" | "rect" | "boxcar" | "none" => WindowKind::Rectangular,
            _ => return None,
        })
    }

    /// The `n` coefficients (periodic form, the one for spectral analysis): a sum of cosines,
    /// `w[i] = a0 - a1 cos(2πi/n) + a2 cos(4πi/n) - a3 cos(6πi/n)`.
    pub fn coefficients(self, n: usize) -> Vec<f32> {
        let a: &[f32] = match self {
            WindowKind::Hann => &[0.5, 0.5],
            WindowKind::Hamming => &[0.54, 0.46],
            WindowKind::Blackman => &[0.42, 0.5, 0.08],
            WindowKind::BlackmanHarris => &[0.35875, 0.48829, 0.14128, 0.01168],
            WindowKind::Rectangular => &[1.0],
        };
        (0..n)
            .map(|i| {
                let phase = std::f32::consts::TAU * i as f32 / n as f32;
                a.iter().enumerate().map(|(k, c)| if k % 2 == 0 { *c } else { -*c } * (phase * k as f32).cos()).sum()
            })
            .collect()
    }
}

/// The FFT lengths the display steps through (the clamp in [`SpectrogramParams`] allows these).
pub const FFT_SIZES: [usize; 9] = [64, 128, 256, 512, 1024, 2048, 4096, 8192, 16_384];
/// The decibel range (loudest minus quietest level drawn) the display allows.
pub const MIN_RANGE_DB: f32 = 20.0;
pub const MAX_RANGE_DB: f32 = 200.0;

/// `size` moved `steps` places along [`FFT_SIZES`] (negative: fewer, finer in time), stopping at
/// the ends. A size that is not on the list counts as the nearest one at or above it.
pub fn step_fft_size(size: usize, steps: i32) -> usize {
    let at = FFT_SIZES.iter().position(|s| *s >= size).unwrap_or(FFT_SIZES.len() - 1) as i64;
    FFT_SIZES[(at + i64::from(steps)).clamp(0, FFT_SIZES.len() as i64 - 1) as usize]
}

/// The level (0..=255) a decibel value is drawn as: `floor_db` is 0, `ceil_db` is 255, linear
/// between, clamped. Not a number draws 0.
pub fn db_to_level(db: f32, floor_db: f32, ceil_db: f32) -> u8 {
    let span = (ceil_db - floor_db).max(1e-3);
    (((db - floor_db) / span).clamp(0.0, 1.0) * 255.0).round() as u8
}

impl SpectrogramParams {
    /// The decibel range drawn: `ceil_db - floor_db`.
    pub fn range_db(&self) -> f32 {
        self.ceil_db - self.floor_db
    }

    /// These with the loudest level kept and the quietest moved so `range` dB are drawn
    /// (clamped to [`MIN_RANGE_DB`]..=[`MAX_RANGE_DB`]; not a number changes nothing).
    pub fn with_range_db(self, range: f32) -> SpectrogramParams {
        if !range.is_finite() {
            return self;
        }
        SpectrogramParams { floor_db: self.ceil_db - range.clamp(MIN_RANGE_DB, MAX_RANGE_DB), ..self }
    }

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

/// What every FFT of one set of parameters shares: the transform, its window and the dB scale.
struct Analysis {
    fft: Arc<dyn Fft<f32>>,
    /// The window, as long as the FFT.
    window: Vec<f32>,
    /// `steps[k]` is the smallest squared magnitude drawn as level `k + 1`; not a number where no
    /// magnitude reaches that level.
    steps: Vec<f32>,
}

impl Analysis {
    fn new(p: &SpectrogramParams) -> Analysis {
        let n = p.fft_len();
        let window = p.window.coefficients(n);
        // Scaled so a full-scale sine reads 0 dBFS.
        let gain = 2.0 / window.iter().sum::<f32>().max(f32::MIN_POSITIVE);
        let scale = |peak: f32| db_to_level(10.0 * (peak * gain * gain).max(1e-20).log10(), p.floor_db, p.ceil_db);
        // The scale as a table of steps, found by bisection over the bit patterns of the positive
        // floats (which sort like the floats). A table cannot put a louder sound below a quieter
        // one, whatever `log10` does in its last bit, and that is what lets the loudest level of
        // some FFTs be kept in place of their loudest magnitude (see `Overview`).
        let mut low = 0u32;
        let steps = (1..=255u8)
            .map(|level| {
                let mut high = f32::INFINITY.to_bits();
                if scale(f32::from_bits(high)) < level {
                    return f32::NAN;
                }
                while low < high {
                    let mid = low + (high - low) / 2;
                    if scale(f32::from_bits(mid)) >= level { high = mid } else { low = mid + 1 }
                }
                f32::from_bits(low)
            })
            .collect();
        Analysis { fft: FftPlanner::new().plan_fft_forward(n), window, steps }
    }

    /// The level (0..=255) drawn for the squared magnitude `peak`.
    fn level(&self, peak: f32) -> u8 {
        self.steps.partition_point(|step| *step <= peak) as u8
    }

    fn analyser(&self) -> Analyser<'_> {
        Analyser::new(self.fft.clone(), &self.window)
    }

    /// Frames between two FFTs of the zoomed-out grid: half a window. FFT `g` of the grid is
    /// centred on frame `(g + 0.5) * hop`.
    fn hop(&self) -> usize {
        (self.window.len() / 2).max(1)
    }

    /// One past the last FFT of the grid whose window can reach a file of `frames` frames. The
    /// first that can is -1.
    fn grid_end(&self, frames: usize) -> i64 {
        (frames / self.hop()) as i64 + 2
    }
}

/// How many FFTs of the grid one block of an [`Overview`] holds.
///
/// A block costs one byte per frequency bin, which comes to one byte per 16 frames at any FFT
/// length: 10.8 MB for an hour of one channel at 48 kHz, 1.6 % of what its samples take. A
/// column still runs the FFTs at its two ends that fill no whole block, 15 on average, and they
/// are nearly all a far view costs. So blocks twice as long would halve the memory and double the
/// time of every far view (measured: 6 to 20 ms for three hours against 4 to 10 ms).
pub const BLOCK_HOPS: usize = 16;

/// One channel's spectrogram at a coarse time step, for drawing views far zoomed out without
/// analysing all the audio on screen again.
///
/// It holds, for each block of [`BLOCK_HOPS`] FFTs of the zoomed-out grid, the level of the
/// loudest of them in every frequency bin. It depends on the FFT length, the window and the dB scale it was
/// built for, not on the frequency axis or the picture's size.
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    fft_len: usize,
    floor_db: f32,
    ceil_db: f32,
    window: WindowKind,
    /// Length of the audio it was built from, in frames.
    frames: usize,
    /// Frequency bins per block: 0 Hz to Nyquist.
    bins: usize,
    /// One level per bin, block after block. Only whole blocks are kept.
    levels: Vec<u8>,
}

impl Overview {
    /// Analyse all of `samples` with `p`'s FFT length and dB scale. This runs every FFT a view of
    /// the whole file would, once.
    pub fn build(samples: &[f32], p: &SpectrogramParams) -> Overview {
        let analysis = Analysis::new(p);
        let hop = analysis.hop() as i64;
        let bins = analysis.window.len() / 2 + 1;
        let blocks = analysis.grid_end(samples.len()) as usize / BLOCK_HOPS;
        let mut levels = vec![0u8; blocks * bins];
        levels.par_chunks_mut(bins).enumerate().for_each_init(
            || analysis.analyser(),
            |analyser, (block, levels)| {
                analyser.peaks.fill(0.0);
                let first = (block * BLOCK_HOPS) as i64;
                for g in first..first + BLOCK_HOPS as i64 {
                    analyser.add(samples, g * hop - hop / 2);
                }
                for (level, peak) in levels.iter_mut().zip(&analyser.peaks) {
                    *level = analysis.level(*peak);
                }
            },
        );
        Overview { fft_len: p.fft_len(), floor_db: p.floor_db, ceil_db: p.ceil_db, window: p.window, frames: samples.len(), bins, levels }
    }

    /// Memory held, in bytes.
    pub fn bytes(&self) -> usize {
        self.levels.len()
    }

    /// Whether this overview can stand in for FFTs of `frames` frames of audio analysed with `p`.
    fn serves(&self, frames: usize, p: &SpectrogramParams) -> bool {
        self.frames == frames && self.fft_len == p.fft_len() && self.floor_db == p.floor_db && self.ceil_db == p.ceil_db && self.window == p.window
    }

    /// Raise `into` (one level per bin) to the loudest of the whole blocks inside the grid range
    /// `[from, to)`, and return the part of the range those blocks cover: empty when there are none.
    fn fold(&self, from: i64, to: i64, into: &mut [u8]) -> Range<i64> {
        let per = BLOCK_HOPS as i64;
        let first = from.saturating_add(per - 1).div_euclid(per).max(0);
        let last = to.div_euclid(per);
        let held = usize::try_from(first).ok().zip(usize::try_from(last).ok()).filter(|(first, last)| first < last);
        let Some(blocks) = held.and_then(|(first, last)| self.levels.get(first * self.bins..last * self.bins)) else { return from..from };
        for block in blocks.chunks_exact(self.bins.max(1)) {
            for (level, held) in into.iter_mut().zip(block) {
                *level = (*level).max(*held);
            }
        }
        first * per..last * per
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
    render_columns(samples, sample_rate, Columns { start, end, cols }, rows, p, None)
}

/// [`render`] for `columns`, reading what it can from `overview`: the same picture, sooner once
/// a column spans [`BLOCK_HOPS`] FFTs or more. The overview must have been built from these
/// `samples`; one built for another length, FFT length or dB scale is left unused.
pub fn render_columns(
    samples: &[f32],
    sample_rate: u32,
    columns: Columns,
    rows: usize,
    p: &SpectrogramParams,
    overview: Option<&Overview>,
) -> Vec<u8> {
    let Columns { start, end, cols } = columns;
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
    let analysis = Analysis::new(p);
    let overview = overview.filter(|o| o.serves(samples.len(), p));
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
    // Zoomed out: the grid's FFTs are centred on the odd multiples of half a hop, so a column
    // that starts and ends on a hop (see `Columns`) holds a whole number of them, evenly inside
    // it. Only those whose window reaches the file are computed, however wide the request.
    let hop = analysis.hop() as i64;
    let grid_end = analysis.grid_end(samples.len());
    let grid_index = |frame: f64| (frame / hop as f64 - 0.5).ceil() as i64;

    // Column-major while computing (each column is its own FFTs), transposed to rows at the end.
    let mut by_column = vec![0u8; cols * rows];
    by_column.par_chunks_mut(rows).enumerate().for_each_init(
        || (analysis.analyser(), vec![0u8; half + 1]),
        |(analyser, held), (c, column)| {
            analyser.peaks.fill(0.0);
            // The loudest level per bin of the overview's blocks inside this column, if any.
            held.fill(0);
            if per_col > n as f64 {
                let from = grid_index(start + c as f64 * per_col).max(-1);
                let to = grid_index(start + (c + 1) as f64 * per_col).min(grid_end);
                let read = overview.map_or(from..from, |o| o.fold(from, to, held));
                for g in (from..read.start).chain(read.end..to) {
                    analyser.add(samples, g * hop - hop / 2);
                }
            } else {
                let centre = start + (c as f64 + 0.5) * per_col;
                analyser.add(samples, (centre.floor() as i64).saturating_sub(hop));
            }
            for (level, (lo, hi)) in column.iter_mut().zip(&bands) {
                let peak = analyser.peaks.get(*lo..*hi).unwrap_or(&[]).iter().copied().fold(0.0f32, f32::max);
                let read = held.get(*lo..*hi).unwrap_or(&[]).iter().copied().max().unwrap_or(0);
                *level = analysis.level(peak).max(read);
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

    /// Hiss that swells and fades with a few short tones in it, so no two stretches look alike.
    fn busy(frames: usize, rate: u32) -> Vec<f32> {
        let mut x: Vec<f32> = noise(frames, 0.3).into_iter().enumerate().map(|(i, v)| v * (0.55 + 0.45 * (i as f32 / 9_000.0).sin())).collect();
        for (k, at) in (frames / 7..frames).step_by(frames / 5).enumerate() {
            let tone = sine(500.0 + 700.0 * k as f32, 0.9, 900, rate);
            x[at..at + 900].copy_from_slice(&tone);
        }
        x
    }

    #[test]
    fn an_overview_draws_the_same_picture_as_the_audio() {
        let rate = 8_000;
        // A block is 16 hops of 128 frames. The file ends 1,691 frames into a block.
        let block = BLOCK_HOPS * 128;
        let frames = 300 * block + 1_691;
        let x = busy(frames, rate);
        for (log_freq, rows) in [(true, 96), (false, 41)] {
            let p = SpectrogramParams { fft_size: 256, log_freq, ..SpectrogramParams::default() };
            let overview = Overview::build(&x, &p);
            assert_eq!(overview.bytes(), 300 * 129, "whole blocks only, one byte per bin");
            let same = |what: &str, c: Columns| {
                let live = render(&x, rate, c.start, c.end, c.cols, rows, &p);
                assert!(live.iter().any(|v| *v > 100), "{what}: there is something to see");
                let read = render_columns(&x, rate, c, rows, &p, Some(&overview));
                let differ = live.iter().zip(&read).filter(|(a, b)| a != b).count();
                assert_eq!(differ, 0, "{what}, log {log_freq}: {differ} of {} levels differ", live.len());
            };
            // The whole file fitted to four widths, as the cost example asks for it: columns that
            // are not a whole number of hops.
            for cols in [3, 37, 150, 640] {
                same(&format!("fitted to {cols} columns"), Columns { start: 0.0, end: frames as f64, cols });
            }
            // As the window asks: columns of whole hops on the file's grid, from under one block
            // to many, in views that start and end part-way through a block.
            for hops in [10.0, 16.0, 23.0, 32.0, 39.0, 1_000.0] {
                let per_col = hops * 128.0;
                for start in [0.0, 1_000.0, 2.5 * block as f64, 131.0 * block as f64 + 777.0] {
                    let c = Columns::covering(start, (start + 500.0 * per_col).min(frames as f64), per_col, &p);
                    same(&format!("{per_col} frames a column from {start}"), c);
                }
            }
            // A view that begins before the file and ends after it.
            same("wider than the file", Columns { start: -3.3 * block as f64, end: frames as f64 + 5.1 * block as f64, cols: 61 });
            same("far wider than the file", Columns { start: -1e12, end: 1e12, cols: 2_000 });
        }
    }

    #[test]
    fn far_views_are_read_from_the_overview_and_near_ones_are_not() {
        let rate = 8_000;
        let p = SpectrogramParams { fft_size: 256, ..SpectrogramParams::default() };
        let (frames, rows) = (200 * BLOCK_HOPS * 128, 32);
        let silence = vec![0.0; frames];
        // An overview of other audio of the same length: wrong on purpose, to see where it is read.
        let loud = Overview::build(&noise(frames, 0.5), &p);
        let far = Columns { start: 0.0, end: frames as f64, cols: 50 };
        assert!(render_columns(&silence, rate, far, rows, &p, Some(&loud)).iter().all(|v| *v > 100), "four blocks a column: read");
        let near = Columns { start: 0.0, end: frames as f64 / 10.0, cols: 40 };
        assert!(render_columns(&silence, rate, near, rows, &p, Some(&loud)).iter().all(|v| *v == 0), "half a block a column: analysed");
        // Built for another FFT length, dB scale or file length, it is not used at all.
        for other in [
            SpectrogramParams { fft_size: 512, ..p },
            SpectrogramParams { floor_db: -90.0, ..p },
            SpectrogramParams { ceil_db: -6.0, ..p },
            SpectrogramParams { window: WindowKind::Blackman, ..p },
        ] {
            assert!(render_columns(&silence, rate, far, rows, &other, Some(&loud)).iter().all(|v| *v == 0), "{other:?}");
        }
        assert!(render_columns(&silence[..frames - 1], rate, far, rows, &p, Some(&loud)).iter().all(|v| *v == 0));
        // The frequency axis and the picture's height are not part of it.
        let linear = SpectrogramParams { log_freq: false, min_hz: 300.0, ..p };
        assert!(render_columns(&silence, rate, far, 7, &linear, Some(&loud)).iter().all(|v| *v > 100));
        // Nothing to analyse, nothing kept.
        assert_eq!(Overview::build(&[], &p).bytes(), 0);
        assert_eq!(Overview::build(&silence[..1_000], &p).bytes(), 0);
    }

    #[test]
    fn the_level_table_is_the_decibel_scale() {
        for p in [
            SpectrogramParams::default(),
            SpectrogramParams { fft_size: 256, floor_db: -72.5, ceil_db: -3.0, ..SpectrogramParams::default() },
            SpectrogramParams { floor_db: -300.0, ..SpectrogramParams::default() },
        ] {
            let analysis = Analysis::new(&p);
            let gain = 2.0 / analysis.window.iter().sum::<f32>();
            let scale = |peak: f32| {
                let db = 10.0 * (peak * gain * gain).max(1e-20).log10();
                (((db - p.floor_db) / (p.ceil_db - p.floor_db)).clamp(0.0, 1.0) * 255.0).round() as u8
            };
            assert!(analysis.steps.windows(2).all(|w| w[0] <= w[1]), "steps rise");
            // Every step and its neighbours, then a walk through the floats.
            let around = analysis.steps.iter().flat_map(|s| [s.to_bits().saturating_sub(1), s.to_bits(), s.to_bits() + 1]);
            for bits in around.chain((0..f32::INFINITY.to_bits()).step_by(104_729)).chain([0, f32::INFINITY.to_bits()]) {
                let peak = f32::from_bits(bits);
                assert_eq!(analysis.level(peak), scale(peak), "{peak:e} with {p:?}");
            }
        }
        // A scale that is not a number draws black, as before.
        let wild = Analysis::new(&SpectrogramParams { floor_db: f32::NAN, ..SpectrogramParams::default() });
        assert_eq!((wild.level(0.0), wild.level(1e9), wild.level(f32::INFINITY)), (0, 0, 0));
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
        let wild = SpectrogramParams {
            fft_size: usize::MAX,
            min_hz: f32::NAN,
            max_hz: f32::INFINITY,
            floor_db: 0.0,
            ceil_db: 0.0,
            log_freq: true,
            window: WindowKind::Hann,
        };
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

    #[test]
    fn windows_have_their_known_coefficients() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        // Periodic form, n = 8: the start is a0 - a1 + a2 - a3, the middle (i = 4) the sum with
        // alternating signs flipped, and i = 2 sits at a quarter turn where only even terms stay.
        let hann = WindowKind::Hann.coefficients(8);
        assert!(close(hann[0], 0.0) && close(hann[4], 1.0) && close(hann[2], 0.5) && close(hann[1], 0.146_446_6));
        let hamming = WindowKind::Hamming.coefficients(8);
        assert!(close(hamming[0], 0.08) && close(hamming[4], 1.0) && close(hamming[2], 0.54));
        let blackman = WindowKind::Blackman.coefficients(8);
        assert!(close(blackman[0], 0.0) && close(blackman[4], 1.0) && close(blackman[2], 0.34), "{blackman:?}");
        let bh = WindowKind::BlackmanHarris.coefficients(8);
        assert!(close(bh[0], 0.000_06) && close(bh[4], 1.0) && close(bh[2], 0.217_47), "{bh:?}");
        assert!(WindowKind::Rectangular.coefficients(8).iter().all(|w| *w == 1.0));
        // Symmetric about the middle (w[i] == w[n - i]) and empty for n = 0.
        for kind in WindowKind::ALL {
            let w = kind.coefficients(64);
            assert!((1..64).all(|i| close(w[i], w[64 - i])), "{kind:?}");
            assert!(kind.coefficients(0).is_empty());
        }
        // Hann is exactly what the display drew before windows were a choice.
        let old: Vec<f32> = (0..16).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / 16.0).cos()).collect();
        assert_eq!(WindowKind::Hann.coefficients(16), old);
    }

    #[test]
    fn window_names_round_trip() {
        for kind in WindowKind::ALL {
            assert_eq!(WindowKind::parse(kind.name()), Some(kind));
            assert_eq!(WindowKind::parse(&kind.label().to_uppercase()), Some(kind));
        }
        assert_eq!(WindowKind::parse("Blackman Harris"), Some(WindowKind::BlackmanHarris));
        assert_eq!(WindowKind::parse("rect"), Some(WindowKind::Rectangular));
        assert_eq!(WindowKind::parse("kaiser"), None);
        assert_eq!(WindowKind::parse(""), None);
    }

    #[test]
    fn decibels_map_onto_levels() {
        assert_eq!(db_to_level(-100.0, -100.0, 0.0), 0);
        assert_eq!(db_to_level(0.0, -100.0, 0.0), 255);
        assert_eq!(db_to_level(-50.0, -100.0, 0.0), 128);
        assert_eq!(db_to_level(-200.0, -100.0, 0.0), 0, "below the floor");
        assert_eq!(db_to_level(12.0, -100.0, 0.0), 255, "above the ceiling");
        assert_eq!(db_to_level(f32::NAN, -100.0, 0.0), 0);
        // A narrower range makes the same level louder: -30 dB of a 60 dB range is half scale.
        assert_eq!(db_to_level(-30.0, -60.0, 0.0), 128);
        assert_eq!(db_to_level(-30.0, -120.0, 0.0), 191);
        assert_eq!(db_to_level(-9.0, -9.0, -9.0), 0, "an empty range does not divide by zero");
    }

    #[test]
    fn the_decibel_range_is_kept_in_bounds() {
        let p = SpectrogramParams::default();
        assert_eq!(p.range_db(), 100.0);
        let wide = p.with_range_db(180.0);
        assert_eq!((wide.floor_db, wide.ceil_db, wide.range_db()), (-180.0, 0.0, 180.0));
        assert_eq!(p.with_range_db(60.0).floor_db, -60.0);
        assert_eq!(p.with_range_db(5.0).range_db(), MIN_RANGE_DB);
        assert_eq!(p.with_range_db(1e9).range_db(), MAX_RANGE_DB);
        assert_eq!(p.with_range_db(f32::NAN), p);
    }

    #[test]
    fn resolution_steps_stop_at_the_ends() {
        assert_eq!(step_fft_size(2048, 1), 4096);
        assert_eq!(step_fft_size(2048, -1), 1024);
        assert_eq!(step_fft_size(16_384, 1), 16_384);
        assert_eq!(step_fft_size(64, -1), 64);
        assert_eq!(step_fft_size(2048, 100), 16_384);
        assert_eq!(step_fft_size(2048, -100), 64);
        assert_eq!(step_fft_size(3000, 0), 4096, "off the list: the next size up");
        assert_eq!(step_fft_size(usize::MAX, -1), 8192);
    }

    #[test]
    fn the_window_changes_the_picture_and_the_range_the_levels() {
        let rate = 8_000;
        // Between two bins (31.25 Hz apart): a tone exactly on a bin does not leak in any window.
        let x = sine(1_015.0, 0.5, 8_000, rate);
        let leak = |window| {
            let p = SpectrogramParams { fft_size: 256, window, min_hz: 40.0, log_freq: false, ..SpectrogramParams::default() };
            let image = render(&x, rate, 4_000.0, 4_001.0, 1, 64, &p);
            image.iter().filter(|l| **l > 40).count()
        };
        // The rectangular window spreads a tone over more of the spectrum than a Blackman-Harris one.
        assert!(
            leak(WindowKind::Rectangular) > leak(WindowKind::BlackmanHarris),
            "{} {}",
            leak(WindowKind::Rectangular),
            leak(WindowKind::BlackmanHarris)
        );
        // A quiet tone (about -50 dBFS) is below a 20 dB range, dim in 60 and bright in 180.
        let quiet = sine(1_000.0, 0.003, 8_000, rate);
        let loudest = |range| {
            let p = SpectrogramParams { fft_size: 256, ..SpectrogramParams::default() }.with_range_db(range);
            render(&quiet, rate, 4_000.0, 4_001.0, 1, 64, &p).into_iter().max().unwrap_or(0)
        };
        assert_eq!(loudest(20.0), 0);
        assert!(loudest(60.0) > 0 && loudest(180.0) > loudest(60.0), "{} {}", loudest(60.0), loudest(180.0));
    }
}
