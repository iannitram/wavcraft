//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::presets::{ExportChoice, ExportPresets};
use wavcraft_core::save::{AiffKind, SampleType, SaveFormat, SizeEstimate, estimate_size, save_aiff_kind};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::{DocState, Document, History, OpenError, Selection, SpectrogramParams, Viewport, format_time};

use crate::control;
use crate::player::Player;
use crate::spectrum::{Job, Renderer};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const WAVE: Color32 = Color32::from_rgb(64, 200, 140);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const RULER_HEIGHT: f32 = 22.0;

/// A file being opened on a worker thread.
struct Opening {
    path: PathBuf,
    result: Receiver<Result<Document, String>>,
    /// Set to stop it (another file was opened meanwhile).
    cancel: Arc<AtomicBool>,
    /// How far the streaming has got, in thousandths.
    progress: Arc<AtomicU32>,
}

/// What a shown size estimate was worked out for: the choice and the document step.
type SizeKey = (SaveFormat, SampleType, AiffKind, usize);

/// A save running on a worker thread.
struct Saving {
    path: PathBuf,
    /// The step being saved: it becomes the saved state when the save succeeds.
    state: Arc<DocState>,
    ty: SampleType,
    format: SaveFormat,
    aiff: AiffKind,
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
    /// File › Sample Type on Save: the type every save writes, or `None` to keep the file's own.
    save_type: Option<SampleType>,
    /// How AIFF files are written (the export dialog, an export preset, or `SAVE … aiff=`).
    aiff_kind: AiffKind,
    /// The Save As / Export dialog is up.
    export_open: bool,
    /// What the export dialog (and `SAVEOPTS`) has chosen.
    export_choice: ExportChoice,
    /// The named choices, and the file they are kept in.
    export_presets: ExportPresets,
    export_presets_path: Option<PathBuf>,
    /// The name typed in the dialog's preset box.
    preset_name: String,
    /// The estimated size shown, with what it was worked out for.
    size_cache: Option<(SizeKey, Result<SizeEstimate, String>)>,
    /// The format the next Save As dialog starts on (from the export dialog).
    dialog_format: Option<SaveFormat>,
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
}

