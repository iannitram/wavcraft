//! The Effects Rack panel (f31): a window with the rack's slots (effect, power, settings), the
//! master power, input and output gain and Mix, rack and effect presets, Preview and Apply. It
//! edits a [`RackConfig`]; the app pushes that to the player and runs Apply.

use egui::Align2;
use wavcraft_core::effects;
use wavcraft_core::presets::{Presets, RACK};
use wavcraft_core::rack::{GAIN_DB, RackConfig};

use crate::effect_dialog::EffectDialog;

/// What the panel asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Open the settings dialog for this slot.
    EditSlot(usize),
    /// Start or stop playback (the preview: play with the rack on).
    TogglePlay,
    /// Process the selection (or the file) through the rack.
    Apply,
}

pub struct RackPanel {
    pub config: RackConfig,
    pub open: bool,
    /// The config last handed to the player.
    pub pushed: Option<RackConfig>,
    /// The name typed for saving a preset.
    name: String,
    /// What the last preset action said.
    note: String,
    presets: Option<Presets>,
    pub requests: Vec<Request>,
    /// A VST3 scan running in the background (f128).
    scan: Option<std::sync::mpsc::Receiver<wavcraft_core::vst3::ScanReport>>,
    /// The last finished scan's skipped bundles.
    skipped: Vec<String>,
}

impl RackPanel {
    pub fn new() -> RackPanel {
        RackPanel {
            config: RackConfig::new(),
            open: false,
            pushed: None,
            name: String::new(),
            note: String::new(),
            presets: Presets::default_location(),
            requests: Vec::new(),
            scan: None,
            skipped: Vec::new(),
        }
    }

    /// The folders to scan for VST3 plugins: `WAVCRAFT_VST3_PATH` (a list, for tests) or the
    /// standard ones.
    fn vst3_dirs() -> Vec<std::path::PathBuf> {
        match std::env::var_os("WAVCRAFT_VST3_PATH") {
            Some(v) => std::env::split_paths(&v).collect(),
            None => wavcraft_core::vst3::default_dirs(),
        }
    }

