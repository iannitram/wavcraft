//! SMPTE timecode (roadmap A098) and frame snapping (A128): `hh:mm:ss:ff` at 24, 25, 29.97 (drop
//! and non-drop), 30 and 59.94 frames a second, from a sample position and back.
//!
//! The model: a *timecode frame* is numbered from 0 at the start of the file and lasts
//! `den / num` seconds (`num / den` is the true frame rate: 30000/1001 for 29.97). Sample frame `s`
//! at `rate` Hz lies in timecode frame `floor(s * num / (den * rate))`, and that timecode frame
//! begins at the first sample frame `ceil(n * den * rate / num)`. Everything is integer maths
//! (`u128` inside), so a long file does not drift.
//!
//! The label of timecode frame `n` counts `nominal` frames to the labelled second (24, 25, 30, 60).
//! At 29.97 the true rate is a little under 30, so non-drop timecode slowly runs behind the clock;
//! drop-frame fixes the labels (never the pictures): it skips the numbers 0 and 1 at the start of
//! every minute except each tenth minute, which saves 2 x 9 = 18 numbers in 10 minutes
//! (10 x 60 x 30 - 18 = 17,982 timecode frames, the true count of a ten-minute span at 29.97).
//! 59.94 is non-drop here.

/// A SMPTE frame rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmpteRate {
    Fps24,
    Fps25,
    Fps2997Df,
    Fps2997Ndf,
    Fps30,
    Fps5994,
    /// A user-set whole rate, [`CUSTOM_MIN_FPS`] to [`CUSTOM_MAX_FPS`] (A102); non-drop.
    Custom(u16),
    /// Compact Disc: 75 frames (sectors) a second, written `mm:ss:ff` with minutes past 59 kept (A103).
    Cd75,
}

/// The lowest user-set frame rate.
pub const CUSTOM_MIN_FPS: u16 = 2;
/// The highest user-set frame rate.
pub const CUSTOM_MAX_FPS: u16 = 1000;
/// The custom rate before a person sets one.
pub const CUSTOM_DEFAULT_FPS: u16 = 48;

/// Timecode frames in ten minutes of 29.97 drop-frame.
const FRAMES_PER_TEN_MINUTES: u64 = 17_982;
/// Timecode frames in a minute that drops two numbers.
const FRAMES_PER_DROP_MINUTE: u64 = 1_798;

impl SmpteRate {
    pub const ALL: [SmpteRate; 6] =
        [SmpteRate::Fps24, SmpteRate::Fps25, SmpteRate::Fps2997Df, SmpteRate::Fps2997Ndf, SmpteRate::Fps30, SmpteRate::Fps5994];

    /// A custom rate, or `None` outside 2 to 1000.
    pub fn custom(fps: u32) -> Option<SmpteRate> {
        u16::try_from(fps).ok().filter(|f| (CUSTOM_MIN_FPS..=CUSTOM_MAX_FPS).contains(f)).map(SmpteRate::Custom)
    }

    /// Frames counted to a labelled second.
    pub fn nominal(self) -> u64 {
        match self {
            SmpteRate::Custom(n) => u64::from(n.clamp(CUSTOM_MIN_FPS, CUSTOM_MAX_FPS)),
            SmpteRate::Cd75 => 75,
            SmpteRate::Fps24 => 24,
            SmpteRate::Fps25 => 25,
            SmpteRate::Fps2997Df | SmpteRate::Fps2997Ndf | SmpteRate::Fps30 => 30,
            SmpteRate::Fps5994 => 60,
        }
    }

    /// The true rate as (numerator, denominator) frames per second.
    pub fn ratio(self) -> (u64, u64) {
        match self {
            SmpteRate::Custom(_) | SmpteRate::Cd75 => (self.nominal(), 1),
            SmpteRate::Fps24 => (24, 1),
            SmpteRate::Fps25 => (25, 1),
            SmpteRate::Fps2997Df | SmpteRate::Fps2997Ndf => (30_000, 1001),
            SmpteRate::Fps30 => (30, 1),
            SmpteRate::Fps5994 => (60_000, 1001),
        }
    }

    pub fn is_drop(self) -> bool {
        self == SmpteRate::Fps2997Df
    }

    /// Seconds in one timecode frame.
    pub fn frame_seconds(self) -> f64 {
        let (n, d) = self.ratio();
        d as f64 / n as f64
    }

