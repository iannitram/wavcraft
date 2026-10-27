//! Frequency analysis (roadmap A309-A312): the spectrum of one moment, or the average spectrum of a
//! range, in dB with 0 dB a full-scale sine.
//!
//! - [`spectrum_at`] takes one windowed FFT centred on a frame (audio past either end of the file
//!   counts as silence).
//! - [`scan`] averages the power spectra of windows laid 50 % apart over a range. It reads the
//!   range in chunks of [`BLOCK_FRAMES`] and keeps only the overlap between windows, so memory
//!   does not grow with the range; it checks the cancel flag between chunks and between windows.
//! - Levels: a sine of amplitude 1 that falls on a bin's centre reads 0 dB whatever the window
//!   (the window's gain is divided out). A tone between two bins reads up to the window's
//!   scalloping loss lower in the bin itself (1.4 dB for Hann); [`Spectrum::peak`] interpolates
//!   between bins and so lands within a few tenths of a dB of the tone's real level.
//! - Per channel, plus a mean of the channels' power.

use std::fmt;
use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::doc::DocState;
use crate::doc::edit::ChannelMask;
use crate::store::BLOCK_FRAMES;

/// The smallest and largest FFT size.
pub const MIN_FFT: usize = 256;
pub const MAX_FFT: usize = 65_536;
/// The level shown for silence (and the floor of every bin).
pub const FLOOR_DB: f32 = -200.0;

/// The window applied before the FFT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FftWindow {
    Hann,
    Blackman,
    Rectangular,
}

impl FftWindow {
    pub const ALL: [FftWindow; 3] = [FftWindow::Hann, FftWindow::Blackman, FftWindow::Rectangular];

    pub fn name(self) -> &'static str {
        match self {
            FftWindow::Hann => "Hann",
            FftWindow::Blackman => "Blackman",
            FftWindow::Rectangular => "Rectangular",
        }
    }

    /// The window by its (case-insensitive) name.
    pub fn parse(s: &str) -> Option<FftWindow> {
        FftWindow::ALL.into_iter().find(|w| w.name().eq_ignore_ascii_case(s.trim()))
    }

    fn coefficients(self, n: usize) -> Vec<f32> {
        let step = std::f64::consts::TAU / n as f64;
        (0..n)
            .map(|i| {
                let x = i as f64 * step;
                (match self {
                    FftWindow::Hann => 0.5 - 0.5 * x.cos(),
                    FftWindow::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
                    FftWindow::Rectangular => 1.0,
                }) as f32
            })
            .collect()
    }
}

#[derive(Debug)]
pub enum FreqError {
    /// The FFT size is not a power of two from [`MIN_FFT`] to [`MAX_FFT`].
    BadFftSize(usize),
    /// No channel is selected, or the range is empty or past the end of the document.
    BadRequest(&'static str),
    Io(io::Error),
}

impl fmt::Display for FreqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FreqError::BadFftSize(n) => write!(f, "FFT size {n} is not a power of two from {MIN_FFT} to {MAX_FFT}"),
            FreqError::BadRequest(why) => f.write_str(why),
            FreqError::Io(e) => write!(f, "could not read the audio: {e}"),
        }
    }
}

impl std::error::Error for FreqError {}

impl From<io::Error> for FreqError {
    fn from(e: io::Error) -> Self {
        FreqError::Io(e)
    }
}

/// A peak found in a spectrum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub hz: f32,
    pub db: f32,
}

/// Level per bin, `fft_size / 2 + 1` bins from 0 Hz to Nyquist.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    pub sample_rate: u32,
    pub fft_size: usize,
    /// Which document channel each row of `channels` is.
    pub channel_ids: Vec<usize>,
    /// dB per bin, one row per analysed channel.
    pub channels: Vec<Vec<f32>>,
    /// dB of the channels' mean power.
    pub mean: Vec<f32>,
}

impl Spectrum {
    pub fn bins(&self) -> usize {
        self.fft_size / 2 + 1
    }

    pub fn bin_hz(&self) -> f32 {
        self.sample_rate as f32 / self.fft_size as f32
    }

    /// The frequency of bin `bin`.
    pub fn hz_of(&self, bin: usize) -> f32 {
        bin as f32 * self.bin_hz()
    }

