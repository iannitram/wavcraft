//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::appearance;
use wavcraft_core::io::OpenOptions;
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::{DocState, Document, FileFormat, History, OpenError, Selection, SpectrogramParams, Viewport, format_time};

use crate::control;
use crate::player::{Devices, Player};
use crate::spectrum::{Job, Renderer};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const BACKGROUND_LIGHT: Color32 = Color32::from_rgb(246, 247, 249);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const RULER_HEIGHT: f32 = 22.0;

/// The Preferences dialog's pages (a page for each group of settings that exists today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefPage {
    General,
    Data,
    MarkersMetadata,
    PlaybackRecording,
    AudioHardware,
    SpectralDisplays,
    TimeDisplay,
    Loudness,
    Appearance,
}

impl PrefPage {
    const ALL: [PrefPage; 9] = [
        PrefPage::General,
        PrefPage::Appearance,
        PrefPage::Data,
        PrefPage::MarkersMetadata,
        PrefPage::PlaybackRecording,
        PrefPage::SpectralDisplays,
        PrefPage::TimeDisplay,
        PrefPage::Loudness,
        PrefPage::AudioHardware,
    ];

    fn title(self) -> &'static str {
        match self {
            PrefPage::General => "General",
            PrefPage::Data => "Data",
            PrefPage::MarkersMetadata => "Markers and Metadata",
            PrefPage::PlaybackRecording => "Playback and Recording",
            PrefPage::AudioHardware => "Audio Hardware",
            PrefPage::SpectralDisplays => "Spectral Displays",
            PrefPage::TimeDisplay => "Time Display",
            PrefPage::Loudness => "Loudness and Diagnostics",
            PrefPage::Appearance => "Appearance",
        }
    }
}

