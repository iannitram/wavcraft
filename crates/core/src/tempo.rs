//! A document's tempo and time signature, and Bars and Beats times (`bars:beats.ticks`).
//!
//! The mapping between samples and musical time is `soundcraft-time`'s `TempoMap` (a constant
//! tempo and meter here; its 960 ticks to a quarter note). A time reads `bar:beat.tick`, bars and
//! beats counted from 1, ticks from 0: `1:1.000` is the start, `2:3.480` bar 2, beat 3, 480 ticks
//! in. Roadmap A101.

use soundcraft_time::{BarBeat, SampleRate, TempoMap};

/// The slowest and fastest tempo accepted, in beats a minute.
pub const BPM_RANGE: std::ops::RangeInclusive<f64> = 20.0..=400.0;

/// Tempo (beats a minute, a beat being the meter's denominator note) and time signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for Tempo {
    fn default() -> Self {
        Tempo { bpm: 120.0, numerator: 4, denominator: 4 }
    }
}

impl Tempo {
    /// `Some` when the tempo is in [`BPM_RANGE`], the numerator 1 to 32 and the denominator 1, 2, 4, 8, 16 or 32.
    pub fn new(bpm: f64, numerator: u32, denominator: u32) -> Option<Tempo> {
        let ok = BPM_RANGE.contains(&bpm) && (1..=32).contains(&numerator) && [1, 2, 4, 8, 16, 32].contains(&denominator);
        ok.then_some(Tempo { bpm, numerator, denominator })
    }

    /// A time signature written `3/4`.
    pub fn parse_meter(text: &str) -> Option<(u32, u32)> {
        let (n, d) = text.trim().split_once('/')?;
        let (n, d) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
        Tempo::new(120.0, n, d).map(|t| (t.numerator, t.denominator))
    }

    /// The meter as `4/4`.
    pub fn meter_text(&self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    /// The tempo as `120` or `93.5`.
    pub fn bpm_text(&self) -> String {
        let t = format!("{:.3}", self.bpm);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    fn map(&self) -> Option<TempoMap> {
        TempoMap::new(self.bpm, self.numerator, self.denominator).ok()
    }

    /// Seconds in one beat.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm * 4.0 / f64::from(self.denominator)
    }

    /// Seconds in one bar.
    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * f64::from(self.numerator)
    }

    /// `frame` at `rate` as `bar:beat.tick`.
    pub fn format(&self, frame: usize, rate: u32) -> String {
        match (self.map(), SampleRate::new(rate)) {
            (Some(map), Ok(sr)) => Self::text(&map, map.samples_to_ticks(i64::try_from(frame).unwrap_or(i64::MAX), sr)),
            _ => "1:1.000".to_string(),
        }
    }

    /// The ruler label at `seconds`.
    pub fn format_tick(&self, seconds: f64, rate: u32) -> String {
        self.format((seconds.max(0.0) * f64::from(rate)).round() as usize, rate)
    }

    fn text(map: &TempoMap, tick: i64) -> String {
        let bb = map.bar_beat_at_tick(tick.max(0));
        format!("{}:{}.{:03}", bb.bar, bb.beat, bb.tick)
    }

