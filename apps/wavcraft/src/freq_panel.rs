//! Window › Frequency Analysis: level against frequency for the cursor's moment, or the average
//! over a scanned selection (roadmap A309-A312). Read-only: it never changes the document.
//!
//! The spectrum under the cursor is one FFT, computed on the window's thread and cached until
//! the cursor, the settings or the document change. A scan of the selection runs on a worker
//! thread (it can be minutes of audio) with a progress bar and a Cancel button. Up to
//! [`MAX_HOLDS`] snapshots can be held and are drawn faintly behind the live curve; they live in
//! memory only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use wavcraft_core::DocState;
use wavcraft_core::analysis::freq::{self, FLOOR_DB, FftWindow, MAX_FFT, MIN_FFT, Spectrum};
use wavcraft_core::doc::edit::ChannelMask;

/// Snapshots that can be held at once; the oldest goes when a ninth is taken.
pub const MAX_HOLDS: usize = 8;
const DEFAULT_FFT: usize = 4096;
const TOP_DB: f32 = 0.0;
const BOTTOM_DB: f32 = -120.0;
/// The lowest frequency on the log axis.
const LOG_LOW_HZ: f32 = 20.0;
const GRID: Color32 = Color32::from_gray(45);
const TEXT: Color32 = Color32::from_gray(170);
const CHANNEL_COLORS: [Color32; 4] =
    [Color32::from_rgb(64, 200, 140), Color32::from_rgb(90, 160, 235), Color32::from_rgb(235, 190, 80), Color32::from_rgb(220, 110, 200)];
const MEAN_COLOR: Color32 = Color32::from_gray(210);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Lines,
    Area,
    Bars,
}

impl Style {
    fn name(self) -> &'static str {
        match self {
            Style::Lines => "Lines",
            Style::Area => "Area",
            Style::Bars => "Bars",
        }
    }
}

/// A scan on its worker thread.
struct Scanning {
    result: Receiver<Result<Option<Spectrum>, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    /// The document step it reads (a scan of an older step is dropped when it finishes).
    doc: usize,
}

/// What the cached cursor spectrum was made from.
#[derive(PartialEq)]
struct LiveKey {
    doc: usize,
    frame: u64,
    fft: usize,
    window: FftWindow,
    channels: u64,
}

pub struct FreqPanel {
    fft_size: usize,
    window: FftWindow,
    log_axis: bool,
    style: Style,
    scanning: Option<Scanning>,
    /// The result of the last scan, shown until cleared.
    scanned: Option<Spectrum>,
    live: Option<(LiveKey, Spectrum)>,
    holds: Vec<Spectrum>,
    error: Option<String>,
}

impl Default for FreqPanel {
    fn default() -> Self {
        FreqPanel {
            fft_size: DEFAULT_FFT,
            window: FftWindow::Hann,
            log_axis: true,
            style: Style::Lines,
            scanning: None,
            scanned: None,
            live: None,
            holds: Vec::new(),
            error: None,
        }
    }
}

impl FreqPanel {
    /// A new file was opened: nothing measured on the old one applies.
    pub fn reset(&mut self) {
        if let Some(s) = self.scanning.take() {
            s.cancel.store(true, Ordering::Relaxed);
        }
        self.scanned = None;
        self.live = None;
        self.holds.clear();
        self.error = None;
    }