impl WaveApp {
    /// `session_dir`: the `--session-dir` flag, which wins over the environment variable and the
    /// settings file for this run.
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, session_dir: Option<PathBuf>) -> WaveApp {
        let settings_path = Settings::default_path();
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
        let mut app = WaveApp {
            doc: None,
            generation: 0,
            view: Viewport { start: 0.0, frames_per_px: 1.0 },
            selection: Selection::default(),
            player: Player::new(),
            params: SpectrogramParams::default(),
            show_waveform: true,
            show_spectrum: true,
            spectrum: None,
            renderer: Renderer::new(&cc.egui_ctx, palette()),
            opening: None,
            error: sessions.warning.clone(),
            notice: None,
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
            save_type: None,
            aiff_kind: AiffKind::Aiff,
            export_open: false,
            export_choice: ExportChoice::default(),
            export_presets_path: ExportPresets::default_path(),
            export_presets: ExportPresets::default_path().as_deref().map(ExportPresets::load).unwrap_or_default(),
            preset_name: String::new(),
            size_cache: None,
            dialog_format: None,
            confirm_close: false,
            allow_close: false,
            editor_width: 800.0,
            show_history: true,
            insert_silence: None,
            confirm_clear_history: false,
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
            ActionId::ZoomIn => self.view.zoom_at(2.0, width * 0.5, width, frames),
            ActionId::ZoomOut => self.view.zoom_at(0.5, width * 0.5, width, frames),
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
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let own_format = SaveFormat::from_path(&self.doc_path).filter(|f| SaveFormat::from_file_format(state.source().format) == Some(*f));
        let path = match to {
            Some(p) => p,
            None if own_format.is_some() && !save_as => self.doc_path.clone(),
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let first = self.dialog_format.take().or(own_format).unwrap_or(SaveFormat::Wav);
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
        let aiff = self.aiff_kind;
        let (job_state, job_path, job_progress, ctx) = (state.clone(), path.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(save_aiff_kind(&job_state, &job_path, format, ty, aiff, &mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.player.stop();
        self.saving = Some(Saving { path, state, ty, format, aiff, result: rx, progress, then_close, doc_id: self.doc_id });
        Ok(())
    }

    fn finish_saving(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.saving else { return };
        let result = match saving.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the save stopped unexpectedly".to_string()),
        };
        let Some(Saving { path, state, ty, format, aiff, then_close, doc_id, .. }) = self.saving.take() else { return };
        match result {
            // Another file was opened while this one was being saved: the file on disk is
            // written, but the open document is not the one saved, so its path, saved mark and
            // close request stay as they are.
            Ok(()) if doc_id != self.doc_id => {
                log::info!(target: "wavcraft", "a save of an earlier file finished after another was opened: {}", path.display());
                self.notice =
                    Some(format!("Saved {} ({} {}), the file open before this one", path.display(), ty.describe(), format_label(format, aiff)));
            }
            Ok(()) => {
                self.saved = Some(state);
                self.doc_path = path.clone();
                if let Some(doc) = self.doc.as_mut() {
                    doc.path = path.clone();
                }
                self.error = None;
                self.notice = Some(format!("Saved {} ({} {})", path.display(), ty.describe(), format_label(format, aiff)));
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

    /// Save As…: the export dialog first (format, sample type, AIFF kind, estimated size,
    /// presets); its button then runs the file dialog.
    fn open_export_dialog(&mut self) {
        if self.editor.is_none() || self.saving.is_some() {
            return;
        }
        if !self.export_open {
            let state = self.editor.as_ref().map(|e| e.history.current().clone());
            let own = state.as_ref().and_then(|st| SaveFormat::from_file_format(st.source().format));
            let ty = self.save_type.or_else(|| state.as_ref().map(|st| SampleType::for_source(st.source()))).unwrap_or(SampleType::Int24);
            self.export_choice = ExportChoice { format: own.unwrap_or(self.export_choice.format), ty, aiff: self.aiff_kind }.fitted();
        }
        self.export_open = true;
    }

    /// Make the export choice the one saves use (the type and AIFF kind; the format follows the
    /// file name).
    fn apply_export_choice(&mut self) {
        self.save_type = Some(self.export_choice.ty);
        self.aiff_kind = self.export_choice.aiff;
    }

    /// The estimated size of saving the open document with the export choice.
    fn export_estimate(&mut self) -> Option<Result<SizeEstimate, String>> {
        let state = self.editor.as_ref()?.history.current().clone();
        let c = self.export_choice;
        let key = (c.format, c.ty, c.aiff, Arc::as_ptr(&state) as usize);
        if self.size_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let est = estimate_size(&state, c.format, c.ty, c.aiff).map_err(|e| e.to_string());
            self.size_cache = Some((key, est));
        }
        self.size_cache.as_ref().map(|(_, e)| e.clone())
    }

    /// Keep the presets in their file; the message if that failed.
    fn store_presets(&self) -> Result<(), String> {
        match &self.export_presets_path {
            Some(file) => self.export_presets.save(file).map_err(|e| format!("could not save the presets to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so presets last until WavCraft closes".to_string()),
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let est = self.export_estimate();
        let mut open = true;
        let (mut go, mut choice) = (false, self.export_choice);
        let mut save_preset = false;
        let mut delete = None;
        let mut load = None;
        egui::Window::new("Save As / Export")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    for f in [SaveFormat::Wav, SaveFormat::Aiff, SaveFormat::Flac] {
                        ui.radio_value(&mut choice.format, f, f.name());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Sample type:");
                    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
                        ui.add_enabled_ui(choice.format.accepts(ty), |ui| ui.radio_value(&mut choice.ty, ty, ty.describe()));
                    }
                });
                if choice.format == SaveFormat::Aiff {
                    ui.horizontal(|ui| {
                        ui.label("AIFF:");
                        for k in AiffKind::ALL {
                            ui.radio_value(&mut choice.aiff, k, k.describe());
                        }
                    });
                    if choice.ty == SampleType::Float32 {
                        ui.weak("32-bit float is always AIFF-C fl32, big-endian.");
                    }
                }
                match &est {
                    Some(Ok(e)) => ui.label(format_size_label(e)),
                    Some(Err(why)) => ui.colored_label(Color32::from_rgb(235, 120, 90), why),
                    None => ui.label(""),
                };
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("export_preset").selected_text("Choose…").show_ui(ui, |ui| {
                        for p in &self.export_presets.presets {
                            if ui.selectable_label(false, &p.name).clicked() {
                                load = Some(p.name.clone());
                            }
                        }
                    });
                    ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("name").desired_width(120.0));
                    if ui.button("Save preset").clicked() {
                        save_preset = true;
                    }
                    if ui.button("Delete").clicked() {
                        delete = Some(self.preset_name.clone());
                    }
                });
                ui.separator();
                if ui.button("Save As…").clicked() {
                    go = true;
                }
            });
        self.export_choice = choice.fitted();
        if let Some(name) = load
            && let Some(c) = self.export_presets.get(&name)
        {
            self.export_choice = c;
            self.preset_name = name;
        }
        if save_preset {
            let r = self.export_presets.set(&self.preset_name, self.export_choice).and_then(|()| self.store_presets());
            match r {
                Ok(()) => self.notice = Some(format!("Saved the export preset {}", self.preset_name.trim())),
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(name) = delete {
            if self.export_presets.remove(&name) {
                match self.store_presets() {
                    Ok(()) => self.notice = Some(format!("Deleted the export preset {}", name.trim())),
                    Err(e) => self.error = Some(e),
                }
            } else {
                self.error = Some(format!("No export preset named {}", name.trim()));
            }
        }
        if go || !open {
            self.export_open = false;
        }
        if go {
            self.apply_export_choice();
            self.dialog_format = Some(self.export_choice.format);
            if let Err(e) = self.save(ctx, true, None, false) {
                self.error = Some(e);
            }
        }
    }

    /// `SAVEOPTS estimate|set <fmt> <bits> [aiff=<kind>]` and `SAVEOPTS show` (control channel).
    fn control_saveopts(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        let verb = words.next().unwrap_or("");
        if verb == "show" {
            return format!("OK {}", self.export_choice.describe());
        }
        let (Some(f), Some(t)) = (words.next(), words.next()) else {
            return "ERR SAVEOPTS estimate|set <wav|aiff|flac> <16|24|32|float> [aiff=aiff|aifc|sowt]".to_string();
        };
        let (Some(format), Some(ty)) = (SaveFormat::parse(f), SampleType::parse(t)) else {
            return format!("ERR no format or sample type {f} {t}: wav|aiff|flac, 16|24|32|float");
        };
        let mut aiff = self.export_choice.aiff;
        for w in words {
            match w.strip_prefix("aiff=").and_then(AiffKind::parse) {
                Some(k) => aiff = k,
                None => return format!("ERR aiff=<aiff|aifc|sowt>, not {w}"),
            }
        }
        let choice = ExportChoice { format, ty, aiff };
        match verb {
            "set" if !format.accepts(ty) => format!("ERR {} cannot hold {} samples", format.name(), ty.describe()),
            "set" => {
                self.export_choice = choice;
                self.apply_export_choice();
                format!("OK {}", choice.describe())
            }
            "estimate" => {
                let Some(editor) = &self.editor else { return "ERR no file is open".to_string() };
                match estimate_size(editor.history.current(), format, ty, aiff) {
                    Ok(e) => format!("OK bytes={} exact={}", e.bytes, u8::from(e.exact)),
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR SAVEOPTS estimate|set|show".to_string(),
        }
    }

    /// `EXPORTPRESET save|load|delete <name>` and `EXPORTPRESET list` (control channel).
    fn control_export_preset(&mut self, rest: &str) -> String {
        let (verb, name) = rest.split_once(' ').unwrap_or((rest, ""));
        match verb {
            "list" => format!("OK {}", self.export_presets.presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("|")),
            "save" => match self.export_presets.set(name, self.export_choice).and_then(|()| self.store_presets()) {
                Ok(()) => format!("OK {}", self.export_choice.describe()),
                Err(e) => format!("ERR {e}"),
            },
            "load" => match self.export_presets.get(name) {
                Some(c) => {
                    self.export_choice = c;
                    self.apply_export_choice();
                    self.preset_name = name.trim().to_string();
                    format!("OK {}", c.describe())
                }
                None => format!("ERR no export preset named {:?}", name.trim()),
            },
            "delete" => {
                if !self.export_presets.remove(name) {
                    return format!("ERR no export preset named {:?}", name.trim());
                }
                match self.store_presets() {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR EXPORTPRESET save|load|delete <name>, or list".to_string(),
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
            "SAVE" => {
                // `SAVE <path> [bits=16|24|32] [float=0|1]`: the options end the line.
                match parse_save_args(rest) {
                    Err(e) => format!("ERR {e}"),
                    Ok((path, ty, aiff)) => {
                        let to = (!path.is_empty()).then(|| PathBuf::from(path));
                        // `aiff=` is for this save only.
                        let kept = self.aiff_kind;
                        self.aiff_kind = aiff.unwrap_or(kept);
                        let r = self.save_with(ctx, to.is_some(), to, false, ty.or(self.save_type), ty.is_some());
                        self.aiff_kind = kept;
                        match r {
                            Ok(()) => "OK".to_string(),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                }
            }
            "SAVEOPTS" => self.control_saveopts(rest),
            "EXPORTPRESET" => self.control_export_preset(rest),
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
                if ui.add_enabled(has_doc, egui::Button::new("Save As…").shortcut_text(save_as)).clicked() {
                    self.open_export_dialog();
                    ui.close();
                }
                ui.menu_button("Sample Type on Save", |ui| {
                    ui.radio_value(&mut self.save_type, None, "Keep the file's own");
                    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
                        ui.radio_value(&mut self.save_type, Some(ty), ty.describe());
                    }
                    ui.label("FLAC takes 16 or 24 bit; it uses 24 bit for the others.");
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
        painter.rect_filled(rect, 0.0, BACKGROUND);
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
        draw_ruler(&painter, ruler, &self.view, rate);

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
            for ch in 0..channels {
                let lane = lane(area, ch, channels);
                let mid = lane.center().y;
                painter.line_segment([pos2(lane.left(), mid), pos2(lane.right(), mid)], Stroke::new(1.0, Color32::from_gray(50)));
                let half = lane.height() * 0.5 - 2.0;
                for (i, (lo, hi)) in doc.waveform_columns(ch, start, end, cols).into_iter().enumerate() {
                    let x = lane.left() + i as f32 + 0.5;
                    let (top, bottom) = (mid - hi.clamp(-1.0, 1.0) * half, mid - lo.clamp(-1.0, 1.0) * half);
                    painter.line_segment([pos2(x, top.min(bottom - 1.0)), pos2(x, bottom)], Stroke::new(1.0, WAVE));
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
            painter.rect_filled(Rect::from_min_max(pos2(a.max(rect.left()), body.top()), pos2(b.min(rect.right()), body.bottom())), 0.0, SELECTION);
        }
        let line = |frame: usize, color: Color32| {
            let x = rect.left() + self.view.x_of(frame as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, color));
            }
        };
        line(self.selection.start(), CURSOR);
        if self.player.is_playing() {
            line(self.player.position(), PLAYHEAD);
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
        self.close_prompt(&ctx);
        self.export_window(&ctx);
        let (save_as, save) = ctx.input_mut(|i| {
            let save_as = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::S);
            (save_as, i.consume_key(egui::Modifiers::COMMAND, Key::S))
        });
        if save_as {
            self.open_export_dialog();
        } else if save && let Err(e) = self.save(&ctx, false, None, false) {
            self.error = Some(e);
        }
        for id in pressed_actions(&ctx) {
            let _ = self.run_action(id, &ctx);
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
        self.clear_history_prompt(&ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
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

/// The vertical slice of `area` for channel `ch` of `channels`.
fn lane(area: Rect, ch: usize, channels: usize) -> Rect {
    let h = area.height() / channels.max(1) as f32;
    Rect::from_min_max(pos2(area.left(), area.top() + h * ch as f32), pos2(area.right(), area.top() + h * (ch + 1) as f32))
}

/// Tick marks and times along the top, at a round step that leaves room for the labels.
fn draw_ruler(painter: &egui::Painter, rect: Rect, view: &Viewport, rate: u32) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(32));
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
        painter.line_segment([pos2(x, rect.bottom() - 6.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(140)));
        painter.text(pos2(x + 3.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, tick_label(t), egui::FontId::monospace(11.0), Color32::from_gray(190));
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

/// "AIFF-C, little-endian (sowt)" for an AIFF save, else the format's name.
fn format_label(format: SaveFormat, aiff: AiffKind) -> &'static str {
    if format == SaveFormat::Aiff { aiff.describe() } else { format.name() }
}

/// The estimated size line of the export dialog.
fn format_size_label(e: &SizeEstimate) -> String {
    let human = wavcraft_core::space::format_bytes(e.bytes);
    if e.exact {
        format!("File size: {human} ({} bytes)", e.bytes)
    } else {
        format!("File size: about {human} (estimate: FLAC depends on the audio)")
    }
}

/// `SAVE`'s arguments: a path (may hold spaces) then `bits=<16|24|32>` and `float=<0|1>` in any
/// order. Returns the path and the sample type they name, if any.
fn parse_save_args(rest: &str) -> Result<(&str, Option<SampleType>, Option<AiffKind>), String> {
    let (mut path, mut bits, mut float, mut aiff) = (rest.trim(), None, None, None);
    while let Some((head, last)) = path.rsplit_once(char::is_whitespace).or(if path.contains('=') { Some(("", path)) } else { None }) {
        if let Some(v) = last.strip_prefix("bits=") {
            bits = Some(v.parse::<u32>().map_err(|_| format!("bits=<16|24|32>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("aiff=") {
            aiff = Some(AiffKind::parse(v).ok_or_else(|| format!("aiff=<aiff|aifc|sowt>, not {v}"))?);
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
    Ok((path, ty, aiff))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn save_arguments_name_the_path_and_the_sample_type() {
        assert_eq!(parse_save_args("/a b/c.wav"), Ok(("/a b/c.wav", None, None)));
        assert_eq!(parse_save_args("/a b/c.wav bits=24 float=0"), Ok(("/a b/c.wav", Some(SampleType::Int24), None)));
        assert_eq!(parse_save_args("x.aiff float=1"), Ok(("x.aiff", Some(SampleType::Float32), None)));
        assert_eq!(parse_save_args("x.aiff bits=32 float=1"), Ok(("x.aiff", Some(SampleType::Float32), None)));
        assert_eq!(parse_save_args("x.flac bits=16"), Ok(("x.flac", Some(SampleType::Int16), None)));
        assert_eq!(parse_save_args(""), Ok(("", None, None)));
        assert_eq!(parse_save_args("/a b/c.aiff aiff=sowt"), Ok(("/a b/c.aiff", None, Some(AiffKind::AifcSowt))));
        assert_eq!(parse_save_args("x.aiff bits=24 aiff=aifc"), Ok(("x.aiff", Some(SampleType::Int24), Some(AiffKind::Aifc))));
        assert!(parse_save_args("x.aiff aiff=big").is_err());
        assert!(parse_save_args("x.wav bits=20").is_err());
        assert!(parse_save_args("x.wav bits=24 float=1").is_err());
        assert!(parse_save_args("x.wav float=2").is_err());
    }
}
