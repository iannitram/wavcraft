//! Amplitude Statistics (v1 item G4): peak, true peak, RMS, integrated loudness, DC offset and
//! clipped samples of some audio, measured without changing it. The audio is fed in chunks
//! (`AmplitudeScan::push`) so a long file never has to be in memory at once. True peak and
//! integrated loudness come from SoundCraft's `soundcraft_dsp::meter::LoudnessMeter`.

use soundcraft_dsp::gain_to_db;
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
        AmplitudeScan {
            loudness: LoudnessMeter::new(sample_rate, channels),
            peak: vec![0.0; channels],
            sum: vec![0.0; channels],
            sum_sq: vec![0.0; channels],
            clipped: vec![0; channels],
            frames: 0,
        }
    }

    /// Adds one chunk of planar audio (`planar[c]` is channel `c`). The chunk's length is the
    /// shortest of the scan's channels; channels beyond the scan's count are ignored; a chunk with
    /// fewer channels than the scan is ignored whole, and a scan of 0 channels ignores every chunk.
    /// NaN and infinite samples count as 0.0.
    pub fn push(&mut self, planar: &[Vec<f32>]) {
        let ch = self.peak.len();
        if ch == 0 || planar.len() < ch {
            return;
        }
        let Some(active) = planar.get(..ch) else { return };
        let n = active.iter().map(Vec::len).min().unwrap_or(0);
        for (c, samples) in active.iter().enumerate() {
            for x in samples.iter().take(n) {
                let v = if x.is_finite() { *x } else { 0.0 };
                let a = v.abs();
                if let Some(peak) = self.peak.get_mut(c) {
                    *peak = peak.max(a);
                }
                if let Some(sum) = self.sum.get_mut(c) {
                    *sum += f64::from(v);
                }
                if let Some(sum_sq) = self.sum_sq.get_mut(c) {
                    *sum_sq += f64::from(v) * f64::from(v);
                }
                if a >= 1.0
                    && let Some(clipped) = self.clipped.get_mut(c)
                {
                    *clipped += 1;
                }
            }
        }
        self.loudness.process(active, n);
        self.frames += n as u64;
    }

    /// The statistics of everything pushed so far.
    pub fn finish(&self) -> AmplitudeStats {
        let frames_f = self.frames as f64;
        let channels = self
            .peak
            .iter()
            .zip(&self.sum)
            .zip(&self.sum_sq)
            .zip(&self.clipped)
            .map(|(((peak, sum), sum_sq), clipped)| {
                let (rms_db, dc_offset_percent) = if self.frames == 0 {
                    (-144.0, 0.0)
                } else {
                    let rms = (*sum_sq / frames_f).sqrt() as f32;
                    (gain_to_db(rms), (*sum / frames_f * 100.0) as f32)
                };
                ChannelStats { peak_db: gain_to_db(*peak), rms_db, dc_offset_percent, clipped_samples: *clipped }
            })
            .collect();
        AmplitudeStats { channels, true_peak_db: self.loudness.true_peak_db(), integrated_lufs: self.loudness.integrated_lufs(), frames: self.frames }
    }
}

/// The statistics of `planar` in one go: a scan of `planar.len()` channels, one push, finish.
pub fn amplitude_stats(planar: &[Vec<f32>], sample_rate: f32) -> AmplitudeStats {
    let mut scan = AmplitudeScan::new(sample_rate, planar.len());
    scan.push(planar);
    scan.finish()
}
