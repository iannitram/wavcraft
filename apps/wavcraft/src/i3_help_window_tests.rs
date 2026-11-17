//! I3.2: `HELP shortcuts` answers `OK rows=<n>` and opens Help › Keyboard Shortcuts; the window
//! draws 'Keyboard Shortcuts', 'Undo' and 'Ctrl+Z'; the first help row is Undo on Ctrl+Z.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "i3_help_window_tests.rs"] mod i3_help_window_tests;`
use super::*;

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-i3-help-window-{}-{name}", std::process::id()));
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

fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder");
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// Draw the Help › Keyboard Shortcuts window and read back the text it drew.
///
/// A window settles its size on the first frame and paints its text on the second, so run two.
fn rendered_help_window(app: &mut WaveApp) -> Vec<String> {
    let ctx = app.ctx.clone();
    let mut labels = Vec::new();
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |_ui| app.shortcut_help_window(&ctx));
        output.textures_delta.clear();
        labels.clear();
        for shape in &output.shapes {
            if let egui::epaint::Shape::Text(text) = &shape.shape {
                labels.push(text.galley.text().to_string());
            }
        }
    }
    labels
}

#[test]
fn help_shortcuts_answers_rows_and_opens_the_window() {
    let scratch = Scratch::new("opens");
    let mut app = a_window(&scratch);
    assert_eq!(ask(&mut app, "HELP shortcuts"), "OK rows=67");
    assert!(app.show_shortcuts, "HELP shortcuts opens the window");
    let text = rendered_help_window(&mut app);
    let undo_chord = if cfg!(target_os = "macos") { "⌘Z" } else { "Ctrl+Z" };
    assert!(text.contains(&"Keyboard Shortcuts".to_string()), "{text:?}");
    assert!(text.contains(&"Undo".to_string()), "{text:?}");
    assert!(text.contains(&undo_chord.to_string()), "{text:?}");
}

#[test]
fn the_first_help_row_is_undo_on_ctrl_z() {
    let rows = shortcut_help_rows(false);
    assert_eq!(rows.len(), 67, "one row per bound chord at this commit");
    assert_eq!(rows[0], ("edit.undo", "Undo", "Ctrl+Z".to_string()));
    let mac = shortcut_help_rows(true);
    assert_eq!(mac[0].1, "Undo", "the first row's label is Undo on the Mac list too");
    assert_eq!(mac[0].2, "⌘Z");
}
