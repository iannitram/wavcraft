//! Zoom and scroll arithmetic, selections and time formatting. All positions are in frames.

use crate::smpte::{CUSTOM_DEFAULT_FPS, SmpteRate};
use crate::tempo::Tempo;
use std::ops::Range;

use crate::doc::edit::ChannelMask;

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

    /// The Zoom tool's click: zoom in (or out, with `out`) by 2x around `frame`, which stays
    /// where it is on screen.
    pub fn zoom_click(&mut self, frame: f64, out: bool, width_px: f32, total_frames: usize) {
        let anchor_x = self.x_of(frame).clamp(0.0, width_px.max(0.0));
        let factor = if out { 0.5 } else { 2.0 };
        self.zoom_at(factor, anchor_x, width_px, total_frames);
    }

    /// Fill the view with the range between frames `a` and `b` (either order), as the Zoom
    /// tool's drag and a right-drag in the ruler do. A range too small to show at the closest
    /// zoom is centred at that zoom; an empty or non-finite range changes nothing.
    pub fn zoom_to_range(&mut self, a: f64, b: f64, width_px: f32, total_frames: usize) {
        if !a.is_finite() || !b.is_finite() || a == b {
            return;
        }
        let (lo, hi) = (a.min(b).max(0.0), a.max(b).min(total_frames as f64));
        if hi <= lo {
            return;
        }
        let width = f64::from(width_px.max(1.0));
        let closest = 1.0 / MAX_PX_PER_FRAME;
        let scale = ((hi - lo) / width).max(closest);
        self.frames_per_px = scale;
        self.start = (lo + hi) * 0.5 - scale * width * 0.5;
        self.clamp(width_px, total_frames);
    }

    /// The Hand tool and middle-button drag: the picture follows the pointer, so dragging right
    /// by `dx_px` shows earlier audio.
    pub fn drag_by(&mut self, dx_px: f32, width_px: f32, total_frames: usize) {
        self.scroll_by(-dx_px, width_px, total_frames);
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

/// Which end of a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// How close (in pixels) the pointer must be to a selection edge to grab it.
pub const GRAB_PX: f32 = 5.0;

impl Selection {
    /// The whole file `0..total_frames`.
    pub fn all(total_frames: usize) -> Selection {
        Selection { anchor: 0, head: total_frames }
    }

    /// What is on screen in `view` (`width_px` wide), clamped to the file.
    pub fn visible(view: &Viewport, width_px: f32, total_frames: usize) -> Selection {
        let a = view.start.max(0.0).round().min(total_frames as f64) as usize;
        let b = view.end(width_px).max(0.0).round().min(total_frames as f64) as usize;
        Selection { anchor: a.min(b), head: b }
    }

    /// The edge of a non-empty selection within `tol_px` of pixel `x_px` (the nearer one when
    /// both are), or `None`. A bare cursor has no edges to grab.
    pub fn edge_near(&self, view: &Viewport, x_px: f32, tol_px: f32) -> Option<Edge> {
        if self.is_empty() {
            return None;
        }
        let ds = (view.x_of(self.start() as f64) - x_px).abs();
        let de = (view.x_of(self.end() as f64) - x_px).abs();
        match (ds <= tol_px, de <= tol_px) {
            (true, true) => Some(if ds <= de { Edge::Start } else { Edge::End }),
            (true, false) => Some(Edge::Start),
            (false, true) => Some(Edge::End),
            _ => None,
        }
    }

    /// Pick up `edge`: the opposite edge becomes the anchor and the grabbed edge the head, so
    /// setting `head` afterwards moves that edge (and flips the selection if it crosses).
    pub fn grab(&mut self, edge: Edge) {
        let (s, e) = (self.start(), self.end());
        *self = match edge {
            Edge::Start => Selection { anchor: e, head: s },
            Edge::End => Selection { anchor: s, head: e },
        };
    }

    /// Right-click in the waveform (Audition's default): the edge nearest `frame` moves there,
    /// so the selection grows or shrinks to the click and the other edge stays. A cursor with no
    /// selection becomes a selection from the cursor to `frame`. Equally near: the end moves.
    /// `frame` is clamped to `total` frames.
    pub fn extend_nearest_to(&self, frame: usize, total: usize) -> Selection {
        let frame = frame.min(total);
        let (start, end) = (self.start().min(total), self.end().min(total));
        if start == end {
            return Selection { anchor: start, head: frame };
        }
        if frame.abs_diff(start) < frame.abs_diff(end) { Selection { anchor: end, head: frame } } else { Selection { anchor: start, head: frame } }
    }

    /// Shift-click: move the nearer edge to `frame` (a cursor extends toward it).
    pub fn extend_to(&mut self, frame: usize) {
        if !self.is_empty() {
            let (s, e) = (self.start(), self.end());
            let near_start = frame.abs_diff(s) <= frame.abs_diff(e);
            self.grab(if near_start { Edge::Start } else { Edge::End });
        }
        self.head = frame;
    }

    /// Shift+arrow: move the moving end by `delta` frames, within `0..=total_frames`.
    pub fn extend_by(&mut self, delta: i64, total_frames: usize) {
        let h = (self.head as i64).saturating_add(delta).clamp(0, total_frames as i64);
        self.head = h as usize;
    }

    /// Shift+Home / Shift+End: move the moving end to `frame` (clamped to the file).
    pub fn extend_edge_to(&mut self, frame: usize, total_frames: usize) {
        self.head = frame.min(total_frames);
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
    /// `hh:mm:ss:ff` at a whole rate the user sets, 2 to 1000 frames a second (A102).
    Custom(u16),
    /// Compact Disc `mm:ss:ff` at 75 frames a second (A103).
    Cd75,
    /// `bar:beat.tick` at the document's tempo and time signature ([`Tempo`]). Roadmap A101.
    BarsBeats,
}

impl TimeFormat {
    /// Every format once; `Custom` carries the default rate (the settings file keeps the user's).
    pub const ALL: [TimeFormat; 12] = [
        TimeFormat::Decimal,
        TimeFormat::Compact,
        TimeFormat::Samples,
        TimeFormat::Smpte24,
        TimeFormat::Smpte25,
        TimeFormat::Smpte2997Df,
        TimeFormat::Smpte2997Ndf,
        TimeFormat::Smpte30,
        TimeFormat::Smpte5994,
        TimeFormat::Custom(CUSTOM_DEFAULT_FPS),
        TimeFormat::Cd75,
        TimeFormat::BarsBeats,
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
            TimeFormat::Custom(n) => Some(SmpteRate::Custom(n)),
            TimeFormat::Cd75 => Some(SmpteRate::Cd75),
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
            TimeFormat::Custom(_) => "custom",
            TimeFormat::Cd75 => "cd75",
            TimeFormat::BarsBeats => "bars_beats",
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
            TimeFormat::Custom(_) => "Custom frame rate…",
            TimeFormat::Cd75 => "Compact Disc (mm:ss:ff, 75 fps)",
            TimeFormat::BarsBeats => "Bars and Beats",
        }
    }

    /// The menu text with the rate a custom format is set to.
    pub fn menu_with_rate(self) -> String {
        match self {
            TimeFormat::Custom(n) => format!("Custom frame rate ({n} fps)…"),
            other => other.menu().to_string(),
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
        self.format_with(frame, sample_rate, &Tempo::default())
    }

    /// [`format`](Self::format) with the document's tempo for Bars and Beats.
    pub fn format_with(self, frame: usize, sample_rate: u32, tempo: &Tempo) -> String {
        match self {
            TimeFormat::BarsBeats => tempo.format(frame, sample_rate),
            TimeFormat::Decimal => format_time(frame, sample_rate),
            TimeFormat::Compact => compact(frame, sample_rate),
            TimeFormat::Samples => frame.to_string(),
            _ => self.smpte().map_or_else(String::new, |r| r.format(frame, sample_rate)),
        }
    }

    /// A ruler tick at `seconds`, exact for that format (a tick is not rounded through a frame).
    pub fn format_tick(self, seconds: f64, sample_rate: u32) -> String {
        self.format_tick_with(seconds, sample_rate, &Tempo::default())
    }

    /// [`format_tick`](Self::format_tick) with the document's tempo for Bars and Beats.
    pub fn format_tick_with(self, seconds: f64, sample_rate: u32, tempo: &Tempo) -> String {
        let seconds = seconds.max(0.0);
        match self {
            TimeFormat::BarsBeats => tempo.format_tick(seconds, sample_rate),
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
        self.parse_with(text, sample_rate, &Tempo::default())
    }

    /// [`parse`](Self::parse) with the document's tempo for Bars and Beats (`bar:beat.tick`).
    pub fn parse_with(self, text: &str, sample_rate: u32, tempo: &Tempo) -> Option<usize> {
        let text = text.trim();
        let rate = u64::from(sample_rate);
        if text.is_empty() || rate == 0 {
            return None;
        }
        let frames = match self {
            TimeFormat::BarsBeats => return tempo.parse(text, sample_rate),
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
            match self.smpte() {
                // The frame group: one frame of this format's rate (at least 1 sample).
                Some(r) if seps_after == 0 => ((f64::from(sample_rate) * r.frame_seconds()).round() as u64).max(1),
                Some(_) => rate * unit_secs(seps_after - 1),
                None => rate * unit_secs(seps_after - usize::from(has_fraction)),
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
        self.tick_step_with(min_seconds, sample_rate, &Tempo::default())
    }

    /// [`tick_step`](Self::tick_step) with the document's tempo for Bars and Beats: sixty-fourths
    /// of a beat up to a beat, then 1, 2, 4 ... bars.
    pub fn tick_step_with(self, min_seconds: f64, sample_rate: u32, tempo: &Tempo) -> f64 {
        let rate = f64::from(sample_rate.max(1));
        match self {
            TimeFormat::BarsBeats => {
                let (beat, bar) = (tempo.beat_seconds(), tempo.bar_seconds());
                let mut candidates: Vec<f64> = [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0].iter().map(|d| beat / d).collect();
                candidates.extend((0..20).map(|k| bar * f64::from(1u32 << k)));
                candidates.into_iter().find(|s| *s >= min_seconds).unwrap_or(bar * 1_048_576.0)
            }
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
            TimeFormat::Decimal | TimeFormat::Compact => seconds_step(min_seconds),
            _ => {
                // SMPTE: whole frames below a second.
                const FRAMES: [f64; 5] = [1.0, 2.0, 5.0, 10.0, 15.0];
                let frame = self.smpte().map_or(1.0 / 30.0, |r| r.frame_seconds());
                FRAMES.iter().map(|f| f * frame).find(|s| *s >= min_seconds).unwrap_or_else(|| seconds_step(min_seconds.max(1.0)))
            }
        }
    }

    /// How many equal parts the fine (minor) ticks cut a labelled step of `step_seconds` into:
    /// 1 when it cannot be cut into whole units of the format (one sample, one SMPTE frame).
    pub fn subdivisions(self, step_seconds: f64, sample_rate: u32) -> u32 {
        self.subdivisions_with(step_seconds, sample_rate, &Tempo::default())
    }

    /// [`subdivisions`](Self::subdivisions) with the document's tempo for Bars and Beats: a bar
    /// cuts into its beats, a beat into quarters, a double bar in halves.
    pub fn subdivisions_with(self, step_seconds: f64, sample_rate: u32, tempo: &Tempo) -> u32 {
        if self == TimeFormat::BarsBeats {
            let near = |a: f64, b: f64| (a - b).abs() <= b.abs() * 1e-6 + 1e-12;
            let (beat, bar) = (tempo.beat_seconds(), tempo.bar_seconds());
            return if near(step_seconds, bar) {
                tempo.numerator.max(1)
            } else if step_seconds > bar {
                2
            } else if near(step_seconds, beat) {
                4
            } else if near(step_seconds, 2.0 * beat) {
                2
            } else {
                1
            };
        }
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
            _ if step_seconds < 1.0 - 1e-9 && self.smpte().is_some() => {
                let frame = self.smpte().map_or(1.0 / 30.0, |r| r.frame_seconds());
                match (step_seconds / frame).round() as u32 {
                    2 => 2,
                    5 => 5,
                    10 => 5,
                    15 => 3,
                    _ => 1,
                }
            }
            _ => whole_seconds(step_seconds),
        }
    }
}

/// The smallest "round" number of seconds that is at least `min_seconds`.
fn seconds_step(min_seconds: f64) -> f64 {
    const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// A band of frequencies for a spectral selection (B135, B141): `lo_hz..hi_hz`, with an optional
/// feather that falls off outside it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Band {
    pub lo_hz: f32,
    pub hi_hz: f32,
    /// Width of the soft edge above and below the band, in Hz (0 is a hard edge).
    pub feather_hz: f32,
    /// Width of the soft edge before and after the time range, in milliseconds.
    pub feather_ms: f32,
}

impl Band {
    /// The band between `a` and `b` Hz in either order, held to `0..=nyquist` (a value that is
    /// not a number counts as 0), with hard edges.
    pub fn new(a: f32, b: f32, nyquist: f32) -> Band {
        let nyquist = if nyquist.is_finite() { nyquist.max(0.0) } else { 0.0 };
        let clean = |x: f32| if x.is_finite() { x.clamp(0.0, nyquist) } else { 0.0 };
        let (a, b) = (clean(a), clean(b));
        Band { lo_hz: a.min(b), hi_hz: a.max(b), feather_hz: 0.0, feather_ms: 0.0 }
    }
}

/// `lo-hi` in Hz, as STATE and the control channel report it.
impl std::fmt::Display for Band {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.lo_hz, self.hi_hz)
    }
}

/// A time range and a frequency band together: a rectangle on the spectral display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralSelection {
    pub time: Selection,
    pub band: Band,
    /// Sample rate, which turns the time feather from milliseconds into frames.
    pub rate: u32,
}

/// 1 at `dist <= 0`, falling along a raised cosine to 0 at `dist >= width`; 0.5 half way.
fn raised_cosine(dist: f64, width: f64) -> f64 {
    if dist <= 0.0 {
        1.0
    } else if width <= 0.0 || dist >= width {
        0.0
    } else {
        0.5 * (1.0 + (std::f64::consts::PI * dist / width).cos())
    }
}

impl SpectralSelection {
    /// How much of the audio at `frame` and `hz` the selection takes, 0 to 1: 1 inside the
    /// rectangle, 0 outside, and across each feather (which lies outside the edge) a raised
    /// cosine, so the middle of a feather is exactly 0.5. The time and frequency parts multiply.
    pub fn mask(&self, frame: usize, hz: f32) -> f32 {
        let (start, end) = (self.time.start(), self.time.end());
        // Frames before the start are `start - frame` away; frame `end` is 1 away from the last one inside.
        let t_dist = if frame < start {
            (start - frame) as f64
        } else if frame >= end {
            (frame - end + 1) as f64
        } else {
            0.0
        };
        let t_width = f64::from(self.band.feather_ms.max(0.0)) * f64::from(self.rate) / 1000.0;
        let hz = f64::from(hz);
        let f_dist = (f64::from(self.band.lo_hz) - hz).max(hz - f64::from(self.band.hi_hz)).max(0.0);
        let f_width = f64::from(self.band.feather_hz.max(0.0));
        (raised_cosine(t_dist, t_width) * raised_cosine(f_dist, f_width)) as f32
    }

    /// The selection moved by `dframes` and `dhz`, stopping at the file (`0..total` frames) and
    /// at `0..=nyquist` Hz with its length and width unchanged. A backwards drag stays backwards.
    pub fn moved(&self, dframes: i64, dhz: f32, total: usize, nyquist: f32) -> SpectralSelection {
        let (start, len) = (self.time.start() as i64, self.time.len() as i64);
        let total = total as i64;
        let new_start = (start.saturating_add(dframes)).clamp(0, (total - len).max(0));
        let shift = new_start - start;
        let shift_frame = |x: usize| (x as i64 + shift).max(0) as usize;
        let time = Selection { anchor: shift_frame(self.time.anchor), head: shift_frame(self.time.head) };
        let nyquist = if nyquist.is_finite() { nyquist.max(0.0) } else { 0.0 };
        let width = self.band.hi_hz - self.band.lo_hz;
        let dhz = if dhz.is_finite() { dhz } else { 0.0 };
        let lo = (self.band.lo_hz + dhz).clamp(0.0, (nyquist - width).max(0.0));
        SpectralSelection { time, band: Band { lo_hz: lo, hi_hz: lo + width, ..self.band }, rate: self.rate }
    }
}

/// Where `hz` sits on the spectral display's frequency axis `lo..hi`, 0 at `lo` (the bottom) to 1
/// at `hi` (the top), on the log axis when `log`. Held to 0..=1.
pub fn fraction_of_hz(lo: f32, hi: f32, log: bool, hz: f32) -> f32 {
    let t = if log { (hz / lo).ln() / (hi / lo).ln() } else { (hz - lo) / (hi - lo) };
    if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
}

/// The inverse of [`fraction_of_hz`]: the frequency at `t` (0 bottom, 1 top) of the axis.
pub fn hz_at_fraction(lo: f32, hi: f32, log: bool, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
}

/// Which channels a drag that starts at `y_fraction` of the editor's height (0 is the top of the
/// waveform area, 1 the bottom) selects. With `context` on (the default) and a stereo file, the
/// top quarter selects the left channel only, the bottom quarter the right only, and the middle
/// both. Files with other channel counts, or `context` off, always select every channel.
pub fn channels_for_drag(y_fraction: f32, channels: usize, context: bool) -> ChannelMask {
    if !context || channels != 2 || !y_fraction.is_finite() {
        return ChannelMask::ALL;
    }
    if y_fraction <= CHANNEL_STRIP {
        ChannelMask::only(0)
    } else if y_fraction >= 1.0 - CHANNEL_STRIP {
        ChannelMask::only(1)
    } else {
        ChannelMask::ALL
    }
}

/// The mask after a drag that starts at `y_fraction`. A negative fraction means the drag began
/// above the waveform area (on the time ruler): that keeps the `current` mask unchanged.
pub fn mask_after_drag(y_fraction: f32, channels: usize, context: bool, current: ChannelMask) -> ChannelMask {
    if y_fraction < 0.0 { current } else { channels_for_drag(y_fraction, channels, context) }
}

/// How tall the top and bottom channel strips are, as a fraction of the editor's height.
pub const CHANNEL_STRIP: f32 = 0.25;

/// `mask` for a `channels`-channel file as letters: `L`, `R`, `LR` for stereo, otherwise the
/// channel numbers from 1 (`1,3`); `-` for none.
pub fn channels_text(mask: ChannelMask, channels: usize) -> String {
    let active = mask.active(channels);
    match (channels, active.as_slice()) {
        (_, []) => "-".to_string(),
        (2, [0]) => "L".to_string(),
        (2, [1]) => "R".to_string(),
        (2, [0, 1]) => "LR".to_string(),
        (_, list) => list.iter().map(|c| (c + 1).to_string()).collect::<Vec<_>>().join(","),
    }
}

/// Select from the start of the file to the cursor (the selection's first frame).
pub fn select_to_start(selection: &Range<u64>, total_frames: u64) -> Range<u64> {
    0..selection.start.min(total_frames)
}

/// Select from the cursor (the selection's last frame) to the end of the file.
pub fn select_to_end(selection: &Range<u64>, total_frames: u64) -> Range<u64> {
    selection.end.min(total_frames)..total_frames
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

    #[test]
    fn ruler_origin_drag_keeps_the_channel_mask() {
        assert_eq!(mask_after_drag(-0.1, 2, true, ChannelMask::ALL), ChannelMask::ALL);
        assert_eq!(mask_after_drag(-0.1, 2, true, ChannelMask::only(1)), ChannelMask::only(1));
        assert_eq!(mask_after_drag(0.05, 2, true, ChannelMask::ALL), ChannelMask::only(0));
        assert_eq!(mask_after_drag(0.0, 2, true, ChannelMask::ALL), ChannelMask::only(0), "the lane top is in the lane");
    }

    #[test]
    fn stereo_drags_pick_a_channel_by_height() {
        assert_eq!(channels_for_drag(0.05, 2, true), ChannelMask::only(0));
        assert_eq!(channels_for_drag(0.95, 2, true), ChannelMask::only(1));
        assert_eq!(channels_for_drag(0.5, 2, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.25, 2, true), ChannelMask::only(0), "the strip's edge counts");
        assert_eq!(channels_for_drag(0.26, 2, true), ChannelMask::ALL);
        // Off, mono, surround and bad numbers always take every channel.
        assert_eq!(channels_for_drag(0.05, 2, false), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.05, 1, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(0.95, 6, true), ChannelMask::ALL);
        assert_eq!(channels_for_drag(f32::NAN, 2, true), ChannelMask::ALL);
    }

    #[test]
    fn channel_masks_read_as_text() {
        assert_eq!(channels_text(ChannelMask::only(0), 2), "L");
        assert_eq!(channels_text(ChannelMask::only(1), 2), "R");
        assert_eq!(channels_text(ChannelMask::ALL, 2), "LR");
        assert_eq!(channels_text(ChannelMask::ALL, 1), "1");
        assert_eq!(channels_text(ChannelMask::only(2).with(0), 4), "1,3");
        assert_eq!(channels_text(ChannelMask::NONE, 2), "-");
        assert_eq!(channels_text(ChannelMask::only(1), 1), "-", "a channel the file lacks");
    }

    #[test]
    fn select_variants_follow_the_view_and_the_cursor() {
        assert_eq!(select_to_start(&(400..900), 1000), 0..400);
        assert_eq!(select_to_start(&(0..0), 1000), 0..0);
        assert_eq!(select_to_end(&(400..900), 1000), 900..1000);
        assert_eq!(select_to_end(&(5000..6000), 1000), 1000..1000);
    }

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
    fn custom_and_cd_formats_read_snap_and_name_themselves() {
        let r = 44_100;
        let c = TimeFormat::Custom(1000);
        assert_eq!(c.format(r as usize, r), "00:00:01:000");
        assert_eq!(c.parse("00:00:00:999", r).map(|f| c.format(f, r)), Some("00:00:00:999".into()));
        assert_eq!(c.parse("00:00:00:1000", r), None);
        let two = TimeFormat::Custom(2);
        assert_eq!(two.parse("00:00:01:01", r), Some(r as usize * 3 / 2));
        assert_eq!(two.parse("00:00:00:02", r), None);
        assert_eq!(TimeFormat::Cd75.format(588 * 75 * 61 + 5, r), "01:01:00");
        assert_eq!(TimeFormat::Cd75.format_tick(61.0, r), "01:01:00");
        assert_eq!(TimeFormat::Cd75.snap_to_frame(300, r, 10_000), 588);
        assert_eq!(TimeFormat::Custom(75).snap_to_frame(300, r, 10_000), 588, "75 fps custom is the CD grid");
        assert_eq!(TimeFormat::from_name("custom"), Some(TimeFormat::Custom(48)));
        assert_eq!(TimeFormat::from_name("cd75"), Some(TimeFormat::Cd75));
        assert_eq!(TimeFormat::Custom(30).menu_with_rate(), "Custom frame rate (30 fps)…");
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
        assert_eq!(TimeFormat::Smpte25.unit_at("01:02:03:15", 10, r), Some(1920), "one frame at 25 fps");
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
        // Bars and Beats at 120 bpm 4/4 (a beat 0.5 s, a bar 2 s): sixty-fourths of a beat, then bars.
        let t = Tempo::default();
        assert!((TimeFormat::BarsBeats.tick_step_with(0.0001, 48_000, &t) - 0.5 / 64.0).abs() < 1e-12);
        assert_eq!(TimeFormat::BarsBeats.tick_step_with(3.0, 48_000, &t), 4.0);
        assert_eq!(TimeFormat::BarsBeats.subdivisions_with(2.0, 48_000, &t), 4, "one bar cuts into its four beats");
        assert_eq!(TimeFormat::Compact.tick_step(5000.0, 48_000), 3600.0);
        assert!((TimeFormat::Smpte24.tick_step(0.02, 48_000) - 1.0 / 24.0).abs() < 1e-12);
        assert!((TimeFormat::Smpte5994.tick_step(0.01, 48_000) - 1001.0 / 60_000.0).abs() < 1e-12);
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
    fn hand_drag_moves_the_picture_with_the_pointer() {
        let mut v = Viewport { start: 100_000.0, frames_per_px: 100.0 };
        v.drag_by(50.0, 1000.0, HOUR);
        assert_eq!(v.start, 95_000.0, "drag right: earlier audio");
        v.drag_by(-50.0, 1000.0, HOUR);
        assert_eq!(v.start, 100_000.0);
        v.drag_by(1e9, 1000.0, HOUR);
        assert_eq!(v.start, 0.0, "stops at the start");
        v.drag_by(-1e12, 1000.0, HOUR);
        assert!((v.end(1000.0) - HOUR as f64).abs() < 1e-6, "stops at the end");
    }

    #[test]
    fn zoom_click_keeps_the_clicked_frame_under_the_pointer() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        let frame = 1_000_000.0;
        let x = v.x_of(frame);
        v.zoom_click(frame, false, 1200.0, HOUR);
        assert!((v.x_of(frame) - x).abs() < 1e-3);
        assert!((v.frames_per_px - Viewport::fit(HOUR, 1200.0).frames_per_px / 2.0).abs() < 1e-9);
        v.zoom_click(frame, true, 1200.0, HOUR);
        assert_eq!(v, Viewport::fit(HOUR, 1200.0), "out again is the whole file");
    }

    #[test]
    fn zoom_to_range_fills_the_view() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        v.zoom_to_range(480_000.0, 240_000.0, 1200.0, HOUR);
        assert!((v.start - 240_000.0).abs() < 1e-6 && (v.end(1200.0) - 480_000.0).abs() < 1e-6);
        // Past the file's ends it is clipped to the file.
        v.zoom_to_range(-5_000.0, 96_000.0, 1200.0, HOUR);
        assert_eq!(v.start, 0.0);
        assert!((v.end(1200.0) - 96_000.0).abs() < 1e-6);
        // Empty or bad ranges change nothing.
        let before = v;
        v.zoom_to_range(500.0, 500.0, 1200.0, HOUR);
        v.zoom_to_range(f64::NAN, 10.0, 1200.0, HOUR);
        assert_eq!(v, before);
        // Tiny ranges stop at the closest zoom, centred on the range.
        v.zoom_to_range(100_000.0, 100_002.0, 1200.0, HOUR);
        assert_eq!(v.frames_per_px, 1.0 / 16.0);
        assert!((v.frame_at(600.0) - 100_001.0).abs() < 1e-6);
    }

    #[test]
    fn selections_are_ordered_whichever_way_they_were_dragged() {
        let s = Selection { anchor: 900, head: 100 };
        assert_eq!((s.start(), s.end(), s.len(), s.is_empty()), (100, 900, 800, false));
        assert!(Selection::at(42).is_empty());
    }

    #[test]
    fn edges_are_grabbed_within_a_few_pixels_and_cross_cleanly() {
        let v = Viewport { start: 0.0, frames_per_px: 10.0 };
        let mut s = Selection { anchor: 1000, head: 500 }; // 500..1000 = px 50..100
        assert_eq!(s.edge_near(&v, 52.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(s.edge_near(&v, 97.0, GRAB_PX), Some(Edge::End));
        assert_eq!(s.edge_near(&v, 75.0, GRAB_PX), None);
        assert_eq!(Selection::at(500).edge_near(&v, 50.0, GRAB_PX), None, "a cursor has no edges");
        // Both edges in reach: the nearer wins.
        let thin = Selection { anchor: 500, head: 540 };
        assert_eq!(thin.edge_near(&v, 51.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(thin.edge_near(&v, 52.0, GRAB_PX), Some(Edge::Start));
        assert_eq!(thin.edge_near(&v, 53.0, GRAB_PX), Some(Edge::End));
        // Drag the start edge right past the end: the selection flips, the other edge stays.
        s.grab(Edge::Start);
        s.head = 1500;
        assert_eq!((s.start(), s.end()), (1000, 1500));
        s.grab(Edge::End);
        s.head = 200;
        assert_eq!((s.start(), s.end()), (200, 1000));
    }

    #[test]
    fn shift_click_moves_the_nearer_edge() {
        let mut s = Selection { anchor: 500, head: 1000 };
        s.extend_to(1400);
        assert_eq!((s.start(), s.end()), (500, 1400));
        s.extend_to(100);
        assert_eq!((s.start(), s.end()), (100, 1400));
        s.extend_to(120);
        assert_eq!((s.start(), s.end()), (120, 1400));
        let mut c = Selection::at(700);
        c.extend_to(300);
        assert_eq!((c.start(), c.end()), (300, 700));
        let mut c = Selection::at(700);
        c.extend_to(900);
        assert_eq!((c.start(), c.end()), (700, 900));
    }

    #[test]
    fn double_click_is_the_visible_range_and_triple_is_everything() {
        let v = Viewport { start: 1000.0, frames_per_px: 10.0 };
        let s = Selection::visible(&v, 200.0, 100_000);
        assert_eq!((s.start(), s.end()), (1000, 3000));
        let past = Selection::visible(&Viewport { start: 99_000.0, frames_per_px: 10.0 }, 500.0, 100_000);
        assert_eq!((past.start(), past.end()), (99_000, 100_000), "clamped to the file");
        let a = Selection::all(100_000);
        assert_eq!((a.start(), a.end()), (0, 100_000));
    }

    #[test]
    fn shift_arrows_move_the_head_within_the_file() {
        let mut s = Selection::at(100);
        s.extend_by(50, 1000);
        assert_eq!((s.start(), s.end()), (100, 150));
        s.extend_by(-80, 1000);
        assert_eq!((s.start(), s.end()), (70, 100), "crosses the anchor and flips");
        s.extend_by(-1_000_000, 1000);
        assert_eq!((s.start(), s.end()), (0, 100));
        s.extend_by(i64::MAX, 1000);
        assert_eq!((s.start(), s.end()), (100, 1000));
        s.extend_edge_to(0, 1000);
        assert_eq!((s.start(), s.end()), (0, 100));
        s.extend_edge_to(5000, 1000);
        assert_eq!((s.start(), s.end()), (100, 1000));
    }

    #[test]
    fn times_read_like_a_clock() {
        assert_eq!(format_time(0, 48_000), "0:00.000");
        assert_eq!(format_time(48_000 * 61 + 24_000, 48_000), "1:01.500");
        assert_eq!(format_time(HOUR + 48_000 * 125 + 48, 48_000), "1:02:05.001");
        assert_eq!(format_time(44_099, 44_100), "0:00.999");
        assert_eq!(format_time(5, 0), "0:00.000");
    }

    #[test]
    fn right_click_moves_the_nearest_edge() {
        let s = Selection { anchor: 100, head: 200 };
        // Outside on the right: the end grows. Outside on the left: the start grows.
        assert_eq!(s.extend_nearest_to(300, 1000), Selection { anchor: 100, head: 300 });
        assert_eq!(s.extend_nearest_to(40, 1000), Selection { anchor: 200, head: 40 });
        // Inside: the nearer edge shortens the selection.
        assert_eq!(s.extend_nearest_to(130, 1000), Selection { anchor: 200, head: 130 });
        assert_eq!(s.extend_nearest_to(170, 1000), Selection { anchor: 100, head: 170 });
        // Equally near: the end moves.
        assert_eq!(s.extend_nearest_to(150, 1000), Selection { anchor: 100, head: 150 });
    }

    #[test]
    fn right_click_works_whichever_way_the_selection_was_dragged() {
        let s = Selection { anchor: 200, head: 100 };
        let e = s.extend_nearest_to(300, 1000);
        assert_eq!((e.start(), e.end()), (100, 300));
        let e = s.extend_nearest_to(20, 1000);
        assert_eq!((e.start(), e.end()), (20, 200));
    }

    #[test]
    fn right_click_on_a_cursor_selects_from_it_and_is_clamped() {
        assert_eq!(Selection::at(500).extend_nearest_to(800, 1000), Selection { anchor: 500, head: 800 });
        let e = Selection::at(500).extend_nearest_to(100, 1000);
        assert_eq!((e.start(), e.end()), (100, 500));
        assert_eq!(Selection::at(500).extend_nearest_to(5000, 1000).end(), 1000);
        assert_eq!(Selection::at(0).extend_nearest_to(0, 0), Selection::at(0));
        // A selection past the end (the file got shorter) is clamped too.
        assert_eq!(Selection { anchor: 50, head: 900 }.extend_nearest_to(10, 100).end(), 100);
    }
}
