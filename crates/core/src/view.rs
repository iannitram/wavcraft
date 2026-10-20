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

    /// Show the frames `[start, end)` across `width_px` pixels (clamped to the file and to the
    /// closest zoom). Bad numbers leave the view as it was.
    pub fn set_range(&mut self, start: f64, end: f64, width_px: f32, total_frames: usize) {
        if !start.is_finite() || !end.is_finite() || end <= start {
            return;
        }
        self.frames_per_px = (end - start) / f64::from(width_px.max(1.0));
        self.start = start;
        self.clamp(width_px, total_frames);
    }

    /// The zoom navigator's handle: the visible range as `(left, right)` pixels of a bar
    /// `nav_px` wide that stands for the whole file.
    pub fn nav_handle(&self, view_px: f32, nav_px: f32, total_frames: usize) -> (f32, f32) {
        let total = total_frames.max(1) as f64;
        let to_x = |frame: f64| (frame / total * f64::from(nav_px)).clamp(0.0, f64::from(nav_px)) as f32;
        (to_x(self.start), to_x(self.end(view_px)))
    }

    /// Click outside the handle on the navigator: centre the view on pixel `x` of the bar.
    pub fn nav_jump(&mut self, x: f32, view_px: f32, nav_px: f32, total_frames: usize) {
        let frame = nav_frame(x, nav_px, total_frames);
        self.start = frame - self.frames_per_px * f64::from(view_px) * 0.5;
        self.clamp(view_px, total_frames);
    }

    /// Drag on the navigator with the pointer at pixel `x` of the bar. `grab` is how many frames
    /// the pointer was right of the view's start when the drag began (used by the body).
    pub fn nav_drag(&mut self, part: NavPart, x: f32, grab: f64, view_px: f32, nav_px: f32, total_frames: usize) {
        let frame = nav_frame(x, nav_px, total_frames);
        let end = self.end(view_px);
        let least = f64::from(view_px.max(1.0)) / MAX_PX_PER_FRAME;
        match part {
            NavPart::Body => {
                self.start = frame - grab;
                self.clamp(view_px, total_frames);
            }
            NavPart::Left => self.set_range(frame.min(end - least), end, view_px, total_frames),
            NavPart::Right => self.set_range(self.start, frame.max(self.start + least), view_px, total_frames),
        }
    }
}

/// What a press on the zoom navigator grabbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavPart {
    /// The middle of the handle: dragging scrolls.
    Body,
    /// The left edge: dragging zooms, the right end staying put.
    Left,
    /// The right edge.
    Right,
}

/// What is under pixel `x` of a navigator whose handle runs `handle.0..handle.1`: an edge when
/// within `edge_px` of it, the body between, `None` outside (a click there jumps).
pub fn nav_hit(handle: (f32, f32), x: f32, edge_px: f32) -> Option<NavPart> {
    let (l, r) = handle;
    // Narrow handles still get a body: the edges share the width.
    let edge = edge_px.min(((r - l) / 3.0).max(1.0));
    if x < l - edge_px.min(3.0) || x > r + edge_px.min(3.0) {
        None
    } else if x <= l + edge {
        Some(NavPart::Left)
    } else if x >= r - edge {
        Some(NavPart::Right)
    } else {
        Some(NavPart::Body)
    }
}

fn nav_frame(x: f32, nav_px: f32, total_frames: usize) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    (f64::from(x) / f64::from(nav_px.max(1.0))).clamp(0.0, 1.0) * total_frames as f64
}

/// Vertical zoom of the waveform: 1 shows the full range, 8 shows one eighth of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmpZoom(f32);

impl AmpZoom {
    pub const MIN: f32 = 1.0;
    pub const MAX: f32 = 1024.0;

    pub fn full() -> AmpZoom {
        AmpZoom(1.0)
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// Zoom by `factor` (above 1 zooms in), stopping at the limits; bad numbers change nothing.
    pub fn by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.0 = (self.0 * factor).clamp(Self::MIN, Self::MAX);
        }
    }
}

impl Default for AmpZoom {
    fn default() -> Self {
        AmpZoom::full()
    }
}

/// How the amplitude ruler labels the scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmpScale {
    Db,
    #[default]
    Percent,
    Samples,
    Normalized,
}

/// One mark on the amplitude ruler: a level from 0 up to full scale 1, and its text (for the
/// negative side the text of a linear scale gets a minus sign: [`AmpScale::label`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub amp: f32,
    pub label: String,
}

impl AmpScale {
    pub const ALL: [AmpScale; 4] = [AmpScale::Db, AmpScale::Percent, AmpScale::Samples, AmpScale::Normalized];

