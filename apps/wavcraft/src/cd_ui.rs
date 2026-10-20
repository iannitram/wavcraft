//! The CD Layout view (f121): the track list with pause, title, performer and ISRC fields, the
//! total time against the disc limit, and Export (one WAV plus a CUE sheet). Also the control
//! verb `CD`, so a script can drive it. Burning a disc is not here (roadmap C243).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use egui::Color32;
use wavcraft_core::DocState;
use wavcraft_core::cd::{CdError, CdLayout, DiscLimit, format_msf, isrc_valid};
use wavcraft_core::io::OpenOptions;

use crate::multitrack_ui::tokens;

const RED: Color32 = Color32::from_rgb(235, 80, 70);

#[derive(Default)]
pub struct CdView {
    pub layout: CdLayout,
    /// The CD Layout is the view shown.
    pub show: bool,
    pub message: Option<String>,
    export_dir: String,
}

impl CdView {
    fn add_file(&mut self, path: &Path, sessions: &Path) -> Result<usize, CdError> {
        let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
        let state =
            wavcraft_core::io::open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).map_err(|e| CdError(format!("{}: {e}", path.display())))?;
        let name = path.file_stem().map_or_else(|| "track".to_string(), |n| n.to_string_lossy().into_owned());
        Ok(self.layout.add(Arc::new(state), &name))
    }

    fn export(&mut self, dir: &Path) -> Result<String, CdError> {
        let r = self.layout.export(dir, "disc", &mut |_| {}, &AtomicBool::new(false))?;
        Ok(format!("exported {} ({} CD frames) and {}", r.wav.display(), r.total_frames, r.cue.display()))
    }

    /// The view: toolbar, the track table, the total.
    pub fn ui(&mut self, ui: &mut egui::Ui, current: Option<Arc<DocState>>, doc_name: &str, sessions: &Path) {
        ui.horizontal(|ui| {
            ui.strong("CD Layout");
            ui.separator();
            if ui.button("Add File…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                self.message = self.add_file(&p, sessions).err().map(|e| e.to_string());
            }
            if ui.add_enabled(current.is_some(), egui::Button::new("Add Open Document")).clicked()
                && let Some(d) = current
            {
                self.layout.add(d, doc_name);
            }
            ui.separator();
            ui.label("Disc");
            ui.selectable_value(&mut self.layout.limit, DiscLimit::Min74, "74 min");
            ui.selectable_value(&mut self.layout.limit, DiscLimit::Min80, "80 min");
        });
        if let Some(m) = &self.message {
            ui.colored_label(RED, m);
        }
        ui.separator();
        let mut action: Option<(usize, i8)> = None; // (track, -1 up, 1 down, 0 remove)
        let places = self.layout.timeline();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("cd-tracks").striped(true).num_columns(8).show(ui, |ui| {
                for h in ["#", "Source", "Pause (s)", "Title", "Performer", "ISRC", "Start", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for (i, (t, p)) in self.layout.tracks.iter_mut().zip(&places).enumerate() {
                    ui.label((i + 1).to_string());
                    let src = ui.label(&t.name);
                    if let Some(c) = t.conversion() {
                        src.on_hover_text(format!("Converted on export: {c}"));
                        ui.colored_label(Color32::from_rgb(230, 170, 60), "converted");
                    }
                    ui.add(egui::DragValue::new(&mut t.pause_secs).range(0.0..=wavcraft_core::cd::MAX_PAUSE_SECS).speed(0.1).fixed_decimals(2));
                    ui.text_edit_singleline(&mut t.title);
                    ui.text_edit_singleline(&mut t.performer);
                    let bad = !t.isrc.is_empty() && !isrc_valid(&t.isrc);
                    let mut field = egui::TextEdit::singleline(&mut t.isrc).desired_width(120.0).hint_text("US-ABC-24-00001");
                    if bad {
                        field = field.text_color(RED);
                    }
                    ui.add(field).on_hover_text(if bad {
                        "Not an ISRC: 2 letters, 3 letters or digits, 2 digits, 5 digits"
                    } else {
                        "ISRC (optional)"
                    });
                    ui.label(format_msf(p.index01));
                    ui.horizontal(|ui| {
                        if ui.small_button("▲").clicked() {
                            action = Some((i, -1));
                        }
                        if ui.small_button("▼").clicked() {
                            action = Some((i, 1));
                        }
                        if ui.small_button("✕").clicked() {
                            action = Some((i, 0));
                        }
                    });
                    ui.end_row();
                }
            });
            if self.layout.tracks.is_empty() {
                ui.label("No tracks yet: Add File… or Add Open Document.");
            }
            ui.separator();
            let total = self.layout.total_frames();
            let text = format!("Total {} of {} minutes", format_msf(total), self.layout.limit.minutes());
            if self.layout.over_limit() {
                ui.colored_label(RED, format!("{text}: over the limit"));
            } else {
                ui.label(text);
            }
            for w in self.layout.warnings() {
                ui.colored_label(Color32::from_rgb(230, 170, 60), w);
            }
            ui.horizontal(|ui| {
                ui.label("Export to folder");
                ui.text_edit_singleline(&mut self.export_dir);
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.export_dir = p.display().to_string();
                }
                if ui.add_enabled(!self.export_dir.is_empty() && !self.layout.tracks.is_empty(), egui::Button::new("Export WAV + CUE")).clicked() {
                    let dir = PathBuf::from(&self.export_dir);
                    self.message = Some(self.export(&dir).unwrap_or_else(|e| format!("Export failed: {e}")));
                }
            });
            ui.label("Burning the disc is not part of WavCraft yet: a burning program takes the WAV and the CUE sheet.");
        });
        if let Some((i, a)) = action {
            let _ = match a {
                0 => self.layout.remove(i),
                -1 if i > 0 => self.layout.move_track(i, i - 1),
                1 if i + 1 < self.layout.tracks.len() => self.layout.move_track(i, i + 1),
                _ => Ok(()),
            };
        }
    }

    /// One `CD …` control line; the answer starts `OK` or `ERR`.
    pub fn control(&mut self, rest: &str, current: Option<Arc<DocState>>, sessions: &Path) -> String {
        let words = match tokens(rest) {
            Ok(w) => w,
            Err(e) => return format!("ERR {e}"),
        };
        match self.control_inner(&words, current, sessions) {
            Ok(s) => s,
            Err(e) => format!("ERR {e}"),
        }
    }

    fn control_inner(&mut self, w: &[String], current: Option<Arc<DocState>>, sessions: &Path) -> Result<String, CdError> {
        let word = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
        let usage = |u: &str| Err(CdError(format!("usage: {u}")));
        // Track numbers are 1-based, as in the list.
        let index = |s: &str| s.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).ok_or_else(|| CdError("<i> is a track number from 1".into()));
        match word(0) {
            "add" if w.len() == 2 => {
                self.add_file(Path::new(word(1)), sessions)?;
                Ok(format!("OK tracks={}", self.layout.tracks.len()))
            }
            "adddoc" => {
                let d = current.ok_or_else(|| CdError("no document is open".into()))?;
                self.layout.add(d, "document");
                Ok(format!("OK tracks={}", self.layout.tracks.len()))
            }
            "pause" if w.len() == 3 => {
                let secs: f64 = word(2).parse().map_err(|_| CdError("<s> is a number of seconds".into()))?;
                self.layout.set_pause(index(word(1))?, secs)?;
                Ok("OK".into())
            }
            "meta" if w.len() >= 3 => {
                let i = index(word(1))?;
                // A bad field puts the whole track back as it was.
                let mut pairs = Vec::new();
                for kv in &w[2..] {
                    pairs.push(kv.split_once('=').ok_or_else(|| CdError("fields are key=value (title, performer, isrc)".into()))?);
                }
                let backup = self.layout.tracks.get(i).cloned().ok_or_else(|| CdError(format!("no track {}", i + 1)))?;
                let result = pairs.iter().try_for_each(|(k, v)| self.layout.set_meta(i, k, v));
                if result.is_err() {
                    self.layout.tracks[i] = backup;
                }
                result?;
                Ok("OK".into())
            }
            "move" if w.len() == 3 => {
                self.layout.move_track(index(word(1))?, index(word(2))?)?;
                Ok("OK".into())
            }
            "remove" if w.len() == 2 => {
                self.layout.remove(index(word(1))?)?;
                Ok(format!("OK tracks={}", self.layout.tracks.len()))
            }
            "limit" if matches!(word(1), "74" | "80") => {
                self.layout.limit = if word(1) == "74" { DiscLimit::Min74 } else { DiscLimit::Min80 };
                Ok("OK".into())
            }
            "view" => {
                self.show = true;
                Ok("OK view=cd".into())
            }
            "state" => {
                let tracks: Vec<String> = self
                    .layout
                    .tracks
                    .iter()
                    .zip(self.layout.timeline())
                    .map(|(t, p)| {
                        format!(
                            "{{\"name\":{:?},\"title\":{:?},\"isrc\":{:?},\"pause\":{},\"index01\":\"{}\",\"frames\":{}}}",
                            t.name,
                            t.title,
                            t.isrc,
                            t.pause_secs,
                            format_msf(p.index01),
                            p.frames
                        )
                    })
                    .collect();
                Ok(format!(
                    "OK total={} frames={} limit={} over={} [{}]",
                    format_msf(self.layout.total_frames()),
                    self.layout.total_frames(),
                    self.layout.limit.minutes(),
                    self.layout.over_limit(),
                    tracks.join(",")
                ))
            }
            "export" if w.len() == 2 => self.export(Path::new(word(1))).map(|m| format!("OK {m}")),
            _ => usage(
                "CD add <path> | adddoc | pause <i> <s> | meta <i> title=.. performer=.. isrc=.. | move <i> <j> | remove <i> | limit 74|80 | view | state | export <dir>",
            ),
        }
    }
}
