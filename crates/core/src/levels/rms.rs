//! RMS level reading (roadmap A217, core only): the second number beside the peak meter.
//!
//! Definition: the RMS of the last `window` samples of a channel, as dBFS. Squares of the most
//! recent `window` samples are kept in a ring and summed as they enter and leave, so a reading is
//! `10 * log10(sum / window)`. The window is always `window` samples long: samples not fed yet
//! count as silence, so a meter that has only seen half a window reads 3 dB lower than the same
//! signal over a full one. A sample that is NaN or infinite counts as silence. Anything at or
//! below [`FLOOR_DB`] (including silence, an empty meter and a reading that cannot be computed)
//! reads exactly [`FLOOR_DB`], the same floor the peak bars use.
//!
//! This module only measures. It is not wired into the audio callback or the Levels panel yet.

use super::{FLOOR_DB, METER_RATE};

/// The default RMS window, milliseconds.
pub const RMS_WINDOW_MS: f32 = 300.0;
/// The longest window, samples (about 87 s at 48 kHz); longer requests are cut to this.
pub const MAX_WINDOW: usize = 1 << 22;

/// A mean square (linear power, full scale sine = 0.5) as dBFS: `10 * log10(mean_square)`.
pub fn mean_square_to_db(mean_square: f64) -> f32 {
    if !mean_square.is_finite() || mean_square <= 0.0 {
        FLOOR_DB
    } else {
        let db = (10.0 * mean_square.log10()) as f32;
        db.max(FLOOR_DB)
    }
}

/// How many samples `window_ms` milliseconds is at `rate` Hz.
pub fn window_samples(rate: f32, window_ms: f32) -> usize {
    if !(rate.is_finite() && rate > 0.0 && window_ms.is_finite() && window_ms > 0.0) {
        1
    } else {
        ((rate * window_ms / 1000.0).round() as usize).clamp(1, MAX_WINDOW)
    }
}

/// One channel's RMS over a sliding window.
#[derive(Debug, Clone, PartialEq)]
pub struct RmsMeter {
    window: usize,
    squares: Vec<f64>,
    pos: usize,
    sum: f64,
}

impl RmsMeter {
    /// A silent meter over `window` samples (0 becomes 1; more than [`MAX_WINDOW`] is cut).
    pub fn new(window: usize) -> RmsMeter {
        let window = window.clamp(1, MAX_WINDOW);
        RmsMeter { window, squares: vec![0.0; window], pos: 0, sum: 0.0 }
    }

    /// The window length, samples.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Feed the next samples of the channel, oldest first.
    pub fn feed(&mut self, samples: &[f32]) {
        for s in samples {
            let sq = if s.is_finite() { *s as f64 * *s as f64 } else { 0.0 };
            self.sum += sq - self.squares[self.pos];
            self.squares[self.pos] = sq;
            self.pos = (self.pos + 1) % self.window;
        }
        if self.sum < 0.0 {
            self.sum = 0.0;
        }
    }

    /// The RMS of the window, dBFS, never below [`FLOOR_DB`].
    pub fn rms_db(&self) -> f32 {
        mean_square_to_db(self.sum / self.window as f64)
    }

    /// Change the window length (same limits as [`RmsMeter::new`]). The meter starts over, silent.
    pub fn set_window(&mut self, window: usize) {
        *self = RmsMeter::new(window);
    }

    /// Back to silence, same window.
    pub fn reset(&mut self) {
        self.squares.fill(0.0);
        self.pos = 0;
        self.sum = 0.0;
    }
}

/// What a channel's RMS reading shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RmsReading {
    /// The window's RMS, dBFS.
    pub rms_db: f32,
}

/// Two channel RMS meters, the pair of [`super::Levels`].
#[derive(Debug, Clone, PartialEq)]
pub struct RmsLevels {
    rate: f32,
    meters: [RmsMeter; 2],
}

impl Default for RmsLevels {
    fn default() -> Self {
        RmsLevels::new(METER_RATE, RMS_WINDOW_MS)
    }
}

impl RmsLevels {
    /// Two silent meters whose window is `window_ms` at `rate` Hz.
    pub fn new(rate: f32, window_ms: f32) -> RmsLevels {
        let w = window_samples(rate, window_ms);
        RmsLevels { rate, meters: [RmsMeter::new(w), RmsMeter::new(w)] }
    }

    /// The window length, samples (both channels share it).
    pub fn window(&self) -> usize {
        self.meters[0].window()
    }

    /// Feed channel `ch` (0 or 1; a larger number means 1) its next samples.
    pub fn feed(&mut self, ch: usize, samples: &[f32]) {
        self.meters[ch.min(1)].feed(samples);
    }

    /// Channel `ch`'s reading (a larger number means 1).
    pub fn reading(&self, ch: usize) -> RmsReading {
        RmsReading { rms_db: self.meters[ch.min(1)].rms_db() }
    }

    /// Set both windows to `window_ms` at the rate given to [`RmsLevels::new`]. Both start over.
    pub fn set_window_ms(&mut self, window_ms: f32) {
        let w = window_samples(self.rate, window_ms);
        for m in &mut self.meters {
            m.set_window(w);
        }
    }

    /// Both channels back to silence, same window.
    pub fn reset(&mut self) {
        for m in &mut self.meters {
            m.reset();
        }
    }
}