    /// Draw the panel. `selection` is the selected frame range (empty is just the cursor).
    pub fn show(&mut self, ui: &mut egui::Ui, doc: Option<&Arc<DocState>>, channels: ChannelMask, selection: std::ops::Range<u64>) {
        ui.horizontal(|ui| {
            ui.heading("Frequency Analysis");
        });
        let Some(doc) = doc else {
            ui.label("No file open.");
            return;
        };
        self.poll(doc);
        let cursor = selection.start;

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("freq-fft").selected_text(format!("FFT {}", self.fft_size)).show_ui(ui, |ui| {
                let mut n = MIN_FFT;
                while n <= MAX_FFT {
                    ui.selectable_value(&mut self.fft_size, n, n.to_string());
                    n *= 2;
                }
            });
            egui::ComboBox::from_id_salt("freq-window").selected_text(self.window.name()).show_ui(ui, |ui| {
                for w in FftWindow::ALL {
                    ui.selectable_value(&mut self.window, w, w.name());
                }
            });
            egui::ComboBox::from_id_salt("freq-style").selected_text(self.style.name()).show_ui(ui, |ui| {
                for s in [Style::Lines, Style::Area, Style::Bars] {
                    ui.selectable_value(&mut self.style, s, s.name());
                }
            });
            ui.checkbox(&mut self.log_axis, "Log frequency");
            ui.separator();
            let has_selection = selection.end > selection.start;
            if self.scanning.is_some() {
                if ui.button("Cancel scan").clicked()
                    && let Some(s) = &self.scanning
                {
                    s.cancel.store(true, Ordering::Relaxed);
                }
            } else if ui
                .add_enabled(has_selection, egui::Button::new("Scan Selection"))
                .on_hover_text("Average the spectrum over the selected range")
                .clicked()
            {
                self.start_scan(doc, channels, selection.clone());
            }
            if self.scanned.is_some() && ui.button("Follow Cursor").on_hover_text("Drop the scan and show the spectrum under the cursor").clicked() {
                self.scanned = None;
            }
        });
        ui.horizontal_wrapped(|ui| {
            let can_hold = self.scanning.is_none();
            if ui
                .add_enabled(can_hold, egui::Button::new(format!("Hold ({}/{MAX_HOLDS})", self.holds.len())))
                .on_hover_text("Keep this curve as a faint overlay")
                .clicked()
                && let Some(sp) = self.shown(doc, channels, cursor).cloned()
            {
                if self.holds.len() == MAX_HOLDS {
                    self.holds.remove(0);
                }
                self.holds.push(sp);
            }
            if ui.add_enabled(!self.holds.is_empty(), egui::Button::new("Clear Holds")).clicked() {
                self.holds.clear();
            }
            if ui
                .add_enabled(self.scanning.is_none(), egui::Button::new("Copy Graph Data"))
                .on_hover_text("Copy Hz and dB, tab separated, per channel")
                .clicked()
                && let Some(sp) = self.shown(doc, channels, cursor)
            {
                let text = sp.graph_data();
                ui.ctx().copy_text(text);
            }
        });
        if let Some(s) = &self.scanning {
            ui.add(egui::ProgressBar::new(s.progress.load(Ordering::Relaxed) as f32 / 1000.0).text("Scanning…"));
            ui.ctx().request_repaint();
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(235, 110, 100), e);
        }

        let hold_count = self.holds.len();
        let (log, style) = (self.log_axis, self.style);
        let shown = self.shown(doc, channels, cursor).cloned();
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 20, 24));
        let Some(sp) = shown else {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Select a channel to analyse", FontId::proportional(14.0), TEXT);
            return;
        };
        let plot = Rect::from_min_max(pos2(rect.left() + 38.0, rect.top() + 6.0), pos2(rect.right() - 8.0, rect.bottom() - 18.0));
        if plot.width() < 20.0 || plot.height() < 20.0 {
            return;
        }
        let axis = Axis { plot, log, nyquist: sp.sample_rate as f32 / 2.0 };
        draw_grid(&painter, &axis);
        for h in &self.holds {
            for row in &h.channels {
                draw_curve(&painter, &axis, row, h.bin_hz(), Style::Lines, Color32::from_white_alpha(40));
            }
        }
        for (i, row) in sp.channels.iter().enumerate() {
            draw_curve(&painter, &axis, row, sp.bin_hz(), style, CHANNEL_COLORS[sp.channel_ids[i] % CHANNEL_COLORS.len()]);
        }
        if sp.channels.len() > 1 {
            draw_curve(&painter, &axis, &sp.mean, sp.bin_hz(), Style::Lines, MEAN_COLOR);
        }

        // The readout under the pointer.
        let mut readout = if self.scanned.is_some() { "Scan".to_string() } else { "Cursor".to_string() };
        readout.push_str(&format!(" · FFT {} · {} · {hold_count} held", sp.fft_size, self.window.name()));
        if let Some(p) = response.hover_pos().filter(|p| plot.contains(*p)) {
            let hz = axis.hz_at(p.x);
            painter.line_segment([pos2(p.x, plot.top()), pos2(p.x, plot.bottom())], Stroke::new(1.0, Color32::from_gray(90)));
            readout = format!("{hz:.1} Hz");
            for (i, row) in sp.channels.iter().enumerate() {
                let db = Spectrum::level_at(row, sp.bin_hz(), hz);
                readout.push_str(&format!("   ch {}: {db:.1} dB", sp.channel_ids[i] + 1));
            }
            if sp.channels.len() > 1 {
                readout.push_str(&format!("   mean: {:.1} dB", Spectrum::level_at(&sp.mean, sp.bin_hz(), hz)));
            }
        }
        painter.text(pos2(plot.left() + 4.0, rect.bottom() - 2.0), Align2::LEFT_BOTTOM, readout, FontId::monospace(11.0), TEXT);
    }

    /// The spectrum to show: the scan if there is one, else the one under the cursor.
    fn shown(&mut self, doc: &Arc<DocState>, channels: ChannelMask, cursor: u64) -> Option<&Spectrum> {
        if self.scanned.is_some() {
            return self.scanned.as_ref();
        }
        let key = LiveKey {
            doc: Arc::as_ptr(doc) as usize,
            frame: cursor,
            fft: self.fft_size,
            window: self.window,
            channels: channels.within(doc.channel_count()).bits(),
        };
        if self.live.as_ref().is_none_or(|(k, _)| *k != key) {
            self.live = match freq::spectrum_at(doc, cursor, channels, self.fft_size, self.window) {
                Ok(sp) => Some((key, sp)),
                Err(e) => {
                    log::debug!(target: "wavcraft", "frequency analysis: {e}");
                    None
                }
            };
        }
        self.live.as_ref().map(|(_, s)| s)
    }

    fn start_scan(&mut self, doc: &Arc<DocState>, channels: ChannelMask, range: std::ops::Range<u64>) {
        self.error = None;
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (d, c, p) = (doc.clone(), cancel.clone(), progress.clone());
        let (fft, window) = (self.fft_size, self.window);
        let spawned = std::thread::Builder::new().name("freq scan".into()).spawn(move || {
            let r = freq::scan_with_progress(&d, range, channels, fft, window, &c, Some(&p)).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        match spawned {
            Ok(_) => self.scanning = Some(Scanning { result: rx, cancel, progress, doc: Arc::as_ptr(doc) as usize }),
            Err(e) => self.error = Some(format!("could not start the scan: {e}")),
        }
    }

    fn poll(&mut self, doc: &Arc<DocState>) {
        let Some(s) = &self.scanning else { return };
        match s.result.try_recv() {
            Ok(r) => {
                let same_doc = s.doc == Arc::as_ptr(doc) as usize;
                self.scanning = None;
                match r {
                    Ok(Some(sp)) if same_doc => self.scanned = Some(sp),
                    Ok(_) => {}
                    Err(e) => self.error = Some(e),
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.scanning = None;
                self.error = Some("the scan stopped unexpectedly".to_string());
            }
        }
    }
}

/// Frequency and level to screen position.
struct Axis {
    plot: Rect,
    log: bool,
    nyquist: f32,
}

impl Axis {
    fn low(&self) -> f32 {
        if self.log { LOG_LOW_HZ } else { 0.0 }
    }

    fn x_of(&self, hz: f32) -> f32 {
        let (lo, hi) = (self.low(), self.nyquist.max(lo_plus(self.low())));
        let t = if self.log { (hz.max(lo) / lo).ln() / (hi / lo).ln() } else { (hz - lo) / (hi - lo) };
        self.plot.left() + t.clamp(0.0, 1.0) * self.plot.width()
    }

    fn hz_at(&self, x: f32) -> f32 {
        let (lo, hi) = (self.low(), self.nyquist.max(lo_plus(self.low())));
        let t = ((x - self.plot.left()) / self.plot.width()).clamp(0.0, 1.0);
        if self.log { lo * (hi / lo).powf(t) } else { lo + t * (hi - lo) }
    }

    fn y_of(&self, db: f32) -> f32 {
        let t = (db.clamp(BOTTOM_DB, TOP_DB) - TOP_DB) / (BOTTOM_DB - TOP_DB);
        self.plot.top() + t * self.plot.height()
    }
}

fn lo_plus(lo: f32) -> f32 {
    lo + 1.0
}

fn draw_grid(painter: &egui::Painter, axis: &Axis) {
    let font = FontId::monospace(10.0);
    let mut db = TOP_DB;
    while db >= BOTTOM_DB {
        let y = axis.y_of(db);
        painter.line_segment([pos2(axis.plot.left(), y), pos2(axis.plot.right(), y)], Stroke::new(1.0, GRID));
        painter.text(pos2(axis.plot.left() - 3.0, y), Align2::RIGHT_CENTER, format!("{db:.0}"), font.clone(), TEXT);
        db -= 20.0;
    }
    let ticks: Vec<f32> = if axis.log {
        [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0].to_vec()
    } else {
        let step = nice_step(axis.nyquist / 8.0);
        (0..).map(|i| i as f32 * step).take_while(|&f| f <= axis.nyquist).collect()
    };
    for hz in ticks.into_iter().filter(|&f| f <= axis.nyquist) {
        let x = axis.x_of(hz);
        painter.line_segment([pos2(x, axis.plot.top()), pos2(x, axis.plot.bottom())], Stroke::new(1.0, GRID));
        painter.text(pos2(x, axis.plot.bottom() + 2.0), Align2::CENTER_TOP, hz_label(hz), font.clone(), TEXT);
    }
}

fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz}") }
}

