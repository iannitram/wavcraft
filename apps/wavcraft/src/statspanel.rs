//! Window › Amplitude Statistics: the scan runs on a worker thread (streamed from the document,
//! never the whole file in memory) and the panel shows the numbers per channel. The maths is in
//! `wavcraft_core::stats`.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wavcraft_core::stats::{self, DEFAULT_RMS_WINDOW_MS, Stats};
use wavcraft_core::{DocState, format_time};

/// What a button in the panel asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Peak,
    NextClip,
    PrevClip,
}

struct Job {
    result: Receiver<Result<Option<Stats>, String>>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    state: Arc<DocState>,
    whole: bool,
}

/// The last result, with the document step it measured.
pub struct Measured {
    pub state: Arc<DocState>,
    pub stats: Stats,
    pub whole: bool,
}

#[derive(Default)]
pub struct StatsPanel {
    pub show: bool,
    job: Option<Job>,
    pub measured: Option<Measured>,
    pub error: Option<String>,
}

impl StatsPanel {
    /// Start a scan of `range` of `state` on a worker thread (a running scan is cancelled).
    pub fn start(&mut self, state: Arc<DocState>, range: Range<u64>, whole: bool, ctx: &egui::Context) {
        self.cancel();
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (s, p, c, ctx) = (state.clone(), progress.clone(), cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("amplitude statistics".into()).spawn(move || {
            let mut report = |f: f64| {
                p.store((f.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let out = stats::scan(&s, range, DEFAULT_RMS_WINDOW_MS, &mut report, &c).map_err(|e| e.to_string());
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.error = None;
                self.job = Some(Job { result: rx, progress, cancel, state, whole });
            }
            Err(e) => self.error = Some(format!("could not start the scan: {e}")),
        }
    }

    pub fn cancel(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Take a finished scan's result (call every frame).
    pub fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.result.try_recv() {
            Ok(Ok(Some(stats))) => {
                if let Some(j) = self.job.take() {
                    log::info!(target: "wavcraft", "amplitude statistics: {}", stats.summary());
                    self.measured = Some(Measured { state: j.state, stats, whole: j.whole });
                }
            }
            Ok(Ok(None)) => self.job = None,
            Ok(Err(e)) => {
                self.job = None;
                self.error = Some(format!("Amplitude Statistics: {e}"));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("Amplitude Statistics: the scan stopped unexpectedly".to_string());
            }
        }
    }

    /// Drop results that belong to another document.
    pub fn forget(&mut self) {
        self.cancel();
        self.measured = None;
    }

    /// Draw the window. `current` is the document step on screen; `selection` is what Scan would
    /// measure now (a range, or `None` for the whole file).
    pub fn ui(&mut self, ctx: &egui::Context, current: Option<&Arc<DocState>>, selection: Option<Range<u64>>) -> (bool, Option<Nav>) {
        if !self.show {
            return (false, None);
        }
        let mut open = true;
        let mut scan = false;
        let mut nav = None;
        let mut cancel = false;
        egui::Window::new("Amplitude Statistics").open(&mut open).default_width(420.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let scope = match (&selection, current) {
                    (Some(r), Some(d)) => {
                        format!("Selection {} – {}", format_time(r.start as usize, d.sample_rate()), format_time(r.end as usize, d.sample_rate()))
                    }
                    (None, Some(_)) => "Whole file".to_string(),
                    _ => "No file open".to_string(),
                };
                ui.label(scope);
                if self.running() {
                    let p = self.job.as_ref().map_or(0, |j| j.progress.load(Ordering::Relaxed));
                    ui.add(egui::ProgressBar::new(p as f32 / 1000.0).desired_width(120.0).show_percentage());
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                } else if ui.add_enabled(current.is_some(), egui::Button::new("Scan")).clicked() {
                    scan = true;
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(235, 80, 70), e);
            }
            let Some(m) = &self.measured else {
                ui.label("Press Scan to measure.");
                return;
            };
            let stale = current.is_none_or(|c| !Arc::ptr_eq(c, &m.state));
            if stale {
                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "The file changed since this scan: scan again.");
            }
            let st = &m.stats;
            let rate = st.sample_rate;
            ui.label(format!(
                "{} measured: {} – {}  (RMS window {} ms)",
                if m.whole { "Whole file" } else { "Selection" },
                format_time(st.range.start as usize, rate),
                format_time(st.range.end as usize, rate),
                st.window_frames * 1000 / u64::from(rate.max(1))
            ));
            egui::Grid::new("amp-stats").striped(true).num_columns(1 + st.channels.len()).show(ui, |ui| {
                ui.label("");
                for i in 0..st.channels.len() {
                    ui.strong(channel_name(i, st.channels.len()));
                }
                ui.end_row();
                let row = |ui: &mut egui::Ui, name: &str, f: &dyn Fn(usize) -> String| {
                    ui.label(name);
                    for i in 0..st.channels.len() {
                        ui.label(f(i));
                    }
                    ui.end_row();
                };
                let at = |frame: u64| format_time(frame as usize, rate);
                row(ui, "Peak amplitude", &|i| db(st.channels[i].peak_db()));
                row(ui, "Peak at", &|i| at(st.channels[i].peak_frame));
                row(ui, "Minimum sample", &|i| format!("{:.6}", st.channels[i].min));
                row(ui, "Maximum sample", &|i| format!("{:.6}", st.channels[i].max));
                row(ui, "Possibly clipped runs", &|i| format!("{} ({} samples)", st.channels[i].clipped_runs, st.channels[i].clipped_samples));
                row(ui, "DC offset", &|i| format!("{:.4} %", st.channels[i].dc_percent()));
                row(ui, "Minimum RMS", &|i| db(st.channels[i].rms_min_db()));
                row(ui, "Maximum RMS", &|i| db(st.channels[i].rms_max_db()));
                row(ui, "Average RMS", &|i| db(st.channels[i].rms_avg_db()));
                row(ui, "Total RMS", &|i| db(st.channels[i].rms_total_db()));
                row(ui, "Dynamic range", &|i| {
                    st.dynamic_range_db.get(i).copied().flatten().map_or_else(|| "–".to_string(), |d| format!("{d:.2} dB"))
                });
                row(ui, "Actual bit depth", &|i| match st.channels[i].bit_depth {
                    0 => "silence".to_string(),
                    32 => "float".to_string(),
                    b => format!("{b} bits"),
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!stale, egui::Button::new("Go to Peak")).clicked() {
                    nav = Some(Nav::Peak);
                }
                if ui.add_enabled(!stale, egui::Button::new("◀ Previous Clipped")).clicked() {
                    nav = Some(Nav::PrevClip);
                }
                if ui.add_enabled(!stale, egui::Button::new("Next Clipped ▶")).clicked() {
                    nav = Some(Nav::NextClip);
                }
            });
        });
        if cancel {
            self.cancel();
        }
        self.show = open;
        (scan, nav)
    }
}

fn channel_name(i: usize, n: usize) -> String {
    match (n, i) {
        (2, 0) => "Left".to_string(),
        (2, 1) => "Right".to_string(),
        _ => format!("Ch {}", i + 1),
    }
}

fn db(x: f64) -> String {
    if x.is_finite() { format!("{x:.2} dBFS") } else { "−∞".to_string() }
}