    /// The name in the settings file and on the control channel.
    pub fn name(self) -> &'static str {
        match self {
            AmpScale::Db => "db",
            AmpScale::Percent => "percent",
            AmpScale::Samples => "samples",
            AmpScale::Normalized => "normalized",
        }
    }

    /// The menu text.
    pub fn menu(self) -> &'static str {
        match self {
            AmpScale::Db => "Decibels",
            AmpScale::Percent => "Percentage",
            AmpScale::Samples => "Sample values",
            AmpScale::Normalized => "Normalized (-1 to 1)",
        }
    }

    pub fn parse(name: &str) -> Option<AmpScale> {
        Self::ALL.into_iter().find(|s| s.name() == name.trim().to_ascii_lowercase())
    }

    /// Full scale in this scale's unit (a linear scale only).
    fn full_scale(self) -> f64 {
        match self {
            AmpScale::Percent => 100.0,
            AmpScale::Samples => 32768.0,
            _ => 1.0,
        }
    }

    /// The text for a level `amp` (signed, 1 is full scale).
    pub fn label(self, amp: f32) -> String {
        let a = f64::from(amp);
        match self {
            AmpScale::Db => {
                if a == 0.0 {
                    "-inf".to_string()
                } else {
                    format!("{:.0}", 20.0 * a.abs().log10())
                }
            }
            AmpScale::Normalized => linear_text(a),
            _ => linear_text(a * self.full_scale()),
        }
    }

    /// The marks for a lane whose half height is `half_px` pixels at vertical zoom `zoom`, at
    /// least `min_gap_px` apart, from the centre up to the top edge (only levels in view).
    pub fn ticks(self, zoom: AmpZoom, half_px: f32, min_gap_px: f32) -> Vec<Tick> {
        let visible = 1.0 / f64::from(zoom.get());
        let px_per_amp = f64::from(half_px.max(1.0)) * f64::from(zoom.get());
        let gap = f64::from(min_gap_px.max(1.0));
        let mut out = Vec::new();
        match self {
            AmpScale::Db => {
                // Full scale down, skipping marks that would crowd the one before.
                let mut last = f64::NEG_INFINITY;
                for db in [0, -1, -2, -3, -4, -6, -9, -12, -15, -18, -24, -30, -36, -42, -48, -60, -72, -84, -96, -120, -144] {
                    let amp = 10f64.powf(f64::from(db) / 20.0);
                    if amp > visible * 1.0001 {
                        continue;
                    }
                    let y = amp * px_per_amp;
                    if last - y >= gap || last == f64::NEG_INFINITY {
                        out.push(Tick { amp: amp as f32, label: format!("{db}") });
                        last = y;
                    }
                }
            }
            _ => {
                let full = self.full_scale();
                let step = nice_step(gap * full / px_per_amp);
                let top = visible * full;
                let mut k = 0u32;
                while f64::from(k) * step <= top * 1.0001 && k < 200 {
                    let value = f64::from(k) * step;
                    let amp = (value / full) as f32;
                    out.push(Tick { amp, label: linear_text(value) });
                    k += 1;
                }
            }
        }
        out
    }
}

