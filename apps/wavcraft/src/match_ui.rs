//! The Match Loudness panel (f33; `match` in `panels::REGISTRY`): the open document in a list, Scan (measure), the target
//! settings, and Run (one undoable step). The work is `wavcraft_core::match_loudness`, on a worker
//! thread so the window stays responsive; the app pushes the finished edit on the history.

use crate::theme;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wavcraft_core::DocState;
use wavcraft_core::loudness::{self, Loudness};
use wavcraft_core::match_loudness::{self, MATCH_TO, MatchResult, MatchTo, PRESETS, Target};

/// A finished Run, for the app to push on the history if the document is still the one it ran on.
pub struct Finished {
    pub base: Arc<DocState>,
    pub result: MatchResult,
}

enum Work {
    Scan(Receiver<io::Result<Loudness>>),
    Run(Receiver<Result<MatchResult, String>>, Arc<DocState>),
}

struct Job {
    work: Work,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    what: &'static str,
}

pub struct MatchUi {
    pub target: Target,
    preset: Option<usize>,
    /// The last scan, and the name of the file it measured.
    scan: Option<(String, Loudness)>,
    job: Option<Job>,
    status: Option<(bool, String)>,
    /// The Before and After measures of the last Run, drawn as a table after Run.
    result: Option<(Loudness, Loudness)>,
    finished: Option<Finished>,
}

/// One row of the Before and After table: a measure and its two values, as drawn.
#[derive(Debug, PartialEq)]
pub struct ResultRow {
    pub label: &'static str,
    pub before: String,
    pub after: String,
}

/// The three rows of the result table, in order: integrated loudness, loudness range, true peak.
/// A value that is not finite (silence) reads "n/a".
pub fn result_rows(before: &Loudness, after: &Loudness) -> Vec<ResultRow> {
    fn shown(x: f64, unit: &str) -> String {
        if x.is_finite() { format!("{x:.1}{unit}") } else { "n/a".to_string() }
    }
    vec![
        ResultRow { label: "Integrated loudness", before: shown(before.integrated_lufs, " LUFS"), after: shown(after.integrated_lufs, " LUFS") },
        ResultRow { label: "Loudness range", before: shown(before.range_lu, " LU"), after: shown(after.range_lu, " LU") },
        ResultRow { label: "True peak", before: shown(before.true_peak_dbtp, " dBTP"), after: shown(after.true_peak_dbtp, " dBTP") },
    ]
}

impl MatchUi {
    pub fn new() -> MatchUi {
        MatchUi { target: Target::default(), preset: Some(0), scan: None, job: None, status: None, result: None, finished: None }
    }

