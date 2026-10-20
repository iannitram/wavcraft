//! Zoom and scroll arithmetic, selections and time formatting. All positions are in frames.

/// The closest zoom: one frame spread over this many pixels.
const MAX_PX_PER_FRAME: f64 = 16.0;

/// Which part of the file is on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The frame at the left edge.
    pub start: f64,
    /// How many frames one pixel covers (below 1 when zoomed in past single samples).
    pub frames_per_px: f64,
}

impl Viewport {
    /// The whole file across `width_px` pixels.
    pub fn fit(total_frames: usize, width_px: f32) -> Viewport {
        Viewport { start: 0.0, frames_per_px: fit_scale(total_frames, width_px) }
    }

    /// The frame just past the right edge.
    pub fn end(&self, width_px: f32) -> f64 {
        self.start + self.frames_per_px * f64::from(width_px.max(0.0))
    }

    /// The frame under pixel `x` (0 = left edge).
    pub fn frame_at(&self, x: f32) -> f64 {
        self.start + self.frames_per_px * f64::from(x)
    }

    /// The pixel where `frame` is drawn (may be off screen).
    pub fn x_of(&self, frame: f64) -> f32 {
        ((frame - self.start) / self.frames_per_px) as f32
    }

    /// Zoom by `factor` (above 1 zooms in) keeping the frame under pixel `anchor_x` where it is.
    pub fn zoom_at(&mut self, factor: f64, anchor_x: f32, width_px: f32, total_frames: usize) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.frame_at(anchor_x);
        let widest = fit_scale(total_frames, width_px);
        self.frames_per_px = (self.frames_per_px / factor).clamp((1.0 / MAX_PX_PER_FRAME).min(widest), widest);
        self.start = anchor - self.frames_per_px * f64::from(anchor_x);
        self.clamp(width_px, total_frames);
    }

    /// Scroll by `dx_px` pixels (positive moves later in the file).
    pub fn scroll_by(&mut self, dx_px: f32, width_px: f32, total_frames: usize) {
        if dx_px.is_finite() {
            self.start += self.frames_per_px * f64::from(dx_px);
        }
        self.clamp(width_px, total_frames);
    }

    /// Scroll just enough to bring `frame` on screen, leaving a small margin.
    pub fn reveal(&mut self, frame: f64, width_px: f32, total_frames: usize) {
        let margin = self.frames_per_px * f64::from(width_px) * 0.1;
        if frame < self.start {
            self.start = frame - margin;
        } else if frame > self.end(width_px) {
            self.start = frame + margin - self.frames_per_px * f64::from(width_px);
        }
        self.clamp(width_px, total_frames);
    }

    /// Keep the view inside the file and the zoom inside its limits.
    pub fn clamp(&mut self, width_px: f32, total_frames: usize) {
        let widest = fit_scale(total_frames, width_px);
        if !self.frames_per_px.is_finite() || self.frames_per_px <= 0.0 {
            self.frames_per_px = widest;
        }
        self.frames_per_px = self.frames_per_px.clamp((1.0 / MAX_PX_PER_FRAME).min(widest), widest);
        let last_start = (total_frames as f64 - self.frames_per_px * f64::from(width_px.max(1.0))).max(0.0);
        self.start = if self.start.is_finite() { self.start.clamp(0.0, last_start) } else { 0.0 };
    }
}

impl Viewport {
    /// Move the left edge to `frame`, keeping the zoom.
    pub fn set_start(&mut self, frame: usize, width_px: f32, total_frames: usize) {
        self.start = frame as f64;
        self.clamp(width_px, total_frames);
    }

    /// Move the view so its right edge is at `frame`, keeping the zoom.
    pub fn set_end(&mut self, frame: usize, width_px: f32, total_frames: usize) {
        self.start = frame as f64 - self.frames_per_px * f64::from(width_px.max(1.0));
        self.clamp(width_px, total_frames);
    }

