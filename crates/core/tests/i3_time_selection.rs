use wavcraft_core::actions::{action, ActionId};
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};

#[test]
fn i3_time_selection_uses_t_in_the_menu_action_table() {
    let row = action(ActionId::ToolSelect);
    assert_eq!(row.name, "tool.select");
    assert_eq!(row.menu, "Time Selection Tool");
    let keys: Vec<Shortcut> = Keymap::new().chords(Command::Action(ActionId::ToolSelect)).collect();
    assert_eq!(keys, vec![Shortcut { cmd: false, shift: false, alt: false, key: "T" }]);
}
