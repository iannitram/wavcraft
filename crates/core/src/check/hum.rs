//! Mains hum: steady narrow peaks at 50 or 60 Hz and their harmonics, found in long STFT windows.
//! Each region's edges are then refined with a short sliding window that tracks the fundamental.

use rustfft::num_complex::Complex32;

use super::{Config, Finding, Kind, Severity, Sink, Ticker, read_padded};
use crate::spectrogram::Samples;
use crate::stft::Stft;

/// FFT length of a window: about 0.34 s at 48 kHz, so a 50 Hz and a 60 Hz peak are 3.4 bins apart.
const FFT: usize = 16_384;
/// Highest harmonic of the fundamental that is looked at.
const HARMONICS: usize = 5;
/// Bins either side of a peak that are skipped, and the furthest bin that counts, when taking the
/// median of its neighbours.
const SKIP: usize = 3;
const REACH: usize = 12;
/// Windows without hum that a region bridges: speech over the hum hides it for a moment.
const MAX_MISSES: u32 = 4;
/// The sliding window that refines a region's edges: about 85 ms long, moved 10 ms at a time. At
/// this length a bin is 11.7 Hz, so the neighbours of the fundamental are only 1 to 3 bins out:
/// the speech partials 15 Hz or more away from it stay out of its median.
const FINE: usize = 4_096;
const FINE_HOP: usize = 480;
const FINE_NEIGHBOURS: (usize, usize) = (1, 3);

/// `None` when the scan was cancelled.
pub(super) fn hum_pass(src: &dyn Samples, channel: usize, cfg: &Config, sink: &mut Sink, tick: &mut Ticker) -> Option<()> {
    let len = src.len();
    let rate = f64::from(cfg.sample_rate);
    if len < FFT || rate / 2.0 <= cfg.hum_hz.iter().copied().fold(0.0, f64::max) * HARMONICS as f64 {
        return tick.step(len as u64);
    }
    let Some(mut coarse) = Analyser::new(FFT) else { return tick.step(len as u64) };
    let margin = 10f32.powf(cfg.hum_margin_db as f32 / 20.0);
    let min_frames = u64::from(cfg.sample_rate) * u64::from(cfg.hum_min_ms) / 1_000;
    // The open region: first frame, end frame of its last window with hum, the fundamental, and
    // the windows since without.
    let mut open: Option<(u64, u64, f64, u32)> = None;
    // Coarse regions (first frame, end frame, fundamental), refined once the scan is over.
    let mut regions: Vec<(u64, u64, f64)> = Vec::new();
    let close = |region: Option<(u64, u64, f64, u32)>, regions: &mut Vec<(u64, u64, f64)>| {
        if let Some((start, end, hz, _)) = region.filter(|r| r.1 - r.0 >= min_frames) {
            regions.push((start, end, hz));
        }
    };

    let mut start = 0usize;
    while start + FFT <= len {
        coarse.analyse(src, start as i64);
        let hit = hum_at(&coarse.mag, rate, FFT, cfg, margin, (SKIP, REACH));
        let (s, e) = (start as u64, (start + FFT) as u64);
        open = match (open, hit) {
            (Some((a, _, hz, _)), Some(h)) if h == hz => Some((a, e, hz, 0)),
            (Some((a, b, hz, misses)), None) if misses < MAX_MISSES => Some((a, b, hz, misses + 1)),
            (previous, h) => {
                close(previous, &mut regions);
                h.map(|hz| (s, e, hz, 0))
            }
        };
        start += coarse.stft.hop();
        tick.step(coarse.stft.hop() as u64)?;
    }
    close(open, &mut regions);

    let mut fine = Analyser::new(FINE);
    for (start, end, hz) in regions {
        let (start, end) = match fine.as_mut() {
            Some(fine) => refine(fine, src, hz, (start, end), rate, margin),
            None => (start, end),
        };
        sink.push(Finding { kind: Kind::MainsHum, severity: Severity::Warning, channel: Some(channel), start, end, value: hz });
    }
    Some(())
}

