//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::meta;
use wavcraft_core::panels::{self, PanelSet};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::{DocState, Document, FileFormat, GRAB_PX, History, OpenError, Selection, SpectrogramParams, Viewport, format_time};

use crate::control;
use crate::effect_dialog::{Answer, EffectDialog};
use crate::player::Player;
use crate::spectrum::{Job, Renderer};
use crate::video::{VideoPanel, Waiters};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const WAVE: Color32 = Color32::from_rgb(64, 200, 140);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const RULER_HEIGHT: f32 = 22.0;

/// Longest silence Insert Silence accepts, in seconds (24 hours).
const MAX_SILENCE_SECS: f64 = 86_400.0;

/// Frames for an Insert Silence of `secs` seconds at `rate` Hz, or an error message when it is longer than 24 hours.
fn silence_frames(secs: f64, rate: u32) -> Result<u64, String> {
    if !secs.is_finite() || secs <= 0.0 || secs > MAX_SILENCE_SECS {
        return Err("Insert Silence: the length can be at most 24 hours".to_string());
    }
    Ok((secs * f64::from(rate)).round() as u64)
}

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

/// What a primary-button drag in the editor does (f64: a minimal switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tool {
    /// Click places the cursor, drag selects.
    #[default]
    Select,
    /// Drag scrolls the view.
    Hand,
    /// Click zooms in at the point (Alt: out), drag zooms to the range.
    Zoom,
}

#[derive(Default)]
struct HudState {
    gain_db: f64,
    pinned: bool,
    hidden: bool,
    only_for_selection: bool,
    drag_start_db: Option<f64>,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Hand => "hand",
            Tool::Zoom => "zoom",
        }
    }
}

