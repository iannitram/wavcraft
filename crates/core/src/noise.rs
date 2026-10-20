//! Noise prints (A337 / B066): the average noise magnitude spectrum of a selection, per channel,
//! kept in memory for the session (saving prints is a later row). Noise Reduction
//! ([`crate::effects::NoiseReduction`]) reads the current print.
//!
//! A print stores a *density*: the mean FFT magnitude divided by the root of the window energy,
//! so it does not depend on the FFT size it was captured at. Noise Reduction scales it back to
//! whatever FFT size it runs with.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use rustfft::num_complex::Complex32;

use crate::doc::DocState;
use crate::stft::Stft;

/// The FFT length a print is captured at.
pub const PRINT_FFT: usize = 4096;

/// The average noise spectrum of a selection.
#[derive(Debug, Clone, PartialEq)]
pub struct NoisePrint {
    pub sample_rate: u32,
    /// Per channel, [`PRINT_FFT`]` / 2 + 1` bins: mean magnitude over the selection's frames
    /// divided by the root of the window energy.
    pub density: Vec<Vec<f32>>,
    /// How many FFT frames were averaged.
    pub frames: usize,
}

impl NoisePrint {
    /// The print for document channel `channel`: a print with fewer channels serves the extra
    /// ones with its last.
    pub fn channel(&self, channel: usize) -> &[f32] {
        &self.density[channel.min(self.density.len().saturating_sub(1))]
    }

    /// The noise magnitude per bin of an `n`-point Hann FFT (`n / 2 + 1` values), by linear
    /// interpolation across frequency.
    pub fn magnitudes(&self, channel: usize, n: usize) -> Vec<f32> {
        let d = self.channel(channel);
        let scale = (0.375 * n as f32).sqrt();
        (0..=n / 2)
            .map(|k| {
                let q = k as f32 * PRINT_FFT as f32 / n as f32;
                let i = (q.floor() as usize).min(d.len() - 1);
                let j = (i + 1).min(d.len() - 1);
                let t = q - i as f32;
                (d[i] * (1.0 - t) + d[j] * t) * scale
            })
            .collect()
    }
}

/// Learn the noise spectrum of `range` of `doc` on every channel: FFT frames of
/// [`PRINT_FFT`] at 50 % overlap, magnitudes averaged. Reads through the store a frame at a
/// time, so a long selection is never held in RAM. A selection shorter than one FFT frame
/// (about 85 ms at 48 kHz) is refused.
pub fn capture(doc: &DocState, range: Range<u64>) -> Result<NoisePrint, String> {
    let len = doc.len_frames();
    if range.start >= range.end || range.end > len {
        return Err("Select some noise first".to_string());
    }
    let n = PRINT_FFT as u64;
    if range.end - range.start < n {
        return Err(format!(
            "The noise selection is too short: at least {PRINT_FFT} frames are needed ({} ms)",
            1000 * n / u64::from(doc.sample_rate().max(1))
        ));
    }
    let stft = Stft::new(PRINT_FFT, 50)?;
    let energy = stft.window_energy().sqrt();
    let hop = stft.hop() as u64;
    let mut density = Vec::new();
    let mut frames = 0;
    let (mut scratch, mut spec) = (Vec::new(), vec![Complex32::new(0.0, 0.0); stft.bins()]);
    let mut buf = vec![0.0f32; PRINT_FFT];
    for c in 0..doc.channel_count() {
        let mut sum = vec![0.0f64; stft.bins()];
        frames = 0;
        let mut at = range.start;
        while at + n <= range.end {
            let got = doc.read(c, at, &mut buf).map_err(|e| e.to_string())?;
            if got != PRINT_FFT {
                return Err("The document is shorter than its length".to_string());
            }
            stft.analyse(&buf, at as i64, at as i64, &mut scratch, &mut spec);
            for (s, x) in sum.iter_mut().zip(&spec) {
                *s += f64::from(x.norm());
            }
            frames += 1;
            at += hop;
        }
        density.push(sum.iter().map(|s| (s / frames as f64) as f32 / energy).collect());
    }
    Ok(NoisePrint { sample_rate: doc.sample_rate(), density, frames })
}

static CURRENT: Mutex<Option<Arc<NoisePrint>>> = Mutex::new(None);

/// Make `print` the session's current noise print.
pub fn set_current(print: NoisePrint) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(print));
}

/// The session's current noise print, if one was captured.
pub fn current() -> Option<Arc<NoisePrint>> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Forget the current print.
pub fn clear() {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
