//! The Diagnostics window (B050): choose DeClipper or DeClicker and its settings, scan for what
//! it would fix (on a worker thread, cancellable), look at the list, then repair chosen rows or
//! all of them. The window only holds state and draws; the app runs the repair so it lands on
//! the history as one undo step named for the tool.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::Align2;
use wavcraft_core::DocState;
use wavcraft_core::diagnostics::{Region, Tool, declick, declip};
use wavcraft_core::doc::edit::{ChannelMask, EditError};

/// What the user asked for this frame.
#[derive(Debug, PartialEq)]
pub enum Ask {
    Scan,
    Cancel,
    /// Move the cursor to this region.
    Goto(Region),
    /// Repair these rows (indices into the list).
    Repair(Vec<usize>),
}

/// A finished scan, tied to the document it read.
pub struct Scanned {
    tool: Tool,
    doc: Arc<DocState>,
    regions: Vec<Region>,
    skipped: Vec<bool>,
    truncated: bool,
}

struct Running {
    rx: Receiver<Result<Scanned, String>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct DiagPanel {
    pub open: bool,
    /// 0 DeClipper, 1 DeClicker.
    kind: usize,
    declip: [f64; 2],
    declick: [f64; 2],
    running: Option<Running>,
    found: Option<Scanned>,
    selected: BTreeSet<usize>,
    /// A control-channel scan waiting for its answer.
    pub reply: Option<Sender<String>>,
    message: String,
}

/// `declip|declick [key=value...]` as the tool with those settings.
pub fn parse_tool(args: &str) -> Result<Tool, String> {
    let (name, rest) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));
    match name.to_ascii_lowercase().as_str() {
        "declip" => {
            let v = declip::Params::parse(rest)?;
            Ok(Tool::DeClip(declip::Params { threshold_db: v[0], min_run: v[1] as u32 }))
        }
        "declick" => {
            let v = declick::Params::parse(rest)?;
            Ok(Tool::DeClick(declick::Params::from_list(&v)))
        }
        _ => Err(format!("{name:?}: write declip or declick")),
    }
}

/// `all` or `i,j,k` as row indices below `count`.
pub fn parse_rows(args: &str, count: usize) -> Result<Vec<usize>, String> {
    let args = args.trim();
    if args.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    let mut rows = BTreeSet::new();
    for word in args.split(',') {
        let i: usize = word.trim().parse().map_err(|_| format!("{word:?} is not a row number"))?;
        if i >= count {
            return Err(format!("row {i} is not in the list of {count}"));
        }
        rows.insert(i);
    }
    Ok(rows.into_iter().collect())
}

impl DiagPanel {
    pub fn new() -> DiagPanel {
        let (d, c) = (declip::Params::default(), declick::Params::default());
        DiagPanel { declip: [d.threshold_db, f64::from(d.min_run)], declick: [c.sensitivity, c.max_width_ms], ..DiagPanel::default() }
    }

    pub fn scanning(&self) -> bool {
        self.running.is_some()
    }

    /// The tool as the window's settings have it.
    pub fn tool(&self) -> Tool {
        if self.kind == 0 {
            Tool::DeClip(declip::Params { threshold_db: self.declip[0], min_run: self.declip[1] as u32 })
        } else {
            Tool::DeClick(declick::Params::from_list(&self.declick))
        }
    }

    /// Show `tool` with its settings in the window (the control channel's scan).
    pub fn set_tool(&mut self, tool: Tool) {
        match tool {
            Tool::DeClip(p) => {
                self.kind = 0;
                self.declip = [p.threshold_db, f64::from(p.min_run)];
            }
            Tool::DeClick(p) => {
                self.kind = 1;
                self.declick = [p.sensitivity, p.max_width_ms];
            }
        }
    }

