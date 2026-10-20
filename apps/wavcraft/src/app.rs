//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::convert::{self, ConvertSettings, PresetFile};
use wavcraft_core::doc::edit as doc_edit;
use wavcraft_core::doc::newdoc::{self, NewDocOptions};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::match_loudness::Target;
use wavcraft_core::meta;
use wavcraft_core::panels::{self, DockSide, Layout, PanelSet};
use wavcraft_core::save::{FileKind, Part, SampleType, SaveFormat, save, write_file};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};
use wavcraft_core::snap::ruler_step;
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::transport::{self, Follow, Mode, Transport};
use wavcraft_core::view::nav_hit;
use wavcraft_core::workspaces::{self, Snapshot, Workspaces};
use wavcraft_core::{
    AmpScale, AmpZoom, ChannelMask, Clip, DocState, Document, Edit, FileFormat, GRAB_PX, History, NavPart, OpenError, SampleFormat, Selection,
    SpectrogramParams, TimeFormat, Tool, Viewport, format_time,
};

use crate::control;
use crate::convert_ui::{self, ConvertDialog};
use crate::doclist::{DocList, Where};
use crate::effect_dialog::{Answer, EffectDialog};
use crate::files;
use crate::markers_panel::MarkersUi;
use crate::match_ui::MatchUi;
use crate::meters_ui::{self, MetersUi};
use crate::mixpaste;
use crate::player::{Devices, Player};
use crate::rack_panel::{RackPanel, Request};
use crate::recovery::{self, Choice, Recovery};
use crate::shortcut_editor::{Change, ShortcutDialog};
use crate::spectrum::{Job, Renderer};
use crate::statspanel::{Nav, StatsPanel};
use wavcraft_core::doc::rec_edit::{self, RecMode, RecPlan, RecTarget};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const WAVE: Color32 = Color32::from_rgb(64, 200, 140);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const ACCENT: Color32 = Color32::from_rgb(90, 140, 230);
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

/// A new document being made from a clip on a worker thread (Copy to New, Paste to New).
struct Making {
    what: &'static str,
    result: Receiver<Result<DocState, String>>,
    progress: Arc<AtomicU32>,
}

/// File > New > Audio File…: the values in the dialog while it is up.
struct NewAudio {
    rate: u32,
    channels: usize,
    bits: u8,
}

impl Default for NewAudio {
    fn default() -> Self {
        NewAudio { rate: 48_000, channels: 2, bits: 32 }
    }
}

/// The sample type a bit depth in the dialog (or `bits=` of `NEWDOC`) means.
fn sample_format_for_bits(bits: u8) -> Option<SampleFormat> {
    match bits {
        16 => Some(SampleFormat::Int16),
        24 => Some(SampleFormat::Int24),
        32 => Some(SampleFormat::Float32),
        _ => None,
    }
}

/// File > Save All, working through the unsaved documents one save at a time.
struct SaveAllRun {
    /// `doc_id`s still to save.
    queue: Vec<u64>,
    /// May open a Save As dialog for a document with no file (the menu); a script's run skips it.
    interactive: bool,
    /// The document that was current when it started: made current again at the end.
    back_to: u64,
    /// Documents it could not save without a dialog.
    skipped: usize,
}

/// A finished take being brought into the document on a worker thread (C026, C027, C030): the
/// take is opened from its own file by the normal streamed open, then one edit is built from it.
struct Integrating {
    result: Receiver<Result<wavcraft_core::Edit, String>>,
    base: Arc<DocState>,
    doc_id: u64,
    path: PathBuf,
    plan: RecPlan,
}

