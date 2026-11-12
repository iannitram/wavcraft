//! Tests for #1219: Space on a focused button in a floated panel plays and does not click, as it
//! does in the main window (#1177).
//!
//! Keyboard focus is kept per viewport, so a floated panel's focus is read in its own viewport's
//! frame (`ViewportFocus::of`, as `floating_windows` reads it after drawing the panel). Each frame
//! here does what `WaveApp::update` does: the root window's keys first, with the flags the float
//! set in the previous frame, then the floated panel's viewport.
//!
//! No window or audio is needed: the keys and the focus are the whole decision, so these run on a
//! bare `egui::Context`.
use super::*;

/// What the floated panel's viewport is asked to focus this frame.
#[derive(Clone, Copy)]
enum Hold {
    Button,
    Text,
}

/// The floated panel: a "Go" button and a text field. `hold` asks one of them for focus. Returns
/// whether Go was clicked.
fn panel(ui: &mut egui::Ui, text: &mut String, hold: Option<Hold>) -> bool {
    let go = ui.button("Go");
    let field = ui.text_edit_singleline(text);
    match hold {
        Some(Hold::Button) => go.request_focus(),
        Some(Hold::Text) => field.request_focus(),
        None => {}
    }
    go.clicked()
}

/// One frame of the floated panel's own viewport, as `floating_windows` runs it. Returns the focus
/// the frame ends with, and whether Go was clicked.
fn float_frame(ctx: &egui::Context, text: &mut String, hold: Option<Hold>) -> (ViewportFocus, bool) {
    let id = egui::ViewportId::from_hash_of(("float", "test-panel"));
    let mut seen = (ViewportFocus::default(), false);
    ctx.show_viewport_immediate(id, egui::ViewportBuilder::default(), |ui, _class| {
        let clicked = panel(ui, text, hold);
        seen = (ViewportFocus::of(ui.ctx()), clicked);
    });
    seen
}

/// One frame of the app: the root window's keys (`last` is the focus the float set last frame),
/// then the floated panel. Returns the commands, the focus the float ends with, and the Go click.
fn app_frame(
    ctx: &egui::Context,
    keymap: &Keymap,
    text: &mut String,
    hold: Option<Hold>,
    last: ViewportFocus,
    events: Vec<egui::Event>,
) -> (Vec<Command>, ViewportFocus, bool) {
    let mut commands = Vec::new();
    let mut seen = (ViewportFocus::default(), false);
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        commands = pressed_commands(ui.ctx(), keymap, last.typing, last.other);
        seen = float_frame(ui.ctx(), text, hold);
    });
    output.textures_delta.clear();
    (commands, seen.0, seen.1)
}

fn space() -> egui::Event {
    egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

#[test]
fn space_with_a_button_focused_in_a_float_plays_and_does_not_click() {
    let (ctx, keymap) = (egui::Context::default(), Keymap::new());
    let mut text = String::from("10");
    // The float's button takes focus in its own frame.
    let (_, focus, _) = app_frame(&ctx, &keymap, &mut text, Some(Hold::Button), ViewportFocus::default(), Vec::new());
    assert!(!focus.typing, "a button is not typing");
    assert!(focus.other, "the button holds the float's focus");

    let (got, _, clicked) = app_frame(&ctx, &keymap, &mut text, None, focus, vec![space()]);
    assert_eq!(got, vec![Command::PlayStop]);
    assert!(!clicked, "Space does not click the button");
}

#[test]
fn space_with_a_text_field_focused_in_a_float_types_and_does_not_toggle_playback() {
    let (ctx, keymap) = (egui::Context::default(), Keymap::new());
    let mut text = String::from("10");
    let (_, focus, _) = app_frame(&ctx, &keymap, &mut text, Some(Hold::Text), ViewportFocus::default(), Vec::new());
    assert!(focus.typing, "a text field in the float is typing");
    assert!(!focus.other);

    let (got, _, _) = app_frame(&ctx, &keymap, &mut text, None, focus, vec![space(), egui::Event::Text(" ".to_string())]);
    assert!(got.is_empty(), "Space does not toggle playback: {got:?}");
    assert_eq!(text, "10 ", "Space typed a space");
}