    /// Scan `range` of `channels` of `doc` on a worker thread. Any scan under way is cancelled.
    pub fn start_scan(&mut self, tool: Tool, doc: Arc<DocState>, range: Range<u64>, channels: ChannelMask, ctx: &egui::Context) {
        self.cancel();
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("diagnostics scan".into()).spawn(move || {
            let run = || -> Result<Scanned, EditError> {
                let found = tool.scan(&doc, range, channels, &flag)?;
                let skipped = tool.skipped(&doc, &found.regions, &flag)?;
                Ok(Scanned { tool, doc: doc.clone(), regions: found.regions, skipped, truncated: found.truncated })
            };
            let _ = tx.send(run().map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        if spawned.is_err() {
            self.message = "Could not start the scan".to_string();
            return;
        }
        self.found = None;
        self.selected.clear();
        self.message.clear();
        self.running = Some(Running { rx, cancel });
    }

    pub fn cancel(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Take a finished scan in; a control-channel scan waiting for it gets its answer.
    pub fn poll(&mut self) {
        let Some(r) = self.running.as_ref() else { return };
        let result = match r.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the scan stopped unexpectedly".to_string()),
        };
        self.running = None;
        let answer = match result {
            Ok(s) => {
                let skipped = s.skipped.iter().filter(|&&x| x).count();
                let a = format!("OK regions={} skipped={skipped}", s.regions.len());
                self.message.clear();
                self.found = Some(s);
                a
            }
            Err(why) => {
                self.message = format!("Scan: {why}");
                format!("ERR {}", self.message)
            }
        };
        if let Some(tx) = self.reply.take() {
            let _ = tx.send(answer);
        }
    }

    pub fn region_count(&self) -> usize {
        self.found.as_ref().map_or(0, |s| s.regions.len())
    }

    /// The tool, document and regions for `rows`, or why they cannot be repaired now.
    pub fn plan(&self, rows: &[usize], current: &Arc<DocState>) -> Result<(Tool, Arc<DocState>, Vec<Region>), String> {
        let Some(s) = &self.found else { return Err("scan first".to_string()) };
        if !Arc::ptr_eq(&s.doc, current) {
            return Err("the list is stale: the file changed since the scan; scan again".to_string());
        }
        let regions: Vec<Region> = rows.iter().filter_map(|&i| s.regions.get(i).copied()).collect();
        if regions.is_empty() {
            return Err("no rows chosen".to_string());
        }
        Ok((s.tool, s.doc.clone(), regions))
    }

    pub fn say(&mut self, text: String) {
        self.message = text;
    }

    /// Draw the window. `current` is the open document (`None`: no file), `rate` its sample rate.
    pub fn ui(&mut self, ctx: &egui::Context, current: Option<&Arc<DocState>>, busy: bool) -> Option<Ask> {
        if !self.open {
            return None;
        }
        let mut ask = None;
        let mut open = true;
        egui::Window::new("Diagnostics").open(&mut open).default_width(420.0).anchor(Align2::RIGHT_TOP, egui::vec2(-8.0, 40.0)).show(ctx, |ui| {
            let scanning = self.running.is_some();
            ui.add_enabled_ui(!scanning, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.kind, 0, "DeClipper");
                    ui.selectable_value(&mut self.kind, 1, "DeClicker");
                });
                if self.kind == 0 {
                    ui.add(
                        egui::Slider::new(&mut self.declip[0], declip::Params::THRESHOLD_DB.start..=declip::Params::THRESHOLD_DB.end)
                            .text("Threshold dB"),
                    );
                    let mut run = self.declip[1] as u32;
                    ui.add(egui::Slider::new(&mut run, declip::Params::MIN_RUN.start..=declip::Params::MIN_RUN.end - 1).text("Minimum run"));
                    self.declip[1] = f64::from(run);
                } else {
                    ui.add(
                        egui::Slider::new(&mut self.declick[0], declick::Params::SENSITIVITY.start..=declick::Params::SENSITIVITY.end)
                            .text("Sensitivity %"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.declick[1], declick::Params::MAX_WIDTH_MS.start..=declick::Params::MAX_WIDTH_MS.end)
                            .text("Max width ms"),
                    );
                }
            });
            ui.horizontal(|ui| {
                if scanning {
                    ui.spinner();
                    ui.label("Scanning...");
                    if ui.button("Cancel").clicked() {
                        ask = Some(Ask::Cancel);
                    }
                } else if ui.add_enabled(current.is_some() && !busy, egui::Button::new("Scan")).clicked() {
                    ask = Some(Ask::Scan);
                }
            });
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            let Some(found) = &self.found else { return };
            let stale = current.is_none_or(|c| !Arc::ptr_eq(&found.doc, c));
            let skipped = found.skipped.iter().filter(|&&x| x).count();
            ui.separator();
            ui.label(format!("{} regions found, {skipped} would be skipped (too long, or at a file edge)", found.regions.len()));
            if found.truncated {
                ui.colored_label(egui::Color32::YELLOW, "The scan stopped at its limit: repair, then scan again for the rest.");
            }
            if stale {
                ui.colored_label(egui::Color32::LIGHT_RED, "Stale: the file changed since this scan. Scan again.");
            }
            let rate = f64::from(found.doc.sample_rate());
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("diag_rows").striped(true).show(ui, |ui| {
                    ui.strong("Time");
                    ui.strong("Ch");
                    ui.strong("Length");
                    ui.strong("Peak");
                    ui.strong("");
                    ui.end_row();
                    for (i, r) in found.regions.iter().enumerate() {
                        let mut on = self.selected.contains(&i);
                        let time = format!("{:.4} s", r.start as f64 / rate);
                        let resp = ui.selectable_label(on, time);
                        if resp.clicked() {
                            on = !on;
                            ask = Some(Ask::Goto(*r));
                        }
                        if on {
                            self.selected.insert(i);
                        } else {
                            self.selected.remove(&i);
                        }
                        ui.label(r.channel.to_string());
                        ui.label(format!("{} fr", r.len));
                        ui.label(format!("{:.1} dB", 20.0 * f64::from(r.peak.max(1e-9)).log10()));
                        ui.label(if found.skipped[i] { "skipped" } else { "" });
                        ui.end_row();
                    }
                });
            });
            ui.horizontal(|ui| {
                let can = !stale && !busy && !scanning && !found.regions.is_empty();
                if ui.add_enabled(can && !self.selected.is_empty(), egui::Button::new("Repair selected")).clicked() {
                    ask = Some(Ask::Repair(self.selected.iter().copied().collect()));
                }
                if ui.add_enabled(can, egui::Button::new("Repair All")).clicked() {
                    ask = Some(Ask::Repair((0..found.regions.len()).collect()));
                }
            });
        });
        self.open = open;
        ask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_and_rows_parse() {
        assert!(matches!(parse_tool("declip threshold_db=-1 min_run=4"), Ok(Tool::DeClip(p)) if p.min_run == 4));
        assert!(matches!(parse_tool("DECLICK sensitivity=70"), Ok(Tool::DeClick(p)) if p.sensitivity == 70.0));
        assert!(parse_tool("declip min_run=1").is_err());
        assert!(parse_tool("normalize").is_err());
        assert_eq!(parse_rows("all", 3), Ok(vec![0, 1, 2]));
        assert_eq!(parse_rows("2, 0,2", 3), Ok(vec![0, 2]));
        assert!(parse_rows("3", 3).is_err());
        assert!(parse_rows("x", 3).is_err());
    }
}
