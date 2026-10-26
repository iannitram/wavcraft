//! The shortcut table (roadmap A295): one test per default shortcut, that its chord resolves to
//! the right command, plus the checks on the table as a whole.

use std::collections::HashSet;

use wavcraft_core::actions::{ActionId, TABLE};
use wavcraft_core::shortcuts::{BINDINGS, Command, Keymap, Shortcut, conflicts};

fn resolves(chord: &str, expected: Command) {
    let map = Keymap::new();
    assert_eq!(map.resolve_text(chord), Ok(expected), "{chord}");
    // The Mac spelling of the same chord is the same chord.
    let mac = chord.replace("Ctrl", "Cmd");
    assert_eq!(map.resolve_text(&mac), Ok(expected), "{mac}");
}

macro_rules! shortcuts {
    ($($name:ident: $chord:literal => $command:expr;)*) => {
        $( #[test] fn $name() { resolves($chord, $command); } )*
        /// Every chord the tests above name, with its command.
        const LISTED: &[(&str, Command)] = &[$(($chord, $command)),*];
    };
}

use Command::{Action as A, Open, PlayStop, Save, SaveAs};

shortcuts! {
    open_ctrl_o: "Ctrl+O" => Open;
    save_ctrl_s: "Ctrl+S" => Save;
    save_as_ctrl_shift_s: "Ctrl+Shift+S" => SaveAs;
    play_stop_space: "Space" => PlayStop;
    undo_ctrl_z: "Ctrl+Z" => A(ActionId::Undo);
    redo_ctrl_shift_z: "Ctrl+Shift+Z" => A(ActionId::Redo);
    redo_ctrl_y: "Ctrl+Y" => A(ActionId::Redo);
    repeat_last_f3: "F3" => A(ActionId::RepeatLast);
    repeat_last_shift_r: "Shift+R" => A(ActionId::RepeatLast);
    repeat_last_dialog_f2: "F2" => A(ActionId::RepeatLastDialog);
    repeat_last_dialog_ctrl_r: "Ctrl+R" => A(ActionId::RepeatLastDialog);
    cut_ctrl_x: "Ctrl+X" => A(ActionId::Cut);
    copy_ctrl_c: "Ctrl+C" => A(ActionId::Copy);
    paste_ctrl_v: "Ctrl+V" => A(ActionId::Paste);
    capture_noise_print_shift_p: "Shift+P" => A(ActionId::CaptureNoisePrint);
    delete_delete: "Delete" => A(ActionId::Delete);
    delete_backspace: "Backspace" => A(ActionId::Delete);
    crop_ctrl_t: "Ctrl+T" => A(ActionId::Crop);
    select_all_ctrl_a: "Ctrl+A" => A(ActionId::SelectAll);
    deselect_all_ctrl_shift_a: "Ctrl+Shift+A" => A(ActionId::DeselectAll);
    deselect_all_ctrl_d: "Ctrl+D" => A(ActionId::DeselectAll);
    clear_time_selection_g: "G" => A(ActionId::ClearTimeSelection);
    zero_inward_shift_i: "Shift+I" => A(ActionId::ZeroInward);
    zero_outward_shift_o: "Shift+O" => A(ActionId::ZeroOutward);
    zero_left_to_left_shift_h: "Shift+H" => A(ActionId::ZeroLeftToLeft);
    zero_left_to_right_shift_j: "Shift+J" => A(ActionId::ZeroLeftToRight);
    zero_right_to_left_shift_k: "Shift+K" => A(ActionId::ZeroRightToLeft);
    zero_right_to_right_shift_l: "Shift+L" => A(ActionId::ZeroRightToRight);
    set_in_i: "I" => A(ActionId::SetIn);
    set_out_o: "O" => A(ActionId::SetOut);
    left_channel_up: "ArrowUp" => A(ActionId::LeftChannel);
    right_channel_down: "ArrowDown" => A(ActionId::RightChannel);
    cursor_to_start_home: "Home" => A(ActionId::CursorToStart);
    cursor_to_end_end: "End" => A(ActionId::CursorToEnd);
    zoom_in_equals: "=" => A(ActionId::ZoomIn);
    zoom_out_minus: "-" => A(ActionId::ZoomOut);
    zoom_to_selection_shift_s: "Shift+S" => A(ActionId::ZoomToSelection);
    zoom_out_full_ctrl_backslash: "Ctrl+\\" => A(ActionId::ZoomOutFull);
    tool_select_t: "T" => A(ActionId::ToolSelect);
    tool_hand_h: "H" => A(ActionId::ToolHand);
    tool_zoom_z: "Z" => A(ActionId::ToolZoom);
}

#[test]
fn the_default_table_has_no_chord_on_two_commands() {
    let found = conflicts(BINDINGS);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(Keymap::new().check(), Ok(()));
}

#[test]
fn a_conflict_is_detected_and_named() {
    // Ctrl+Z on Redo as well as Undo: the check must say so.
    let two = [BINDINGS[4], wavcraft_core::shortcuts::Binding { command: A(ActionId::Redo), chord: BINDINGS[4].chord }];
    assert_eq!(conflicts(&two).len(), 1);
    // The same command twice on one chord is not a conflict.
    assert!(conflicts(&[BINDINGS[4], BINDINGS[4]]).is_empty());
}

#[test]
fn every_row_of_the_table_has_a_test_above() {
    assert_eq!(LISTED.len(), BINDINGS.len(), "a binding was added or removed without its test");
    let map = Keymap::new();
    for b in BINDINGS {
        let listed = LISTED.iter().any(|(text, cmd)| Shortcut::parse(text) == Ok(b.chord) && *cmd == b.command);
        assert!(listed, "{} on {} has no test", b.chord, b.command.name());
        assert_eq!(map.resolve(b.chord), Some(b.command));
    }
}

#[test]
fn every_default_shortcut_resolves_to_a_registered_action() {
    let ids: HashSet<_> = TABLE.iter().map(|a| a.id).collect();
    let map = Keymap::new();
    for b in BINDINGS {
        if let Command::Action(id) = b.command {
            assert!(ids.contains(&id), "{id:?} is not in the action table");
        }
        assert_eq!(map.resolve(b.chord), Some(b.command), "{} resolves", b.chord);
    }
}

#[test]
fn every_action_in_a_binding_exists_and_command_names_are_unique() {
    let ids: HashSet<_> = TABLE.iter().map(|a| a.id).collect();
    let mut names = HashSet::new();
    for b in BINDINGS {
        if let Command::Action(id) = b.command {
            assert!(ids.contains(&id), "{id:?} is not in the action table");
        }
    }
    for c in [Open, Save, SaveAs, PlayStop] {
        assert!(names.insert(c.name()));
        assert!(TABLE.iter().all(|a| a.name != c.name()), "{} clashes with an action name", c.name());
    }
}

#[test]
fn menus_show_the_first_chord_in_each_platforms_spelling() {
    let map = Keymap::new();
    assert_eq!(map.menu_text(A(ActionId::Undo), false), "Ctrl+Z");
    assert_eq!(map.menu_text(A(ActionId::Undo), true), "⌘Z");
    assert_eq!(map.menu_text(A(ActionId::Redo), false), "Ctrl+Shift+Z");
    assert_eq!(map.menu_text(SaveAs, true), "⇧⌘S");
    assert_eq!(map.menu_text(A(ActionId::Silence), false), "", "Audition has no key for Silence");
}

#[test]
fn chords_parse_in_any_case_and_order_and_bad_ones_are_refused() {
    assert_eq!(Shortcut::parse("shift+ctrl+z"), Shortcut::parse("Ctrl+Shift+Z"));
    assert_eq!(Shortcut::parse("Cmd+Z"), Shortcut::parse("Ctrl+Z"));
    assert_eq!(Shortcut::parse("space").map(|c| c.key), Ok("Space"));
    assert!(Shortcut::parse("Ctrl+Nonsense").is_err());
    assert!(Shortcut::parse("Hyper+Z").is_err());
    assert!(Shortcut::parse("").is_err());
    assert!(Keymap::new().resolve_text("Ctrl+Shift+Alt+F12").is_err(), "unbound");
}

#[test]
fn an_override_replaces_a_command_s_chords_and_frees_the_old_ones() {
    let new = Shortcut::parse("Ctrl+Alt+U").unwrap();
    let map = Keymap::with_overrides(&[(new, A(ActionId::Undo))]).unwrap();
    assert_eq!(map.resolve(new), Some(A(ActionId::Undo)));
    assert_eq!(map.resolve_text("Ctrl+Z"), Err("no command is bound to Ctrl+Z".to_string()));
    // Taking a chord another command has moves it; two overrides on one chord are refused.
    let taken = Shortcut::parse("Ctrl+X").unwrap();
    let map = Keymap::with_overrides(&[(taken, A(ActionId::Paste))]).unwrap();
    assert_eq!(map.resolve(taken), Some(A(ActionId::Paste)));
    assert!(map.chords(A(ActionId::Cut)).next().is_none());
    assert!(Keymap::with_overrides(&[(new, A(ActionId::Undo)), (new, A(ActionId::Redo))]).is_err());
}
