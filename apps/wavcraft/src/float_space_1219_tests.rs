//! Tests for #1219: Space on a focused button in a floated panel's own viewport.
//!
//! A floated panel is drawn in its own viewport, and keyboard focus is kept per viewport, so the
//! root window cannot see it. Each frame here runs the way the window does: the root reads the keys
//! first with the float flags from the previous frame (`float_focus`, recorded inside the viewport),
//! then the viewport draws its widget.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "float_space_1219_tests.rs"] mod float_space_1219_tests;`
use super::*;

#[derive(Clone, Copy, PartialEq)]
enum Widget {
    Button,
    Text,
}

/// One frame. `flags` is `(text field focused, other widget focused)` in the floated viewport,
/// as the last frame left it; it is updated for the next frame. Returns the commands the root
/// read from the keys, and whether the floated button was clicked.
fn float_frame(
    ctx: &egui::Context,
    keymap: &Keymap,
    events: Vec<egui::Event>,
    widget: Widget,
    want_focus: bool,
    text: &mut String,
    flags: &mut (bool, bool),
) -> (Vec<Command>, bool) {
    let mut got = Vec::new();
    let mut clicked = false;
    let vid = egui::ViewportId::from_hash_of(("float", "1219-test"));
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        got = pressed_commands(ui.ctx(), keymap, flags.0, flags.1);
        ui.ctx().show_viewport_immediate(vid, egui::ViewportBuilder::default(), |ui, _class| {
            match widget {
                Widget::Button => {
                    let r = ui.button("Go");
                    if want_focus {
                        r.request_focus();
                    }
                    clicked = r.clicked();
                }
                Widget::Text => {
                    let r = ui.text_edit_singleline(text);
                    if want_focus {
                        r.request_focus();
                    }
                }
            }
            *flags = float_focus(ui.ctx());
        });
    });
    output.textures_delta.clear();
    (got, clicked)
}

fn space_press() -> egui::Event {
    egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

#[test]
fn space_on_a_focused_button_in_a_float_plays_and_does_not_click() {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut text = String::new();
    let mut flags = (false, false);
    let _ = float_frame(&ctx, &keymap, Vec::new(), Widget::Button, true, &mut text, &mut flags);
    let _ = float_frame(&ctx, &keymap, Vec::new(), Widget::Button, false, &mut text, &mut flags);
    assert_eq!(flags, (false, true), "a button in the float is focus, not typing");
    let (got, clicked) = float_frame(&ctx, &keymap, vec![space_press()], Widget::Button, false, &mut text, &mut flags);
    assert_eq!(got, vec![Command::PlayStop]);
    assert!(!clicked);
}

#[test]
fn space_in_a_focused_text_field_in_a_float_types_and_does_not_toggle_playback() {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let mut text = String::new();
    let mut flags = (false, false);
    let _ = float_frame(&ctx, &keymap, Vec::new(), Widget::Text, true, &mut text, &mut flags);
    let _ = float_frame(&ctx, &keymap, Vec::new(), Widget::Text, false, &mut text, &mut flags);
    assert_eq!(flags, (true, false), "a text field in the float is typing");
    let space_text = egui::Event::Text(" ".to_string());
    let (got, _) = float_frame(&ctx, &keymap, vec![space_press(), space_text], Widget::Text, false, &mut text, &mut flags);
    assert!(got.is_empty());
    assert_eq!(text, " ");
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
    frame(vec![space_press()], false);
    assert!(clicked);
}