/// What a click in the Preferences dialog asks for (carried out after the dialog is drawn).
enum PrefAction {
    Set(&'static str, String),
    ChooseSessionFolder,
    Preset(&'static str, String),
    RefreshDevices,
    ResetAll,
}

/// Frames at the start in which a held Shift means "reset the preferences".
const SHIFT_RESET_FRAMES: u32 = 30;

/// A file being opened on a worker thread.
struct Opening {
    path: PathBuf,
    result: Receiver<Result<Document, String>>,
    /// Set to stop it (another file was opened meanwhile).
    cancel: Arc<AtomicBool>,
    /// How far the streaming has got, in thousandths.
    progress: Arc<AtomicU32>,
}

/// A save running on a worker thread.
struct Saving {
    path: PathBuf,
    /// The step being saved: it becomes the saved state when the save succeeds.
    state: Arc<DocState>,
    ty: SampleType,
    result: Receiver<Result<(), String>>,
    progress: Arc<AtomicU32>,
    /// Close the window once it is saved (Save from the close prompt).
    then_close: bool,
    /// Which open document it saves ([`WaveApp::doc_id`] when it started): a save that finishes
    /// after another file was opened must not give that file its path (Codex review CX-W1).
    doc_id: u64,
}

/// The newest spectrogram picture, one texture per channel, kept until the next one arrives.
struct SpectrumCache {
    job: Job,
    textures: Vec<TextureHandle>,
}

pub struct WaveApp {
    doc: Option<Document>,
    /// Bumped on every open, so cached pictures of the previous file are dropped.
    generation: usize,
    view: Viewport,
    selection: Selection,
    player: Player,
    params: SpectrogramParams,
    show_waveform: bool,
    show_spectrum: bool,
    spectrum: Option<SpectrumCache>,
    renderer: Renderer,
    opening: Option<Opening>,
    error: Option<String>,
    /// A message that is not an error (the session folder was changed).
    notice: Option<String>,
    /// Where opened files' session stores go, and how that was chosen.
    sessions: SessionFolder,
    settings: Settings,
    /// The settings file (`None` when the system has no config folder).
    settings_path: Option<PathBuf>,
    fit_on_next_frame: bool,
    /// The window title last sent. Sending one asks for a repaint, so it is only sent on change.
    title: String,
    /// The open file's editing state: its history, the active channels, the clipboard.
    editor: Option<Editor>,
    /// Where the open file came from (kept while an edit leaves the document empty).
    doc_path: PathBuf,
    /// Counts the files opened in this window: names the document a save belongs to.
    doc_id: u64,
    /// The views' copy of the document, being read back after an edit (newest request only).
    rebuilding: Option<Receiver<Result<Document, OpenError>>>,
    /// Requests from the control channel, when `WAVCRAFT_CONTROL_PORT` turned it on.
    control: Option<Receiver<control::Request>>,
    /// The step last opened or saved: the file on disk holds it (no unsaved changes while the
    /// current step is this one; undoing back to it clears the mark, as in Audition).
    saved: Option<Arc<DocState>>,
    saving: Option<Saving>,
    /// The "save changes before closing?" prompt is up.
    confirm_close: bool,
    /// Closing was confirmed (or nothing was unsaved): let the next close request through.
    allow_close: bool,
    /// The editor's width last frame, for zoom keys (which have no pointer to zoom around).
    editor_width: f32,
    /// Window › History.
    show_history: bool,
    /// Insert › Silence…: the length being typed, in seconds, while the dialog is up.
    insert_silence: Option<String>,
    /// Clear History asks first: it can't be undone.
    confirm_clear_history: bool,
    /// Edit › Preferences…: the page showing while the dialog is up.
    prefs: Option<PrefPage>,
    /// The look last given to egui: (dark, brightness in thousandths); `None` until the first frame.
    applied_look: Option<(bool, i32, i32)>,
    /// The spectrogram palette in use (name, brightness, contrast, as thousandths).
    applied_palette: Option<(settings::SpecPalette, i32, i32)>,
    /// Whether the dark theme is in force (from the Theme preference, or the system's), and the
    /// brightness: what the editor's own painting uses.
    dark: bool,
    brightness: f32,
    contrast: f32,
    /// The Appearance page's preset name box.
    preset_name: String,
    /// The devices the Audio Hardware page lists (read when the dialog opens or on Refresh).
    devices: Devices,
    /// The device list being read (the system can be slow to answer).
    devices_rx: Option<Receiver<Devices>>,
    /// Frames still to look at for a held Shift (Shift at launch resets the preferences).
    shift_frames: u32,
}

impl WaveApp {
    /// `session_dir`: the `--session-dir` flag, which wins over the environment variable and the
    /// settings file for this run.
    /// `reset_preferences`: the `--reset-preferences` flag, which moves the settings file aside
    /// before it is read.
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, session_dir: Option<PathBuf>, reset_preferences: bool) -> WaveApp {
        let settings_path = Settings::default_path();
        let reset_note = if reset_preferences { settings_path.as_deref().map(move_settings_aside) } else { None };
        let settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let env = std::env::var_os(settings::ENV_SESSION_DIR);
        let sessions = settings::resolve_session_folder(session_dir, env, &settings, default_sessions_root(), settings::check_folder);
        // Folders left by runs that ended without closing their store (a crash, a kill, a
        // signal) are removed in the background; this run's own store is never touched.
        let roots: Vec<PathBuf> = [sessions.path.clone(), default_sessions_root()].into_iter().fold(Vec::new(), |mut v, p| {
            if !v.contains(&p) {
                v.push(p);
            }
            v
        });
        let _ = std::thread::Builder::new().name("session sweep".into()).spawn(move || {
            for root in roots {
                wavcraft_core::store::sweep_stale_sessions(Some(&root));
            }
        });
        let initial_palette = (settings.spec_palette, milli(settings.ui_brightness), milli(settings.ui_contrast));
        let initial_colours = palette(settings.spec_palette, settings.ui_brightness, settings.ui_contrast);
        let mut app = WaveApp {
            doc: None,
            generation: 0,
            view: Viewport { start: 0.0, frames_per_px: 1.0 },
            selection: Selection::default(),
            player: Player::new(settings.output_device.clone()),
            params: {
                let mut p = SpectrogramParams::default();
                settings.apply_to_spectrogram(&mut p);
                p
            },
            show_waveform: true,
            show_spectrum: true,
            spectrum: None,
            renderer: Renderer::new(&cc.egui_ctx, initial_colours),
            opening: None,
            error: sessions.warning.clone(),
            notice: reset_note,
            sessions,
            settings,
            settings_path,
            fit_on_next_frame: false,
            title: String::new(),
            editor: None,
            doc_path: PathBuf::new(),
            doc_id: 0,
            rebuilding: None,
            saved: None,
            saving: None,
            confirm_close: false,
            allow_close: false,
            editor_width: 800.0,
            show_history: true,
            insert_silence: None,
            confirm_clear_history: false,
            prefs: None,
            applied_look: None,
            applied_palette: Some(initial_palette),
            dark: true,
            brightness: 1.0,
            contrast: 1.0,
            preset_name: String::new(),
            devices: Devices::default(),
            devices_rx: None,
            shift_frames: SHIFT_RESET_FRAMES,
            control: {
                let ctx = cc.egui_ctx.clone();
                control::start_from_env(move || ctx.request_repaint())
            },
        };
        if let Some(path) = file {
            app.open(path, &cc.egui_ctx);
        }
        app
    }

    /// Give egui the theme and brightness from the preferences (only when they changed, or when
    /// the system theme changed while the preference is "system").
    fn apply_look(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            settings::Theme::Dark => true,
            settings::Theme::Light => false,
            settings::Theme::System => ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark),
        };
        let key = (dark, milli(self.settings.ui_brightness), milli(self.settings.ui_contrast));
        self.dark = dark;
        self.brightness = self.settings.ui_brightness;
        self.contrast = self.settings.ui_contrast;
        let pal = (self.settings.spec_palette, key.1, key.2);
        if self.applied_palette != Some(pal) {
            self.applied_palette = Some(pal);
            self.renderer = Renderer::new(ctx, palette(pal.0, self.brightness, self.contrast));
            self.spectrum = None;
        }
        if self.applied_look == Some(key) {
            return;
        }
        self.applied_look = Some(key);
        let (b, k) = (self.brightness, self.contrast);
        let tint = |c: Color32| contrast(brighten(c, b), k);
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        for c in [&mut v.panel_fill, &mut v.window_fill, &mut v.extreme_bg_color, &mut v.faint_bg_color, &mut v.code_bg_color] {
            *c = tint(*c);
        }
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.bg_fill = tint(w.bg_fill);
            w.weak_bg_fill = tint(w.weak_bg_fill);
        }
        ctx.set_visuals(v);
    }

    /// Give the open file's editor the crossfade lengths from the preferences.
    fn apply_crossfades(&mut self) {
        let (cut_join, edge) = self.settings.crossfades();
        if let Some(e) = self.editor.as_mut() {
            e.crossfades = wavcraft_core::doc::edit::Crossfades { cut_join, edge };
        }
    }

    /// Write the settings file (the message says why not, in words).
    fn save_settings(&self) -> Result<(), String> {
        match &self.settings_path {
            Some(file) => self.settings.save(file).map_err(|e| format!("could not save the settings to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so the setting lasts until WavCraft closes".to_string()),
        }
    }

    /// Set preference `key` (the dialog's and `PREF set`'s one way in): check the value, carry
    /// it out (reopen the output, change the crossfades, use the folder) and save the file.
    fn pref_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let mut next = self.settings.clone();
        next.set(key, value)?;
        if key == "session_folder" {
            let path = next.session_folder.clone();
            if let Some(p) = &path {
                settings::check_folder(p).map_err(|why| format!("the session folder {} can't be used: {why}", p.display()))?;
            }
            self.set_session_folder(path);
            return Ok(());
        }
        self.settings = next;
        match key {
            "crossfade_cut_join_ms" | "crossfade_edge_ms" => self.apply_crossfades(),
            "output_device" => self.player.set_output(self.settings.output_device.clone()),
            "spec_resolution" | "spec_db_range" => {
                self.settings.apply_to_spectrogram(&mut self.params);
                self.spectrum = None;
            }
            _ => {}
        }
        self.save_settings()
    }

    /// `APPEARANCE preset save|load|delete <name>` and the page's buttons: change the presets
    /// (and, for load, the live appearance, which `apply_look` picks up) and save the file.
    fn preset_do(&mut self, verb: &str, name: &str) -> Result<(), String> {
        match verb {
            "save" => self.settings.save_preset(name)?,
            "load" => self.settings.load_preset(name)?,
            "delete" => self.settings.delete_preset(name)?,
            _ => return Err("APPEARANCE preset save|load|delete <name>".to_string()),
        }
        self.save_settings()
    }

    /// Put every preference back to its default: the settings file is moved aside, not deleted.
    fn reset_preferences(&mut self) {
        let note = self.settings_path.as_deref().map(move_settings_aside);
        self.settings = Settings::default();
        if self.sessions.source == FolderSource::SettingsFile {
            self.sessions = SessionFolder { path: default_sessions_root(), source: FolderSource::Default, warning: None };
        }
        self.apply_crossfades();
        self.player.set_output(None);
        self.settings.apply_to_spectrogram(&mut self.params);
        self.spectrum = None;
        self.error = None;
        self.notice = Some(note.unwrap_or_else(|| "Preferences reset to their defaults.".to_string()));
    }

    /// Edit › Preferences…
    fn open_preferences(&mut self) {
        self.devices_rx = Some(crate::player::list_devices_in_background());
        self.prefs.get_or_insert(PrefPage::General);
    }

    /// The Preferences dialog: a page list on the left, the page on the right.
    fn preferences_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut page) = self.prefs else { return };
        if let Some(rx) = &self.devices_rx {
            match rx.try_recv() {
                Ok(d) => {
                    self.devices = d;
                    self.devices_rx = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.devices_rx = None,
            }
        }
        let mut open = true;
        let mut action: Option<PrefAction> = None;
        egui::Window::new("Preferences").open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(130.0);
                    for p in PrefPage::ALL {
                        ui.selectable_value(&mut page, p, p.title());
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.heading(page.title());
                    ui.add_space(6.0);
                    match page {
                        PrefPage::General => {
                            ui.label("Session folder: where an opened file is kept while you work on it.");
                            ui.monospace(self.sessions.path.display().to_string());
                            ui.label(format!("({})", self.sessions.source.describe()));
                            ui.horizontal(|ui| {
                                if ui.button("Choose…").clicked() {
                                    action = Some(PrefAction::ChooseSessionFolder);
                                }
                                if ui.button("Use the default").clicked() {
                                    action = Some(PrefAction::Set("session_folder", String::new()));
                                }
                            });
                            ui.add_space(12.0);
                            pref_number(ui, "Zoom In / Out factor", "zoom_factor", self.settings.zoom_factor, settings::MIN_ZOOM_FACTOR..=settings::MAX_ZOOM_FACTOR, "x", &mut action);
                            pref_choice(ui, "Extend selection from", "extend_selection", self.settings.extend_selection.name(), settings::ExtendSelection::ALL.iter().map(|c| c.name()), &mut action);
                            ui.label("Stored only for now: Shift-click extension is not on this branch.");
                            pref_check(ui, "Context-sensitive channel editing", "context_channel_editing", self.settings.context_channel_editing, &mut action);
                            ui.label("Stored only for now: per-channel pointer editing is not on this branch.");
                            ui.add_space(12.0);
                            if ui.button("Reset all preferences").on_hover_text("The settings file is kept aside, not deleted").clicked() {
                                action = Some(PrefAction::ResetAll);
                            }
                        }
                        PrefPage::Data => {
                            ui.label("Edit crossfades smooth the joins an edit makes (0 turns one off).");
                            for (label, key, value, default) in [
                                ("Delete / Cut join (ms)", "crossfade_cut_join_ms", self.settings.cut_join_ms, settings::DEFAULT_CUT_JOIN_MS),
                                ("Every other edit boundary (ms)", "crossfade_edge_ms", self.settings.edge_ms, settings::DEFAULT_EDGE_MS),
                            ] {
                                ui.horizontal(|ui| {
                                    let mut v = value;
                                    if ui.add(egui::DragValue::new(&mut v).range(0..=settings::MAX_CROSSFADE_MS).suffix(" ms")).changed() {
                                        action = Some(PrefAction::Set(key, v.to_string()));
                                    }
                                    ui.label(label);
                                    if value != default && ui.small_button("Default").clicked() {
                                        action = Some(PrefAction::Set(key, default.to_string()));
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Sample rate conversion quality");
                                egui::ComboBox::from_id_salt("src_quality").selected_text(self.settings.src_quality.name()).show_ui(ui, |ui| {
                                    for q in settings::SrcQuality::ALL {
                                        if ui.selectable_label(self.settings.src_quality == q, q.name()).clicked() {
                                            action = Some(PrefAction::Set("src_quality", q.name().to_string()));
                                        }
                                    }
                                });
                            });
                            ui.label("Stored only for now: nothing on this branch converts sample rates yet.");
                            ui.add_space(8.0);
                            ui.label("Float headroom: files are held as 32-bit float, so a level over 0 dBFS inside the editor is kept, not clipped; it is only limited when written to a fixed-point file.");
                        }
                        PrefPage::MarkersMetadata => {
                            ui.label("Saved files: what goes in by default (the save options dialog can override it).");
                            ui.label("Stored only for now: the save options that read these are not on this branch.");
                            for (label, key, on) in [
                                ("Include markers when saving", "save_markers", self.settings.save_markers),
                                ("Include metadata when saving", "save_metadata", self.settings.save_metadata),
                            ] {
                                let mut v = on;
                                if ui.checkbox(&mut v, label).changed() {
                                    action = Some(PrefAction::Set(key, v.to_string()));
                                }
                            }
                            ui.add_space(6.0);
                            ui.label("Marker name pattern ({n} is the marker's number):");
                            let id = ui.id().with("marker_pattern");
                            let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| self.settings.marker_name_pattern.clone());
                            let r = ui.text_edit_singleline(&mut text);
                            if r.lost_focus() {
                                action = Some(PrefAction::Set("marker_name_pattern", text.clone()));
                                ui.data_mut(|d| d.remove::<String>(id));
                            } else {
                                ui.data_mut(|d| d.insert_temp(id, text));
                            }
                            ui.label(format!("Next marker: {}", self.settings.marker_name(1)));
                            if self.settings.marker_name_pattern != settings::DEFAULT_MARKER_PATTERN && ui.small_button("Default").clicked() {
                                action = Some(PrefAction::Set("marker_name_pattern", settings::DEFAULT_MARKER_PATTERN.to_string()));
                                ui.data_mut(|d| d.remove::<String>(id));
                            }
                        }
                        PrefPage::PlaybackRecording => {
                            ui.label("Stored only for now: the transport, shuttle and recording features that read these are not on this branch.");
                            ui.horizontal(|ui| {
                                ui.label("Auto-scroll while playing");
                                egui::ComboBox::from_id_salt("autoscroll").selected_text(self.settings.autoscroll.name()).show_ui(ui, |ui| {
                                    for a in settings::AutoScroll::ALL {
                                        if ui.selectable_label(self.settings.autoscroll == a, a.name()).clicked() {
                                            action = Some(PrefAction::Set("autoscroll", a.name().to_string()));
                                        }
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label("J/K/L speed steps");
                                let id = ui.id().with("jkl_steps");
                                let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| self.settings.get("jkl_speed_steps").unwrap_or_default());
                                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
                                if r.lost_focus() {
                                    action = Some(PrefAction::Set("jkl_speed_steps", text.clone()));
                                    ui.data_mut(|d| d.remove::<String>(id));
                                } else {
                                    ui.data_mut(|d| d.insert_temp(id, text));
                                }
                                ui.label("(slowest first, e.g. 0.5,1,2,4)");
                            });
                            for (label, key, value) in [
                                ("Pre-roll (s)", "preroll_s", self.settings.preroll().as_secs_f32()),
                                ("Post-roll (s)", "postroll_s", self.settings.postroll().as_secs_f32()),
                            ] {
                                let mut v = value;
                                ui.horizontal(|ui| {
                                    if ui.add(egui::DragValue::new(&mut v).range(0.0..=settings::MAX_ROLL_S).speed(0.1).suffix(" s")).changed() {
                                        action = Some(PrefAction::Set(key, v.to_string()));
                                    }
                                    ui.label(label);
                                });
                            }
                            let mut off = self.settings.record_offset_ms;
                            ui.horizontal(|ui| {
                                let max = settings::MAX_RECORD_OFFSET_MS;
                                if ui.add(egui::DragValue::new(&mut off).range(-max..=max).suffix(" ms")).changed() {
                                    action = Some(PrefAction::Set("record_offset_ms", off.to_string()));
                                }
                                ui.label("Record offset (placeholder until recording lands)");
                            });
                        }
                        PrefPage::Appearance => {
                            pref_choice(ui, "Theme", "theme", self.settings.theme.name(), settings::Theme::ALL.iter().map(|c| c.name()), &mut action);
                            pref_choice(ui, "Waveform colour", "waveform_colour", self.settings.waveform_colour.name(), settings::WaveColour::ALL.iter().map(|c| c.name()), &mut action);
                            pref_number(ui, "UI brightness", "ui_brightness", self.settings.ui_brightness, settings::MIN_BRIGHTNESS..=settings::MAX_BRIGHTNESS, "x", &mut action);
                            pref_number(ui, "UI contrast", "ui_contrast", self.settings.ui_contrast, settings::MIN_CONTRAST..=settings::MAX_CONTRAST, "x", &mut action);
                            pref_choice(ui, "Spectrogram palette", "spec_palette", self.settings.spec_palette.name(), settings::SpecPalette::ALL.iter().map(|c| c.name()), &mut action);
                            ui.separator();
                            ui.label("Colours (a pick overrides the theme's own):");
                            for (item, key) in appearance::ITEMS.iter().zip(COLOR_KEYS) {
                                ui.horizontal(|ui| {
                                    let now = self.settings.colors.get(item).flatten();
                                    let mut rgb = now.unwrap_or_else(|| default_rgb(item, &self.settings, self.dark));
                                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                                        action = Some(PrefAction::Set(key, appearance::format_hex(rgb)));
                                    }
                                    ui.label(format!("{item} ({})", appearance::colour_text(now)));
                                    if now.is_some() && ui.small_button("Default").clicked() {
                                        action = Some(PrefAction::Set(key, "default".to_string()));
                                    }
                                });
                            }
                            ui.separator();
                            ui.label("Presets:");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("preset name").desired_width(140.0));
                                if ui.button("Save").clicked() {
                                    action = Some(PrefAction::Preset("save", self.preset_name.clone()));
                                }
                            });
                            for name in self.settings.preset_names() {
                                ui.horizontal(|ui| {
                                    ui.label(&name);
                                    if ui.small_button("Load").clicked() {
                                        action = Some(PrefAction::Preset("load", name.clone()));
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        action = Some(PrefAction::Preset("delete", name.clone()));
                                    }
                                });
                            }
                            ui.label("Everything on this page applies at once.");
                        }
                        PrefPage::SpectralDisplays => {
                            let res: Vec<String> = settings::SPEC_RESOLUTIONS.iter().map(u32::to_string).collect();
                            pref_choice(ui, "Resolution (FFT length)", "spec_resolution", &self.settings.spec_resolution.to_string(), res.iter().map(String::as_str), &mut action);
                            pref_number(ui, "dB range", "spec_db_range", self.settings.spec_db_range as f32, settings::MIN_SPEC_DB_RANGE as f32..=settings::MAX_SPEC_DB_RANGE as f32, "dB", &mut action);
                            ui.label("Resolution and dB range apply to the spectral display at once.");
                            pref_choice(ui, "Window function", "spec_window", self.settings.spec_window.name(), settings::SpecWindow::ALL.iter().map(|c| c.name()), &mut action);
                            ui.label("Stored only for now: the display always analyses with a Hann window.");
                            pref_check(ui, "Play only selected frequencies (placeholder)", "spec_play_selected_freqs", self.settings.spec_play_selected_freqs, &mut action);
                            ui.label("Stored only: there is no frequency selection to play yet.");
                        }
                        PrefPage::TimeDisplay => {
                            pref_choice(ui, "Default time format", "time_format", self.settings.time_format.name(), settings::TimeFormat::ALL.iter().map(|c| c.name()), &mut action);
                            pref_number(ui, "Frame rate (SMPTE)", "frame_rate", self.settings.frame_rate, settings::MIN_FRAME_RATE..=settings::MAX_FRAME_RATE, "fps", &mut action);
                            ui.label("Stored only for now: the time displays still use minutes:seconds.");
                        }
                        PrefPage::Loudness => {
                            pref_number(ui, "Default loudness target", "loudness_target_lufs", self.settings.loudness_target_lufs, settings::MIN_LOUDNESS_TARGET_LUFS..=settings::MAX_LOUDNESS_TARGET_LUFS, "LUFS", &mut action);
                            pref_number(ui, "Default true-peak ceiling", "true_peak_ceiling_dbtp", self.settings.true_peak_ceiling_dbtp, settings::MIN_TRUE_PEAK_DBTP..=settings::MAX_TRUE_PEAK_DBTP, "dBTP", &mut action);
                            ui.label("Stored only for now: loudness features read these when they are merged.");
                        }
                        PrefPage::AudioHardware => {
                            for (label, key, list, now) in [
                                ("Output", "output_device", &self.devices.output, &self.settings.output_device),
                                ("Input (for recording)", "input_device", &self.devices.input, &self.settings.input_device),
                            ] {
                                let shown = now.clone().unwrap_or_else(|| settings::SYSTEM_DEFAULT.to_string());
                                let gone = now.as_ref().is_some_and(|n| !list.contains(n));
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(key)
                                        .width(300.0)
                                        .selected_text(if gone { format!("{shown} (not connected)") } else { shown.clone() })
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_label(now.is_none(), settings::SYSTEM_DEFAULT).clicked() {
                                                action = Some(PrefAction::Set(key, String::new()));
                                            }
                                            for name in list.iter() {
                                                if ui.selectable_label(now.as_ref() == Some(name), name).clicked() {
                                                    action = Some(PrefAction::Set(key, name.clone()));
                                                }
                                            }
                                        });
                                });
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Refresh the device lists").clicked() {
                                    action = Some(PrefAction::RefreshDevices);
                                }
                                if self.devices_rx.is_some() {
                                    ui.spinner();
                                    ui.label("Looking for devices…");
                                }
                            });
                            ui.add_space(6.0);
                            match &self.player.error {
                                Some(e) => ui.colored_label(CURSOR, format!("No sound: {e}")),
                                None => ui.label(format!("Playing on: {}", self.player.device)),
                            };
                        }
                    }
                });
            });
        });
        self.prefs = open.then_some(page);
        let result = match action {
            Some(PrefAction::Set(key, value)) => self.pref_set(key, &value),
            Some(PrefAction::ChooseSessionFolder) => rfd::FileDialog::new()
                .set_directory(&self.sessions.path)
                .pick_folder()
                .map_or(Ok(()), |p| self.pref_set("session_folder", &p.to_string_lossy())),
            Some(PrefAction::Preset(verb, name)) => self.preset_do(verb, &name),
            Some(PrefAction::RefreshDevices) => {
                self.devices_rx = Some(crate::player::list_devices_in_background());
                Ok(())
            }
            Some(PrefAction::ResetAll) => {
                self.reset_preferences();
                Ok(())
            }
            None => Ok(()),
        };
        if let Err(e) = result {
            self.error = Some(e);
        }
    }

    /// Stream `path` into a session store on a worker thread; the window stays responsive for
    /// long files. A file still opening is cancelled first.
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        if let Some(old) = self.opening.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (ctx, job_path, job_cancel, job_progress) = (ctx.clone(), path.clone(), cancel.clone(), progress.clone());
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        std::thread::spawn(move || {
            let mut last = 0;
            let mut report = |p: f64| {
                let permille = (p.clamp(0.0, 1.0) * 1000.0) as u32;
                job_progress.store(permille, Ordering::Relaxed);
                if permille >= last + 10 {
                    last = permille;
                    ctx.request_repaint();
                }
            };
            let result = Document::open_in(&job_path, &opts, &mut report, &job_cancel).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.opening = Some(Opening { path, result: rx, cancel, progress });
        self.error = None;
        self.notice = None;
    }

    fn finish_opening(&mut self) {
        let Some(Opening { path, result: rx, .. }) = &self.opening else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            // The worker ended without an answer: say so instead of showing "Opening…" forever.
            Err(TryRecvError::Disconnected) => Err("the decoder stopped unexpectedly".to_string()),
        };
        match result {
            Ok(doc) => {
                self.player.set_audio(Some(doc.audio.clone()));
                let cap = History::default_cap(&self.sessions.path);
                self.editor = doc.state.clone().map(|state| Editor::new(History::new(state, cap)));
                self.apply_crossfades();
                self.saved = self.editor.as_ref().map(|e| e.history.current().clone());
                self.doc_path = doc.path.clone();
                self.doc_id += 1;
                self.rebuilding = None;
                self.doc = Some(doc);
                self.generation += 1;
                self.selection = Selection::default();
                self.spectrum = None;
                self.fit_on_next_frame = true;
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
        self.opening = None;
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        let picked =
            rfd::FileDialog::new().add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"]).pick_file();
        if let Some(path) = picked {
            self.open(path, ctx);
        }
    }

    /// File › Session Folder…: pick a folder, check it, use it from the next open and save it.
    fn pick_session_folder(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_directory(&self.sessions.path).pick_folder() else { return };
        self.set_session_folder(Some(path));
    }

    /// Use `path` (the default when `None`) for the next opens, and keep it in the settings.
    fn set_session_folder(&mut self, path: Option<PathBuf>) {
        let (path, source) = match path {
            Some(p) => (p, FolderSource::SettingsFile),
            None => (default_sessions_root(), FolderSource::Default),
        };
        if let Err(why) = settings::check_folder(&path) {
            self.error = Some(format!("The session folder {} can't be used: {why}. Still using {}.", path.display(), self.sessions.path.display()));
            self.notice = None;
            return;
        }
        let overridden = matches!(self.sessions.source, FolderSource::CommandLine | FolderSource::Environment);
        self.settings.session_folder = (source == FolderSource::SettingsFile).then(|| path.clone());
        let saved = match &self.settings_path {
            Some(file) => self.settings.save(file).map_err(|e| format!("could not save the setting to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so it lasts until WavCraft closes".to_string()),
        };
        self.sessions = SessionFolder { path, source, warning: None };
        let mut msg = format!("Session folder: {} (from the next file opened)", self.sessions.path.display());
        match saved {
            Ok(()) if overridden => msg.push_str(". Saved, but --session-dir or WAVCRAFT_SESSION_DIR will win again at the next start"),
            Ok(()) => {}
            Err(e) => msg.push_str(&format!("; {e}")),
        }
        self.error = None;
        self.notice = Some(msg);
    }

    fn toggle_play(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.player.is_playing() {
            self.player.stop();
        } else if self.selection.is_empty() {
            self.player.play(self.selection.start(), doc.frames());
        } else {
            self.player.play(self.selection.start(), self.selection.end());
        }
    }

    /// The selection as a frame range (`start == end` is just the cursor).
    fn selection_range(&self) -> Range<u64> {
        self.selection.start() as u64..self.selection.end() as u64
    }

    /// Run an action from the table on the open file (menu, key or control channel). Returns
    /// whether the document changed.
    fn run_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if actions::action(id).kind == Kind::Window {
            return self.run_window_action(id, ctx);
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        match editor.run(id, sel) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                if matches!(id, ActionId::CursorToStart | ActionId::CursorToEnd) {
                    self.player.stop();
                    let frames = self.doc.as_ref().map_or(0, Document::frames);
                    self.view.reveal(self.selection.start() as f64, self.editor_width, frames);
                }
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{}: {e}", actions::action(id).menu);
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// The actions the window carries out itself: zoom, In/Out points at the playhead, and the
    /// Insert Silence dialog.
    fn run_window_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        if frames == 0 && id != ActionId::InsertSilence {
            return Err("no file is open".to_string());
        }
        let width = self.editor_width.max(1.0);
        // Where play is, or the cursor when stopped.
        let playhead = if self.player.is_playing() { self.player.position() } else { self.selection.start() };
        match id {
            ActionId::ZoomIn => self.view.zoom_at(f64::from(self.settings.zoom_factor), width * 0.5, width, frames),
            ActionId::ZoomOut => self.view.zoom_at(1.0 / f64::from(self.settings.zoom_factor), width * 0.5, width, frames),
            ActionId::ZoomOutFull => self.fit_on_next_frame = true,
            ActionId::ZoomToSelection if !self.selection.is_empty() => {
                let (a, b) = (self.selection.start() as f64, self.selection.end() as f64);
                self.view.frames_per_px = ((b - a) / f64::from(width)).max(1.0 / 64.0);
                self.view.start = a;
                self.view.clamp(width, frames);
            }
            ActionId::SetIn => {
                let end = self.selection.end().max(playhead);
                self.selection = Selection { anchor: playhead, head: end };
            }
            ActionId::SetOut => {
                let start = self.selection.start().min(playhead);
                self.selection = Selection { anchor: start, head: playhead };
            }
            ActionId::InsertSilence if self.editor.is_some() => self.insert_silence = Some("1.000".to_string()),
            // Repeat Last Command with its dialog: Insert Silence opens its dialog with the last
            // length; the other repeatable commands have no dialog and simply run again.
            ActionId::RepeatLastDialog => {
                let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
                if !editor.enabled(id, &self.selection_range()) {
                    return Err(format!("{} is not available now", actions::action(id).menu));
                }
                if let Some(Repeat::InsertSilence(frames)) = editor.last {
                    let secs = frames as f64 / f64::from(editor.history.current().sample_rate().max(1));
                    self.insert_silence = Some(format!("{secs:.3}"));
                } else {
                    return self.run_action(ActionId::RepeatLast, ctx);
                }
            }
            _ => return Err(format!("{} is not available now", actions::action(id).menu)),
        }
        Ok(false)
    }

    /// Insert › Silence…: a length in seconds, inserted at the cursor on every channel.
    fn insert_silence_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = self.insert_silence.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Insert Silence").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Duration (seconds):");
                let r = ui.text_edit_singleline(text);
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    choice = Some(true);
                }
                r.request_focus();
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                let secs = self.insert_silence.take().and_then(|t| t.trim().parse::<f64>().ok()).filter(|s| s.is_finite() && *s > 0.0);
                let Some(secs) = secs else {
                    self.error = Some("Insert Silence: type a length in seconds, above 0".to_string());
                    return;
                };
                self.run_insert_silence(secs, ctx);
            }
            Some(false) => self.insert_silence = None,
            None => {}
        }
    }

    fn run_insert_silence(&mut self, secs: f64, ctx: &egui::Context) -> Option<String> {
        let at = self.selection.start() as u64;
        let editor = self.editor.as_mut()?;
        let frames = (secs * f64::from(editor.history.current().sample_rate())).round() as u64;
        match editor.insert_silence(at, frames) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.notice = out.message;
                self.error = None;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                None
            }
            Err(e) => {
                let why = format!("Insert Silence: {e}");
                self.error = Some(why.clone());
                Some(why)
            }
        }
    }

    /// Window › History: every step, the current one highlighted; click one to go to it.
    fn history_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("History");
        let Some(editor) = self.editor.as_ref() else {
            ui.label("No file open.");
            return;
        };
        let (cursor, dropped) = (editor.history.cursor(), editor.history.dropped_total());
        let labels: Vec<String> = editor.history.steps().iter().map(|s| s.label().to_string()).collect();
        let (held, dir) = (editor.history.held_bytes(), editor.history.current().store().dir().to_path_buf());
        if dropped > 0 {
            ui.label(format!("Oldest {dropped} steps removed to stay under {:.1} GB", editor.history.cap_disk_bytes() as f64 / 1e9));
        }
        let mut go = None;
        egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                if ui.selectable_label(i == cursor, label).clicked() && i != cursor {
                    go = Some(i);
                }
            }
        });
        ui.separator();
        let free = wavcraft_core::space::free_space(&dir).map_or_else(|| "unknown".to_string(), |b| format!("{:.1} GB", b as f64 / 1e9));
        ui.label(format!("Session: {:.1} MB · free {free}", held as f64 / 1e6));
        if ui.add_enabled(labels.len() > 1, egui::Button::new("Clear History…")).clicked() {
            self.confirm_clear_history = true;
        }
        if let Some(i) = go
            && self.saving.is_none()
            && let Some(editor) = self.editor.as_mut()
            && editor.history.go_to(i).is_some()
        {
            let len = editor.history.current().len_frames() as usize;
            self.selection = Selection { anchor: self.selection.start().min(len), head: self.selection.end().min(len) };
            self.rebuild(ui.ctx());
        }
    }

    fn clear_history_prompt(&mut self, ctx: &egui::Context) {
        if !self.confirm_clear_history {
            return;
        }
        let mut choice = None;
        egui::Window::new("Clear History").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.label("Remove every step but the current one? This can't be undone.");
            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                if let Some(e) = self.editor.as_mut() {
                    e.history.clear();
                }
                self.confirm_clear_history = false;
            }
            Some(false) => self.confirm_clear_history = false,
            None => {}
        }
    }

    /// Read the current step back for the views and the player on a worker thread (task 05
    /// replaces this with views that read the document directly).
    fn rebuild(&mut self, ctx: &egui::Context) {
        let Some(editor) = &self.editor else { return };
        let (tx, rx) = channel();
        let (state, path, ctx) = (editor.history.current().as_ref().clone(), self.doc_path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Document::from_state(path, state));
            ctx.request_repaint();
        });
        self.player.stop();
        self.rebuilding = Some(rx);
    }

    fn finish_rebuild(&mut self) {
        let Some(rx) = &self.rebuilding else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(OpenError::Io("the worker stopped unexpectedly".to_string())),
        };
        self.rebuilding = None;
        match result {
            Ok(doc) => {
                self.player.set_audio(Some(doc.audio.clone()));
                self.doc = Some(doc);
                self.generation += 1;
                self.spectrum = None;
            }
            Err(OpenError::Empty) => {
                self.player.set_audio(None);
                self.doc = None;
                self.notice = Some("The file is empty now (Edit › Undo brings the audio back)".to_string());
            }
            Err(e) => {
                log::warn!(target: "wavcraft", "could not show the edited document: {e}");
                self.error = Some(format!("Could not show the edited document: {e}"));
            }
        }
    }

    /// Whether the current step differs from the one on disk.
    fn unsaved(&self) -> bool {
        match (&self.editor, &self.saved) {
            (Some(e), Some(saved)) => !Arc::ptr_eq(e.history.current(), saved),
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// File › Save: to the file's own path when it is a WAV (the only format written so far),
    /// else as Save As. `to` skips the dialog (the control channel).
    fn save(&mut self, ctx: &egui::Context, save_as: bool, to: Option<PathBuf>, then_close: bool) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let own = !self.doc_path.as_os_str().is_empty() && state.source().format == FileFormat::Wav;
        let path = match to {
            Some(p) => p,
            None if own && !save_as => self.doc_path.clone(),
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let mut dialog = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(format!("{stem}.wav"));
                if let Some(dir) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
        };
        let ty = SampleType::for_source(state.source());
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (job_state, job_path, job_progress, ctx) = (state.clone(), path.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(save_wav(&job_state, &job_path, ty, &mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.player.stop();
        self.saving = Some(Saving { path, state, ty, result: rx, progress, then_close, doc_id: self.doc_id });
        Ok(())
    }

    fn finish_saving(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.saving else { return };
        let result = match saving.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the save stopped unexpectedly".to_string()),
        };
        let Some(Saving { path, state, ty, then_close, doc_id, .. }) = self.saving.take() else { return };
        match result {
            // Another file was opened while this one was being saved: the file on disk is
            // written, but the open document is not the one saved, so its path, saved mark and
            // close request stay as they are.
            Ok(()) if doc_id != self.doc_id => {
                log::info!(target: "wavcraft", "a save of an earlier file finished after another was opened: {}", path.display());
                self.notice = Some(format!("Saved {} ({} WAV), the file open before this one", path.display(), ty.describe()));
            }
            Ok(()) => {
                self.saved = Some(state);
                self.doc_path = path.clone();
                if let Some(doc) = self.doc.as_mut() {
                    doc.path = path.clone();
                }
                self.error = None;
                self.notice = Some(format!("Saved {} ({} WAV)", path.display(), ty.describe()));
                if then_close {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(e) => {
                self.error = Some(format!("Not saved: {e}. {} is unchanged.", path.display()));
            }
        }
    }

    /// The "save changes?" prompt when the window is closed with unsaved changes.
    fn close_prompt(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if !self.confirm_close {
            return;
        }
        let name = self.doc.as_ref().map_or_else(|| "the file".to_string(), Document::name);
        let mut choice = None;
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.label(format!("Save changes to {name} before closing?"));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            },
        );
        match choice {
            Some(0) => {
                self.confirm_close = false;
                if let Err(e) = self.save(ctx, false, None, true) {
                    self.error = Some(e);
                }
            }
            Some(1) => {
                self.confirm_close = false;
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(_) => self.confirm_close = false,
            None => {}
        }
    }

    /// Answer one control-channel line (see `control.rs`).
    fn control_request(&mut self, line: &str, ctx: &egui::Context) -> String {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let sel = |s: &Self| format!("{}-{}", s.selection.start(), s.selection.end());
        match cmd {
            "STATE" => {
                let busy = self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some();
                let unsaved = u8::from(self.unsaved());
                match &self.editor {
                    Some(e) => {
                        let doc = e.history.current();
                        format!(
                            "OK frames={} channels={} rate={} selection={} steps={} cursor={} busy={} unsaved={unsaved} undo={} redo={}",
                            doc.len_frames(),
                            doc.channel_count(),
                            doc.sample_rate(),
                            sel(self),
                            e.history.steps().len(),
                            e.history.cursor(),
                            u8::from(busy),
                            e.history.undo_label().unwrap_or("-"),
                            e.history.redo_label().unwrap_or("-")
                        )
                    }
                    None => format!("OK frames=0 channels=0 rate=0 selection=0-0 steps=0 cursor=0 busy={} unsaved=0 undo=- redo=-", u8::from(busy)),
                }
            }
            "OPEN" if !rest.is_empty() => {
                self.open(PathBuf::from(rest), ctx);
                "OK".to_string()
            }
            "SELECT" => {
                let mut it = rest.split_whitespace().map(str::parse::<usize>);
                match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => {
                        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames() as usize);
                        self.selection = Selection { anchor: a.min(len), head: b.min(len) };
                        format!("OK selection={}", sel(self))
                    }
                    _ => "ERR SELECT <start> <end>, in frames".to_string(),
                }
            }
            "ACTION" => match actions::by_name(rest) {
                Some(a) => match self.run_action(a.id, ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                None => format!("ERR no action named {rest:?}"),
            },
            // Minimise or restore the window (scripted checks of hidden-window behaviour).
            "WINDOW" => match rest {
                "minimize" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    "OK".to_string()
                }
                "restore" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    "OK".to_string()
                }
                _ => "ERR WINDOW minimize|restore".to_string(),
            },
            "INSERT_SILENCE" => match rest.trim().parse::<f64>() {
                Ok(secs) if secs.is_finite() && secs > 0.0 => match self.run_insert_silence(secs, ctx) {
                    None => format!("OK selection={}", sel(self)),
                    Some(e) => format!("ERR {e}"),
                },
                _ => "ERR INSERT_SILENCE <seconds>, above 0".to_string(),
            },
            // The History panel's click: go to step <index> (0 is Open).
            "HISTORY" => match (rest.trim().parse::<usize>(), self.editor.as_mut()) {
                (Ok(i), Some(e)) if self.saving.is_none() => match e.history.go_to(i) {
                    Some(_) => {
                        let len = e.history.current().len_frames() as usize;
                        self.selection = Selection { anchor: self.selection.start().min(len), head: self.selection.end().min(len) };
                        self.rebuild(ctx);
                        "OK".to_string()
                    }
                    None => format!("ERR no step {i}"),
                },
                _ => "ERR HISTORY <step>, with a file open".to_string(),
            },
            // `PREF get <key>` / `PREF set <key> <value>` (keys: see `settings::KEYS`).
            "PREF" => {
                let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
                let (key, value) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));
                match verb {
                    "get" => self.settings.get(key).map_or_else(|e| format!("ERR {e}"), |v| format!("OK {v}")),
                    "set" => self.pref_set(key, value).map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string()),
                    _ => "ERR PREF get <key> | PREF set <key> <value>".to_string(),
                }
            }
            // `APPEARANCE preset save|load|delete <name>`, `APPEARANCE preset list`.
            "APPEARANCE" => {
                let (what, args) = rest.split_once(' ').unwrap_or((rest, ""));
                let (verb, name) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));
                match (what, verb) {
                    ("preset", "list") => format!("OK {}", self.settings.preset_names().join("|")),
                    ("preset", _) => self.preset_do(verb, name).map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string()),
                    _ => "ERR APPEARANCE preset save|load|delete <name> | APPEARANCE preset list".to_string(),
                }
            }
            // The devices the system offers, and the one the player is on.
            "DEVICES" => {
                let d = crate::player::list_devices();
                format!("OK output={} input={} playing_on={}", d.output.join("|"), d.input.join("|"), self.player.device)
            }
            "SAVE" => {
                let to = (!rest.is_empty()).then(|| PathBuf::from(rest));
                match self.save(ctx, to.is_some(), to, false) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // A plain QUIT refuses with unsaved changes; `QUIT discard` closes anyway.
            "QUIT" if self.unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or QUIT discard".to_string(),
            "QUIT" => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                "OK".to_string()
            }
            _ => format!("ERR unknown request {cmd:?}"),
        }
    }

    /// The Edit menu, built from the action table: labels that name the step, Audition's keys.
    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let sel = self.selection_range();
        let mac = cfg!(target_os = "macos");
        let mut chosen = None;
        for a in TABLE.iter().filter(|a| a.name.starts_with("edit.")) {
            if matches!(a.id, ActionId::Cut | ActionId::Delete | ActionId::Crop | ActionId::ZeroInward | ActionId::SetIn | ActionId::LeftChannel) {
                ui.separator();
            }
            let (enabled, label) = self.editor.as_ref().map_or((false, a.menu.to_string()), |e| (e.enabled(a.id, &sel), e.label(a.id)));
            let enabled = enabled && self.saving.is_none();
            let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
        }
        ui.separator();
        if ui.add(egui::Button::new("Preferences…").shortcut_text(if mac { "⌘," } else { "Ctrl+," })).clicked() {
            self.open_preferences();
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…  Ctrl+O").clicked() {
                    self.pick_file(ui.ctx());
                }
                let mac = cfg!(target_os = "macos");
                let has_doc = self.editor.is_some() && self.saving.is_none();
                let (save, save_as) = if mac { ("⌘S", "⇧⌘S") } else { ("Ctrl+S", "Ctrl+Shift+S") };
                if ui.add_enabled(has_doc, egui::Button::new("Save").shortcut_text(save)).clicked()
                    && let Err(e) = self.save(ui.ctx(), false, None, false)
                {
                    self.error = Some(e);
                }
                if ui.add_enabled(has_doc, egui::Button::new("Save As…").shortcut_text(save_as)).clicked()
                    && let Err(e) = self.save(ui.ctx(), true, None, false)
                {
                    self.error = Some(e);
                }
                ui.separator();
                let now = format!("Where opened files are kept while you work on them. Now: {}", self.sessions.path.display());
                if ui.button("Session Folder…").on_hover_text(&now).clicked() {
                    self.pick_session_folder();
                }
                if ui.button("Use the Default Session Folder").on_hover_text(default_sessions_root().display().to_string()).clicked() {
                    self.set_session_folder(None);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| self.edit_menu(ui));
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_waveform, "Waveform");
                ui.checkbox(&mut self.show_spectrum, "Spectral frequency display");
                if ui.checkbox(&mut self.params.log_freq, "Logarithmic frequency axis").changed() {
                    self.spectrum = None;
                }
                ui.separator();
                let (sel, mac) = (self.selection_range(), cfg!(target_os = "macos"));
                let mut chosen = None;
                for a in TABLE.iter().filter(|a| a.name.starts_with("view.")) {
                    let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel));
                    let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
                    if ui.add_enabled(enabled, egui::Button::new(a.menu).shortcut_text(keys)).clicked() {
                        chosen = Some(a.id);
                    }
                }
                if let Some(id) = chosen {
                    let _ = self.run_action(id, ui.ctx());
                }
                ui.separator();
                ui.checkbox(&mut self.show_history, "History panel");
            });
            let label = if self.player.is_playing() { "■ Stop" } else { "▶ Play" };
            if ui.button(label).on_hover_text("Space").clicked() {
                self.toggle_play();
            }
        });
    }

    fn status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(o) = &self.opening {
                ui.spinner();
                let permille = o.progress.load(Ordering::Relaxed);
                ui.label(format!("Opening {}… {}%", o.path.display(), permille / 10));
            } else if let Some(s) = &self.saving {
                ui.spinner();
                ui.label(format!("Saving {}… {}%", s.path.display(), s.progress.load(Ordering::Relaxed) / 10));
            } else if self.rebuilding.is_some() {
                ui.spinner();
                ui.label("Updating the view…");
            } else if let Some(e) = &self.error {
                ui.colored_label(CURSOR, e);
            } else if let Some(n) = &self.notice {
                ui.label(n);
            } else if let Some(doc) = &self.doc {
                let rate = doc.sample_rate();
                ui.label(doc.name());
                ui.separator();
                ui.label(format!(
                    "{:?} {:?} · {} Hz · {} ch · {}",
                    doc.info.format,
                    doc.info.sample_format,
                    rate,
                    doc.channels(),
                    format_time(doc.frames(), rate)
                ));
                // What was wrong with the file that still opened: the first in the bar, all of
                // them on hover.
                if let Some(first) = doc.warnings().first() {
                    ui.separator();
                    ui.colored_label(CURSOR, format!("Warning: {first}")).on_hover_text(doc.warnings().join("\n"));
                }
                ui.separator();
                if self.selection.is_empty() {
                    ui.label(format!("Cursor {}", format_time(self.selection.start(), rate)));
                } else {
                    let (a, b) = (self.selection.start(), self.selection.end());
                    ui.label(format!("Selection {} – {}  ({})", format_time(a, rate), format_time(b, rate), format_time(b - a, rate)));
                }
                ui.separator();
                match &self.player.error {
                    Some(e) => ui.colored_label(CURSOR, format!("No sound: {e}")),
                    None if self.player.failed() => ui.colored_label(CURSOR, "No sound: the output device stopped (Play tries it again)"),
                    None => ui.label(format!("Out: {}", self.player.device)),
                };
            } else {
                ui.label("File › Open, or drop an audio file here.");
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        let c = &self.settings.colors;
        let ground = c.background.map_or(if self.dark { BACKGROUND } else { BACKGROUND_LIGHT }, |[r, g, b]| Color32::from_rgb(r, g, b));
        painter.rect_filled(rect, 0.0, contrast(brighten(ground, self.brightness), self.contrast));
        let rgb = |o: Option<[u8; 3]>, d: Color32| o.map_or(d, |[r, g, b]| Color32::from_rgb(r, g, b));
        let (selection_colour, cursor_colour, playhead_colour) = (
            c.selection.map_or(SELECTION, |[r, g, b]| Color32::from_rgba_unmultiplied(r, g, b, 90)),
            rgb(c.cursor, CURSOR),
            rgb(c.playhead, PLAYHEAD),
        );
        let Some(doc) = &self.doc else { return };
        let (frames, rate, channels) = (doc.frames(), doc.sample_rate(), doc.channels().max(1));
        let width = rect.width().max(1.0);
        self.editor_width = width;
        if self.fit_on_next_frame {
            self.view = Viewport::fit(frames, width);
            self.fit_on_next_frame = false;
        }

        // Wheel zooms around the pointer; Shift+wheel and sideways scrolling move through time.
        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch, shift) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.modifiers.shift));
            let x = pointer.x - rect.left();
            if pinch != 1.0 {
                self.view.zoom_at(f64::from(pinch), x, width, frames);
            }
            if shift {
                self.view.scroll_by(-(scroll.x + scroll.y), width, frames);
            } else {
                if scroll.y != 0.0 {
                    self.view.zoom_at(f64::from((scroll.y * 0.01).exp()), x, width, frames);
                }
                if scroll.x != 0.0 {
                    self.view.scroll_by(-scroll.x, width, frames);
                }
            }
        }
        // Keep the playhead on screen while playing (not while the pointer is busy in the editor).
        if self.player.is_playing() && !response.is_pointer_button_down_on() {
            self.view.reveal(self.player.position() as f64, width, frames);
        }
        self.view.clamp(width, frames);

        // Click places the cursor, drag selects.
        let frame_at = |view: &Viewport, pos: Pos2| view.frame_at(pos.x - rect.left()).clamp(0.0, frames as f64).round() as usize;
        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // A drag is only reported once the pointer has moved a few pixels: anchor the
            // selection where the button went down, not where the pointer has got to.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            self.selection = Selection { anchor: frame_at(&self.view, origin), head: frame_at(&self.view, pos) };
        } else if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selection.head = frame_at(&self.view, pos);
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selection = Selection::at(frame_at(&self.view, pos));
        }

        let (start, end) = (self.view.start, self.view.end(width));
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(rect.left(), ruler.bottom()), rect.max);
        draw_ruler(&painter, ruler, &self.view, rate, self.dark);

        let (wave_area, spectrum_area) = match (self.show_waveform, self.show_spectrum) {
            (true, true) => {
                let split = body.top() + body.height() * 0.5;
                (Some(Rect::from_min_max(body.min, pos2(body.right(), split))), Some(Rect::from_min_max(pos2(body.left(), split), body.max)))
            }
            (true, false) => (Some(body), None),
            (false, true) => (None, Some(body)),
            (false, false) => (None, None),
        };

        if let Some(area) = wave_area {
            let cols = area.width().round().max(1.0) as usize;
            let [r, g, b] = self.settings.colors.waveform.unwrap_or_else(|| self.settings.waveform_rgb());
            let wave = Color32::from_rgb(r, g, b);
            for ch in 0..channels {
                let lane = lane(area, ch, channels);
                let mid = lane.center().y;
                painter.line_segment(
                    [pos2(lane.left(), mid), pos2(lane.right(), mid)],
                    Stroke::new(1.0, Color32::from_gray(if self.dark { 50 } else { 200 })),
                );
                let half = lane.height() * 0.5 - 2.0;
                for (i, (lo, hi)) in doc.waveform_columns(ch, start, end, cols).into_iter().enumerate() {
                    let x = lane.left() + i as f32 + 0.5;
                    let (top, bottom) = (mid - hi.clamp(-1.0, 1.0) * half, mid - lo.clamp(-1.0, 1.0) * half);
                    painter.line_segment([pos2(x, top.min(bottom - 1.0)), pos2(x, bottom)], Stroke::new(1.0, wave));
                }
            }
        }

        if let Some(area) = spectrum_area {
            // Half resolution across, capped rows: plenty for the eye. The columns sit on a grid
            // fixed to the file, so scrolling moves the picture without changing it.
            let cols = (area.width() * 0.5).round().clamp(1.0, 2048.0);
            let rows = (area.height() / channels as f32).round().clamp(1.0, 384.0) as usize;
            let columns = Columns::covering(start, end, self.view.frames_per_px * f64::from(area.width() / cols), &self.params);
            self.renderer.ask(Job { generation: self.generation, columns, rows, params: self.params }, &doc.audio);
            if let Some(picture) = self.renderer.take()
                && picture.job.generation == self.generation
                && picture.job.params == self.params
            {
                let texture = |(ch, image)| ui.ctx().load_texture(format!("spectrum-{ch}"), image, TextureOptions::LINEAR);
                self.spectrum = Some(SpectrumCache { job: picture.job, textures: picture.images.into_iter().enumerate().map(texture).collect() });
            }
            if let Some(cache) = &self.spectrum {
                // A picture is drawn where its own frames are on screen. While the next one is
                // being made, the last one still lines up with the ruler and the waveform.
                let (left, right) = (area.left() + self.view.x_of(cache.job.columns.start), area.left() + self.view.x_of(cache.job.columns.end));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                for (ch, texture) in cache.textures.iter().enumerate() {
                    let lane = lane(area, ch, channels).shrink2(vec2(0.0, 1.0));
                    painter.with_clip_rect(lane).image(
                        texture.id(),
                        Rect::from_min_max(pos2(left, lane.top()), pos2(right, lane.bottom())),
                        uv,
                        Color32::WHITE,
                    );
                }
            }
        }

        // Selection, cursor and playhead over everything.
        if !self.selection.is_empty() {
            let (a, b) = (rect.left() + self.view.x_of(self.selection.start() as f64), rect.left() + self.view.x_of(self.selection.end() as f64));
            painter.rect_filled(
                Rect::from_min_max(pos2(a.max(rect.left()), body.top()), pos2(b.min(rect.right()), body.bottom())),
                0.0,
                selection_colour,
            );
        }
        let line = |frame: usize, color: Color32| {
            let x = rect.left() + self.view.x_of(frame as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, color));
            }
        };
        line(self.selection.start(), cursor_colour);
        if self.player.is_playing() {
            line(self.player.position(), playhead_colour);
            ui.ctx().request_repaint();
        }
    }
}

