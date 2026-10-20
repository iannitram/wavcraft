//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::convert::{self, ConvertSettings, PresetFile};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::panels::{self, PanelSet};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::snap::ruler_step;
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::view::nav_hit;
use wavcraft_core::{
    AmpScale, AmpZoom, ChannelMask, DocState, Document, Edit, FileFormat, GRAB_PX, History, NavPart, OpenError, Selection, SpectrogramParams,
    TimeFormat, Tool, Viewport, format_time,
};

use crate::control;
use crate::convert_ui::{self, ConvertDialog};
use crate::doclist::{DocList, Where};
use crate::effect_dialog::{Answer, EffectDialog};
use crate::mixpaste;
use crate::player::{Devices, Player};
use crate::recovery::{self, Choice, Recovery};
use crate::spectrum::{Job, Renderer};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const WAVE: Color32 = Color32::from_rgb(64, 200, 140);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const RULER_HEIGHT: f32 = 22.0;
/// The amplitude ruler's column, left of the editor.
const AMP_RULER_WIDTH: f32 = 46.0;
const NAV_HEIGHT: f32 = 34.0;

/// The Preferences dialog's pages (a page for each group of settings that exists today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefPage {
    General,
    Data,
    AudioHardware,
}

impl PrefPage {
    const ALL: [PrefPage; 3] = [PrefPage::General, PrefPage::Data, PrefPage::AudioHardware];

    fn title(self) -> &'static str {
        match self {
            PrefPage::General => "General",
            PrefPage::Data => "Data",
            PrefPage::AudioHardware => "Audio Hardware",
        }
    }
}

/// What a click in the Preferences dialog asks for (carried out after the dialog is drawn).
enum PrefAction {
    Set(&'static str, String),
    ChooseSessionFolder,
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

/// A file being streamed into the open document's session store for Insert > Audio from File.
struct Inserting {
    path: PathBuf,
    result: Receiver<Result<DocState, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    /// The document it belongs to ([`WaveApp::doc_id`] when it started): a result that arrives
    /// after another file was opened is dropped.
    doc_id: u64,
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

/// A conversion running on a worker thread (Convert Sample Type).
struct Converting {
    /// The step it converts: the result is dropped if the document has moved on.
    from: Arc<DocState>,
    result: Receiver<Result<Edit, String>>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    doc_id: u64,
}
/// An open document that is not the current one: the per-document fields of [`WaveApp`], parked
/// until it is switched to (the current document's own live in the window's fields).
struct DocSlot {
    doc: Option<Document>,
    editor: Option<Editor>,
    doc_path: PathBuf,
    doc_id: u64,
    saved: Option<Arc<DocState>>,
    view: Viewport,
    selection: Selection,
    rebuilding: Option<Receiver<Result<Document, OpenError>>>,
}

/// Whether `editor`'s current step differs from `saved`, the one on disk.
fn is_unsaved(editor: &Option<Editor>, saved: &Option<Arc<DocState>>) -> bool {
    match (editor, saved) {
        (Some(e), Some(saved)) => !Arc::ptr_eq(e.history.current(), saved),
        (Some(_), None) => true,
        _ => false,
    }
}

fn path_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned())
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
    /// Vertical (amplitude) zoom of the waveform.
    amp_zoom: AmpZoom,
    /// The zoom navigator drag in progress: the part grabbed and the frames between the view's
    /// start and the pointer when it was grabbed.
    nav_drag: Option<(NavPart, f64)>,
    selection: Selection,
    /// The tool the pointer uses in the editor (Time Selection is the only one built).
    tool: Tool,
    player: Player,
    params: SpectrogramParams,
    /// Which registered panels are shown (Window menu; kept in the settings file).
    panels: PanelSet,
    spectrum: Option<SpectrumCache>,
    renderer: Renderer,
    opening: Option<Opening>,
    inserting: Option<Inserting>,
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
    /// Names the current document (the save in flight belongs to one).
    doc_id: u64,
    /// Counts the files opened in this window; the next `doc_id`.
    doc_serial: u64,
    /// The other open documents; the fields above and below hold the current one.
    docs: DocList<DocSlot>,
    /// The "save changes?" prompt is for closing the current document, not the window.
    closing_doc: bool,
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
    /// Insert › Silence…: the length being typed, in seconds, while the dialog is up.
    insert_silence: Option<String>,
    /// Edit › Mix Paste…: the dialog's fields while it is up.
    mix_paste: Option<mixpaste::Dialog>,
    /// Clear History asks first: it can't be undone.
    confirm_clear_history: bool,
    /// Frames a selection drag snaps to besides the ruler: the edges of the selection that was
    /// there when the gesture began (the cursor is its start).
    snap_targets: Vec<u64>,
    /// Copy and Paste's side of the system clipboard.
    os_clipboard: crate::oscb::OsClipboard,
    /// A History row had keyboard focus last frame: Up and Down move through the states then.
    history_focus: bool,
    /// The History row to give keyboard focus next frame (after an arrow key moved the cursor).
    history_refocus: Option<usize>,
    /// Auto Save: the journal writer, and the offer to recover a crashed run's work.
    recovery: Recovery,
    /// Edit › Convert Sample Type…: the dialog while it is up, and the conversion once started.
    convert_dialog: Option<ConvertDialog>,
    converting: Option<Converting>,
    /// Edit › Interpret Sample Rate…: the rate being typed while the dialog is up.
    interpret: Option<String>,
    /// Named conversion settings (built in, and the user's from `convert-presets.txt`).
    presets: PresetFile,
    /// The effect parameter dialog (f14), while it is up.
    effect_dialog: Option<EffectDialog>,
    /// View › Time Display (kept in the settings file).
    time_format: TimeFormat,
    /// View › Time panel / Selection/View panel.
    show_time_panel: bool,
    show_sel_view_panel: bool,
    /// The Selection/View box being typed in: its name and its text.
    panel_edit: Option<(&'static str, String)>,
    /// Edit › Preferences…: the page showing while the dialog is up.
    prefs: Option<PrefPage>,
    /// The devices the Audio Hardware page lists (read when the dialog opens or on Refresh).
    devices: Devices,
    /// The device list being read (the system can be slow to answer).
    devices_rx: Option<Receiver<Devices>>,
    /// Frames still to look at for a held Shift (Shift at launch resets the preferences).
    shift_frames: u32,
    /// A Generate dialog's Preview is playing (f21): the player holds the preview, not the file.
    previewing: bool,
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
        let roots = recovery::roots(&sessions.path, &default_sessions_root());
        let recovery = {
            let ctx = cc.egui_ctx.clone();
            Recovery::new(roots, move || ctx.request_repaint())
        };
        let mut app = WaveApp {
            doc: None,
            generation: 0,
            view: Viewport { start: 0.0, frames_per_px: 1.0 },
            amp_zoom: AmpZoom::full(),
            nav_drag: None,
            selection: Selection::default(),
            snap_targets: Vec::new(),
            tool: Tool::default(),
            player: Player::new(),
            player: Player::new(settings.output_device.clone()),
            params: SpectrogramParams::default(),
            panels: PanelSet::from_hidden(&settings.hidden_panels),
            spectrum: None,
            renderer: Renderer::new(&cc.egui_ctx, palette()),
            opening: None,
            inserting: None,
            error: sessions.warning.clone(),
            notice: None,
            time_format: settings.time_display,
            notice: reset_note,
            sessions,
            settings,
            settings_path,
            fit_on_next_frame: false,
            title: String::new(),
            editor: None,
            doc_path: PathBuf::new(),
            doc_id: 0,
            doc_serial: 0,
            docs: DocList::new(),
            closing_doc: false,
            rebuilding: None,
            saved: None,
            saving: None,
            confirm_close: false,
            allow_close: false,
            os_clipboard: crate::oscb::OsClipboard::new(),
            editor_width: 800.0,
            show_time_panel: true,
            show_sel_view_panel: true,
            panel_edit: None,
            insert_silence: None,
            mix_paste: None,
            confirm_clear_history: false,
            history_focus: false,
            history_refocus: None,
            recovery,
            convert_dialog: None,
            converting: None,
            interpret: None,
            presets: PresetFile::load(PresetFile::default_path()),
            effect_dialog: None,
            prefs: None,
            devices: Devices::default(),
            devices_rx: None,
            shift_frames: SHIFT_RESET_FRAMES,
            previewing: false,
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
            _ => {}
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
        // A file being inserted belongs to the document that is being replaced.
        if let Some(old) = self.inserting.take() {
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
                // The document that was open stays open, parked; this one is added at the end.
                let old = self.editor.is_some().then(|| self.take_slot());
                self.docs.add(old);
                self.player.set_audio(Some(doc.audio.clone()));
                let cap = History::default_cap(&self.sessions.path);
                // Snapping stays as the owner left it from file to file.
                let snap = self.editor.as_ref().map(|e| e.snap);
                self.editor = doc.state.clone().map(|state| Editor::new(History::new(state, cap)));
                if let (Some(e), Some(snap)) = (self.editor.as_mut(), snap) {
                    e.snap = snap;
                }
                self.apply_crossfades();
                self.saved = self.editor.as_ref().map(|e| e.history.current().clone());
                self.doc_path = doc.path.clone();
                self.doc_serial += 1;
                self.doc_id = self.doc_serial;
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

    /// Recover the `index`th session a crashed run left (Auto Save): its document and undo
    /// history come back as they were at the last journal. Refused while another file has
    /// unsaved changes, or one is opening.
    fn recover_session(&mut self, index: usize, ctx: &egui::Context) -> Result<(), String> {
        if self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some() {
            return Err("Wait for the current job to finish before recovering".to_string());
        }
        if self.unsaved() {
            return Err("Save or close the open file before recovering".to_string());
        }
        let dir = self.recovery.offers.get(index).map(|o| o.dir.clone()).ok_or_else(|| format!("no recoverable session {index}"))?;
        let rec =
            wavcraft_core::autosave::recover(&dir, wavcraft_core::store::StoreOptions::default()).map_err(|e| format!("Could not recover: {e}"))?;
        let cap = History::default_cap(&self.sessions.path);
        let history = rec.into_history(cap).ok_or("Could not recover: the journal holds no steps")?;
        self.doc_path = history.current().source().path.clone();
        self.editor = Some(Editor::new(history));
        self.saved = None;
        self.doc_id += 1;
        self.doc = None;
        self.player.set_audio(None);
        self.selection = Selection::default();
        self.spectrum = None;
        self.fit_on_next_frame = true;
        self.recovery.remove(index);
        self.notice = Some("Recovered the unsaved work (File › Save keeps it)".to_string());
        log::info!(target: "wavcraft", "recovery: restored the session in {}", dir.display());
        self.rebuild(ctx);
        Ok(())
    }

    /// Remove a crashed run's session folder the owner chose to discard.
    fn discard_recovery(&mut self, index: usize) -> Result<(), String> {
        let dir = self.recovery.offers.get(index).map(|o| o.dir.clone()).ok_or_else(|| format!("no recoverable session {index}"))?;
        wavcraft_core::autosave::discard(&dir).map_err(|e| format!("Could not discard: {e}"))?;
        log::info!(target: "wavcraft", "recovery: discarded the session in {} at the owner's request", dir.display());
        self.recovery.remove(index);
        Ok(())
    }

    /// Insert › Audio from File…: pick a file, then [`insert_file`](Self::insert_file).
    fn pick_insert_file(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        let picked =
            rfd::FileDialog::new().add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"]).pick_file();
        match picked {
            Some(path) => self.insert_file(path, ctx),
            None => Ok(()),
        }
    }

    /// Stream `path` into the open document's own session store on a worker thread (never the
    /// whole file in memory), then insert it at the cursor or over the selection as one step.
    fn insert_file(&mut self, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        let why = if self.saving.is_some() {
            Some("wait for the save to finish")
        } else if self.opening.is_some() {
            Some("wait for the file to finish opening")
        } else if self.converting.is_some() {
            Some("wait for the conversion to finish")
        } else if self.inserting.is_some() {
            Some("another file is being inserted")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(why.to_string());
        }
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let opts = OpenOptions { into_store: Some(editor.history.current().store().clone()), ..OpenOptions::default() };
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (ctx, job_path, job_cancel, job_progress) = (ctx.clone(), path.clone(), cancel.clone(), progress.clone());
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
            let result = wavcraft_core::io::open(&job_path, &opts, &mut report, &job_cancel).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.inserting = Some(Inserting { path, result: rx, cancel, progress, doc_id: self.doc_id });
        self.error = None;
        self.notice = None;
        Ok(())
    }

    fn finish_inserting(&mut self, ctx: &egui::Context) {
        let Some(Inserting { path, result: rx, doc_id, .. }) = &self.inserting else { return };
        let (path, doc_id) = (path.clone(), *doc_id);
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the decoder stopped unexpectedly".to_string()),
        };
        self.inserting = None;
        if doc_id != self.doc_id {
            return;
        }
        let sel = self.selection_range();
        let outcome = match (result, self.editor.as_mut()) {
            (Ok(file), Some(editor)) => editor.insert_file(&file, sel),
            (Err(e), _) => Err(e),
            (_, None) => Err("no file is open".to_string()),
        };
        match outcome {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.notice = out.message;
                self.error = None;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
            }
            Err(e) => {
                let why = format!("Insert Audio from File: {}: {e}", path.display());
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why);
            }
        }
    }
    /// Park the current document's fields (the window's are left empty).
    fn take_slot(&mut self) -> DocSlot {
        DocSlot {
            doc: self.doc.take(),
            editor: self.editor.take(),
            doc_path: std::mem::take(&mut self.doc_path),
            doc_id: self.doc_id,
            saved: self.saved.take(),
            view: self.view,
            selection: self.selection,
            rebuilding: self.rebuilding.take(),
        }
    }

    /// Make a parked document the current one.
    fn put_slot(&mut self, s: DocSlot) {
        self.player.stop();
        (self.doc, self.editor, self.doc_path, self.doc_id) = (s.doc, s.editor, s.doc_path, s.doc_id);
        (self.saved, self.view, self.selection, self.rebuilding) = (s.saved, s.view, s.selection, s.rebuilding);
        self.player.set_audio(self.doc.as_ref().map(|d| d.audio.clone()));
        self.generation += 1;
        self.spectrum = None;
    }

    /// Switch to open document `i` (0 is the first opened).
    fn switch_doc(&mut self, i: usize) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.editor.is_none() || i >= self.docs.len(true) {
            return Err(format!("no document {i}"));
        }
        if i == self.docs.current() {
            return Ok(());
        }
        // One clipboard for all documents: a clip copied here can be pasted there.
        let clips = self.editor.as_mut().map(|e| (std::mem::take(&mut e.clipboards), e.current_clipboard));
        let old = self.take_slot();
        let next = self.docs.switch(i, old).unwrap_or_else(|old| old);
        self.put_slot(next);
        if let (Some((clips, current)), Some(e)) = (clips, self.editor.as_mut()) {
            e.clipboards = clips;
            e.current_clipboard = current;
        }
        Ok(())
    }