    /// Zoom so the view covers `frames`, keeping the left edge (the zoom limits apply).
    pub fn set_duration(&mut self, frames: usize, width_px: f32, total_frames: usize) {
        self.frames_per_px = frames.max(1) as f64 / f64::from(width_px.max(1.0));
        self.clamp(width_px, total_frames);
    }
}

fn fit_scale(total_frames: usize, width_px: f32) -> f64 {
    (total_frames.max(1) as f64 / f64::from(width_px.max(1.0))).max(1.0 / MAX_PX_PER_FRAME)
}

/// A time range in frames; `anchor` is where the drag began, `head` where it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    /// A zero-length selection: just the cursor.
    pub fn at(frame: usize) -> Selection {
        Selection { anchor: frame, head: frame }
    }

    /// First selected frame.
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// One past the last selected frame.
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Number of frames selected.
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// Whether nothing is selected (the selection is only a cursor position).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

impl Selection {
    /// Type a new start. A cursor moves; a range keeps its end (and swaps if passed).
    pub fn with_start(&self, frame: usize, total: usize) -> Selection {
        let f = frame.min(total);
        if self.is_empty() { Selection::at(f) } else { Selection { anchor: f, head: self.end() } }
    }

    /// Type a new end. A cursor becomes a range from itself to `frame`; a range keeps its start.
    pub fn with_end(&self, frame: usize, total: usize) -> Selection {
        Selection { anchor: self.start(), head: frame.min(total) }
    }

    /// Type a new duration: the start stays, the end moves (kept inside the file).
    pub fn with_len(&self, frames: usize, total: usize) -> Selection {
        let a = self.start();
        Selection { anchor: a, head: a.saturating_add(frames).min(total) }
    }
}

/// How times are written (View › Time Display). Roadmap A099; other SMPTE rates are a later row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    /// `m:ss.mmm` / `h:mm:ss.mmm` ([`format_time`]).
    #[default]
    Decimal,
    /// `m:ss` with only the fraction digits that are needed (`1:02.5`, `0:03`).
    Compact,
    /// The frame (sample) number.
    Samples,
    /// `hh:mm:ss:ff` at 30 frames a second.
    Smpte30,
}

/// SMPTE frames per second (the only rate so far).
pub const SMPTE_FPS: u64 = 30;

impl TimeFormat {
    pub const ALL: [TimeFormat; 4] = [TimeFormat::Decimal, TimeFormat::Compact, TimeFormat::Samples, TimeFormat::Smpte30];

