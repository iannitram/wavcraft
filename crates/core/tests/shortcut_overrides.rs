//! The user's shortcut overrides (roadmap A291): assign, conflicts, clear, restore defaults, the
//! key file's round trip, and a broken key file being ignored.

use wavcraft_core::actions::ActionId;
use wavcraft_core::shortcuts::{self, Command, Keymap, Overrides, Shortcut};

fn chord(text: &str) -> Shortcut {
    Shortcut::parse(text).expect("a chord")
}

const SELECT_ALL: Command = Command::Action(ActionId::SelectAll);
const UNDO: Command = Command::Action(ActionId::Undo);
const REDO: Command = Command::Action(ActionId::Redo);

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-shortcuts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

#[test]
fn assign_replaces_the_commands_chords_and_resolves() {
    let mut map = Keymap::new();
    assert_eq!(map.assign(SELECT_ALL, chord("Ctrl+K")), Ok(()));
    assert_eq!(map.resolve(chord("Ctrl+K")), Some(SELECT_ALL));
    assert_eq!(map.resolve(chord("Ctrl+A")), None, "the old chord no longer runs it");
    assert_eq!(map.chords(SELECT_ALL).collect::<Vec<_>>(), vec![chord("Ctrl+K")]);
    assert!(map.check().is_ok());
}

#[test]
fn a_chord_in_use_is_a_conflict_naming_its_command() {
    let mut map = Keymap::new();
    let before = map.clone();
    let err = map.assign(SELECT_ALL, chord("Ctrl+Z")).expect_err("Ctrl+Z is Undo");
    assert_eq!(err.holder, UNDO);
    assert_eq!(err.chord, chord("Ctrl+Z"));
    assert!(map == before, "a refused assign changes nothing");
}

#[test]
fn reassign_takes_the_chord_from_its_holder() {
    let mut map = Keymap::new();
    assert_eq!(map.assign_reassign(SELECT_ALL, chord("Ctrl+Z")), Some(UNDO));
    assert_eq!(map.resolve(chord("Ctrl+Z")), Some(SELECT_ALL));
    assert_eq!(map.chords(UNDO).count(), 0, "Undo had only that chord");
    assert!(map.check().is_ok());
    // A holder with a second chord keeps it.
    let mut map = Keymap::new();
    assert_eq!(map.assign_reassign(SELECT_ALL, chord("Ctrl+Y")), Some(REDO));
    assert_eq!(map.chords(REDO).collect::<Vec<_>>(), vec![chord("Ctrl+Shift+Z")]);
}

#[test]
fn assigning_a_commands_own_chord_is_no_conflict() {
    let mut map = Keymap::new();
    assert_eq!(map.assign(SELECT_ALL, chord("Ctrl+A")), Ok(()));
    assert!(map.is_default());
}

#[test]
fn clear_removes_every_chord_and_reset_restores_the_defaults() {
    let mut map = Keymap::new();
    map.clear(REDO);
    assert_eq!(map.chords(REDO).count(), 0);
    assert_eq!(map.resolve(chord("Ctrl+Y")), None);
    map.assign_reassign(SELECT_ALL, chord("Ctrl+Z"));
    assert!(!map.is_default());
    map.reset();
    assert!(map.is_default());
    assert!(map == Keymap::new());
    assert_eq!(map.resolve(chord("Ctrl+Z")), Some(UNDO));
    assert_eq!(map.resolve(chord("Ctrl+Y")), Some(REDO));
}

#[test]
fn overrides_round_trip_through_text() {
    let mut map = Keymap::new();
    map.assign(SELECT_ALL, chord("Ctrl+K")).expect("free");
    map.assign_reassign(Command::Save, chord("Ctrl+Z"));
    map.clear(REDO);
    let text = map.overrides().to_text();
    let back = Keymap::from_overrides(&Overrides::parse(&text).expect("parses")).expect("applies");
    assert!(back == map, "{text}");
    assert!(text.contains("edit.redo = none"), "{text}");
    // Defaults write no overrides.
    assert!(Keymap::new().overrides().entries.is_empty());
}

#[test]
fn overrides_round_trip_through_a_file_and_defaults_remove_it() {
    let dir = scratch("roundtrip");
    let path = dir.join("shortcuts.txt");
    let mut map = Keymap::new();
    map.assign(SELECT_ALL, chord("Ctrl+K")).expect("free");
    shortcuts::save(&path, &map).expect("saved");
    let (loaded, warning) = shortcuts::load(&path);
    assert_eq!(warning, None);
    assert!(loaded == map);
    shortcuts::save(&path, &Keymap::new()).expect("saved");
    assert!(!path.exists(), "no overrides, no file");
    let (loaded, warning) = shortcuts::load(&path);
    assert!(loaded == Keymap::new() && warning.is_none(), "a missing file is the defaults");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_broken_file_is_ignored_with_a_warning() {
    let dir = scratch("broken");
    let path = dir.join("shortcuts.txt");
    for bad in [
        "this is not a key file",
        "edit.selectAll = Ctrl+Bogus\n",
        "no.such.command = Ctrl+K\n",
        "edit.selectAll = Ctrl+Z\nedit.undo = Ctrl+Z\n", // two commands, one chord
        "edit.selectAll = Ctrl+K\nedit.copy = Ctrl+K\n",
        "edit.selectAll = none\nedit.selectAll = Ctrl+K\n",
        "\u{0}\u{1}\u{2}",
    ] {
        std::fs::write(&path, bad).expect("written");
        let (map, warning) = shortcuts::load(&path);
        assert!(map == Keymap::new(), "{bad:?} gives the defaults");
        assert!(warning.is_some_and(|w| w.contains("ignored")), "{bad:?} warns");
    }
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).expect("written");
    let (map, warning) = shortcuts::load(&path);
    assert!(map == Keymap::new());
    assert!(warning.is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn comments_and_blank_lines_are_fine_and_names_round_trip() {
    let text = "# my keys\n\nedit.selectAll = ctrl+k\n";
    let map = Keymap::from_overrides(&Overrides::parse(text).expect("parses")).expect("applies");
    assert_eq!(map.resolve(chord("Ctrl+K")), Some(SELECT_ALL));
    for c in Command::all() {
        assert_eq!(Command::from_name(c.name()), Some(c));
    }
    assert!(Command::all().len() > shortcuts::BINDINGS.len() - 10);
}

#[test]
fn every_chord_text_parses_back() {
    for b in shortcuts::BINDINGS {
        assert_eq!(Shortcut::parse(&b.chord.to_ascii()), Ok(b.chord));
    }
}

#[test]
fn a_file_may_take_a_chord_from_its_holder() {
    // What "reassign" saves: selectAll takes Ctrl+Z and Undo is left with nothing.
    let text = "edit.selectAll = Ctrl+Z\nedit.undo = none\n";
    let map = Keymap::from_overrides(&Overrides::parse(text).expect("parses")).expect("applies");
    assert_eq!(map.resolve(chord("Ctrl+Z")), Some(SELECT_ALL));
    assert_eq!(map.chords(UNDO).count(), 0);
}