impl eframe::App for WaveApp {
    /// Runs before every frame, and also while the window is hidden, minimised or covered (eframe
    /// skips `ui` then, but still calls this on `request_repaint`): the workers' results and the
    /// control channel must not wait for the window to be shown (the 2026-10-19 by-eye check
    /// found the channel silent behind another window).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.finish_opening();
        self.finish_rebuild();
        self.finish_saving(ctx);
        while let Some(req) = self.control.as_ref().and_then(|rx| rx.try_recv().ok()) {
            let answer = self.control_request(&req.line, ctx);
            let _ = req.reply.send(answer);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_look(&ctx);
        self.close_prompt(&ctx);
        let (save_as, save) = ctx.input_mut(|i| {
            let save_as = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::S);
            (save_as, i.consume_key(egui::Modifiers::COMMAND, Key::S))
        });
        if (save || save_as)
            && let Err(e) = self.save(&ctx, save_as, None, false)
        {
            self.error = Some(e);
        }
        for id in pressed_actions(&ctx) {
            let _ = self.run_action(id, &ctx);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Comma)) {
            self.open_preferences();
        }
        // Shift held as the window comes up: reset the preferences (once).
        if self.shift_frames > 0 {
            self.shift_frames -= 1;
            if ctx.input(|i| i.modifiers.shift) {
                self.shift_frames = 0;
                self.reset_preferences();
            }
        }
        // Home and Zoom Out Full come from the action table now (Audition's Home and Cmd+\).
        let (space, open, dropped) = ctx.input(|i| {
            (
                // Not `key_pressed`: that also counts auto-repeat, so a long press would toggle again.
                i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Space, pressed: true, repeat: false, .. })),
                i.modifiers.command && i.key_pressed(Key::O),
                i.raw.dropped_files.first().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()),
            )
        });
        if space {
            self.toggle_play();
        }
        if open {
            self.pick_file(&ctx);
        }
        if let Some(path) = dropped {
            self.open(path, &ctx);
        }
        let star = if self.unsaved() { "*" } else { "" };
        let title = self.doc.as_ref().map_or_else(|| "WavCraft".to_string(), |d| format!("{star}{} — WavCraft", d.name()));
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.show_history {
            egui::Panel::right("history").default_size(220.0).show(ui, |ui| self.history_panel(ui));
        }
        self.insert_silence_dialog(&ctx);
        self.preferences_dialog(&ctx);
        self.clear_history_prompt(&ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
    }
}

