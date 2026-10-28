//! Keys that switch workspace (roadmap A012): `workspace.switch.<n>` for the first nine
//! workspaces, in the order the Workspace menu lists them, bound by default to Alt+Shift+<n>.

use wavcraft_core::panels::{DockSide, Layout, PanelSet};
use wavcraft_core::shortcuts::{BINDINGS, Command, Keymap, Shortcut, conflicts};
use wavcraft_core::workspaces::{Snapshot, Workspaces};

#[test]
fn nine_switch_commands_exist_with_stable_names() {
    for n in 1..=9u8 {
        let name = format!("workspace.switch.{n}");
        let c = Command::from_name(&name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(c, Command::Workspace(n));
        assert_eq!(c.name(), name);
        assert!(!c.label().is_empty());
    }
    assert_eq!(Command::from_name("workspace.switch.0"), None);
    assert_eq!(Command::from_name("workspace.switch.10"), None);
}

#[test]
fn alt_shift_digits_resolve_to_them_and_do_not_conflict() {
    let map = Keymap::new();
    for n in 1..=9u8 {
        assert_eq!(map.resolve_text(&format!("Alt+Shift+{n}")), Ok(Command::Workspace(n)));
        assert_eq!(map.resolve_text(&format!("Cmd+Alt+Shift+{n}")).ok(), None, "no chord with Ctrl");
    }
    assert!(conflicts(BINDINGS).is_empty());
    assert_eq!(map.check(), Ok(()));
}

#[test]
fn a_switch_command_can_be_rebound_in_the_editor() {
    let mut map = Keymap::new();
    let chord = Shortcut::parse("Ctrl+Alt+1").unwrap();
    map.assign(Command::Workspace(1), chord).unwrap();
    assert_eq!(map.resolve(chord), Some(Command::Workspace(1)));
    let back = Keymap::from_overrides(&map.overrides()).unwrap();
    assert_eq!(back.resolve(chord), Some(Command::Workspace(1)));
}

#[test]
fn the_nth_workspace_is_in_menu_order_and_switching_changes_the_layout() {
    let dir = std::env::temp_dir().join(format!("wavcraft-wskeys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut w = Workspaces::new(Some(dir.clone()), "Default");
    let (mut set, mut layout) = (PanelSet::default(), Layout::default());
    layout.move_to("history", DockSide::Bottom, None).unwrap();
    set.set("selection", false);
    let other = Snapshot::capture(&set, &layout);
    w.save("Zed", &other).unwrap();
    w.save("Alpha", &other).unwrap();
    // Default first, then the built-in workspaces (A001), then the saved ones by name.
    let n = w.names().len();
    assert_eq!(w.nth(1).as_deref(), Some("Default"));
    assert_eq!(w.nth(n - 1).as_deref(), Some("Alpha"));
    assert_eq!(w.nth(n).as_deref(), Some("Zed"));
    assert_eq!(w.nth(n + 1), None);
    assert_eq!(w.nth(0), None);
    let snap = w.switch(&w.nth(1).unwrap()).unwrap();
    assert_eq!(snap, Snapshot::builtin());
    let snap = w.switch(&w.nth(n).unwrap()).unwrap();
    assert_eq!(snap, other);
    assert_ne!(snap, Snapshot::builtin());
    let _ = std::fs::remove_dir_all(&dir);
}