/// 1, 2 or 5 times a power of ten, at least `min`.
fn nice_step(min: f32) -> f32 {
    let p = 10f32.powf(min.max(1.0).log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * p).find(|&s| s >= min).unwrap_or(10.0 * p)
}

/// The level at each pixel column of the plot: the loudest bin that column covers, or, where a
/// bin covers several columns, the line between its neighbours.
fn column_levels(row: &[f32], bin_hz: f32, axis: &Axis) -> Vec<f32> {
    let cols = axis.plot.width().floor().max(1.0) as usize;
    (0..cols)
        .map(|c| {
            let (a, b) = (axis.hz_at(axis.plot.left() + c as f32), axis.hz_at(axis.plot.left() + c as f32 + 1.0));
            let (fa, fb) = (a / bin_hz, b / bin_hz);
            let (i0, i1) = (fa.ceil() as usize, (fb.floor() as usize).min(row.len().saturating_sub(1)));
            if i0 <= i1 && i0 < row.len() {
                row[i0..=i1].iter().cloned().fold(FLOOR_DB, f32::max)
            } else {
                Spectrum::level_at(row, bin_hz, (a + b) * 0.5)
            }
        })
        .collect()
}

fn draw_curve(painter: &egui::Painter, axis: &Axis, row: &[f32], bin_hz: f32, style: Style, color: Color32) {
    let levels = column_levels(row, bin_hz, axis);
    let x = |c: usize| axis.plot.left() + c as f32 + 0.5;
    let painter = painter.with_clip_rect(axis.plot);
    match style {
        Style::Lines => {
            let pts: Vec<Pos2> = levels.iter().enumerate().map(|(c, &db)| pos2(x(c), axis.y_of(db))).collect();
            painter.add(Shape::line(pts, Stroke::new(1.2, color)));
        }
        Style::Area => {
            let fill = color.gamma_multiply(0.35);
            for (c, &db) in levels.iter().enumerate() {
                painter.line_segment([pos2(x(c), axis.y_of(db)), pos2(x(c), axis.plot.bottom())], Stroke::new(1.0, fill));
            }
            let pts: Vec<Pos2> = levels.iter().enumerate().map(|(c, &db)| pos2(x(c), axis.y_of(db))).collect();
            painter.add(Shape::line(pts, Stroke::new(1.0, color)));
        }
        Style::Bars => {
            let mut c = 0;
            while c < levels.len() {
                let end = (c + 3).min(levels.len());
                let db = levels[c..end].iter().cloned().fold(FLOOR_DB, f32::max);
                let r = Rect::from_min_max(
                    pos2(axis.plot.left() + c as f32, axis.y_of(db)),
                    pos2(axis.plot.left() + end as f32 - 1.0, axis.plot.bottom()),
                );
                painter.rect_filled(r.expand2(vec2(0.0, 0.0)), 0.0, color.gamma_multiply(0.8));
                c += 4;
            }
        }
    }
}

