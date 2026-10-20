//! The Levels panel (a horizontal L/R peak meter at the bottom) and the Amplitude/Loudness
//! statistics window (f26). The meter maths is `wavcraft_core::levels`; the measurement is
//! `wavcraft_core::loudness`, run on a worker thread so the window stays responsive.

use std::io;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use wavcraft_core::DocState;
use wavcraft_core::levels::{FLOOR_DB, Levels, Reading};
use wavcraft_core::loudness::{self, Loudness};

const TRACK: Color32 = Color32::from_rgb(30, 33, 40);
const GREEN: Color32 = Color32::from_rgb(70, 190, 120);
const AMBER: Color32 = Color32::from_rgb(225, 185, 60);
const RED: Color32 = Color32::from_rgb(235, 70, 60);
const HOLD: Color32 = Color32::from_gray(235);
/// Colour changes at these levels, dBFS.
const AMBER_FROM: f32 = -18.0;
const RED_FROM: f32 = -6.0;

/// A measurement running on a worker thread.
struct Running {
    scope: String,
    result: Receiver<io::Result<Loudness>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

/// Window state for the meters and the statistics window.
pub struct MetersUi {
    pub levels: Levels,
    last: Option<Instant>,
    /// Window › Levels.
    pub show_levels: bool,
    /// Window › Amplitude/Loudness Statistics.
    pub show_stats: bool,
    running: Option<Running>,
    /// The last result, with what it measured.
    done: Option<(String, Result<Loudness, String>)>,
}

impl MetersUi {
    pub fn new() -> MetersUi {
        MetersUi { levels: Levels::new(), last: None, show_levels: true, show_stats: false, running: None, done: None }
    }

    /// Feed the bars the peaks taken from the player since the previous call.
    pub fn update(&mut self, peaks: [f32; 2]) {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        self.last = Some(now);
        self.levels.update(peaks, dt);
    }

    /// Whether the window should keep repainting for the bars (while they move).
    pub fn animating(&self) -> bool {
        self.levels.active() || self.running.is_some()
    }

    /// A new file: the old file's levels and statistics mean nothing now.
    pub fn new_file(&mut self) {
        self.levels.reset();
        self.done = None;
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The bottom panel.
    pub fn panel(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let (response, painter) = ui.allocate_painter(vec2(width, 54.0), Sense::click());
        let rect = response.rect;
        let (label_w, clip_w, num_w) = (16.0, 18.0, 48.0);
        let bar = Rect::from_min_max(pos2(rect.left() + label_w, rect.top() + 2.0), pos2(rect.right() - clip_w - num_w - 8.0, rect.bottom() - 16.0));
        let font = FontId::proportional(11.0);
        let dim = Color32::from_gray(150);
        let x_of = |db: f32| bar.left() + bar.width() * ((db.clamp(FLOOR_DB, 0.0) - FLOOR_DB) / -FLOOR_DB);
        let lane_h = (bar.height() - 3.0) / 2.0;
        let mut reset = false;
        for ch in 0..2 {
            let top = bar.top() + ch as f32 * (lane_h + 3.0);
            let lane = Rect::from_min_size(pos2(bar.left(), top), vec2(bar.width(), lane_h));
            let r = self.levels.reading(ch);
            painter.text(pos2(rect.left() + 2.0, lane.center().y), Align2::LEFT_CENTER, if ch == 0 { "L" } else { "R" }, font.clone(), dim);
            painter.rect_filled(lane, 1.0, TRACK);
            paint_bar(&painter, lane, r.level_db, &x_of);
            if r.hold_db > FLOOR_DB {
                let x = x_of(r.hold_db);
                painter.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(2.0, HOLD));
            }
            let clip = Rect::from_min_size(pos2(bar.right() + 4.0, top), vec2(clip_w, lane_h));
            painter.rect_filled(clip, 2.0, if r.clipped { RED } else { TRACK });
            if r.clipped {
                painter.text(clip.center(), Align2::CENTER_CENTER, "!", font.clone(), Color32::WHITE);
            }
            let text = if r.hold_db > FLOOR_DB { format!("{:.1}", r.hold_db) } else { "-inf".to_string() };
            painter.text(pos2(rect.right() - 2.0, lane.center().y), Align2::RIGHT_CENTER, text, font.clone(), dim);
            if response.clicked()
                && response.interact_pointer_pos().is_some_and(|p| p.x >= clip.left() - 2.0 && clip.expand2(vec2(2.0, 4.0)).contains(p))
            {
                reset = true;
            }
        }
        // A scale under the bars, every 6 dB.
        let mut db = FLOOR_DB;
        while db <= 0.0 {
            let x = x_of(db);
            painter.line_segment([pos2(x, bar.bottom() + 1.0), pos2(x, bar.bottom() + 4.0)], Stroke::new(1.0, dim));
            painter.text(pos2(x, bar.bottom() + 5.0), Align2::CENTER_TOP, format!("{db:.0}"), FontId::proportional(9.0), dim);
            db += 6.0;
        }
        if response.clicked() && !reset {
            // A click anywhere else on the panel clears the lights too (Audition: click the meter).
            reset = true;
        }
        if reset {
            self.levels.reset_clip();
        }
        response.on_hover_text("Peak level, dBFS. Click to clear the clip lights.");
    }

