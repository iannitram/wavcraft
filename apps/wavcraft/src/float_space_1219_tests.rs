//! Tests for #1219: keyboard focus inside a floated panel's own viewport.
//!
//! The floated panels are drawn by the real float loop, `WaveApp::floating_windows`, which records
//! the focus of each floated viewport (`float_typing`, `float_other_focus`). Each frame here runs the
//! window's order: the root reads the keys with the flags the last frame left (`pressed_commands`),
//! then the real float loop draws the floated panels. The panel is "files": its Dock button comes
//! first, then other widgets, then its filter text field: Tab once focuses the button, and Tab on until
//! the field has focus.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "float_space_1219_tests.rs"] mod float_space_1219_tests;`
use super::*;

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-1219-{}-{name}", std::process::id()));
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

/// The window as the program builds it, with its session folder in `scratch`, and the "files"
/// panel floated in its own window. Its settings file and recent-files list are cut loose.
fn a_window_with_a_float(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder");
    app.settings_path = None;
    app.recent_path = None;
    app.layout.float("files", Some(([40.0, 40.0], [320.0, 260.0]))).expect("the files panel floats");
    app
}

/// One frame as the window runs it, inside one run: the root reads the keys with the float flags
/// the previous frame left (`pressed_commands`), then the real float loop draws the floated panels,
/// which sets the flags for the next frame.
fn frame(app: &mut WaveApp, events: Vec<egui::Event>) -> Vec<Command> {
    let ctx = app.ctx.clone();
    let mut got = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        got = pressed_commands(ui.ctx(), &app.keymap, app.float_typing, app.float_other_focus);
        app.floating_windows(ui.ctx());
    });
    output.textures_delta.clear();
    got
}

fn key(key: Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

fn files_still_float(app: &WaveApp) -> bool {
    app.layout.floats(&app.panels).iter().any(|f| f.id == "files")
}

#[test]
fn space_on_a_focused_button_in_a_float_plays_and_does_not_click() {
    let scratch = Scratch::new("button");
    let mut app = a_window_with_a_float(&scratch);
    let _ = frame(&mut app, Vec::new());
    let _ = frame(&mut app, Vec::new());
    let _ = frame(&mut app, vec![key(Key::Tab)]);
    assert!(app.float_other_focus && !app.float_typing, "the float's Dock button has focus, and it is not typing");
    let got = frame(&mut app, vec![key(Key::Space)]);
    assert_eq!(got, vec![Command::PlayStop]);
    assert!(files_still_float(&app), "the Dock button was not clicked, so the panel did not dock back");
}

#[test]
fn space_in_a_focused_text_field_in_a_float_types_and_does_not_toggle_playback() {
    let scratch = Scratch::new("text");
    let mut app = a_window_with_a_float(&scratch);
    let _ = frame(&mut app, Vec::new());
    let _ = frame(&mut app, Vec::new());
    // Tab on until the filter field has the keyboard: the panel's other focusable widgets come first.
    for _ in 0..6 {
        if app.float_typing {
            break;
        }
        let _ = frame(&mut app, vec![key(Key::Tab)]);
        let _ = frame(&mut app, Vec::new());
    }
    assert!(app.float_typing, "the float's filter field has focus, and it is typing");
    let space_text = egui::Event::Text(" ".to_string());
    let got = frame(&mut app, vec![key(Key::Space), space_text]);
    assert!(got.is_empty());
    assert_eq!(app.files.filter, " ");
}

/// What egui does with Space on a focused button when the app reads no key first: it clicks.
#[test]
fn egui_clicks_a_focused_button_on_space_when_nothing_takes_the_key() {
    let ctx = egui::Context::default();
    let mut clicked = false;
    let vid = egui::ViewportId::from_hash_of(("float", "1219-probe"));
    let mut frame = |events: Vec<egui::Event>, want_focus: bool| {
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            ui.ctx().show_viewport_immediate(vid, egui::ViewportBuilder::default(), |ui, _class| {
                let r = ui.button("Go");
                if want_focus {
                    r.request_focus();
                }
                clicked = r.clicked();
            });
        });
        output.textures_delta.clear();
    };
    frame(Vec::new(), true);
    frame(Vec::new(), false);
    frame(vec![key(Key::Space)], false);
    assert!(clicked);
}