    /// Close the current document without asking; the next one (or the last) takes its place.
    fn remove_current(&mut self) {
        self.player.stop();
        drop(self.take_slot());
        match self.docs.close_current() {
            Some(next) => self.put_slot(next),
            None => {
                self.generation += 1;
                self.spectrum = None;
            }
        }
    }

    /// File › Close: close the current document, asking first when it has unsaved changes.
    fn close_doc(&mut self) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.unsaved() {
            self.closing_doc = true;
            self.confirm_close = true;
        } else {
            self.remove_current();
        }
        Ok(())
    }

    /// Name, unsaved mark and length of open document `i`.
    fn doc_info(&self, i: usize) -> (String, bool, u64) {
        let frames = |e: &Option<Editor>| e.as_ref().map_or(0, |e| e.history.current().len_frames());
        match self.docs.locate(i) {
            Where::Current => (path_name(&self.doc_path), self.unsaved(), frames(&self.editor)),
            Where::Parked(p) => self
                .docs
                .parked()
                .get(p)
                .map_or_else(|| (String::new(), false, 0), |s| (path_name(&s.doc_path), is_unsaved(&s.editor, &s.saved), frames(&s.editor))),
        }
    }

    /// The tab strip above the editor, when more than one document is open.
    fn doc_tabs(&mut self, ui: &mut egui::Ui) {
        let (n, cur, mut go) = (self.docs.len(self.editor.is_some()), self.docs.current(), None);
        ui.horizontal_wrapped(|ui| {
            for i in 0..n {
                let (name, unsaved, _) = self.doc_info(i);
                let label = if unsaved { format!("*{name}") } else { name };
                if ui.selectable_label(i == cur, label).clicked() && i != cur {
                    go = Some(i);
                }
            }
        });
        if let Some(i) = go
            && let Err(e) = self.switch_doc(i)
        {
            self.error = Some(e);
        }
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

    /// Show or hide a registered panel and keep the choice in the settings file.
    fn set_panel(&mut self, id: &str, shown: bool) {
        if !self.panels.set(id, shown) {
            return;
        }
        self.settings.hidden_panels = self.panels.hidden_ids();
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the panel choice to {}: {e}", file.display());
        }
    }

    /// View › Time Display: use `fmt` for the ruler, the status bar and the panels, and keep it.
    fn set_time_format(&mut self, fmt: TimeFormat) {
        self.time_format = fmt;
        self.panel_edit = None;
        self.settings.time_display = fmt;
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the time display to {}: {e}", file.display());
        }
    }

    /// `frame` in the chosen time format at the open file's rate.
    fn fmt_time(&self, frame: usize) -> String {
        self.time_format.format(frame, self.doc.as_ref().map_or(0, |d| d.sample_rate()))
    }

    /// Keep `settings` in the settings file; the error says why not.
    fn save_settings(&self) -> Result<(), String> {
        match &self.settings_path {
            Some(file) => self.settings.save(file).map_err(|e| format!("could not save the setting to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so it lasts until WavCraft closes".to_string()),
        }
    }

    /// Right-click in the waveform at `frame`: the nearest edge of the selection moves there
    /// (Audition's default). With the "context menu" setting nothing moves; the menu is the
    /// pointer's business. Returns whether the selection moved.
    fn right_click_at(&mut self, frame: usize, total: usize) -> bool {
        if self.settings.right_click_menu || self.tool != Tool::TimeSelection {
            return false;
        }
        self.selection = self.selection.right_click_extend(frame, total);
        true
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
    /// The visible range as `<first>-<last>` frames, for STATE and NAV.
    fn view_text(&self) -> String {
        let frames = self.doc.as_ref().map_or(0, Document::frames) as f64;
        format!("{}-{}", self.view.start.round(), self.view.end(self.editor_width).min(frames).round())
    }

    /// Use `scale` on the amplitude ruler and keep it in the settings file.
    fn set_amp_scale(&mut self, scale: AmpScale) {
        self.settings.amp_scale = scale;
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the ruler scale to {}: {e}", file.display());
        }
    }

    fn selection_range(&self) -> Range<u64> {
        self.selection.start() as u64..self.selection.end() as u64
    }

    /// Run an action from the table on the open file (menu, key or control channel). Returns
    /// whether the document changed.
    fn run_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.converting.is_some() {
            return Err("wait for the conversion to finish".to_string());
        }
        if actions::action(id).kind == Kind::Window {
            return self.run_window_action(id, ctx);
        }
        let mut sel = self.selection_range();
        // Markers go where play is, when it is playing (a point at the playhead).
        if matches!(id, ActionId::MarkerAdd | ActionId::MarkerNext | ActionId::MarkerPrev) && self.player.is_playing() {
            let at = self.player.position() as u64;
            sel = at..at;
        }
        if id == ActionId::Paste {
            self.take_system_clipboard();
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        match editor.run(id, sel) {
            Ok(mut out) => {
                if matches!(id, ActionId::Copy | ActionId::Cut)
                    && let Some(clip) = editor.clipboard()
                    && let Err(why) = self.os_clipboard.publish(clip)
                {
                    log::info!(target: "wavcraft", "not on the system clipboard: {why}");
                    out.message = Some(format!("Copied to clipboard {} only: {why}", editor.current_clipboard + 1));
                }
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                if matches!(id, ActionId::CursorToStart | ActionId::CursorToEnd | ActionId::MarkerNext | ActionId::MarkerPrev) {
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

    /// Before a Paste: if another program put an audio file on the system clipboard since we last
    /// looked, it becomes the current internal clipboard's clip, and the Paste uses it.
    fn take_system_clipboard(&mut self) {
        let Some(path) = self.os_clipboard.incoming() else { return };
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        match self.os_clipboard.take(&path, &opts) {
            Ok(clip) => {
                if let Some(e) = self.editor.as_mut() {
                    e.set_clipboard(clip);
                }
            }
            Err(why) => {
                log::warn!(target: "wavcraft", "audio on the system clipboard not used: {why}");
                self.notice = Some(format!("Audio on the system clipboard was not used: {why}"));
            }
        }
    }

    /// The actions the window carries out itself: zoom, In/Out points at the playhead, and the
    /// Insert Silence dialog.
    fn run_window_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        if id == ActionId::ToolTimeSelection {
            self.tool = Tool::TimeSelection;
            return Ok(false);
        }
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        let fmt = match id {
            ActionId::TimeDecimal => Some(TimeFormat::Decimal),
            ActionId::TimeCompact => Some(TimeFormat::Compact),
            ActionId::TimeSamples => Some(TimeFormat::Samples),
            ActionId::TimeSmpte30 => Some(TimeFormat::Smpte30),
            _ => None,
        };
        if let Some(fmt) = fmt {
            self.set_time_format(fmt);
            return Ok(false);
        }
        if frames == 0 && !matches!(id, ActionId::InsertSilence | ActionId::InsertFile) {
            return Err("no file is open".to_string());
        }
        let width = self.editor_width.max(1.0);
        // Where play is, or the cursor when stopped.
        let playhead = if self.player.is_playing() && !self.previewing { self.player.position() } else { self.selection.start() };
        match id {
            ActionId::ZoomIn => self.view.zoom_at(2.0, width * 0.5, width, frames),
            ActionId::ZoomOut => self.view.zoom_at(0.5, width * 0.5, width, frames),
            ActionId::ZoomOutFull => self.fit_on_next_frame = true,
            ActionId::ZoomAmpIn => self.amp_zoom.by(2.0),
            ActionId::ZoomAmpOut => self.amp_zoom.by(0.5),
            ActionId::ZoomAmpReset => self.amp_zoom = AmpZoom::full(),
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
            ActionId::ConvertSampleType if self.editor.is_some() => {
                let doc = self.editor.as_ref().map(|e| e.history.current().clone());
                self.convert_dialog = doc.map(|d| ConvertDialog::new(d.sample_rate(), d.channel_count()));
            }
            ActionId::InterpretSampleRate if self.editor.is_some() => {
                self.interpret = self.editor.as_ref().map(|e| e.history.current().sample_rate().to_string());
            }
            ActionId::InsertFile => self.pick_insert_file(ctx)?,
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
                } else if let Some(Repeat::Effect(i, params)) = editor.last.clone().filter(|r| matches!(r, Repeat::Effect(_, p) if !p.is_empty())) {
                    // An effect with settings reopens its dialog on the last values.
                    self.effect_dialog = EffectDialog::new(i, Some(params));
                } else {
                    return self.run_action(ActionId::RepeatLast, ctx);
                }
            }
            _ => return Err(format!("{} is not available now", actions::action(id).menu)),
        }
        Ok(false)
    }

    /// Edit › Mix Paste: paste the clipboard (or a file) at the cursor with the request's mode.
    /// One History step, "Mix Paste".
    fn run_mix_paste(&mut self, req: mixpaste::Request, ctx: &egui::Context) -> Result<(), String> {
        let at = self.selection.start() as u64;
        let source = match &req.file {
            Some(path) => Some(mixpaste::clip_from_file(path, &self.sessions.path)?),
            None => None,
        };
        let editor = self.editor.as_mut().ok_or("Mix Paste: no file is open")?;
        let out = editor.mix_paste(at, source, &req.opts, &AtomicBool::new(false)).map_err(|e| format!("Mix Paste: {e}"))?;
        self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
        self.notice = out.message;
        self.error = None;
        if out.doc_changed {
            self.rebuild(ctx);
        }
        Ok(())
    }

    fn mix_paste_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.mix_paste.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Mix Paste").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            choice = dialog.ui(ui);
        });
        match choice {
            Some(true) => match dialog.request() {
                Ok(req) => {
                    self.mix_paste = None;
                    if let Err(why) = self.run_mix_paste(req, ctx) {
                        self.error = Some(why);
                    }
                }
                Err(why) => self.error = Some(format!("Mix Paste: {why}")),
            },
            Some(false) => self.mix_paste = None,
            None => {}
        }
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

    /// Convert Sample Type on a worker thread (the dialog's OK, or the control channel). The
    /// result becomes one History step when it finishes.
    fn start_convert(&mut self, settings: &ConvertSettings, ctx: &egui::Context) -> Result<(), String> {
        if self.converting.is_some() || self.inserting.is_some() || self.saving.is_some() || self.opening.is_some() || self.rebuilding.is_some() {
            return Err("wait for the current job to finish".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let from = editor.history.current().clone();
        editor.history.can_afford(convert::estimate_bytes(&from, settings)).map_err(|e| format!("Convert Sample Type: {e}"))?;
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (job_from, job_settings, job_cancel, job_progress, ctx) = (from.clone(), settings.clone(), cancel.clone(), progress.clone(), ctx.clone());
        log::info!(target: "wavcraft", "convert sample type: {}", settings.to_text());
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
            let _ = tx.send(convert::convert(&job_from, &job_settings, &job_cancel, &mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.player.stop();
        self.converting = Some(Converting { from, result: rx, progress, cancel, doc_id: self.doc_id });
        Ok(())
    }

    fn finish_converting(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.converting else { return };
        let result = match job.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the conversion stopped unexpectedly".to_string()),
        };
        let Some(Converting { from, doc_id, .. }) = self.converting.take() else { return };
        let edit = match result {
            Ok(e) => e,
            Err(e) => {
                log::warn!(target: "wavcraft", "convert sample type failed: {e}");
                self.error = Some(format!("Convert Sample Type: {e}"));
                return;
            }
        };
        let Some(editor) = self.editor.as_mut().filter(|e| doc_id == self.doc_id && Arc::ptr_eq(e.history.current(), &from)) else {
            log::info!(target: "wavcraft", "a conversion finished after the document changed; dropped");
            return;
        };
        if edit.is_noop() {
            self.notice = Some("Convert Sample Type: nothing to change".to_string());
            return;
        }
        let (old_rate, new_rate, new_len) = (from.sample_rate(), edit.state.sample_rate(), edit.state.len_frames() as usize);
        self.notice = Some(format!(
            "Converted to {} Hz, {} channel(s), {} (a save writes {})",
            new_rate,
            edit.state.channel_count(),
            format_time(new_len, new_rate),
            SampleType::for_source(edit.state.source()).describe()
        ));
        editor.history.push_edit(edit);
        // The selection keeps its place in time.
        let scale = |f: usize| ((f as u128 * u128::from(new_rate) / u128::from(old_rate.max(1))) as usize).min(new_len);
        self.selection = Selection { anchor: scale(self.selection.anchor), head: scale(self.selection.head) };
        self.fit_on_next_frame = true;
        self.error = None;
        self.rebuild(ctx);
    }

    /// Interpret Sample Rate: relabel the rate, one History step, no sample touched.
    fn run_interpret(&mut self, rate: u32, ctx: &egui::Context) -> Result<(), String> {
        if self.converting.is_some() || self.saving.is_some() {
            return Err("wait for the current job to finish".to_string());
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let edit = convert::interpret_rate(editor.history.current(), rate).map_err(|e| format!("Interpret Sample Rate: {e}"))?;
        if edit.is_noop() {
            return Ok(());
        }
        editor.history.push_edit(edit);
        self.notice = Some(format!("Interpreting the file as {rate} Hz"));
        self.error = None;
        self.fit_on_next_frame = true;
        self.rebuild(ctx);
        Ok(())
    }

    /// The two conversion dialogs, when up.
    fn convert_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(d) = self.convert_dialog.as_mut() {
            match d.show(ctx, &mut self.presets) {
                convert_ui::Outcome::Open => {}
                convert_ui::Outcome::Cancel => self.convert_dialog = None,
                convert_ui::Outcome::Apply(s) => {
                    self.convert_dialog = None;
                    if let Err(e) = self.start_convert(&s, ctx) {
                        self.error = Some(e);
                    }
                }
            }
        }
        if let Some(text) = self.interpret.as_mut() {
            let current = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
            if let Some(answer) = convert_ui::interpret_dialog(ctx, text, current) {
                self.interpret = None;
                if let Some(text) = answer {
                    match text.trim().parse::<u32>() {
                        Ok(rate) => {
                            if let Err(e) = self.run_interpret(rate, ctx) {
                                self.error = Some(e);
                            }
                        }
                        Err(_) => self.error = Some("Interpret Sample Rate: type a whole number of Hz".to_string()),
                    }
                }
            }
        }
    }

    /// The selection as `<start>-<end>` in the chosen format (STATE reports it).
    fn selection_text(&self) -> String {
        format!("{}-{}", self.fmt_time(self.selection.start()), self.fmt_time(self.selection.end()))
    }

    /// Window › Time panel: the playhead (or, stopped, the cursor) as a large readout.
    fn time_panel(&mut self, ui: &mut egui::Ui) {
        let frame = if self.player.is_playing() { self.player.position() } else { self.selection.start() };
        let text = if self.doc.is_some() { self.fmt_time(frame) } else { self.time_format.format(0, 0) };
        ui.horizontal(|ui| {
            ui.label("Time");
            ui.label(egui::RichText::new(text).monospace().size(34.0));
            ui.label(self.time_format.menu());
        });
    }

    /// Window › Selection/View panel: begin, end and duration of the selection and of the visible
    /// view, in the chosen format. Type a value and press Enter (or leave the box) to apply it.
    fn selection_view_panel(&mut self, ui: &mut egui::Ui) {
        let (frames, rate) = self.doc.as_ref().map_or((0, 0), |d| (d.frames(), d.sample_rate()));
        let width = self.editor_width.max(1.0);
        let (vs, ve) = (self.view.start.round() as usize, (self.view.end(width).round() as usize).min(frames));
        let (ss, se) = (self.selection.start(), self.selection.end());
        let values =
            [("sel_start", ss), ("sel_end", se), ("sel_len", se - ss), ("view_start", vs), ("view_end", ve), ("view_len", ve.saturating_sub(vs))];
        let mut committed = None;
        egui::Grid::new("selection_view").num_columns(4).show(ui, |ui| {
            ui.label("");
            ui.strong("Begin");
            ui.strong("End");
            ui.strong("Duration");
            ui.end_row();
            for (row, label) in [(0, "Selection"), (3, "View")] {
                ui.label(label);
                for (name, value) in &values[row..row + 3] {
                    if let Some(frame) = self.time_box(ui, name, *value, rate) {
                        committed = Some((*name, frame));
                    }
                }
                ui.end_row();
            }
        });
        let Some((name, frame)) = committed else { return };
        if self.doc.is_none() {
            return;
        }
        match name {
            "sel_start" => self.selection = self.selection.with_start(frame, frames),
            "sel_end" => self.selection = self.selection.with_end(frame, frames),
            "sel_len" => self.selection = self.selection.with_len(frame, frames),
            "view_start" => self.view.set_start(frame, width, frames),
            "view_end" => self.view.set_end(frame, width, frames),
            _ => self.view.set_duration(frame, width, frames),
        }
        if name.starts_with("sel") {
            self.player.stop();
        }
    }

    /// One text box of the Selection/View panel. Shows `value` formatted until it is clicked into;
    /// returns the frame typed when Enter is pressed or the box is left (Escape drops the text).
    fn time_box(&mut self, ui: &mut egui::Ui, name: &'static str, value: usize, rate: u32) -> Option<usize> {
        let editing = matches!(&self.panel_edit, Some((n, _)) if *n == name);
        let mut text = match (&self.panel_edit, editing) {
            (Some((_, t)), true) => t.clone(),
            _ => self.time_format.format(value, rate),
        };
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).font(egui::TextStyle::Monospace));
        if r.changed() || (r.has_focus() && !editing) {
            self.panel_edit = Some((name, text.clone()));
        }
        if !r.lost_focus() {
            return None;
        }
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        self.panel_edit = None;
        if escaped || rate == 0 {
            return None;
        }
        match self.time_format.parse(&text, rate) {
            Some(frame) if self.time_format.format(frame, rate) != self.time_format.format(value, rate) => Some(frame),
            Some(_) => None,
            None => {
                self.error = Some(format!("\"{text}\" is not a time in the {} format", self.time_format.menu()));
                None
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
        let mut delete = None;
        let mut focus = false;
        let refocus = self.history_refocus.take();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                let r = ui.selectable_label(i == cursor, label);
                if refocus == Some(i) {
                    r.request_focus();
                    r.scroll_to_me(None);
                }
                focus |= r.has_focus();
                if r.clicked() && i != cursor {
                    go = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Delete State")).on_hover_text("Removes this state and every later one").clicked() {
                        delete = Some(i);
                        ui.close();
                    }
                });
            }
        });
        self.history_focus = focus || refocus.is_some();
        ui.separator();
        let free = wavcraft_core::space::free_space(&dir).map_or_else(|| "unknown".to_string(), |b| format!("{:.1} GB", b as f64 / 1e9));
        ui.label(format!("Session: {:.1} MB · free {free}", held as f64 / 1e6));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(cursor > 0, egui::Button::new("Delete State"))
                .on_hover_text("Removes the highlighted state and every later one")
                .clicked()
            {
                delete = Some(cursor);
            }
            if ui.add_enabled(labels.len() > 1, egui::Button::new("Clear History…")).clicked() {
                self.confirm_clear_history = true;
            }
        });
        if self.saving.is_none() && self.converting.is_none() {
            if let Some(i) = delete {
                self.history_delete(i, ui.ctx());
            } else if let Some(i) = go {
                self.history_go(i, ui.ctx());
            }
        }
    }

    /// Go to History step `i` (a click, an arrow key or the control channel). False if there is none.
    fn history_go(&mut self, i: usize, ctx: &egui::Context) -> bool {
        let Some(editor) = self.editor.as_mut() else { return false };
        if editor.history.go_to(i).is_none() {
            return false;
        }
        self.history_changed(ctx);
        true
    }

    /// Delete History step `i` and the steps after it. The steps removed, or `None`.
    fn history_delete(&mut self, i: usize, ctx: &egui::Context) -> Option<usize> {
        let editor = self.editor.as_mut()?;
        let before = editor.history.current().clone();
        let gone = editor.history.delete_step(i)?;
        if !Arc::ptr_eq(&before, editor.history.current()) {
            self.history_changed(ctx);
        }
        Some(gone)
    }

    /// The current step changed: keep the selection inside it and redraw.
    fn history_changed(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.editor.as_ref() else { return };
        let len = editor.history.current().len_frames() as usize;
        self.selection = Selection { anchor: self.selection.start().min(len), head: self.selection.end().min(len) };
        self.rebuild(ctx);
    }

    /// Up or Down with the History panel focused: move one state. The cursor's new index.
    fn history_arrow(&mut self, down: bool, ctx: &egui::Context) -> Option<usize> {
        let cursor = self.editor.as_ref()?.history.cursor();
        let to = history_arrow_target(cursor, self.editor.as_ref()?.history.steps().len(), down)?;
        self.history_go(to, ctx).then_some(to)
    }

    /// Draw the effect dialog, if up, and carry out its answer.
    fn effect_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.effect_dialog.as_mut() else { return };
        match dialog.show(ctx) {
            Some(Answer::Preview) => {
                if let Err(why) = self.start_preview() {
                    self.error = Some(why);
                }
            }
            Some(answer) => {
                let _ = self.finish_effect_dialog(answer == Answer::Ok, ctx);
            }
            None => {}
        }
        // The preview loops while the dialog is up.
        if self.previewing && !self.player.is_playing() {
            let _ = self.start_preview();
        }
        if self.previewing {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Play a short loop of the open Generate dialog's sound through the player (f21). The file
    /// is put back by [`WaveApp::end_preview`] when the dialog closes. Returns the preview's
    /// length in frames.
    fn start_preview(&mut self) -> Result<usize, String> {
        let Some(dialog) = self.effect_dialog.as_ref().filter(|d| d.generator) else { return Err("no generator dialog is open".to_string()) };
        let Some(doc) = self.doc.as_ref() else { return Err("no file is open".to_string()) };
        let g = wavcraft_core::generate::all()[usize::from(dialog.index)];
        wavcraft_core::generate::validate(g, &dialog.values)?;
        let rate = doc.sample_rate();
        let secs = g.duration(&dialog.values).min(PREVIEW_SECONDS);
        let frames = (secs * f64::from(rate)).round() as usize;
        let channels = wavcraft_core::generate::render(g, &dialog.values, rate, doc.audio.channels.len().max(1), frames, PREVIEW_SEED);
        self.player.set_audio(Some(Arc::new(wavcraft_core::AudioBuffer { sample_rate: rate, channels })));
        self.player.play(0, frames);
        self.previewing = true;
        Ok(frames)
    }

    /// Stop a preview and give the player the open file back.
    fn end_preview(&mut self) {
        if self.previewing {
            self.previewing = false;
            self.player.set_audio(self.doc.as_ref().map(|d| d.audio.clone()));
        }
    }

    /// Generate › Tones / Noise: insert at the cursor, or replace the selection (f21).
    fn run_generator(&mut self, index: u16, params: Vec<f64>, seed: u64, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let name = wavcraft_core::generate::all().get(usize::from(index)).map_or("Generate", |g| g.name());
        match editor.generate(index, &params, sel, seed) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{name}: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// The Generate menu: one entry per generator, each opening its dialog.
    fn generate_menu(&mut self, ui: &mut egui::Ui) {
        let enabled = self.editor.is_some() && self.saving.is_none();
        for (i, g) in wavcraft_core::generate::all().iter().enumerate() {
            if ui.add_enabled(enabled, egui::Button::new(format!("{}…", g.name().trim_start_matches("Generate ")))).clicked() {
                self.effect_dialog = EffectDialog::new_generator(i as u16, None);
                ui.close();
            }
        }
    }

    /// Close the effect dialog; on OK apply the effect with the dialog's values. Returns whether
    /// the document changed.
    fn finish_effect_dialog(&mut self, ok: bool, ctx: &egui::Context) -> Result<bool, String> {
        let Some(dialog) = self.effect_dialog.take() else { return Err("no effect dialog is open".to_string()) };
        self.end_preview();
        if !ok {
            return Ok(false);
        }
        if dialog.generator {
            return self.run_generator(dialog.index, dialog.values, clock_seed(), ctx);
        }
        self.run_effect(dialog.index, dialog.values, ctx)
    }

    /// Run the effect at `index` with `params` on the selection (the whole file when none).
    fn run_effect(&mut self, index: u16, params: Vec<f64>, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let name = wavcraft_core::effects::all().get(usize::from(index)).map_or("Effect", |e| e.name());
        if !editor.enabled(ActionId::Effect(index), &sel) {
            return Err(format!("{name} is not available now"));
        }
        match editor.run_effect(index, params, sel) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{name}: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
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
        is_unsaved(&self.editor, &self.saved)
    }

    /// Whether any open document has unsaved changes.
    fn any_unsaved(&self) -> bool {
        self.unsaved() || self.docs.parked().iter().any(|s| is_unsaved(&s.editor, &s.saved))
    }

    /// File › Save: to the file's own path when it is a WAV (the only format written so far),
    /// else as Save As. `to` skips the dialog (the control channel).
    fn save(&mut self, ctx: &egui::Context, save_as: bool, to: Option<PathBuf>, then_close: bool) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        if self.converting.is_some() {
            return Err("wait for the conversion to finish".to_string());
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
                if let Some(slot) = self.docs.parked_mut().iter_mut().find(|s| s.doc_id == doc_id) {
                    slot.saved = Some(state);
                    slot.doc_path = path.clone();
                    if let Some(doc) = slot.doc.as_mut() {
                        doc.path = path.clone();
                    }
                    self.notice = Some(format!("Saved {} ({} WAV)", path.display(), ty.describe()));
                } else {
                    log::info!(target: "wavcraft", "a save of a closed file finished: {}", path.display());
                    self.notice = Some(format!("Saved {} ({} WAV), a file closed meanwhile", path.display(), ty.describe()));
                }
            }
            Ok(()) => {
                self.saved = Some(state);
                self.doc_path = path.clone();
                if let Some(doc) = self.doc.as_mut() {
                    doc.path = path.clone();
                }
                self.error = None;
                self.notice = Some(format!("Saved {} ({} WAV)", path.display(), ty.describe()));
                if then_close && std::mem::take(&mut self.closing_doc) {
                    self.remove_current();
                } else if then_close {
                    // Not allow_close: another document may still have unsaved changes.
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
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.any_unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
            self.closing_doc = false;
            // Ask about the current document first; else bring the first unsaved one up.
            if !self.unsaved()
                && let Some(p) = self.docs.parked().iter().position(|s| is_unsaved(&s.editor, &s.saved))
            {
                let _ = self.switch_doc(if p < self.docs.current() { p } else { p + 1 });
            }
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
                    self.closing_doc = false;
                }
            }
            // Drop this document's changes; closing the window goes on to the next unsaved one.
            Some(1) => {
                self.confirm_close = false;
                let only_doc = std::mem::take(&mut self.closing_doc);
                self.remove_current();
                if !only_doc {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Some(_) => {
                self.confirm_close = false;
                self.closing_doc = false;
            }
            None => {}
        }
    }

    /// Shift+Left/Right/Home/End extend the selection (taken before the action table, whose
    /// Home and End would otherwise also fire).
    fn selection_keys(&mut self, ctx: &egui::Context) {
        let Some(frames) = self.doc.as_ref().map(|d| d.frames()) else { return };
        let step = self.view.frames_per_px.round().max(1.0) as i64;
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, k));
        for (key, name) in [(Key::ArrowLeft, "shift_left"), (Key::ArrowRight, "shift_right"), (Key::Home, "shift_home"), (Key::End, "shift_end")] {
            if pressed(key) {
                self.gesture(name, step, frames);
            }
        }
    }

    /// The selection gestures by name, shared by the keys and the control channel.
    fn gesture(&mut self, name: &str, step: i64, frames: usize) -> bool {
        match name {
            "shift_left" => self.selection.extend_by(-step, frames),
            "shift_right" => self.selection.extend_by(step, frames),
            "shift_home" => self.selection.extend_edge_to(0, frames),
            "shift_end" => self.selection.extend_edge_to(frames, frames),
            _ => return false,
        }
        self.view.reveal(self.selection.head as f64, self.editor_width, frames);
        true
    }

    /// Answer `GESTURE <name> [args]` (see `control.rs`): the pointer gestures at a frame, as
    /// the editor would see them.
    fn control_gesture(&mut self, rest: &str) -> String {
        let frames = self.doc.as_ref().map_or(0, |d| d.frames());
        let mut it = rest.split_whitespace();
        let name = it.next().unwrap_or("");
        let nums: Vec<usize> = it.map_while(|w| w.parse().ok()).collect();
        let at = |i: usize| nums.get(i).copied().map(|f| f.min(frames));
        match (name, at(0)) {
            ("shift_click", Some(f)) => self.selection.extend_to(f),
            ("double_click", _) => self.selection = Selection::visible(&self.view, self.editor_width, frames),
            ("triple_click", _) => self.selection = Selection::all(frames),
            // Press near an edge at <from>, drag to <to>.
            ("edge_drag", Some(from)) => {
                let (Some(to), true) = (at(1), self.view.frames_per_px > 0.0) else { return "ERR GESTURE edge_drag <from> <to>".to_string() };
                match self.selection.edge_near(&self.view, self.view.x_of(from as f64), GRAB_PX) {
                    Some(edge) => {
                        self.selection.grab(edge);
                        self.selection.head = to;
                    }
                    None => return format!("ERR no selection edge near frame {from}"),
                }
            }
            ("shift_left" | "shift_right", _) => {
                let step = at(0).map_or(1, |n| n as i64);
                self.gesture(name, step, frames);
            }
            ("shift_home" | "shift_end", _) => {
                self.gesture(name, 0, frames);
            }
            // Right-click at <frame>: the nearest edge of the selection moves there (or nothing, with the
            // "context menu" setting).
            ("right_click", Some(f)) => {
                if self.editor.is_none() {
                    return "ERR no file is open".to_string();
                }
                let moved = self.right_click_at(f, frames);
                return format!("OK selection={}-{} moved={}", self.selection.start(), self.selection.end(), u8::from(moved));
            }
            _ => return "ERR GESTURE right_click <frame> | shift_click <frame> | double_click | triple_click | edge_drag <from> <to> | shift_left|shift_right [frames] | shift_home | shift_end".to_string(),
        }
        format!("OK selection={}-{}", self.selection.start(), self.selection.end())
    }

    /// Answer one control-channel line (see `control.rs`).
    fn control_request(&mut self, line: &str, ctx: &egui::Context) -> String {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let sel = |s: &Self| format!("{}-{}", s.selection.start(), s.selection.end());
        match cmd {
            "STATE" => {
                let busy = self.opening.is_some()
                    || self.inserting.is_some()
                    || self.rebuilding.is_some()
                    || self.saving.is_some()
                    || self.converting.is_some();
                let unsaved = u8::from(self.unsaved());
                match &self.editor {
                    Some(e) => {
                        let doc = e.history.current();
                        format!(
                            "OK frames={} channels={} rate={} selection={} steps={} cursor={} busy={} unsaved={unsaved} snap={} snapzero={} enabled={} clipboard={} clip_frames={} time_display={} selection_text={} tool={} right_click={} view={} vzoom={:.2} ampscale={} undo={} redo={}",
                            doc.len_frames(),
                            doc.channel_count(),
                            doc.sample_rate(),
                            sel(self),
                            e.history.steps().len(),
                            e.history.cursor(),
                            u8::from(busy),
                            u8::from(e.snap.enabled),
                            u8::from(e.snap.zero_crossings),
                            e.enabled_flags(),
                            e.current_clipboard + 1,
                            e.clipboard().map_or(0, |c| c.len_frames()),
                            self.time_format.name(),
                            self.selection_text(),
                            self.tool.name(),
                            if self.settings.right_click_menu { "menu" } else { "extend" },
                            self.view_text(),
                            self.amp_zoom.get(),
                            self.settings.amp_scale.name(),
                            e.history.undo_label().unwrap_or("-"),
                            e.history.redo_label().unwrap_or("-")
                        )
                    }
                    None => format!(
                        "OK frames=0 channels=0 rate=0 selection=0-0 steps=0 cursor=0 busy={} unsaved=0 snap=1 snapzero=0 enabled= clipboard=1 clip_frames=0 time_display={} selection_text=- tool={} right_click={} view=0-0 vzoom=1.00 ampscale={} undo=- redo=-",
                        u8::from(busy),
                        self.time_format.name(),
                        self.tool.name(),
                        if self.settings.right_click_menu { "menu" } else { "extend" },
                        self.settings.amp_scale.name()
                    ),
                }
            }
            // The snap step a selection gesture calls, for scripted checks: `SNAP <frame> [frames_per_px]`
            // (the view's zoom when omitted); the selection's edges are the extra targets.
            "SNAP" => {
                let mut it = rest.split_whitespace();
                let frame = it.next().and_then(|f| f.parse::<u64>().ok());
                let fpp = it.next().map_or(Some(self.view.frames_per_px), |f| f.parse::<f64>().ok());
                match (frame, fpp, self.editor.as_ref()) {
                    (Some(frame), Some(fpp), Some(e)) => {
                        format!("OK frame={}", e.snap(frame, fpp, &[self.selection.start() as u64, self.selection.end() as u64]))
                    }
                    _ => "ERR SNAP <frame> [frames_per_px], with a file open".to_string(),
                }
            }
            // Scripted checks of the system clipboard: what is on it, or put a file there as if
            // another program had.
            "OSCLIP" => match rest.split_once(' ') {
                Some(("set", path)) => match self.os_clipboard.put_foreign(Path::new(path)) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                },
                _ => {
                    let (first, ours) = self.os_clipboard.peek();
                    format!("OK ours={} path={}", u8::from(ours), first.map_or("-".to_string(), |p| p.display().to_string()))
                }
            },
            // The markers of the current state, in order: `OK count=<n> <id>,<start>,<end>,<name>;…`.
            "MARKERS" => {
                let list: Vec<String> = self.editor.as_ref().map_or_else(Vec::new, |e| {
                    e.history.current().markers().iter().map(|m| format!("{},{},{},{}", m.id, m.start, m.end, m.name)).collect()
                });
                format!("OK count={} {}", list.len(), list.join(";"))
            }
            "OPEN" if !rest.is_empty() => {
                self.open(PathBuf::from(rest), ctx);
                "OK".to_string()
            }
            // Insert › Audio from File at the cursor (or over the selection); poll STATE until busy=0.
            "INSERT_FILE" if !rest.is_empty() => match self.insert_file(PathBuf::from(rest), ctx) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {e}"),
            },
            "INSERT_FILE" => "ERR INSERT_FILE <path>".to_string(),
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
            // Window › <panel>: `window.toggle.<id>`, `window.show.<id>`, `window.hide.<id>`.
            "ACTION" if rest.starts_with("window.") => {
                let mut next = self.panels.clone();
                match next.apply_action(rest.trim()) {
                    Some(Ok(_)) => {
                        let ids: Vec<&str> =
                            panels::REGISTRY.iter().map(|p| p.id).filter(|id| next.is_shown(id) != self.panels.is_shown(id)).collect();
                        for id in ids {
                            self.set_panel(id, next.is_shown(id));
                        }
                        format!("OK selection={} changed=0 {}", sel(self), self.panels.report())
                    }
                    Some(Err(e)) => format!("ERR {e}"),
                    None => format!("ERR no action named {rest:?}"),
                }
            }
            // Which panels are shown: `OK waveform=1 spectral=1 history=0`.
            "PANELS" => format!("OK {}", self.panels.report()),
            "GESTURE" => self.control_gesture(rest),
            "SELECT_TIME" => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                let fmt = self.time_format;
                let mut it = rest.split_whitespace().map(|t| fmt.parse(t, rate));
                match (it.next(), it.next(), it.next()) {
                    (Some(Some(a)), Some(Some(b)), None) => {
                        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames() as usize);
                        self.selection = Selection { anchor: a.min(len), head: b.min(len) };
                        format!("OK selection={}", sel(self))
                    }
                    _ => format!("ERR SELECT_TIME <start> <end>, each written as {} (no spaces inside a time)", self.time_format.menu()),
                }
            }
            // Set the visible range the way the zoom navigator does, in frames.
            "NAV" => {
                let mut it = rest.split_whitespace().map(str::parse::<f64>);
                let frames = self.doc.as_ref().map_or(0, Document::frames);
                match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) if frames > 0 && a.is_finite() && b > a => {
                        self.view.set_range(a, b, self.editor_width.max(1.0), frames);
                        format!("OK view={}", self.view_text())
                    }
                    _ => "ERR NAV <start> <end>, in frames, with a file open".to_string(),
                }
            }
            // Choose the amplitude ruler's scale (the ruler's context menu does the same).
            "AMPSCALE" => match AmpScale::parse(rest) {
                Some(scale) => {
                    self.set_amp_scale(scale);
                    format!("OK ampscale={}", scale.name())
                }
                None => "ERR AMPSCALE db|percent|samples|normalized".to_string(),
            },
            "ACTION" => match actions::id_by_name(rest) {
                Some(id) => match self.run_action(id, ctx) {
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
            // A number the effects use: `PARAM amplify.db 6`, `PARAM normalize.db -1`.
            "PARAM" => {
                let mut it = rest.split_whitespace();
                match (it.next(), it.next().and_then(|v| v.parse::<f32>().ok()), self.editor.as_mut()) {
                    (Some(name), Some(v), Some(e)) => match e.set_param(name, v) {
                        Ok(()) => "OK".to_string(),
                        Err(why) => format!("ERR {why}"),
                    },
                    _ => "ERR PARAM <amplify.db|normalize.db> <number>, with a file open".to_string(),
                }
            }
            // `EFFECT` lists the effects and their settings; `EFFECT <name> key=value ...` runs one
            // on the selection (the whole file when none), the unset keys at their defaults.
            "EFFECT" if rest.is_empty() => {
                let all: Vec<String> =
                    wavcraft_core::effects::all().iter().map(|e| format!("{} [{}]", e.id(), wavcraft_core::effects::describe(*e))).collect();
                format!("OK {}", all.join("; "))
            }
            "EFFECT" => {
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let name = name.strip_prefix("effect.").unwrap_or(name);
                match wavcraft_core::effects::all().iter().position(|e| e.id() == name) {
                    None => format!("ERR no effect named {name:?}"),
                    Some(i) => match wavcraft_core::effects::parse_params(wavcraft_core::effects::all()[i], args) {
                        Err(e) => format!("ERR {e}"),
                        Ok(params) => match self.run_effect(i as u16, params, ctx) {
                            Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                            Err(e) => format!("ERR {e}"),
                        },
                    },
                }
            }
            // The effect dialog: `EFFECT_DIALOG <name> [key=value ...]` opens it on those values,
            // `EFFECT_DIALOG` says what is open, `EFFECT_DIALOG OK|CANCEL` presses the button.
            "EFFECT_DIALOG" => match rest.trim() {
                "" => match &self.effect_dialog {
                    Some(d) => format!("OK open={} values={:?}", d.id(), d.values),
                    None => "OK open=none".to_string(),
                },
                "OK" | "CANCEL" => match self.finish_effect_dialog(rest.trim() == "OK", ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                _ => {
                    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    let name = name.strip_prefix("effect.").unwrap_or(name);
                    match wavcraft_core::effects::all().iter().position(|e| e.id() == name) {
                        Some(i) if self.editor.is_some() => match wavcraft_core::effects::parse_params(wavcraft_core::effects::all()[i], args) {
                            Err(e) => format!("ERR {e}"),
                            Ok(v) => {
                                self.effect_dialog = EffectDialog::new(i as u16, Some(v));
                                "OK".to_string()
                            }
                        },
                        _ => format!("ERR no effect named {name:?}, or no file is open"),
                    }
                }
            },
            // Edit › Mix Paste (see `mixpaste::parse` for the settings).
            "MIXPASTE" => match mixpaste::parse(rest) {
                Ok(req) => match self.run_mix_paste(req, ctx) {
                    Ok(()) => format!("OK selection={}", sel(self)),
                },
            },
            // Generate (f21): `GENERATE` lists the generators; `GENERATE tone key=value ... [seed=N]`
            // inserts at the cursor or replaces the selection. `GENERATE_DIALOG <name> [key=value ...]`
            // opens the dialog, `GENERATE_DIALOG PREVIEW` starts its preview (`OK preview=<frames>`),
            // `GENERATE_DIALOG OK|CANCEL` presses a button.
            "GENERATE" if rest.is_empty() => {
                let all: Vec<String> = wavcraft_core::generate::all()
                    .iter()
                    .map(|g| format!("{} [{}]", g.id(), wavcraft_core::effects::describe_list(g.parameters())))
                    .collect();
                format!("OK {}", all.join("; "))
            }
            "GENERATE" => {
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let (seed, args) = split_seed(args);
                match wavcraft_core::generate::find(name) {
                    None => format!("ERR no generator named {name:?}"),
                    Some(g) => match (wavcraft_core::generate::parse_params(g, &args), seed) {
                        (Err(e), _) | (_, Err(e)) => format!("ERR {e}"),
                        (Ok(params), Ok(seed)) => {
                            let i = wavcraft_core::generate::all().iter().position(|x| x.id() == g.id()).unwrap_or(0);
                            match self.run_generator(i as u16, params, seed, ctx) {
                                Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                                Err(e) => format!("ERR {e}"),
                            }
                        }
                    },
                }
            }
            "GENERATE_DIALOG" => match rest.trim() {
                "OK" | "CANCEL" => match self.finish_effect_dialog(rest.trim() == "OK", ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                "PREVIEW" => match self.start_preview() {
                    Ok(frames) => format!("OK preview={frames}"),
                    Err(e) => format!("ERR {e}"),
                },
                "" => match &self.effect_dialog {
                    Some(d) => format!("OK open={} values={:?} previewing={}", d.id(), d.values, u8::from(self.previewing)),
                    None => "OK open=none".to_string(),
                },
                _ => {
                    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    match wavcraft_core::generate::all().iter().position(|g| g.id() == name) {
                        Some(i) if self.editor.is_some() => match wavcraft_core::generate::parse_params(wavcraft_core::generate::all()[i], args) {
                            Err(e) => format!("ERR {e}"),
                            Ok(v) => {
                                self.effect_dialog = EffectDialog::new_generator(i as u16, Some(v));
                                "OK".to_string()
                            }
                        },
                        _ => format!("ERR no generator named {name:?}, or no file is open"),
                    }
                }
            },
            "INSERT_SILENCE" => match rest.trim().parse::<f64>() {
                Ok(secs) if secs.is_finite() && secs > 0.0 => match self.run_insert_silence(secs, ctx) {
                    None => format!("OK selection={}", sel(self)),
                    Some(e) => format!("ERR {e}"),
                },
                _ => "ERR INSERT_SILENCE <seconds>, above 0".to_string(),
            },
            // Auto Save: the journal writer's progress, and the sessions a crashed run left.
            "AUTOSAVE" => {
                let (offered, written, failed, pending) = self.recovery.stats();
                format!(
                    "OK offered={offered} written={written} failed={failed} pending={} scanned={} recoverable={}",
                    u8::from(pending),
                    u8::from(self.recovery.scanned),
                    self.recovery.offers.len()
                )
            }
            "RECOVER" | "DISCARD_RECOVERY" => match rest.trim().parse::<usize>().or_else(|_| if rest.trim().is_empty() { Ok(0) } else { Err(()) }) {
                Ok(i) => {
                    let r = if cmd == "RECOVER" { self.recover_session(i, ctx) } else { self.discard_recovery(i) };
                    r.map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string())
                }
                Err(()) => format!("ERR {cmd} [index]"),
            },
            // Convert Sample Type with `key=value` settings (`rate=44100 channels=1 bits=16`), or
            // `preset=<name>`; it runs in the background: poll STATE until busy=0.
            "CONVERT" => {
                let parsed = match rest.strip_prefix("preset=") {
                    Some(name) => self.presets.find(name).map(|p| p.settings).ok_or_else(|| format!("no preset named {:?}", name.trim())),
                    None => ConvertSettings::parse(rest),
                };
                match parsed.and_then(|s| self.start_convert(&s, ctx)) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // Interpret Sample Rate: relabel the rate, no resampling.
            "INTERPRET" => match rest.trim().parse::<u32>() {
                Ok(rate) => match self.run_interpret(rate, ctx) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                },
                Err(_) => "ERR INTERPRET <hz>".to_string(),
            },
            // PRESET list | PRESET save <name> | <settings> | PRESET delete <name>
            "PRESET" => {
                let (verb, arg) = rest.split_once(' ').unwrap_or((rest, ""));
                match verb {
                    "list" => format!(
                        "OK {}",
                        self.presets.all().iter().map(|p| format!("{} | {}", p.name, p.settings.to_text())).collect::<Vec<_>>().join(" ; ")
                    ),
                    "save" => match arg.split_once('|') {
                        Some((name, settings)) => match ConvertSettings::parse(settings).and_then(|s| self.presets.save(name, s)) {
                            Ok(()) => "OK".to_string(),
                            Err(e) => format!("ERR {e}"),
                        },
                        None => "ERR PRESET save <name> | <settings>".to_string(),
                    },
                    "delete" => match self.presets.delete(arg) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                    _ => "ERR PRESET list | save <name> | <settings> | delete <name>".to_string(),
                }
            }
            // The History panel's click: go to step <index> (0 is Open).
            "HISTORY" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true) if self.saving.is_none() && self.converting.is_none() => {
                    if self.history_go(i, ctx) {
                        "OK".to_string()
                    } else {
                        format!("ERR no step {i}")
                    }
                }
                _ => "ERR HISTORY <step>, with a file open".to_string(),
            },
            // The panel's Delete State: step <index> and every later step go (0 can't).
            "HISTORY_DELETE" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true) if self.saving.is_none() && self.converting.is_none() => match self.history_delete(i, ctx) {
                    Some(n) => format!("OK removed={n}"),
                    None => format!("ERR no step {i} to delete (Open can't be)"),
                },
                _ => "ERR HISTORY_DELETE <step>, with a file open".to_string(),
            },
            // Clear History without the panel's confirmation: the current state is all that stays.
            "HISTORY_CLEAR" => match self.editor.as_mut() {
                Some(e) if self.saving.is_none() && self.converting.is_none() => {
                    e.history.clear();
                    "OK".to_string()
                }
                _ => "ERR HISTORY_CLEAR, with a file open".to_string(),
            },
            // The panel's Up / Down keys.
            "HISTORY_KEY" if self.editor.is_some() && self.saving.is_none() && self.converting.is_none() => match rest.trim() {
                k @ ("up" | "down") => match self.history_arrow(k == "down", ctx) {
                    Some(i) => format!("OK cursor={i}"),
                    None => "OK cursor=unchanged".to_string(),
                },
                _ => "ERR HISTORY_KEY up|down".to_string(),
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
            // The devices the system offers, and the one the player is on.
            "DEVICES" => {
                let d = crate::player::list_devices();
                format!("OK output={} input={} playing_on={}", d.output.join("|"), d.input.join("|"), self.player.device)
            }
            // The open documents, in the order opened, and the current one.
            "DOCS" => {
                let n = self.docs.len(self.editor.is_some());
                let mut out = format!("OK count={n} current={}", self.docs.current());
                for i in 0..n {
                    let (name, unsaved, frames) = self.doc_info(i);
                    out.push_str(&format!(" | {i} unsaved={} frames={frames} {name}", u8::from(unsaved)));
                }
                out
            }
            "SWITCH" => match rest.trim().parse::<usize>().map_err(|_| "SWITCH <index>, from DOCS".to_string()).and_then(|i| self.switch_doc(i)) {
                Ok(()) => format!("OK current={}", self.docs.current()),
                Err(e) => format!("ERR {e}"),
            },
            // Close the current document; with unsaved changes `ERR …` unless `CLOSE discard`.
            "CLOSE" if self.unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or CLOSE discard".to_string(),
            "CLOSE" if self.editor.is_none() => "ERR no file is open".to_string(),
            "CLOSE" if self.saving.is_some() => "ERR wait for the save to finish".to_string(),
            "CLOSE" => {
                self.remove_current();
                format!("OK count={}", self.docs.len(self.editor.is_some()))
            }
            "SAVE" => {
                let to = (!rest.is_empty()).then(|| PathBuf::from(rest));
                match self.save(ctx, to.is_some(), to, false) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // A plain QUIT refuses with unsaved changes; `QUIT discard` closes anyway.
            "QUIT" if self.any_unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or QUIT discard".to_string(),
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
            if matches!(
                a.id,
                ActionId::Cut
                    | ActionId::Clipboard1
                    | ActionId::Delete
                    | ActionId::Crop
                    | ActionId::ZeroInward
                    | ActionId::SetIn
                    | ActionId::LeftChannel
            ) {
                ui.separator();
            }
            let (enabled, label) = self.editor.as_ref().map_or((false, a.menu.to_string()), |e| (e.enabled(a.id, &sel), e.label(a.id)));
            let enabled = enabled && self.saving.is_none() && self.converting.is_none();
            let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        ui.separator();
        for a in TABLE.iter().filter(|a| a.name.starts_with("marker.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
            if ui.add_enabled(enabled, egui::Button::new(a.menu).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        if ui.add_enabled(self.editor.is_some() && self.saving.is_none(), egui::Button::new("Mix Paste…")).clicked() {
            self.mix_paste = Some(mixpaste::Dialog::new());
            ui.close();
        }
        if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
        }
        ui.separator();
        if ui.add(egui::Button::new("Preferences…").shortcut_text(if mac { "⌘," } else { "Ctrl+," })).clicked() {
            self.open_preferences();
        }
    }

    /// The Effects menu: the one-shot effects from the action table (`effect.*`), run on the
    /// selection (the whole file with none). Amplify and Normalize use the numbers set by `PARAM`.
    fn effects_menu(&mut self, ui: &mut egui::Ui) {
        let sel = self.selection_range();
        let mut chosen = None;
        for a in actions::all_actions().filter(|a| a.name.starts_with("effect.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            // An effect with settings opens its dialog (a "…" entry); one without runs at once.
            let has_dialog =
                matches!(a.id, ActionId::Effect(i) if wavcraft_core::effects::all().get(usize::from(i)).is_some_and(|e| !e.parameters().is_empty()));
            let text = if has_dialog { format!("{}…", a.menu) } else { a.menu.to_string() };
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                chosen = Some((a.id, has_dialog));
            }
        }
        match chosen {
            Some((ActionId::Effect(i), true)) => {
                // Start from the settings this effect last ran with, if it was the last command.
                let last = self.editor.as_ref().and_then(|e| match &e.last {
                    Some(Repeat::Effect(j, p)) if *j == i => Some(p.clone()),
                    _ => None,
                });
                self.effect_dialog = EffectDialog::new(i, last);
                ui.close();
            }
            Some((id, _)) => {
                let _ = self.run_action(id, ui.ctx());
            }
            None => {}
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
                if ui.add_enabled(has_doc, egui::Button::new("Close").shortcut_text(if mac { "⌘W" } else { "Ctrl+W" })).clicked()
                    && let Err(e) = self.close_doc()
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
            ui.menu_button("Window", |ui| {
                for p in panels::REGISTRY {
                    let mut shown = self.panels.is_shown(p.id);
                    if ui.checkbox(&mut shown, p.title).changed() {
                        self.set_panel(p.id, shown);
                    }
                }
            });
            ui.menu_button("Effects", |ui| self.effects_menu(ui));
            ui.menu_button("Generate", |ui| self.generate_menu(ui));
            ui.menu_button("View", |ui| {
                if ui.checkbox(&mut self.params.log_freq, "Logarithmic frequency axis").changed() {
                    self.spectrum = None;
                }
                ui.separator();
                let (sel, mac) = (self.selection_range(), cfg!(target_os = "macos"));
                let mut chosen = None;
                for a in TABLE.iter().filter(|a| a.name.starts_with("view.") && !a.name.starts_with("view.time_display.")) {
                    let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel));
                    let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
                    let label = self.editor.as_ref().map_or_else(|| a.menu.to_string(), |e| e.label(a.id));
                    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                        chosen = Some(a.id);
                    }
                }
                if let Some(id) = chosen {
                    let _ = self.run_action(id, ui.ctx());
                }
                ui.separator();
                let mut picked = None;
                ui.menu_button("Time Display", |ui| {
                    for f in TimeFormat::ALL {
                        if ui.radio(self.time_format == f, f.menu()).clicked() {
                            picked = Some(f);
                            ui.close();
                        }
                    }
                });
                if let Some(f) = picked {
                    self.set_time_format(f);
                }
                ui.checkbox(&mut self.show_time_panel, "Time panel");
                ui.checkbox(&mut self.show_sel_view_panel, "Selection/View panel");
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
            } else if let Some(i) = &self.inserting {
                ui.spinner();
                ui.label(format!("Inserting {}… {}%", i.path.display(), i.progress.load(Ordering::Relaxed) / 10));
            } else if let Some(s) = &self.saving {
                ui.spinner();
                ui.label(format!("Saving {}… {}%", s.path.display(), s.progress.load(Ordering::Relaxed) / 10));
            } else if let Some(c) = &self.converting {
                ui.spinner();
                ui.label(format!("Converting sample type… {}%", c.progress.load(Ordering::Relaxed) / 10));
                if ui.button("Cancel").clicked() {
                    c.cancel.store(true, Ordering::Relaxed);
                }
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
                    self.fmt_time(doc.frames())
                ));
                // What was wrong with the file that still opened: the first in the bar, all of
                // them on hover.
                if let Some(first) = doc.warnings().first() {
                    ui.separator();
                    ui.colored_label(CURSOR, format!("Warning: {first}")).on_hover_text(doc.warnings().join("\n"));
                }
                ui.separator();
                if self.selection.is_empty() {
                    ui.label(format!("Cursor {}", self.fmt_time(self.selection.start())));
                } else {
                    let (a, b) = (self.selection.start(), self.selection.end());
                    ui.label(format!("Selection {} – {}  ({})", self.fmt_time(a), self.fmt_time(b), self.fmt_time(b - a)));
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

    /// The tools bar above the editor: the tools, the current one marked, and its options.
    fn tools_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tool in wavcraft_core::tools::ALL {
                let label = match tool.key() {
                    Some(k) => format!("{} ({k})", tool.label()),
                    None => tool.label().to_string(),
                };
                let button = ui.add_enabled(tool.enabled(), egui::Button::selectable(self.tool == *tool, label));
                if button.clicked() && *tool == Tool::TimeSelection {
                    self.tool = *tool;
                }
            }
            ui.separator();
            // The current tool's options.
            if self.tool == Tool::TimeSelection {
                let mut menu = self.settings.right_click_menu;
                if ui
                    .checkbox(&mut menu, "Right-click opens a menu")
                    .on_hover_text("Off: right-click extends or shortens the selection to the click")
                    .changed()
                {
                    self.settings.right_click_menu = menu;
                    match self.save_settings() {
                        Ok(()) => self.notice = None,
                        Err(e) => self.notice = Some(e),
                    }
                }
            }
        });
    }
    /// The zoom buttons and the navigator bar above the editor.
    fn zoom_panel(&mut self, ui: &mut egui::Ui) {
        let open = self.doc.is_some();
        ui.horizontal(|ui| {
            let buttons = [
                (ActionId::ZoomIn, "Zoom In", "Zoom in on time (=)"),
                (ActionId::ZoomOut, "Zoom Out", "Zoom out on time (-)"),
                (ActionId::ZoomToSelection, "Selection", "Zoom to the selection (Shift+S)"),
                (ActionId::ZoomOutFull, "Full", "Zoom out to the whole file"),
                (ActionId::ZoomAmpIn, "Amp +", "Zoom in vertically (Alt+=, Alt+wheel)"),
                (ActionId::ZoomAmpOut, "Amp -", "Zoom out vertically (Alt+-, Alt+wheel)"),
            ];
            for (id, text, tip) in buttons {
                let usable = open && (id != ActionId::ZoomToSelection || !self.selection.is_empty());
                if ui.add_enabled(usable, egui::Button::new(text)).on_hover_text(tip).clicked() {
                    let _ = self.run_action(id, ui.ctx());
                }
            }
        });
        self.navigator(ui);
    }

    /// The whole file in a thin strip with the visible range as a handle: drag it to scroll,
    /// drag its edges to zoom, click outside it to jump there.
    fn navigator(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(vec2(ui.available_width(), NAV_HEIGHT), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 2.0, Color32::from_gray(24));
        let Some(doc) = &self.doc else { return };
        let (frames, channels) = (doc.frames(), doc.channels().max(1));
        let (nav_px, view_px) = (rect.width().max(1.0), self.editor_width.max(1.0));
        // The overview: the loudest of the channels in each column, from the peaks.
        let cols = nav_px.round().max(1.0) as usize;
        let mut merged = vec![(0.0_f32, 0.0_f32); cols];
        for ch in 0..channels {
            for (m, (lo, hi)) in merged.iter_mut().zip(doc.waveform_columns(ch, 0.0, frames as f64, cols)) {
                *m = (m.0.min(lo), m.1.max(hi));
            }
        }
        let (mid, half) = (rect.center().y, rect.height() * 0.5 - 2.0);
        for (i, (lo, hi)) in merged.into_iter().enumerate() {
            let x = rect.left() + i as f32 + 0.5;
            let (top, bottom) = (mid - hi.clamp(-1.0, 1.0) * half, mid - lo.clamp(-1.0, 1.0) * half);
            painter.line_segment([pos2(x, top.min(bottom - 1.0)), pos2(x, bottom)], Stroke::new(1.0, Color32::from_rgb(40, 110, 80)));
        }

        // Presses and drags.
        let frame_at = |x: f32| f64::from((x / nav_px).clamp(0.0, 1.0)) * frames as f64;
        let handle = self.view.nav_handle(view_px, nav_px, frames);
        let local_x = |p: Pos2| p.x - rect.left();
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let x = local_x(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
            self.nav_drag = Some(match nav_hit(handle, x, 6.0) {
                Some(part) => (part, frame_at(x) - self.view.start),
                None => {
                    // A press outside the handle jumps there and carries on as a drag of it.
                    self.view.nav_jump(x, view_px, nav_px, frames);
                    (NavPart::Body, self.view.frames_per_px * f64::from(view_px) * 0.5)
                }
            });
        }
        if let (Some((part, grab)), Some(pos)) = (self.nav_drag, resp.interact_pointer_pos()) {
            self.view.nav_drag(part, local_x(pos), grab, view_px, nav_px, frames);
        }
        if resp.drag_stopped() {
            self.nav_drag = None;
        }
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
            && nav_hit(handle, local_x(pos), 6.0).is_none()
        {
            self.view.nav_jump(local_x(pos), view_px, nav_px, frames);
        }
        if let Some(pos) = resp.hover_pos().or(resp.interact_pointer_pos()) {
            let part = self.nav_drag.map(|(p, _)| p).or_else(|| nav_hit(handle, local_x(pos), 6.0));
            match part {
                Some(NavPart::Left | NavPart::Right) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                Some(NavPart::Body) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                None => {}
            }
        }
        self.view.clamp(view_px, frames);

        // The handle, drawn from the view as it is now.
        let (l, r) = self.view.nav_handle(view_px, nav_px, frames);
        let r = r.max(l + 3.0);
        let handle_rect = Rect::from_min_max(pos2(rect.left() + l, rect.top()), pos2((rect.left() + r).min(rect.right()), rect.bottom()));
        painter.rect_filled(handle_rect, 2.0, Color32::from_rgba_premultiplied(60, 80, 120, 70));
        painter.rect_stroke(handle_rect, 2.0, Stroke::new(1.5, Color32::from_rgb(120, 160, 230)), egui::StrokeKind::Inside);
        if !self.selection.is_empty() {
            let (a, b) = (self.selection.start() as f32 / frames.max(1) as f32, self.selection.end() as f32 / frames.max(1) as f32);
            let strip = Rect::from_min_max(
                pos2(rect.left() + a * nav_px, rect.bottom() - 3.0),
                pos2(rect.left() + (b * nav_px).max(a * nav_px + 1.0), rect.bottom()),
            );
            painter.rect_filled(strip, 0.0, Color32::from_rgb(130, 150, 210));
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let full = response.rect;
        painter.rect_filled(full, 0.0, BACKGROUND);
        let Some(doc) = &self.doc else { return };
        // The amplitude ruler takes a column on the left; the time axis starts right of it.
        let amp_col = Rect::from_min_max(full.min, pos2(full.left() + AMP_RULER_WIDTH, full.bottom()));
        let rect = Rect::from_min_max(pos2(amp_col.right(), full.top()), full.max);
        let ruler_resp = ui.interact(amp_col, ui.id().with("amp-ruler"), Sense::click());
        let mut chosen_scale = None;
        let (frames, rate, channels) = (doc.frames(), doc.sample_rate(), doc.channels().max(1));
        let width = rect.width().max(1.0);
        self.editor_width = width;
        if self.fit_on_next_frame {
            self.view = Viewport::fit(frames, width);
            self.fit_on_next_frame = false;
        }

        // Alt+wheel zooms the waveform vertically.
        let alt = ui.input(|i| i.modifiers.alt);
        if alt && (response.hovered() || ruler_resp.hovered()) {
            let scroll = ui.input(|i| i.smooth_scroll_delta());
            self.amp_zoom.by(((scroll.y + scroll.x) * 0.01).exp());
        }
        // Wheel zooms around the pointer; Shift+wheel and sideways scrolling move through time.
        if let Some(pointer) = response.hover_pos().filter(|_| !alt) {
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

        // Click places the cursor, drag selects; near a selection edge a drag adjusts it,
        // Shift-click extends, double-click selects what is on screen, triple-click everything.
        // Drags and clicks snap (Editor::snap) to the ruler, the selection's edges and, when on,
        // zero crossings.
        let fpp = self.view.frames_per_px;
        let snap =
            |editor: &Option<Editor>, frame: usize, targets: &[u64]| editor.as_ref().map_or(frame, |e| e.snap(frame as u64, fpp, targets) as usize);
        let frame_at = |view: &Viewport, pos: Pos2| view.frame_at(pos.x - rect.left()).clamp(0.0, frames as f64).round() as usize;
        let edge_at = |sel: &Selection, view: &Viewport, pos: Pos2| sel.edge_near(view, pos.x - rect.left(), GRAB_PX);
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some(pos) = response.hover_pos().or(response.interact_pointer_pos())
            && edge_at(&self.selection, &self.view, pos).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            // A drag is only reported once the pointer has moved a few pixels: anchor the
            // selection where the button went down, not where the pointer has got to.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            self.snap_targets = vec![self.selection.start() as u64, self.selection.end() as u64];
            if let Some(edge) = edge_at(&self.selection, &self.view, origin) {
                self.selection.grab(edge);
                self.selection.head = snap(&self.editor, frame_at(&self.view, pos), &self.snap_targets);
            } else if shift {
                self.selection.extend_to(snap(&self.editor, frame_at(&self.view, pos), &self.snap_targets));
            } else {
                let (anchor, head) = (
                    snap(&self.editor, frame_at(&self.view, origin), &self.snap_targets),
                    snap(&self.editor, frame_at(&self.view, pos), &self.snap_targets),
                );
                self.selection = Selection { anchor, head };
            }
        } else if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selection.head = snap(&self.editor, frame_at(&self.view, pos), &self.snap_targets);
        } else if response.triple_clicked() {
            self.selection = Selection::all(frames);
        } else if response.double_clicked() {
            self.selection = Selection::visible(&self.view, width, frames);
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let targets = [self.selection.start() as u64, self.selection.end() as u64];
            let at = snap(&self.editor, frame_at(&self.view, pos), &targets);
            if shift {
                self.selection.extend_to(at);
            } else {
                self.selection = Selection::at(at);
            }
        } else if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.right_click_at(frame_at(&self.view, pos), frames);
        }
        if self.settings.right_click_menu {
            let sel = self.selection_range();
            let on: Vec<(ActionId, bool)> =
                MENU_ACTIONS.iter().map(|&id| (id, self.saving.is_none() && self.editor.as_ref().is_some_and(|e| e.enabled(id, &sel)))).collect();
            let mut picked = None;
            response.context_menu(|ui| {
                for (id, enabled) in on {
                    if ui.add_enabled(enabled, egui::Button::new(actions::action(id).menu)).clicked() {
                        picked = Some(id);
                        ui.close();
                    }
                }
            });
            if let Some(id) = picked {
                let _ = self.run_action(id, ui.ctx());
            }
        }
        let Some(doc) = &self.doc else { return };

        let (start, end) = (self.view.start, self.view.end(width));
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(rect.left(), ruler.bottom()), rect.max);
        draw_ruler(&painter, ruler, &self.view, rate, self.time_format);
        if let Some(e) = &self.editor {
            draw_markers(&painter, ruler, &self.view, e.history.current().markers());
        }

        let (wave_area, spectrum_area) = match (self.panels.is_shown("waveform"), self.panels.is_shown("spectral")) {
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
            let zoom = self.amp_zoom.get();
            let scale = self.settings.amp_scale;
            for ch in 0..channels {
                let lane = lane(area, ch, channels);
                let mid = lane.center().y;
                painter.line_segment([pos2(lane.left(), mid), pos2(lane.right(), mid)], Stroke::new(1.0, Color32::from_gray(50)));
                let half = lane.height() * 0.5 - 2.0;
                for (i, (lo, hi)) in doc.waveform_columns(ch, start, end, cols).into_iter().enumerate() {
                    let x = lane.left() + i as f32 + 0.5;
                    let (top, bottom) = (mid - (hi * zoom).clamp(-1.0, 1.0) * half, mid - (lo * zoom).clamp(-1.0, 1.0) * half);
                    painter.line_segment([pos2(x, top.min(bottom - 1.0)), pos2(x, bottom)], Stroke::new(1.0, WAVE));
                }
                draw_amp_ruler(
                    &painter,
                    Rect::from_min_max(pos2(amp_col.left(), lane.top()), pos2(amp_col.right(), lane.bottom())),
                    scale,
                    self.amp_zoom,
                );
            }
        }
        painter.rect_filled(Rect::from_min_max(amp_col.min, pos2(amp_col.right(), amp_col.top() + RULER_HEIGHT)), 0.0, Color32::from_gray(32));
        let current_scale = self.settings.amp_scale;
        ruler_resp.context_menu(|ui| {
            for s in AmpScale::ALL {
                if ui.radio(current_scale == s, s.menu()).clicked() {
                    chosen_scale = Some(s);
                    ui.close();
                }
            }
        });

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

        // Channel enable (A132): a button at each lane's left edge; a disabled lane is dimmed and
        // takes no part in the selection.
        let enabled = self.editor.as_ref().map_or(ChannelMask::ALL, |e| e.channels.within(channels));
        let lanes_area = wave_area.or(spectrum_area).unwrap_or(body);
        let mut toggle = None;
        for ch in 0..channels {
            let (lane, on) = (lane(lanes_area, ch, channels), enabled.contains(ch));
            if !on {
                painter.rect_filled(lane, 0.0, Color32::from_black_alpha(120));
            }
            let button = Rect::from_min_size(lane.left_top() + vec2(3.0, 3.0), vec2(20.0, 16.0));
            // Click and drag both, so a press here does not start a selection underneath.
            if ui.interact(button, ui.id().with(("channel", ch)), Sense::click_and_drag()).clicked() {
                toggle = Some(ch);
            }
            painter.rect_filled(button, 3.0, if on { Color32::from_rgb(50, 110, 90) } else { Color32::from_gray(45) });
            let name = if channels == 2 { ["L", "R"][ch].to_string() } else { (ch + 1).to_string() };
            painter.text(
                button.center(),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::monospace(11.0),
                if on { Color32::WHITE } else { Color32::from_gray(120) },
            );
        }

        // Selection, cursor and playhead over everything.
        if !self.selection.is_empty() {
            let (a, b) = (rect.left() + self.view.x_of(self.selection.start() as f64), rect.left() + self.view.x_of(self.selection.end() as f64));
            for area in [wave_area, spectrum_area].into_iter().flatten() {
                for ch in (0..channels).filter(|&c| enabled.contains(c)) {
                    let lane = lane(area, ch, channels);
                    painter.rect_filled(
                        Rect::from_min_max(pos2(a.max(rect.left()), lane.top()), pos2(b.min(rect.right()), lane.bottom())),
                        0.0,
                        SELECTION,
                    );
                }
            }
        }
        let line = |frame: usize, color: Color32| {
            let x = rect.left() + self.view.x_of(frame as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, color));
            }
        };
        line(self.selection.start(), CURSOR);
        if self.player.is_playing() && !self.previewing {
            line(self.player.position(), PLAYHEAD);
            ui.ctx().request_repaint();
        }
        if let Some(ch) = toggle {
            let _ = self.run_action(ActionId::ToggleChannel(ch as u8), ui.ctx());
        }
        if let Some(scale) = chosen_scale {
            self.set_amp_scale(scale);
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
        self.finish_inserting(ctx);
        self.finish_rebuild();
        self.finish_saving(ctx);
        self.recovery.poll();
        let unsaved = self.unsaved();
        self.recovery.track(self.editor.as_ref(), unsaved, ctx);
        self.finish_converting(ctx);
        while let Some(req) = self.control.as_ref().and_then(|rx| rx.try_recv().ok()) {
            let answer = self.control_request(&req.line, ctx);
            let _ = req.reply.send(answer);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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
        // Up and Down belong to the History panel while one of its rows has focus (before the
        // action table gets them: there they pick the left or right channel).
        if self.history_focus && self.saving.is_none() && self.converting.is_none() {
            let (up, down) =
                ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)));
            if let Some(to) = (up || down).then(|| self.history_arrow(down, &ctx)).flatten() {
                self.history_refocus = Some(to);
            }
        }
        self.selection_keys(&ctx);
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
        let (space, open, dropped, hovering, dropped_without_path) = ctx.input(|i| {
            (
                // Not `key_pressed`: that also counts auto-repeat, so a long press would toggle again.
                i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Space, pressed: true, repeat: false, .. })),
                i.modifiers.command && i.key_pressed(Key::O),
                dropped_path(&i.raw.dropped_files),
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty() && dropped_path(&i.raw.dropped_files).is_none(),
            )
        });
        if space {
            self.toggle_play();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::W))
            && let Err(e) = self.close_doc()
        {
            self.error = Some(e);
        }
        if open {
            self.pick_file(&ctx);
        }
        if let Some(path) = dropped {
            self.open(path, &ctx);
        } else if dropped_without_path {
            self.error = Some("The dropped item has no file path here, so it was not opened (use File › Open)".to_string());
        }
        if hovering {
            // The OS is carrying a file over the window: say that dropping opens it.
            egui::Area::new(egui::Id::new("drop-hint")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
                &ctx,
                |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.heading("Drop a file to open it"));
                },
            );
        }
        let star = if self.unsaved() { "*" } else { "" };
        let title = self.doc.as_ref().map_or_else(|| "WavCraft".to_string(), |d| format!("{star}{} — WavCraft", d.name()));
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("tools").show(ui, |ui| self.tools_bar(ui));
        egui::Panel::top("zoom").show(ui, |ui| self.zoom_panel(ui));
        if self.docs.len(self.editor.is_some()) > 1 {
            egui::Panel::top("docs").show(ui, |ui| self.doc_tabs(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.show_sel_view_panel {
            egui::Panel::bottom("selection_view").show(ui, |ui| self.selection_view_panel(ui));
        }
        if self.show_time_panel {
            egui::Panel::bottom("time_panel").show(ui, |ui| self.time_panel(ui));
        }
        if self.panels.is_shown("history") {
            egui::Panel::right("history").default_size(220.0).show(ui, |ui| self.history_panel(ui));
        }
        self.insert_silence_dialog(&ctx);
        self.convert_dialogs(&ctx);
        self.effect_dialog_ui(&ctx);
        self.mix_paste_dialog(&ctx);
        self.preferences_dialog(&ctx);
        self.clear_history_prompt(&ctx);
        match self.recovery.show(&ctx) {
            Some(Choice::Recover(i)) => {
                if let Err(e) = self.recover_session(i, &ctx) {
                    self.error = Some(e);
                }
            }
            Some(Choice::Discard(i)) => {
                if let Err(e) = self.discard_recovery(i) {
                    self.error = Some(e);
                }
            }
            None => {}
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
    }
}

/// The History step an Up or Down key moves to from `cursor` among `len` steps (none at an end).
fn history_arrow_target(cursor: usize, len: usize, down: bool) -> Option<usize> {
    if down { (cursor + 1 < len).then_some(cursor + 1) } else { cursor.checked_sub(1) }
}

/// The file to open from the files dropped on the window this frame: the first with a path. The
/// path is empty or missing where the platform hands over no file (a browser, some Wayland
/// compositors, #66); then nothing is opened.
fn dropped_path(files: &[Arc<dyn egui::DroppedFile + Send + Sync>]) -> Option<PathBuf> {
    files.iter().map(|f| f.path()).find(|p| !p.as_os_str().is_empty()).map(std::path::Path::to_path_buf)
}
/// What the waveform's right-click menu offers (only with the "context menu" setting).
const MENU_ACTIONS: [ActionId; 5] = [ActionId::Cut, ActionId::Copy, ActionId::Paste, ActionId::Delete, ActionId::SelectAll];

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
        // Alt+1 .. Alt+8 enable or disable channel 1 .. 8 (A132).
        let mut pressed: Vec<ActionId> = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8]
            .into_iter()
            .enumerate()
            .filter_map(|(n, key)| i.consume_key(egui::Modifiers::ALT, key).then_some(ActionId::ToggleChannel(n as u8)))
            .collect();
        pressed.extend(keys.iter().filter_map(|(id, k)| {
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
        }));
        pressed
    })
}