    /// The first frame whose time reads as `text` (`bar:beat.tick`; `bar:beat` and `bar` also
    /// read, with the missing parts at their start), or `None` when `text` is not a time or the
    /// beat or tick is past the end of its bar or beat.
    pub fn parse(&self, text: &str, rate: u32) -> Option<usize> {
        let map = self.map()?;
        let sr = SampleRate::new(rate).ok()?;
        let (bar_beat, tick) = match text.trim().split_once('.') {
            Some((b, t)) if !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit()) => (b, t.parse::<i64>().ok()?),
            Some(_) => return None,
            None => (text.trim(), 0),
        };
        let (bar, beat) = match bar_beat.split_once(':') {
            Some((a, b)) => (a, b),
            None => (bar_beat, "1"),
        };
        let digits = |s: &str| (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse::<i64>().ok()).flatten();
        let (bar, beat) = (digits(bar)?, digits(beat)?);
        let meter = map.meter_at_tick(0);
        if bar < 1 || beat < 1 || beat > i64::from(self.numerator) || tick >= meter.ticks_per_beat() {
            return None;
        }
        let want = map.tick_at_bar_beat(BarBeat { bar, beat, tick });
        let mut f = map.tick_to_samples(want, sr).max(0);
        while f > 0 && map.samples_to_ticks(f - 1, sr) >= want {
            f -= 1;
        }
        while map.samples_to_ticks(f, sr) < want {
            f += 1;
        }
        usize::try_from(f).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_and_known_positions_read_as_expected() {
        let t = Tempo::default(); // 120 bpm, 4/4: a beat is 0.5 s, a bar 2 s
        assert_eq!(t.format(0, 48_000), "1:1.000");
        assert_eq!(t.format(24_000, 48_000), "1:2.000");
        assert_eq!(t.format(96_000, 48_000), "2:1.000");
        assert_eq!(t.format(96_000 + 48_000 + 12_000, 48_000), "2:3.480");
        let waltz = Tempo::new(60.0, 3, 4).unwrap(); // a beat is 1 s, a bar 3 s
        assert_eq!(waltz.format(48_000 * 7, 48_000), "3:2.000");
        let eighths = Tempo::new(120.0, 6, 8).unwrap(); // a beat is 0.25 s
        assert_eq!(eighths.format(12_000 * 7, 48_000), "2:2.000");
    }

    #[test]
    fn parse_and_format_round_trip() {
        for t in [Tempo::default(), Tempo::new(93.5, 3, 4).unwrap(), Tempo::new(200.0, 7, 8).unwrap(), Tempo::new(47.0, 5, 4).unwrap()] {
            for rate in [8000u32, 22_050, 44_100, 48_000, 96_000] {
                for frame in [0usize, 1, 7, 441, 4410, rate as usize - 1, rate as usize, 12_345_678, 3600 * rate as usize + 17] {
                    let text = t.format(frame, rate);
                    let back = t.parse(&text, rate).unwrap_or_else(|| panic!("{text:?} did not parse"));
                    assert_eq!(t.format(back, rate), text, "{t:?} at {rate} Hz, frame {frame}");
                    assert!(back <= frame, "the parsed frame is the first with that text");
                }
            }
        }
    }

    #[test]
    fn typed_forms_and_refusals() {
        let t = Tempo::default();
        // The first frame that reads so: up to half a tick (about 10 frames here) before the exact one.
        let near = |text: &str, exact: usize| {
            let f = t.parse(text, 48_000).unwrap();
            assert!(f <= exact && exact - f <= 12, "{text}: {f} vs {exact}");
            assert_eq!(t.format(f, 48_000), t.format(exact, 48_000));
        };
        near("2:1.000", 96_000);
        near("2:1", 96_000);
        near("2", 96_000);
        near(" 1:3.480 ", 48_000 + 12_000);
        for bad in ["", "x", "0:1.000", "1:0.000", "1:5.000", "1:1.960", "1:1.", "1:1.x", "-1:1.000", "1:1:1"] {
            assert_eq!(t.parse(bad, 48_000), None, "{bad:?}");
        }
        assert_eq!(t.parse("1:1.000", 0), None);
    }

    #[test]
    fn tempo_and_meter_are_checked() {
        assert!(Tempo::new(19.9, 4, 4).is_none());
        assert!(Tempo::new(400.1, 4, 4).is_none());
        assert!(Tempo::new(f64::NAN, 4, 4).is_none());
        assert!(Tempo::new(120.0, 0, 4).is_none());
        assert!(Tempo::new(120.0, 4, 3).is_none());
        assert_eq!(Tempo::parse_meter("3/4"), Some((3, 4)));
        assert_eq!(Tempo::parse_meter("6 / 8"), Some((6, 8)));
        assert_eq!(Tempo::parse_meter("4/5"), None);
        assert_eq!(Tempo::parse_meter("44"), None);
        assert_eq!(Tempo::new(93.5, 4, 4).unwrap().bpm_text(), "93.5");
        assert_eq!(Tempo::default().bpm_text(), "120");
    }

    #[test]
    fn ruler_labels_follow_the_tempo() {
        let t = Tempo::default();
        assert_eq!(t.format_tick(2.0, 44_100), "2:1.000");
        assert_eq!(t.format_tick(0.5, 44_100), "1:2.000");
        assert!((t.bar_seconds() - 2.0).abs() < 1e-12 && (t.beat_seconds() - 0.5).abs() < 1e-12);
    }
}