/// Move the settings file aside (`--reset-preferences`, Shift at launch, the dialog's button) and
/// say what happened.
fn move_settings_aside(path: &std::path::Path) -> String {
    match settings::reset_aside(path) {
        Ok(Some(aside)) => {
            log::info!(target: "wavcraft", "preferences reset: {} moved to {}", path.display(), aside.display());
            format!("Preferences reset to their defaults (the old settings were kept as {})", aside.display())
        }
        Ok(None) => "Preferences reset to their defaults".to_string(),
        Err(e) => {
            log::warn!(target: "wavcraft", "could not move the settings file aside: {e}");
            format!("Could not move the old settings file aside ({e}); using the defaults, which replace it at the next change")
        }
    }
}

/// The actions whose keys were pressed this frame. Keys with more modifiers are taken first, so
/// Cmd+Shift+Z (Redo) is not read as Cmd+Z (Undo).
fn pressed_actions(ctx: &egui::Context) -> Vec<ActionId> {
    let mut keys: Vec<(ActionId, actions::Shortcut)> = TABLE.iter().flat_map(|a| a.keys.iter().map(move |k| (a.id, *k))).collect();
    keys.sort_by_key(|(_, k)| std::cmp::Reverse(u8::from(k.cmd) + u8::from(k.shift) + u8::from(k.alt)));
    ctx.input_mut(|i| {
        keys.iter()
            .filter_map(|(id, k)| {
                let key = Key::from_name(k.key)?;
                let mut mods = egui::Modifiers::NONE;
                if k.cmd {
                    mods |= egui::Modifiers::COMMAND;
                }
                if k.shift {
                    mods |= egui::Modifiers::SHIFT;
                }
                if k.alt {
                    mods |= egui::Modifiers::ALT;
                }
                i.consume_key(mods, key).then_some(*id)
            })
            .collect()
    })
}