/// The vertical slice of `area` for channel `ch` of `channels`.
fn lane(area: Rect, ch: usize, channels: usize) -> Rect {
    let h = area.height() / channels.max(1) as f32;
    Rect::from_min_max(pos2(area.left(), area.top() + h * ch as f32), pos2(area.right(), area.top() + h * (ch + 1) as f32))
}

/// The amplitude ruler for one lane: marks and numbers on both sides of the centre line, at the
/// current vertical zoom.
fn draw_amp_ruler(painter: &egui::Painter, lane: Rect, scale: AmpScale, zoom: AmpZoom) {
    painter.rect_filled(lane, 0.0, Color32::from_gray(28));
    let (mid, half) = (lane.center().y, lane.height() * 0.5 - 2.0);
    let font = egui::FontId::monospace(10.0);
    let ink = Color32::from_gray(170);
    for tick in scale.ticks(zoom, half, 16.0) {
        let offset = tick.amp * zoom.get() * half;
        for sign in [1.0_f32, -1.0] {
            if tick.amp == 0.0 && sign < 0.0 {
                continue;
            }
            let y = mid - sign * offset;
            painter.line_segment([pos2(lane.right() - 5.0, y), pos2(lane.right(), y)], Stroke::new(1.0, ink));
            let anchor = if y - lane.top() < 6.0 {
                egui::Align2::RIGHT_TOP
            } else if lane.bottom() - y < 6.0 {
                egui::Align2::RIGHT_BOTTOM
            } else {
                egui::Align2::RIGHT_CENTER
            };
            painter.text(pos2(lane.right() - 7.0, y), anchor, scale.label(sign * tick.amp), font.clone(), ink);
        }
    }
    painter.line_segment([pos2(lane.right() - 0.5, lane.top()), pos2(lane.right() - 0.5, lane.bottom())], Stroke::new(1.0, Color32::from_gray(60)));
}