/// The control channel's `FREQ [fft=4096] [window=hann] [start end]`: the peak of each channel.
/// With `start end` (frames) it averages that range, else the selection if there is one, else it
/// looks at the cursor's moment.
pub fn control(doc: &DocState, channels: ChannelMask, selection: std::ops::Range<u64>, args: &str) -> String {
    let (mut fft, mut window, mut range) = (DEFAULT_FFT, FftWindow::Hann, None);
    let mut nums = Vec::new();
    for tok in args.split_whitespace() {
        if let Some(v) = tok.strip_prefix("fft=") {
            match v.parse() {
                Ok(n) => fft = n,
                Err(_) => return format!("ERR bad fft size {v:?}"),
            }
        } else if let Some(v) = tok.strip_prefix("window=") {
            match FftWindow::parse(v) {
                Some(w) => window = w,
                None => return format!("ERR window must be hann, blackman or rectangular, not {v:?}"),
            }
        } else if let Ok(n) = tok.parse::<u64>() {
            nums.push(n);
        } else {
            return format!("ERR FREQ [fft=N] [window=W] [start end]: cannot read {tok:?}");
        }
    }
    match nums[..] {
        [] => {}
        [a, b] => range = Some(a..b),
        _ => return "ERR FREQ [fft=N] [window=W] [start end]".to_string(),
    }
    let range = range.or_else(|| (selection.end > selection.start).then(|| selection.clone()));
    let never = AtomicBool::new(false);
    let result = match &range {
        Some(r) => freq::scan(doc, r.clone(), channels, fft, window, &never).map(|o| o.map(|s| (s, format!("range={}-{}", r.start, r.end)))),
        None => freq::spectrum_at(doc, selection.start, channels, fft, window).map(|s| Some((s, format!("at={}", selection.start)))),
    };
    match result {
        Ok(Some((sp, what))) => {
            let mut out = format!("OK {what} fft={fft} window={}", window.name().to_ascii_lowercase());
            for (i, id) in sp.channel_ids.iter().enumerate() {
                match sp.peak(i, 20.0, sp.sample_rate as f32 / 2.0) {
                    Some(p) => out.push_str(&format!(" ch{}={:.1}Hz/{:.1}dB", id + 1, p.hz, p.db)),
                    None => out.push_str(&format!(" ch{}=-", id + 1)),
                }
            }
            out
        }
        Ok(None) => "ERR cancelled".to_string(),
        Err(e) => format!("ERR {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_maps_both_ways() {
        let plot = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 200.0));
        for log in [true, false] {
            let a = Axis { plot, log, nyquist: 24_000.0 };
            for hz in [100.0, 1000.0, 12_000.0] {
                assert!((a.hz_at(a.x_of(hz)) - hz).abs() < hz * 1e-3, "{hz} on {log}");
            }
            assert!(a.y_of(0.0) < a.y_of(-60.0) && a.y_of(-60.0) < a.y_of(-120.0));
        }
    }

    #[test]
    fn nice_steps_are_round() {
        assert_eq!(nice_step(3000.0), 5000.0);
        assert_eq!(nice_step(2000.0), 2000.0);
        assert_eq!(nice_step(1.0), 1.0);
    }

    #[test]
    fn a_column_takes_the_loudest_bin_it_covers() {
        let plot = Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 100.0));
        let a = Axis { plot, log: false, nyquist: 100.0 };
        let mut row = vec![-100.0f32; 101];
        row[55] = -3.0;
        let levels = column_levels(&row, 1.0, &a);
        assert_eq!(levels.len(), 10);
        assert_eq!(levels.iter().cloned().fold(f32::MIN, f32::max), -3.0);
        assert_eq!(levels[5], -3.0);
    }
}
