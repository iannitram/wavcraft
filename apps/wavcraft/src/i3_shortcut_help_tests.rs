//! Locked I3 expectations. The builder must not edit this file.
use super::{pressed_actions, shortcut_help_content, shortcut_help_rows};

use egui::Key;
use wavcraft_core::actions::{self, ActionId};

fn key_action(key: Key, cmd: bool, shift: bool) -> Vec<ActionId> {
    let ctx = egui::Context::default();
    let mut modifiers = egui::Modifiers::NONE;
    if cmd {
        modifiers |= egui::Modifiers::COMMAND;
    }
    if shift {
        modifiers |= egui::Modifiers::SHIFT;
    }
    let events = vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }];
    let mut found = Vec::new();
    let _ = ctx.run_ui(egui::RawInput { events, modifiers, ..Default::default() }, |ui| {
        found = pressed_actions(ui.ctx());
    });
    found
}

macro_rules! shortcut_case {
    ($test:ident, $id:ident, $name:literal, $label:literal, $key:ident, $cmd:literal, $shift:literal, $win:literal, $mac:literal) => {
        #[test]
        fn $test() {
            assert!(shortcut_help_rows(false).contains(&($name, $label, $win.to_string())));
            assert!(shortcut_help_rows(true).contains(&($name, $label, $mac.to_string())));
            assert_eq!(actions::by_name($name).map(|row| row.id), Some(ActionId::$id));
            assert_eq!(key_action(Key::$key, $cmd, $shift), vec![ActionId::$id]);
        }
    };
}

#[rustfmt::skip]
shortcut_case!(
    i3_help_undo, Undo, "edit.undo", "Undo",
    Z, true, false, "Ctrl+Z", "⌘Z"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_redo_shift_z, Redo, "edit.redo", "Redo",
    Z, true, true, "Ctrl+Shift+Z", "⇧⌘Z"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_redo_y, Redo, "edit.redo", "Redo",
    Y, true, false, "Ctrl+Y", "⌘Y"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_repeat_f3, RepeatLast, "edit.repeatLast", "Repeat Last Command",
    F3, false, false, "F3", "F3"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_repeat_shift_r, RepeatLast, "edit.repeatLast", "Repeat Last Command",
    R, false, true, "Shift+R", "⇧R"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_repeat_dialog_f2, RepeatLastDialog, "edit.repeatLastDialog", "Repeat Last Command…",
    F2, false, false, "F2", "F2"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_repeat_dialog_ctrl_r, RepeatLastDialog, "edit.repeatLastDialog", "Repeat Last Command…",
    R, true, false, "Ctrl+R", "⌘R"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_cut, Cut, "edit.cut", "Cut",
    X, true, false, "Ctrl+X", "⌘X"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_copy, Copy, "edit.copy", "Copy",
    C, true, false, "Ctrl+C", "⌘C"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_paste, Paste, "edit.paste", "Paste",
    V, true, false, "Ctrl+V", "⌘V"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_delete, Delete, "edit.delete", "Delete",
    Delete, false, false, "Delete", "⌦"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_backspace, Delete, "edit.delete", "Delete",
    Backspace, false, false, "Backspace", "⌫"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_crop, Crop, "edit.crop", "Crop",
    T, true, false, "Ctrl+T", "⌘T"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_select_all, SelectAll, "edit.selectAll", "Select All",
    A, true, false, "Ctrl+A", "⌘A"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_deselect_shift_a, DeselectAll, "edit.deselectAll", "Deselect All",
    A, true, true, "Ctrl+Shift+A", "⇧⌘A"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_deselect_ctrl_d, DeselectAll, "edit.deselectAll", "Deselect All",
    D, true, false, "Ctrl+D", "⌘D"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_clear_time, ClearTimeSelection, "edit.clearTimeSelection", "Clear Time Selection",
    G, false, false, "G", "G"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_inward, ZeroInward, "edit.zeroCrossings.inward", "Zero Crossings: Adjust Selection Inward",
    I, false, true, "Shift+I", "⇧I"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_outward, ZeroOutward, "edit.zeroCrossings.outward", "Zero Crossings: Adjust Selection Outward",
    O, false, true, "Shift+O", "⇧O"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_left_left, ZeroLeftToLeft, "edit.zeroCrossings.leftToLeft", "Zero Crossings: Left Side to Left",
    H, false, true, "Shift+H", "⇧H"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_left_right, ZeroLeftToRight, "edit.zeroCrossings.leftToRight", "Zero Crossings: Left Side to Right",
    J, false, true, "Shift+J", "⇧J"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_right_left, ZeroRightToLeft, "edit.zeroCrossings.rightToLeft", "Zero Crossings: Right Side to Left",
    K, false, true, "Shift+K", "⇧K"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zero_right_right, ZeroRightToRight, "edit.zeroCrossings.rightToRight", "Zero Crossings: Right Side to Right",
    L, false, true, "Shift+L", "⇧L"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_set_in, SetIn, "edit.setIn", "Set In Point to Playhead",
    I, false, false, "I", "I"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_set_out, SetOut, "edit.setOut", "Set Out Point to Playhead",
    O, false, false, "O", "O"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_left_channel, LeftChannel, "edit.leftChannel", "Left Channel Only",
    ArrowUp, false, false, "↑", "↑"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_right_channel, RightChannel, "edit.rightChannel", "Right Channel Only",
    ArrowDown, false, false, "↓", "↓"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_home, CursorToStart, "view.cursorToStart", "Playhead to Start",
    Home, false, false, "Home", "Home"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_end, CursorToEnd, "view.cursorToEnd", "Playhead to End",
    End, false, false, "End", "End"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zoom_in, ZoomIn, "view.zoomIn", "Zoom In",
    Equals, false, false, "=", "="
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zoom_out, ZoomOut, "view.zoomOut", "Zoom Out",
    Minus, false, false, "−", "−"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zoom_selection, ZoomToSelection, "view.zoomToSelection", "Zoom to Selection",
    S, false, true, "Shift+S", "⇧S"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zoom_full, ZoomOutFull, "view.zoomOutFull", "Zoom Out Full",
    Backslash, true, false, "Ctrl+\\", "⌘\\"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_time_selection, ToolSelect, "tool.select", "Time Selection Tool",
    T, false, false, "T", "T"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_hand, ToolHand, "tool.hand", "Hand Tool",
    H, false, false, "H", "H"
);

