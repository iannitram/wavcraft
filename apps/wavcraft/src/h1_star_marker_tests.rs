//! Tests for H1.1 (#1178): a typed `*` adds a marker. egui 0.36 has no key for numpad multiply, so
//! `pressed_commands` reads the `*` text event as Add Marker when no text field has the keyboard.
use super::*;

/// One frame with `events` and no focused widget; `in_float` is passed as is.
fn press_events(events: Vec<egui::Event>) -> Vec<Command> {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut got = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        got = pressed_commands(ui.ctx(), &keymap, false, false);
    });
    output.textures_delta.clear();
    got
}

#[test]
fn a_typed_star_adds_a_marker() {
    assert_eq!(press_events(vec![egui::Event::Text("*".to_string())]), vec![Command::Action(ActionId::MarkerAdd)]);
    assert_eq!(press_events(vec![egui::Event::Text("8".to_string())]), Vec::<Command>::new());

    // With a text field focused, the `*` is typed into it and adds no marker.
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut text = String::from("10");
    let mut frame = |events: Vec<egui::Event>, ask_focus: bool| -> Vec<Command> {
        let raw = egui::RawInput { events, ..Default::default() };
        let mut found = Vec::new();
        let mut output = ctx.run_ui(raw, |ui| {
            let r = ui.text_edit_singleline(&mut text);
            if ask_focus {
                r.request_focus();
            }
            found = pressed_commands(ui.ctx(), &keymap, false, false);
        });
        output.textures_delta.clear();
        found
    };
    let _ = frame(Vec::new(), true);
    let _ = frame(Vec::new(), false);
    assert_eq!(frame(vec![egui::Event::Text("*".to_string())], false), Vec::<Command>::new());
}
