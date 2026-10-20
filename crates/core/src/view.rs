//! Zoom and scroll arithmetic, selections and time formatting. All positions are in frames.

use crate::smpte::SmpteRate;

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

/// How times are written (View › Time Display). Roadmap A099 and A098.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    /// `m:ss.mmm` / `h:mm:ss.mmm` ([`format_time`]).
    #[default]
    Decimal,
    /// `m:ss` with only the fraction digits that are needed (`1:02.5`, `0:03`).
    Compact,
    /// The frame (sample) number.
    Samples,
    /// SMPTE `hh:mm:ss:ff` at 24 frames a second.
    Smpte24,
    /// SMPTE at 25 frames a second.
    Smpte25,
    /// SMPTE at 29.97 frames a second, drop-frame (`hh:mm:ss;ff`).
    Smpte2997Df,
    /// SMPTE at 29.97 frames a second, non-drop.
    Smpte2997Ndf,
    /// SMPTE at 30 frames a second.
    Smpte30,
    /// SMPTE at 59.94 frames a second.
    Smpte5994,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 9] = [
        TimeFormat::Decimal,
        TimeFormat::Compact,
        TimeFormat::Samples,
        TimeFormat::Smpte24,
        TimeFormat::Smpte25,
        TimeFormat::Smpte2997Df,
        TimeFormat::Smpte2997Ndf,
        TimeFormat::Smpte30,
        TimeFormat::Smpte5994,
    ];

    /// The SMPTE rate, for the SMPTE formats.
    pub fn smpte(self) -> Option<SmpteRate> {
        match self {
            TimeFormat::Smpte24 => Some(SmpteRate::Fps24),
            TimeFormat::Smpte25 => Some(SmpteRate::Fps25),
            TimeFormat::Smpte2997Df => Some(SmpteRate::Fps2997Df),
            TimeFormat::Smpte2997Ndf => Some(SmpteRate::Fps2997Ndf),
            TimeFormat::Smpte30 => Some(SmpteRate::Fps30),
            TimeFormat::Smpte5994 => Some(SmpteRate::Fps5994),
            _ => None,
        }
    }

    /// Snap `frame` to the nearest frame boundary of this format (A128): only the SMPTE formats
    /// have frames; any other format returns `frame` unchanged. Kept at or below `total`.
    /// The snapping switch (`snap.rs`, PR #95) calls this when it is on and the display is SMPTE.
    pub fn snap_to_frame(self, frame: usize, sample_rate: u32, total: usize) -> usize {
        self.smpte().map_or(frame, |r| r.snap(frame, sample_rate, total))
    }

    /// The name used in the settings file, the control channel and action names.
    pub fn name(self) -> &'static str {
        match self {
            TimeFormat::Decimal => "decimal",
            TimeFormat::Compact => "compact",
            TimeFormat::Samples => "samples",
            TimeFormat::Smpte24 => "smpte_24",
            TimeFormat::Smpte25 => "smpte_25",
            TimeFormat::Smpte2997Df => "smpte_2997df",
            TimeFormat::Smpte2997Ndf => "smpte_2997ndf",
            TimeFormat::Smpte30 => "smpte_30",
            TimeFormat::Smpte5994 => "smpte_5994",
        }
    }

    /// The menu text.
    pub fn menu(self) -> &'static str {
        match self {
            TimeFormat::Decimal => "Decimal (mm:ss.ddd)",
            TimeFormat::Compact => "Compact",
            TimeFormat::Samples => "Samples",
            TimeFormat::Smpte24 => "SMPTE 24 fps",
            TimeFormat::Smpte25 => "SMPTE 25 fps",
            TimeFormat::Smpte2997Df => "SMPTE 29.97 fps drop",
            TimeFormat::Smpte2997Ndf => "SMPTE 29.97 fps non-drop",
            TimeFormat::Smpte30 => "SMPTE 30 fps",
            TimeFormat::Smpte5994 => "SMPTE 59.94 fps",
        }
    }

    pub fn from_name(name: &str) -> Option<TimeFormat> {
        let name = name.trim();
        // "smpte30" is how the 30 fps format was written before the other rates came.
        let name = if name == "smpte30" { "smpte_30" } else { name };
        TimeFormat::ALL.into_iter().find(|f| f.name() == name)
    }

    /// `frame` at `sample_rate` in this format.
    pub fn format(self, frame: usize, sample_rate: u32) -> String {
        match self {
            TimeFormat::Decimal => format_time(frame, sample_rate),
            TimeFormat::Compact => compact(frame, sample_rate),
            TimeFormat::Samples => frame.to_string(),
            _ => self.smpte().map_or_else(String::new, |r| r.format(frame, sample_rate)),
        }
    }

    /// A ruler tick at `seconds`, exact for that format (a tick is not rounded through a frame).
    pub fn format_tick(self, seconds: f64, sample_rate: u32) -> String {
        let seconds = seconds.max(0.0);
        match self {
            TimeFormat::Decimal => format_time((seconds * 1000.0).round() as usize, 1000),
            TimeFormat::Compact => compact((seconds * 1000.0).round() as usize, 1000),
            TimeFormat::Samples => ((seconds * f64::from(sample_rate)).round() as usize).to_string(),
            _ => self.smpte().map_or_else(String::new, |r| r.format_seconds(seconds)),
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
            _ => return self.smpte()?.parse(text, sample_rate),
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

fn compact(frame: usize, sample_rate: u32) -> String {
    let full = format_time(frame, sample_rate);
    let trimmed = full.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
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
    fn the_smpte_formats_read_and_snap() {
        let r = 48_000;
        assert_eq!(TimeFormat::Smpte2997Df.format(1800 * 1001 * 48 / 30, r), "00:01:00;02");
        assert_eq!(TimeFormat::Smpte2997Df.parse("00:00:59;29", r).map(|f| TimeFormat::Smpte2997Df.format(f, r)), Some("00:00:59;29".into()));
        assert_eq!(TimeFormat::Smpte25.snap_to_frame(2900, r, 1_000_000), 3840);
        assert_eq!(TimeFormat::Decimal.snap_to_frame(2900, r, 1_000_000), 2900, "only SMPTE has frames");
        assert_eq!(TimeFormat::from_name("smpte30"), Some(TimeFormat::Smpte30), "the old settings spelling");
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
