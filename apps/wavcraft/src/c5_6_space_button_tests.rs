//! Tests for C5.6 (#1081): Space on a focused button.
//!
//! The app reads its keys at the top of each frame (`pressed_commands`, before any widget is built),
//! so a Space press reaches Play/Stop even when a button has focus. These tests run that order: the
//! keys first, then the button, as the window does.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "c5_6_space_button_tests.rs"] mod c5_6_space_button_tests;`
use super::*;

/// Focus a button in one frame, then send events in the next. Returns the commands the keys gave
/// and whether the button was clicked.
fn events_on_a_focused_button(events: Vec<egui::Event>) -> (Vec<Command>, bool) {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.button("Go").request_focus();
    });
    output.textures_delta.clear();

    let mut got = Vec::new();
    let mut clicked = false;
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        got = pressed_commands(ui.ctx(), &keymap, false, false);
        clicked = ui.button("Go").clicked();
    });
    output.textures_delta.clear();
    (got, clicked)
}

#[test]
fn space_on_a_focused_button_plays_and_does_not_click() {
    let press = egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let (got, clicked) = events_on_a_focused_button(vec![press]);
    assert_eq!(got, vec![Command::PlayStop]);
    assert!(!clicked);
}

#[test]
fn star_on_a_focused_button_does_not_add_a_marker() {
    let (got, clicked) = events_on_a_focused_button(vec![egui::Event::Text("*".to_string())]);
    assert!(got.is_empty());
    assert!(!clicked);
}