    /// The level at `hz` (linear between bins).
    pub fn level_at(row: &[f32], bin_hz: f32, hz: f32) -> f32 {
        if row.is_empty() {
            return FLOOR_DB;
        }
        let x = (hz / bin_hz).clamp(0.0, (row.len() - 1) as f32);
        let i = (x.floor() as usize).min(row.len() - 1);
        let j = (i + 1).min(row.len() - 1);
        let t = x - i as f32;
        row[i] * (1.0 - t) + row[j] * t
    }

    /// The loudest peak of row `row` (an index into `channels`) between `lo` and `hi` Hz, its
    /// frequency and level refined by a parabola through the bin and its neighbours.
    pub fn peak(&self, row: usize, lo: f32, hi: f32) -> Option<Peak> {
        peak_in(self.channels.get(row)?, self.bin_hz(), lo, hi)
    }

    /// [`Spectrum::peak`] of the channel mean.
    pub fn mean_peak(&self, lo: f32, hi: f32) -> Option<Peak> {
        peak_in(&self.mean, self.bin_hz(), lo, hi)
    }

    /// Tab-separated `Hz\tdB` lines, one block per channel (and a mean block when there is more
    /// than one channel), for pasting into a spreadsheet.
    pub fn graph_data(&self) -> String {
        let mut out = String::new();
        let mut block = |title: String, row: &[f32]| {
            out.push_str(&title);
            out.push_str("\nHz\tdB\n");
            for (b, v) in row.iter().enumerate() {
                out.push_str(&format!("{:.1}\t{:.2}\n", b as f32 * self.sample_rate as f32 / self.fft_size as f32, v));
            }
        };
        for (row, id) in self.channels.iter().zip(&self.channel_ids) {
            block(format!("Channel {}", id + 1), row);
        }
        if self.channels.len() > 1 {
            block("Mean".to_string(), &self.mean);
        }
        out
    }
}