/// A save running on a worker thread.
struct Saving {
    path: PathBuf,
    /// The step being saved: it becomes the saved state when the save succeeds.
    state: Arc<DocState>,
    ty: SampleType,
    format: SaveFormat,
    result: Receiver<Result<(), String>>,
    progress: Arc<AtomicU32>,
    /// Close the window once it is saved (Save from the close prompt).
    then_close: bool,
    /// Which open document it saves ([`WaveApp::doc_id`] when it started): a save that finishes
    /// after another file was opened must not give that file its path (Codex review CX-W1).
    doc_id: u64,
    /// A copy (Save Selection As, Export): the document keeps its own path and unsaved mark.
    copy: Option<&'static str>,
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

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Reveal in Explorer"
    } else {
        "Show in Folder"
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
    /// Play mode and options (loop, skip selection, return on stop, auto-scroll).
    transport: Transport,
    params: SpectrogramParams,
    /// Which registered panels are shown (Window menu; kept in the settings file).
    panels: PanelSet,
    /// Where the panels are docked (kept in the settings file).
    layout: Layout,
    /// The panel under the pointer last frame, for the maximise and close keys.
    hover: Option<&'static str>,
    hover_next: Option<&'static str>,
    /// Where the docked tabs and zones were drawn this frame, for dropping a dragged tab.
    tab_rects: Vec<(String, Rect)>,
    zone_rects: Vec<(DockSide, Rect)>,
    /// The layout changed while the pointer was down (a zone being resized): save it on release.
    layout_dirty: bool,
    /// How the last floating window was made: "os" (own operating-system window) or "embedded"
    /// (an egui window inside the main one); reported by `PANELS`.
    float_kind: Option<&'static str>,
    /// Saved workspaces (layout snapshots) and the one in use.
    workspaces: Workspaces,
    /// The name being typed in Window › Workspace › Save as New Workspace.
    workspace_name: String,
    spectrum: Option<SpectrumCache>,
    renderer: Renderer,
    opening: Option<Opening>,
    inserting: Option<Inserting>,
    /// Test hook (`HOLD_BUSY`, fake input only): counts as an open or save running until then.
    test_busy_until: Option<std::time::Instant>,
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
    /// File › Sample Type on Save: the type every save writes, or `None` to keep the file's own.
    save_type: Option<SampleType>,
    /// The "save changes before closing?" prompt is up.
    confirm_close: bool,
    /// Closing was confirmed (or nothing was unsaved): let the next close request through.
    allow_close: bool,
    /// The editor's width last frame, for zoom keys (which have no pointer to zoom around).
    editor_width: f32,
    /// A new document being made from a clip.
    making: Option<Making>,
    /// File > New > Audio File…, while its dialog is up.
    new_audio: Option<NewAudio>,
    /// File > Close All is going through the documents, asking about unsaved ones.
    close_all: bool,
    save_all: Option<SaveAllRun>,
    /// Window › Files.
    show_files: bool,
    files: files::View,
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
    /// View › Snap to Frames: with a SMPTE time display, the cursor and selection edges set from
    /// the ruler land on frame boundaries. (PR #95 adds the general snapping switch; once both
    /// are merged this is "snapping is on" there, and `TimeFormat::snap_to_frame` is the call.)
    snap_frames: bool,
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
    /// Window › Levels and the statistics window (f26).
    meters: MetersUi,
    /// Recording a take (File › New Recording…, the Record button, `RECORD` on the control channel).
    rec: Recording,
    /// The Effects Rack panel and its settings (f31).
    rack: RackPanel,
    /// The keyboard shortcuts in use (the defaults; the user's key file will override them).
    keymap: Keymap,
    /// Window › Amplitude Statistics.
    stats: StatsPanel,
    /// Window › Match Loudness (f33).
    matcher: MatchUi,
    /// Window › Metadata.
    show_metadata: bool,
    /// The Metadata panel's text boxes (one per `meta::FIELDS`) and the source they were filled
    /// from: refilled whenever the current step's metadata is another one.
    meta_draft: Vec<String>,
    meta_for: Option<Arc<wavcraft_core::SourceFormat>>,
    /// Window › Markers: the panel, the picked markers, a handle being dragged.
    markers_ui: MarkersUi,
    /// A take being brought into the document.
    integrating: Option<Integrating>,
    /// Where the key file is (`None`: the system has no config folder).
    shortcuts_path: Option<PathBuf>,
    /// Edit › Keyboard Shortcuts…, while it is open.
    shortcut_dialog: Option<ShortcutDialog>,
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
        let shortcuts_path = wavcraft_core::shortcuts::default_path();
        let (keymap, shortcuts_warning) = shortcuts_path.as_deref().map(wavcraft_core::shortcuts::load).unwrap_or_default();
        if let Some(w) = shortcuts_warning {
            log::warn!(target: "wavcraft", "{w}");
        }
        if let Err(e) = keymap.check() {
            log::error!(target: "wavcraft", "keyboard shortcuts conflict: {e}");
        }
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
            transport: Transport::default(),
            params: SpectrogramParams::default(),
            panels: PanelSet::from_hidden(&settings.hidden_panels),
            layout: Layout::decode(&settings.panel_layout, &settings.zone_sizes),
            hover: None,
            hover_next: None,
            tab_rects: Vec::new(),
            zone_rects: Vec::new(),
            layout_dirty: false,
            float_kind: None,
            workspaces: Workspaces::new(settings_path.as_deref().and_then(|p| p.parent()).map(|d| d.join("workspaces")), &settings.workspace),
            workspace_name: String::new(),
            spectrum: None,
            renderer: Renderer::new(&cc.egui_ctx, palette()),
            opening: None,
            inserting: None,
            test_busy_until: None,
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
            save_type: None,
            confirm_close: false,
            allow_close: false,
            os_clipboard: crate::oscb::OsClipboard::new(),
            editor_width: 800.0,
            snap_frames: true,
            show_time_panel: true,
            show_sel_view_panel: true,
            panel_edit: None,
            making: None,
            new_audio: None,
            close_all: false,
            save_all: None,
            show_files: true,
            files: files::View::default(),
            insert_silence: None,
            mix_paste: None,
            stats: StatsPanel::default(),
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
            meters: MetersUi::new(),
            rec: Recording::default(),
            rack: RackPanel::new(),
            keymap,
            matcher: MatchUi::new(),
            show_metadata: false,
            meta_draft: Vec::new(),
            meta_for: None,
            markers_ui: MarkersUi::new(),
            integrating: None,
            shortcuts_path,
            shortcut_dialog: None,
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
        if self.rec.is_active() {
            self.error = Some("Stop recording before opening a file".to_string());
            return;
        }
        if self.integrating.is_some() {
            self.error = Some("Wait for the recording to be brought into the file".to_string());
            return;
        }
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
                let clip = self.editor.as_mut().map(|e| (std::mem::take(&mut e.clipboards), e.current_clipboard));
                let old = self.editor.is_some().then(|| self.take_slot());
                self.docs.add(old);
                self.player.set_audio(Some(doc.audio.clone()));
                self.meters.new_file();
                self.set_audio(Some(doc.audio.clone()));
                self.matcher.new_file();
                let cap = History::default_cap(&self.sessions.path);
                // Snapping stays as the owner left it from file to file.
                let snap = self.editor.as_ref().map(|e| e.snap);
                self.editor = doc.state.clone().map(|state| Editor::new(History::new(state, cap)));
                if let (Some(e), Some(snap)) = (self.editor.as_mut(), snap) {
                    e.snap = snap;
                }
                self.apply_crossfades();
                if let (Some((clips, current)), Some(e)) = (clip, self.editor.as_mut()) {
                    e.clipboards = clips;
                    e.current_clipboard = current;
                }
                self.saved = self.editor.as_ref().map(|e| e.history.current().clone());
                self.doc_path = doc.path.clone();
                self.doc_serial += 1;
                self.doc_id = self.doc_serial;
                self.stats.forget();
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
        // The clipboard outlives the document it was copied from.
        let clip = self.editor.as_mut().map(|e| (std::mem::take(&mut e.clipboards), e.current_clipboard));
        drop(self.take_slot());
        match self.docs.close_current() {
            Some(next) => {
                self.put_slot(next);
                if let (Some((clips, current)), Some(e)) = (clip, self.editor.as_mut()) {
                    e.clipboards = clips;
                    e.current_clipboard = current;
                }
            }
            None => {
                self.generation += 1;
                self.spectrum = None;
            }
        }
    }

    /// The session folder and store setup for a new document.
    fn new_doc_options(&self) -> NewDocOptions {
        NewDocOptions { sessions_root: Some(self.sessions.path.clone()), ..NewDocOptions::default() }
    }

    /// Add `state` as a new untitled document and make it current. It has no file, so it counts
    /// as unsaved until Save As, and Close or Quit ask about it.
    fn add_document(&mut self, state: DocState, ctx: &egui::Context) {
        let clip = self.editor.as_mut().map(|e| (std::mem::take(&mut e.clipboards), e.current_clipboard));
        let old = self.editor.is_some().then(|| self.take_slot());
        self.docs.add(old);
        self.player.stop();
        self.player.set_audio(None);
        let frames = state.len_frames();
        let mut editor = Editor::new(History::new(state, History::default_cap(&self.sessions.path)));
        if let Some((clips, current)) = clip {
            editor.clipboards = clips;
            editor.current_clipboard = current;
        }
        self.editor = Some(editor);
        (self.doc, self.saved, self.doc_path, self.rebuilding) = (None, None, PathBuf::new(), None);
        self.doc_serial += 1;
        self.doc_id = self.doc_serial;
        self.generation += 1;
        self.selection = Selection::default();
        self.spectrum = None;
        self.fit_on_next_frame = true;
        if frames > 0 {
            self.rebuild(ctx);
        }
    }

    /// File › New › Audio File: an empty untitled document.
    fn new_audio_file(&mut self, rate: u32, channels: usize, bits: u8, ctx: &egui::Context) -> Result<(), String> {
        let fmt = sample_format_for_bits(bits).ok_or_else(|| "the bit depth must be 16, 24 or 32 (float)".to_string())?;
        let state = newdoc::empty(&self.new_doc_options(), rate, channels, fmt).map_err(|e| e.to_string())?;
        self.add_document(state, ctx);
        self.error = None;
        self.notice =
            Some(format!("New empty file: {rate} Hz, {channels} ch, {} bit", if bits == 32 { "32-bit float".to_string() } else { bits.to_string() }));
        Ok(())
    }

    /// Stream `clip` into a new document on a worker thread.
    fn start_making(&mut self, what: &'static str, clip: Clip, ctx: &egui::Context) -> Result<(), String> {
        if self.making.is_some() {
            return Err("a new document is already being made".to_string());
        }
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (opts, job_progress, ctx) = (self.new_doc_options(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
            let _ = tx.send(newdoc::from_clip(&opts, &clip, &mut report, &AtomicBool::new(false)).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.making = Some(Making { what, result: rx, progress });
        self.error = None;
        Ok(())
    }

    fn finish_making(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.making else { return };
        let result = match m.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the worker stopped unexpectedly".to_string()),
        };
        let what = m.what;
        self.making = None;
        match result {
            Ok(state) => self.add_document(state, ctx),
            Err(e) => self.error = Some(format!("{what}: {e}")),
        }
    }

    /// Edit › Copy to New: the selection (the whole file with none) becomes a new document.
    fn copy_to_new(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let doc = editor.history.current();
        let len = doc.len_frames();
        let range = if sel.start < sel.end { sel.start.min(len)..sel.end.min(len) } else { 0..len };
        if range.is_empty() {
            return Err("Copy to New: there is no audio to copy".to_string());
        }
        let clip = doc_edit::copy(doc, range, editor.channels.within(doc.channel_count())).map_err(|e| e.to_string())?;
        self.start_making("Copy to New", clip, ctx)
    }

    /// Edit › Paste to New: the clipboard becomes a new document.
    fn paste_to_new(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(clip) = self.editor.as_ref().and_then(|e| e.clipboard().cloned()) else {
            return Err("Paste to New: the clipboard is empty".to_string());
        };
        self.start_making("Paste to New", clip, ctx)
    }

    /// The `doc_id` and unsaved mark of open document `i`.
    fn doc_id_at(&self, i: usize) -> Option<(u64, bool)> {
        match self.docs.locate(i) {
            Where::Current => self.editor.as_ref().map(|_| (self.doc_id, self.unsaved())),
            Where::Parked(p) => self.docs.parked().get(p).map(|s| (s.doc_id, is_unsaved(&s.editor, &s.saved))),
        }
    }

    fn unsaved_count(&self) -> usize {
        (0..self.docs.len(self.editor.is_some())).filter(|&i| self.doc_id_at(i).is_some_and(|(_, u)| u)).count()
    }

    /// File › Close All. In the window it goes through the documents in order, asking about each
    /// unsaved one (Cancel stops). From a script (`interactive` false) it refuses while any
    /// document is unsaved, as `CLOSE` does.
    fn close_all_docs(&mut self, interactive: bool) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() || self.making.is_some() || self.save_all.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if interactive {
            self.close_all = true;
            return Ok(());
        }
        let n = self.unsaved_count();
        if n > 0 {
            return Err(format!("{n} document(s) have unsaved changes: SAVE them first, or CLOSE discard each"));
        }
        while self.editor.is_some() {
            self.remove_current();
        }
        Ok(())
    }

    /// Carries Close All on: closes documents from the first on, and stops at an unsaved one to
    /// ask. Called every frame; it waits for a prompt or a save in progress.
    fn pump_close_all(&mut self) {
        if !self.close_all || self.confirm_close || self.saving.is_some() {
            return;
        }
        loop {
            if self.editor.is_none() {
                self.close_all = false;
                return;
            }
            if self.docs.current() != 0 && self.switch_doc(0).is_err() {
                self.close_all = false;
                return;
            }
            if self.unsaved() {
                self.closing_doc = true;
                self.confirm_close = true;
                return;
            }
            self.remove_current();
        }
    }

    /// File › Save All: every unsaved document, one after another. A document with a WAV file
    /// saves to it; one without asks for a file name (the window) or is skipped (a script).
    fn save_all_docs(&mut self, interactive: bool) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() || self.making.is_some() || self.save_all.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let queue: Vec<u64> = (0..self.docs.len(true)).filter_map(|i| self.doc_id_at(i)).filter(|(_, u)| *u).map(|(id, _)| id).collect();
        if queue.is_empty() {
            self.notice = Some("Save All: nothing to save".to_string());
            return Ok(());
        }
        self.save_all = Some(SaveAllRun { queue, interactive, back_to: self.doc_id, skipped: 0 });
        Ok(())
    }

    fn pump_save_all(&mut self, ctx: &egui::Context) {
        if self.saving.is_some() || self.confirm_close {
            return;
        }
        let Some(mut run) = self.save_all.take() else { return };
        while !run.queue.is_empty() {
            let id = run.queue.remove(0);
            let n = self.docs.len(self.editor.is_some());
            let Some(i) = (0..n).find(|&i| self.doc_id_at(i).is_some_and(|(d, u)| d == id && u)) else { continue };
            if self.switch_doc(i).is_err() {
                break;
            }
            let own =
                !self.doc_path.as_os_str().is_empty() && self.editor.as_ref().is_some_and(|e| e.history.current().source().format == FileFormat::Wav);
            if !own && !run.interactive {
                run.skipped += 1;
                continue;
            }
            match self.save(ctx, false, None, false) {
                Ok(()) if self.saving.is_some() => {
                    self.save_all = Some(run);
                    return;
                }
                // The file dialog was cancelled: stop here.
                Ok(()) => {
                    run.queue.clear();
                    break;
                }
                Err(e) => {
                    self.error = Some(format!("Save All: {e}"));
                    run.queue.clear();
                    break;
                }
            }
        }
        let n = self.docs.len(self.editor.is_some());
        if let Some(i) = (0..n).find(|&i| self.doc_id_at(i).is_some_and(|(d, _)| d == run.back_to)) {
            let _ = self.switch_doc(i);
        }
        if run.skipped > 0 {
            self.error = Some(format!("Save All: {} document(s) have no file yet: use Save As", run.skipped));
        }
    }

    /// The File › New › Audio File dialog: sample rate, channels, bit depth.
    fn new_audio_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.new_audio.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("New Audio File").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("new_audio").num_columns(2).show(ui, |ui| {
                ui.label("Sample rate:");
                egui::ComboBox::from_id_salt("rate").selected_text(format!("{} Hz", d.rate)).show_ui(ui, |ui| {
                    for r in [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
                        ui.selectable_value(&mut d.rate, r, format!("{r} Hz"));
                    }
                });
                ui.end_row();
                ui.label("Channels:");
                egui::ComboBox::from_id_salt("channels")
                    .selected_text(match d.channels {
                        1 => "Mono".to_string(),
                        2 => "Stereo".to_string(),
                        6 => "5.1".to_string(),
                        n => format!("{n} channels"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.channels, 1, "Mono");
                        ui.selectable_value(&mut d.channels, 2, "Stereo");
                        ui.selectable_value(&mut d.channels, 6, "5.1");
                    });
                ui.end_row();
                ui.label("Bit depth:");
                egui::ComboBox::from_id_salt("bits")
                    .selected_text(if d.bits == 32 { "32-bit float".to_string() } else { format!("{}-bit", d.bits) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.bits, 16, "16-bit");
                        ui.selectable_value(&mut d.bits, 24, "24-bit");
                        ui.selectable_value(&mut d.bits, 32, "32-bit float");
                    });
                ui.end_row();
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
                if let Some(d) = self.new_audio.take()
                    && let Err(e) = self.new_audio_file(d.rate, d.channels, d.bits, ctx)
                {
                    self.error = Some(e);
                }
            }
            Some(false) => self.new_audio = None,
            None => {}
        }
    }

    /// The actions that add, close or save documents: the window carries them out.
    /// `interactive` is false for the control channel, which cannot answer a dialog.
    fn run_doc_action(&mut self, id: ActionId, ctx: &egui::Context, interactive: bool) -> Result<bool, String> {
        match id {
            ActionId::CopyToNew => self.copy_to_new(ctx)?,
            ActionId::PasteToNew => self.paste_to_new(ctx)?,
            ActionId::CloseAll => self.close_all_docs(interactive)?,
            ActionId::SaveAll => self.save_all_docs(interactive)?,
            _ => return Err(format!("{} is not available now", actions::action(id).menu)),
        }
        Ok(false)
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

    /// The Files panel's rows, one per open document, in the order opened.
    fn file_rows(&self) -> Vec<files::Row> {
        (0..self.docs.len(self.editor.is_some()))
            .filter_map(|i| {
                let (path, editor, unsaved) = match self.docs.locate(i) {
                    Where::Current => (&self.doc_path, &self.editor, self.unsaved()),
                    Where::Parked(p) => {
                        let s = self.docs.parked().get(p)?;
                        (&s.doc_path, &s.editor, is_unsaved(&s.editor, &s.saved))
                    }
                };
                let state = editor.as_ref()?.history.current();
                let src = state.source();
                let bits = match src.sample_format {
                    SampleFormat::Int8 => "8",
                    SampleFormat::Int16 => "16",
                    SampleFormat::Int24 => "24",
                    SampleFormat::Int32 => "32",
                    SampleFormat::Float32 => "32f",
                    SampleFormat::Float64 => "64f",
                    SampleFormat::Compressed => "-",
                };
                Some(files::Row {
                    doc: i,
                    name: path_name(path),
                    unsaved,
                    frames: state.len_frames(),
                    rate: state.sample_rate(),
                    channels: state.channel_count(),
                    bits: bits.to_string(),
                    format: format!("{:?}", src.format),
                    path: path.to_string_lossy().into_owned(),
                })
            })
            .collect()
    }

    /// The document the shown row `i` stands for (rows are sorted and filtered).
    fn file_row_doc(&self, i: usize) -> Result<usize, String> {
        let rows = self.file_rows();
        self.files
            .arrange(&rows)
            .get(i)
            .map(|r| r.doc)
            .ok_or_else(|| format!("no row {i} (the table shows {} rows)", self.files.arrange(&rows).len()))
    }

    /// Reveal a document's file in the system file manager (Linux: its folder).
    fn reveal_file(&self, doc: usize) -> Result<(), String> {
        let path = self.file_rows().into_iter().find(|r| r.doc == doc).map(|r| r.path).unwrap_or_default();
        let (prog, args) = files::reveal_command(std::env::consts::OS, &path).ok_or("this document has no file yet: Save As first")?;
        std::process::Command::new(&prog).args(&args).spawn().map(drop).map_err(|e| format!("could not run {prog}: {e}"))
    }

    /// Window › Files: the open documents as a table.
    fn files_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Files");
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.text_edit_singleline(&mut self.files.filter);
        });
        let rows = self.file_rows();
        let shown: Vec<files::Row> = self.files.arrange(&rows).into_iter().cloned().collect();
        let cur = self.docs.current();
        let busy = self.saving.is_some() || self.making.is_some() || self.save_all.is_some();
        let (mut go, mut open, mut close, mut reveal) = (None, false, false, false);
        egui::ScrollArea::both().max_height((ui.available_height() - 40.0).max(60.0)).show(ui, |ui| {
            egui::Grid::new("files_grid").striped(true).show(ui, |ui| {
                for col in files::Col::ALL {
                    let arrow = match self.files.sort {
                        Some((c, true)) if c == col => " ▲",
                        Some((c, false)) if c == col => " ▼",
                        _ => "",
                    };
                    if ui.button(format!("{}{arrow}", col.title())).clicked() {
                        self.files.click_header(col);
                    }
                }
                ui.end_row();
                for r in &shown {
                    let on = r.doc == cur;
                    let cells = [
                        r.name.clone(),
                        if r.unsaved { "*".to_string() } else { String::new() },
                        format_time(r.frames as usize, r.rate),
                        r.rate.to_string(),
                        r.channels.to_string(),
                        r.bits.clone(),
                        r.format.clone(),
                        r.path.clone(),
                    ];
                    for text in cells {
                        let resp = ui.selectable_label(on, text);
                        if resp.clicked() || resp.double_clicked() {
                            go = Some(r.doc);
                        }
                        resp.context_menu(|ui| {
                            go = Some(r.doc);
                            if ui.button("Open…").clicked() {
                                open = true;
                                ui.close();
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Close")).clicked() {
                                close = true;
                                ui.close();
                            }
                            if ui.add_enabled(!r.path.is_empty(), egui::Button::new(reveal_label())).clicked() {
                                reveal = true;
                                ui.close();
                            }
                        });
                    }
                    ui.end_row();
                }
            });
        });
        ui.horizontal(|ui| {
            open |= ui.button("Open…").clicked();
            close |= ui.add_enabled(!busy && self.editor.is_some(), egui::Button::new("Close")).clicked();
            reveal |= ui.add_enabled(self.editor.is_some(), egui::Button::new(reveal_label())).clicked();
        });
        // Clicking a row makes it current; Open, Close and Reveal then act on the current document.
        if let Some(i) = go
            && let Err(e) = self.switch_doc(i)
        {
            self.error = Some(e);
        }
        if open {
            self.pick_file(ui.ctx());
        }
        if close && let Err(e) = self.close_doc() {
            self.error = Some(e);
        }
        if reveal && let Err(e) = self.reveal_file(self.docs.current()) {
            self.error = Some(e);
        }
    }

    /// `FILES …` from the control channel.
    fn files_request(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        match words.next().map(str::to_ascii_lowercase).as_deref() {
            None => {
                let rows = self.file_rows();
                let shown = self.files.arrange(&rows);
                let mut out = format!(
                    "OK count={} total={} current={} sort={} filter={:?}",
                    shown.len(),
                    rows.len(),
                    self.docs.current(),
                    self.files.sort_text(),
                    self.files.filter.trim()
                );
                for (i, r) in shown.iter().enumerate() {
                    out.push_str(&format!(
                        " | {i} doc={} current={} unsaved={} frames={} rate={} ch={} bits={} format={} name={} path={}",
                        r.doc,
                        u8::from(r.doc == self.docs.current()),
                        u8::from(r.unsaved),
                        r.frames,
                        r.rate,
                        r.channels,
                        r.bits,
                        r.format,
                        r.name,
                        r.path
                    ));
                }
                out
            }
            Some("sort") => {
                let (col, dir) = (words.next(), words.next().map(str::to_ascii_lowercase));
                match (col.map(str::to_ascii_lowercase).as_deref(), col.and_then(files::Col::parse), dir.as_deref()) {
                    (Some("none"), _, None) => self.files.sort = None,
                    (_, Some(c), Some("asc")) => self.files.sort = Some((c, true)),
                    (_, Some(c), Some("desc")) => self.files.sort = Some((c, false)),
                    _ => return "ERR FILES sort <name|unsaved|duration|rate|channels|bits|format|path> asc|desc, or FILES sort none".to_string(),
                }
                format!("OK sort={}", self.files.sort_text())
            }
            Some("filter") => {
                let text = rest.trim().get("filter".len()..).unwrap_or("").trim();
                self.files.filter = text.to_string();
                format!("OK filter={text:?}")
            }
            Some("select") => match words.next().and_then(|w| w.parse::<usize>().ok()) {
                Some(i) => match self.file_row_doc(i).and_then(|d| self.switch_doc(d).map(|()| d)) {
                    Ok(d) => format!("OK current={d}"),
                    Err(e) => format!("ERR {e}"),
                },
                None => "ERR FILES select <row>, from FILES".to_string(),
            },
            // What Reveal would run for row <i> (the current document without one). It is not run from here.
            Some("reveal") => {
                let doc = match words.next() {
                    Some(w) => w.parse::<usize>().map_err(|_| "FILES reveal [row]".to_string()).and_then(|i| self.file_row_doc(i)),
                    None => Ok(self.docs.current()),
                };
                match doc.map(|d| self.file_rows().into_iter().find(|r| r.doc == d).map(|r| r.path).unwrap_or_default()) {
                    Ok(path) => {
                        files::reveal_text(&path).map_or_else(|| "ERR this document has no file yet".to_string(), |c| format!("OK would run: {c}"))
                    }
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR FILES [sort <col> asc|desc | filter <text> | select <row> | reveal [row]]".to_string(),
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
        self.save_panels();
    }

    /// Keep which panels are shown and where they are docked in the settings file.
    fn save_panels(&mut self) {
        self.settings.hidden_panels = self.panels.hidden_ids();
        (self.settings.panel_layout, self.settings.zone_sizes) = self.layout.encode();
        self.settings.workspace = self.workspaces.current().to_string();
        self.layout_dirty = false;
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the panel layout to {}: {e}", file.display());
        }
    }

    /// The layout as a workspace would keep it.
    fn snapshot(&self) -> Snapshot {
        Snapshot::capture(&self.panels, &self.layout)
    }

    /// Put a workspace's panels and layout on screen and keep them in the settings file.
    fn apply_snapshot(&mut self, snap: &Snapshot) {
        (self.panels, self.layout) = snap.restore();
        self.save_panels();
    }

    /// `WORKSPACE save|switch|reset|delete <name>` (also the Window › Workspace menu). Returns
    /// the reason it was refused.
    fn workspace_command(&mut self, text: &str) -> Result<(), String> {
        let (verb, name) = text.trim().split_once(char::is_whitespace).map_or((text.trim(), ""), |(v, n)| (v, n.trim()));
        match verb {
            "save" => {
                let snap = self.snapshot();
                self.workspaces.save(name, &snap)?;
                self.save_panels();
            }
            "switch" => {
                let snap = self.workspaces.switch(name)?;
                self.apply_snapshot(&snap);
            }
            "reset" => {
                let snap = self.workspaces.reset(Some(name))?;
                self.apply_snapshot(&snap);
            }
            "delete" => self.workspaces.delete(name)?,
            _ => return Err("WORKSPACE save <name> | switch <name> | reset [name] | delete <name>".into()),
        }
        Ok(())
    }

    /// The answer to `WORKSPACES` and `WORKSPACE …`.
    fn workspaces_report(&self) -> String {
        format!("OK {}", self.workspaces.report(&self.snapshot()))
    }

    /// Close a panel (hide it in the registry; the Window menu brings it back).
    fn close_panel(&mut self, id: &str) {
        self.layout.apply_command(&mut self.panels, &format!("close {id}")).ok();
        self.save_panels();
    }

    /// The answer to `PANELS` and `PANEL …`: which panels are shown, then the zones.
    fn panels_report(&self) -> String {
        let kind = self.float_kind.map_or(String::new(), |k| format!(" float_window={k}"));
        format!("OK {} {}{kind}", self.panels.report(), self.layout.report(&self.panels))
    }

    /// The body of a docked panel.
    fn panel_body(&mut self, id: &str, ui: &mut egui::Ui) {
        match id {
            "history" => self.history_panel(ui),
            "selection" => self.selection_panel(ui),
            _ => {}
        }
    }

    /// Window › Selection: the cursor or selection in the file's time, in words.
    fn selection_panel(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = &self.doc else {
            ui.label("No file open.");
            return;
        };
        let rate = doc.sample_rate();
        egui::Grid::new("selection-grid").num_columns(2).show(ui, |ui| {
            if self.selection.is_empty() {
                ui.label("Cursor");
                ui.label(format_time(self.selection.start(), rate));
                ui.end_row();
            } else {
                let (a, b) = (self.selection.start(), self.selection.end());
                for (name, frames) in [("Start", a), ("End", b), ("Length", b - a)] {
                    ui.label(name);
                    ui.label(format_time(frames, rate));
                    ui.end_row();
                }
            }
            ui.label("File");
            ui.label(format_time(doc.frames(), rate));
            ui.end_row();
        });
    }

    /// One docked zone: a tab strip (drag a tab to move it, wheel to change tab, right-click for
    /// the menu) over the front tab's body. Nothing is drawn for a zone with no shown panel.
    fn dock_zone(&mut self, ui: &mut egui::Ui, zone: DockSide) {
        let tabs: Vec<&'static str> = self.layout.visible(zone, &self.panels).into_iter().filter_map(|id| panels::by_id(id).map(|p| p.id)).collect();
        let Some(front) = self.layout.active_in(zone, &self.panels).and_then(panels::by_id).map(|p| p.id) else { return };
        let size = self.layout.size(zone).unwrap_or(220.0);
        let panel = match zone {
            DockSide::Left => egui::Panel::left("zone-left"),
            DockSide::Bottom => egui::Panel::bottom("zone-bottom"),
            _ => egui::Panel::right("zone-right"),
        };
        let shown = panel.default_size(size).size_range(panels::ZONE_SIZE_RANGE.0..=panels::ZONE_SIZE_RANGE.1).show(ui, |ui| {
            let mut wheel = 0.0f32;
            let strip = egui::ScrollArea::horizontal().id_salt(("tabs", zone.name())).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for id in &tabs {
                        let title = panels::by_id(id).map_or(*id, |p| p.title);
                        let label = ui.selectable_label(*id == front, title);
                        let r = ui.interact(label.rect, label.id.with("tab"), Sense::click_and_drag());
                        self.tab_rects.push((id.to_string(), label.rect));
                        if r.clicked() {
                            self.layout.activate(id);
                            self.layout_dirty = true;
                        }
                        if r.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), id.to_string());
                        }
                        r.context_menu(|ui| self.panel_menu(ui, id));
                    }
                });
            });
            if ui.rect_contains_pointer(strip.inner_rect) {
                wheel =
                    ui.input(|i| i.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.x + delta.y } else { 0.0 }).sum());
            }
            if wheel != 0.0 {
                self.layout.cycle(zone, &self.panels, if wheel < 0.0 { 1 } else { -1 });
                self.layout_dirty = true;
            }
            ui.separator();
            self.panel_body(front, ui);
        });
        let rect = shown.response.rect;
        self.zone_rects.push((zone, rect));
        if shown.response.contains_pointer() {
            self.hover_next = Some(front);
        }
        let now = if zone == DockSide::Bottom { rect.height() } else { rect.width() };
        if self.layout.set_size(zone, now) {
            self.layout_dirty = true;
        }
    }

    /// Every floating panel in a window of its own: an operating-system window where the backend
    /// has more than one (the viewport), else egui's in-window floating window (the same call
    /// falls back by itself). Closing the window docks the panel back where it came from.
    fn floating_windows(&mut self, ctx: &egui::Context) {
        let floats: Vec<panels::Float> = self.layout.floats(&self.panels).into_iter().cloned().collect();
        let mut dock_back: Vec<String> = Vec::new();
        for f in floats {
            let title = panels::by_id(&f.id).map_or("Panel", |p| p.title);
            let vid = egui::ViewportId::from_hash_of(("float", f.id.as_str()));
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("WavCraft: {title}"))
                .with_position(egui::pos2(f.pos[0], f.pos[1]))
                .with_inner_size(vec2(f.size[0], f.size[1]))
                .with_min_inner_size(vec2(160.0, 100.0));
            let mut dock = false;
            let mut kind = "os";
            let mut rect: Option<([f32; 2], [f32; 2])> = None;
            ctx.show_viewport_immediate(vid, builder, |ui, class| {
                let mut body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Dock").on_hover_text("Put the panel back in a zone").clicked() {
                            dock = true;
                        }
                    });
                    ui.separator();
                    self.panel_body(&f.id, ui);
                };
                if class == egui::ViewportClass::EmbeddedWindow {
                    kind = "embedded";
                    body(ui);
                    // Embedded: the window is an egui area; its rect is in the main window.
                    let area = ui.ctx().memory(|m| m.area_rect(egui::Id::new(vid)));
                    rect = area.map(|r| ([r.min.x, r.min.y], [r.width(), r.height()]));
                } else {
                    egui::CentralPanel::default().show(ui, |ui| body(ui));
                    let (outer, inner, close) =
                        ui.ctx().input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
                    if close {
                        dock = true;
                    }
                    if let (Some(o), Some(i)) = (outer, inner) {
                        rect = Some(([o.min.x, o.min.y], [i.width(), i.height()]));
                    }
                }
            });
            self.float_kind = Some(kind);
            if dock {
                dock_back.push(f.id.clone());
            } else if let Some((pos, size)) = rect
                && self.layout.set_float_rect(&f.id, pos, size)
            {
                self.layout_dirty = true;
            }
        }
        if self.layout.floats(&self.panels).is_empty() {
            self.float_kind = None;
        }
        for id in dock_back {
            self.layout.dock(&id, None).ok();
            self.save_panels();
        }
    }

    /// Window › Workspace: switch, save as new, reset to the saved layout, delete.
    fn workspace_menu(&mut self, ui: &mut egui::Ui) {
        let current = self.workspaces.current().to_string();
        let modified = self.workspaces.is_modified(&self.snapshot());
        let mut result: Result<(), String> = Ok(());
        let mut done = false;
        for name in self.workspaces.names() {
            let label = if name == current && modified { format!("{name} (changed)") } else { name.clone() };
            if ui.radio(name == current, label).clicked() && name != current {
                result = self.workspace_command(&format!("switch {name}"));
                done = true;
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.workspace_name).hint_text("Name").desired_width(120.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save as New Workspace").clicked() || enter) && !self.workspace_name.trim().is_empty() {
                result = self.workspace_command(&format!("save {}", self.workspace_name));
                if result.is_ok() {
                    self.workspace_name.clear();
                }
                done = true;
            }
        });
        if ui.add_enabled(modified, egui::Button::new("Reset to Saved Layout")).clicked() {
            result = self.workspace_command(&format!("reset {current}"));
            done = true;
        }
        ui.menu_button("Delete Workspace", |ui| {
            let others: Vec<String> = self.workspaces.names().into_iter().filter(|n| *n != current && n != workspaces::DEFAULT).collect();
            if others.is_empty() {
                ui.add_enabled(false, egui::Label::new("No other saved workspaces"));
            }
            for name in others {
                if ui.button(&name).clicked() {
                    result = self.workspace_command(&format!("delete {name}"));
                    done = true;
                    ui.close();
                }
            }
        });
        if let Err(e) = result {
            self.error = Some(e);
        }
        if done {
            ui.close();
        }
    }

    /// Float a panel, or with `group` every tab of its zone, near the top left of the main window.
    fn float_panel(&mut self, ctx: &egui::Context, id: &str, group: bool) {
        let origin = ctx.input(|i| i.viewport().outer_rect).map_or([120.0, 120.0], |r| [r.min.x + 80.0, r.min.y + 100.0]);
        let ids: Vec<String> = match (group, self.layout.zone_of(id)) {
            (true, Some(z)) => self.layout.visible(z, &self.panels).into_iter().map(String::from).collect(),
            _ => vec![id.to_string()],
        };
        for (n, id) in ids.iter().enumerate() {
            let off = n as f32 * 28.0;
            self.layout.float(id, Some(([origin[0] + off, origin[1] + off], [320.0, 260.0]))).ok();
        }
        self.save_panels();
    }

    /// The right-click menu of a docked panel's tab.
    fn panel_menu(&mut self, ui: &mut egui::Ui, id: &str) {
        let here = self.layout.zone_of(id);
        for zone in panels::ZONES {
            if Some(zone) != here && ui.button(format!("Move to {}", zone.name())).clicked() {
                self.layout.move_to(id, zone, None).ok();
                self.save_panels();
                ui.close();
            }
        }
        if self.layout.is_floating(id) {
            if ui.button("Dock").clicked() {
                self.layout.dock(id, None).ok();
                self.save_panels();
                ui.close();
            }
        } else {
            if ui.button("Undock Panel (float)").clicked() {
                self.float_panel(ui.ctx(), id, false);
                ui.close();
            }
            if ui.button("Undock Frame (float all tabs)").clicked() {
                self.float_panel(ui.ctx(), id, true);
                ui.close();
            }
        }
        if ui.button("Maximize (`)").clicked() {
            self.layout.toggle_maximize(id);
            ui.close();
        }
        if ui.button("Close (Ctrl+W)").clicked() {
            self.close_panel(id);
            ui.close();
        }
    }

    /// While a tab is dragged: where it would land (a tab, a zone, or an edge of the window),
    /// shown as a highlight; letting go moves it there.
    fn dock_drag(&mut self, ctx: &egui::Context) {
        let Some(payload) = egui::DragAndDrop::payload::<String>(ctx) else { return };
        let id: String = (*payload).clone();
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
        let window = ctx.content_rect();
        let edge = |zone: DockSide| match zone {
            DockSide::Left => Rect::from_min_max(window.min, pos2(window.left() + window.width() * 0.2, window.bottom())),
            DockSide::Right => Rect::from_min_max(pos2(window.right() - window.width() * 0.2, window.top()), window.max),
            _ => Rect::from_min_max(pos2(window.left(), window.bottom() - window.height() * 0.25), window.max),
        };
        let mut target: Option<(DockSide, Option<usize>, Rect)> = None;
        if let Some((tab, rect)) = self.tab_rects.iter().find(|(_, r)| r.contains(pos))
            && let Some((zone, index)) = self.layout.position(tab)
        {
            target = Some((zone, Some(index), *rect));
        } else if let Some((zone, rect)) = self.zone_rects.iter().find(|(_, r)| r.contains(pos)) {
            target = Some((*zone, None, *rect));
        } else if let Some(zone) = [DockSide::Left, DockSide::Right, DockSide::Bottom].into_iter().find(|z| edge(*z).contains(pos)) {
            target = Some((zone, None, edge(zone)));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some((_, _, rect)) = target {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-drop")));
            painter.rect_filled(rect, 2.0, ACCENT.gamma_multiply(0.35));
            painter.rect_stroke(rect, 2.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        }
        if ctx.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            if let Some((zone, index, _)) = target
                && self.layout.move_to(&id, zone, index).is_ok()
            {
                self.save_panels();
            }
        } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx);
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

    /// A frame set from the ruler, snapped to a timecode frame boundary when the display is SMPTE
    /// and snapping to frames is on.
    fn snap_gesture(&self, frame: usize) -> usize {
        match &self.doc {
            Some(d) if self.snap_frames => self.time_format.snap_to_frame(frame, d.sample_rate(), d.frames()),
            _ => frame,
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

    /// Start a take. Nothing plays while it runs (C024).
    fn start_recording(&mut self, fmt: wavcraft_core::record::RecFormat) -> Result<PathBuf, String> {
        self.start_recording_max(fmt, None)
    }

    /// Start a take that stops by itself after `max_secs` of recorded audio, if given (C028).
    fn start_recording_max(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) -> Result<PathBuf, String> {
        if self.op_running() {
            return Err("wait for the open or save to finish".to_string());
        }
        self.begin_recording(fmt, max_secs)
    }

    /// An open or save is running (or the test hook says so).
    fn op_running(&self) -> bool {
        self.opening.is_some() || self.saving.is_some() || self.test_busy_until.is_some_and(|t| std::time::Instant::now() < t)
    }

    /// A timed start whose time has come. A take has its own writer and shares nothing with an
    /// open or save, so it starts right away even while one runs; it is never dropped. The delay
    /// and the overlap are logged, shown in the status bar and kept in `STATE`.
    fn start_timed(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) {
        let busy = self.op_running();
        match self.begin_recording(fmt, max_secs) {
            Ok(path) => {
                let late = self.rec.last_timed.map_or(0, |t| t.0);
                self.rec.last_timed = Some((late, busy));
                let msg = format!(
                    "Timed recording started {late} ms after its time{} -> {}",
                    if busy { ", while an open or save was running" } else { "" },
                    path.display()
                );
                log::info!(target: "wavcraft", "{msg}");
                if busy || late >= 1000 {
                    self.notice = Some(msg);
                }
            }
            Err(why) => log::error!(target: "wavcraft", "Timed recording could NOT start: {why}"),
        }
    }

    fn begin_recording(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) -> Result<PathBuf, String> {
        self.player.set_muted(true);
        match self.rec.start(fmt, max_secs) {
            Ok(path) => {
                self.error = None;
                self.notice = None;
                Ok(path)
            }
            Err(e) => {
                self.player.set_muted(false);
                let why = format!("Recording: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Why the document may not be edited now: a recording is going into it, or has just been
    /// recorded and is being brought in.
    fn edit_lock(&self) -> Option<&'static str> {
        (self.rec.going_into_doc().is_some() || self.integrating.is_some()).then_some("wait for the recording to finish going into the file")
    }

    /// Record into the open document (C026 insert at the cursor or replace the selection, C027
    /// punch-in, C030 target channel). The take is written to its own file first, exactly like a
    /// take for a new file; only when it has stopped is it brought into the document, as one edit.
    /// Nothing in the document changes before then, and the edit is refused if it did meanwhile.
    fn start_recording_into(
        &mut self,
        want: wavcraft_core::record::RecFormat,
        mode: RecMode,
        target: RecTarget,
        preroll_ms: u64,
    ) -> Result<PathBuf, String> {
        let fail = |app: &mut WaveApp, e: String| {
            let why = format!("Recording: {e}");
            log::warn!(target: "wavcraft", "{why}");
            app.error = Some(why.clone());
            Err(why)
        };
        if self.opening.is_some() || self.saving.is_some() || self.rebuilding.is_some() || self.integrating.is_some() {
            return fail(self, "wait for the open, save or earlier recording to finish".to_string());
        }
        let Some(editor) = self.editor.as_ref() else { return fail(self, "open a file to record into first".to_string()) };
        let base = editor.history.current().clone();
        let sel = self.selection_range();
        let plan = RecPlan { mode, target, cursor: sel.start, selection: sel.clone() };
        if let Err(e) = plan.check(&base) {
            return fail(self, e);
        }
        let fmt = wavcraft_core::record::RecFormat { rate: base.sample_rate(), channels: plan.take_channels(&base), bits: want.bits };
        let punch = mode == RecMode::Punch;
        let limit = plan.limit_frames().map(Max::Frames);
        let from = sel.start.saturating_sub(preroll_ms * u64::from(base.sample_rate()) / 1000);
        self.player.set_muted(!punch);
        match self.rec.start_with(fmt, limit, punch) {
            Ok(path) => {
                self.rec.set_into(plan, base, self.doc_id, punch.then_some(from), fmt.rate);
                if punch {
                    // The preroll and the selection play from the original audio while the take waits.
                    self.player.play(from as usize, sel.end as usize);
                }
                self.error = None;
                self.notice = None;
                Ok(path)
            }
            Err(e) => {
                self.player.set_muted(false);
                fail(self, e)
            }
        }
    }

    /// Open the finished take by the normal streamed open and build the one edit that puts it in
    /// the document; `finish_integration` applies it.
    fn integrate_take(&mut self, take: &wavcraft_core::record::Take, into: crate::recording::IntoDoc, ctx: &egui::Context) {
        let Some(editor) = self.editor.as_ref() else { return };
        let (tx, rx) = channel();
        let (ctx, path, base, plan, xf) = (ctx.clone(), take.path.clone(), into.base.clone(), into.plan.clone(), editor.crossfades);
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let result = wavcraft_core::io::open(&path, &opts, &mut |_| {}, &cancel)
                .map_err(|e| e.to_string())
                .and_then(|take| rec_edit::apply_take(&base, &take, &plan, xf).map_err(|e| e.to_string()));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.integrating = Some(Integrating { result: rx, base: into.base, doc_id: into.doc_id, path: take.path.clone(), plan: into.plan });
    }

    fn finish_integration(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.integrating else { return };
        let result = match job.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the worker stopped unexpectedly".to_string()),
        };
        let Some(Integrating { base, doc_id, path, plan, .. }) = self.integrating.take() else { return };
        let kept = format!("The take is kept in {}", path.display());
        let edit = match result {
            Ok(edit) => edit,
            Err(e) => {
                let why = format!("Recording could not be brought into the file: {e}. {kept}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why);
                return;
            }
        };
        let Some(editor) = self.editor.as_mut() else { return };
        if doc_id != self.doc_id || !Arc::ptr_eq(editor.history.current(), &base) {
            let why = format!("The file changed while recording, so the take was not put in. {kept}");
            log::warn!(target: "wavcraft", "{why}");
            self.error = Some(why);
            return;
        }
        if edit.is_noop() {
            self.notice = Some(format!("Nothing was recorded. {kept}"));
            return;
        }
        let (old_len, new_len) = (base.len_frames(), edit.state.len_frames());
        let selected = match &edit.change {
            wavcraft_core::doc::edit::Change::SameLength { range, .. } => range.clone(),
            wavcraft_core::doc::edit::Change::LengthFrom { frame } => {
                let replaced = if plan.mode == RecMode::Replace { plan.selection.end - plan.selection.start } else { 0 };
                *frame..(*frame + (new_len + replaced).saturating_sub(old_len)).min(new_len)
            }
            wavcraft_core::doc::edit::Change::None => plan.cursor..plan.cursor,
        };
        let label = edit.label.clone();
        editor.history.push_edit(edit);
        log::info!(target: "wavcraft", "{label}: frames {}..{} of {new_len}, one history step", selected.start, selected.end);
        self.selection = Selection { anchor: selected.start as usize, head: selected.end as usize };
        self.error = None;
        self.notice = Some(format!("{label}: {kept}"));
        self.rebuild(ctx);
    }

    /// Stop the take and, when `open` is set, open it in the editor by the normal path.
    fn finish_recording(&mut self, open: bool, ctx: &egui::Context) -> Option<wavcraft_core::record::Take> {
        let take = self.rec.stop()?;
        let into = self.rec.take_into();
        self.player.set_muted(false);
        if into.is_some() {
            self.player.stop();
        }
        if let Some(e) = &take.error {
            self.error = Some(format!("Recording stopped early: {e} The take so far is kept in {}", take.path.display()));
        }
        if take.dropped_frames > 0 {
            let ms = take.dropped_frames * 1000 / u64::from(take.fmt.rate.max(1));
            let why = format!(
                "Recording lost {} frames (about {ms} ms) because the disk could not keep up. The take is kept in {} but has gaps.",
                take.dropped_frames,
                take.path.display()
            );
            log::warn!(target: "wavcraft", "{why}");
            self.error.get_or_insert(why);
        }
        if let Some(into) = into {
            if open && take.frames > 0 {
                self.integrate_take(&take, into, ctx);
            } else {
                log::info!(target: "wavcraft", "the take {} was not put in the file ({} frames); it is kept", take.path.display(), take.frames);
                if take.frames == 0 {
                    self.notice = Some("Nothing was recorded".to_string());
                }
            }
        } else if open && take.frames > 0 {
            self.open(take.path.clone(), ctx);
            if take.error.is_none() {
                self.notice = Some(format!("Recorded {}", take.path.display()));
            }
        }
        Some(take)
    }

    /// Record now with the last format used (the default the first time): no dialog (C031).
    fn quick_record(&mut self) -> Result<PathBuf, String> {
        let fmt = self.rec.last_format();
        self.start_recording(fmt)
    }

    /// Space and the menu's Play button: stop while playing, resume while paused, else play.
    fn toggle_play(&mut self) {
        if self.rec.is_active() {
            return;
        }
        match self.transport.mode {
            Mode::Playing => self.stop_play(),
            Mode::Paused => self.resume_play(),
            Mode::Stopped => self.start_play(),
        }
    }

    /// Hand the player the audio of the open file (or none); playback ends.
    fn set_audio(&mut self, audio: Option<Arc<wavcraft_core::AudioBuffer>>) {
        self.transport.mode = Mode::Stopped;
        self.player.set_audio(audio);
    }

    /// Stop at once with the cursor left where it is (the file or the cursor changed under it).
    fn halt_play(&mut self) {
        self.player.stop();
        self.transport.mode = Mode::Stopped;
    }

    /// Play from the cursor, or the selection, as the Loop and Skip Selection options say.
    fn start_play(&mut self) {
        let Some(doc) = &self.doc else { return };
        let width = self.editor_width.max(1.0);
        let req = transport::Request {
            selection: self.selection.start()..self.selection.end(),
            cursor: self.selection.start(),
            frames: doc.frames(),
            view: self.view.start..self.view.end(width),
            rate: doc.sample_rate(),
            looping: self.transport.looping,
            skip_selection: self.transport.skip_selection,
        };
        let Some((plan, from)) = transport::plan_for(&req) else {
            self.notice = Some("Nothing to play from here".to_string());
            return;
        };
        if self.player.start(&plan, from) {
            self.transport.mode = Mode::Playing;
            self.transport.play_start = self.selection.start();
        }
    }

    fn pause_play(&mut self) {
        match self.transport.mode {
            Mode::Playing => {
                self.player.pause();
                self.transport.mode = Mode::Paused;
            }
            Mode::Paused => self.resume_play(),
            Mode::Stopped => {}
        }
    }

    fn resume_play(&mut self) {
        if self.player.resume() {
            self.transport.mode = Mode::Playing;
        } else {
            self.stop_play();
        }
    }

    /// Stop. With nothing selected the cursor returns to where play started, or stays where
    /// playback stopped when Return to Start Position is off.
    fn stop_play(&mut self) {
        let was = self.transport.mode;
        let at = self.player.position();
        self.halt_play();
        if was != Mode::Stopped && self.selection.is_empty() {
            let frames = self.doc.as_ref().map_or(0, Document::frames);
            self.selection = Selection::at(self.transport.cursor_after_stop(at).min(frames));
        }
    }

    /// Advance the silent clock and notice when playback has run out by itself.
    fn poll_transport(&mut self) {
        self.player.tick();
        if self.transport.mode == Mode::Playing && !self.player.is_playing() {
            self.stop_play();
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

    /// Run what a shortcut is bound to (a key press or the control channel's `KEY`). Returns
    /// whether the document changed.
    fn run_command(&mut self, command: Command, ctx: &egui::Context) -> Result<bool, String> {
        match command {
            Command::Action(id) => self.run_action(id, ctx),
            Command::Save | Command::SaveAs => self.save(ctx, command == Command::SaveAs, None, false).map(|()| false),
            Command::PlayStop => {
                self.toggle_play();
                Ok(false)
            }
            Command::Open => {
                self.pick_file(ctx);
                Ok(false)
            }
        }
    }

    /// Run an action from the table on the open file (menu, key or control channel). Returns
    /// whether the document changed.
    fn run_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if let Some(why) = self.edit_lock() {
            return Err(why.to_string());
        }
        if self.converting.is_some() {
            return Err("wait for the conversion to finish".to_string());
        }
        if matches!(id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
            return self.run_doc_action(id, ctx, true).inspect_err(|e| self.error = Some(e.clone()));
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
                    self.halt_play();
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

    /// What Scan would measure: the selection, or the whole file when there is none.
    fn stats_scope(&self) -> (Range<u64>, bool) {
        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames());
        if self.selection.is_empty() { (0..len, true) } else { (self.selection_range(), false) }
    }

    /// Start the panel's scan on a worker thread.
    fn start_stats(&mut self, ctx: &egui::Context) {
        let (range, whole) = self.stats_scope();
        let Some(state) = self.editor.as_ref().map(|e| e.history.current().clone()) else { return };
        self.stats.start(state, range, whole, ctx);
    }

    /// Move the cursor to the peak or to the next / previous possibly clipped run of the last scan.
    fn stats_nav(&mut self, nav: Nav) -> Result<(), String> {
        let current = self.editor.as_ref().map(|e| e.history.current().clone());
        let Some(m) = self.stats.measured.as_ref() else { return Err("scan first (Window › Amplitude Statistics)".to_string()) };
        if current.is_none_or(|c| !Arc::ptr_eq(&c, &m.state)) {
            return Err("the file changed since the last scan: scan again".to_string());
        }
        let cursor = self.selection.start() as u64;
        let found = match nav {
            Nav::Peak => m.stats.peak_location().map(|(_, f)| f),
            Nav::NextClip => m.stats.next_clip_after(cursor),
            Nav::PrevClip => m.stats.prev_clip_before(cursor),
        };
        let Some(frame) = found else {
            return Err(match nav {
                Nav::Peak => "nothing was measured".to_string(),
                Nav::NextClip => "no further possibly clipped run".to_string(),
                Nav::PrevClip => "no earlier possibly clipped run".to_string(),
            });
        };
        self.selection = Selection::at(frame as usize);
        self.player.stop();
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        self.view.reveal(frame as f64, self.editor_width, frames);
        Ok(())
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
            ActionId::TimeSmpte24 => Some(TimeFormat::Smpte24),
            ActionId::TimeSmpte25 => Some(TimeFormat::Smpte25),
            ActionId::TimeSmpte2997Df => Some(TimeFormat::Smpte2997Df),
            ActionId::TimeSmpte2997Ndf => Some(TimeFormat::Smpte2997Ndf),
            ActionId::TimeSmpte30 => Some(TimeFormat::Smpte30),
            ActionId::TimeSmpte5994 => Some(TimeFormat::Smpte5994),
            _ => None,
        };
        if id == ActionId::SnapFrames {
            self.snap_frames = !self.snap_frames;
            return Ok(false);
        }
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
        let playhead = if self.transport.mode != Mode::Stopped { self.player.position() } else { self.selection.start() };
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
            ActionId::Play => match self.transport.mode {
                Mode::Stopped => self.start_play(),
                Mode::Paused => self.resume_play(),
                Mode::Playing => {}
            },
            ActionId::Pause => self.pause_play(),
            ActionId::Stop => self.stop_play(),
            ActionId::ToggleLoop => {
                self.transport.looping = !self.transport.looping;
                self.player.set_looping(self.transport.looping);
            }
            ActionId::ToggleSkipSelection => self.transport.skip_selection = !self.transport.skip_selection,
            ActionId::ToggleReturnOnStop => self.transport.return_on_stop = !self.transport.return_on_stop,
            ActionId::ToggleAutoScroll => self.transport.auto_scroll = !self.transport.auto_scroll,
            ActionId::FollowPage => {
                self.transport.follow = Follow::Page;
                self.transport.auto_scroll = true;
            }
            ActionId::FollowCenter => {
                self.transport.follow = Follow::Center;
                self.transport.auto_scroll = true;
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
            ActionId::StatsShow => self.stats.show = true,
            ActionId::StatsGotoPeak => self.stats_nav(Nav::Peak)?,
            ActionId::StatsNextClip => self.stats_nav(Nav::NextClip)?,
            ActionId::StatsPrevClip => self.stats_nav(Nav::PrevClip)?,
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
        if let Some(why) = self.edit_lock() {
            return Some(why.to_string());
        }
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
        let scale = |f: usize| (wavcraft_core::doc::rescale_frame(f as u64, old_rate, new_rate) as usize).min(new_len);
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
        if let Some(slot) = dialog.rack_slot {
            // Opened from the rack: keep the settings in the slot, run nothing.
            if ok {
                self.rack.set_slot_params(slot, dialog.index, dialog.values);
            }
            return Ok(false);
        }
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

    /// Process the selection (the whole file when none) through the Effects Rack as one step,
    /// then reset the rack to bypass (f31). Returns whether the document changed.
    fn apply_rack(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        if !self.rack.config.is_active() {
            return Err("the Effects Rack has nothing switched on".to_string());
        }
        match editor.run_rack(&self.rack.config, sel) {
            Ok(out) => {
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rack.config.bypass();
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("Effects Rack: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Draw the Effects Rack window, carry out what it asked for, and keep the player's copy of
    /// the rack current.
    fn rack_ui(&mut self, ctx: &egui::Context) {
        let can_apply = self.editor.is_some() && self.saving.is_none();
        self.rack.show(ctx, self.player.is_playing(), can_apply);
        for request in std::mem::take(&mut self.rack.requests) {
            match request {
                Request::EditSlot(i) => self.effect_dialog = self.rack.dialog_for(i),
                Request::TogglePlay => self.toggle_play(),
                Request::Apply => {
                    let _ = self.apply_rack(ctx);
                }
            }
        }
        self.sync_rack();
    }

    /// Hand the player the rack when it differs from the one it has.
    fn sync_rack(&mut self) {
        if self.rack.pushed.as_ref() != Some(&self.rack.config) {
            match self.player.set_rack(&self.rack.config) {
                Ok(()) => self.rack.pushed = Some(self.rack.config.clone()),
                Err(why) => {
                    log::warn!(target: "wavcraft", "Effects Rack: {why}");
                    self.error = Some(format!("Effects Rack: {why}"));
                    self.rack.pushed = Some(self.rack.config.clone());
                }
            }
        }
    }

    /// Set metadata fields as ONE undoable step ("Edit Metadata"). Nothing changes (and no step
    /// is made) when every value is already what the file has.
    fn apply_metadata(&mut self, changes: &[(String, String)]) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let current = editor.history.current().clone();
        let mut meta = current.source().meta.clone();
        for (field, value) in changes {
            if meta.get(field).as_deref() != Some(value.as_str()) {
                meta.set(field, value)?;
            }
        }
        if meta == current.source().meta {
            return Ok(false);
        }
        editor.history.push_metadata("Edit Metadata", current.with_metadata(meta));
        self.error = None;
        Ok(true)
    }

    /// Window › Metadata: the Broadcast Wave fields and the INFO title, artist and comment of the
    /// current step. Apply makes one history step.
    fn metadata_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Metadata");
        let Some(editor) = self.editor.as_ref() else {
            ui.label("No file open.");
            return;
        };
        let src = editor.history.current().source().clone();
        if self.meta_for.as_ref().is_none_or(|a| !Arc::ptr_eq(a, &src)) || self.meta_draft.len() != meta::FIELDS.len() {
            self.meta_draft = meta::FIELDS.iter().map(|f| src.meta.get(f).unwrap_or_default()).collect();
            self.meta_for = Some(src.clone());
        }
        let mut apply = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (field, text) in meta::FIELDS.iter().zip(self.meta_draft.iter_mut()) {
                ui.label(field.replace('_', " "));
                if *field == "coding_history" {
                    ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY));
                } else {
                    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                }
            }
            ui.add_space(6.0);
            apply = ui.button("Apply").clicked();
        });
        if apply {
            let changes: Vec<(String, String)> = meta::FIELDS.iter().map(|f| f.to_string()).zip(self.meta_draft.iter().cloned()).collect();
            match self.apply_metadata(&changes) {
                Ok(_) => self.meta_for = None,
                Err(e) => self.error = Some(format!("Metadata: {e}")),
            }
        }
    }

    fn markers_panel(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = self.editor.as_mut() else {
            ui.heading("Markers");
            ui.label("No file open.");
            return;
        };
        let out = self.markers_ui.panel(ui, editor, &mut self.settings);
        if let Some(span) = out.select {
            self.selection = Selection { anchor: span.start as usize, head: span.end as usize };
            self.player.stop();
            let frames = self.doc.as_ref().map_or(0, Document::frames);
            self.view.reveal(span.start as f64, self.editor_width, frames);
        }
        if out.columns_changed
            && let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            log::warn!(target: "wavcraft", "could not save the settings: {e}");
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
        self.matcher.audio_changed();
        let Some(editor) = &self.editor else { return };
        let (tx, rx) = channel();
        let (state, path, ctx) = (editor.history.current().as_ref().clone(), self.doc_path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Document::from_state(path, state));
            ctx.request_repaint();
        });
        self.halt_play();
        self.rebuilding = Some(rx);
    }

    /// A Match Loudness run from the window finished: one step on the history, unless the audio
    /// changed while it ran.
    fn finish_match(&mut self, ctx: &egui::Context) {
        let Some(done) = self.matcher.take_finished() else { return };
        let Some(editor) = self.editor.as_mut() else { return };
        if !Arc::ptr_eq(&done.base, editor.history.current()) {
            self.matcher.say(false, "The audio changed while matching; nothing was applied. Run it again.".to_string());
            return;
        }
        let changed = !done.result.edit.is_noop();
        editor.history.push_edit(done.result.edit);
        self.matcher.say(true, done.result.message.clone());
        self.notice = Some(done.result.message);
        if changed {
            self.rebuild(ctx);
        }
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
                self.set_audio(Some(doc.audio.clone()));
                self.doc = Some(doc);
                self.generation += 1;
                self.spectrum = None;
            }
            Err(OpenError::Empty) => {
                self.set_audio(None);
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

    /// File › Save: to the file's own path when it is a format a save writes (WAV, AIFF, FLAC),
    /// else as Save As. `to` skips the dialog (the control channel); the format follows its
    /// extension.
    fn save(&mut self, ctx: &egui::Context, save_as: bool, to: Option<PathBuf>, then_close: bool) -> Result<(), String> {
        self.save_with(ctx, save_as, to, then_close, self.save_type, false)
    }

    /// [`WaveApp::save`] with the sample type given (`explicit`: asked for by name, so a format
    /// that can't hold it refuses instead of using the nearest type).
    fn save_with(
        &mut self,
        ctx: &egui::Context,
        save_as: bool,
        to: Option<PathBuf>,
        then_close: bool,
        ty: Option<SampleType>,
        explicit: bool,
    ) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        if self.converting.is_some() {
            return Err("wait for the conversion to finish".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let own_format = SaveFormat::from_path(&self.doc_path).filter(|f| SaveFormat::from_file_format(state.source().format) == Some(*f));
        let path = match to {
            Some(p) => p,
            None if own_format.is_some() && !save_as => self.doc_path.clone(),
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let first = own_format.unwrap_or(SaveFormat::Wav);
                let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.{}", first.extension()));
                for f in [first, SaveFormat::Wav, SaveFormat::Aiff, SaveFormat::Flac].iter().fold(Vec::new(), |mut v, f| {
                    if !v.contains(f) {
                        v.push(*f);
                    }
                    v
                }) {
                    let exts: &[&str] = if f == SaveFormat::Aiff { &["aiff", "aif"] } else { &[f.extension()] };
                    dialog = dialog.add_filter(f.name(), exts);
                }
                if let Some(dir) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
        };
        // The extension picks the format; a name without one is a WAV.
        let (path, format) = match SaveFormat::from_path(&path) {
            Some(f) => (path, f),
            None if path.extension().is_none() => (path.with_extension("wav"), SaveFormat::Wav),
            None => return Err(format!("{} is not a format WavCraft saves: use .wav, .aiff or .flac", path.display())),
        };
        let wanted = ty.unwrap_or_else(|| SampleType::for_source(state.source()));
        if !format.accepts(wanted) && (explicit || ty.is_some()) {
            return Err(format!("{} cannot hold {} samples: use 16 or 24 bit", format.name(), wanted.describe()));
        }
        let ty = format.fit(wanted);
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (job_state, job_path, job_progress, ctx) = (state.clone(), path.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(save(&job_state, &job_path, format, ty, &mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.halt_play();
        self.saving = Some(Saving { path, state, ty, format, result: rx, progress, then_close, doc_id: self.doc_id, copy: None });
        Ok(())
    }

    /// File › Save Selection As (`selection`: only the selected range of the enabled channels) and
    /// File › Export › File (the whole document): a new WAV beside whatever else, written by the
    /// save path. The open document keeps its own path, name and unsaved mark. `to` skips the
    /// dialog (the control channel).
    fn save_copy(&mut self, ctx: &egui::Context, selection: bool, to: Option<PathBuf>) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let part = if selection {
            let r = self.selection_range();
            if r.start >= r.end {
                return Err("select some audio first".to_string());
            }
            Part::range(&state, r, editor.channels)
        } else {
            Part::whole(&state)
        };
        let path = match to {
            Some(p) => p,
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let suffix = if selection { "selection" } else { "export" };
                let mut dialog = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(format!("{stem}-{suffix}.wav"));
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
            let _ = tx.send(write_file(&job_state, &job_path, FileKind::Wav(ty), &part, &mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.halt_play();
        let copy = Some(if selection { "selection" } else { "export" });
        self.saving = Some(Saving { path, state, ty, format: SaveFormat::Wav, result: rx, progress, then_close: false, doc_id: self.doc_id, copy });
        Ok(())
    }

    fn finish_saving(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.saving else { return };
        let result = match saving.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the save stopped unexpectedly".to_string()),
        };
        let Some(Saving { path, state, ty, format, then_close, doc_id, copy, .. }) = self.saving.take() else { return };
        if let Some(what) = copy {
            match result {
                Ok(()) => {
                    self.error = None;
                    self.notice =
                        Some(format!("Wrote the {what} to {} ({}); the open file is unchanged", path.display(), FileKind::Wav(ty).describe()));
                }
                Err(e) => self.error = Some(format!("Not written: {e}. {} is unchanged.", path.display())),
            }
            return;
        }
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
                    self.notice = Some(format!("Saved {} ({} {})", path.display(), ty.describe(), format.name()));
                } else {
                    log::info!(target: "wavcraft", "a save of a closed file finished: {}", path.display());
                    self.notice = Some(format!("Saved {} ({} {}), a file closed meanwhile", path.display(), ty.describe(), format.name()));
                }
            }
            Ok(()) => {
                self.saved = Some(state);
                self.doc_path = path.clone();
                if let Some(doc) = self.doc.as_mut() {
                    doc.path = path.clone();
                }
                self.error = None;
                self.notice = Some(format!("Saved {} ({} {})", path.display(), ty.describe(), format.name()));
                if then_close && std::mem::take(&mut self.closing_doc) {
                    self.remove_current();
                } else if then_close {
                    // Not allow_close: another document may still have unsaved changes.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(e) => {
                self.close_all = false;
                self.closing_doc = false;
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
        let name = path_name(&self.doc_path);
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
                    self.close_all = false;
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
                self.close_all = false;
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
                self.poll_transport();
                let busy = self.op_running()
                    || self.inserting.is_some()
                    || self.rebuilding.is_some()
                    || self.converting.is_some()
                    || self.making.is_some()
                    || self.save_all.is_some()
                    || self.integrating.is_some();
                let unsaved = u8::from(self.unsaved());
                let t = &self.transport;
                let playhead = if t.mode == Mode::Stopped { self.selection.start() } else { self.player.position() };
                let tstate = format!(
                    "play={} pos={playhead} loop={} skip={} return={} follow={} autoscroll={} wraps={} output={} view={}-{}",
                    t.mode.name(),
                    u8::from(t.looping),
                    u8::from(t.skip_selection),
                    u8::from(t.return_on_stop),
                    t.follow.name(),
                    u8::from(t.auto_scroll),
                    self.player.wraps(),
                    if self.player.has_output() { "device" } else { "none" },
                    self.view.start.max(0.0) as u64,
                    self.view.end(self.editor_width.max(1.0)).max(0.0) as u64
                );
                match &self.editor {
                    Some(e) => {
                        let doc = e.history.current();
                        format!(
                            "OK frames={} channels={} rate={} selection={} steps={} cursor={} busy={} unsaved={unsaved} {tstate} snap={} snapzero={} enabled={} clipboard={} clip_frames={} time_display={} selection_text={} tool={} right_click={} view={} vzoom={:.2} ampscale={}{} snap_frames={} msel={} undo={} redo={} path={}",
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
                            self.rec.state_fields(),
                            u8::from(self.snap_frames),
                            {
                                let picked: Vec<String> = self.markers_ui.pick.indices(doc.markers()).iter().map(usize::to_string).collect();
                                if picked.is_empty() { "-".to_string() } else { picked.join(",") }
                            },
                            e.history.undo_label().unwrap_or("-"),
                            e.history.redo_label().unwrap_or("-"),
                            self.doc_path.display()
                        )
                    }
                    None => format!(
                        "OK frames=0 channels=0 rate=0 selection=0-0 steps=0 cursor=0 busy={} unsaved=0 {tstate} snap=1 snapzero=0 enabled= clipboard=1 clip_frames=0 time_display={} selection_text=- tool={} right_click={} view=0-0 vzoom=1.00 ampscale={}{} snap_frames={} msel=- undo=- redo=- path=",
                        u8::from(busy),
                        self.time_format.name(),
                        self.tool.name(),
                        if self.settings.right_click_menu { "menu" } else { "extend" },
                        self.settings.amp_scale.name(),
                        self.rec.state_fields(),
                        u8::from(self.snap_frames)
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
            // Test hook: count as busy (open or save running) for <ms>. Fake input only.
            "HOLD_BUSY" if std::env::var_os("WAVCRAFT_FAKE_INPUT").is_some() => match rest.trim().parse::<u64>() {
                Ok(ms) => {
                    self.test_busy_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                    "OK".to_string()
                }
                Err(_) => "ERR HOLD_BUSY <ms>".to_string(),
            },
            // `MARKER rename <i> <name>`, `MARKER move <i> <frame>`, `MARKER select <i> [add]`: <i> is the
            // position in the MARKERS list (from 0).
            "MARKER" => self.control_marker(rest),
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
            "PANELS" => self.panels_report(),
            // `WORKSPACES` lists them; `WORKSPACE save|switch|reset|delete <name>`.
            "WORKSPACES" => self.workspaces_report(),
            "WORKSPACE" => match self.workspace_command(rest) {
                Ok(()) => self.workspaces_report(),
                Err(e) => format!("ERR {e}"),
            },
            // `PANEL move <id> <left|right|bottom>`, `PANEL maximize <id>` (again to restore),
            // `PANEL close <id>`, `PANEL activate <id>`.
            "PANEL" => match self.layout.apply_command(&mut self.panels, rest) {
                Ok(()) => {
                    self.save_panels();
                    self.panels_report()
                }
                Err(e) => format!("ERR {e}"),
            },
            "GESTURE" => self.control_gesture(rest),
            // The step a ruler gesture calls, for scripted checks: `SNAP_FRAME <frame>`.
            "SNAP_FRAME" => match rest.trim().parse::<usize>() {
                Ok(f) => format!("OK frame={}", self.snap_gesture(f)),
                Err(_) => "ERR SNAP_FRAME <frame>".to_string(),
            },
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
                Some(id) => match if matches!(id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
                    self.run_doc_action(id, ctx, false)
                } else {
                    self.run_action(id, ctx)
                } {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                None => format!("ERR no action named {rest:?}"),
            },
            // The session's noise print (f29): `NOISE_PRINT` → `OK frames=<averaged> channels=<n> rate=<hz>`, or `OK none`.
            "NOISE_PRINT" => match wavcraft_core::noise::current() {
                Some(p) => format!("OK frames={} channels={} rate={}", p.frames, p.density.len(), p.sample_rate),
                None => "OK none".to_string(),
            },
            // Press a shortcut: `KEY Ctrl+Z` runs what the table binds it to, as the key would.
            // (Open is refused: it would wait for a file dialog nobody can answer.)
            "KEY" => match self.keymap.resolve_text(rest) {
                Ok(Command::Open) => "ERR file.open opens a dialog: use OPEN <path>".to_string(),
                Ok(command) => match self.run_command(command, ctx) {
                    Ok(changed) => format!("OK command={} selection={} changed={}", command.name(), sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                Err(e) => format!("ERR {e}"),
            },
            "SHORTCUTS" | "SHORTCUT" => self.shortcut_request(cmd, rest),
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
            // The Effects Rack (f31): `RACK` says its state, `RACK set <slot> <effect|none>
            // [key=value ...]`, `RACK power <slot|master> on|off`, `RACK in|out <dB>`,
            // `RACK mix <pct>`, `RACK apply`. Slots count from 1.
            "RACK" if rest.trim() == "apply" => match self.apply_rack(ctx) {
                Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                Err(e) => format!("ERR {e}"),
            },
            "RACK" => self.rack.command(rest),
            // `PRESET save|load|delete|list <effect|rack> [name]`.
            // (The conversion presets' `PRESET list|save <name> | ...` is further down; a second word
            // of `rack`, `effect` or `effect.<id>` makes it this one.)
            "PRESET" if rest.split_whitespace().nth(1).is_some_and(|k| k == "rack" || k == "effect" || k.starts_with("effect.")) => {
                self.rack.preset(rest, &mut self.effect_dialog)
            }
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
            // META get [field] | META set <field> <value>: the metadata of the current step.
            "META" => {
                let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                let meta = &editor.history.current().source().meta;
                match (verb, rest.trim()) {
                    ("get", "") => format!("OK {}", meta.summary()),
                    ("get", field) => match meta.get(field) {
                        Some(v) => format!("OK {}", meta::escape_lines(&v)),
                        None => format!("ERR no metadata field {field:?}"),
                    },
                    ("set", args) => {
                        let (field, value) = args.split_once(' ').unwrap_or((args, ""));
                        match self.apply_metadata(&[(field.to_string(), meta::unescape_lines(value))]) {
                            Ok(changed) => format!("OK changed={}", u8::from(changed)),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                    _ => "ERR META get [field] | META set <field> <value>".to_string(),
                }
            }
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
            // Amplitude statistics of a range (frames), or of the selection / whole file; the
            // numbers come back on one line and the panel and its navigation use them.
            "STATS" => {
                let Some(state) = self.editor.as_ref().map(|e| e.history.current().clone()) else { return "ERR no file is open".to_string() };
                let mut it = rest.split_whitespace().map(str::parse::<u64>);
                let (range, whole) = match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => (a..b, false),
                    (None, _) => self.stats_scope(),
                    _ => return "ERR STATS [<start> <end>], in frames".to_string(),
                };
                self.stats.cancel();
                match wavcraft_core::stats::scan(&state, range, wavcraft_core::stats::DEFAULT_RMS_WINDOW_MS, &mut |_| {}, &AtomicBool::new(false)) {
                    Ok(Some(stats)) => {
                        let line = format!("OK {}", stats.summary());
                        self.stats.measured = Some(crate::statspanel::Measured { state, stats, whole });
                        line
                    }
                    Ok(None) => "ERR cancelled".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
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
                (Ok(i), true)
                    if self.saving.is_none() && self.converting.is_none() && self.rec.going_into_doc().is_none() && self.integrating.is_none() =>
                {
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
            // A new empty document: `NEWDOC rate=48000 ch=2 bits=32` (all optional; bits 16, 24, 32 float).
            "NEWDOC" => {
                let mut d = NewAudio::default();
                let mut bad = None;
                for kv in rest.split_whitespace() {
                    match kv.split_once('=') {
                        Some(("rate", v)) => d.rate = v.parse().unwrap_or(0),
                        Some(("ch", v)) => d.channels = v.parse().unwrap_or(0),
                        Some(("bits", v)) => d.bits = v.parse().unwrap_or(0),
                        _ => bad = Some(kv),
                    }
                }
                match bad {
                    Some(kv) => format!("ERR NEWDOC rate=<hz> ch=<n> bits=<16|24|32>, not {kv:?}"),
                    None => match self.new_audio_file(d.rate, d.channels, d.bits, ctx) {
                        Ok(()) => format!("OK count={} current={}", self.docs.len(true), self.docs.current()),
                        Err(e) => format!("ERR {e}"),
                    },
                }
            }
            "FILES" => self.files_request(rest),
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
            // f26: the bars as numbers; `METERS PUSH <l> <r>` feeds the tap as the audio callback
            // would (linear peaks), for a run without an output device; `METERS RESET` clears the lights.
            "METERS" => {
                let mut words = rest.split_whitespace();
                match words.next() {
                    Some("PUSH") => match (words.next().map(str::parse::<f32>), words.next().map(str::parse::<f32>)) {
                        (Some(Ok(l)), Some(Ok(r))) => self.player.push_levels([l, r]),
                        _ => return "ERR METERS PUSH <left> <right>, linear peaks".to_string(),
                    },
                    Some("RESET") => self.meters.levels.reset_clip(),
                    Some(_) => return "ERR METERS [PUSH <l> <r> | RESET]".to_string(),
                    None => {}
                }
                self.meters.update(self.player.take_levels());
                meters_ui::readings_line(&self.meters.levels)
            }
            // f26: loudness of the selection (the whole file when none), or of `<start> <end>` frames.
            "LOUDNESS" => {
                let Some(e) = &self.editor else { return "ERR LOUDNESS needs a file".to_string() };
                let state = e.history.current().clone();
                let len = state.len_frames();
                let mut it = rest.split_whitespace().map(str::parse::<u64>);
                let (range, scope) = match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => (a..b, "range"),
                    (None, _) if self.selection.is_empty() => (0..len, "file"),
                    (None, _) => (self.selection.start() as u64..self.selection.end() as u64, "selection"),
                    _ => return "ERR LOUDNESS [<start> <end>], in frames".to_string(),
                };
                match wavcraft_core::loudness::measure(&state, range, &AtomicBool::new(false), &mut |_| {}) {
                    Ok(m) => format!("OK scope={scope} {}", m.line()),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // RECORD start [rate=48000 ch=1 bits=24] [max=<s>] [in=<s>|at=<unix s>] | quick | pause | resume | cancel | stop
            "RECORD" => {
                let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
                match verb {
                    "start" => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                        match crate::recording::parse_start_args(args, now) {
                            Ok((f, o)) if o.delay_secs.is_some_and(|d| d > 0) => {
                                match self.rec.arm(f, std::time::Duration::from_secs(o.delay_secs.unwrap_or(0)), o.max_secs) {
                                    Ok(()) => format!("OK armed in={}", o.delay_secs.unwrap_or(0)),
                                    Err(e) => format!("ERR {e}"),
                                }
                            }
                            Ok((f, o)) if o.mode.is_some() => {
                                let mode = o.mode.unwrap_or(RecMode::Insert);
                                match self.start_recording_into(f, mode, o.target.unwrap_or_default(), o.preroll_ms.unwrap_or(DEFAULT_PREROLL_MS)) {
                                    Ok(path) => format!("OK path={}", path.display()),
                                    Err(e) => format!("ERR {e}"),
                                }
                            }
                            Ok((f, o)) => match self.start_recording_max(f, o.max_secs) {
                                Ok(path) => format!("OK path={}", path.display()),
                                Err(e) => format!("ERR {e}"),
                            },
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                    "quick" => match self.quick_record() {
                        Ok(path) => format!("OK path={}", path.display()),
                        Err(e) => format!("ERR {e}"),
                    },
                    "pause" | "resume" => {
                        if !self.rec.is_active() {
                            "ERR not recording".to_string()
                        } else if self.rec.set_paused(verb == "pause") {
                            "OK".to_string()
                        } else {
                            format!("ERR already {}", if verb == "pause" { "paused" } else { "recording" })
                        }
                    }
                    "cancel" if self.rec.disarm() => "OK".to_string(),
                    "cancel" => "ERR nothing is waiting to start".to_string(),
                    "stop" => match self.finish_recording(true, ctx) {
                        Some(t) if t.error.is_none() => format!("OK frames={} dropped={} path={}", t.frames, t.dropped_frames, t.path.display()),
                        Some(t) => format!("ERR the take stopped early: {} (kept at {})", t.error.unwrap_or_default(), t.path.display()),
                        None => "ERR not recording".to_string(),
                    },
                    _ => "ERR RECORD start [rate=48000 ch=1 bits=24] [max=<s>] [in=<s>|at=<unix s>] | quick | pause | resume | cancel | stop"
                        .to_string(),
                }
            }
            // f33: `MATCH [target=<LUFS>] [tp=<dBTP>] [limit=on|off] [tol=<LU>] [lookahead=<ms>]
            // [release=<ms>] [preset=podcast|broadcast]`: Match Loudness on the whole file, one step.
            "MATCH" => {
                let Some(e) = &mut self.editor else { return "ERR MATCH needs a file".to_string() };
                let mut t = Target::PODCAST;
                for word in rest.split_whitespace() {
                    let (k, v) = word.split_once('=').unwrap_or((word, ""));
                    let num = v.parse::<f64>();
                    match (k, num) {
                        ("preset", _) => match v {
                            "podcast" => t = Target::PODCAST,
                            "broadcast" => t = Target::BROADCAST,
                            _ => return "ERR MATCH preset=podcast|broadcast".to_string(),
                        },
                        ("target", Ok(n)) => t.lufs = n,
                        ("tp", Ok(n)) => t.max_true_peak_dbtp = n,
                        ("tol", Ok(n)) => t.tolerance_lu = n,
                        ("lookahead", Ok(n)) => t.look_ahead_ms = n,
                        ("release", Ok(n)) => t.release_ms = n,
                        ("limit", _) if v == "on" || v == "off" => t.limit = v == "on",
                        _ => return format!("ERR MATCH: cannot read {word:?}; keys target tp tol limit lookahead release preset"),
                    }
                }
                let state = e.history.current().clone();
                match wavcraft_core::match_loudness::run(&state, &t, None, &AtomicBool::new(false), &mut |_| {}) {
                    Ok(r) => {
                        let line = format!(
                            "OK gain={:.2} limited={} on_target={} before={:.2} after={:.2} true_peak={:.2} changed={}",
                            r.gain_db,
                            u8::from(r.limited),
                            u8::from(r.on_target),
                            r.before.integrated_lufs,
                            r.after.integrated_lufs,
                            r.after.true_peak_dbtp,
                            u8::from(!r.edit.is_noop())
                        );
                        if !r.edit.is_noop() {
                            e.history.push_edit(r.edit);
                            self.notice = Some(r.message);
                            self.rebuild(ctx);
                        }
                        line
                    }
                    Err(err) => format!("ERR {err}"),
                }
            }
            "SAVE" => {
                // `SAVE <path> [bits=16|24|32] [float=0|1]`: the options end the line.
                match parse_save_args(rest) {
                    Err(e) => format!("ERR {e}"),
                    Ok((path, ty)) => {
                        let to = (!path.is_empty()).then(|| PathBuf::from(path));
                        match self.save_with(ctx, to.is_some(), to, false, ty.or(self.save_type), ty.is_some()) {
                            Ok(()) => "OK".to_string(),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                }
            }
            "SAVE_SELECTION" | "EXPORT" if !rest.is_empty() => match self.save_copy(ctx, cmd == "SAVE_SELECTION", Some(PathBuf::from(rest))) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {e}"),
            },
            "SAVE_SELECTION" | "EXPORT" => format!("ERR {cmd} <path>"),
            // A plain QUIT refuses with unsaved changes; `QUIT discard` closes anyway.
            "QUIT" if self.any_unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or QUIT discard".to_string(),
            "QUIT" => {
                self.finish_recording(false, ctx);
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                "OK".to_string()
            }
            _ => format!("ERR unknown request {cmd:?}"),
        }
    }

    /// The `MARKER` control verb (see `control_request`).
    fn control_marker(&mut self, rest: &str) -> String {
        const USAGE: &str = "ERR MARKER rename <i> <name> | move <i> <frame> | select <i> [add]";
        if self.saving.is_some() {
            return "ERR wait for the save to finish".to_string();
        }
        let mut it = rest.splitn(3, ' ');
        let (verb, index, arg) = (it.next().unwrap_or(""), it.next().and_then(|i| i.parse::<usize>().ok()), it.next().unwrap_or("").trim());
        let Some(editor) = self.editor.as_mut() else { return "ERR no file is open".to_string() };
        let markers = editor.history.current().markers().clone();
        let Some(index) = index else { return USAGE.to_string() };
        let Some(id) = markers.id_at(index) else { return format!("ERR no marker {index} (there are {})", markers.len()) };
        match verb {
            "rename" if !arg.is_empty() => {
                if editor.rename_marker(id, arg) {
                    "OK".to_string()
                } else {
                    "ERR the name is empty or unchanged".to_string()
                }
            }
            "move" => match arg.parse::<u64>() {
                Ok(frame) if editor.move_marker(id, frame) => "OK".to_string(),
                Ok(_) => "ERR the marker would not move (same place, or another marker is there)".to_string(),
                Err(_) => USAGE.to_string(),
            },
            "select" if arg.is_empty() || arg == "add" => {
                if let Some(span) = self.markers_ui.click(&markers, index, false, arg == "add") {
                    self.selection = Selection { anchor: span.start as usize, head: span.end as usize };
                }
                format!("OK selection={}-{}", self.selection.start(), self.selection.end())
            }
            _ => USAGE.to_string(),
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
            let keys = self.keymap.menu_text(Command::Action(a.id), mac);
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        ui.separator();
        for a in TABLE.iter().filter(|a| a.name.starts_with("marker.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            let keys = self.keymap.menu_text(Command::Action(a.id), mac);
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
        if ui.button("Keyboard Shortcuts…").clicked() {
            self.shortcut_dialog = Some(ShortcutDialog::default());
        }
    }

    /// Apply a shortcut change and save the key file. `Err` says what went wrong (a conflict, or
    /// the file not saved: the change still holds until the app closes).
    fn change_shortcuts(&mut self, change: Change) -> Result<String, String> {
        let outcome = match change {
            Change::Assign { command, chord, reassign: false } => self
                .keymap
                .assign(command, chord)
                .map(|()| format!("{} is {}", command.name(), chord.to_ascii()))
                .map_err(|c| format!("conflict: {} is already {}", c.chord.to_ascii(), c.holder.name()))?,
            Change::Assign { command, chord, reassign: true } => match self.keymap.assign_reassign(command, chord) {
                Some(old) => format!("{} is {} (taken from {})", command.name(), chord.to_ascii(), old.name()),
                None => format!("{} is {}", command.name(), chord.to_ascii()),
            },
            Change::Clear(command) => {
                self.keymap.clear(command);
                format!("{} has no chord", command.name())
            }
            Change::Reset => {
                self.keymap.reset();
                "default shortcuts restored".to_string()
            }
        };
        if let Err(e) = self.keymap.check() {
            log::error!(target: "wavcraft", "keyboard shortcuts conflict: {e}");
        }
        if let Some(path) = &self.shortcuts_path
            && let Err(e) = wavcraft_core::shortcuts::save(path, &self.keymap)
        {
            return Err(format!("changed, but the key file {} could not be saved: {e}", path.display()));
        }
        Ok(outcome)
    }

    /// `SHORTCUTS` and `SHORTCUT set|clear|reset` on the control channel (see `control.rs`).
    fn shortcut_request(&mut self, verb: &str, rest: &str) -> String {
        if verb == "SHORTCUTS" {
            let mut out = String::from("OK");
            for command in Command::all() {
                let chords: Vec<String> = self.keymap.chords(command).map(|c| c.to_ascii()).collect();
                out.push_str(&format!(" {}={}", command.name(), if chords.is_empty() { "-".to_string() } else { chords.join("|") }));
            }
            return out;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        let find = |name: &str| Command::from_name(name).ok_or_else(|| format!("no command named {name:?}"));
        let change = match words.as_slice() {
            ["reset"] => Ok(Change::Reset),
            ["clear", name] => find(name).map(Change::Clear),
            ["set", name, chord] | ["set", name, chord, "reassign"] => {
                find(name).and_then(|command| Shortcut::parse(chord).map(|chord| Change::Assign { command, chord, reassign: words.len() == 4 }))
            }
            _ => Err("SHORTCUT set <command> <chord> [reassign] | clear <command> | reset".to_string()),
        };
        match change.and_then(|c| self.change_shortcuts(c)) {
            Ok(m) => format!("OK {m}"),
            Err(e) => format!("ERR {e}"),
        }
    }

    /// The Effects menu: the one-shot effects from the action table (`effect.*`), run on the
    /// selection (the whole file with none). Amplify and Normalize use the numbers set by `PARAM`.
    fn effects_menu(&mut self, ui: &mut egui::Ui) {
        let sel = self.selection_range();
        let mut chosen = None;
        {
            // Capture Noise Print belongs with Noise Reduction: it learns what that effect removes.
            let a = actions::action(ActionId::CaptureNoisePrint);
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            let keys = self.keymap.menu_text(Command::Action(a.id), cfg!(target_os = "macos"));
            if ui.add_enabled(enabled, egui::Button::new(a.menu).shortcut_text(keys)).clicked() {
                chosen = Some((a.id, false));
            }
            ui.separator();
        }
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
        ui.separator();
        if ui.button("Effects Rack…").clicked() {
            self.rack.open = !self.rack.open;
            ui.close();
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
                let open = format!("Open…  {}", self.keymap.menu_text(Command::Open, cfg!(target_os = "macos")));
                if ui.button(open).clicked() {
                    self.pick_file(ui.ctx());
                }
                ui.menu_button("New", |ui| {
                    if ui.button("Audio File…").clicked() {
                        self.new_audio = Some(NewAudio::default());
                        ui.close();
                    }
                });
                let mac = cfg!(target_os = "macos");
                let has_doc = self.editor.is_some() && self.saving.is_none();
                let (save, save_as) = (self.keymap.menu_text(Command::Save, mac), self.keymap.menu_text(Command::SaveAs, mac));
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
                let busy = self.saving.is_some() || self.making.is_some() || self.save_all.is_some();
                if ui.add_enabled(self.editor.is_some() && !busy, egui::Button::new("Save All")).clicked()
                    && let Err(e) = self.save_all_docs(true)
                {
                    self.error = Some(e);
                }
                if ui.add_enabled(self.editor.is_some() && !busy, egui::Button::new("Close All")).clicked()
                    && let Err(e) = self.close_all_docs(true)
                {
                    self.error = Some(e);
                }
                let idle = !self.rec.is_active() && self.opening.is_none() && self.saving.is_none();
                if ui.add_enabled(idle, egui::Button::new("New Recording…")).clicked() {
                    self.rec.dialog = Some(self.rec.last_format());
                }
                if ui.add_enabled(idle, egui::Button::new("Record Now").shortcut_text("Shift+Space")).clicked() {
                    let _ = self.quick_record();
                }
                ui.menu_button("Sample Type on Save", |ui| {
                    ui.radio_value(&mut self.save_type, None, "Keep the file's own");
                    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
                        ui.radio_value(&mut self.save_type, Some(ty), ty.describe());
                    }
                    ui.label("FLAC takes 16 or 24 bit; it uses 24 bit for the others.");
                });
                let has_sel = has_doc && self.selection.start() < self.selection.end();
                if ui.add_enabled(has_sel, egui::Button::new("Save Selection As…")).clicked()
                    && let Err(e) = self.save_copy(ui.ctx(), true, None)
                {
                    self.error = Some(e);
                }
                ui.menu_button("Export", |ui| {
                    if ui.add_enabled(has_doc, egui::Button::new("File…")).clicked()
                        && let Err(e) = self.save_copy(ui.ctx(), false, None)
                    {
                        self.error = Some(e);
                    }
                });
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
                ui.separator();
                ui.menu_button("Workspace", |ui| self.workspace_menu(ui));
                if ui.button("Reset Panel Layout").clicked() {
                    self.layout = Layout::default();
                    self.save_panels();
                    ui.close();
                }
                ui.add_enabled_ui(self.editor.is_some(), |ui| ui.checkbox(&mut self.stats.show, "Amplitude Statistics"));
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
                    let keys = self.keymap.menu_text(Command::Action(a.id), mac);
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
                ui.checkbox(&mut self.meters.show_levels, "Levels");
                ui.checkbox(&mut self.meters.show_stats, "Amplitude/Loudness Statistics");
                ui.checkbox(&mut self.matcher.show, "Match Loudness");
                ui.checkbox(&mut self.show_metadata, "Metadata panel");
                ui.checkbox(&mut self.markers_ui.show, "Markers panel");
                ui.checkbox(&mut self.show_files, "Files panel");
            });
            let label = if self.transport.mode == Mode::Playing { "■ Stop" } else { "▶ Play" };
            if ui.add_enabled(!self.rec.is_active(), egui::Button::new(label)).on_hover_text("Space").clicked() {
                self.toggle_play();
            }
            if self.rec.is_active() {
                if ui.button("■ Stop Recording").clicked() {
                    self.finish_recording(true, ui.ctx());
                }
                let label = if self.rec.is_paused() { "● Resume" } else { "❚❚ Pause" };
                if ui.button(label).on_hover_text("Space").clicked() {
                    self.rec.set_paused(!self.rec.is_paused());
                }
            } else if self.rec.is_armed() {
                if ui.button("Cancel Timed Recording").clicked() {
                    self.rec.disarm();
                }
            } else if ui
                .add_enabled(self.opening.is_none() && self.saving.is_none(), egui::Button::new("● Record"))
                .on_hover_text("New Recording (Shift+Space records at once with the last settings)")
                .clicked()
            {
                self.rec.dialog = Some(self.rec.last_format());
            }
        });
    }

    fn status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some((text, warn)) = self.rec.status_text() {
                ui.colored_label(if warn { CURSOR } else { Color32::from_rgb(230, 230, 230) }, text);
            } else if let Some(o) = &self.opening {
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
            } else if let Some(m) = &self.making {
                ui.spinner();
                ui.label(format!("{}… {}%", m.what, m.progress.load(Ordering::Relaxed) / 10));
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
            } else if self.editor.is_some() {
                ui.label(format!("{} (no audio yet): paste into it, or File › Save As once it has some", path_name(&self.doc_path)));
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
    /// Whether an editor display (waveform, spectral) is drawn: shown, and not hidden by another
    /// panel being maximised.
    fn editor_shows(&self, id: &str) -> bool {
        match self.layout.maximized(&self.panels) {
            Some(m) if !panels::is_movable(m) => m == id,
            Some(_) => false,
            None => self.panels.is_shown(id),
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
            if (shift || scroll.x != 0.0) && self.transport.mode != Mode::Stopped && (scroll.x != 0.0 || scroll.y != 0.0) {
                // Moving the view by hand during playback turns auto-scroll off.
                self.transport.auto_scroll = false;
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
        // Auto-scroll: the view follows the playhead (not while the pointer is busy in the editor).
        if self.transport.mode == Mode::Playing && self.transport.auto_scroll && !response.is_pointer_button_down_on() {
            transport::follow_view(self.transport.follow, &mut self.view, self.player.position() as f64, width, frames);
        }
        self.view.clamp(width, frames);

        // Click places the cursor, drag selects; near a selection edge a drag adjusts it,
        // Shift-click extends, double-click selects what is on screen, triple-click everything.
        // Drags and clicks snap (Editor::snap) to the ruler, the selection's edges and, when on,
        // zero crossings.
        let fpp = self.view.frames_per_px;
        let snap =
            |editor: &Option<Editor>, frame: usize, targets: &[u64]| editor.as_ref().map_or(frame, |e| e.snap(frame as u64, fpp, targets) as usize);
        let (snap_fmt, snap_on) = (self.time_format, self.snap_frames);
        let frame_at = |view: &Viewport, pos: Pos2| {
            let frame = view.frame_at(pos.x - rect.left()).clamp(0.0, frames as f64).round() as usize;
            if snap_on { snap_fmt.snap_to_frame(frame, rate, frames) } else { frame }
        };
        let edge_at = |sel: &Selection, view: &Viewport, pos: Pos2| sel.edge_near(view, pos.x - rect.left(), GRAB_PX);
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some(pos) = response.hover_pos().or(response.interact_pointer_pos())
            && edge_at(&self.selection, &self.view, pos).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let ruler_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        if response.drag_started()
            && let Some(e) = &self.editor
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let markers = e.history.current().markers()
            && let Some(id) = self.markers_ui.handle_at(markers, &self.view, ruler_rect, origin)
        {
            // A marker's flag was grabbed: the drag moves it (one step, when released).
            let grabbed = frame_at(&self.view, origin) as u64;
            self.markers_ui.start_drag(markers, id, grabbed);
        }
        if self.markers_ui.drag.is_some() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.markers_ui.drag_to(frame_at(&self.view, pos) as u64);
            }
            if response.drag_stopped()
                && let (Some(d), Some(e)) = (self.markers_ui.drag.take(), self.editor.as_mut())
                && e.move_marker(d.id, d.start)
                && let Some(i) = e.history.current().markers().index_of(d.id)
            {
                self.markers_ui.pick.click(e.history.current().markers(), i, false, false);
            }
        } else if response.drag_started_by(egui::PointerButton::Primary)
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
            let shown = self.markers_ui.shown(e.history.current().markers(), frames as u64);
            draw_markers(&painter, ruler, &self.view, &shown, &self.markers_ui.pick.ids);
            if response.hover_pos().is_some_and(|p| self.markers_ui.handle_at(&shown, &self.view, ruler, p).is_some()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }

        let (wave_area, spectrum_area) = match (self.editor_shows("waveform"), self.editor_shows("spectral")) {
            (true, true) => {
                let split = body.top() + body.height() * 0.5;
                (Some(Rect::from_min_max(body.min, pos2(body.right(), split))), Some(Rect::from_min_max(pos2(body.left(), split), body.max)))
            }
            (true, false) => (Some(body), None),
            (false, true) => (None, Some(body)),
            (false, false) => (None, None),
        };

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if wave_area.is_some_and(|a| a.contains(pos)) {
                self.hover_next = Some("waveform");
            } else if spectrum_area.is_some_and(|a| a.contains(pos)) {
                self.hover_next = Some("spectral");
            }
        }

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
        }
        if self.transport.mode != Mode::Stopped {
            // The playhead: a heavier white line from a marker on the ruler, with its time.
            let at = self.player.position();
            let x = rect.left() + self.view.x_of(at as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, ruler.top()), pos2(x, rect.bottom())], Stroke::new(2.0, PLAYHEAD));
                let top = ruler.top();
                painter.add(egui::Shape::convex_polygon(vec![pos2(x - 6.0, top), pos2(x + 6.0, top), pos2(x, top + 10.0)], PLAYHEAD, Stroke::NONE));
                let galley = painter.layout_no_wrap(format_time(at, rate), egui::FontId::monospace(11.0), PLAYHEAD);
                let label_at = pos2((x + 9.0).min(rect.right() - galley.size().x - 4.0), top + 1.0);
                painter.rect_filled(Rect::from_min_size(label_at, galley.size()).expand(2.0), 2.0, BACKGROUND);
                painter.galley(label_at, galley, PLAYHEAD);
            }
        }
        if self.transport.mode == Mode::Playing {
            ui.ctx().request_repaint();
        }
        if let Some(ch) = toggle {
            let _ = self.run_action(ActionId::ToggleChannel(ch as u8), ui.ctx());
        }
        if let Some(scale) = chosen_scale {
            self.set_amp_scale(scale);
        }
    }

    /// The Transport panel: Play, Pause, Stop, Loop, go to start / end, the marker buttons (off
    /// until markers exist), the options and the current time.
    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        let mut chosen: Option<ActionId> = None;
        let rate = self.doc.as_ref().map_or(0, Document::sample_rate);
        let enabled = self.doc.is_some() && self.saving.is_none();
        let t = self.transport;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let mut button = |ui: &mut egui::Ui, label: &str, tip: &str, id: ActionId, on: bool| {
                    if ui.add(egui::Button::new(label).selected(on)).on_hover_text(tip).clicked() {
                        chosen = Some(id);
                    }
                };
                button(ui, "|◀ Start", "Go to start (Home)", ActionId::CursorToStart, false);
                ui.add_enabled(false, egui::Button::new("◀ Marker")).on_disabled_hover_text("Previous marker (no markers yet)");
                button(ui, "▶ Play", "Play (Space)", ActionId::Play, t.mode == Mode::Playing);
                button(ui, "❚❚ Pause", "Pause; again to resume", ActionId::Pause, t.mode == Mode::Paused);
                button(ui, "■ Stop", "Stop", ActionId::Stop, false);
                ui.add_enabled(false, egui::Button::new("Marker ▶")).on_disabled_hover_text("Next marker (no markers yet)");
                button(ui, "End ▶|", "Go to end (End)", ActionId::CursorToEnd, false);
                ui.separator();
                button(ui, "Loop", "Repeat the selection, or the view with nothing selected", ActionId::ToggleLoop, t.looping);
                button(
                    ui,
                    "Skip Selection",
                    "Play around the selection, skipping it, to hear a cut",
                    ActionId::ToggleSkipSelection,
                    t.skip_selection,
                );
                button(
                    ui,
                    "Return on Stop",
                    "Stop puts the cursor back where play started (Shift+X)",
                    ActionId::ToggleReturnOnStop,
                    t.return_on_stop,
                );
                button(
                    ui,
                    "Auto-Scroll",
                    "The view follows the playhead; moving it by hand turns this off",
                    ActionId::ToggleAutoScroll,
                    t.auto_scroll,
                );
                button(
                    ui,
                    if t.follow == Follow::Page { "Pagewise" } else { "Centered" },
                    "How auto-scroll follows: click to switch",
                    if t.follow == Follow::Page { ActionId::FollowCenter } else { ActionId::FollowPage },
                    false,
                );
            });
            let frame = if t.mode == Mode::Stopped { self.selection.start() } else { self.player.position() };
            ui.separator();
            ui.monospace(format_time(frame, rate.max(1)));
            ui.label(t.mode.name());
        });
        if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
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
        self.meters.update(self.player.take_levels());
        self.finish_match(ctx);
        if self.meters.animating() || self.matcher.animating() || self.player.is_playing() {
            ctx.request_repaint();
        }
        self.finish_integration(ctx);
        if self.rec.punch_waiting() {
            let at = self.player.is_playing().then(|| self.player.position());
            self.rec.punch_tick(at);
            ctx.request_repaint_after(std::time::Duration::from_millis(4));
        }
        if let Some(why) = self.rec.refresh() {
            log::warn!(target: "wavcraft", "recording: {why}");
            self.finish_recording(true, ctx);
        }
        self.poll_transport();
        self.finish_making(ctx);
        self.pump_save_all(ctx);
        self.pump_close_all();
        if self.rec.max_reached() {
            log::info!(target: "wavcraft", "recording: the time limit is reached, stopping");
            self.finish_recording(true, ctx);
        }
        if let Some((fmt, max)) = self.rec.due() {
            self.start_timed(fmt, max);
        }
        if self.rec.is_active() || self.rec.is_armed() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.stats.poll();
        while let Some(req) = self.control.as_ref().and_then(|rx| rx.try_recv().ok()) {
            let answer = self.control_request(&req.line, ctx);
            let _ = req.reply.send(answer);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.rec.is_active() {
            // Closing the window ends the take cleanly; a kill is covered by the header.
            if ctx.input(|i| i.viewport().close_requested()) {
                self.finish_recording(false, &ctx);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        self.close_prompt(&ctx);
        if let Some(fmt) = self.rec.dialog(&ctx, self.editor.is_some()) {
            // Errors show in the status bar.
            match self.rec.dialog_mode {
                Some(mode) if self.editor.is_some() => {
                    let _ = self.start_recording_into(fmt, mode, self.rec.dialog_target, DEFAULT_PREROLL_MS);
                }
                _ => {
                    let _ = self.start_recording(fmt);
                }
            }
        }
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
        // ` maximises the panel under the pointer (and restores); Ctrl+W closes it.
        let typing = ctx.egui_wants_keyboard_input();
        let (grave, close) =
            ctx.input_mut(|i| (!typing && i.consume_key(egui::Modifiers::NONE, Key::Backtick), i.consume_key(egui::Modifiers::COMMAND, Key::W)));
        if grave {
            if self.layout.maximized(&self.panels).is_some() {
                self.layout.restore();
            } else if let Some(id) = self.hover {
                self.layout.toggle_maximize(id);
            }
        }
        if close && let Some(id) = self.hover {
            self.close_panel(id);
        }
        // While the shortcuts dialog waits for a chord, or its search box has the keyboard, keys
        // are not commands.
        let keys_for_dialog =
            self.shortcut_dialog.as_ref().is_some_and(|d| d.capturing()) || (self.shortcut_dialog.is_some() && ctx.egui_wants_keyboard_input());
        let pressed = if keys_for_dialog { Vec::new() } else { pressed_commands(&ctx, &self.keymap) };
        for command in pressed {
            if let Err(e) = self.run_command(command, &ctx) {
                // A key that can't act now (no file open, a save running) is not an error to show,
                // except a failed save, which the old key handler reported.
                if matches!(command, Command::Save | Command::SaveAs) {
                    self.error = Some(e);
                }
            }
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
        let (space, shift_space, open, dropped, hovering, dropped_without_path) = ctx.input(|i| {
            let space = i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Space, pressed: true, repeat: false, .. }));
            (
                space && !i.modifiers.shift,
                space && i.modifiers.shift,
                i.modifiers.command && i.key_pressed(Key::O),
                dropped_path(&i.raw.dropped_files),
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty() && dropped_path(&i.raw.dropped_files).is_none(),
            )
        });
        // Not `key_pressed` above: that also counts auto-repeat, so a long press would toggle again.
        if shift_space && !ctx.egui_wants_keyboard_input() {
            // Shift+Space: record now (C031); while recording it stops the take.
            if self.rec.is_active() {
                self.finish_recording(true, &ctx);
            } else if self.rec.is_armed() {
                self.rec.disarm();
            } else {
                let _ = self.quick_record();
            }
        } else if space && self.rec.is_active() {
            // Space while recording pauses and resumes the take (C029).
            self.rec.set_paused(!self.rec.is_paused());
        } else if space {
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
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()));
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
        let title = if self.editor.is_some() { format!("{star}{} — WavCraft", path_name(&self.doc_path)) } else { "WavCraft".to_string() };
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
        if self.show_files {
            egui::Panel::left("files").default_size(420.0).show(ui, |ui| self.files_panel(ui));
        }
        if self.show_sel_view_panel {
            egui::Panel::bottom("selection_view").show(ui, |ui| self.selection_view_panel(ui));
        }
        if self.show_time_panel {
            egui::Panel::bottom("time_panel").show(ui, |ui| self.time_panel(ui));
        }
        if self.meters.show_levels {
            egui::Panel::bottom("levels").show(ui, |ui| self.meters.panel(ui));
        }
        if self.meters.show_stats {
            let target = self.editor.as_ref().map(|e| {
                let state = e.history.current().clone();
                let (range, scope) = if self.selection.is_empty() {
                    (0..state.len_frames(), "the whole file")
                } else {
                    (self.selection.start() as u64..self.selection.end() as u64, "the selection")
                };
                (state, range, scope.to_string())
            });
            self.meters.stats_window(&ctx, target);
        }
        if self.matcher.show {
            let file = self.editor.as_ref().map(|e| {
                let name = self.doc_path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned());
                (name, e.history.current().clone())
            });
            self.matcher.window(&ctx, file);
        }
        egui::Panel::bottom("transport").show(ui, |ui| self.transport_panel(ui));
        if self.panels.is_shown("history") {
            egui::Panel::right("history").default_size(220.0).show(ui, |ui| self.history_panel(ui));
        }
        if self.markers_ui.show {
            egui::Panel::right("markers").default_size(380.0).show(ui, |ui| self.markers_panel(ui));
        }
        if self.show_metadata {
            egui::Panel::right("metadata").default_size(260.0).show(ui, |ui| self.metadata_panel(ui));
        }
        self.tab_rects.clear();
        self.zone_rects.clear();
        self.hover_next = None;
        let maximized = self.layout.maximized(&self.panels).filter(|id| panels::is_movable(id)).map(String::from);
        if let Some(id) = maximized {
            // One docked panel fills the window; the tab strips are not drawn.
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(panels::by_id(&id).map_or("", |p| p.title));
                    if ui.small_button("Restore (`)").clicked() {
                        self.layout.restore();
                    }
                });
                ui.separator();
                self.panel_body(&id, ui);
                if ui.rect_contains_pointer(ui.max_rect()) {
                    self.hover_next = panels::by_id(&id).map(|p| p.id);
                }
            });
        } else {
            if self.layout.maximized(&self.panels).is_none() {
                self.dock_zone(ui, DockSide::Left);
                self.dock_zone(ui, DockSide::Right);
                self.dock_zone(ui, DockSide::Bottom);
            }
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
        }
        self.floating_windows(&ctx);
        self.insert_silence_dialog(&ctx);
        self.convert_dialogs(&ctx);
        self.effect_dialog_ui(&ctx);
        self.mix_paste_dialog(&ctx);
        self.preferences_dialog(&ctx);
        self.new_audio_dialog(&ctx);
        self.rack_ui(&ctx);
        if let Some(mut dialog) = self.shortcut_dialog.take() {
            let (open, change) = dialog.show(&ctx, &self.keymap, cfg!(target_os = "macos"));
            if let Some(change) = change
                && let Err(e) = self.change_shortcuts(change)
            {
                self.error = Some(e);
            }
            if open {
                self.shortcut_dialog = Some(dialog);
            }
        }
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
        {
            let current = self.editor.as_ref().map(|e| e.history.current().clone());
            let scope = (!self.selection.is_empty()).then(|| self.selection_range());
            let (scan, nav) = self.stats.ui(&ctx, current.as_ref(), scope);
            if scan {
                self.start_stats(&ctx);
            }
            if let Some(nav) = nav {
                match self.stats_nav(nav) {
                    Ok(()) => self.error = None,
                    Err(e) => self.error = Some(format!("Amplitude Statistics: {e}")),
                }
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
        self.dock_drag(&ctx);
        self.hover = self.hover_next;
        if self.layout_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.save_panels();
        }
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

/// The commands whose chords were pressed this frame. Chords with more modifiers are taken first,
/// so Cmd+Shift+Z (Redo) is not read as Cmd+Z (Undo).
fn pressed_commands(ctx: &egui::Context, keymap: &Keymap) -> Vec<Command> {
    // Open, Save, Save As and Play/Stop are read by the window's own handlers below (they also
    // know about recording, the drop target and text fields), so the keymap's rows for them only
    // serve the menus and `KEY`.
    let mut rows: Vec<(Command, Shortcut)> = keymap
        .bindings()
        .iter()
        .filter(|b| !matches!(b.command, Command::Open | Command::Save | Command::SaveAs | Command::PlayStop))
        .map(|b| (b.command, b.chord))
        .collect();
    rows.sort_by_key(|(_, k)| std::cmp::Reverse(u8::from(k.cmd) + u8::from(k.shift) + u8::from(k.alt)));
    ctx.input_mut(|i| {
        rows.iter()
            .filter_map(|(command, k)| {
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
                // Play/Stop must not toggle again on auto-repeat during a long press.
                let fresh = *command != Command::PlayStop
                    || i.events.iter().any(|e| matches!(e, egui::Event::Key { key: pressed, pressed: true, repeat: false, .. } if *pressed == key));
                (i.consume_key(mods, key) && fresh).then_some(*command)
            })
            .collect()
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
fn draw_markers(painter: &egui::Painter, rect: Rect, view: &Viewport, markers: &wavcraft_core::Markers, picked: &[u64]) {
    let painter = painter.with_clip_rect(rect);
    for m in markers.iter() {
        let color = if picked.contains(&m.id) { Color32::from_rgb(255, 255, 255) } else { Color32::from_rgb(240, 190, 60) };
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
        TimeFormat::Decimal | TimeFormat::Compact => ruler_step(min_seconds),
        _ => {
            // SMPTE: whole frames below a second.
            const FRAMES: [f64; 5] = [1.0, 2.0, 5.0, 10.0, 15.0];
            let frame = fmt.smpte().map_or(1.0 / 30.0, |r| r.frame_seconds());
            FRAMES.iter().map(|f| f * frame).find(|s| *s >= min_seconds).unwrap_or_else(|| ruler_step(min_seconds.max(1.0)))
        }
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

/// `SAVE`'s arguments: a path (may hold spaces) then `bits=<16|24|32>` and `float=<0|1>` in any
/// order. Returns the path and the sample type they name, if any.
fn parse_save_args(rest: &str) -> Result<(&str, Option<SampleType>), String> {
    let (mut path, mut bits, mut float) = (rest.trim(), None, None);
    while let Some((head, last)) = path.rsplit_once(char::is_whitespace).or(if path.contains('=') { Some(("", path)) } else { None }) {
        if let Some(v) = last.strip_prefix("bits=") {
            bits = Some(v.parse::<u32>().map_err(|_| format!("bits=<16|24|32>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("float=") {
            float = Some(match v {
                "0" => false,
                "1" => true,
                _ => return Err(format!("float=<0|1>, not {v}")),
            });
        } else {
            break;
        }
        path = head.trim();
    }
    let ty = match (bits, float) {
        (None, None) => None,
        (Some(16), None | Some(false)) => Some(SampleType::Int16),
        (Some(24), None | Some(false)) => Some(SampleType::Int24),
        (Some(32), None | Some(false)) => Some(SampleType::Int32),
        (None | Some(32), Some(true)) => Some(SampleType::Float32),
        (None, Some(false)) => None,
        (b, f) => return Err(format!("no sample type for bits={b:?} float={f:?}: 16, 24, 32 bit integer or float=1 (32 bit)")),
    };
    Ok((path, ty))
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
        assert!((ruler_step_for(TimeFormat::Smpte24, 0.02, 48_000) - 1.0 / 24.0).abs() < 1e-12);
        assert!((ruler_step_for(TimeFormat::Smpte5994, 0.01, 48_000) - 1001.0 / 60_000.0).abs() < 1e-12);
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

    #[test]
    fn save_arguments_name_the_path_and_the_sample_type() {
        assert_eq!(parse_save_args("/a b/c.wav"), Ok(("/a b/c.wav", None)));
        assert_eq!(parse_save_args("/a b/c.wav bits=24 float=0"), Ok(("/a b/c.wav", Some(SampleType::Int24))));
        assert_eq!(parse_save_args("x.aiff float=1"), Ok(("x.aiff", Some(SampleType::Float32))));
        assert_eq!(parse_save_args("x.aiff bits=32 float=1"), Ok(("x.aiff", Some(SampleType::Float32))));
        assert_eq!(parse_save_args("x.flac bits=16"), Ok(("x.flac", Some(SampleType::Int16))));
        assert_eq!(parse_save_args(""), Ok(("", None)));
        assert!(parse_save_args("x.wav bits=20").is_err());
        assert!(parse_save_args("x.wav bits=24 float=1").is_err());
        assert!(parse_save_args("x.wav float=2").is_err());
    }
}
