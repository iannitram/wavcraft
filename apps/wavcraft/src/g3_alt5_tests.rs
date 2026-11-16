//! G3.4: Alt+5 reaches the Match Loudness panel toggle through the window's key pass, and the
//! other channel keys still toggle their channels.

use super::*;

fn alt_key(key: Key) -> Vec<Command> {
    let ctx = egui::Context::default();
    let events = vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::ALT }];
    let mut found = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        found = pressed_commands(ui.ctx(), &Keymap::new(), false);
    });
    output.textures_delta.clear();
    found
}

#[test]
fn alt_5_in_the_window_runs_the_match_panel_toggle() {
    assert_eq!(alt_key(Key::Num5), vec![Command::Action(ActionId::ToggleMatchPanel)]);
}

#[test]
fn alt_1_to_4_and_6_to_8_still_toggle_their_channels() {
    for (key, ch) in [(Key::Num1, 0u8), (Key::Num2, 1), (Key::Num3, 2), (Key::Num4, 3), (Key::Num6, 5), (Key::Num7, 6), (Key::Num8, 7)] {
        assert_eq!(alt_key(key), vec![Command::Action(ActionId::ToggleChannel(ch))], "{key:?}");
    }
}