fn peak_in(row: &[f32], bin_hz: f32, lo: f32, hi: f32) -> Option<Peak> {
    if row.is_empty() {
        return None;
    }
    let first = ((lo / bin_hz).ceil().max(0.0)) as usize;
    let last = ((hi / bin_hz).floor() as usize).min(row.len() - 1);
    if first > last {
        return None;
    }
    let (mut best, mut best_db) = (first, f32::MIN);
    for (i, &v) in row.iter().enumerate().take(last + 1).skip(first) {
        if v > best_db {
            best = i;
            best_db = v;
        }
    }
    // A parabola through the bin and its neighbours (in dB), when it has both.
    if best > 0 && best + 1 < row.len() {
        let (a, b, c) = (row[best - 1], row[best], row[best + 1]);
        let denom = a - 2.0 * b + c;
        if denom < -1e-6 {
            let d = (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
            return Some(Peak { hz: (best as f32 + d) * bin_hz, db: b - 0.25 * (a - c) * d });
        }
    }
    Some(Peak { hz: best as f32 * bin_hz, db: best_db })
}

fn check_size(n: usize) -> Result<(), FreqError> {
    if (MIN_FFT..=MAX_FFT).contains(&n) && n.is_power_of_two() { Ok(()) } else { Err(FreqError::BadFftSize(n)) }
}

/// Windows and transforms, accumulating power per bin.
struct Analyser {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    /// Divides a bin's magnitude into a sine's amplitude: half the window's sum.
    norm: f32,
    scratch: Vec<Complex32>,
}

impl Analyser {
    fn new(n: usize, w: FftWindow) -> Analyser {
        let window = w.coefficients(n);
        let norm = window.iter().sum::<f32>() * 0.5;
        Analyser { fft: FftPlanner::new().plan_fft_forward(n), window, norm, scratch: vec![Complex32::new(0.0, 0.0); n] }
    }

    /// Add the power spectrum of `samples` (exactly the FFT size) into `power`.
    fn add(&mut self, samples: &[f32], power: &mut [f64]) {
        for ((s, w), c) in samples.iter().zip(&self.window).zip(self.scratch.iter_mut()) {
            *c = Complex32::new(s * w, 0.0);
        }
        self.fft.process(&mut self.scratch);
        let k = 1.0 / (self.norm as f64 * self.norm as f64);
        for (p, c) in power.iter_mut().zip(&self.scratch) {
            *p += (c.re as f64 * c.re as f64 + c.im as f64 * c.im as f64) * k;
        }
    }
}

fn to_db(power: f64) -> f32 {
    ((10.0 * power.max(1e-30).log10()) as f32).max(FLOOR_DB)
}

fn finish(doc: &DocState, ids: Vec<usize>, n: usize, power: &[Vec<f64>], count: usize) -> Spectrum {
    let bins = n / 2 + 1;
    let c = count.max(1) as f64;
    let channels: Vec<Vec<f32>> = power.iter().map(|p| p.iter().map(|&v| to_db(v / c)).collect()).collect();
    let k = power.len().max(1) as f64;
    let mean = (0..bins).map(|b| to_db(power.iter().map(|p| p[b]).sum::<f64>() / k / c)).collect();
    Spectrum { sample_rate: doc.sample_rate(), fft_size: n, channel_ids: ids, channels, mean }
}

/// One windowed FFT centred on `frame`, per channel in `channels`.
pub fn spectrum_at(doc: &DocState, frame: u64, channels: ChannelMask, fft_size: usize, window: FftWindow) -> Result<Spectrum, FreqError> {
    check_size(fft_size)?;
    let ids = channels.active(doc.channel_count());
    if ids.is_empty() {
        return Err(FreqError::BadRequest("no channel is selected"));
    }
    let n = fft_size;
    let start = frame as i128 - (n / 2) as i128;
    let len = doc.len_frames() as i128;
    // The part of the window that lies inside the document.
    let from = start.max(0).min(len);
    let to = (start + n as i128).clamp(from, len);
    let mut an = Analyser::new(n, window);
    let mut power = vec![vec![0.0f64; n / 2 + 1]; ids.len()];
    let mut buf = vec![0.0f32; n];
    for (row, &c) in power.iter_mut().zip(&ids) {
        buf.fill(0.0);
        let at = (from - start) as usize;
        let want = (to - from) as usize;
        if want > 0 {
            let got = doc.read(c, from as u64, &mut buf[at..at + want])?;
            if got != want {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
        }
        an.add(&buf, row);
    }
    Ok(finish(doc, ids, n, &power, 1))
}

/// The average spectrum of `range`. `Ok(None)` when `cancel` was set.
pub fn scan(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    fft_size: usize,
    window: FftWindow,
    cancel: &AtomicBool,
) -> Result<Option<Spectrum>, FreqError> {
    scan_with_progress(doc, range, channels, fft_size, window, cancel, None)
}

/// [`scan`], also publishing how far it has got in thousandths.
pub fn scan_with_progress(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    fft_size: usize,
    window: FftWindow,
    cancel: &AtomicBool,
    progress: Option<&AtomicU32>,
) -> Result<Option<Spectrum>, FreqError> {
    check_size(fft_size)?;
    let ids = channels.active(doc.channel_count());
    if ids.is_empty() {
        return Err(FreqError::BadRequest("no channel is selected"));
    }
    if range.start >= range.end || range.end > doc.len_frames() {
        return Err(FreqError::BadRequest("the range is empty or past the end of the file"));
    }
    let n = fft_size;
    let hop = n / 2;
    let mut an = Analyser::new(n, window);
    let mut power = vec![vec![0.0f64; n / 2 + 1]; ids.len()];
    // Per channel: the samples read and not yet dropped (at most one chunk plus one window).
    let mut pending: Vec<Vec<f32>> = vec![Vec::new(); ids.len()];
    let mut chunk = vec![0.0f32; BLOCK_FRAMES];
    let mut windows = 0usize;
    let total = range.end - range.start;
    let mut at = range.start;
    while at < range.end {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let m = (range.end - at).min(BLOCK_FRAMES as u64) as usize;
        for (p, &c) in pending.iter_mut().zip(&ids) {
            let got = doc.read(c, at, &mut chunk[..m])?;
            if got != m {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
            p.extend_from_slice(&chunk[..m]);
        }
        at += m as u64;
        while pending[0].len() >= n {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            for (p, row) in pending.iter_mut().zip(power.iter_mut()) {
                an.add(&p[..n], row);
                p.drain(..hop);
            }
            windows += 1;
        }
        if let Some(pr) = progress {
            pr.store((((at - range.start) as u128 * 1000) / total as u128) as u32, Ordering::Relaxed);
        }
    }
    if windows == 0 {
        // A range shorter than the FFT: one window, padded with silence.
        for (p, row) in pending.iter_mut().zip(power.iter_mut()) {
            p.resize(n, 0.0);
            an.add(&p[..n], row);
        }
        windows = 1;
    }
    if let Some(pr) = progress {
        pr.store(1000, Ordering::Relaxed);
    }
    Ok(Some(finish(doc, ids, n, &power, windows)))
}