/// A number for the ruler: whole when it is, else with up to three decimals.
fn linear_text(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The smallest 1, 2, 2.5 or 5 times a power of ten that is at least `min`.
fn nice_step(min: f64) -> f64 {
    if !min.is_finite() || min <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(min.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= min * 0.9999).unwrap_or(10.0 * mag)
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

    /// Right-click in the waveform (Audition's default): the edge nearest `frame` moves there,
    /// so the selection grows or shrinks to the click and the other edge stays. A cursor with no
    /// selection becomes a selection from the cursor to `frame`. Equally near: the end moves.
    /// `frame` is clamped to `total` frames.
    pub fn right_click_extend(&self, frame: usize, total: usize) -> Selection {
        let frame = frame.min(total);
        let (start, end) = (self.start().min(total), self.end().min(total));
        if start == end {
            return Selection { anchor: start, head: frame };
        }
        if frame.abs_diff(start) < frame.abs_diff(end) { Selection { anchor: end, head: frame } } else { Selection { anchor: start, head: frame } }
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
    fn right_click_moves_the_nearest_edge() {
        let s = Selection { anchor: 100, head: 200 };
        // Outside on the right: the end grows. Outside on the left: the start grows.
        assert_eq!(s.right_click_extend(300, 1000), Selection { anchor: 100, head: 300 });
        assert_eq!(s.right_click_extend(40, 1000), Selection { anchor: 200, head: 40 });
        // Inside: the nearer edge shortens the selection.
        assert_eq!(s.right_click_extend(130, 1000), Selection { anchor: 200, head: 130 });
        assert_eq!(s.right_click_extend(170, 1000), Selection { anchor: 100, head: 170 });
        // Equally near: the end moves.
        assert_eq!(s.right_click_extend(150, 1000), Selection { anchor: 100, head: 150 });
    }

    #[test]
    fn right_click_works_whichever_way_the_selection_was_dragged() {
        let s = Selection { anchor: 200, head: 100 };
        let e = s.right_click_extend(300, 1000);
        assert_eq!((e.start(), e.end()), (100, 300));
        let e = s.right_click_extend(20, 1000);
        assert_eq!((e.start(), e.end()), (20, 200));
    }

    #[test]
    fn right_click_on_a_cursor_selects_from_it_and_is_clamped() {
        assert_eq!(Selection::at(500).right_click_extend(800, 1000), Selection { anchor: 500, head: 800 });
        let e = Selection::at(500).right_click_extend(100, 1000);
        assert_eq!((e.start(), e.end()), (100, 500));
        assert_eq!(Selection::at(500).right_click_extend(5000, 1000).end(), 1000);
        assert_eq!(Selection::at(0).right_click_extend(0, 0), Selection::at(0));
        // A selection past the end (the file got shorter) is clamped too.
        assert_eq!(Selection { anchor: 50, head: 900 }.right_click_extend(10, 100).end(), 100);
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
    fn set_range_shows_exactly_the_range_asked_for() {
        let mut v = Viewport::fit(HOUR, 1200.0);
        v.set_range(1_000_000.0, 2_200_000.0, 1200.0, HOUR);
        assert_eq!(v.start, 1_000_000.0);
        assert!((v.end(1200.0) - 2_200_000.0).abs() < 1e-6);
        let before = v;
        v.set_range(5.0, 5.0, 1200.0, HOUR);
        v.set_range(f64::NAN, 9.0, 1200.0, HOUR);
        assert_eq!(v, before, "bad ranges change nothing");
        // Past the end or too tight: clamped like any other zoom.
        v.set_range(HOUR as f64 - 10.0, HOUR as f64 + 1e6, 1200.0, HOUR);
        assert!(v.end(1200.0) <= HOUR as f64 + 1e-6);
        v.set_range(100.0, 100.5, 1200.0, HOUR);
        assert_eq!(v.frames_per_px, 1.0 / 16.0);
    }

    #[test]
    fn the_navigator_handle_is_the_visible_part_of_the_bar() {
        let mut v = Viewport::fit(HOUR, 1000.0);
        assert_eq!(v.nav_handle(1000.0, 500.0, HOUR), (0.0, 500.0));
        v.set_range(HOUR as f64 / 4.0, HOUR as f64 / 2.0, 1000.0, HOUR);
        let (l, r) = v.nav_handle(1000.0, 400.0, HOUR);
        assert!((l - 100.0).abs() < 1e-3 && (r - 200.0).abs() < 1e-3, "{l} {r}");
    }

    #[test]
    fn navigator_clicks_and_drags_scroll_and_zoom() {
        let (view_px, nav_px) = (1000.0_f32, 400.0_f32);
        let mut v = Viewport::fit(HOUR, view_px);
        v.set_range(HOUR as f64 * 0.25, HOUR as f64 * 0.5, view_px, HOUR);
        let span = v.end(view_px) - v.start;
        // A click outside the handle centres the view there, same zoom.
        v.nav_jump(300.0, view_px, nav_px, HOUR);
        assert!((v.start + span / 2.0 - HOUR as f64 * 0.75).abs() < 1.0);
        assert!((v.end(view_px) - v.start - span).abs() < 1e-3);
        // Jumping to the far end stops at the end of the file.
        v.nav_jump(400.0, view_px, nav_px, HOUR);
        assert!((v.end(view_px) - HOUR as f64).abs() < 1e-3);
        // Dragging the body by 40 bar pixels moves the view by a tenth of the file.
        v.set_range(HOUR as f64 * 0.25, HOUR as f64 * 0.5, view_px, HOUR);
        let handle = v.nav_handle(view_px, nav_px, HOUR);
        let grab = (150.0 / f64::from(nav_px)) * HOUR as f64 - v.start;
        assert_eq!(nav_hit(handle, 150.0, 5.0), Some(NavPart::Body));
        v.nav_drag(NavPart::Body, 190.0, grab, view_px, nav_px, HOUR);
        assert!((v.start - HOUR as f64 * 0.35).abs() < 1.0, "{}", v.start);
        assert!((v.end(view_px) - v.start - span).abs() < 1e-3, "scrolling keeps the zoom");
        // The left edge zooms and the right end stays.
        let end = v.end(view_px);
        v.nav_drag(NavPart::Left, 100.0, 0.0, view_px, nav_px, HOUR);
        assert!((v.start - HOUR as f64 * 0.25).abs() < 1.0);
        assert!((v.end(view_px) - end).abs() < 1.0);
        // The right edge zooms and the left end stays; crossing over keeps a minimum width.
        let start = v.start;
        v.nav_drag(NavPart::Right, 399.0, 0.0, view_px, nav_px, HOUR);
        assert_eq!(v.start, start);
        assert!(v.end(view_px) > HOUR as f64 * 0.99);
        v.nav_drag(NavPart::Right, 0.0, 0.0, view_px, nav_px, HOUR);
        assert!(v.end(view_px) > v.start && v.frames_per_px >= 1.0 / 16.0);
        v.nav_drag(NavPart::Body, f32::NAN, 0.0, view_px, nav_px, HOUR);
        assert!(v.start.is_finite());
    }

    #[test]
    fn navigator_hit_testing_finds_edges_body_and_outside() {
        let handle = (100.0, 200.0);
        assert_eq!(nav_hit(handle, 101.0, 6.0), Some(NavPart::Left));
        assert_eq!(nav_hit(handle, 199.0, 6.0), Some(NavPart::Right));
        assert_eq!(nav_hit(handle, 150.0, 6.0), Some(NavPart::Body));
        assert_eq!(nav_hit(handle, 50.0, 6.0), None);
        assert_eq!(nav_hit(handle, 260.0, 6.0), None);
        // A sliver of a handle can still be grabbed and still has a middle.
        assert_eq!(nav_hit((100.0, 103.0), 101.5, 6.0), Some(NavPart::Body));
    }

    #[test]
    fn vertical_zoom_stays_within_its_limits() {
        let mut z = AmpZoom::default();
        z.by(0.5);
        assert_eq!(z.get(), 1.0, "never below the full range");
        for _ in 0..40 {
            z.by(2.0);
        }
        assert_eq!(z.get(), AmpZoom::MAX);
        z.by(f32::NAN);
        z.by(0.0);
        z.by(-2.0);
        assert_eq!(z.get(), AmpZoom::MAX);
    }

    #[test]
    fn scale_names_round_trip() {
        for s in AmpScale::ALL {
            assert_eq!(AmpScale::parse(s.name()), Some(s));
        }
        assert_eq!(AmpScale::parse(" DB "), Some(AmpScale::Db));
        assert_eq!(AmpScale::parse("furlongs"), None);
    }

    #[test]
    fn labels_follow_the_scale() {
        assert_eq!(AmpScale::Percent.label(0.5), "50");
        assert_eq!(AmpScale::Percent.label(-1.0), "-100");
        assert_eq!(AmpScale::Samples.label(0.5), "16384");
        assert_eq!(AmpScale::Normalized.label(0.25), "0.25");
        assert_eq!(AmpScale::Db.label(0.5), "-6");
        assert_eq!(AmpScale::Db.label(-1.0), "0");
        assert_eq!(AmpScale::Db.label(0.0), "-inf");
    }

    #[test]
    fn linear_ticks_are_round_and_spaced() {
        // 100 px half height at zoom 1: percent marks every 20 or more px apart.
        let t = AmpScale::Percent.ticks(AmpZoom::full(), 100.0, 25.0);
        let labels: Vec<_> = t.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["0", "25", "50", "75", "100"], "25 % is 25 px at 100 px per 100 %");
        assert!(t.windows(2).all(|w| (w[1].amp - w[0].amp) * 100.0 >= 24.9));
        // Zoomed 4x: only the lowest quarter is in view, with finer marks.
        let z = {
            let mut z = AmpZoom::full();
            z.by(4.0);
            z
        };
        let t = AmpScale::Normalized.ticks(z, 100.0, 40.0);
        assert!(t.iter().all(|t| t.amp <= 0.25 + 1e-6), "{t:?}");
        assert!(t.len() >= 3 && t[0].amp == 0.0);
        // Sample values at full range.
        let t = AmpScale::Samples.ticks(AmpZoom::full(), 100.0, 25.0);
        assert_eq!(t.last().map(|t| t.label.as_str()), Some("30000"), "{t:?}");
    }

    #[test]
    fn decibel_ticks_start_at_full_scale_and_thin_out_when_crowded() {
        let roomy = AmpScale::Db.ticks(AmpZoom::full(), 200.0, 20.0);
        assert_eq!(roomy[0], Tick { amp: 1.0, label: "0".to_string() });
        assert!(roomy.iter().any(|t| t.label == "-6"));
        let tight = AmpScale::Db.ticks(AmpZoom::full(), 40.0, 20.0);
        assert!(tight.len() < roomy.len());
        // Marks are in descending level, never closer than the gap.
        for w in roomy.windows(2) {
            assert!(w[1].amp < w[0].amp);
            assert!((w[0].amp - w[1].amp) * 200.0 >= 19.9);
        }
        // Zoomed 8x: nothing above one eighth of full scale (-18 dB) is in view.
        let mut z = AmpZoom::full();
        z.by(8.0);
        let t = AmpScale::Db.ticks(z, 100.0, 20.0);
        assert!(t.iter().all(|t| t.amp <= 0.125 + 1e-4), "{t:?}");
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
}