#[rustfmt::skip]
shortcut_case!(
    i3_help_zoom_tool, ToolZoom, "tool.zoom", "Zoom Tool",
    Z, false, false, "Z", "Z"
);

#[test]
#[rustfmt::skip]
fn i3_help_rows_are_in_menu_order_with_no_unbound_or_extra_rows() {
    assert_eq!(shortcut_help_rows(false), vec![
        ("edit.undo", "Undo", "Ctrl+Z".to_string()),
        ("edit.redo", "Redo", "Ctrl+Shift+Z".to_string()),
        ("edit.redo", "Redo", "Ctrl+Y".to_string()),
        ("edit.repeatLast", "Repeat Last Command", "F3".to_string()),
        ("edit.repeatLast", "Repeat Last Command", "Shift+R".to_string()),
        ("edit.repeatLastDialog", "Repeat Last Command…", "F2".to_string()),
        ("edit.repeatLastDialog", "Repeat Last Command…", "Ctrl+R".to_string()),
        ("edit.cut", "Cut", "Ctrl+X".to_string()),
        ("edit.copy", "Copy", "Ctrl+C".to_string()),
        ("edit.paste", "Paste", "Ctrl+V".to_string()),
        ("edit.delete", "Delete", "Delete".to_string()),
        ("edit.delete", "Delete", "Backspace".to_string()),
        ("edit.crop", "Crop", "Ctrl+T".to_string()),
        ("edit.selectAll", "Select All", "Ctrl+A".to_string()),
        ("edit.deselectAll", "Deselect All", "Ctrl+Shift+A".to_string()),
        ("edit.deselectAll", "Deselect All", "Ctrl+D".to_string()),
        ("edit.clearTimeSelection", "Clear Time Selection", "G".to_string()),
        ("edit.zeroCrossings.inward", "Zero Crossings: Adjust Selection Inward", "Shift+I".to_string()),
        ("edit.zeroCrossings.outward", "Zero Crossings: Adjust Selection Outward", "Shift+O".to_string()),
        ("edit.zeroCrossings.leftToLeft", "Zero Crossings: Left Side to Left", "Shift+H".to_string()),
        ("edit.zeroCrossings.leftToRight", "Zero Crossings: Left Side to Right", "Shift+J".to_string()),
        ("edit.zeroCrossings.rightToLeft", "Zero Crossings: Right Side to Left", "Shift+K".to_string()),
        ("edit.zeroCrossings.rightToRight", "Zero Crossings: Right Side to Right", "Shift+L".to_string()),
        ("edit.setIn", "Set In Point to Playhead", "I".to_string()),
        ("edit.setOut", "Set Out Point to Playhead", "O".to_string()),
        ("edit.leftChannel", "Left Channel Only", "↑".to_string()),
        ("edit.rightChannel", "Right Channel Only", "↓".to_string()),
        ("view.cursorToStart", "Playhead to Start", "Home".to_string()),
        ("view.cursorToEnd", "Playhead to End", "End".to_string()),
        ("view.zoomIn", "Zoom In", "=".to_string()),
        ("view.zoomOut", "Zoom Out", "−".to_string()),
        ("view.zoomToSelection", "Zoom to Selection", "Shift+S".to_string()),
        ("view.zoomOutFull", "Zoom Out Full", "Ctrl+\\".to_string()),
        ("tool.select", "Time Selection Tool", "T".to_string()),
        ("tool.hand", "Hand Tool", "H".to_string()),
        ("tool.zoom", "Zoom Tool", "Z".to_string()),
    ]);
}

fn rendered_text(mac: bool) -> Vec<String> {
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| shortcut_help_content(ui, mac));
    let mut labels = Vec::new();
    for shape in &output.shapes {
        if let egui::epaint::Shape::Text(text) = &shape.shape {
            labels.push(text.galley.text().to_string());
        }
    }
    labels
}

#[test]
fn i3_help_content_draws_windows_labels_and_keys() {
    let text = rendered_text(false);
    assert!(text.contains(&"Undo".to_string()));
    assert!(text.contains(&"Ctrl+Z".to_string()));
}

#[test]
fn i3_help_content_draws_mac_labels_and_keys() {
    let text = rendered_text(true);
    assert!(text.contains(&"Undo".to_string()));
    assert!(text.contains(&"⌘Z".to_string()));
}
