//! Shortcut sets (A293) and the visual keyboard's logic (A292).

use wavcraft_core::actions::ActionId;
use wavcraft_core::keyboard::{ANSI_ROWS, Mods};
use wavcraft_core::shortcut_sets::{SetStore, valid_name};
use wavcraft_core::shortcuts::{BINDINGS, Command, Keymap, Shortcut};

const SELECT_ALL: Command = Command::Action(ActionId::SelectAll);
const UNDO: Command = Command::Action(ActionId::Undo);

fn chord(text: &str) -> Shortcut {
    Shortcut::parse(text).expect("a chord")
}

fn store(name: &str) -> SetStore {
    let dir = std::env::temp_dir().join(format!("wavcraft-sets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    SetStore::new(dir)
}

fn with_select_all(key: &str) -> Keymap {
    let mut m = Keymap::new();
    m.assign(SELECT_ALL, chord(key)).expect("free chord");
    m
}

#[test]
fn a_saved_set_loads_back_the_same_keymap() {
    let s = store("roundtrip");
    let map = with_select_all("Ctrl+K");
    assert_eq!(s.save("Mine", &map).as_deref(), Ok("Mine"));
    assert!(s.load("Mine").expect("loads") == map);
    assert_eq!(s.list(), vec!["Mine".to_string()]);
}

#[test]
fn sets_are_listed_sorted_and_the_default_is_not_stored() {
    let s = store("list");
    s.save("beta", &with_select_all("Ctrl+K")).expect("saves");
    s.save("Alpha", &with_select_all("Ctrl+L")).expect("saves");
    assert_eq!(s.list(), vec!["Alpha".to_string(), "beta".to_string()]);
    assert!(s.load("default").expect("built in") == Keymap::new());
}

#[test]
fn the_default_set_cannot_be_saved_over_or_deleted() {
    let s = store("default");
    assert!(s.save("default", &Keymap::new()).is_err());
    assert!(s.save("Default", &Keymap::new()).is_err());
    assert!(s.delete("default").is_err());
    assert!(s.list().is_empty());
}

#[test]
fn names_are_checked() {
    assert!(valid_name("  ").is_err());
    assert!(valid_name("a/b").is_err());
    assert!(valid_name("..").is_err());
    assert!(valid_name(&"x".repeat(41)).is_err());
    assert_eq!(valid_name("  My set_2 ").as_deref(), Ok("My set_2"));
}

#[test]
fn delete_removes_a_set_and_an_unknown_one_is_an_error() {
    let s = store("delete");
    s.save("Mine", &with_select_all("Ctrl+K")).expect("saves");
    assert_eq!(s.delete("Mine"), Ok(()));
    assert!(s.list().is_empty());
    assert!(s.delete("Mine").is_err());
    assert!(s.load("Mine").is_err());
}

#[test]
fn saving_again_replaces_the_set() {
    let s = store("replace");
    s.save("Mine", &with_select_all("Ctrl+K")).expect("saves");
    s.save("Mine", &with_select_all("Ctrl+L")).expect("saves");
    let m = s.load("Mine").expect("loads");
    assert_eq!(m.resolve(chord("Ctrl+L")), Some(SELECT_ALL));
    assert_eq!(m.resolve(chord("Ctrl+K")), None);
}

#[test]
fn the_active_set_is_remembered_and_applies_at_start() {
    let s = store("active");
    assert_eq!(s.active(), None);
    s.save("Mine", &with_select_all("Ctrl+K")).expect("saves");
    s.set_active(Some("Mine")).expect("writes");
    assert_eq!(s.active().as_deref(), Some("Mine"));
    let (map, why) = s.startup(Keymap::new());
    assert!(why.is_none());
    assert_eq!(map.resolve(chord("Ctrl+K")), Some(SELECT_ALL));
    s.set_active(None).expect("clears");
    assert_eq!(s.active(), None);
    assert!(s.startup(Keymap::new()).0 == Keymap::new());
}

#[test]
fn an_active_set_that_was_deleted_or_is_broken_falls_back() {
    let s = store("fallback");
    s.save("Mine", &with_select_all("Ctrl+K")).expect("saves");
    s.set_active(Some("Mine")).expect("writes");
    s.delete("Mine").expect("deletes");
    assert_eq!(s.active(), None, "a set that is gone is the default");
    s.save("Bad", &Keymap::new()).expect("saves");
    s.set_active(Some("Bad")).expect("writes");
    std::fs::write(s.dir().join("shortcut-sets").join("Bad.txt"), "this is not a key file\n").expect("writes");
    let (map, why) = s.startup(with_select_all("Ctrl+Q"));
    assert!(why.is_some_and(|w| w.contains("Bad")));
    assert_eq!(map.resolve(chord("Ctrl+Q")), Some(SELECT_ALL), "the fallback is used");
}

#[test]
fn keys_with_lists_the_keys_bound_for_exactly_those_modifiers() {
    let map = Keymap::new();
    let ctrl = map.keys_with(Mods { cmd: true, ..Mods::default() });
    assert!(ctrl.contains(&("Z", UNDO)));
    assert!(ctrl.contains(&("A", SELECT_ALL)));
    assert!(!ctrl.iter().any(|(k, _)| *k == "Space"), "Space is bound with no modifier only");
    let none = map.keys_with(Mods::default());
    assert!(none.contains(&("Space", Command::PlayStop)));
    assert!(!none.contains(&("Z", UNDO)));
    let both = map.keys_with(Mods { cmd: true, shift: true, alt: false });
    assert!(both.iter().any(|(k, c)| *k == "Z" && *c == Command::Action(ActionId::Redo)));
}

#[test]
fn keys_with_follows_the_keymap() {
    let map = with_select_all("Ctrl+K");
    let ctrl = map.keys_with(Mods { cmd: true, ..Mods::default() });
    assert!(ctrl.contains(&("K", SELECT_ALL)));
    assert!(!ctrl.contains(&("A", SELECT_ALL)));
}

#[test]
fn commands_on_key_ignores_modifiers() {
    let map = Keymap::new();
    let on_z = map.commands_on_key("Z");
    // Undo and Redo (Ctrl+Z, Ctrl+Shift+Z) and the Zoom tool (Z).
    assert_eq!(on_z.len(), 3, "Undo, Redo and the Zoom tool: {on_z:?}");
    assert!(map.commands_on_key("Q").is_empty());
}

#[test]
fn modifiers_parse_in_any_order_and_case() {
    assert_eq!(Mods::parse("shift+CTRL"), Ok(Mods { cmd: true, shift: true, alt: false }));
    assert_eq!(Mods::parse("none"), Ok(Mods::default()));
    assert_eq!(Mods::parse(""), Ok(Mods::default()));
    assert!(Mods::parse("Ctrl+Hyper").is_err());
    assert_eq!(Mods { cmd: true, shift: true, alt: true }.text(), "Ctrl+Alt+Shift");
}

#[test]
fn every_default_bound_key_is_on_the_drawn_keyboard() {
    let drawn: Vec<&str> = ANSI_ROWS.iter().flat_map(|r| r.iter()).filter_map(|k| k.name).collect();
    for b in BINDINGS {
        assert!(drawn.contains(&b.chord.key), "{} is not drawn", b.chord.key);
    }
    let mut seen = drawn.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), drawn.len(), "a key is drawn twice");
}
