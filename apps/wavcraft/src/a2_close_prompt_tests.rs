//! Tests for A2.2 (#1156): closing the window with unsaved changes is held back and asks first, and
//! a clean document closes without a prompt.
//!
//! `close_prompt` runs once per frame. On a frame where the viewport reports a close request, it
//! sends `CancelClose` and raises `confirm_close` when any document has unsaved changes. The tests
//! build the window as the program does (on eframe's headless creation context), feed one frame a
//! viewport `Close` event, and read the commands the frame sent.
use super::*;

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-a2-close-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The window as the program builds it, with its session folder in `scratch`. Its settings file and
/// recent-files list are cut loose at once, so nothing a test does is written to the user's own.
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// Open a new document with one Insert Silence of 100 frames, so it has unsaved changes.
fn a_document_with_unsaved_changes(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    app.add_document(state, &ctx);
    app.editor.as_mut().expect("the document is open").insert_silence(0, 100).expect("the Insert Silence");
    assert!(app.unsaved(), "the document has unsaved changes");
}

/// Open a new document with 100 frames of silence and save it to `path`, running the save to its
/// end, so it is clean. (An empty document has no audio to save.)
fn a_saved_document(app: &mut WaveApp, path: PathBuf) {
    let ctx = app.ctx.clone();
    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    app.add_document(state, &ctx);
    app.editor.as_mut().expect("the document is open").insert_silence(0, 100).expect("the Insert Silence");
    app.save(&ctx, false, Some(path.clone()), false).expect("the save starts");
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while app.saving.is_some() {
        assert!(Instant::now() < deadline, "the save did not finish");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.finish_saving(ui.ctx()));
        output.textures_delta.clear();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(path.is_file(), "the save wrote its file: {:?} at {}", app.error, path.display());
    assert!(!app.unsaved(), "the saved document is clean");
}

/// Run one frame of the window with a viewport `Close` event in its input, and return the viewport
/// commands that frame sent.
fn one_frame_with_close_request(app: &mut WaveApp) -> Vec<egui::ViewportCommand> {
    let ctx = app.ctx.clone();
    let mut input = egui::RawInput::default();
    input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    let mut output = ctx.run_ui(input, |ui| app.close_prompt(ui.ctx()));
    output.textures_delta.clear();
    output.viewport_output.values().flat_map(|v| v.commands.iter().cloned()).collect()
}

#[test]
fn closing_with_unsaved_changes_asks_first() {
    let scratch = Scratch::new("asks-first");
    let mut app = a_window(&scratch);
    a_document_with_unsaved_changes(&mut app);

    let sent = one_frame_with_close_request(&mut app);

    assert!(app.confirm_close, "the save prompt is up");
    assert!(sent.iter().any(|c| matches!(c, egui::ViewportCommand::CancelClose)), "the close was held back: {sent:?}");
    let editor = app.editor.as_ref().expect("the document is still open");
    assert_eq!(editor.history.current().len_frames(), 100, "the edit is still in the document");
}

#[test]
fn a_clean_document_closes_without_a_prompt() {
    let scratch = Scratch::new("clean-closes");
    let mut app = a_window(&scratch);
    a_saved_document(&mut app, scratch.0.join("clean.wav"));

    let sent = one_frame_with_close_request(&mut app);

    assert!(!app.confirm_close, "no save prompt for a clean document");
    assert!(!sent.iter().any(|c| matches!(c, egui::ViewportCommand::CancelClose)), "the close was not held back: {sent:?}");
}
