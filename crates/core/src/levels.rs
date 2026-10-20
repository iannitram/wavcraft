//! Peak level meters (f26, roadmap A213): what the player sends, as dBFS bars with peak hold and
//! a clip light.
//!
//! Two halves, so the audio thread never waits for the window:
//! - [`LevelTap`]: the audio callback records the largest sample magnitude it sent on each of the
//!   first two channels with one lock-free `fetch_max` per channel per callback. No lock, no
//!   allocation.
//! - [`Levels`]: the window takes those peaks once a frame ([`LevelTap::take`]) and feeds them,
//!   with the time since the last frame, to soundcraft-dsp's `PeakMeter` per channel (instant
//!   attack, 20 dB/s fall, peak hold, clip latch).

use std::sync::atomic::{AtomicU32, Ordering};

use soundcraft_dsp::meter::PeakMeter;

/// How long the peak-hold tick stays before it falls, in seconds.
pub const HOLD_SECS: f32 = 1.5;
/// How far down the bars go, dBFS.
pub const FLOOR_DB: f32 = -60.0;
/// A channel's meter can have this many frames of rate to turn a frame's duration into samples.
const METER_RATE: f32 = 48_000.0;

/// The audio thread's side: the peak since the window last looked, per channel.
#[derive(Debug, Default)]
pub struct LevelTap {
    /// Non-negative `f32` bits, which order the same as the numbers.
    peak: [AtomicU32; 2],
}

impl LevelTap {
    /// Record a magnitude for channel `ch` (0 or 1; others ignored). Lock-free; safe to call from
    /// the audio callback.
    pub fn record(&self, ch: usize, magnitude: f32) {
        if let Some(slot) = self.peak.get(ch)
            && magnitude.is_finite()
            && magnitude > 0.0
        {
            slot.fetch_max(magnitude.to_bits(), Ordering::Relaxed);
        }
    }

    /// The peaks since the last call, and start over.
    pub fn take(&self) -> [f32; 2] {
        [f32::from_bits(self.peak[0].swap(0, Ordering::Relaxed)), f32::from_bits(self.peak[1].swap(0, Ordering::Relaxed))]
    }
}

/// What a channel's bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// The bar, dBFS (falls 20 dB/s).
    pub level_db: f32,
    /// The peak-hold tick, dBFS.
    pub hold_db: f32,
    /// A sample reached full scale since the light was last reset.
    pub clipped: bool,
}

/// The window's side: two channel meters.
pub struct Levels {
    meters: [PeakMeter; 2],
}

impl Default for Levels {
    fn default() -> Self {
        Levels::new()
    }
}

impl Levels {
    pub fn new() -> Levels {
        let mut meters = [PeakMeter::new(METER_RATE), PeakMeter::new(METER_RATE)];
        for m in &mut meters {
            m.set_hold_secs(HOLD_SECS);
        }
        Levels { meters }
    }

    /// Feed the peaks taken from the tap (linear) covering `dt_secs` since the previous call.
    pub fn update(&mut self, peaks: [f32; 2], dt_secs: f32) {
        let frames = (dt_secs.clamp(0.0, 1.0) * METER_RATE) as usize;
        for (m, p) in self.meters.iter_mut().zip(peaks) {
            m.feed(p, frames);
        }
    }

    pub fn reading(&self, ch: usize) -> Reading {
        let m = &self.meters[ch.min(1)];
        Reading { level_db: m.level_db(), hold_db: m.peak_hold_db(), clipped: m.clipped() }
    }

    /// Whether any bar or hold tick is still above the floor (the window keeps repainting while so).
    pub fn active(&self) -> bool {
        self.meters.iter().any(|m| m.level_db() > FLOOR_DB || m.peak_hold_db() > FLOOR_DB)
    }

    /// Put the clip lights out (a click on one).
    pub fn reset_clip(&mut self) {
        self.meters.iter_mut().for_each(PeakMeter::reset_clip);
    }

    /// Drop everything (a new file).
    pub fn reset(&mut self) {
        self.meters.iter_mut().for_each(PeakMeter::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    #[test]
    fn the_tap_keeps_the_largest_and_take_starts_over() {
        let tap = LevelTap::default();
        tap.record(0, 0.25);
        tap.record(0, 0.5);
        tap.record(0, 0.1);
        tap.record(1, 0.125);
        tap.record(2, 9.0);
        tap.record(1, f32::NAN);
        tap.record(1, -0.5);
        assert_eq!(tap.take(), [0.5, 0.125]);
        assert_eq!(tap.take(), [0.0, 0.0]);
    }

    #[test]
    fn a_peak_shows_at_once_holds_then_falls_at_twenty_db_a_second() {
        let mut l = Levels::new();
        l.update([0.5, 0.0], 0.05);
        let r = l.reading(0);
        assert!((r.level_db - db(0.5)).abs() < 0.01, "{r:?}");
        assert!((r.hold_db - db(0.5)).abs() < 0.01);
        assert!(l.reading(1).level_db < -100.0, "the other channel is quiet");
        // Within the hold time the tick stays while the bar falls.
        for _ in 0..20 {
            l.update([0.0, 0.0], 0.05);
        }
        let r = l.reading(0);
        assert!((r.hold_db - db(0.5)).abs() < 0.01, "held for 1 s: {r:?}");
        assert!((r.level_db - (db(0.5) - 20.0)).abs() < 0.5, "fell about 20 dB in 1 s: {r:?}");
        // After the hold time the tick falls too, and nothing stays lit forever.
        for _ in 0..400 {
            l.update([0.0, 0.0], 0.05);
        }
        assert!(!l.active());
    }

    #[test]
    fn the_clip_light_latches_until_reset() {
        let mut l = Levels::new();
        l.update([0.99, 1.0], 0.02);
        assert_eq!((l.reading(0).clipped, l.reading(1).clipped), (false, true));
        for _ in 0..500 {
            l.update([0.0, 0.0], 0.05);
        }
        assert!(l.reading(1).clipped, "still lit long after the signal fell");
        l.reset_clip();
        assert!(!l.reading(1).clipped);
    }
}
