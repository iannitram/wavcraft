//! Amplitude Statistics (v1 item G4): peak, true peak, RMS, integrated loudness, DC offset and
//! clipped samples of some audio, measured without changing it. The audio is fed in chunks
//! (`AmplitudeScan::push`) so a long file never has to be in memory at once. True peak and
//! integrated loudness come from SoundCraft's `soundcraft_dsp::meter::LoudnessMeter`.

// Stub for wav-c-118-c: remove this allowance with the todo!() bodies.
#![allow(clippy::todo, dead_code)]

use soundcraft_dsp::meter::LoudnessMeter;

/// One channel's statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelStats {
    /// Highest absolute sample value in dBFS (-144 for silence or no audio).
    pub peak_db: f32,
    /// RMS over all frames in dBFS (an RMS of 0.5 reads -6.02; -144 for silence or no audio).
    pub rms_db: f32,
    /// Mean sample value as a percentage of full scale (0.25 reads 25.0; 0.0 with no audio).
    pub dc_offset_percent: f32,
    /// Number of samples whose absolute value is 1.0 or more.
    pub clipped_samples: u64,
}

/// The statistics of everything pushed so far.
#[derive(Debug, Clone, PartialEq)]
pub struct AmplitudeStats {
    /// One entry per channel, in channel order.
    pub channels: Vec<ChannelStats>,
    /// Highest 4x-oversampled true peak over all channels, in dBTP.
    pub true_peak_db: f32,
    /// Gated integrated loudness (ITU-R BS.1770), in LUFS.
    pub integrated_lufs: f32,
    /// Frames measured.
    pub frames: u64,
}

/// A running measurement, fed chunk by chunk.
#[derive(Debug, Clone)]
pub struct AmplitudeScan {
    loudness: LoudnessMeter,
    peak: Vec<f32>,
    sum: Vec<f64>,
    sum_sq: Vec<f64>,
    clipped: Vec<u64>,
    frames: u64,
}

impl AmplitudeScan {
    /// A scan of `channels` channels at `sample_rate` Hz.
    pub fn new(sample_rate: f32, channels: usize) -> AmplitudeScan {
        todo!("wav-c-118-c: new({sample_rate}, {channels})")
    }

    /// Adds one chunk of planar audio (`planar[c]` is channel `c`). The chunk's length is the
    /// shortest of the scan's channels; channels beyond the scan's count are ignored; a chunk with
    /// fewer channels than the scan is ignored whole, and a scan of 0 channels ignores every chunk.
    /// NaN and infinite samples count as 0.0.
    pub fn push(&mut self, planar: &[Vec<f32>]) {
        todo!("wav-c-118-c: push({} channels)", planar.len())
    }

    /// The statistics of everything pushed so far.
    pub fn finish(&self) -> AmplitudeStats {
        todo!("wav-c-118-c: finish")
    }
}

/// The statistics of `planar` in one go: a scan of `planar.len()` channels, one push, finish.
pub fn amplitude_stats(planar: &[Vec<f32>], sample_rate: f32) -> AmplitudeStats {
    todo!("wav-c-118-c: amplitude_stats({} channels, {sample_rate})", planar.len())
}