    /// The name used in the settings file, the control channel and action names.
    pub fn name(self) -> &'static str {
        match self {
            TimeFormat::Decimal => "decimal",
            TimeFormat::Compact => "compact",
            TimeFormat::Samples => "samples",
            TimeFormat::Smpte30 => "smpte30",
        }
    }

    /// The menu text.
    pub fn menu(self) -> &'static str {
        match self {
            TimeFormat::Decimal => "Decimal (mm:ss.ddd)",
            TimeFormat::Compact => "Compact",
            TimeFormat::Samples => "Samples",
            TimeFormat::Smpte30 => "SMPTE 30 fps",
        }
    }

    pub fn from_name(name: &str) -> Option<TimeFormat> {
        TimeFormat::ALL.into_iter().find(|f| f.name() == name.trim())
    }

    /// `frame` at `sample_rate` in this format.
    pub fn format(self, frame: usize, sample_rate: u32) -> String {
        match self {
            TimeFormat::Decimal => format_time(frame, sample_rate),
            TimeFormat::Compact => compact(frame, sample_rate),
            TimeFormat::Samples => frame.to_string(),
            TimeFormat::Smpte30 => {
                if sample_rate == 0 {
                    return smpte(0);
                }
                let rate = u64::from(sample_rate);
                let (secs, rest) = (frame as u64 / rate, frame as u64 % rate);
                smpte(secs * SMPTE_FPS + rest * SMPTE_FPS / rate)
            }
        }
    }

    /// A ruler tick at `seconds`, exact for that format (a tick is not rounded through a frame).
    pub fn format_tick(self, seconds: f64, sample_rate: u32) -> String {
        let seconds = seconds.max(0.0);
        match self {
            TimeFormat::Decimal => format_time((seconds * 1000.0).round() as usize, 1000),
            TimeFormat::Compact => compact((seconds * 1000.0).round() as usize, 1000),
            TimeFormat::Samples => ((seconds * f64::from(sample_rate)).round() as usize).to_string(),
            TimeFormat::Smpte30 => smpte((seconds * SMPTE_FPS as f64).round() as u64),
        }
    }

    /// The frame a typed time means, or `None` if it does not read in this format. The frame is
    /// the first one whose formatted time is the typed one, so `parse(format(f))` formats the
    /// same as `f`. Decimal and Compact take `[h:]m:ss[.fff]` or plain seconds (`12.5`); SMPTE
    /// takes `[[hh:]mm:]ss:ff` (`;` also separates the frames).
    pub fn parse(self, text: &str, sample_rate: u32) -> Option<usize> {
        let text = text.trim();
        let rate = u64::from(sample_rate);
        if text.is_empty() || rate == 0 {
            return None;
        }
        let frames = match self {
            TimeFormat::Samples => text.replace([',', '_'], "").parse::<u64>().ok()?,
            TimeFormat::Decimal | TimeFormat::Compact => {
                let (whole, frac) = match text.split_once('.') {
                    Some((w, f)) => (w, f),
                    None => (text, ""),
                };
                if !frac.bytes().all(|b| b.is_ascii_digit()) || (text.contains('.') && frac.is_empty()) {
                    return None;
                }
                let ms: u64 = format!("{frac:0<3}")[..3].parse().ok()?;
                let secs = clock(whole)?;
                // First frame at or after the instant (see the doc above).
                secs.checked_mul(rate)?.checked_add((ms * rate).div_ceil(1000))?
            }
            TimeFormat::Smpte30 => {
                let parts: Vec<&str> = text.split([':', ';']).collect();
                if parts.len() < 2 || parts.len() > 4 {
                    return None;
                }
                let (ff, head) = parts.split_last()?;
                let ff: u64 = ff.parse().ok().filter(|f| *f < SMPTE_FPS)?;
                let secs = clock(&head.join(":"))?;
                secs.checked_mul(rate)?.checked_add((ff * rate).div_ceil(SMPTE_FPS))?
            }
        };
        usize::try_from(frames).ok()
    }
}

/// `[[h:]m:]s` as seconds; a field after the first must be under 60.
fn clock(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let mut secs = 0u64;
    for (i, p) in parts.iter().enumerate() {
        let v: u64 = p.parse().ok()?;
        if i > 0 && v >= 60 {
            return None;
        }
        secs = secs.checked_mul(60)?.checked_add(v)?;
    }
    Some(secs)
}