/// Tick marks and times along the top, at a round step that leaves room for the labels.
fn draw_ruler(painter: &egui::Painter, rect: Rect, view: &Viewport, rate: u32, fmt: TimeFormat) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(32));
    let seconds_per_px = view.frames_per_px / f64::from(rate.max(1));
    let step = ruler_step_for(fmt, seconds_per_px * 110.0, rate);
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
        painter.line_segment([pos2(x, rect.bottom() - 6.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(140)));
        painter.text(
            pos2(x + 3.0, rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            fmt.format_tick(t, rate),
            egui::FontId::monospace(11.0),
            Color32::from_gray(190),
        );
    }
}

/// Markers on the ruler: a point is a small flag with its name, a range a band with a flag at
/// each end of the name.
fn draw_markers(painter: &egui::Painter, rect: Rect, view: &Viewport, markers: &wavcraft_core::Markers) {
    let color = Color32::from_rgb(240, 190, 60);
    let painter = painter.with_clip_rect(rect);
    for m in markers.iter() {
        let x0 = rect.left() + view.x_of(m.start as f64);
        let x1 = rect.left() + view.x_of(m.end.max(m.start) as f64);
        if x1 < rect.left() - 200.0 || x0 > rect.right() {
            continue;
        }
        if !m.is_point() {
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, rect.bottom() - 5.0), pos2(x1.max(x0 + 1.0), rect.bottom())),
                0.0,
                color.gamma_multiply(0.6),
            );
        }
        painter.line_segment([pos2(x0, rect.top()), pos2(x0, rect.bottom())], Stroke::new(1.0, color));
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(x0, rect.top()), pos2(x0 + 8.0, rect.top()), pos2(x0, rect.top() + 8.0)],
            color,
            Stroke::NONE,
        ));
        painter.text(pos2(x0 + 10.0, rect.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, &m.name, egui::FontId::proportional(10.0), color);
    }
}