/// A row with a combo box for a named choice (`current` is shown; a pick asks to set `key`).
fn pref_choice<'a>(
    ui: &mut egui::Ui,
    label: &str,
    key: &'static str,
    current: &str,
    all: impl Iterator<Item = &'a str>,
    action: &mut Option<PrefAction>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(key).selected_text(current).show_ui(ui, |ui| {
            for name in all {
                if ui.selectable_label(current == name, name).clicked() {
                    *action = Some(PrefAction::Set(key, name.to_string()));
                }
            }
        });
    });
}

/// A row with a number box (dragging or typing asks to set `key`).
fn pref_number(
    ui: &mut egui::Ui,
    label: &str,
    key: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    unit: &str,
    action: &mut Option<PrefAction>,
) {
    ui.horizontal(|ui| {
        let mut v = value;
        let speed = (range.end() - range.start()) / 400.0;
        if ui.add(egui::DragValue::new(&mut v).range(range).speed(speed).max_decimals(2).suffix(format!(" {unit}"))).changed() {
            *action = Some(PrefAction::Set(key, v.to_string()));
        }
        ui.label(label);
    });
}

/// A check box for a true/false preference.
fn pref_check(ui: &mut egui::Ui, label: &str, key: &'static str, on: bool, action: &mut Option<PrefAction>) {
    let mut v = on;
    if ui.checkbox(&mut v, label).changed() {
        *action = Some(PrefAction::Set(key, v.to_string()));
    }
}