pub struct WaveApp {
    doc: Option<Document>,
    tool: Tool,
    /// Where the current drag began (set when a drag starts, for the Zoom tool and the ruler).
    drag_origin: Option<Pos2>,
    /// Bumped on every open, so cached pictures of the previous file are dropped.
    generation: usize,
    view: Viewport,
    selection: Selection,
    player: Player,
    params: SpectrogramParams,
    /// Which registered panels are shown (Window menu; kept in the settings file).
    panels: PanelSet,
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
    hud: HudState,
    /// Insert › Silence…: the length being typed, in seconds, while the dialog is up.
    insert_silence: Option<String>,
    /// Clear History asks first: it can't be undone.
    confirm_clear_history: bool,
    /// Window › Metadata.
    show_metadata: bool,
    /// The Metadata panel's text boxes (one per `meta::FIELDS`) and the source they were filled
    /// from: refilled whenever the current step's metadata is another one.
    meta_draft: Vec<String>,
    meta_for: Option<Arc<wavcraft_core::SourceFormat>>,
    /// Video › Attach video file…: the picture of a video file at the playhead (f122).
    video: Option<VideoPanel>,
    show_video: bool,
    /// Control-channel `VIDEO frame` requests waiting for their picture.
    video_waiters: Waiters,
    /// A History row had keyboard focus last frame: Up and Down move through the states then.
    history_focus: bool,
    /// The History row to give keyboard focus next frame (after an arrow key moved the cursor).
    history_refocus: Option<usize>,
    /// The effect parameter dialog (f14), while it is up.
    effect_dialog: Option<EffectDialog>,
    /// A Generate dialog's Preview is playing (f21): the player holds the preview, not the file.
    previewing: bool,
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
            tool: Tool::default(),
            drag_origin: None,
            selection: Selection::default(),
            player: Player::new(),
            params: SpectrogramParams::default(),
            panels: PanelSet::from_hidden(&settings.hidden_panels),
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
            confirm_close: false,
            allow_close: false,
            editor_width: 800.0,
            hud: HudState::default(),
            insert_silence: None,
            confirm_clear_history: false,
            show_metadata: false,
            meta_draft: Vec::new(),
            meta_for: None,
            video: None,
            show_video: true,
            video_waiters: Waiters::default(),
            history_focus: false,
            history_refocus: None,
            effect_dialog: None,
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
                self.drop_video();
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
    /// Associate a video file with the open document (Video › Attach video file…).
    fn attach_video(&mut self, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("open a file first: the video goes with the open document".to_string());
        }
        self.drop_video();
        self.video = Some(VideoPanel::attach(path, ctx));
        self.show_video = true;
        Ok(())
    }

    fn drop_video(&mut self) {
        self.video_waiters.fail_all("ERR video detached");
        self.video = None;
    }

    fn pick_video(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Video", &["mov", "mp4", "m4v"]).pick_file()
            && let Err(e) = self.attach_video(path, ctx)
        {
            self.error = Some(e);
        }
    }

    /// Where the playhead is (or the cursor when stopped), in seconds.
    fn playhead_secs(&self) -> Option<f64> {
        let rate = self.editor.as_ref()?.history.current().sample_rate();
        let frame = if self.player.is_playing() { self.player.position() } else { self.selection.start() };
        (rate > 0).then(|| frame as f64 / f64::from(rate))
    }

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
        let mut sel = self.selection_range();
        // Markers go where play is, when it is playing (a point at the playhead).
        if matches!(id, ActionId::MarkerAdd | ActionId::MarkerNext | ActionId::MarkerPrev) && self.player.is_playing() {
            let at = self.player.position() as u64;
            sel = at..at;
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        match editor.run(id, sel) {
            Ok(out) => {
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

    /// The actions the window carries out itself: zoom, In/Out points at the playhead, and the
    /// Insert Silence dialog.
    fn run_window_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        if frames == 0 && id != ActionId::InsertSilence {
            return Err("no file is open".to_string());
        }
        let width = self.editor_width.max(1.0);
        // Where play is, or the cursor when stopped.
        let playhead = if self.player.is_playing() && !self.previewing { self.player.position() } else { self.selection.start() };
        match id {
            ActionId::ZoomIn => self.view.zoom_at(2.0, width * 0.5, width, frames),
            ActionId::ZoomOut => self.view.zoom_at(0.5, width * 0.5, width, frames),
            ActionId::ZoomOutFull => self.fit_on_next_frame = true,
            ActionId::ToolSelect => self.tool = Tool::Select,
            ActionId::ToolHand => self.tool = Tool::Hand,
            ActionId::ToolZoom => self.tool = Tool::Zoom,
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
        let rate = editor.history.current().sample_rate();
        let frames = match silence_frames(secs, rate) {
            Ok(f) => f,
            Err(why) => {
                self.error = Some(why.clone());
                return Some(why);
            }
        };
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
        if self.saving.is_none() {
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
        let keys = src.meta.keys();
        if self.meta_for.as_ref().is_none_or(|a| !Arc::ptr_eq(a, &src)) || self.meta_draft.len() != keys.len() {
            self.meta_draft = keys.iter().map(|f| src.meta.get(f).unwrap_or_default()).collect();
            self.meta_for = Some(src.clone());
        }
        let mut apply = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (field, text) in keys.iter().zip(self.meta_draft.iter_mut()) {
                match field.strip_prefix("chname.") {
                    Some(n) => ui.label(format!("channel {n} name")),
                    None => ui.label(field.replace('_', " ")),
                };
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
            let changes: Vec<(String, String)> = keys.iter().cloned().zip(self.meta_draft.iter().cloned()).collect();
            match self.apply_metadata(&changes) {
                Ok(_) => self.meta_for = None,
                Err(e) => self.error = Some(format!("Metadata: {e}")),
            }
        }
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

    /// Answer `GESTURE <name> [args]` (see `control.rs`): the selection gestures at a frame, as
    /// the editor would see them, and the mouse gestures of the navigation tools
    /// (`hand_drag <dx_px>`, `zoom_click <frame> [out]`, `ruler_rdrag <start> <end>`), through
    /// the same code the pointer uses.
    fn control_gesture(&mut self, rest: &str) -> String {
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        let mut it = rest.split_whitespace();
        let name = it.next().unwrap_or("");
        let nums: Vec<usize> = it.map_while(|w| w.parse().ok()).collect();
        let at = |i: usize| nums.get(i).copied().map(|f| f.min(frames));
        // Navigation gestures answer with the view they leave.
        let nav_width = self.editor_width.max(1.0);
        let words: Vec<&str> = rest.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok()).filter(|n| n.is_finite());
        if frames == 0 && matches!(name, "hand_drag" | "zoom_click" | "ruler_rdrag") {
            return "ERR no file is open".to_string();
        }
        let nav = match (name, num(1), num(2)) {
            ("hand_drag", Some(dx), _) => {
                self.view.drag_by(dx as f32, nav_width, frames);
                true
            }
            ("zoom_click", Some(frame), _) => {
                self.view.zoom_click(frame, words.get(2) == Some(&"out"), nav_width, frames);
                true
            }
            ("ruler_rdrag", Some(a), Some(b)) => {
                self.view.zoom_to_range(a, b, nav_width, frames);
                true
            }
            _ => false,
        };
        if nav {
            return format!("OK view={:.0}-{:.0}", self.view.start, self.view.end(nav_width));
        }
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
            _ => return "ERR GESTURE shift_click <frame> | double_click | triple_click | edge_drag <from> <to> | shift_left|shift_right [frames] | shift_home | shift_end | hand_drag <dx_px> | zoom_click <frame> [out] | ruler_rdrag <start> <end>".to_string(),
        }
        format!("OK selection={}-{}", self.selection.start(), self.selection.end())
    }

    /// Answer one control-channel line (see `control.rs`).
    fn control_request(&mut self, line: &str, ctx: &egui::Context) -> String {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let sel = |s: &Self| format!("{}-{}", s.selection.start(), s.selection.end());
        match cmd {
            "STATE" => {
                let busy = self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some();
                let unsaved = u8::from(self.unsaved());
                let width = self.editor_width.max(1.0);
                let view = format!("tool={} view={:.0}-{:.0}", self.tool.name(), self.view.start, self.view.end(width));
                match &self.editor {
                    Some(e) => {
                        let doc = e.history.current();
                        format!(
                            "OK frames={} channels={} rate={} selection={} steps={} cursor={} busy={} unsaved={unsaved} {view} undo={} redo={}",
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
                    None => {
                        format!("OK frames=0 channels=0 rate=0 selection=0-0 steps=0 cursor=0 busy={} unsaved=0 {view} undo=- redo=-", u8::from(busy))
                    }
                }
            }
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
            // The History panel's click: go to step <index> (0 is Open).
            "HISTORY" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true) if self.saving.is_none() => {
                    if self.history_go(i, ctx) {
                        "OK".to_string()
                    } else {
                        format!("ERR no step {i}")
                    }
                }
                _ => "ERR HISTORY <step>, with a file open".to_string(),
            },
            "VIDEO" => {
                let (sub, arg) = rest.split_once(' ').unwrap_or((rest, ""));
                match sub {
                    "attach" if !arg.trim().is_empty() => match self.attach_video(PathBuf::from(arg.trim()), ctx) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                    "status" => match &self.video {
                        Some(v) => format!("OK {}", v.status.text()),
                        None => "ERR no video attached".to_string(),
                    },
                    "detach" => {
                        self.drop_video();
                        "OK".to_string()
                    }
                    _ => "ERR VIDEO attach <path> | frame <seconds> | status | detach".to_string(),
                }
            }
            // The panel's Delete State: step <index> and every later step go (0 can't).
            "HISTORY_DELETE" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true) if self.saving.is_none() => match self.history_delete(i, ctx) {
                    Some(n) => format!("OK removed={n}"),
                    None => format!("ERR no step {i} to delete (Open can't be)"),
                },
                _ => "ERR HISTORY_DELETE <step>, with a file open".to_string(),
            },
            // Clear History without the panel's confirmation: the current state is all that stays.
            "HISTORY_CLEAR" => match self.editor.as_mut() {
                Some(e) if self.saving.is_none() => {
                    e.history.clear();
                    "OK".to_string()
                }
                _ => "ERR HISTORY_CLEAR, with a file open".to_string(),
            },
            // The panel's Up / Down keys.
            "HISTORY_KEY" if self.editor.is_some() && self.saving.is_none() => match rest.trim() {
                k @ ("up" | "down") => match self.history_arrow(k == "down", ctx) {
                    Some(i) => format!("OK cursor={i}"),
                    None => "OK cursor=unchanged".to_string(),
                },
                _ => "ERR HISTORY_KEY up|down".to_string(),
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
            // CHNAME get | CHNAME set <ch> <name>: channel names (<ch> counts from 1); one undo step.
            "CHNAME" => {
                let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                match verb {
                    "get" => format!(
                        "OK {}",
                        editor
                            .history
                            .current()
                            .source()
                            .meta
                            .chnames
                            .iter()
                            .map(|n| meta::escape_lines(n).replace(' ', "_"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    "set" => {
                        let (ch, name) = rest.trim_start().split_once(' ').unwrap_or((rest.trim(), ""));
                        match ch.parse::<usize>() {
                            Ok(n) => match self.apply_metadata(&[(format!("chname.{n}"), name.to_string())]) {
                                Ok(changed) => format!("OK changed={}", u8::from(changed)),
                                Err(e) => format!("ERR {e}"),
                            },
                            Err(_) => "ERR CHNAME set <channel from 1> <name>".to_string(),
                        }
                    }
                    _ => "ERR CHNAME get | CHNAME set <channel from 1> <name>".to_string(),
                }
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
        ui.separator();
        for a in TABLE.iter().filter(|a| a.name.starts_with("marker.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            let keys = a.keys.first().map(|k| k.text(mac)).unwrap_or_default();
            if ui.add_enabled(enabled, egui::Button::new(a.menu).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
        }
    }

    /// The Effects menu: one entry per registered effect, run on the selection (the whole file
    /// when nothing is selected) and the enabled channels.
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
                for a in TABLE.iter().filter(|a| a.name.starts_with("view.") || a.name.starts_with("tool.")) {
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
                ui.checkbox(&mut self.show_metadata, "Metadata panel");
            });
            ui.menu_button("Video", |ui| {
                if ui.add_enabled(self.editor.is_some(), egui::Button::new("Attach video file…")).clicked() {
                    self.pick_video(ui.ctx());
                }
                if ui.add_enabled(self.video.is_some(), egui::Button::new("Detach video")).clicked() {
                    self.drop_video();
                }
                ui.checkbox(&mut self.show_video, "Video panel");
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

    fn draw_hud(&mut self, ctx: &egui::Context) {
        if !actions::hud_visible(self.hud.hidden, self.hud.only_for_selection, !self.selection.is_empty()) {
            return;
        }
        egui::Area::new(egui::Id::new("amplitude-hud")).movable(!self.hud.pinned).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Gain {:.1} dB", self.hud.gain_db));
                    if ui.button(if self.hud.pinned { "Unpin" } else { "Pin" }).clicked() {
                        self.hud.pinned = !self.hud.pinned;
                    }
                    if ui.button("Hide").clicked() {
                        self.hud.hidden = true;
                    }
                });
                let drag = ui.add(egui::Label::new("Drag vertically to change").sense(Sense::drag()));
                if drag.drag_started() {
                    self.hud.drag_start_db = Some(self.hud.gain_db);
                }
                if drag.dragged() {
                    let start = self.hud.drag_start_db.unwrap_or(self.hud.gain_db);
                    self.hud.gain_db = actions::hud_drag_gain_db(start, -f64::from(drag.drag_delta().y));
                }
                if drag.drag_stopped() {
                    self.hud.drag_start_db = None;
                }
                ui.checkbox(&mut self.hud.only_for_selection, "Show only for selections");
            });
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let Some(doc) = &self.doc else { return };
        let (frames, rate, channels) = (doc.frames(), doc.sample_rate(), doc.channels().max(1));
        let chnames: Vec<String> = self.editor.as_ref().map(|e| e.history.current().source().meta.chnames.clone()).unwrap_or_default();
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

        // Select tool: click places the cursor, drag selects; near a selection edge a drag adjusts
        // it, Shift-click extends, double-click selects what is on screen, triple-click everything.
        let frame_at = |view: &Viewport, pos: Pos2| view.frame_at(pos.x - rect.left()).clamp(0.0, frames as f64).round() as usize;
        let edge_at = |sel: &Selection, view: &Viewport, pos: Pos2| sel.edge_near(view, pos.x - rect.left(), GRAB_PX);
        if let Some(pos) = response.hover_pos().or(response.interact_pointer_pos())
            && edge_at(&self.selection, &self.view, pos).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let ruler_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        if response.drag_started() {
            self.drag_origin = ui.input(|i| i.pointer.press_origin()).or_else(|| response.interact_pointer_pos());
        }
        let drag_dx = response.drag_delta().x;
        // Middle-button drag scrolls whatever the tool (A084); the Hand tool does it with the
        // primary button (A050).
        if response.dragged_by(egui::PointerButton::Middle) || (self.tool == Tool::Hand && response.dragged_by(egui::PointerButton::Primary)) {
            self.view.drag_by(drag_dx, width, frames);
        }
        // A range drag zooms to it: right-drag in the ruler (A091), or the Zoom tool's drag (A051).
        let range_zoom = (response.drag_stopped_by(egui::PointerButton::Secondary) && self.drag_origin.is_some_and(|o| ruler_rect.contains(o)))
            || (self.tool == Tool::Zoom && response.drag_stopped_by(egui::PointerButton::Primary));
        if range_zoom
            && let (Some(origin), Some(end)) = (self.drag_origin, ui.input(|i| i.pointer.latest_pos()).or_else(|| response.interact_pointer_pos()))
        {
            let (a, b) = (self.view.frame_at(origin.x - rect.left()), self.view.frame_at(end.x - rect.left()));
            self.view.zoom_to_range(a, b, width, frames);
        }
        if self.tool == Tool::Zoom
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let out = ui.input(|i| i.modifiers.alt);
            self.view.zoom_click(self.view.frame_at(pos.x - rect.left()), out, width, frames);
        }

        // Selection gestures belong to the Select tool, primary button only.
        let select = self.tool == Tool::Select;
        let shift = ui.input(|i| i.modifiers.shift);
        if select
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            // A drag is only reported once the pointer has moved a few pixels: anchor the
            // selection where the button went down, not where the pointer has got to.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if let Some(edge) = edge_at(&self.selection, &self.view, origin) {
                self.selection.grab(edge);
                self.selection.head = frame_at(&self.view, pos);
            } else if shift {
                self.selection.extend_to(frame_at(&self.view, pos));
            } else {
                self.selection = Selection { anchor: frame_at(&self.view, origin), head: frame_at(&self.view, pos) };
            }
        } else if select
            && response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selection.head = frame_at(&self.view, pos);
        } else if select && response.triple_clicked_by(egui::PointerButton::Primary) {
            self.selection = Selection::all(frames);
        } else if select && response.double_clicked_by(egui::PointerButton::Primary) {
            self.selection = Selection::visible(&self.view, width, frames);
        } else if select
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            if shift {
                self.selection.extend_to(frame_at(&self.view, pos));
            } else {
                self.selection = Selection::at(frame_at(&self.view, pos));
            }
        }

        let (start, end) = (self.view.start, self.view.end(width));
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(rect.left(), ruler.bottom()), rect.max);
        draw_ruler(&painter, ruler, &self.view, rate);
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
            for ch in 0..channels {
                let lane = lane(area, ch, channels);
                let mid = lane.center().y;
                painter.line_segment([pos2(lane.left(), mid), pos2(lane.right(), mid)], Stroke::new(1.0, Color32::from_gray(50)));
                let half = lane.height() * 0.5 - 2.0;
                if let Some(name) = chnames.get(ch).filter(|n| !n.is_empty()) {
                    painter.text(
                        pos2(lane.left() + 4.0, lane.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        name,
                        egui::FontId::proportional(12.0),
                        Color32::from_gray(170),
                    );
                }
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
        if self.player.is_playing() && !self.previewing {
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
            // `VIDEO frame` is answered when the worker has the picture, not before.
            if let Some(arg) = req.line.strip_prefix("VIDEO frame") {
                match (arg.trim().parse::<f64>(), self.video.as_mut()) {
                    (Ok(secs), Some(v)) if secs.is_finite() => {
                        let seq = v.request(secs, true);
                        self.video_waiters.add(seq, req.reply);
                    }
                    (_, None) => {
                        let _ = req.reply.send("ERR no video attached".to_string());
                    }
                    _ => {
                        let _ = req.reply.send("ERR VIDEO frame <seconds>".to_string());
                    }
                }
                continue;
            }
            let answer = self.control_request(&req.line, ctx);
            let _ = req.reply.send(answer);
        }
        if let Some(v) = self.video.as_mut() {
            for (seq, text) in v.poll(ctx) {
                self.video_waiters.answer(seq, text);
            }
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
        self.selection_keys(&ctx);
        // Up and Down belong to the History panel while one of its rows has focus (before the
        // action table gets them: there they pick the left or right channel).
        if self.history_focus && self.saving.is_none() {
            let (up, down) =
                ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)));
            if let Some(to) = (up || down).then(|| self.history_arrow(down, &ctx)).flatten() {
                self.history_refocus = Some(to);
            }
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
        if self.show_video && self.video.is_some() {
            let secs = self.playhead_secs();
            if let Some(v) = self.video.as_mut() {
                if let Some(secs) = secs {
                    v.follow(secs);
                }
                egui::Panel::right("video").default_size(360.0).show(ui, |ui| v.ui(ui));
            }
        }
        if self.panels.is_shown("history") {
            egui::Panel::right("history").default_size(220.0).show(ui, |ui| self.history_panel(ui));
        }
        if self.show_metadata {
            egui::Panel::right("metadata").default_size(260.0).show(ui, |ui| self.metadata_panel(ui));
        }
        self.insert_silence_dialog(&ctx);
        self.effect_dialog_ui(&ctx);
        self.clear_history_prompt(&ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.editor(ui));
        self.draw_hud(&ctx);
    }
}

