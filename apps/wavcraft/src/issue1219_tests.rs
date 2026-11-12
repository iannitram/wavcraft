//! The float key pass runs before its widgets, with input delivered to that frame alone.
//! Feed RawInput directly to a child viewport, without sending its Space event to the root.
use super::*;

#[derive(Clone, Copy)]
enum Focus {
    Button,
    Text,
}

fn space(repeat: bool) -> egui::Event {
    egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat, modifiers: egui::Modifiers::NONE }
}

fn panel(ui: &mut egui::Ui, text: &mut String, focus: Option<Focus>) -> bool {
    let button = ui.button("Dock");
    let field = ui.text_edit_singleline(text);
    match focus {
        Some(Focus::Button) => button.request_focus(),
        Some(Focus::Text) => field.request_focus(),
        None => {}
    }
    button.clicked()
}

fn float_input(events: Vec<egui::Event>) -> egui::RawInput {
    let viewport_id = egui::ViewportId::from_hash_of("float-input-test");
    let mut raw = egui::RawInput { viewport_id, events, ..Default::default() };
    raw.viewports.insert(viewport_id, egui::ViewportInfo { parent: Some(egui::ViewportId::ROOT), ..Default::default() });
    raw
}

fn frame(ctx: &egui::Context, text: &mut String, focus: Option<Focus>, events: Vec<egui::Event>) -> (Vec<Command>, bool) {
    let mut result = (Vec::new(), false);
    let mut output = ctx.run_ui(float_input(events), |ui| {
        assert_eq!(ui.ctx().viewport_id(), float_input(vec![]).viewport_id);
        result.0 = float_space_commands(ui.ctx(), &Keymap::new());
        result.1 = panel(ui, text, focus);
    });
    output.textures_delta.clear();
    result
}

#[test]
fn space_on_a_focused_float_button_plays_and_does_not_click() {
    let float = egui::Context::default();
    let mut text = String::from("10");
    frame(&float, &mut text, Some(Focus::Button), vec![]);
    // No Space is sent to the root: the real backend delivers it only to the OS float.
    let mut root_input = float_input(vec![]);
    root_input.viewport_id = egui::ViewportId::ROOT;
    let mut output = float.run_ui(root_input, |ui| {
        assert!(pressed_commands_with_focus(ui.ctx(), &Keymap::new(), false, true).is_empty());
    });
    output.textures_delta.clear();
    let (commands, clicked) = frame(&float, &mut text, None, vec![space(false)]);
    assert_eq!(commands, vec![Command::PlayStop]);
    assert!(!clicked, "Space must be consumed before Dock reads it");
}

#[test]
fn space_in_a_focused_float_text_field_types_without_playback() {
    let float = egui::Context::default();
    let mut text = String::from("10");
    frame(&float, &mut text, Some(Focus::Text), vec![]);
    // Let TextEdit observe the focus and settle its cursor before typing.
    frame(&float, &mut text, None, vec![]);
    let (commands, clicked) = frame(&float, &mut text, None, vec![space(false), egui::Event::Text(" ".into())]);
    assert!(commands.is_empty());
    assert!(!clicked);
    assert_eq!(text, "10 ");
}

#[test]
fn held_space_in_a_float_plays_once_and_never_clicks() {
    let float = egui::Context::default();
    let mut text = String::new();
    frame(&float, &mut text, Some(Focus::Button), vec![]);
    assert_eq!(frame(&float, &mut text, None, vec![space(false)]), (vec![Command::PlayStop], false));
    for _ in 0..10 {
        assert_eq!(frame(&float, &mut text, None, vec![space(true)]), (vec![], false));
    }
}

#[test]
fn embedded_float_space_is_consumed_once_by_the_root() {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut text = String::new();
    let mut commands = Vec::new();
    let mut clicked = false;
    for events in [vec![], vec![space(false)]] {
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            commands = pressed_commands(ui.ctx(), &keymap, false);
            ui.ctx().show_viewport_immediate(egui::ViewportId::from_hash_of("float-test"), egui::ViewportBuilder::default(), |ui, class| {
                assert!(class == egui::ViewportClass::EmbeddedWindow);
                clicked = panel(ui, &mut text, Some(Focus::Button));
            });
        });
        output.textures_delta.clear();
    }
    assert_eq!(commands, vec![Command::PlayStop]);
    assert!(!clicked);
}

#[test]
fn egui_space_without_the_float_key_pass_clicks_the_button() {
    let ctx = egui::Context::default();
    let mut text = String::new();
    frame(&ctx, &mut text, Some(Focus::Button), vec![]);
    let mut clicked = false;
    let mut output = ctx.run_ui(float_input(vec![space(false)]), |ui| {
        clicked = panel(ui, &mut text, None);
    });
    output.textures_delta.clear();
    assert!(clicked, "baseline: egui handles Space as a button click");
}

#[test]
fn other_float_focus_blocks_editing_shortcuts_but_allows_space() {
    let ctx = egui::Context::default();
    let mut commands = Vec::new();
    let mut delete = space(false);
    if let egui::Event::Key { key, .. } = &mut delete {
        *key = Key::Backspace;
    }
    let mut output = ctx.run_ui(egui::RawInput { events: vec![delete, space(false)], ..Default::default() }, |ui| {
        commands = pressed_commands_with_focus(ui.ctx(), &Keymap::new(), false, true);
    });
    output.textures_delta.clear();
    assert_eq!(commands, vec![Command::PlayStop]);
}