    /// A new file, or the audio changed: the old scan means nothing now.
    pub fn new_file(&mut self) {
        self.scan = None;
        self.status = None;
        self.result = None;
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The audio changed under the scan (an edit, undo or redo).
    pub fn audio_changed(&mut self) {
        self.scan = None;
        self.result = None;
    }

    pub fn take_finished(&mut self) -> Option<Finished> {
        self.poll();
        self.finished.take()
    }

    pub fn say(&mut self, ok: bool, text: String) {
        self.status = Some((ok, text));
    }

    /// After a Run: show its Before and After measures as the table, in place of the status line.
    pub fn show_result(&mut self, before: Loudness, after: Loudness) {
        self.status = None;
        self.result = Some((before, after));
    }

    /// Scan `state` on a worker.
    pub fn start_scan(&mut self, state: Arc<DocState>, name: String) {
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (c, p) = (cancel.clone(), progress.clone());
        let n = name.clone();
        self.status = None;
        self.result = None;
        if std::thread::Builder::new()
            .name("match-scan".into())
            .spawn(move || {
                let len = state.len_frames();
                let _ = tx.send(loudness::measure(&state, 0..len, &c, &mut |f| p.store((f * 1000.0) as u32, Ordering::Relaxed)));
            })
            .is_ok()
        {
            self.job = Some(Job { work: Work::Scan(rx), cancel, progress, what: "Scanning" });
            self.scan = Some((n, Loudness { frames: 0, ..EMPTY }));
        }
    }

    /// Run the match on `state` on a worker.
    pub fn start_run(&mut self, state: Arc<DocState>) {
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (c, p, target, base) = (cancel.clone(), progress.clone(), self.target, state.clone());
        let before = self.scan.as_ref().map(|(_, m)| *m).filter(|m| m.frames == state.len_frames());
        self.status = None;
        self.result = None;
        if std::thread::Builder::new()
            .name("match-run".into())
            .spawn(move || {
                let r = match_loudness::run(&state, &target, before, &c, &mut |f| p.store((f * 1000.0) as u32, Ordering::Relaxed));
                let _ = tx.send(r.map_err(|e| e.to_string()));
            })
            .is_ok()
        {
            self.job = Some(Job { work: Work::Run(rx, base), cancel, progress, what: "Matching" });
        }
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match &job.work {
            Work::Scan(rx) => {
                let outcome = match rx.try_recv() {
                    Ok(r) => r,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err(io::Error::other("the scan stopped unexpectedly")),
                };
                self.job = None;
                match outcome {
                    Ok(m) => {
                        if let Some((_, slot)) = &mut self.scan {
                            *slot = m;
                        }
                    }
                    Err(e) => {
                        self.scan = None;
                        self.status =
                            Some((false, if e.kind() == io::ErrorKind::Interrupted { "Scan cancelled".to_string() } else { e.to_string() }));
                    }
                }
            }
            Work::Run(rx, base) => {
                let outcome = match rx.try_recv() {
                    Ok(r) => r,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err("the match stopped unexpectedly".to_string()),
                };
                let base = base.clone();
                self.job = None;
                match outcome {
                    Ok(result) => self.finished = Some(Finished { base, result }),
                    Err(e) => self.status = Some((false, e)),
                }
            }
        }
    }

    pub fn animating(&self) -> bool {
        self.job.is_some()
    }

    /// The panel's body. `file` is the open document's name and state.
    pub fn panel(&mut self, ui: &mut egui::Ui, file: Option<(String, Arc<DocState>)>) {
        self.poll();
        {
            ui.label("Files");
            match &file {
                Some((name, _)) => {
                    let line = match self.scan.as_ref().filter(|(n, m)| n == name && m.frames > 0) {
                        Some((_, m)) => format!("{name}   {:.1} LUFS   {:.1} LU range   {:.1} dBTP", m.integrated_lufs, m.range_lu, m.true_peak_dbtp),
                        None => format!("{name}   (not scanned)"),
                    };
                    ui.monospace(line);
                }
                None => {
                    ui.label("No file is open.");
                }
            }
            ui.separator();
            egui::Grid::new("match-target").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                let mut changed = false;
                ui.label("Match to");
                let names = match_to_choices();
                let selected = match_to_name(self.target.match_to);
                egui::ComboBox::from_id_salt("match-to").selected_text(selected).show_ui(ui, |ui| {
                    for (name, (_, m)) in names.iter().zip(MATCH_TO.iter()) {
                        if ui.selectable_label(self.target.match_to == *m, *name).clicked() {
                            self.target.match_to = *m;
                            changed = true;
                        }
                    }
                });
                ui.end_row();
                ui.label("Preset");
                let current = self.preset.map_or("Custom", |i| PRESETS[i].0);
                egui::ComboBox::from_id_salt("match-preset").selected_text(current).show_ui(ui, |ui| {
                    for (i, (name, t)) in PRESETS.iter().enumerate() {
                        if ui.selectable_label(self.preset == Some(i), *name).clicked() {
                            self.target = *t;
                            self.preset = Some(i);
                        }
                    }
                });
                ui.end_row();
                ui.label("Target loudness");
                changed |= ui.add(egui::DragValue::new(&mut self.target.lufs).range(-70.0..=0.0).speed(0.1).suffix(" LUFS")).changed();
                ui.end_row();
                ui.label("Tolerance");
                changed |= ui.add(egui::DragValue::new(&mut self.target.tolerance_lu).range(0.0..=5.0).speed(0.05).suffix(" LU")).changed();
                ui.end_row();
                ui.label("Maximum true peak");
                changed |= ui.add(egui::DragValue::new(&mut self.target.max_true_peak_dbtp).range(-30.0..=0.0).speed(0.1).suffix(" dBTP")).changed();
                ui.end_row();
                ui.label("Use true peak limiting");
                changed |= ui.checkbox(&mut self.target.limit, "").changed();
                ui.end_row();
                if self.target.limit {
                    ui.label("Look-ahead");
                    changed |= ui.add(egui::DragValue::new(&mut self.target.look_ahead_ms).range(1.0..=50.0).speed(0.5).suffix(" ms")).changed();
                    ui.end_row();
                    ui.label("Release");
                    changed |= ui.add(egui::DragValue::new(&mut self.target.release_ms).range(10.0..=2000.0).speed(5.0).suffix(" ms")).changed();
                    ui.end_row();
                }
                if changed {
                    self.preset = PRESETS.iter().position(|(_, t)| *t == self.target);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let can = file.is_some() && self.job.is_none();
                if ui.add_enabled(can, egui::Button::new("Scan")).clicked()
                    && let Some((name, state)) = file.clone()
                {
                    self.start_scan(state, name);
                }
                if ui.add_enabled(can, egui::Button::new("Run")).on_hover_text("One undoable step").clicked()
                    && let Some((_, state)) = file.clone()
                {
                    self.start_run(state);
                }
            });
            if let Some(j) = &self.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("{}… {}%", j.what, j.progress.load(Ordering::Relaxed) / 10));
                    if ui.button("Cancel").clicked() {
                        j.cancel.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Some((before, after)) = &self.result {
                egui::Grid::new("match-result").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label("Before");
                    ui.label("After");
                    ui.end_row();
                    for row in result_rows(before, after) {
                        ui.label(row.label);
                        ui.monospace(row.before);
                        ui.monospace(row.after);
                        ui.end_row();
                    }
                });
            } else if let Some((ok, text)) = &self.status {
                if *ok {
                    ui.label(text);
                } else {
                    ui.colored_label(theme::pal().alert, text);
                }
            }
        }
    }

    /// Check a running scan or run without drawing (the panel may be hidden).
    pub fn tick(&mut self) {
        self.poll();
    }
}

