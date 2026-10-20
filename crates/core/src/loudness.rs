//! Offline loudness measurement (f26, roadmap B125): integrated loudness (ITU-R BS.1770 with its
//! two gates), loudness range (EBU Tech 3342), true peak (4x oversampled) and sample peak, over
//! a range of a document, read in chunks through [`DocState::read_frames`] so a long file is
//! never held in memory.
//!
//! The K-weighting, the 400 ms blocks every 100 ms, the gating and the true-peak interpolator are
//! soundcraft-dsp's [`LoudnessMeter`]. The loudness range is added here from the meter's
//! short-term (3 s) readings, taken every 100 ms once 3 s have been measured.
//!
//! **What the standard says a tone reads.** Loudness is -0.691 + 10 log10 of the K-weighted mean
//! square summed over the channels. K-weighting is built to have about +0.69 dB of gain at 1 kHz,
//! which cancels the -0.691 offset, so a sine of peak amplitude A in one channel reads
//! 10 log10(A^2 / 2) = (peak dBFS) - 3.01 LUFS, and the same sine in both channels of a stereo
//! file (weights 1 + 1) reads 3.01 more: exactly its peak level in dBFS. A 1 kHz stereo sine at
//! -20 dBFS peak therefore reads -20 LUFS.

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_dsp::meter::LoudnessMeter;

use crate::doc::DocState;

/// The result of [`measure`]. Levels with nothing to measure (silence, or a range too short for
/// a 400 ms block) are `f64::NEG_INFINITY`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Frames measured.
    pub frames: u64,
    /// Gated integrated loudness, LUFS.
    pub integrated_lufs: f64,
    /// Loudness range, LU (0 when there are fewer than 3 s to measure).
    pub range_lu: f64,
    /// Highest 4x-oversampled peak, dBTP.
    pub true_peak_dbtp: f64,
    /// Highest sample magnitude over every channel, dBFS.
    pub sample_peak_dbfs: f64,
    /// Highest momentary (400 ms) loudness, LUFS.
    pub max_momentary_lufs: f64,
}

impl Loudness {
    /// One line of `key=value` pairs, for the control channel and logs.
    pub fn line(&self) -> String {
        format!(
            "frames={} integrated={} range={:.2} true_peak={} sample_peak={} max_momentary={}",
            self.frames,
            fmt_db(self.integrated_lufs),
            self.range_lu,
            fmt_db(self.true_peak_dbtp),
            fmt_db(self.sample_peak_dbfs),
            fmt_db(self.max_momentary_lufs)
        )
    }
}

fn fmt_db(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "-inf".to_string() }
}

/// What the meter reports for "nothing" (soundcraft-dsp's `MIN_DB`) and below, as minus infinity.
fn level(v: f32) -> f64 {
    if v.is_finite() && v > -143.0 { f64::from(v) } else { f64::NEG_INFINITY }
}

/// Tech 3342: of the short-term readings, drop those under -70 LUFS, then those more than 20 LU
/// under the mean power of what is left; the range is the 95th minus the 10th percentile (by
/// linear interpolation between neighbours).
pub fn range_of(short_term: &[f32]) -> f64 {
    let power = |l: f32| 10f64.powf((f64::from(l) + 0.691) / 10.0);
    let mean_lufs = |v: &[f32]| -0.691 + 10.0 * (v.iter().map(|l| power(*l)).sum::<f64>() / v.len() as f64).log10();
    let above: Vec<f32> = short_term.iter().copied().filter(|l| *l > -70.0).collect();
    if above.is_empty() {
        return 0.0;
    }
    let gate = mean_lufs(&above) - 20.0;
    let mut kept: Vec<f32> = above.into_iter().filter(|l| f64::from(*l) > gate).collect();
    if kept.is_empty() {
        return 0.0;
    }
    kept.sort_by(f32::total_cmp);
    let at = |p: f64| {
        let x = p * (kept.len() - 1) as f64;
        let (i, frac) = (x.floor() as usize, x.fract());
        let a = f64::from(kept[i]);
        let b = f64::from(kept[(i + 1).min(kept.len() - 1)]);
        a + (b - a) * frac
    };
    at(0.95) - at(0.10)
}

/// Measure `range` (frames; clamped to the document) of `doc`. `cancel` stops it early with
/// `ErrorKind::Interrupted`; `progress` is called with the fraction done after each second or so.
pub fn measure(doc: &DocState, range: Range<u64>, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> io::Result<Loudness> {
    let len = doc.len_frames();
    let (start, end) = (range.start.min(len), range.end.min(len).max(range.start.min(len)));
    let channels = doc.channel_count();
    let rate = doc.sample_rate().max(1);
    let mut meter = LoudnessMeter::new(rate as f32, channels.max(1));
    // One call per 100 ms sub-block, so a short-term reading can be taken after each.
    let block = ((f64::from(rate) * 0.1).round() as usize).max(1);
    let mut planar: Vec<Vec<f32>> = vec![vec![0.0; block]; channels];
    let (mut short_term, mut sample_peak) = (Vec::<f32>::new(), 0.0f32);
    let (mut at, mut blocks) = (start, 0usize);
    let mut last_report = 0u64;
    while at < end {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let want = usize::try_from(end - at).unwrap_or(usize::MAX).min(block);
        for ch in &mut planar {
            ch.resize(want, 0.0);
        }
        let mut bufs: Vec<&mut [f32]> = planar.iter_mut().map(Vec::as_mut_slice).collect();
        let got = doc.read_frames(at, &mut bufs)?;
        if got == 0 {
            break;
        }
        for ch in &planar {
            sample_peak = ch.iter().take(got).filter(|v| v.is_finite()).fold(sample_peak, |m, v| m.max(v.abs()));
        }
        meter.process(&planar, got);
        at += got as u64;
        if got == block {
            blocks += 1;
            if blocks >= 30 {
                short_term.push(meter.short_term_lufs());
            }
        }
        if at - last_report >= u64::from(rate) * 5 {
            last_report = at;
            progress((at - start) as f64 / (end - start).max(1) as f64);
        }
    }
    progress(1.0);
    Ok(Loudness {
        frames: at - start,
        integrated_lufs: level(meter.integrated_lufs()),
        range_lu: range_of(&short_term),
        true_peak_dbtp: level(meter.true_peak_db()),
        sample_peak_dbfs: if sample_peak > 0.0 { 20.0 * f64::from(sample_peak).log10() } else { f64::NEG_INFINITY },
        max_momentary_lufs: level(meter.max_momentary_lufs()),
    })
}
