//! Input metering (f56, roadmap A216, C033, C013): what an input stream hears, for the Levels
//! panel while nothing is recording and for the mic check.
//!
//! The input callback's side is [`InputTap::feed`]: a few atomic operations per callback, no lock
//! and no allocation. The window takes the per-channel peaks once a frame ([`InputTap::take_peaks`],
//! the same [`LevelTap`] the player uses) and, for a mic check, reads the run's totals
//! ([`InputTap::stats`]) when the time is up.
//!
//! [`FakeInput`] is a synthetic source (`WAVCRAFT_FAKE_INPUT=sine:440:-12`) so tests and scripted
//! runs never open a real input device (which on some systems asks for microphone permission).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::levels::LevelTap;

/// A sample at or above this magnitude counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;
/// A peak under this, dBFS, is too quiet to record from.
pub const QUIET_BELOW_DB: f32 = -30.0;
/// The rate the fake input runs at.
pub const FAKE_RATE: u32 = 48_000;

/// `20 log10(x)`, with silence as negative infinity.
pub fn to_db(linear: f32) -> f32 {
    if linear > 0.0 { 20.0 * linear.log10() } else { f32::NEG_INFINITY }
}

/// The input callback writes here; the window reads.
#[derive(Debug, Default)]
pub struct InputTap {
    /// Per-channel peaks since the window last looked (the meter bars).
    levels: LevelTap,
    /// The largest magnitude since the last [`InputTap::reset_stats`], `f32` bits.
    peak_all: AtomicU32,
    /// Sum of squares of every sample since then, `f64` bits.
    sum_sq: AtomicU64,
    /// Samples counted, and how many were at or above [`CLIP_LEVEL`].
    samples: AtomicU64,
    clips: AtomicU64,
}

impl InputTap {
    /// Take in one callback's samples (interleaved, `channels` per frame; the first two channels
    /// are metered, a mono input shows on both). Lock-free and allocation-free.
    pub fn feed(&self, samples: impl Iterator<Item = f32>, channels: usize) {
        let channels = channels.max(1);
        let (mut peak, mut peak_all, mut sum_sq, mut n, mut clips) = ([0.0f32; 2], 0.0f32, 0.0f64, 0u64, 0u64);
        for (i, s) in samples.enumerate() {
            let m = if s.is_finite() { s.abs() } else { 0.0 };
            let ch = i % channels;
            if ch < 2 {
                peak[ch] = peak[ch].max(m);
            }
            peak_all = peak_all.max(m);
            sum_sq += f64::from(m) * f64::from(m);
            n += 1;
            clips += u64::from(m >= CLIP_LEVEL);
        }
        if channels == 1 {
            peak[1] = peak[0];
        }
        self.levels.record(0, peak[0]);
        self.levels.record(1, peak[1]);
        if n == 0 {
            return;
        }
        self.peak_all.fetch_max(peak_all.to_bits(), Ordering::Relaxed);
        let mut cur = self.sum_sq.load(Ordering::Relaxed);
        while let Err(seen) = self.sum_sq.compare_exchange_weak(cur, (f64::from_bits(cur) + sum_sq).to_bits(), Ordering::Relaxed, Ordering::Relaxed) {
            cur = seen;
        }
        self.samples.fetch_add(n, Ordering::Relaxed);
        self.clips.fetch_add(clips, Ordering::Relaxed);
    }

    /// The peaks per channel since the last call (linear), and start over.
    pub fn take_peaks(&self) -> [f32; 2] {
        self.levels.take()
    }

    /// Start a new run of totals (the mic check's start).
    pub fn reset_stats(&self) {
        self.peak_all.store(0, Ordering::Relaxed);
        self.sum_sq.store(0f64.to_bits(), Ordering::Relaxed);
        self.samples.store(0, Ordering::Relaxed);
        self.clips.store(0, Ordering::Relaxed);
    }

    /// The totals since [`InputTap::reset_stats`].
    pub fn stats(&self) -> InputStats {
        let n = self.samples.load(Ordering::Relaxed);
        let rms = if n == 0 { 0.0 } else { (f64::from_bits(self.sum_sq.load(Ordering::Relaxed)) / n as f64).sqrt() as f32 };
        InputStats { peak: f32::from_bits(self.peak_all.load(Ordering::Relaxed)), rms, samples: n, clips: self.clips.load(Ordering::Relaxed) }
    }
}

/// What a listening run heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputStats {
    /// Largest magnitude, linear.
    pub peak: f32,
    /// Root-mean-square over every sample of every channel, linear.
    pub rms: f32,
    pub samples: u64,
    pub clips: u64,
}

/// The mic check's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing came in, or the peak stayed under [`QUIET_BELOW_DB`].
    TooQuiet,
    Ok,
    /// At least one sample reached full scale.
    Clipping,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::TooQuiet => "too-quiet",
            Verdict::Ok => "ok",
            Verdict::Clipping => "clipping",
        }
    }

    pub fn advice(self) -> &'static str {
        match self {
            Verdict::TooQuiet => "Too quiet: raise the input gain or move closer.",
            Verdict::Ok => "Level is fine.",
            Verdict::Clipping => "Clipping: lower the input gain.",
        }
    }
}

impl InputStats {
    pub fn peak_db(&self) -> f32 {
        to_db(self.peak)
    }

    pub fn average_db(&self) -> f32 {
        to_db(self.rms)
    }

    pub fn verdict(&self) -> Verdict {
        if self.clips > 0 {
            Verdict::Clipping
        } else if self.samples == 0 || self.peak_db() < QUIET_BELOW_DB {
            Verdict::TooQuiet
        } else {
            Verdict::Ok
        }
    }

