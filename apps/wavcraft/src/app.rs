//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::doc::edit as doc_edit;
use wavcraft_core::doc::newdoc::{self, NewDocOptions};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::{Clip, DocState, Document, FileFormat, History, OpenError, SampleFormat, Selection, SpectrogramParams, Viewport, format_time};

use crate::control;
use crate::doclist::{DocList, Where};
use crate::files;
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
    /// A new document being made from a clip.
    making: Option<Making>,
    /// File > New > Audio File…, while its dialog is up.
    new_audio: Option<NewAudio>,
    /// File > Close All is going through the documents, asking about unsaved ones.
    close_all: bool,
    save_all: Option<SaveAllRun>,
    /// Window › History.
    show_history: bool,
    /// Window › Files.
    show_files: bool,
    files: files::View,
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
            doc_serial: 0,
            docs: DocList::new(),
            closing_doc: false,
            rebuilding: None,
            saved: None,
            saving: None,
            confirm_close: false,
            allow_close: false,
            editor_width: 800.0,
            making: None,
            new_audio: None,
            close_all: false,
            save_all: None,
            show_history: true,
            show_files: true,
            files: files::View::default(),
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
                // The document that was open stays open, parked; this one is added at the end.
                let clip = self.editor.as_mut().and_then(|e| e.clipboard.take());
                let old = self.editor.is_some().then(|| self.take_slot());
                self.docs.add(old);
                self.player.set_audio(Some(doc.audio.clone()));
                let cap = History::default_cap(&self.sessions.path);
                self.editor = doc.state.clone().map(|state| Editor::new(History::new(state, cap)));
                if let Some(e) = self.editor.as_mut() {
                    e.clipboard = clip;
                }
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
        let clip = self.editor.as_mut().and_then(|e| e.clipboard.take());
        let old = self.take_slot();
        let next = self.docs.switch(i, old).unwrap_or_else(|old| old);
        self.put_slot(next);
        if let (Some(clip), Some(e)) = (clip, self.editor.as_mut()) {
            e.clipboard = Some(clip);
        }
        Ok(())
    }

    /// Close the current document without asking; the next one (or the last) takes its place.
    fn remove_current(&mut self) {
        self.player.stop();
        // The clipboard outlives the document it was copied from.
        let clip = self.editor.as_mut().and_then(|e| e.clipboard.take());
        drop(self.take_slot());
        match self.docs.close_current() {
            Some(next) => {
                self.put_slot(next);
                if let (Some(clip), Some(e)) = (clip, self.editor.as_mut()) {
                    e.clipboard = Some(clip);
                }
            }
            None => {
                self.player.set_audio(None);
                self.generation += 1;
                self.spectrum = None;
                self.selection = Selection::default();
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
        let clip = self.editor.as_mut().and_then(|e| e.clipboard.take());
        let old = self.editor.is_some().then(|| self.take_slot());
        self.docs.add(old);
        self.player.stop();
        self.player.set_audio(None);
        let frames = state.len_frames();
        let mut editor = Editor::new(History::new(state, History::default_cap(&self.sessions.path)));
        editor.clipboard = clip;
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
        let Some(clip) = self.editor.as_ref().and_then(|e| e.clipboard.clone()) else {
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
        if matches!(id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
            return self.run_doc_action(id, ctx, true).inspect_err(|e| self.error = Some(e.clone()));
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

    /// Answer one control-channel line (see `control.rs`).
    fn control_request(&mut self, line: &str, ctx: &egui::Context) -> String {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let sel = |s: &Self| format!("{}-{}", s.selection.start(), s.selection.end());
        match cmd {
            "STATE" => {
                let busy =
                    self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some() || self.making.is_some() || self.save_all.is_some();
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
                Some(a) => match if matches!(a.id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
                    self.run_doc_action(a.id, ctx, false)
                } else {
                    self.run_action(a.id, ctx)
                } {
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
                ui.menu_button("New", |ui| {
                    if ui.button("Audio File…").clicked() {
                        self.new_audio = Some(NewAudio::default());
                        ui.close();
                    }
                });
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
                ui.checkbox(&mut self.show_files, "Files panel");
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
            } else if self.editor.is_some() {
                ui.label(format!("{} (no audio yet): paste into it, or File › Save As once it has some", path_name(&self.doc_path)));
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
        self.finish_making(ctx);
        self.pump_save_all(ctx);
        self.pump_close_all();
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
        }
        let star = if self.unsaved() { "*" } else { "" };
        let title = if self.editor.is_some() { format!("{star}{} — WavCraft", path_name(&self.doc_path)) } else { "WavCraft".to_string() };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if self.docs.len(self.editor.is_some()) > 1 {
            egui::Panel::top("docs").show(ui, |ui| self.doc_tabs(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.show_files {
            egui::Panel::left("files").default_size(420.0).show(ui, |ui| self.files_panel(ui));
        }
        if self.show_history {
            egui::Panel::right("history").default_size(220.0).show(ui, |ui| self.history_panel(ui));
        }
        self.insert_silence_dialog(&ctx);
        self.new_audio_dialog(&ctx);
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
}