/// A region's edges moved to the sliding window that marks them. The first window of [`FINE`]
/// frames that starts within [`FFT`] frames of the coarse start and shows the line at `hz` gives
/// the start; the last such window within [`FFT`] frames of the coarse end gives the end. Each edge
/// is the centre of its window. A coarse edge is kept where no window shows the line, or where the
/// refined edges would leave no region.
fn refine(fine: &mut Analyser, src: &dyn Samples, hz: f64, (start, end): (u64, u64), rate: f64, margin: f32) -> (u64, u64) {
    let reach = FFT as i64;
    let len = src.len() as u64;
    let windows = |coarse: u64| (coarse as i64 - reach..=coarse as i64 + reach).step_by(FINE_HOP);
    let half = (FINE / 2) as i64;
    let mut shows = |p: i64| {
        fine.analyse(src, p);
        let centre = (hz / (rate / FINE as f64)).round() as usize;
        line_stands_out(&fine.mag, centre, margin, FINE_NEIGHBOURS)
    };
    let first = windows(start).find(|&p| shows(p)).map(|p| (p + half).max(0) as u64);
    let last = windows(end).filter(|&p| shows(p)).last().map(|p| (p + half).max(0) as u64);
    match (first, last) {
        (Some(a), Some(b)) if a < b => (a.min(len), b.min(len)),
        _ => (start, end),
    }
}

/// A mains-hum analysis at one FFT size: the magnitude spectrum of a window, and the spectrum's
/// bins.
struct Analyser {
    stft: Stft,
    buf: Vec<f32>,
    scratch: Vec<Complex32>,
    spec: Vec<Complex32>,
    mag: Vec<f32>,
}

impl Analyser {
    /// `None` when `n` is not a valid FFT size.
    fn new(n: usize) -> Option<Analyser> {
        let stft = Stft::new(n, 50).ok()?;
        Some(Analyser {
            buf: vec![0f32; n],
            scratch: Vec::new(),
            spec: vec![Complex32::new(0.0, 0.0); stft.bins()],
            mag: vec![0f32; stft.bins()],
            stft,
        })
    }

    /// The magnitude spectrum of the window of this size at `start` (silence outside the source)
    /// into `mag`.
    fn analyse(&mut self, src: &dyn Samples, start: i64) {
        read_padded(src, start, &mut self.buf);
        self.stft.analyse(&self.buf, 0, 0, &mut self.scratch, &mut self.spec);
        for (m, c) in self.mag.iter_mut().zip(&self.spec) {
            *m = c.norm();
        }
    }
}

/// Whether the bin `centre` stands `margin` over the median of the bins `skip + 1` to `reach` either
/// side of it (and the peak of the bin and its two neighbours is the one compared).
fn line_stands_out(mag: &[f32], centre: usize, margin: f32, (skip, reach): (usize, usize)) -> bool {
    if centre + reach >= mag.len() {
        return false;
    }
    let peak = mag[centre.saturating_sub(1)..=centre + 1].iter().copied().fold(0.0, f32::max);
    let mut around: Vec<f32> = (skip + 1..=reach).flat_map(|d| [mag.get(centre.wrapping_sub(d)), mag.get(centre + d)]).flatten().copied().collect();
    around.sort_by(f32::total_cmp);
    peak > around[around.len() / 2] * margin
}

/// The candidate fundamental with the most harmonics (of 1x to 5x) standing [`Config::hum_margin_db`]
/// over the median of the bins around them, if at least [`Config::hum_harmonics`] do. `mag` is a
/// spectrum of FFT length `n`.
fn hum_at(mag: &[f32], rate: f64, n: usize, cfg: &Config, margin: f32, neighbours: (usize, usize)) -> Option<f64> {
    let bin_hz = rate / n as f64;
    let mut best: Option<(usize, f64)> = None;
    for &f0 in &cfg.hum_hz {
        let count = (1..=HARMONICS).filter(|&k| line_stands_out(mag, (f0 * k as f64 / bin_hz).round() as usize, margin, neighbours)).count();
        if count >= cfg.hum_harmonics as usize && best.is_none_or(|b| count > b.0) {
            best = Some((count, f0));
        }
    }
    best.map(|b| b.1)
}