/// `c` with each colour channel multiplied by `b` (1: unchanged), kept in range.
fn brighten(c: Color32, b: f32) -> Color32 {
    let m = |v: u8| (f32::from(v) * b).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

/// The vertical slice of `area` for channel `ch` of `channels`.
fn lane(area: Rect, ch: usize, channels: usize) -> Rect {
    let h = area.height() / channels.max(1) as f32;
    Rect::from_min_max(pos2(area.left(), area.top() + h * ch as f32), pos2(area.right(), area.top() + h * (ch + 1) as f32))
}

/// Tick marks and times along the top, at a round step that leaves room for the labels.
fn draw_ruler(painter: &egui::Painter, rect: Rect, view: &Viewport, rate: u32, dark: bool) {
    let (ground, tick, text) = if dark { (32, 140, 190) } else { (225, 110, 60) };
    painter.rect_filled(rect, 0.0, Color32::from_gray(ground));
    let seconds_per_px = view.frames_per_px / f64::from(rate.max(1));
    let step = ruler_step(seconds_per_px * 110.0);
    let first = (view.start / f64::from(rate.max(1)) / step).floor() as i64;
    for k in first..first + 200 {
        let t = k as f64 * step;
        let x = rect.left() + view.x_of(t * f64::from(rate));
        if x > rect.right() {
            break;
        }
        if x < rect.left() || t < 0.0 {
            continue;
        }
        painter.line_segment([pos2(x, rect.bottom() - 6.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(tick)));
        painter.text(pos2(x + 3.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, tick_label(t), egui::FontId::monospace(11.0), Color32::from_gray(text));
    }
}

/// The label of the tick at `seconds`, to the millisecond. (Going through a rounded frame number
/// loses a millisecond at rates that 1000 does not divide: 3 ms at 44.1 kHz read "0.002".)
fn tick_label(seconds: f64) -> String {
    format_time((seconds.max(0.0) * 1000.0).round() as usize, 1000)
}

/// The smallest "round" number of seconds that is at least `min_seconds`.
fn ruler_step(min_seconds: f64) -> f64 {
    const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// The keys of the colour items, in the order of [`appearance::ITEMS`].
const COLOR_KEYS: [&str; 5] = ["color.waveform", "color.selection", "color.cursor", "color.playhead", "color.background"];

/// The colour an item has when nothing overrides it, as red, green, blue.
fn default_rgb(item: &str, settings: &Settings, dark: bool) -> [u8; 3] {
    let c = match item {
        "waveform" => return settings.waveform_rgb(),
        "selection" => SELECTION,
        "cursor" => CURSOR,
        "playhead" => PLAYHEAD,
        _ if dark => BACKGROUND,
        _ => BACKGROUND_LIGHT,
    };
    [c.r(), c.g(), c.b()]
}

/// `v` in thousandths, for comparing settings that are floats.
fn milli(v: f32) -> i32 {
    (v * 1000.0).round() as i32
}

/// `c` with its distance from mid grey multiplied by `k` (1: unchanged), kept in range.
fn contrast(c: Color32, k: f32) -> Color32 {
    let m = |v: u8| ((f32::from(v) - 128.0) * k + 128.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

/// 256 colours along `which`'s ramp (quiet to loud), with brightness and contrast applied.
fn palette(which: settings::SpecPalette, brightness: f32, contrast_k: f32) -> Vec<Color32> {
    let stops = which.stops();
    (0..256)
        .map(|i| {
            let t = i as f32 / 255.0;
            let hi = stops.iter().position(|s| s.0 >= t).unwrap_or(stops.len() - 1).max(1);
            let (a, b) = (stops[hi - 1], stops[hi]);
            let f = ((t - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
            let mix = |k: usize| (a.1[k] + (b.1[k] - a.1[k]) * f).round() as u8;
            contrast(brighten(Color32::from_rgb(mix(0), mix(1), mix(2)), brightness), contrast_k)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_scales_colours_and_stays_in_range() {
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 1.0), Color32::from_rgb(100, 200, 50));
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 0.5), Color32::from_rgb(50, 100, 25));
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 1.5), Color32::from_rgb(150, 255, 75));
    }

    #[test]
    fn ruler_steps_are_round_and_wide_enough() {
        assert_eq!(ruler_step(0.0004), 0.001);
        assert_eq!(ruler_step(0.7), 1.0);
        assert_eq!(ruler_step(45.0), 60.0);
        assert_eq!(ruler_step(5000.0), 3600.0);
    }

    #[test]
    fn tick_labels_are_exact_at_any_sample_rate() {
        let labels: Vec<String> = (0..8).map(|k| tick_label(f64::from(k) * 0.001)).collect();
        assert_eq!(labels, ["0:00.000", "0:00.001", "0:00.002", "0:00.003", "0:00.004", "0:00.005", "0:00.006", "0:00.007"]);
        assert_eq!(tick_label(3723.5), "1:02:03.500");
        // The old route, through a frame at 44.1 kHz, dropped a millisecond.
        assert_eq!(format_time((0.003f64 * 44_100.0).round() as usize, 44_100), "0:00.002");
    }

    #[test]
    fn the_palette_runs_dark_to_light() {
        let p = palette(settings::SpecPalette::Ember, 1.0, 1.0);
        assert_eq!(p.len(), 256);
        assert_eq!(p[0], Color32::from_rgb(0, 0, 0));
        assert_eq!(p[255], Color32::from_rgb(255, 250, 215));
        let brightness = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(p.windows(2).all(|w| brightness(w[1]) + 3 >= brightness(w[0])), "brightness never drops");
    }

    #[test]
    fn every_palette_is_256_colours_and_contrast_widens() {
        for which in settings::SpecPalette::ALL {
            assert_eq!(palette(*which, 1.0, 1.0).len(), 256, "{}", which.name());
        }
        assert_eq!(contrast(Color32::from_rgb(100, 128, 200), 1.0), Color32::from_rgb(100, 128, 200));
        assert_eq!(contrast(Color32::from_rgb(100, 128, 200), 2.0), Color32::from_rgb(72, 128, 255));
        let mono = palette(settings::SpecPalette::Mono, 1.0, 1.0);
        assert_eq!((mono[0], mono[255]), (Color32::BLACK, Color32::WHITE));
        assert_ne!(palette(settings::SpecPalette::Glacier, 1.0, 1.0), palette(settings::SpecPalette::Ember, 1.0, 1.0));
    }

    #[test]
    fn lanes_stack_without_gaps() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 300.0));
        assert_eq!(lane(area, 0, 2), Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 200.0)));
        assert_eq!(lane(area, 1, 2), Rect::from_min_max(pos2(0.0, 200.0), pos2(800.0, 300.0)));
        assert_eq!(lane(area, 0, 0), area);
    }
}
