//! Noise prints (A337 / B066): the average noise magnitude spectrum of a selection, per channel,
//! kept in memory for the session (saving to a file: NoisePrint::save and NoisePrint::load). Noise Reduction
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

/// The first eight bytes of a noise print file.
pub const FILE_MAGIC: [u8; 8] = *b"WCNPRINT";
/// The version of the noise print file layout this build reads and writes.
pub const FILE_VERSION: u32 = 1;

/// Header: magic (8), version, sample rate, channels, frames and bins (4 bytes each).
const HEADER_BYTES: usize = 8 + 4 * 5;
/// The most channels a noise print file may hold.
const MAX_FILE_CHANNELS: usize = 64;
/// The largest noise print file `load` will read: the header plus 64 channels of bins.
pub const MAX_FILE_BYTES: u64 = (HEADER_BYTES + MAX_FILE_CHANNELS * (PRINT_FFT / 2 + 1) * 4) as u64;

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(word))
}

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

    /// Write the print to `path`, atomically. Layout, little endian: [`FILE_MAGIC`], then the
    /// version, sample rate, channel count, frames and bins as `u32`, then every channel's
    /// density as `f32`, channel by channel.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let bins = PRINT_FFT / 2 + 1;
        let channels = self.density.len();
        if channels == 0 || channels > MAX_FILE_CHANNELS {
            return Err("The noise print has no channels to save".to_string());
        }
        if self.sample_rate == 0 {
            return Err("The noise print has no sample rate".to_string());
        }
        if self.density.iter().any(|d| d.len() != bins) {
            return Err("The noise print has the wrong number of bins".to_string());
        }
        if self.density.iter().flatten().any(|v| !v.is_finite()) {
            return Err("The noise print has a value that is not a number".to_string());
        }
        let too_long = || "The noise print is too long to save".to_string();
        let frames = u32::try_from(self.frames).map_err(|_| too_long())?;
        let channels_word = u32::try_from(channels).map_err(|_| too_long())?;
        let bins_word = u32::try_from(bins).map_err(|_| too_long())?;

        let mut bytes = Vec::with_capacity(HEADER_BYTES + channels * bins * 4);
        bytes.extend_from_slice(&FILE_MAGIC);
        bytes.extend_from_slice(&FILE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&self.sample_rate.to_le_bytes());
        bytes.extend_from_slice(&channels_word.to_le_bytes());
        bytes.extend_from_slice(&frames.to_le_bytes());
        bytes.extend_from_slice(&bins_word.to_le_bytes());
        for value in self.density.iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        crate::atomic::write_atomic(path, |f| std::io::Write::write_all(f, &bytes)).map_err(|e| e.to_string())
    }

    /// Read a print written by [`NoisePrint::save`]. Every header number is checked, and the
    /// file's length is checked, before anything is allocated from it.
    pub fn load(path: &std::path::Path) -> Result<NoisePrint, String> {
        let too_large = || "The noise print file is too large".to_string();
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE_BYTES {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, MAX_FILE_BYTES + 1), &mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(too_large());
        }
        let not_a_print = || "Not a noise print file".to_string();
        if bytes.len() < HEADER_BYTES {
            return Err("Not a noise print file: it is too short".to_string());
        }
        if bytes.get(..8) != Some(&FILE_MAGIC[..]) {
            return Err(not_a_print());
        }
        let word = |at: usize| read_u32(&bytes, at).ok_or_else(not_a_print);
        if word(8)? != FILE_VERSION {
            return Err("Unsupported noise print version".to_string());
        }
        let sample_rate = word(12)?;
        let channels = word(16)? as usize;
        let frames = word(20)?;
        let bins = word(24)? as usize;
        if sample_rate == 0 {
            return Err("The noise print file has no sample rate".to_string());
        }
        if channels == 0 || channels > MAX_FILE_CHANNELS {
            return Err("The noise print file has a bad channel count".to_string());
        }
        if bins != PRINT_FFT / 2 + 1 {
            return Err("The noise print file has the wrong number of bins".to_string());
        }
        if bytes.len() != HEADER_BYTES + channels * bins * 4 {
            return Err("The noise print file is the wrong length".to_string());
        }
        let frames = usize::try_from(frames).map_err(|_| "The noise print file has a bad frame count".to_string())?;

        let damaged = || "The noise print file is damaged".to_string();
        let body = bytes.get(HEADER_BYTES..).ok_or_else(damaged)?;
        let (words, _) = body.as_chunks::<4>();
        let mut words = words.iter();
        let mut density = Vec::with_capacity(channels);
        for _ in 0..channels {
            let mut channel = Vec::with_capacity(bins);
            for _ in 0..bins {
                let word = *words.next().ok_or_else(damaged)?;
                let value = f32::from_le_bytes(word);
                if !value.is_finite() {
                    return Err("The noise print file has a value that is not a number".to_string());
                }
                channel.push(value);
            }
            density.push(channel);
        }
        Ok(NoisePrint { sample_rate, density, frames })
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