/// The History step an Up or Down key moves to from `cursor` among `len` steps (none at an end).
fn history_arrow_target(cursor: usize, len: usize, down: bool) -> Option<usize> {
    if down { (cursor + 1 < len).then_some(cursor + 1) } else { cursor.checked_sub(1) }
}

/// The actions whose keys were pressed this frame. Keys with more modifiers are taken first, so
/// Cmd+Shift+Z (Redo) is not read as Cmd+Z (Undo).
fn pressed_actions(ctx: &egui::Context) -> Vec<ActionId> {
    // A text field being edited owns the keyboard: Backspace, Delete and Cmd+Z there edit the text, not the audio (#368).
    if ctx.text_edit_focused() {
        return Vec::new();
    }
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

    /// Runs three frames on a fresh context: frame 1 draws a text field and (when `focus`) asks for focus,
    /// frame 2 lets that focus settle, frame 3 delivers one key press and returns what `pressed_actions` reports.
    fn actions_for_key(focus: bool, key: Key, modifiers: egui::Modifiers) -> Vec<ActionId> {
        let ctx = egui::Context::default();
        let mut text = String::from("10");
        let mut frame = |events: Vec<egui::Event>, ask_focus: bool| -> Vec<ActionId> {
            let raw = egui::RawInput { events, ..Default::default() };
            let mut found = Vec::new();
            let mut output = ctx.run_ui(raw, |ui| {
                let r = ui.text_edit_singleline(&mut text);
                if ask_focus {
                    r.request_focus();
                }
                found = pressed_actions(ui.ctx());
            });
            output.textures_delta.clear();
            found
        };
        let _ = frame(Vec::new(), focus);
        let _ = frame(Vec::new(), false);
        let press = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        frame(vec![press], false)
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
        let p = palette();
        assert_eq!(p.len(), 256);
        assert_eq!(p[0], Color32::from_rgb(0, 0, 0));
        assert_eq!(p[255], Color32::from_rgb(255, 250, 215));
        let brightness = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(p.windows(2).all(|w| brightness(w[1]) + 3 >= brightness(w[0])), "brightness never drops");
    }

    #[test]
    fn silence_longer_than_a_day_is_rejected() {
        assert_eq!(silence_frames(1.0, 44_100), Ok(44_100));
        assert_eq!(silence_frames(86_400.0, 48_000), Ok(4_147_200_000));
        assert!(silence_frames(86_400.5, 48_000).is_err());
        assert!(silence_frames(1e20, 44_100).is_err());
        assert!(silence_frames(f64::INFINITY, 44_100).is_err());
        assert!(silence_frames(0.0, 44_100).is_err());
    }

    #[test]
    fn lanes_stack_without_gaps() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 300.0));
        assert_eq!(lane(area, 0, 2), Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 200.0)));
        assert_eq!(lane(area, 1, 2), Rect::from_min_max(pos2(0.0, 200.0), pos2(800.0, 300.0)));
        assert_eq!(lane(area, 0, 0), area);
    }

    #[test]
    fn backspace_in_a_focused_text_field_runs_no_action() {
        let got = actions_for_key(true, Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(got, Vec::<ActionId>::new());
    }

    #[test]
    fn cmd_z_in_a_focused_text_field_runs_no_action() {
        let got = actions_for_key(true, Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(got, Vec::<ActionId>::new());
    }

    #[test]
    fn delete_key_in_a_focused_text_field_runs_no_action() {
        let got = actions_for_key(true, Key::Delete, egui::Modifiers::NONE);
        assert_eq!(got, Vec::<ActionId>::new());
    }

    #[test]
    fn backspace_without_a_focused_text_field_runs_delete() {
        let got = actions_for_key(false, Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(got, vec![ActionId::Delete]);
    }

    #[test]
    fn cmd_z_without_a_focused_text_field_runs_undo() {
        let got = actions_for_key(false, Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(got, vec![ActionId::Undo]);
    }

    #[test]
    fn cmd_shift_z_without_a_focused_text_field_runs_redo_only() {
        let got = actions_for_key(false, Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        assert_eq!(got, vec![ActionId::Redo]);
    }

    #[test]
    fn split_seed_pulls_the_seed_out() {
        let (seed, rest) = split_seed("freq=440 seed=7 dur=2");
        assert_eq!((seed, rest.as_str()), (Ok(7), "freq=440 dur=2"));
        assert!(split_seed("seed=x").0.is_err());
        assert!(split_seed("dur=2").0.is_ok());
    }
}
