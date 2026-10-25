use wavcraft_core::actions::{ActionId, Shortcut, action};

#[test]
fn i3_time_selection_uses_t_in_the_menu_action_table() {
    let row = action(ActionId::ToolSelect);
    assert_eq!(row.name, "tool.select");
    assert_eq!(row.menu, "Time Selection Tool");
    assert_eq!(row.keys, &[Shortcut { cmd: false, shift: false, alt: false, key: "T" }]);
}