    /// The timecode frame holding sample frame `sample` at `rate` Hz (0 for a rate of 0).
    pub fn frame_number(self, sample: usize, rate: u32) -> u64 {
        if rate == 0 {
            return 0;
        }
        let (num, den) = self.ratio();
        (sample as u128 * num as u128 / (den as u128 * u128::from(rate))) as u64
    }

    /// The first sample frame of timecode frame `n` at `rate` Hz.
    pub fn first_sample(self, n: u64, rate: u32) -> u64 {
        let (num, den) = self.ratio();
        let top = n as u128 * den as u128 * u128::from(rate);
        top.div_ceil(num as u128).min(u128::from(u64::MAX)) as u64
    }

    /// The label of timecode frame `n`: (hours, minutes, seconds, frames).
    pub fn label(self, n: u64) -> (u64, u64, u64, u64) {
        let fps = self.nominal();
        let n = if self.is_drop() {
            // Put back the numbers the labels skipped, then count at a plain 30.
            let (tens, rest) = (n / FRAMES_PER_TEN_MINUTES, n % FRAMES_PER_TEN_MINUTES);
            n + 18 * tens + if rest >= 2 { 2 * ((rest - 2) / FRAMES_PER_DROP_MINUTE) } else { 0 }
        } else {
            n
        };
        let secs = n / fps;
        if self == SmpteRate::Cd75 {
            // No hours: minutes run on past 59.
            return (0, secs / 60, secs % 60, n % fps);
        }
        (secs / 3600, secs / 60 % 60, secs % 60, n % fps)
    }

    /// The timecode frame a label names, or `None` if the label does not exist (a frame number
    /// past the rate, a minute or second of 60 or more, or a number drop-frame skips).
    pub fn frame_of_label(self, h: u64, m: u64, s: u64, f: u64) -> Option<u64> {
        let fps = self.nominal();
        let cd = self == SmpteRate::Cd75;
        if (cd && h != 0) || (!cd && m >= 60) || s >= 60 || f >= fps || (self.is_drop() && s == 0 && f < 2 && !m.is_multiple_of(10)) {
            return None;
        }
        let minutes = h.checked_mul(60)?.checked_add(m)?;
        let labelled = minutes.checked_mul(60)?.checked_add(s)?.checked_mul(fps)?.checked_add(f)?;
        let skipped = if self.is_drop() { 2 * (minutes - minutes / 10) } else { 0 };
        Some(labelled - skipped)
    }

    /// `hh:mm:ss:ff`; drop-frame writes `;` before the frames.
    pub fn format_frame(self, n: u64) -> String {
        let (h, m, s, f) = self.label(n);
        if self == SmpteRate::Cd75 {
            return format!("{m:02}:{s:02}:{f:02}");
        }
        // Frame numbers past 99 (rates over 100) take three digits.
        let w = if self.nominal() > 100 { 3 } else { 2 };
        let sep = if self.is_drop() { ';' } else { ':' };
        format!("{h:02}:{m:02}:{s:02}{sep}{f:0w$}")
    }

    /// Sample frame `sample` at `rate` Hz as a timecode.
    pub fn format(self, sample: usize, rate: u32) -> String {
        self.format_frame(self.frame_number(sample, rate))
    }

    /// The timecode of the frame nearest `seconds` (for ruler ticks).
    pub fn format_seconds(self, seconds: f64) -> String {
        let (num, den) = self.ratio();
        self.format_frame((seconds.max(0.0) * num as f64 / den as f64).round() as u64)
    }

    /// The sample frame a typed timecode means (the first sample of its timecode frame), or `None`
    /// if it does not read. Takes `[[[hh:]mm:]ss:]ff`-style text: two to four fields, `:` or `;`
    /// between them, so `1:15` is one second and fifteen frames.
    pub fn parse(self, text: &str, rate: u32) -> Option<usize> {
        let text = text.trim();
        if text.is_empty() || rate == 0 {
            return None;
        }
        let fields: Vec<&str> = text.split([':', ';']).collect();
        if fields.len() < 2 || fields.len() > 4 || fields.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        let mut v = [0u64; 4];
        for (slot, field) in v[4 - fields.len()..].iter_mut().zip(&fields) {
            *slot = field.parse().ok()?;
        }
        let n = self.frame_of_label(v[0], v[1], v[2], v[3])?;
        usize::try_from(self.first_sample(n, rate)).ok()
    }