/// The ruler's tick spacing in seconds for `fmt`: round numbers of the format's own unit (samples
/// in powers 1-2-5, SMPTE frames below a second), else of seconds.
fn ruler_step_for(fmt: TimeFormat, min_seconds: f64, rate: u32) -> f64 {
    match fmt {
        TimeFormat::Samples => {
            let min_frames = (min_seconds * f64::from(rate.max(1))).max(1.0);
            let mut decade = 1.0f64;
            loop {
                if let Some(m) = [1.0, 2.0, 5.0].iter().map(|m| m * decade).find(|f| *f >= min_frames) {
                    return m / f64::from(rate.max(1));
                }
                decade *= 10.0;
            }
        }
        TimeFormat::Smpte30 => {
            const FRAMES: [f64; 5] = [1.0, 2.0, 5.0, 10.0, 15.0];
            FRAMES.iter().map(|f| f / 30.0).find(|s| *s >= min_seconds).unwrap_or_else(|| ruler_step(min_seconds.max(1.0)))
        }
        TimeFormat::Decimal | TimeFormat::Compact => ruler_step(min_seconds),
    }
}

/// 256 colours from black through blue, magenta and orange to pale yellow.
fn palette() -> Vec<Color32> {
    const STOPS: [(f32, [f32; 3]); 6] = [
        (0.0, [0.0, 0.0, 0.0]),
        (0.3, [20.0, 12.0, 80.0]),
        (0.55, [140.0, 30.0, 120.0]),
        (0.75, [235.0, 90.0, 40.0]),
        (0.9, [250.0, 200.0, 60.0]),
        (1.0, [255.0, 250.0, 215.0]),
    ];
    (0..256)
        .map(|i| {
            let t = i as f32 / 255.0;
            let hi = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
            let (a, b) = (STOPS.get(hi - 1).copied().unwrap_or(STOPS[0]), STOPS.get(hi).copied().unwrap_or(STOPS[5]));
            let f = ((t - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
            let mix = |k: usize| (a.1[k] + (b.1[k] - a.1[k]) * f).round() as u8;
            Color32::from_rgb(mix(0), mix(1), mix(2))
        })
        .collect()
}

/// How much of a generator a dialog's Preview plays, in seconds, looped.
const PREVIEW_SECONDS: f64 = 2.0;
/// Previews of noise are the same every time, so a changed setting is what you hear change.
const PREVIEW_SEED: u64 = 0x5EED;

/// A seed for noise that differs between runs.
fn clock_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

/// `seed=N` pulled out of a `GENERATE` argument string: the seed (the clock's when absent) and
/// the rest.
fn split_seed(args: &str) -> (Result<u64, String>, String) {
    let mut seed = Ok(clock_seed());
    let rest: Vec<&str> = args
        .split_whitespace()
        .filter(|w| match w.strip_prefix("seed=") {
            Some(n) => {
                seed = n.parse::<u64>().map_err(|_| format!("seed={n}: a whole number"));
                false
            }
            None => true,
        })
        .collect();
    (seed, rest.join(" "))
}

#[cfg(test)]
mod tests {
    #[test]
    fn history_arrows_stop_at_the_ends() {
        use super::history_arrow_target as t;
        assert_eq!(t(0, 3, false), None);
        assert_eq!(t(0, 3, true), Some(1));
        assert_eq!(t(2, 3, true), None);
        assert_eq!(t(2, 3, false), Some(1));
        assert_eq!(t(0, 1, true), None);
    }

    use super::*;

    #[test]
    fn a_drop_opens_the_first_file_that_has_a_path() {
        #[derive(Debug)]
        struct Dropped(PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                Err("not a file".to_string())
            }
        }
        let file = |path: &str| -> Arc<dyn egui::DroppedFile + Send + Sync> { Arc::new(Dropped(PathBuf::from(path))) };
        assert_eq!(dropped_path(&[]), None);
        assert_eq!(dropped_path(&[file("")]), None);
        assert_eq!(dropped_path(&[file(""), file("/a/b.wav"), file("/c.wav")]), Some(PathBuf::from("/a/b.wav")));
    }

    #[test]
    fn ruler_steps_are_round_and_wide_enough() {
        assert_eq!(ruler_step(0.0004), 0.001);
        assert_eq!(ruler_step(0.7), 1.0);
        assert_eq!(ruler_step(45.0), 60.0);
        assert_eq!(ruler_step(5000.0), 3600.0);
    }

    #[test]
    fn ruler_steps_follow_the_time_format() {
        assert_eq!(ruler_step_for(TimeFormat::Decimal, 0.7, 48_000), 1.0);
        assert!((ruler_step_for(TimeFormat::Samples, 0.0011, 48_000) - 100.0 / 48_000.0).abs() < 1e-12, "100 samples");
        assert!((ruler_step_for(TimeFormat::Smpte30, 0.02, 48_000) - 1.0 / 30.0).abs() < 1e-12, "one frame");
        assert!((ruler_step_for(TimeFormat::Smpte30, 0.2, 48_000) - 10.0 / 30.0).abs() < 1e-12);
        assert_eq!(ruler_step_for(TimeFormat::Smpte30, 3.0, 48_000), 5.0);
    }

    #[test]
    fn tick_labels_are_exact_at_any_sample_rate() {
        let tick_label = |s: f64| TimeFormat::Decimal.format_tick(s, 44_100);
        let labels: Vec<String> = (0..8).map(|k| tick_label(f64::from(k) * 0.001)).collect();
        assert_eq!(labels, ["0:00.000", "0:00.001", "0:00.002", "0:00.003", "0:00.004", "0:00.005", "0:00.006", "0:00.007"]);
        assert_eq!(tick_label(3723.5), "1:02:03.500");
        // The old route, through a frame at 44.1 kHz, dropped a millisecond.
        assert_eq!(wavcraft_core::format_time((0.003f64 * 44_100.0).round() as usize, 44_100), "0:00.002");
    }

    #[test]
    fn the_palette_runs_dark_to_light() {
        let p = palette();
        assert_eq!(p.len(), 256);
        assert_eq!(p[0], Color32::from_rgb(0, 0, 0));
        assert_eq!(p[255], Color32::from_rgb(255, 250, 215));
        let brightness = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(p.windows(2).all(|w| brightness(w[1]) + 3 >= brightness(w[0])), "brightness never drops");
    }

    #[test]
    fn lanes_stack_without_gaps() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 300.0));
        assert_eq!(lane(area, 0, 2), Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 200.0)));
        assert_eq!(lane(area, 1, 2), Rect::from_min_max(pos2(0.0, 200.0), pos2(800.0, 300.0)));
        assert_eq!(lane(area, 0, 0), area);
    }

    #[test]
    fn split_seed_pulls_the_seed_out() {
        let (seed, rest) = split_seed("freq=440 seed=7 dur=2");
        assert_eq!((seed, rest.as_str()), (Ok(7), "freq=440 dur=2"));
        assert!(split_seed("seed=x").0.is_err());
        assert!(split_seed("dur=2").0.is_ok());
    }
}
