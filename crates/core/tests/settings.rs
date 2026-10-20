//! Item 03b: the settings file and choosing the session folder, with its fall-back to the default.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wavcraft_core::settings::{FolderSource, Settings, check_folder, resolve_session_folder};

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-settings-test-{}-{name}", std::process::id()));
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

fn ok(_: &Path) -> Result<(), String> {
    Ok(())
}

#[test]
fn the_settings_file_round_trips_and_ignores_what_it_does_not_know() {
    let dir = TempDir::new("file");
    let file = dir.0.join("nested").join("settings.txt");
    assert_eq!(Settings::load(&file), Settings::default(), "a missing file gives the defaults");
    let s = Settings { session_folder: Some(PathBuf::from("D:/Scratch/wavcraft sessions")), ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file), s);
    Settings::default().save(&file).unwrap();
    assert_eq!(Settings::load(&file), Settings::default(), "an empty value is the default");
    let parsed = Settings::parse("# a comment\nnonsense line\nfuture_key = 3\n  session_folder =  /Volumes/Big/ws  \n");
    assert_eq!(parsed.session_folder, Some(PathBuf::from("/Volumes/Big/ws")));
}

#[test]
fn the_flag_beats_the_environment_beats_the_settings_file_beats_the_default() {
    let default = PathBuf::from("/default/sessions");
    let settings = Settings { session_folder: Some(PathBuf::from("/from/settings")), ..Settings::default() };
    let abs = |p: &str| std::path::absolute(p).unwrap();

    let r = resolve_session_folder(Some("/from/flag".into()), Some(OsString::from("/from/env")), &settings, default.clone(), ok);
    assert_eq!((r.path, r.source, r.warning), (abs("/from/flag"), FolderSource::CommandLine, None));
    let r = resolve_session_folder(None, Some(OsString::from("/from/env")), &settings, default.clone(), ok);
    assert_eq!((r.path, r.source), (abs("/from/env"), FolderSource::Environment));
    let r = resolve_session_folder(None, Some(OsString::new()), &settings, default.clone(), ok);
    assert_eq!((r.path, r.source), (abs("/from/settings"), FolderSource::SettingsFile), "an empty variable is not set");
    let r = resolve_session_folder(None, None, &Settings::default(), default.clone(), ok);
    assert_eq!((r.path, r.source, r.warning), (default, FolderSource::Default, None));
}

#[test]
fn a_folder_that_cannot_be_used_falls_back_to_the_default_and_says_why() {
    let dir = TempDir::new("fallback");
    let default = dir.0.join("default-sessions");

    // A path through a file can't be made into a folder.
    let file = dir.0.join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let bad = file.join("sessions");
    assert!(check_folder(&bad).is_err());
    let r = resolve_session_folder(Some(bad.clone()), None, &Settings::default(), default.clone(), check_folder);
    assert_eq!((&r.path, r.source), (&default, FolderSource::Default));
    let w = r.warning.unwrap();
    assert!(w.contains(&bad.display().to_string()) && w.contains(&default.display().to_string()) && w.contains("--session-dir"), "{w}");

    // A folder the check refuses (as a read-only one would be), from the settings file.
    let settings = Settings { session_folder: Some(dir.0.join("read-only")), ..Settings::default() };
    let refuse = |p: &Path| if p.ends_with("read-only") { Err("it can't be written to (denied)".to_string()) } else { Ok(()) };
    let r = resolve_session_folder(None, None, &settings, default.clone(), refuse);
    assert_eq!(r.path, default);
    assert!(r.warning.unwrap().contains("can't be written to"));
}

#[test]
fn check_folder_makes_a_missing_folder_and_leaves_no_probe_behind() {
    let dir = TempDir::new("check");
    let target = dir.0.join("a").join("b");
    check_folder(&target).unwrap();
    assert!(target.is_dir());
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    assert!(check_folder(Path::new("")).is_err());
}

// ---- A278 / A281 (f70): Auto Save and cache preferences

#[test]
fn the_preferences_default_to_the_old_behaviour() {
    let s = Settings::default();
    assert!(s.autosave_enabled, "Auto Save is on by default");
    assert_eq!((s.autosave_interval_secs, s.autosave_keep_days, s.cache_mb), (2, 30, None));
    assert_eq!(s.cache_bytes(), wavcraft_core::store::default_cache_bytes(), "the cache budget is unchanged until set");
}

#[test]
fn the_preferences_round_trip_through_the_file() {
    let dir = TempDir::new("prefs");
    let file = dir.0.join("settings.txt");
    let mut s = Settings::default();
    s.set_pref("autosave_enabled", "off").unwrap();
    s.set_pref("autosave_interval_secs", "15").unwrap();
    s.set_pref("autosave_keep_days", "7").unwrap();
    s.set_pref("cache_mb", "512").unwrap();
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back, s);
    assert_eq!((back.autosave_enabled, back.autosave_interval_secs, back.autosave_keep_days, back.cache_mb), (false, 15, 7, Some(512)));
    assert_eq!(back.cache_bytes(), 512 << 20);
    s.set_pref("cache_mb", "default").unwrap();
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file).cache_mb, None, "an unset cache size stays unset");
}

#[test]
fn a_file_from_before_the_preferences_still_loads_with_the_defaults() {
    let s = Settings::parse("# old file\nsession_folder = /Volumes/Big/ws\n");
    assert_eq!(s, Settings { session_folder: Some(PathBuf::from("/Volumes/Big/ws")), ..Settings::default() });
}

#[test]
fn bad_preference_values_are_refused_and_change_nothing() {
    let mut s = Settings::default();
    for (k, v) in [
        ("autosave_enabled", "maybe"),
        ("autosave_interval_secs", "0"),
        ("autosave_interval_secs", "x"),
        ("autosave_keep_days", "0"),
        ("cache_mb", "1"),
        ("cache_mb", "-5"),
        ("nope", "1"),
    ] {
        assert!(s.set_pref(k, v).is_err(), "{k}={v}");
    }
    assert_eq!(s, Settings::default());
    // The same values in a hand-edited file are ignored, the rest still read.
    let p = Settings::parse("autosave_interval_secs = 99999\nautosave_keep_days = 10\ncache_mb = abc\n");
    assert_eq!((p.autosave_interval_secs, p.autosave_keep_days, p.cache_mb), (2, 10, None));
    assert_eq!(s.get_pref("autosave_enabled").as_deref(), Some("on"));
    assert_eq!(s.get_pref("cache_mb").as_deref(), Some("default"));
    assert_eq!(s.get_pref("zzz"), None);
}