fn smpte(total_frames: u64) -> String {
    let (ff, secs) = (total_frames % SMPTE_FPS, total_frames / SMPTE_FPS);
    format!("{:02}:{:02}:{:02}:{ff:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn compact(frame: usize, sample_rate: u32) -> String {
    let full = format_time(frame, sample_rate);
    let trimmed = full.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
}

impl TimeFormat {
    /// How many frames one step of the digit group under character `char_idx` of `text` (a time
    /// written in this format) is worth, for scrubbing by dragging (roadmap A105). Decimal and
    /// Compact: hours, minutes, seconds, then the fraction (a fraction digit group of n digits
    /// steps by 10^(3-n) ms). SMPTE: hours, minutes, seconds, frames. Samples: the one digit
    /// (steps by its power of ten). `None` when `text` has no digit there or the rate is 0. At
    /// least 1 frame.
    pub fn unit_at(self, text: &str, char_idx: usize, sample_rate: u32) -> Option<usize> {
        let rate = u64::from(sample_rate);
        let chars: Vec<char> = text.chars().collect();
        if rate == 0 || chars.is_empty() {
            return None;
        }
        let idx = char_idx.min(chars.len() - 1);
        // A separator under the pointer belongs to the group on its right, then on its left.
        let idx = if chars[idx].is_ascii_digit() {
            idx
        } else {
            (idx + 1..chars.len()).find(|i| chars[*i].is_ascii_digit()).or_else(|| (0..idx).rev().find(|i| chars[*i].is_ascii_digit()))?
        };
        if self == TimeFormat::Samples {
            let from_right = chars[idx + 1..].iter().filter(|c| c.is_ascii_digit()).count() as u32;
            return usize::try_from(10u64.checked_pow(from_right)?).ok();
        }
        let is_sep = |c: &char| matches!(c, ':' | ';' | '.');
        let group_start = (0..=idx).rev().find(|i| is_sep(&chars[*i])).map_or(0, |i| i + 1);
        let group_end = (idx..chars.len()).find(|i| is_sep(&chars[*i])).unwrap_or(chars.len());
        let has_fraction = matches!(self, TimeFormat::Decimal | TimeFormat::Compact) && chars.contains(&'.');
        let sep_before = group_start.checked_sub(1).map(|i| chars[i]);
        let seps_after = chars[group_end..].iter().filter(|c| is_sep(c)).count();
        let frames = if sep_before == Some('.') {
            let digits = group_end - group_start;
            let ms = 10u64.pow(3u32.saturating_sub(digits as u32));
            rate * ms / 1000
        } else {
            // Position among the whole-number groups, from the right (0 = the last one).
            let unit_secs = |from_right: usize| 60u64.pow(from_right.min(2) as u32);
            match self {
                TimeFormat::Smpte30 if seps_after == 0 => rate / SMPTE_FPS,
                TimeFormat::Smpte30 => rate * unit_secs(seps_after - 1),
                _ => rate * unit_secs(seps_after - usize::from(has_fraction)),
            }
        };
        usize::try_from(frames.max(1)).ok()
    }

    /// `frame` moved by `steps` of `unit` frames, kept in `0..=total`.
    pub fn scrub(frame: usize, steps: i64, unit: usize, total: usize) -> usize {
        let moved = frame as i128 + i128::from(steps) * unit as i128;
        moved.clamp(0, total as i128) as usize
    }

    /// The smallest "round" ruler step, in seconds, that is at least `min_seconds`, in this
    /// format's own units: samples in 1-2-5 steps, SMPTE frames below a second, else seconds
    /// (A107).
    pub fn tick_step(self, min_seconds: f64, sample_rate: u32) -> f64 {
        let rate = f64::from(sample_rate.max(1));
        match self {
            TimeFormat::Samples => {
                let min_frames = (min_seconds * rate).max(1.0);
                let mut decade = 1.0f64;
                loop {
                    if let Some(m) = [1.0, 2.0, 5.0].iter().map(|m| m * decade).find(|f| *f >= min_frames) {
                        return m / rate;
                    }
                    decade *= 10.0;
                }
            }
            TimeFormat::Smpte30 => {
                const FRAMES: [f64; 5] = [1.0, 2.0, 5.0, 10.0, 15.0];
                FRAMES.iter().map(|f| f / SMPTE_FPS as f64).find(|s| *s >= min_seconds).unwrap_or_else(|| seconds_step(min_seconds.max(1.0)))
            }
            TimeFormat::Decimal | TimeFormat::Compact => seconds_step(min_seconds),
        }
    }

    /// How many equal parts the fine (minor) ticks cut a labelled step of `step_seconds` into:
    /// 1 when it cannot be cut into whole units of the format (one sample, one SMPTE frame).
    pub fn subdivisions(self, step_seconds: f64, sample_rate: u32) -> u32 {
        let rate = f64::from(sample_rate.max(1));
        let near = |a: f64, b: f64| (a - b).abs() <= b.abs() * 1e-6 + 1e-12;
        // The 1-2-5 ladder below a decade: 1 -> 5 parts, 2 -> 2, 5 -> 5.
        let ladder = |units: f64| -> u32 {
            let decade = 10f64.powf(units.log10().floor());
            let m = units / decade;
            if near(m, 2.0) { 2 } else { 5 }
        };
        let whole_seconds = |s: f64| -> u32 {
            match s {
                s if s < 10.0 => ladder(s),
                s if near(s, 10.0) => 5,
                s if near(s, 30.0) => 3,
                s if near(s, 60.0) => 6,
                s if near(s, 120.0) => 4,
                s if near(s, 300.0) => 5,
                s if near(s, 600.0) => 5,
                s if near(s, 1800.0) => 6,
                _ => 6,
            }
        };
        match self {
            TimeFormat::Samples => {
                let frames = (step_seconds * rate).round();
                if frames < 2.0 { 1 } else { ladder(frames) }
            }
            TimeFormat::Smpte30 if step_seconds < 1.0 - 1e-9 => match (step_seconds * SMPTE_FPS as f64).round() as u32 {
                2 => 2,
                5 => 5,
                10 => 5,
                15 => 3,
                _ => 1,
            },
            _ => whole_seconds(step_seconds),
        }
    }
}

/// The smallest "round" number of seconds that is at least `min_seconds`.
fn seconds_step(min_seconds: f64) -> f64 {
    const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// `frame` at `sample_rate` as `m:ss.mmm`, or `h:mm:ss.mmm` from one hour up.
pub fn format_time(frame: usize, sample_rate: u32) -> String {
    if sample_rate == 0 {
        return "0:00.000".to_string();
    }
    let rate = u64::from(sample_rate);
    let frame = frame as u64;
    let ms = (frame % rate) * 1000 / rate;
    let secs = frame / rate;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{ms:03}") } else { format!("{m}:{s:02}.{ms:03}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: usize = 48_000 * 3600;

    #[test]
    fn formats_read_as_documented() {
        let r = 48_000;
        let at = 3723 * r + r / 2; // 1:02:03.500
        assert_eq!(TimeFormat::Decimal.format(at, r as u32), "1:02:03.500");
        assert_eq!(TimeFormat::Compact.format(at, r as u32), "1:02:03.5");
        assert_eq!(TimeFormat::Compact.format(3 * r, r as u32), "0:03");
        assert_eq!(TimeFormat::Compact.format(0, r as u32), "0:00");
        assert_eq!(TimeFormat::Compact.format(62 * r, r as u32), "1:02");
        assert_eq!(TimeFormat::Samples.format(at, r as u32), at.to_string());
        assert_eq!(TimeFormat::Smpte30.format(at, r as u32), "01:02:03:15");
        assert_eq!(TimeFormat::Smpte30.format(0, 0), "00:00:00:00");
        assert_eq!(TimeFormat::Smpte30.format(r - 1, r as u32), "00:00:00:29");
    }

    #[test]
    fn what_is_formatted_parses_back_to_the_same_text() {
        for rate in [8000u32, 22_050, 44_100, 48_000, 96_000] {
            for frame in [0usize, 1, 7, 441, 999, 1000, 4410, rate as usize - 1, rate as usize, 12_345_678, 3600 * rate as usize + 17] {
                for fmt in TimeFormat::ALL {
                    let text = fmt.format(frame, rate);
                    let back = fmt.parse(&text, rate).unwrap_or_else(|| panic!("{fmt:?} {text:?} did not parse"));
                    assert_eq!(fmt.format(back, rate), text, "{fmt:?} at {rate} Hz, frame {frame}");
                    assert!(back <= frame, "{fmt:?}: the parsed frame is the first with that text");
                    if fmt == TimeFormat::Samples {
                        assert_eq!(back, frame);
                    }
                }
            }
        }
    }

    #[test]
    fn typed_times_in_each_format() {
        let r = 48_000;
        assert_eq!(TimeFormat::Decimal.parse("1:02.5", r), Some(62 * 48_000 + 24_000));
        assert_eq!(TimeFormat::Decimal.parse("12.25", r), Some(12 * 48_000 + 12_000));
        assert_eq!(TimeFormat::Decimal.parse("90", r), Some(90 * 48_000));
        assert_eq!(TimeFormat::Compact.parse("1:00:00", r), Some(HOUR));
        assert_eq!(TimeFormat::Samples.parse("1,000", r), Some(1000));
        assert_eq!(TimeFormat::Smpte30.parse("00:00:01:15", r), Some(48_000 + 24_000));
        assert_eq!(TimeFormat::Smpte30.parse("1:15", r), Some(48_000 + 24_000));
        assert_eq!(TimeFormat::Smpte30.parse("00:00:01;15", r), Some(72_000));
        for bad in ["", "abc", "1:2:3:4:5", "1:75.0", "-3", "1.", ".5x", "1::2"] {
            assert_eq!(TimeFormat::Decimal.parse(bad, r), None, "{bad:?}");
        }
        assert_eq!(TimeFormat::Smpte30.parse("00:00:01:30", r), None, "frame 30 does not exist at 30 fps");
        assert_eq!(TimeFormat::Smpte30.parse("5", r), None);
        assert_eq!(TimeFormat::Samples.parse("1.5", r), None);
        assert_eq!(TimeFormat::Decimal.parse("1", 0), None);
    }

    #[test]
    fn ticks_are_exact_in_each_format() {
        assert_eq!(TimeFormat::Decimal.format_tick(3723.5, 44_100), "1:02:03.500");
        assert_eq!(TimeFormat::Compact.format_tick(0.5, 44_100), "0:00.5");
        assert_eq!(TimeFormat::Samples.format_tick(0.5, 44_100), "22050");
        assert_eq!(TimeFormat::Smpte30.format_tick(1.0 / 3.0, 44_100), "00:00:00:10");
        assert_eq!(TimeFormat::Smpte30.format_tick(2.0, 44_100), "00:00:02:00");
    }

    #[test]
    fn format_names_round_trip() {
        for f in TimeFormat::ALL {
            assert_eq!(TimeFormat::from_name(f.name()), Some(f));
        }
        assert_eq!(TimeFormat::from_name("nope"), None);
    }

    #[test]
    fn typed_selection_values_move_the_selection() {
        let s = Selection { anchor: 100, head: 400 };
        assert_eq!(s.with_start(150, 1000), Selection { anchor: 150, head: 400 });
        assert_eq!(s.with_start(900, 1000).start(), 400, "passing the end swaps");
        assert_eq!(s.with_end(2000, 1000), Selection { anchor: 100, head: 1000 });
        assert_eq!(s.with_len(50, 1000), Selection { anchor: 100, head: 150 });
        assert_eq!(s.with_len(5000, 1000).end(), 1000);
        let c = Selection::at(300);
        assert_eq!(c.with_start(10, 1000), Selection::at(10));
        assert_eq!(c.with_end(500, 1000), Selection { anchor: 300, head: 500 });
    }

    #[test]
    fn typed_view_values_move_and_zoom_the_view() {
        let mut v = Viewport::fit(HOUR, 1000.0);
        v.set_duration(48_000 * 60, 1000.0, HOUR);
        assert!((v.end(1000.0) - v.start - 48_000.0 * 60.0).abs() < 1.0);
        v.set_start(48_000 * 600, 1000.0, HOUR);
        assert_eq!(v.start, 48_000.0 * 600.0);
        v.set_end(48_000 * 1200, 1000.0, HOUR);
        assert!((v.end(1000.0) - 48_000.0 * 1200.0).abs() < 1.0);
        v.set_end(HOUR * 2, 1000.0, HOUR);
        assert!(v.end(1000.0) <= HOUR as f64 + 1.0, "kept inside the file");
    }

    #[test]
    fn digit_groups_scrub_by_their_unit() {
        let r = 48_000u32;
        // "1:02:03.500": h at 0, m at 2-3, s at 5-6, ms at 8-10.
        let t = "1:02:03.500";
        assert_eq!(TimeFormat::Decimal.unit_at(t, 0, r), Some(3600 * 48_000));
        assert_eq!(TimeFormat::Decimal.unit_at(t, 3, r), Some(60 * 48_000));
        assert_eq!(TimeFormat::Decimal.unit_at(t, 5, r), Some(48_000));
        assert_eq!(TimeFormat::Decimal.unit_at(t, 9, r), Some(48));
        assert_eq!(TimeFormat::Decimal.unit_at(t, 4, r), Some(48_000), "a separator belongs to the group on its right");
        assert_eq!(TimeFormat::Decimal.unit_at("1:02.5", 5, r), Some(4800), "one fraction digit is tenths");
        assert_eq!(TimeFormat::Decimal.unit_at("0:03", 3, r), Some(48_000), "no fraction: the last group is seconds");
        assert_eq!(TimeFormat::Compact.unit_at("0:03", 0, r), Some(60 * 48_000));
        assert_eq!(TimeFormat::Smpte30.unit_at("01:02:03:15", 10, r), Some(1600));
        assert_eq!(TimeFormat::Smpte30.unit_at("01:02:03:15", 7, r), Some(48_000));
        assert_eq!(TimeFormat::Smpte30.unit_at("01:02:03:15", 1, r), Some(3600 * 48_000));
        assert_eq!(TimeFormat::Samples.unit_at("123456", 5, r), Some(1));
        assert_eq!(TimeFormat::Samples.unit_at("123456", 2, r), Some(1000));
        assert_eq!(TimeFormat::Decimal.unit_at("", 0, r), None);
        assert_eq!(TimeFormat::Decimal.unit_at(t, 0, 0), None);
        assert_eq!(TimeFormat::Decimal.unit_at(t, 99, r), Some(48), "past the end is the last group");
    }

    #[test]
    fn scrubbing_is_clamped_to_the_file() {
        assert_eq!(TimeFormat::scrub(100, 3, 10, 1000), 130);
        assert_eq!(TimeFormat::scrub(100, -50, 10, 1000), 0);
        assert_eq!(TimeFormat::scrub(990, 5, 10, 1000), 1000);
        assert_eq!(TimeFormat::scrub(5, i64::MIN, usize::MAX, 9), 0);
    }

    #[test]
    fn tick_steps_follow_the_format() {
        assert_eq!(TimeFormat::Decimal.tick_step(0.7, 48_000), 1.0);
        assert!((TimeFormat::Samples.tick_step(0.0011, 48_000) - 100.0 / 48_000.0).abs() < 1e-12);
        assert!((TimeFormat::Smpte30.tick_step(0.02, 48_000) - 1.0 / 30.0).abs() < 1e-12);
        assert!((TimeFormat::Smpte30.tick_step(0.2, 48_000) - 10.0 / 30.0).abs() < 1e-12);
        assert_eq!(TimeFormat::Smpte30.tick_step(3.0, 48_000), 5.0);
        assert_eq!(TimeFormat::Compact.tick_step(5000.0, 48_000), 3600.0);
    }

    #[test]
    fn fine_ticks_cut_the_step_into_whole_units() {
        let r = 48_000;
        assert_eq!(TimeFormat::Decimal.subdivisions(1.0, r), 5);
        assert_eq!(TimeFormat::Decimal.subdivisions(0.002, r), 2);
        assert_eq!(TimeFormat::Decimal.subdivisions(0.5, r), 5);
        assert_eq!(TimeFormat::Decimal.subdivisions(30.0, r), 3);
        assert_eq!(TimeFormat::Decimal.subdivisions(60.0, r), 6);
        assert_eq!(TimeFormat::Samples.subdivisions(1.0 / 48_000.0, r), 1, "one sample cannot be cut");
        assert_eq!(TimeFormat::Samples.subdivisions(100.0 / 48_000.0, r), 5);
        assert_eq!(TimeFormat::Samples.subdivisions(20.0 / 48_000.0, r), 2);
        assert_eq!(TimeFormat::Smpte30.subdivisions(1.0 / 30.0, r), 1, "one frame cannot be cut");
        assert_eq!(TimeFormat::Smpte30.subdivisions(10.0 / 30.0, r), 5);
        assert_eq!(TimeFormat::Smpte30.subdivisions(15.0 / 30.0, r), 3);
        assert_eq!(TimeFormat::Smpte30.subdivisions(5.0, r), 5);
        // Every step the ruler can choose divides evenly in its own format.
        for fmt in TimeFormat::ALL {
            for k in 0..60 {
                let step = fmt.tick_step(f64::from(k) * 0.137 + 1e-4, r);
                assert!(fmt.subdivisions(step, r) >= 1, "{fmt:?} {step}");
            }
        }
    }

    #[test]
    fn fit_shows_the_whole_file() {
        let v = Viewport::fit(HOUR, 1200.0);
        assert_eq!(v.start, 0.0);
        assert!((v.end(1200.0) - HOUR as f64).abs() < 1e-3);
        assert!((v.frame_at(600.0) - HOUR as f64 / 2.0).abs() < 1e-3);
        assert!((v.x_of(HOUR as f64 / 4.0) - 300.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_the_frame_under_the_cursor() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        let under = v.frame_at(900.0);
        for _ in 0..10 {
            v.zoom_at(1.5, 900.0, 1200.0, HOUR);
            assert!((v.frame_at(900.0) - under).abs() < 1.0, "{} vs {under}", v.frame_at(900.0));
        }
        assert!(v.frames_per_px < Viewport::fit(HOUR, 1200.0).frames_per_px / 50.0);
        // All the way out again: back to the whole file, never past it.
        for _ in 0..40 {
            v.zoom_at(0.5, 900.0, 1200.0, HOUR);
        }
        assert_eq!(v, Viewport::fit(HOUR, 1200.0));
    }

    #[test]
    fn zoom_and_scroll_stop_at_the_limits() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        for _ in 0..200 {
            v.zoom_at(2.0, 0.0, 1200.0, HOUR);
        }
        assert_eq!(v.frames_per_px, 1.0 / 16.0, "closest zoom is 16 px per frame");
        v.scroll_by(-1e9, 1200.0, HOUR);
        assert_eq!(v.start, 0.0);
        v.scroll_by(1e12, 1200.0, HOUR);
        assert!((v.end(1200.0) - HOUR as f64).abs() < 1e-6, "{}", v.end(1200.0));
        // Bad numbers leave a usable view behind.
        v.zoom_at(f64::NAN, 10.0, 1200.0, HOUR);
        v.zoom_at(0.0, 10.0, 1200.0, HOUR);
        v.scroll_by(f32::NAN, 1200.0, HOUR);
        let mut broken = Viewport { start: f64::NAN, frames_per_px: -3.0 };
        broken.clamp(1200.0, HOUR);
        assert_eq!(broken, Viewport::fit(HOUR, 1200.0));
        // A file shorter than the window still fits and cannot scroll.
        let mut tiny = Viewport::fit(10, 1200.0);
        tiny.scroll_by(500.0, 1200.0, 10);
        assert_eq!(tiny.start, 0.0);
    }

    #[test]
    fn reveal_scrolls_only_when_the_frame_is_off_screen() {
        let mut v = Viewport { start: 100_000.0, frames_per_px: 100.0 };
        let before = v;
        v.reveal(150_000.0, 1000.0, HOUR);
        assert_eq!(v, before, "already visible");
        v.reveal(50_000.0, 1000.0, HOUR);
        assert!(v.start < 50_000.0 && v.end(1000.0) > 50_000.0);
        v.reveal(900_000.0, 1000.0, HOUR);
        assert!(v.start < 900_000.0 && v.end(1000.0) > 900_000.0);
    }

    #[test]
    fn selections_are_ordered_whichever_way_they_were_dragged() {
        let s = Selection { anchor: 900, head: 100 };
        assert_eq!((s.start(), s.end(), s.len(), s.is_empty()), (100, 900, 800, false));
        assert!(Selection::at(42).is_empty());
    }

    #[test]
    fn times_read_like_a_clock() {
        assert_eq!(format_time(0, 48_000), "0:00.000");
        assert_eq!(format_time(48_000 * 61 + 24_000, 48_000), "1:01.500");
        assert_eq!(format_time(HOUR + 48_000 * 125 + 48, 48_000), "1:02:05.001");
        assert_eq!(format_time(44_099, 44_100), "0:00.999");
        assert_eq!(format_time(5, 0), "0:00.000");
    }
}