    /// The statistics window. `measure` is the state and range a click on Measure would use.
    pub fn stats_window(&mut self, ctx: &egui::Context, target: Option<(Arc<DocState>, Range<u64>, String)>) {
        self.poll();
        let mut open = self.show_stats;
        egui::Window::new("Amplitude/Loudness Statistics").open(&mut open).resizable(false).show(ctx, |ui| {
            let can = target.is_some() && self.running.is_none();
            if ui.add_enabled(can, egui::Button::new("Measure")).clicked()
                && let Some((state, range, scope)) = target.clone()
            {
                self.start(state, range, scope);
            }
            if let Some(r) = &self.running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Measuring {}… {}%", r.scope, r.progress.load(Ordering::Relaxed) / 10));
                    if ui.button("Cancel").clicked() {
                        r.cancel.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Some((scope, result)) = &self.done {
                ui.label(format!("Measured: {scope}"));
                match result {
                    Ok(m) => stats_grid(ui, m),
                    Err(e) => {
                        ui.colored_label(RED, e);
                    }
                }
            } else {
                ui.label("Measures the selection, or the whole file when nothing is selected.");
            }
        });
        self.show_stats = open;
    }

    /// Start measuring on a worker thread.
    pub fn start(&mut self, state: Arc<DocState>, range: Range<u64>, scope: String) {
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        let (c, p) = (cancel.clone(), progress.clone());
        let started = std::thread::Builder::new().name("loudness".into()).spawn(move || {
            let r = loudness::measure(&state, range, &c, &mut |f| p.store((f * 1000.0) as u32, Ordering::Relaxed));
            let _ = tx.send(r);
        });
        if started.is_ok() {
            self.running = Some(Running { scope, result: rx, cancel, progress });
        } else {
            self.done = Some((scope, Err("could not start the measurement".to_string())));
        }
    }

    fn poll(&mut self) {
        let Some(r) = &self.running else { return };
        let outcome = match r.result.try_recv() {
            Ok(Ok(m)) => Ok(m),
            Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => Err("cancelled".to_string()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the measurement stopped unexpectedly".to_string()),
        };
        if let Some(r) = self.running.take() {
            self.done = Some((r.scope, outcome));
        }
    }
}

fn paint_bar(painter: &egui::Painter, lane: Rect, level_db: f32, x_of: &dyn Fn(f32) -> f32) {
    if level_db <= FLOOR_DB {
        return;
    }
    // Segments up to -18, -6 and 0, each in its colour, cut at the level.
    let stops = [(FLOOR_DB, AMBER_FROM, GREEN), (AMBER_FROM, RED_FROM, AMBER), (RED_FROM, 0.0, RED)];
    for (from, to, color) in stops {
        if level_db > from {
            let seg = Rect::from_min_max(pos2(x_of(from), lane.top()), pos2(x_of(level_db.min(to)), lane.bottom()));
            painter.rect_filled(seg, 0.0, color);
        }
    }
}

fn db_text(v: f64, unit: &str) -> String {
    if v.is_finite() { format!("{v:.1} {unit}") } else { format!("-inf {unit}") }
}

fn stats_grid(ui: &mut egui::Ui, m: &Loudness) {
    egui::Grid::new("loudness-stats").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        let mut row = |name: &str, value: String| {
            ui.label(name);
            ui.monospace(value);
            ui.end_row();
        };
        row("Integrated loudness", db_text(m.integrated_lufs, "LUFS"));
        row("Loudness range", format!("{:.1} LU", m.range_lu));
        row("Max momentary", db_text(m.max_momentary_lufs, "LUFS"));
        row("True peak", db_text(m.true_peak_dbtp, "dBTP"));
        row("Sample peak", db_text(m.sample_peak_dbfs, "dBFS"));
        row("Length", format!("{} frames", m.frames));
    });
}

/// `METERS` on the control channel: the bars as numbers.
pub fn readings_line(levels: &Levels) -> String {
    let f = |v: f32| if v > FLOOR_DB - 80.0 { format!("{v:.2}") } else { "-inf".to_string() };
    let one = |tag: &str, r: Reading| format!("{tag}={} {tag}_hold={} {tag}_clip={}", f(r.level_db), f(r.hold_db), u8::from(r.clipped));
    format!("OK {} {}", one("l", levels.reading(0)), one("r", levels.reading(1)))
}
