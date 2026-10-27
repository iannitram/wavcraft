//! Metronome click timing and click sounds (roadmap C201).
//!
//! Beat `n` (counted from 0 at the start of the document) falls at `round(n * beat_seconds * rate)`
//! frames, where the beat length comes from the document's [`Tempo`] (a constant tempo and meter).
//! Beat 0 and every `numerator`-th beat after it is the accented first beat of a bar. The click
//! sounds are made here from sine and noise bursts; nothing is sampled from any program. The
//! player mixes them into its output only, so a click never reaches the document or a file.

use crate::tempo::Tempo;

/// One click: which beat, where, and whether it opens a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Click {
    /// Beats from the start of the document, from 0.
    pub beat: u64,
    /// The frame the click starts at.
    pub frame: u64,
    /// The first beat of a bar.
    pub accent: bool,
}

/// The click grid of a document at one sample rate.
#[derive(Debug, Clone, Copy)]
pub struct ClickTrack {
    pub tempo: Tempo,
    pub rate: u32,
}

impl ClickTrack {
    pub fn new(tempo: Tempo, rate: u32) -> ClickTrack {
        ClickTrack { tempo, rate }
    }

    /// Frames in one beat, as an exact fraction.
    pub fn beat_frames(&self) -> f64 {
        self.tempo.beat_seconds() * f64::from(self.rate)
    }

    /// The frame beat `n` starts at.
    pub fn beat_frame(&self, n: u64) -> u64 {
        (n as f64 * self.beat_frames()).round() as u64
    }

    /// Whether beat `n` is the first of its bar.
    pub fn is_accent(&self, n: u64) -> bool {
        n.is_multiple_of(u64::from(self.tempo.numerator.max(1)))
    }

    /// The first beat that starts at or after `frame`.
    pub fn first_beat_at_or_after(&self, frame: u64) -> u64 {
        let bf = self.beat_frames();
        if bf <= 0.0 {
            return 0;
        }
        let mut n = (frame as f64 / bf).floor() as u64;
        while n > 0 && self.beat_frame(n - 1) >= frame {
            n -= 1;
        }
        while self.beat_frame(n) < frame {
            n += 1;
        }
        n
    }

    /// The clicks whose start lies in the frames `[start, end)`.
    pub fn clicks(&self, start: u64, end: u64) -> Vec<Click> {
        let mut out = Vec::new();
        let mut n = self.first_beat_at_or_after(start);
        loop {
            let frame = self.beat_frame(n);
            if frame >= end {
                break;
            }
            out.push(Click { beat: n, frame, accent: self.is_accent(n) });
            n += 1;
        }
        out
    }

    /// The clicks starting in `[start_s, end_s)` seconds.
    pub fn clicks_between(&self, start_s: f64, end_s: f64) -> Vec<Click> {
        let to_frame = |s: f64| (s.max(0.0) * f64::from(self.rate)).round() as u64;
        self.clicks(to_frame(start_s), to_frame(end_s))
    }

    /// Frames in `bars` bars.
    pub fn bars_frames(&self, bars: u32) -> u64 {
        self.beat_frame(u64::from(bars) * u64::from(self.tempo.numerator))
    }
}

/// The click sound at `rate`: a short sine burst (higher and longer for the accent) with a
/// few milliseconds of noise at the start, decaying exponentially. The first sample is never 0,
/// so a click's start can be found in the output. Peak level 0.8; the player scales it.
pub fn click_sound(rate: u32, accent: bool) -> Vec<f32> {
    let (freq, secs) = if accent { (1600.0f64, 0.045f64) } else { (1000.0, 0.035) };
    let n = ((secs * f64::from(rate)) as usize).max(8);
    let mut seed: u32 = if accent { 0x9E37_79B9 } else { 0x85EB_CA6B };
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let attack = (t / 0.0005).clamp(0.2, 1.0);
            let decay = (-t / (secs / 5.0)).exp();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = if t < 0.003 { (f64::from(seed >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * 0.4 } else { 0.0 };
            let tone = (std::f64::consts::TAU * freq * t + std::f64::consts::FRAC_PI_4).sin();
            let v = (tone * 0.6 + noise) * attack * decay;
            // Keep the first sample off zero whatever the noise did.
            let v = if i == 0 && v.abs() < 0.05 { 0.05 } else { v };
            (v * 0.8 / 0.9).clamp(-0.8, 0.8) as f32
        })
        .collect()
}