    /// Snap `sample` to the nearest timecode frame boundary at `rate` Hz (a tie goes to the later
    /// one), kept at or below `total`.
    pub fn snap(self, sample: usize, rate: u32, total: usize) -> usize {
        if rate == 0 {
            return sample;
        }
        let n = self.frame_number(sample, rate);
        let (lo, hi) = (self.first_sample(n, rate), self.first_sample(n + 1, rate));
        let s = sample as u64;
        let pick = if hi.saturating_sub(s) <= s.saturating_sub(lo) { hi } else { lo };
        usize::try_from(pick).unwrap_or(sample).min(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: u32 = 48_000;

    #[test]
    fn labels_at_whole_rates() {
        // 1 h 2 min 3 s and 15 frames at each whole rate is that many timecode frames in.
        for (rate, fps) in [(SmpteRate::Fps24, 24u64), (SmpteRate::Fps25, 25), (SmpteRate::Fps30, 30), (SmpteRate::Fps5994, 60)] {
            let n = (3723 * fps) + 15;
            assert_eq!(rate.label(n), (1, 2, 3, 15), "{rate:?}");
            assert_eq!(rate.frame_of_label(1, 2, 3, 15), Some(n));
        }
        assert_eq!(SmpteRate::Fps24.format_frame(24), "00:00:01:00");
        assert_eq!(SmpteRate::Fps25.format_frame(24), "00:00:00:24");
        assert_eq!(SmpteRate::Fps25.frame_of_label(0, 0, 0, 25), None, "frame 25 does not exist at 25 fps");
        assert_eq!(SmpteRate::Fps5994.frame_of_label(0, 0, 0, 59).map(|n| SmpteRate::Fps5994.format_frame(n)), Some("00:00:00:59".into()));
    }

    #[test]
    fn drop_frame_skips_two_numbers_each_minute_but_the_tenth() {
        let df = SmpteRate::Fps2997Df;
        // The last frame of the first minute, then the first of the next, which is numbered 02.
        assert_eq!(df.format_frame(1799), "00:00:59;29");
        assert_eq!(df.format_frame(1800), "00:01:00;02");
        assert_eq!(df.format_frame(1801), "00:01:00;03");
        // The second minute drops two as well; the tenth minute does not.
        assert_eq!(df.format_frame(1800 + 1798 - 1), "00:01:59;29");
        assert_eq!(df.format_frame(1800 + 1798), "00:02:00;02");
        assert_eq!(df.format_frame(17_981), "00:09:59;29");
        assert_eq!(df.format_frame(17_982), "00:10:00;00");
        assert_eq!(df.format_frame(17_983), "00:10:00;01");
        assert_eq!(df.format_frame(17_982 + 1800), "00:11:00;02");
        // An hour of drop-frame is 107,892 timecode frames (6 x 17,982) and starts a new hour.
        assert_eq!(df.format_frame(107_892), "01:00:00;00");
        // Non-drop does not skip.
        assert_eq!(SmpteRate::Fps2997Ndf.format_frame(1800), "00:01:00:00");
        assert_eq!(SmpteRate::Fps2997Ndf.format_frame(1799), "00:00:59:29");
    }

    #[test]
    fn drop_frame_labels_that_do_not_exist_are_refused() {
        let df = SmpteRate::Fps2997Df;
        assert_eq!(df.frame_of_label(0, 1, 0, 0), None);
        assert_eq!(df.frame_of_label(0, 1, 0, 1), None);
        assert_eq!(df.frame_of_label(0, 1, 0, 2), Some(1800));
        assert_eq!(df.frame_of_label(0, 10, 0, 0), Some(17_982), "the tenth minute keeps its 0 and 1");
        assert_eq!(df.frame_of_label(0, 1, 1, 0), Some(1828), "only the first second of the minute drops");
        assert_eq!(df.frame_of_label(0, 60, 0, 0), None);
        assert_eq!(df.frame_of_label(0, 0, 60, 0), None);
    }

    #[test]
    fn every_timecode_frame_round_trips_through_its_label() {
        for rate in SmpteRate::ALL {
            // Boundaries of the drop minutes and the tenths matter most; sweep around them and a
            // stretch past an hour.
            let mut ns: Vec<u64> = (0..4000).collect();
            ns.extend(17_900..18_100);
            ns.extend(107_800..108_000);
            ns.extend(215_700..215_900);
            for n in ns {
                let (h, m, s, f) = rate.label(n);
                assert_eq!(rate.frame_of_label(h, m, s, f), Some(n), "{rate:?} frame {n} = {h}:{m}:{s}:{f}");
            }
        }
        // And no label repeats.
        let df = SmpteRate::Fps2997Df;
        let mut seen = std::collections::HashSet::new();
        for n in 0..200_000 {
            assert!(seen.insert(df.label(n)), "label repeated at {n}");
        }
    }

    #[test]
    fn parse_and_format_round_trip_in_samples() {
        for rate_hz in [8000u32, 22_050, 44_100, 48_000, 96_000] {
            for fmt in SmpteRate::ALL {
                for sample in [
                    0usize,
                    1,
                    441,
                    999,
                    rate_hz as usize - 1,
                    rate_hz as usize,
                    12_345_678,
                    60 * rate_hz as usize - 1,
                    60 * rate_hz as usize,
                    600 * rate_hz as usize + 7,
                    3600 * rate_hz as usize + 17,
                ] {
                    let text = fmt.format(sample, rate_hz);
                    let back = fmt.parse(&text, rate_hz).unwrap_or_else(|| panic!("{fmt:?} {text:?} did not parse"));
                    assert_eq!(fmt.format(back, rate_hz), text, "{fmt:?} at {rate_hz} Hz, sample {sample}");
                    assert!(back <= sample, "the parsed sample is the first of its frame");
                    assert_eq!(back, fmt.first_sample(fmt.frame_number(sample, rate_hz), rate_hz) as usize);
                }
            }
        }
    }

    #[test]
    fn samples_land_in_the_right_frame_at_29_97() {
        let ndf = SmpteRate::Fps2997Ndf;
        // One real second at 48 kHz is 29.97 frames: still frame 29 (the 30th) of second 0.
        assert_eq!(ndf.format(48_000, R), "00:00:00:29");
        // The 30th frame begins at 30 x 1001 / 30000 s = 1.001 s = 48,048 samples.
        assert_eq!(ndf.first_sample(30, R), 48_048);
        assert_eq!(ndf.format(48_047, R), "00:00:00:29");
        assert_eq!(ndf.format(48_048, R), "00:00:01:00");
        // Drop-frame stays within a frame of the clock over ten minutes: 17,982 frames last
        // 17,982 x 1001 / 30000 s = 599.999... s, about 600 s.
        let ten_min = SmpteRate::Fps2997Df.first_sample(17_982, R);
        assert!(ten_min.abs_diff(600 * u64::from(R)) < 48_000 / 29, "{ten_min}");
        assert_eq!(SmpteRate::Fps2997Df.format(ten_min as usize, R), "00:10:00;00");
        // Whole rates are exact.
        assert_eq!(SmpteRate::Fps25.first_sample(25, R), 48_000);
        assert_eq!(SmpteRate::Fps24.format(48_000 + 2_000, R), "00:00:01:01");
    }

    #[test]
    fn typed_timecodes() {
        let df = SmpteRate::Fps2997Df;
        assert_eq!(df.parse("00:01:00;02", R), Some(df.first_sample(1800, R) as usize));
        assert_eq!(df.parse("00:01:00:02", R), df.parse("00:01:00;02", R), "either separator reads");
        assert_eq!(df.parse("00:01:00;00", R), None, "dropped number");
        assert_eq!(df.parse("1:00;02", R), df.parse("00:01:00;02", R), "leading fields may be left off");
        assert_eq!(SmpteRate::Fps30.parse("1:15", R), Some(48_000 + 24_000));
        assert_eq!(SmpteRate::Fps25.parse("0:0:1:25", R), None);
        for bad in ["", "5", "a:b", "1:2:3:4:5", "00:00:00:-1", "00:00::01", "1.5:00"] {
            assert_eq!(SmpteRate::Fps30.parse(bad, R), None, "{bad:?}");
        }
        assert_eq!(SmpteRate::Fps30.parse("00:00:01:00", 0), None);
    }

    #[test]
    fn snapping_goes_to_the_nearest_frame_boundary() {
        let f25 = SmpteRate::Fps25; // 1,920 samples a frame at 48 kHz
        assert_eq!(f25.snap(0, R, 10 * 48_000), 0);
        assert_eq!(f25.snap(959, R, 10 * 48_000), 0);
        assert_eq!(f25.snap(960, R, 10 * 48_000), 1920, "a tie goes later");
        assert_eq!(f25.snap(1920, R, 10 * 48_000), 1920);
        assert_eq!(f25.snap(1921 + 800, R, 10 * 48_000), 1920);
        assert_eq!(f25.snap(479_999, R, 480_000), 480_000);
        assert_eq!(f25.snap(479_999, R, 479_990), 479_990, "never past the end");
        // At 29.97 the boundaries are not whole numbers of samples; the snap lands on a first sample.
        let ndf = SmpteRate::Fps2997Ndf;
        for s in [1usize, 777, 5000, 48_000, 1_234_567] {
            let snapped = ndf.snap(s, R, usize::MAX);
            let n = ndf.frame_number(snapped, R);
            assert_eq!(snapped as u64, ndf.first_sample(n, R), "{s} -> {snapped}");
            assert!(snapped.abs_diff(s) <= 800, "within half a frame (800.8 samples)");
            assert_eq!(ndf.snap(snapped, R, usize::MAX), snapped, "snapping twice changes nothing");
        }
        assert_eq!(f25.snap(777, 0, 100), 777, "no rate: nothing to snap to");
    }

    #[test]
    fn custom_rates_are_limited_to_2_through_1000() {
        assert_eq!(SmpteRate::custom(1), None);
        assert_eq!(SmpteRate::custom(0), None);
        assert_eq!(SmpteRate::custom(1001), None);
        assert_eq!(SmpteRate::custom(u32::MAX), None);
        assert_eq!(SmpteRate::custom(2), Some(SmpteRate::Custom(2)));
        assert_eq!(SmpteRate::custom(1000), Some(SmpteRate::Custom(1000)));
    }

    #[test]
    fn custom_rates_label_like_whole_rate_timecode() {
        let c2 = SmpteRate::Custom(2);
        assert_eq!(c2.format_frame(0), "00:00:00:00");
        assert_eq!(c2.format_frame(1), "00:00:00:01");
        assert_eq!(c2.format_frame(2), "00:00:01:00");
        assert_eq!(c2.frame_of_label(0, 0, 0, 2), None, "frame 2 does not exist at 2 fps");
        let c48 = SmpteRate::Custom(48);
        assert_eq!(c48.format_frame(48 * 3723 + 47), "01:02:03:47");
        assert_eq!(c48.parse("01:02:03:47", R), Some((48 * 3723 + 47) * 1000));
        let c1000 = SmpteRate::Custom(1000);
        assert_eq!(c1000.format_frame(999), "00:00:00:999", "three digits of frames");
        assert_eq!(c1000.format_frame(1000), "00:00:01:000");
        assert_eq!(c1000.frame_of_label(0, 0, 0, 1000), None);
        assert_eq!(c1000.frame_of_label(0, 0, 0, 999), Some(999));
        assert_eq!(c1000.parse("00:00:01:000", R), Some(R as usize));
        // 100 fps still reads two digits.
        assert_eq!(SmpteRate::Custom(100).format_frame(99), "00:00:00:99");
        assert_eq!(SmpteRate::Custom(101).format_frame(100), "00:00:00:100");
    }

    #[test]
    fn custom_and_cd_round_trip_at_the_boundaries() {
        for fmt in [
            SmpteRate::Custom(2),
            SmpteRate::Custom(7),
            SmpteRate::Custom(100),
            SmpteRate::Custom(101),
            SmpteRate::Custom(999),
            SmpteRate::Custom(1000),
            SmpteRate::Cd75,
        ] {
            for rate_hz in [8000u32, 44_100, 48_000, 96_000] {
                for sample in [
                    0usize,
                    1,
                    44,
                    440,
                    rate_hz as usize - 1,
                    rate_hz as usize,
                    59 * rate_hz as usize,
                    60 * rate_hz as usize - 1,
                    60 * rate_hz as usize,
                    3600 * rate_hz as usize - 1,
                    3600 * rate_hz as usize,
                    3600 * rate_hz as usize + 17,
                    100 * 60 * rate_hz as usize + 3,
                ] {
                    let text = fmt.format(sample, rate_hz);
                    let back = fmt.parse(&text, rate_hz).unwrap_or_else(|| panic!("{fmt:?} {text:?} did not parse"));
                    assert_eq!(fmt.format(back, rate_hz), text, "{fmt:?} at {rate_hz} Hz, sample {sample}");
                    assert_eq!(back as u64, fmt.first_sample(fmt.frame_number(sample, rate_hz), rate_hz));
                    assert!(back <= sample);
                }
            }
            for n in (0..300).chain(tens_of_thousands()) {
                let (h, m, s, f) = fmt.label(n);
                assert_eq!(fmt.frame_of_label(h, m, s, f), Some(n), "{fmt:?} frame {n}");
            }
        }
    }

    fn tens_of_thousands() -> impl Iterator<Item = u64> {
        (4_499..4_501).chain(269_990..270_010).chain(4_499_990..4_500_010)
    }

    #[test]
    fn compact_disc_is_mm_ss_ff_at_75_frames() {
        let cd = SmpteRate::Cd75;
        assert_eq!(cd.ratio(), (75, 1));
        assert_eq!(cd.format_frame(0), "00:00:00");
        assert_eq!(cd.format_frame(74), "00:00:74");
        assert_eq!(cd.format_frame(75), "00:01:00");
        assert_eq!(cd.format_frame(75 * 60 - 1), "00:59:74");
        assert_eq!(cd.format_frame(75 * 60), "01:00:00");
        // No hours: minutes go on past 59 (an 80-minute disc is 360,000 frames).
        assert_eq!(cd.format_frame(75 * 3600), "60:00:00");
        assert_eq!(cd.format_frame(75 * 60 * 80 - 1), "79:59:74");
        assert_eq!(cd.format_frame(75 * 60 * 100), "100:00:00");
        assert_eq!(cd.frame_of_label(0, 0, 0, 75), None, "frame 75 does not exist");
        assert_eq!(cd.frame_of_label(0, 0, 60, 0), None, "second 60 does not exist");
        assert_eq!(cd.frame_of_label(0, 79, 59, 74), Some(75 * 60 * 80 - 1));
        // At 44.1 kHz a CD frame (sector) is exactly 588 samples.
        assert_eq!(cd.first_sample(1, 44_100), 588);
        assert_eq!(cd.format(588 * 75 * 60 - 1, 44_100), "00:59:74");
        assert_eq!(cd.format(588 * 75 * 60 + 587, 44_100), "01:00:00");
        assert_eq!(cd.parse("01:00:00", 44_100), Some(588 * 75 * 60));
        assert_eq!(cd.parse("80:00:00", 44_100), Some(588 * 75 * 60 * 80));
        assert_eq!(cd.parse("1:30", 44_100), Some(588 * 75 + 588 * 30), "ss:ff");
        assert_eq!(cd.parse("00:01:30", 44_100), Some(588 * 75 + 588 * 30));
        assert_eq!(cd.parse("74", 44_100), None, "one field is not a time");
        assert_eq!(cd.parse("01:75", 44_100), None);
        assert_eq!(cd.parse("1:00:00:00", 44_100), None, "no hours field");
        assert_eq!(cd.parse("60:00:00", 44_100), Some(588 * 75 * 3600));
    }

    #[test]
    fn snapping_works_with_custom_and_cd() {
        // 100 fps at 48 kHz: 480 samples a frame.
        let c = SmpteRate::Custom(100);
        assert_eq!(c.snap(239, R, usize::MAX), 0);
        assert_eq!(c.snap(240, R, usize::MAX), 480);
        assert_eq!(c.snap(481, R, usize::MAX), 480);
        // CD at 44.1 kHz: 588 samples a frame.
        let cd = SmpteRate::Cd75;
        assert_eq!(cd.snap(293, 44_100, usize::MAX), 0);
        assert_eq!(cd.snap(294, 44_100, usize::MAX), 588);
        assert_eq!(cd.snap(588 * 5 + 100, 44_100, 588 * 5 + 100), 588 * 5);
        assert_eq!(cd.snap(588 * 5 + 400, 44_100, 588 * 5 + 400), 588 * 5 + 400, "never past the end");
    }

    #[test]
    fn ticks_name_the_nearest_frame() {
        assert_eq!(SmpteRate::Fps25.format_seconds(1.0), "00:00:01:00");
        assert_eq!(SmpteRate::Fps24.format_seconds(0.5), "00:00:00:12");
        // 60 s is timecode frame 1,798.2: frame 1,798 is still in the first minute.
        assert_eq!(SmpteRate::Fps2997Df.format_seconds(60.0), "00:00:59;28");
        assert_eq!(SmpteRate::Fps2997Df.format_seconds(60.07), "00:01:00;02");
    }
}
