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

/// Every key f83 added, with a value that is not its default.
const F83_CHANGED: [(&str, &str); 14] = [
    ("spec_window", "blackman-harris"),
    ("spec_resolution", "4096"),
    ("spec_db_range", "120"),
    ("spec_play_selected_freqs", "true"),
    ("time_format", "smpte"),
    ("frame_rate", "29.97"),
    ("loudness_target_lufs", "-23"),
    ("true_peak_ceiling_dbtp", "-2.5"),
    ("zoom_factor", "3"),
    ("extend_selection", "anchor"),
    ("context_channel_editing", "false"),
    ("theme", "light"),
    ("waveform_colour", "amber"),
    ("ui_brightness", "0.8"),
];

const F83_DEFAULTS: [(&str, &str); 14] = [
    ("spec_window", "hann"),
    ("spec_resolution", "2048"),
    ("spec_db_range", "100"),
    ("spec_play_selected_freqs", "false"),
    ("time_format", "decimal"),
    ("frame_rate", "30"),
    ("loudness_target_lufs", "-16"),
    ("true_peak_ceiling_dbtp", "-1"),
    ("zoom_factor", "2"),
    ("extend_selection", "nearest"),
    ("context_channel_editing", "true"),
    ("theme", "dark"),
    ("waveform_colour", "green"),
    ("ui_brightness", "1"),
];

#[test]
fn the_new_preferences_start_at_their_defaults_and_every_key_is_listed() {
    let s = Settings::default();
    for (k, v) in F83_DEFAULTS {
        assert_eq!(s.get(k).unwrap(), v, "{k}");
        assert!(wavcraft_core::settings::KEYS.contains(&k), "{k} is in KEYS");
    }
    assert_eq!((s.loudness_target(), s.true_peak_ceiling()), (-16.0, -1.0), "the owner's defaults");
}

#[test]
fn every_new_preference_round_trips_through_the_file() {
    let dir = TempDir::new("f83");
    let file = dir.0.join("settings.txt");
    let mut s = Settings::default();
    for (k, v) in F83_CHANGED {
        s.set(k, v).unwrap_or_else(|e| panic!("{k}: {e}"));
        assert_eq!(s.get(k).unwrap(), v, "{k}");
    }
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back, s);
    for (k, v) in F83_CHANGED {
        assert_eq!(back.get(k).unwrap(), v, "{k} after reload");
    }
    assert_eq!(back.frame_rate(), 29.97);
    assert_eq!(back.waveform_rgb(), wavcraft_core::settings::WaveColour::Amber.rgb());
}

#[test]
fn a_settings_file_from_before_the_new_keys_loads_with_their_defaults() {
    let s = Settings::parse("session_folder =\ncrossfade_edge_ms = 7\nautoscroll = off\n");
    assert_eq!(s.edge_ms, 7);
    for (k, v) in F83_DEFAULTS {
        assert_eq!(s.get(k).unwrap(), v, "{k}");
    }
}

#[test]
fn bad_values_for_the_new_preferences_are_refused_and_change_nothing() {
    let bad = [
        ("spec_window", "triangle"),
        ("spec_resolution", "1000"),
        ("spec_resolution", "x"),
        ("spec_db_range", "39"),
        ("spec_db_range", "161"),
        ("spec_play_selected_freqs", "yes"),
        ("time_format", "hex"),
        ("frame_rate", "0"),
        ("frame_rate", "241"),
        ("frame_rate", "nan"),
        ("loudness_target_lufs", "1"),
        ("loudness_target_lufs", "-71"),
        ("true_peak_ceiling_dbtp", "0.5"),
        ("true_peak_ceiling_dbtp", "-21"),
        ("zoom_factor", "1"),
        ("zoom_factor", "9"),
        ("extend_selection", "left"),
        ("context_channel_editing", "1"),
        ("theme", "sepia"),
        ("waveform_colour", "#ff0000"),
        ("ui_brightness", "0.1"),
        ("ui_brightness", "inf"),
    ];
    let mut s = Settings::default();
    for (k, v) in bad {
        assert!(s.set(k, v).is_err(), "{k} = {v} should be refused");
    }
    assert_eq!(s, Settings::default());
    // And a bad value in a file leaves the default, as an unknown key is ignored.
    assert_eq!(Settings::parse("theme = sepia\nzoom_factor = 99\n"), Settings::default());
}

#[test]
fn the_spectral_preferences_set_the_displays_fft_length_and_floor() {
    let mut s = Settings::default();
    s.set("spec_resolution", "512").unwrap();
    s.set("spec_db_range", "80").unwrap();
    let mut p = wavcraft_core::SpectrogramParams::default();
    s.apply_to_spectrogram(&mut p);
    assert_eq!((p.fft_size, p.floor_db, p.ceil_db), (512, -80.0, 0.0));
}
