//! f20: the preferences kept in the settings file (crossfades, devices), `get`/`set` by key, and
//! resetting the file aside.

use std::path::PathBuf;
use std::time::Duration;

use wavcraft_core::bundle;
use wavcraft_core::settings::{self, Settings};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-prefs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn an_old_file_with_only_the_session_folder_still_loads_with_default_preferences() {
    let s = Settings::parse("# WavCraft settings.\nsession_folder = /x/y\n");
    assert_eq!(s.session_folder, Some(PathBuf::from("/x/y")));
    assert_eq!(s.crossfades(), (Duration::from_millis(2), Duration::from_millis(5)));
    assert_eq!(s.output_device, None);
    assert_eq!(s.input_device, None);
}

#[test]
fn new_preferences_round_trip_and_the_old_key_stays_where_it_was() {
    let dir = TempDir::new("roundtrip");
    let file = dir.0.join("settings.txt");
    let mut s = Settings::default();
    s.set("session_folder", "/some/where").unwrap();
    s.set("crossfade_cut_join_ms", "3").unwrap();
    s.set("crossfade_edge_ms", "0").unwrap();
    s.set("output_device", "Speakers (USB = 2)").unwrap();
    s.set("input_device", "Mic").unwrap();
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back, s);
    assert_eq!(back.crossfades(), (Duration::from_millis(3), Duration::ZERO));
    assert_eq!(back.get("output_device").unwrap(), "Speakers (USB = 2)");
    // An older build reads the same file: it only looks for session_folder.
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.lines().any(|l| l == "session_folder = /some/where"), "{text}");
}

#[test]
fn get_and_set_by_key_check_their_values() {
    let mut s = Settings::default();
    assert_eq!(s.get("crossfade_cut_join_ms").unwrap(), "2");
    assert_eq!(s.get("crossfade_edge_ms").unwrap(), "5");
    assert_eq!(s.get("output_device").unwrap(), settings::SYSTEM_DEFAULT);
    assert!(s.get("nonsense").is_err());
    assert!(s.set("nonsense", "1").is_err());
    assert!(s.set("crossfade_edge_ms", "abc").is_err());
    assert!(s.set("crossfade_edge_ms", "-1").is_err());
    assert!(s.set("crossfade_edge_ms", "5000").is_err(), "a crossfade over a second is refused");
    assert_eq!(s.get("crossfade_edge_ms").unwrap(), "5", "a refused value changes nothing");
    s.set("output_device", "Thing").unwrap();
    s.set("output_device", settings::SYSTEM_DEFAULT).unwrap();
    assert_eq!(s.output_device, None, "\"System default\" is no device at all");
    s.set("input_device", "").unwrap();
    assert_eq!(s.input_device, None);
}

#[test]
fn reset_moves_the_file_aside_and_starts_from_defaults() {
    let dir = TempDir::new("reset");
    let file = dir.0.join("settings.txt");
    let aside = dir.0.join("settings.txt.reset-1");
    let mut s = Settings::default();
    s.set("crossfade_edge_ms", "9").unwrap();
    s.save(&file).unwrap();
    assert_eq!(bundle::reset_preferences(&dir.0).unwrap(), ["settings.txt"]);
    assert!(!file.exists() && aside.exists());
    assert_eq!(Settings::load(&file), Settings::default());
    assert_eq!(Settings::load(&aside).crossfades().1, Duration::from_millis(9), "the old file is kept whole");
    // Nothing to move: not an error. A second reset keeps the first copy.
    assert!(bundle::reset_preferences(&dir.0).unwrap().is_empty());
    s.save(&file).unwrap();
    assert_eq!(bundle::reset_preferences(&dir.0).unwrap(), ["settings.txt"]);
    let again = dir.0.join("settings.txt.reset-2");
    assert!(aside.exists() && again.exists());
}
