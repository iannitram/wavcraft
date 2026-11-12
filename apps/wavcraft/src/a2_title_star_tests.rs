//! Tests for A2.1 (#1154): the window title is `window_title(name, unsaved, open)`, and `send_title`
//! sends it only when it changes.
//!
//! The first test checks the pure function. The second runs `send_title` on a real window
//! (`WaveApp::new` on eframe's headless creation context) through an edit and a save, and reads the
//! title the window sent in each frame.
use super::*;

#[test]
fn window_title_marks_unsaved() {
    assert_eq!(window_title("take.wav", true, true), "*take.wav — WavCraft");
    assert_eq!(window_title("take.wav", false, true), "take.wav — WavCraft");
    assert_eq!(window_title("take.wav", true, false), "WavCraft");
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-a2-title-star-{}-{name}", std::process::id()));
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

/// The last title the window sent in `sent`, if it sent one.
fn last_title(sent: &[egui::ViewportCommand]) -> Option<String> {
    sent.iter().rev().find_map(|c| match c {
        egui::ViewportCommand::Title(t) => Some(t.clone()),
        _ => None,
    })
}

/// Run `send_title` in one frame and return the viewport commands that frame sent.
fn title_frame(app: &mut WaveApp, ctx: &egui::Context) -> Vec<egui::ViewportCommand> {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.send_title(ui.ctx()));
    output.textures_delta.clear();
    output.viewport_output.values().flat_map(|v| v.commands.iter().cloned()).collect()
}

#[test]
fn the_title_sent_to_the_window_follows_edit_and_save() {
    let scratch = Scratch::new("follows-edit-and-save");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();

    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    app.add_document(state, &ctx);
    app.editor.as_mut().expect("the document is open").insert_silence(0, 100).expect("the Insert Silence");
    assert!(app.unsaved(), "the new document is unsaved");

    let sent = title_frame(&mut app, &ctx);
    assert_eq!(last_title(&sent).as_deref(), Some("*Untitled — WavCraft"), "the title after the edit: {sent:?}");

    let path = scratch.0.join("saved.wav");
    app.save(&ctx, false, Some(path.clone()), false).expect("the save starts");
    assert!(app.saving.is_some(), "the save is running");

    let mut sent = Vec::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while app.saving.is_some() {
        assert!(Instant::now() < deadline, "the save did not finish within 10 s");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.finish_saving(ui.ctx());
            app.send_title(ui.ctx());
        });
        output.textures_delta.clear();
        sent.extend(output.viewport_output.values().flat_map(|v| v.commands.iter().cloned()));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(path.is_file(), "the save wrote its file");
    assert_eq!(last_title(&sent).as_deref(), Some("saved.wav — WavCraft"), "the title after the save: {sent:?}");
}