pub mod record_count_in;

#[cfg(test)]
mod tests {
    use super::*;

    fn track(bpm: f64, n: u32, d: u32, rate: u32) -> ClickTrack {
        ClickTrack::new(Tempo::new(bpm, n, d).unwrap(), rate)
    }

    fn frames(c: &[Click]) -> Vec<u64> {
        c.iter().map(|c| c.frame).collect()
    }

    #[test]
    fn four_four_at_120_clicks_every_half_second_with_the_accent_on_bar_1() {
        let c = track(120.0, 4, 4, 44_100).clicks_between(0.0, 4.0);
        assert_eq!(frames(&c), (0..8).map(|k| k * 22_050).collect::<Vec<_>>());
        let accents: Vec<bool> = c.iter().map(|c| c.accent).collect();
        assert_eq!(accents, [true, false, false, false, true, false, false, false]);
        let c = track(120.0, 4, 4, 48_000).clicks_between(0.0, 4.0);
        assert_eq!(frames(&c), (0..8).map(|k| k * 24_000).collect::<Vec<_>>());
    }

    #[test]
    fn three_four_at_90_clicks_every_two_thirds_of_a_second() {
        let c = track(90.0, 3, 4, 44_100).clicks_between(0.0, 4.0);
        assert_eq!(frames(&c), [0, 29_400, 58_800, 88_200, 117_600, 147_000]);
        assert_eq!(c.iter().filter(|c| c.accent).map(|c| c.beat).collect::<Vec<_>>(), [0, 3]);
        let c = track(90.0, 3, 4, 48_000).clicks_between(0.0, 4.0);
        assert_eq!(frames(&c), [0, 32_000, 64_000, 96_000, 128_000, 160_000]);
    }

    #[test]
    fn a_window_in_the_middle_gets_the_beats_inside_and_the_end_is_exclusive() {
        let t = track(120.0, 4, 4, 44_100);
        // [0.5 s, 2.0 s): beats at 0.5, 1.0, 1.5; the beat at exactly 2.0 s is out.
        let c = t.clicks_between(0.5, 2.0);
        assert_eq!(frames(&c), [22_050, 44_100, 66_150]);
        assert_eq!(c[0].beat, 1);
        // Just after a beat: the next one is first.
        assert_eq!(frames(&t.clicks(22_051, 44_101)), [44_100]);
        assert!(t.clicks(5, 5).is_empty());
    }

    #[test]
    fn six_eight_beats_are_eighth_notes() {
        // 6/8 at 120: a beat (an eighth) is 0.25 s, a bar 1.5 s with the accent every sixth beat.
        let c = track(120.0, 6, 8, 48_000).clicks_between(0.0, 1.5);
        assert_eq!(c.len(), 6);
        assert_eq!(c[1].frame, 12_000);
        assert!(c[0].accent && c[1..].iter().all(|c| !c.accent));
        assert_eq!(track(120.0, 6, 8, 48_000).bars_frames(1), 72_000);
    }

    #[test]
    fn a_count_in_of_n_bars_is_n_times_the_bar() {
        assert_eq!(track(120.0, 4, 4, 44_100).bars_frames(2), 176_400);
        assert_eq!(track(90.0, 3, 4, 48_000).bars_frames(1), 96_000);
        assert_eq!(track(90.0, 3, 4, 48_000).bars_frames(0), 0);
    }

    #[test]
    fn the_click_sounds_are_short_distinct_and_start_off_zero() {
        for rate in [44_100, 48_000] {
            let (a, n) = (click_sound(rate, true), click_sound(rate, false));
            assert!(a[0] != 0.0 && n[0] != 0.0);
            assert!(a.len() > n.len() && a.len() < rate as usize / 10);
            assert!(a != n);
            assert!(a.iter().chain(&n).all(|v| v.abs() <= 0.8));
            assert!(a.iter().map(|v| v.abs()).fold(0.0, f32::max) > 0.3);
        }
    }
}
