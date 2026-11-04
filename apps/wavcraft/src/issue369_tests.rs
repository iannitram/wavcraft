//! Tests for #369: Save and close must not close a document that changed while its save ran.
//!
//! The window's `finish_saving` closes the document after a Save-and-close only when the open step
//! is still the one that was saved (`closes_after_save`). Before, it closed unconditionally, so an
//! edit confirmed while the save was running (Insert Silence, say) was lost with the document.
//!
//! The first three tests check the decision. The rest run `finish_saving` itself on a real window
//! (`WaveApp::new` on eframe's headless creation context): a real Save-and-close on a worker
//! thread, an Insert Silence confirmed before it finishes, then frames until the save is done.
use super::*;

fn a_doc() -> Arc<DocState> {
    Arc::new(newdoc::empty(&NewDocOptions::default(), 44_100, 1, SampleFormat::Int16).expect("an empty document"))
}

#[test]
fn a_save_that_finished_on_the_open_step_closes_the_document() {
    let saved = a_doc();
    let current = Arc::clone(&saved);
    assert!(closes_after_save(Some(&current), &saved));
}

/// The reported case: the file was written with the state at Save, then an edit (100 frames saved,
/// 580 frames current in the report) made a new step before the save finished. The document must
/// stay open.
#[test]
fn an_edit_made_while_the_save_ran_keeps_the_document_open() {
    let saved = a_doc();
    let edited = a_doc();
    assert!(!closes_after_save(Some(&edited), &saved));
}

#[test]
fn with_no_document_open_nothing_is_kept_open() {
    let saved = a_doc();
    assert!(closes_after_save(None, &saved));
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-issue369-{}-{name}", std::process::id()));
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

/// Whether the window was told to close.
fn told_to_close(sent: &[egui::ViewportCommand]) -> bool {
    sent.iter().any(|c| matches!(c, egui::ViewportCommand::Close))
}

/// Open an unsaved document (one Insert Silence of 100 frames), start the Save-and-close the close
/// prompt starts, and run frames until `finish_saving` has taken the save's result. Returns the
/// viewport commands the window sent meanwhile.
///
/// `closing_doc`: the file's own Close (and Close All) rather than the window's close button.
/// `edit_meanwhile`: confirm another Insert Silence (480 frames) after the save began and before it
/// finishes, as in the report, so the file holds 100 frames and the document 580.
fn save_and_close(app: &mut WaveApp, scratch: &Scratch, closing_doc: bool, edit_meanwhile: bool) -> Vec<egui::ViewportCommand> {
    let ctx = app.ctx.clone();
    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    app.add_document(state, &ctx);
    app.editor.as_mut().expect("the document is open").insert_silence(0, 100).expect("the first Insert Silence");
    assert!(app.unsaved(), "the new document is unsaved");

    (app.closing_doc, app.close_all) = (closing_doc, closing_doc);
    let path = scratch.0.join("saved.wav");
    app.save(&ctx, false, Some(path.clone()), true).expect("the save starts");
    assert!(app.saving.is_some(), "the save is running");
    if edit_meanwhile {
        app.editor.as_mut().expect("the document is still open").insert_silence(100, 480).expect("the second Insert Silence");
    }

    let mut sent = Vec::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while app.saving.is_some() {
        assert!(Instant::now() < deadline, "the save did not finish");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.finish_saving(ui.ctx()));
        output.textures_delta.clear();
        sent.extend(output.viewport_output.values().flat_map(|v| v.commands.iter().cloned()));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(path.is_file(), "the save wrote its file");
    sent
}

/// The control: nothing changed during the save, so Save-and-close of the file closes it, and the
/// Close All that asked for it goes on to the next document.
#[test]
fn a_save_and_close_of_an_unchanged_document_closes_it() {
    let scratch = Scratch::new("closes");
    let mut app = a_window(&scratch);
    save_and_close(&mut app, &scratch, true, false);
    assert!(app.editor.is_none(), "the document was closed");
    assert!(!app.closing_doc);
    assert!(app.close_all, "Close All goes on to the next document");
}

/// The reported case, through `finish_saving`: the edit confirmed while the save ran is not in the
/// file, so the document stays open and unsaved and Close All stops there.
#[test]
fn an_edit_confirmed_while_save_and_close_runs_keeps_the_document_open_and_unsaved() {
    let scratch = Scratch::new("edit-keeps-open");
    let mut app = a_window(&scratch);
    save_and_close(&mut app, &scratch, true, true);

    let editor = app.editor.as_ref().expect("the document stayed open");
    assert_eq!(editor.history.current().len_frames(), 580, "the edit is still in the document");
    assert_eq!(app.saved.as_ref().map(|s| s.len_frames()), Some(100), "the saved step is the one the file holds");
    assert!(app.unsaved(), "the document is marked unsaved");
    assert!(!app.closing_doc, "the close request is dropped");
    assert!(!app.close_all, "Close All stops at this document");
    assert!(app.notice.as_deref().is_some_and(|n| n.contains("stays open and unsaved")), "the notice says why: {:?}", app.notice);
}

/// The control for the window's close button: the save finished on the open step, so the window
/// goes on closing.
#[test]
fn a_save_and_close_from_the_window_close_button_closes_the_window() {
    let scratch = Scratch::new("window-closes");
    let mut app = a_window(&scratch);
    let sent = save_and_close(&mut app, &scratch, false, false);
    assert!(told_to_close(&sent), "the window was told to close: {sent:?}");
}

/// The same edit while the window's close button is saving: the window is not closed either.
#[test]
fn an_edit_confirmed_while_the_window_close_saves_keeps_the_window_open() {
    let scratch = Scratch::new("window-edit-keeps-open");
    let mut app = a_window(&scratch);
    let sent = save_and_close(&mut app, &scratch, false, true);
    assert!(!told_to_close(&sent), "the window was not told to close: {sent:?}");
    assert!(app.editor.is_some(), "the document stayed open");
    assert!(app.unsaved(), "the document is marked unsaved");
}
