//! Mains hum: steady narrow peaks at 50 or 60 Hz and their harmonics, found in long STFT windows.

use rustfft::num_complex::Complex32;

use super::{Config, Finding, Kind, Severity, Sink, read_padded};
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

pub(super) fn hum_pass(src: &dyn Samples, channel: usize, cfg: &Config, sink: &mut Sink) {
    let len = src.len();
    let rate = f64::from(cfg.sample_rate);
    if len < FFT || rate / 2.0 <= cfg.hum_hz.iter().copied().fold(0.0, f64::max) * HARMONICS as f64 {
        return;
    }
    let Ok(stft) = Stft::new(FFT, 50) else { return };
    let (mut buf, mut scratch) = (vec![0f32; FFT], Vec::new());
    let mut spec = vec![Complex32::new(0.0, 0.0); stft.bins()];
    let mut mag = vec![0f32; stft.bins()];
    let margin = 10f32.powf(cfg.hum_margin_db as f32 / 20.0);
    let min_frames = u64::from(cfg.sample_rate) * u64::from(cfg.hum_min_ms) / 1_000;
    // The open region: first frame, end frame of its last window with hum, the fundamental, and
    // the windows since without.
    let mut open: Option<(u64, u64, f64, u32)> = None;
    let close = |region: Option<(u64, u64, f64, u32)>, sink: &mut Sink| {
        if let Some((start, end, hz, _)) = region.filter(|r| r.1 - r.0 >= min_frames) {
            sink.push(Finding { kind: Kind::MainsHum, severity: Severity::Warning, channel: Some(channel), start, end, value: hz });
        }
    };

    let mut start = 0usize;
    while start + FFT <= len {
        read_padded(src, start as i64, &mut buf);
        stft.analyse(&buf, 0, 0, &mut scratch, &mut spec);
        for (m, c) in mag.iter_mut().zip(&spec) {
            *m = c.norm();
        }
        let hit = hum_at(&mag, rate, cfg, margin);
        let (s, e) = (start as u64, (start + FFT) as u64);
        open = match (open, hit) {
            (Some((a, _, hz, _)), Some(h)) if h == hz => Some((a, e, hz, 0)),
            (Some((a, b, hz, misses)), None) if misses < MAX_MISSES => Some((a, b, hz, misses + 1)),
            (previous, h) => {
                close(previous, sink);
                h.map(|hz| (s, e, hz, 0))
            }
        };
        start += stft.hop();
    }
    close(open, sink);
}

/// The candidate fundamental with the most harmonics (of 1x to 5x) standing [`Config::hum_margin_db`]
/// over the median of the bins around them, if at least [`Config::hum_harmonics`] do.
fn hum_at(mag: &[f32], rate: f64, cfg: &Config, margin: f32) -> Option<f64> {
    let bin_hz = rate / FFT as f64;
    let mut best: Option<(usize, f64)> = None;
    for &f0 in &cfg.hum_hz {
        let count = (1..=HARMONICS)
            .filter(|&k| {
                let centre = (f0 * k as f64 / bin_hz).round() as usize;
                if centre + REACH >= mag.len() {
                    return false;
                }
                let peak = mag[centre.saturating_sub(1)..=centre + 1].iter().copied().fold(0.0, f32::max);
                let mut around: Vec<f32> =
                    (SKIP + 1..=REACH).flat_map(|d| [mag.get(centre.wrapping_sub(d)), mag.get(centre + d)]).flatten().copied().collect();
                around.sort_by(f32::total_cmp);
                peak > around[around.len() / 2] * margin
            })
            .count();
        if count >= cfg.hum_harmonics as usize && best.is_none_or(|b| count > b.0) {
            best = Some((count, f0));
        }
    }
    best.map(|b| b.1)
}