/// The Match To choices' names in panel order (the first is the default). The Match To combo lists these.
fn match_to_choices() -> Vec<&'static str> {
    MATCH_TO.iter().map(|(n, _)| *n).collect()
}

/// The text the Match To combo shows for `t`: that choice's name from `MATCH_TO`.
fn match_to_name(t: MatchTo) -> &'static str {
    MATCH_TO.iter().find(|(_, m)| *m == t).map_or("", |(n, _)| *n)
}

const EMPTY: Loudness = Loudness {
    frames: 0,
    integrated_lufs: f64::NEG_INFINITY,
    range_lu: 0.0,
    true_peak_dbtp: f64::NEG_INFINITY,
    sample_peak_dbfs: f64::NEG_INFINITY,
    max_momentary_lufs: f64::NEG_INFINITY,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(integrated_lufs: f64, range_lu: f64, true_peak_dbtp: f64) -> Loudness {
        Loudness { frames: 48000, integrated_lufs, range_lu, true_peak_dbtp, ..EMPTY }
    }

    #[test]
    fn g3_result_rows_show_before_and_after() {
        let before = loud(-23.0, 6.0, -5.0);
        let after = loud(-16.0, 6.0, -1.0);
        let rows = result_rows(&before, &after);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].label, "Integrated loudness");
        assert_eq!(rows[0].before, "-23.0 LUFS");
        assert_eq!(rows[0].after, "-16.0 LUFS");
        assert_eq!(rows[1].before, "6.0 LU");
        assert_eq!(rows[1].after, "6.0 LU");
        assert_eq!(rows[2].before, "-5.0 dBTP");
        assert_eq!(rows[2].after, "-1.0 dBTP");
    }

    #[test]
    fn g3_result_rows_show_na_for_silence() {
        let after = loud(-16.0, 6.0, -1.0);
        let rows = result_rows(&EMPTY, &after);
        assert_eq!(rows[0].before, "n/a");
        assert_eq!(rows[2].before, "n/a");
        assert_eq!(rows[0].after, "-16.0 LUFS");
    }

    #[test]
    fn g3_panel_offers_three_match_to_choices() {
        assert_eq!(match_to_choices(), vec!["ITU-R BS.1770-3 Loudness", "EBU R128 Loudness", "True Peak Amplitude"]);
        assert_eq!(MatchUi::new().target.match_to, MatchTo::Bs1770);
        assert_eq!(MATCH_TO[0].1, MatchTo::Bs1770);
        // The combo's shown text is each choice's own name, so it changes with the choice (a fixed label would fail here).
        for (name, m) in MATCH_TO.iter() {
            assert_eq!(match_to_name(*m), *name);
        }
    }
}
