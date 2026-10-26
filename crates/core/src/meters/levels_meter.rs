//! The Levels meter's per-channel readings (v1 item B4): peak with a dynamic peak hold and a
//! latched clip light, and RMS, all in dBFS, from SoundCraft's `soundcraft_dsp::meter`
//! `PeakMeter` (instant attack, 20 dB/s release, 2 s hold) and `RmsMeter` (300 ms average).
//! `process` neither allocates nor blocks, so the audio callback can feed it.

use soundcraft_dsp::meter::{PeakMeter, RmsMeter};

/// One channel's readings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelLevels {
    /// Peak level in dBFS (falls at 20 dB/s after the audio drops).
    pub peak_db: f32,
    /// Held peak in dBFS (held 2 s, then falls at 20 dB/s).
    pub hold_db: f32,
    /// RMS level in dBFS (an RMS of 0.5 reads -6.02).
    pub rms_db: f32,
    /// True once a sample reached full scale (absolute value 1.0 or more), until `reset_clip`.
    pub clipped: bool,
}

/// Peak, hold, clip and RMS meters for each channel.
#[derive(Debug, Clone)]
pub struct LevelsMeter {
    peaks: Vec<PeakMeter>,
    rms: Vec<RmsMeter>,
}

impl LevelsMeter {
    /// A meter for `channels` channels at `sample_rate` Hz (0 channels: a meter that reads nothing).
    pub fn new(sample_rate: f32, channels: usize) -> LevelsMeter {
        let peaks = (0..channels).map(|_| PeakMeter::new(sample_rate)).collect();
        let rms = (0..channels).map(|_| RmsMeter::new(sample_rate)).collect();
        LevelsMeter { peaks, rms }
    }

    /// Feeds one block of planar audio: `planar[c]` is channel `c`. Channels beyond the meter's
    /// channel count are ignored; a meter channel with no slice in `planar` is left as it is.
    pub fn process(&mut self, planar: &[&[f32]]) {
        for (peak_meter, slice) in self.peaks.iter_mut().zip(planar.iter()) {
            peak_meter.process(slice);
        }
        for (rms_meter, slice) in self.rms.iter_mut().zip(planar.iter()) {
            rms_meter.process(slice);
        }
    }

    /// The readings, one per channel, in channel order.
    pub fn levels(&self) -> Vec<ChannelLevels> {
        self.peaks
            .iter()
            .zip(self.rms.iter())
            .map(|(peak, rms)| ChannelLevels {
                peak_db: peak.level_db(),
                hold_db: peak.peak_hold_db(),
                rms_db: rms.rms_db(),
                clipped: peak.clipped(),
            })
            .collect()
    }

    /// Turns every channel's clip light off.
    pub fn reset_clip(&mut self) {
        for peak_meter in &mut self.peaks {
            peak_meter.reset_clip();
        }
    }

    /// Back to silence: every level at -144 dBFS and no clip light.
    pub fn reset(&mut self) {
        for peak_meter in &mut self.peaks {
            peak_meter.reset();
        }
        for rms_meter in &mut self.rms {
            rms_meter.reset();
        }
    }
}