    /// Start scanning in the background (worker processes, one per bundle, with a timeout).
    pub fn start_scan(&mut self) -> Result<(), String> {
        if self.scan.is_some() {
            return Err("a scan is already running".to_string());
        }
        let exe = std::env::current_exe().map_err(|e| format!("cannot find the app's own executable: {e}"))?;
        let dirs = Self::vst3_dirs();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(wavcraft_core::vst3::scan_with(&exe, &dirs, wavcraft_core::vst3::BUNDLE_TIMEOUT));
        });
        self.scan = Some(rx);
        Ok(())
    }

    /// Take a finished scan's result, if there is one.
    pub fn poll_scan(&mut self) {
        let Some(rx) = &self.scan else { return };
        match rx.try_recv() {
            Ok(report) => {
                log::info!("VST3 scan: {} plugin(s), {} skipped", report.plugins.len(), report.skipped.len());
                for s in &report.skipped {
                    log::warn!("VST3 scan skipped {s}");
                }
                wavcraft_core::vst3::set_found(report.plugins);
                self.skipped = report.skipped;
                self.scan = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.scan = None,
        }
    }

    /// `VST3 scan` starts a scan; `VST3 list` says whether one is running, then the plugins found.
    pub fn vst3_command(&mut self, rest: &str) -> String {
        self.poll_scan();
        match rest.trim() {
            "scan" => match self.start_scan() {
                Ok(()) => "OK scanning=1".to_string(),
                Err(e) => format!("ERR {e}"),
            },
            "list" | "" => {
                let found = wavcraft_core::vst3::found();
                let names: Vec<String> = found.iter().map(|f| format!("{} [{}]", f.name, f.vendor)).collect();
                format!(
                    "OK scanning={} count={} skipped={} plugins={}",
                    u8::from(self.scan.is_some()),
                    found.len(),
                    self.skipped.len(),
                    names.join("; ")
                )
            }
            _ => "ERR VST3 scan|list".to_string(),
        }
    }

    fn presets(&self) -> Result<&Presets, String> {
        self.presets.as_ref().ok_or_else(|| "there is no config folder for presets on this machine".to_string())
    }

    /// Draw the window. `playing` labels the Preview button; `can_apply` enables Apply.
    pub fn show(&mut self, ctx: &egui::Context, playing: bool, can_apply: bool) {
        self.poll_scan();
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Effects Rack").open(&mut open).resizable(false).anchor(Align2::LEFT_TOP, egui::vec2(12.0, 40.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.power, "Power");
                if ui.button(if playing { "■ Stop" } else { "▶ Preview" }).on_hover_text("Play with the rack on").clicked() {
                    self.requests.push(Request::TogglePlay);
                }
                if ui
                    .add_enabled(can_apply, egui::Button::new("Apply"))
                    .on_hover_text("Process the selection (the whole file when none) as one step, then reset the rack to bypass")
                    .clicked()
                {
                    self.requests.push(Request::Apply);
                }
            });
            egui::Grid::new("rack_levels").num_columns(2).show(ui, |ui| {
                ui.label("Input");
                ui.add(egui::Slider::new(&mut self.config.input_db, GAIN_DB.0..=GAIN_DB.1).suffix(" dB").clamping(egui::SliderClamping::Always));
                ui.end_row();
                ui.label("Output");
                ui.add(egui::Slider::new(&mut self.config.output_db, GAIN_DB.0..=GAIN_DB.1).suffix(" dB").clamping(egui::SliderClamping::Always));
                ui.end_row();
                ui.label("Mix");
                ui.add(egui::Slider::new(&mut self.config.mix_pct, 0.0..=100.0).suffix(" %").clamping(egui::SliderClamping::Always));
                ui.end_row();
            });
            ui.separator();
            egui::Grid::new("rack_slots").num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
                for i in 0..self.config.slots.len() {
                    self.slot_row(ui, i);
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let scanning = self.scan.is_some();
                if ui
                    .add_enabled(!scanning, egui::Button::new("Scan VST3"))
                    .on_hover_text("Look for VST3 effects in the standard folders (in separate processes)")
                    .clicked()
                {
                    self.note = match self.start_scan() {
                        Ok(()) => "Scanning for VST3 plugins…".to_string(),
                        Err(e) => e,
                    };
                }
                ui.label(if scanning { "scanning…".to_string() } else { format!("{} VST3 found", wavcraft_core::vst3::found().len()) });
            });
            ui.horizontal(|ui| {
                ui.label("Preset");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(120.0).hint_text("name"));
                if ui.button("Save rack").clicked() {
                    self.note = self.preset_cmd("save", RACK, &self.name.clone(), &mut None);
                }
                ui.menu_button("Load rack", |ui| {
                    let names = self.presets().and_then(|p| p.list(RACK)).unwrap_or_default();
                    if names.is_empty() {
                        ui.label("No saved racks");
                    }
                    for n in names {
                        if ui.button(&n).clicked() {
                            self.note = self.preset_cmd("load", RACK, &n, &mut None);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Delete rack", |ui| {
                    for n in self.presets().and_then(|p| p.list(RACK)).unwrap_or_default() {
                        if ui.button(&n).clicked() {
                            self.note = self.preset_cmd("delete", RACK, &n, &mut None);
                            ui.close();
                        }
                    }
                });
            });
            if !self.note.is_empty() {
                ui.weak(&self.note);
            }
        });
        self.open = open;
    }

    fn slot_row(&mut self, ui: &mut egui::Ui, i: usize) {
        ui.label(format!("{}", i + 1));
        ui.checkbox(&mut self.config.slots[i].enabled, "");
        let current = self.config.slots[i].effect.clone();
        let shown =
            current.as_deref().map_or("(none)", |c| effects::find(c).map_or(c.strip_prefix(wavcraft_core::vst3::PREFIX).unwrap_or(c), |e| e.name()));
        let plugins = wavcraft_core::vst3::found();
        let mut pick = None;
        egui::ComboBox::from_id_salt(("rack_slot", i)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "(none)").clicked() {
                pick = Some(None);
            }
            for e in effects::all() {
                if ui.selectable_label(current.as_deref() == Some(e.id()), e.name()).clicked() {
                    pick = Some(Some(e.id().to_string()));
                }
            }
            if !plugins.is_empty() {
                ui.separator();
                ui.label("VST3 plugins");
            }
            for p in &plugins {
                let spec = format!("{}{}", wavcraft_core::vst3::PREFIX, p.name);
                if ui.selectable_label(current.as_deref() == Some(spec.as_str()), &p.name).clicked() {
                    pick = Some(Some(spec));
                }
            }
        });
        match pick {
            Some(None) => {
                let _ = self.config.clear_slot(i);
            }
            Some(Some(id)) if current.as_deref() != Some(id.as_str()) => {
                if let Err(e) = self.config.set_slot(i, &id, None) {
                    self.note = format!("ERR {e}");
                }
            }
            _ => {}
        }
        let effect = self.config.slots[i].effect.as_deref().and_then(effects::find);
        if ui.add_enabled(effect.is_some_and(|e| !e.parameters().is_empty()), egui::Button::new("Settings…")).clicked() {
            self.requests.push(Request::EditSlot(i));
        }
        match effect {
            Some(e) => {
                ui.menu_button("Presets", |ui| {
                    if ui.button("Save as the name above").clicked() {
                        let params = self.config.slots[i].params.clone();
                        self.note = match self.presets().and_then(|p| p.save_effect(e.id(), &self.name, &params)) {
                            Ok(()) => format!("saved {} preset {:?}", e.name(), self.name),
                            Err(why) => why,
                        };
                        ui.close();
                    }
                    ui.separator();
                    for n in self.presets().and_then(|p| p.list(e.id())).unwrap_or_default() {
                        if ui.button(&n).clicked() {
                            match self.presets().and_then(|p| p.load_effect(e.id(), &n)) {
                                Ok(v) => {
                                    self.config.slots[i].params = v;
                                    self.note = format!("loaded {} preset {n:?}", e.name());
                                }
                                Err(why) => self.note = why,
                            }
                            ui.close();
                        }
                    }
                });
            }
            None => {
                ui.label("");
            }
        }
    }

    /// Store the settings from a closed dialog into the slot it was opened for.
    pub fn set_slot_params(&mut self, slot: usize, index: u16, values: Vec<f64>) {
        if let Some(e) = effects::all().get(usize::from(index))
            && self.config.slots.get(slot).and_then(|s| s.effect.as_deref()) == Some(e.id())
        {
            self.config.slots[slot].params = values;
        }
    }

    /// The dialog for slot `slot`, on its current settings.
    pub fn dialog_for(&self, slot: usize) -> Option<EffectDialog> {
        let s = self.config.slots.get(slot)?;
        let index = effects::all().iter().position(|e| Some(e.id()) == s.effect.as_deref())?;
        let mut d = EffectDialog::new(index as u16, Some(s.params.clone()))?;
        d.rack_slot = Some(slot);
        Some(d)
    }

    /// `RACK` without Apply (the app handles that): returns the reply line.
    pub fn command(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        let result = match words.next() {
            None => Ok(()),
            Some("set") => self.cmd_set(rest.trim_start().strip_prefix("set").unwrap_or("").trim_start()),
            Some("power") => match (words.next(), words.next()) {
                (Some(which), Some(state @ ("on" | "off"))) => {
                    let on = state == "on";
                    if which == "master" {
                        self.config.power = on;
                        Ok(())
                    } else {
                        slot_number(which).and_then(|n| self.config.set_slot_power(n, on))
                    }
                }
                _ => Err("RACK power <slot|master> on|off".to_string()),
            },
            Some(key @ ("in" | "out" | "mix")) => match words.next().map(str::parse::<f64>) {
                Some(Ok(v)) => match key {
                    "in" => self.config.set_input_db(v),
                    "out" => self.config.set_output_db(v),
                    _ => self.config.set_mix(v),
                },
                _ => Err(format!("RACK {key} <number>")),
            },
            Some(other) => Err(format!("unknown RACK command {other:?}")),
        };
        match result {
            Ok(()) => format!("OK {}", self.config.describe()),
            Err(why) => format!("ERR {why}"),
        }
    }

    fn cmd_set(&mut self, args: &str) -> Result<(), String> {
        let (slot, rest) = args.split_once(char::is_whitespace).ok_or("RACK set <slot> <effect|none> [key=value ...]")?;
        let slot = slot_number(slot)?;
        if rest.trim().starts_with(wavcraft_core::vst3::PREFIX) {
            // `vst3:<name>`: a hosted plugin; the name may have spaces.
            return self.config.set_slot(slot, rest.trim(), None);
        }
        let (name, params) = rest.trim().split_once(char::is_whitespace).unwrap_or((rest.trim(), ""));
        let name = name.strip_prefix("effect.").unwrap_or(name);
        if name == "none" || name == "-" {
            return self.config.clear_slot(slot);
        }
        let e = effects::find(name).ok_or_else(|| format!("no effect named {name:?}"))?;
        let values = effects::parse_params(e, params)?;
        self.config.set_slot(slot, name, Some(values))
    }

    /// `PRESET save|load|delete|list <effect|rack> [name]`, against the rack and the open dialog.
    pub fn preset(&mut self, rest: &str, dialog: &mut Option<EffectDialog>) -> String {
        let mut w = rest.trim().splitn(3, char::is_whitespace);
        let (Some(verb), Some(kind)) = (w.next(), w.next()) else { return "ERR PRESET save|load|delete|list <effect|rack> [name]".to_string() };
        let name = w.next().unwrap_or("").trim().to_string();
        let kind = kind.strip_prefix("effect.").unwrap_or(kind);
        let reply = self.preset_cmd(verb, kind, &name, dialog);
        self.note = reply.clone();
        reply
    }

    fn preset_cmd(&mut self, verb: &str, kind: &str, name: &str, dialog: &mut Option<EffectDialog>) -> String {
        let r: Result<String, String> = (|| {
            let presets = self.presets()?.clone();
            match (verb, kind) {
                ("list", k) => Ok(format!("OK {}", presets.list(k)?.join("; "))),
                ("delete", k) => presets.delete(k, name).map(|()| format!("OK deleted {name:?}")),
                ("save", RACK) => presets.save_rack(name, &self.config).map(|()| format!("OK saved rack {name:?}")),
                ("load", RACK) => {
                    self.config = presets.load_rack(name)?;
                    Ok(format!("OK loaded rack {name:?}"))
                }
                ("save", id) => {
                    let params = self.effect_settings(id, dialog.as_ref())?;
                    presets.save_effect(id, name, &params).map(|()| format!("OK saved {id} {name:?}"))
                }
                ("load", id) => {
                    let params = presets.load_effect(id, name)?;
                    self.put_effect_settings(id, params, dialog)?;
                    Ok(format!("OK loaded {id} {name:?}"))
                }
                _ => Err("PRESET save|load|delete|list <effect|rack> [name]".to_string()),
            }
        })();
        r.unwrap_or_else(|why| format!("ERR {why}"))
    }

    /// The settings of `id` to save: the open dialog's when it is for that effect, else the first
    /// rack slot holding it.
    fn effect_settings(&self, id: &str, dialog: Option<&EffectDialog>) -> Result<Vec<f64>, String> {
        if let Some(d) = dialog.filter(|d| d.effect().id() == id) {
            return Ok(d.values.clone());
        }
        self.config
            .slots
            .iter()
            .find(|s| s.effect.as_deref() == Some(id))
            .map(|s| s.params.clone())
            .ok_or_else(|| format!("{id} is not in the rack or an open dialog: nothing to save"))
    }

    /// Where loaded settings go: the open dialog when it is for that effect, else the first
    /// slot holding it, else the first empty slot.
    fn put_effect_settings(&mut self, id: &str, params: Vec<f64>, dialog: &mut Option<EffectDialog>) -> Result<(), String> {
        if let Some(d) = dialog.as_mut().filter(|d| d.effect().id() == id) {
            d.values = params;
            return Ok(());
        }
        let slot = self
            .config
            .slots
            .iter()
            .position(|s| s.effect.as_deref() == Some(id))
            .or_else(|| self.config.slots.iter().position(|s| s.effect.is_none()))
            .ok_or("the rack is full")?;
        self.config.set_slot(slot, id, Some(params))
    }
}

fn slot_number(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n - 1),
        _ => Err(format!("{text:?} is not a slot number (1 and up)")),
    }
}
