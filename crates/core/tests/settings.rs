//! Item 03b: the settings file and choosing the session folder, with its fall-back to the default.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

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
    let hidden = Settings { hidden_panels: vec!["history".into(), "waveform".into()], ..Settings::default() };
    hidden.save(&file).unwrap();
    assert_eq!(Settings::load(&file), hidden, "hidden panels are remembered");
    assert_eq!(Settings::parse("hidden_panels = history , ,spectral\n").hidden_panels, vec!["history", "spectral"]);
    let meta = Settings { metadata_fields: Some(vec!["name".into(), "size".into()]), ..Settings::default() };
    meta.save(&file).unwrap();
    assert_eq!(Settings::load(&file), meta, "metadata fields are remembered");
    let none = Settings { metadata_fields: Some(vec![]), ..Settings::default() };
    none.save(&file).unwrap();
    assert_eq!(Settings::load(&file), none, "an empty choice is not the default");
    Settings::default().save(&file).unwrap();
    let parsed = Settings::parse("# a comment\nnonsense line\nfuture_key = 3\n  session_folder =  /Volumes/Big/ws  \n");
    assert_eq!(parsed.session_folder, Some(PathBuf::from("/Volumes/Big/ws")));
}

#[test]
fn the_panel_layout_is_kept_in_the_settings_file() {
    use wavcraft_core::panels::{DockSide, Layout};
    let dir = TempDir::new("layout");
    let file = dir.0.join("settings.txt");
    let mut layout = Layout::default();
    layout.move_to("history", DockSide::Bottom, None).unwrap();
    layout.set_size(DockSide::Bottom, 310.0);
    let (panel_layout, zone_sizes) = layout.encode();
    Settings { panel_layout, zone_sizes, ..Settings::default() }.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(Layout::decode(&back.panel_layout, &back.zone_sizes), layout);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("panel_layout = left:;right:selection,markers,playlist;bottom:freq,history*"), "{text}");
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

#[test]
fn concurrent_saves_do_not_share_temp_files() {
    let dir = TempDir::new("concurrent-saves");
    let file = Arc::new(dir.0.join("settings.txt"));
    let mut threads = Vec::new();
    for worker in 0..8 {
        let file = Arc::clone(&file);
        threads.push(thread::spawn(move || {
            let settings = Settings { session_folder: Some(PathBuf::from(format!("/w{worker}"))), ..Default::default() };
            for _ in 0..4 {
                settings.save(&file).unwrap();
            }
            settings
        }));
    }

    let writers: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    let text = std::fs::read_to_string(&*file).unwrap();
    assert!(writers.iter().any(|settings| settings.to_text() == text), "{text}");
    let entries: Vec<_> = std::fs::read_dir(&dir.0).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(entries, vec![std::ffi::OsString::from("settings.txt")]);
}

#[test]
fn the_time_display_is_saved_and_read_back() {
    use wavcraft_core::TimeFormat;
    let t = TempDir::new("time-display");
    let file = t.0.join("settings.txt");
    assert_eq!(Settings::load(&file).time_display, TimeFormat::Decimal, "the default");
    for f in TimeFormat::ALL {
        Settings { time_display: f, ..Settings::default() }.save(&file).unwrap();
        assert_eq!(Settings::load(&file).time_display, f);
    }
    assert_eq!(Settings::parse("time_display = nonsense\n").time_display, TimeFormat::Decimal);
}

/// Every key f83 added, with a value that is not its default.
const F83_CHANGED: [(&str, &str); 13] = [
    ("spec_window", "blackman-harris"),
    ("spec_resolution", "4096"),
    ("spec_db_range", "120"),
    ("spec_play_selected_freqs", "true"),
    ("time_display", "smpte_2997df"),
    ("loudness_target_lufs", "-23"),
    ("true_peak_ceiling_dbtp", "-2.5"),
    ("zoom_factor", "3"),
    ("extend_selection", "anchor"),
    ("context_channel_editing", "false"),
    ("theme", "light"),
    ("waveform_colour", "amber"),
    ("ui_brightness", "0.8"),
];

const F83_DEFAULTS: [(&str, &str); 13] = [
    ("spec_window", "hann"),
    ("spec_resolution", "2048"),
    ("spec_db_range", "100"),
    ("spec_play_selected_freqs", "false"),
    ("time_display", "decimal"),
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
        ("time_display", "hex"),
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
    s.set("autosave_enabled", "off").unwrap();
    s.set("autosave_interval_secs", "15").unwrap();
    s.set("autosave_keep_days", "7").unwrap();
    s.set("cache_mb", "512").unwrap();
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back, s);
    assert_eq!((back.autosave_enabled, back.autosave_interval_secs, back.autosave_keep_days, back.cache_mb), (false, 15, 7, Some(512)));
    assert_eq!(back.cache_bytes(), 512 << 20);
    s.set("cache_mb", "default").unwrap();
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
        assert!(s.set(k, v).is_err(), "{k}={v}");
    }
    assert_eq!(s, Settings::default());
    // The same values in a hand-edited file are ignored, the rest still read.
    let p = Settings::parse("autosave_interval_secs = 99999\nautosave_keep_days = 10\ncache_mb = abc\n");
    assert_eq!((p.autosave_interval_secs, p.autosave_keep_days, p.cache_mb), (2, 10, None));
    assert_eq!(s.get("autosave_enabled").unwrap(), "on");
    assert_eq!(s.get("cache_mb").unwrap(), "default");
    assert!(s.get("zzz").is_err());
}

#[test]
fn a262_include_metadata_defaults_on_and_round_trips() {
    assert!(Settings::parse("").save_include_metadata);
    let off = Settings { save_include_metadata: false, ..Settings::default() };
    assert!(!Settings::parse(&off.to_text()).save_include_metadata);
    assert!(Settings::parse(&Settings::default().to_text()).save_include_metadata);
}

#[test]
fn marker_columns_are_saved_and_unknown_names_ignored() {
    use wavcraft_core::settings::MarkerColumns;
    let dir = TempDir::new("marker-columns");
    let file = dir.0.join("settings.txt");
    assert!(Settings::load(&file).marker_columns.duration, "shown by default");
    let mut s = Settings::default();
    s.marker_columns.duration = false;
    s.marker_columns.description = true;
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back.marker_columns, s.marker_columns);
    assert_eq!(back.marker_columns.to_list(), "start,end,type,description");
    assert_eq!(MarkerColumns::from_list("end, bogus ,type").to_list(), "end,type");
    assert_eq!(Settings::parse("marker_columns =
").marker_columns.to_list(), "", "an empty list hides them all");
}