    /// `peak=<dBFS> average=<dBFS> clips=<n> verdict=<word>` (`-inf` for silence).
    pub fn line(&self) -> String {
        let f = |v: f32| if v.is_finite() { format!("{v:.2}") } else { "-inf".to_string() };
        format!("peak={} average={} clips={} verdict={}", f(self.peak_db()), f(self.average_db()), self.clips, self.verdict().word())
    }
}

/// A synthetic input: a sine at a level, or silence, on two channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FakeInput {
    pub hz: f32,
    /// Peak level, dBFS (above 0 clamps to full scale, which clips).
    pub level_db: f32,
    phase: f64,
}

impl FakeInput {
    /// `sine:<hz>[:<dBFS>]` (level defaults to -12) or `silence`.
    pub fn parse(spec: &str) -> Result<FakeInput, String> {
        let spec = spec.trim();
        if spec == "silence" {
            return Ok(FakeInput { hz: 0.0, level_db: f32::NEG_INFINITY, phase: 0.0 });
        }
        let mut parts = spec.split(':');
        if parts.next() != Some("sine") {
            return Err(format!("fake input {spec:?}: expected sine:<hz>[:<dBFS>] or silence"));
        }
        let hz: f32 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .filter(|h| *h > 0.0 && *h < FAKE_RATE as f32 / 2.0)
            .ok_or("fake input: the frequency must be above 0 and below half the rate")?;
        let level_db: f32 = match parts.next() {
            Some(p) => p.parse().map_err(|_| format!("fake input: bad level {p:?}"))?,
            None => -12.0,
        };
        Ok(FakeInput { hz, level_db, phase: 0.0 })
    }

    /// The next `frames` frames, interleaved stereo, fed to `tap` as an input callback would
    /// (no allocation).
    pub fn feed(&mut self, tap: &InputTap, frames: usize) {
        let amp = if self.level_db.is_finite() { 10f32.powf(self.level_db / 20.0) } else { 0.0 };
        let step = std::f64::consts::TAU * f64::from(self.hz) / f64::from(FAKE_RATE);
        let start = self.phase;
        tap.feed((0..frames * 2).map(|i| (amp * (start + step * (i / 2) as f64).sin() as f32).clamp(-1.0, 1.0)), 2);
        self.phase = (start + step * frames as f64) % std::f64::consts::TAU;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spec: &str, secs: f32) -> InputStats {
        let tap = InputTap::default();
        let mut fake = FakeInput::parse(spec).unwrap();
        for _ in 0..(secs * 100.0) as usize {
            fake.feed(&tap, 480);
        }
        tap.stats()
    }

    #[test]
    fn a_sine_at_a_known_level_reads_its_peak_and_its_rms() {
        let s = run("sine:440:-12", 1.0);
        assert!((s.peak_db() - -12.0).abs() < 0.05, "{s:?}");
        assert!((s.average_db() - (-12.0 - 3.0103)).abs() < 0.05, "rms of a sine is 3 dB under its peak: {s:?}");
        assert_eq!(s.clips, 0);
        assert_eq!(s.verdict(), Verdict::Ok);
    }

    #[test]
    fn verdicts_quiet_ok_clipping() {
        assert_eq!(run("sine:440:-50", 0.5).verdict(), Verdict::TooQuiet);
        assert_eq!(run("silence", 0.5).verdict(), Verdict::TooQuiet);
        assert_eq!(run("sine:440:-6", 0.5).verdict(), Verdict::Ok);
        let c = run("sine:440:6", 0.5);
        assert!(c.clips > 0 && c.verdict() == Verdict::Clipping, "{c:?}");
        assert_eq!(InputStats { peak: 0.0, rms: 0.0, samples: 0, clips: 0 }.verdict(), Verdict::TooQuiet, "nothing heard");
    }

    #[test]
    fn the_peaks_per_channel_follow_the_meter_and_mono_shows_on_both() {
        let tap = InputTap::default();
        tap.feed([0.25f32, -0.5, 0.125, 0.0].into_iter(), 2);
        assert_eq!(tap.take_peaks(), [0.25, 0.5]);
        assert_eq!(tap.take_peaks(), [0.0, 0.0], "taking starts over");
        tap.feed([0.0f32, 0.75].into_iter(), 1);
        assert_eq!(tap.take_peaks(), [0.75, 0.75]);
        // Taking the meter's peaks leaves the run's totals alone; resetting clears them.
        assert!((tap.stats().peak - 0.75).abs() < 1e-6);
        tap.reset_stats();
        assert_eq!(tap.stats().samples, 0);
    }

    #[test]
    fn bad_samples_are_ignored_not_propagated() {
        let tap = InputTap::default();
        tap.feed([f32::NAN, f32::INFINITY, 0.5].into_iter(), 1);
        let s = tap.stats();
        assert!((s.peak - 0.5).abs() < 1e-6 && s.rms.is_finite(), "{s:?}");
    }

    #[test]
    fn fake_specs_parse_or_say_why_not() {
        assert!(FakeInput::parse("sine:440").is_ok());
        assert_eq!(FakeInput::parse("sine:1000:-20").unwrap().level_db, -20.0);
        assert!(FakeInput::parse("silence").is_ok());
        for bad in ["", "square:1", "sine", "sine:0", "sine:99999", "sine:440:loud"] {
            assert!(FakeInput::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_line_reports_numbers_and_the_word() {
        assert_eq!(run("sine:440:-12", 0.5).line().split(' ').nth(3), Some("verdict=ok"));
        assert!(run("silence", 0.2).line().starts_with("peak=-inf average=-inf clips=0"));
    }
}
