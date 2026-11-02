//! Locked tests for #1002. The builder must not edit this file.
//!
//! Hook-up, both in apps/wavcraft/src/app.rs:
//! - at the end of the file add
//!   `#[cfg(test)] #[path = "issue1002_tests.rs"] mod issue1002_tests;`
//! - the brief's `labels` list becomes the free function `tool_labels(keymap: &Keymap, mac: bool) -> Vec<(Tool, String)>`,
//!   in the bar's order Select, Hand, Zoom, Marquee. `tools_bar` loops over what it returns.
use super::{Command, Keymap, Shortcut, Tool, tool_labels};

use wavcraft_core::actions::ActionId;

fn texts(keymap: &Keymap, mac: bool) -> Vec<String> {
    tool_labels(keymap, mac).into_iter().map(|(_, label)| label).collect()
}

#[test]
fn the_tools_come_in_the_bars_order() {
    let tools: Vec<Tool> = tool_labels(&Keymap::new(), false).into_iter().map(|(t, _)| t).collect();
    assert_eq!(tools, vec![Tool::Select, Tool::Hand, Tool::Zoom, Tool::Marquee]);
}

#[test]
fn the_default_bindings_give_time_selection_t_hand_h_zoom_z_marquee_e() {
    assert_eq!(texts(&Keymap::new(), false), vec!["Time Selection (T)", "Hand (H)", "Zoom (Z)", "Marquee (E)"]);
}

#[test]
fn the_labels_do_not_show_v_for_the_time_selection_tool() {
    assert!(texts(&Keymap::new(), false).iter().all(|l| !l.contains("(V)")));
}

#[test]
fn the_labels_follow_the_mac_key_text() {
    let keymap = Keymap::new();
    let want: Vec<String> =
        [(ActionId::ToolSelect, "Time Selection"), (ActionId::ToolHand, "Hand"), (ActionId::ToolZoom, "Zoom"), (ActionId::ToolMarquee, "Marquee")]
            .into_iter()
            .map(|(id, name)| format!("{name} ({})", keymap.menu_text(Command::Action(id), true)))
            .collect();
    assert_eq!(texts(&keymap, true), want);
}

#[test]
fn a_rebound_tool_shows_its_new_key() {
    let mut keymap = Keymap::new();
    keymap.assign(Command::Action(ActionId::ToolHand), Shortcut::parse("Ctrl+Alt+Shift+H").unwrap()).unwrap();
    assert_eq!(texts(&keymap, false), vec!["Time Selection (T)", "Hand (Ctrl+Alt+Shift+H)", "Zoom (Z)", "Marquee (E)"]);
}

#[test]
fn a_tool_with_no_key_shows_just_its_name() {
    let mut keymap = Keymap::new();
    keymap.clear(Command::Action(ActionId::ToolZoom));
    assert_eq!(texts(&keymap, false), vec!["Time Selection (T)", "Hand (H)", "Zoom", "Marquee (E)"]);
}

#[test]
fn the_tools_bar_source_has_no_hard_coded_key_letters() {
    let app = include_str!("app.rs");
    for fixed in ["Time Selection (V)", "Hand (H)", "Zoom (Z)", "Marquee (E)"] {
        assert!(!app.contains(&format!("\"{fixed}\"")), "app.rs still has the fixed label {fixed:?}");
    }
}
