//! Locked tests for #1000. The builder must not edit this file.
//!
//! Hook-up, both in apps/wavcraft/src/app.rs:
//! - at the end of the file add
//!   `#[cfg(test)] #[path = "issue1000_tests.rs"] mod issue1000_tests;`
//! - add `fn consume_record_now(ctx: &egui::Context) -> bool` next to `pressed_commands`. It consumes
//!   every Shift+Space key event of the frame (presses and auto-repeats) and returns true only when
//!   the frame holds a fresh press (`pressed: true, repeat: false`, shift held). `ui()` calls it for
//!   the Shift+Space block, behind its existing `!ctx.egui_wants_keyboard_input()` gate.
use super::{Command, Keymap, consume_record_now, pressed_commands};

use egui::Key;

/// egui sets `repeat` itself: a press of a key that is already down is a repeat, so a repeat event
/// only means something in a frame after the key's first press.
fn space(shift: bool, repeat: bool) -> egui::Event {
    let modifiers = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
    egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat, modifiers }
}

/// One frame with `events`: what `consume_record_now` returns, then what the keymap still sees.
fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> (bool, Vec<Command>) {
    let keymap = Keymap::new();
    let mut out = (false, Vec::new());
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        out.0 = consume_record_now(ui.ctx());
        out.1 = pressed_commands(ui.ctx(), &keymap, false, false);
    });
    output.textures_delta.clear();
    out
}

#[test]
fn a_fresh_shift_space_press_acts() {
    let ctx = egui::Context::default();
    assert!(frame(&ctx, vec![space(true, false)]).0);
}

#[test]
fn a_shift_space_repeat_event_does_nothing() {
    let ctx = egui::Context::default();
    let _ = frame(&ctx, vec![space(true, false)]);
    assert!(!frame(&ctx, vec![space(true, true)]).0);
}

#[test]
fn a_held_shift_space_acts_once_over_a_press_and_many_repeats() {
    let ctx = egui::Context::default();
    let mut acted = 0;
    acted += usize::from(frame(&ctx, vec![space(true, false)]).0);
    for _ in 0..10 {
        acted += usize::from(frame(&ctx, vec![space(true, true)]).0);
    }
    assert_eq!(acted, 1);
}

#[test]
fn shift_space_repeat_events_are_consumed_so_play_stop_does_not_see_them() {
    let ctx = egui::Context::default();
    let _ = frame(&ctx, vec![space(true, false)]);
    let (_, commands) = frame(&ctx, vec![space(true, true)]);
    assert!(commands.is_empty(), "{commands:?}");
}

#[test]
fn a_fresh_shift_space_press_is_consumed_so_play_stop_does_not_see_it() {
    let ctx = egui::Context::default();
    let (_, commands) = frame(&ctx, vec![space(true, false)]);
    assert!(commands.is_empty(), "{commands:?}");
}

#[test]
fn a_plain_space_press_does_not_record_and_still_reaches_play_stop() {
    let ctx = egui::Context::default();
    let (acted, commands) = frame(&ctx, vec![space(false, false)]);
    assert!(!acted);
    assert_eq!(commands, vec![Command::PlayStop]);
}

#[test]
fn the_record_offset_preference_row_no_longer_says_placeholder() {
    let app = include_str!("app.rs");
    assert!(!app.contains("Record offset (placeholder"), "the Record offset row still says placeholder");
    assert!(app.contains("ui.label(\"Record offset\");"), "the Record offset row is missing");
}
