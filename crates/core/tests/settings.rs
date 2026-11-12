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
    let s = Settings { session_folder: Some(PathBuf::from("/scratch/wavcraft sessions")), ..Settings::default() };
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
    assert!(
        text.contains("panel_layout = left:tools,files;right:markers,playlist,match;bottom:selection,freq,properties,transport,time,zoom,history*"),
        "{text}"
    );
}

#[test]
fn hiding_the_time_and_selection_view_panels_survives_a_settings_save_and_load() {
    use wavcraft_core::panels::{PanelSet, by_id};
    let dir = TempDir::new("hidden-time-selection");
    let file = dir.0.join("settings.txt");
    assert_eq!(by_id("time").map(|p| p.title), Some("Time"));
    assert_eq!(by_id("selection").map(|p| p.title), Some("Selection/View"));
    let mut set = PanelSet::default();
    assert!(set.set("time", false));
    assert!(set.set("selection", false));
    Settings { hidden_panels: set.hidden_ids(), ..Settings::default() }.save(&file).unwrap();
    let back = PanelSet::from_hidden(&Settings::load(&file).hidden_panels);
    assert!(!back.is_shown("time"), "the Time panel is still hidden after a restart");
    assert!(!back.is_shown("selection"), "Selection/View is still hidden after a restart");
    assert!(back.is_shown("waveform"), "the other panels are still shown");
}

#[test]
fn hiding_the_zoom_panel_survives_a_settings_save_and_load() {
    use wavcraft_core::panels::{PanelSet, by_id};
    let dir = TempDir::new("hidden-zoom");
    let file = dir.0.join("settings.txt");
    assert_eq!(by_id("zoom").map(|p| p.title), Some("Zoom"));
    let mut set = PanelSet::default();
    assert!(set.set("zoom", false));
    Settings { hidden_panels: set.hidden_ids(), ..Settings::default() }.save(&file).unwrap();
    let back = PanelSet::from_hidden(&Settings::load(&file).hidden_panels);
    assert!(!back.is_shown("zoom"), "the Zoom panel is still hidden after a restart");
    assert!(back.is_shown("waveform"), "the other panels are still shown");
}

#[test]
fn the_amplitude_scale_is_saved_and_a_bad_value_is_the_default() {
    use wavcraft_core::AmpScale;
    let dir = TempDir::new("ampscale");
    let file = dir.0.join("settings.txt");
    let s = Settings { amp_scale: AmpScale::Db, ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file).amp_scale, AmpScale::Db);
    assert_eq!(Settings::parse("amp_scale = samples").amp_scale, AmpScale::Samples);
    assert_eq!(Settings::parse("amp_scale = furlongs").amp_scale, AmpScale::default());
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
    ("context_channel_editing", "true"),
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
    ("context_channel_editing", "false"),
    ("theme", "dark"),
    ("waveform_colour", "green"),
    ("ui_brightness", "1"),
];

#[test]
fn context_channel_editing_is_off_by_default() {
    assert!(!Settings::default().context_channel_editing);
}

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
fn old_save_markers_and_metadata_keys_map_onto_save_include_metadata() {
    // Both old keys false means off.
    let s = Settings::parse("save_markers = false\nsave_metadata = false\n");
    assert!(!s.save_include_metadata, "both false -> off");

    // Only one old key false means on.
    let s = Settings::parse("save_markers = false\n");
    assert!(s.save_include_metadata, "only markers false -> on");

    let s = Settings::parse("save_metadata = false\n");
    assert!(s.save_include_metadata, "only metadata false -> on");

    // One false and one true means on.
    let s = Settings::parse("save_markers = false\nsave_metadata = true\n");
    assert!(s.save_include_metadata, "markers false, metadata true -> on");

    let s = Settings::parse("save_markers = true\nsave_metadata = false\n");
    assert!(s.save_include_metadata, "markers true, metadata false -> on");

    // Both true means on.
    let s = Settings::parse("save_markers = true\nsave_metadata = true\n");
    assert!(s.save_include_metadata, "both true -> on");

    // When the new key is present, it wins regardless of old keys.
    let s = Settings::parse("save_markers = false\nsave_metadata = false\nsave_include_metadata = 1\n");
    assert!(s.save_include_metadata, "new key wins (1)");

    let s = Settings::parse("save_markers = true\nsave_metadata = true\nsave_include_metadata = 0\n");
    assert!(!s.save_include_metadata, "new key wins (0)");
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
    assert_eq!(
        Settings::parse(
            "marker_columns =
"
        )
        .marker_columns
        .to_list(),
        "",
        "an empty list hides them all"
    );
}

#[test]
fn the_custom_frame_rate_is_saved_and_read_back() {
    use wavcraft_core::TimeFormat;
    let t = TempDir::new("custom-fps");
    let file = t.0.join("settings.txt");
    assert_eq!(Settings::load(&file).custom_fps, 48, "the default");
    for n in [2u16, 7, 48, 999, 1000] {
        Settings { time_display: TimeFormat::Custom(n), custom_fps: n, ..Settings::default() }.save(&file).unwrap();
        let back = Settings::load(&file);
        assert_eq!((back.custom_fps, back.time_display), (n, TimeFormat::Custom(n)));
    }
    // Out of range or unreadable: the earlier value stays.
    for bad in ["1", "0", "1001", "-5", "abc", "", "24.5", "99999999999"] {
        assert_eq!(Settings::parse(&format!("custom_fps = {bad}\n")).custom_fps, 48, "{bad:?}");
    }
    assert_eq!(Settings::parse("custom_fps = 120\ntime_display = custom\n").time_display, TimeFormat::Custom(120), "either line order");
    assert_eq!(Settings::parse("time_display = custom\ncustom_fps = 120\n").time_display, TimeFormat::Custom(120));
    assert_eq!(Settings::parse("time_display = cd75\n").time_display, TimeFormat::Cd75);
}

#[test]
fn the_playback_quality_is_stored_and_a_bad_word_is_the_default() {
    use wavcraft_core::resample::Quality;
    let dir = TempDir::new("quality");
    let file = dir.0.join("settings.txt");
    assert_eq!(Settings::default().playback_quality, Quality::High);
    let s = Settings { playback_quality: Quality::Fast, ..Default::default() };
    s.save(&file).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("playback_quality = fast"));
    assert_eq!(Settings::load(&file).playback_quality, Quality::Fast);
    assert_eq!(Settings::parse("playback_quality = best\n").playback_quality, Quality::High);
    assert!(Settings::default().set("playback_quality", "best").is_err());
}

#[test]
fn the_monitor_preference_round_trips_and_defaults_off() {
    assert!(!Settings::parse("").monitor_input, "off by default");
    let s = Settings { monitor_input: true, ..Settings::default() };
    assert!(Settings::parse(&s.to_text()).monitor_input);
}

#[test]
fn right_click_setting_round_trips_and_defaults_to_extend() {
    assert!(!Settings::default().right_click_menu);
    assert!(!Settings::parse("").right_click_menu);
    let menu = Settings { right_click_menu: true, ..Settings::default() };
    assert!(menu.to_text().contains("right_click = menu"));
    assert!(Settings::parse(&menu.to_text()).right_click_menu);
    // The default is not written, so a file without the key extends.
    let extend = Settings::default();
    assert!(!extend.to_text().contains("right_click"));
    assert!(!Settings::parse(&extend.to_text()).right_click_menu);
    // Unknown values mean the default.
    assert!(!Settings::parse("right_click = banana").right_click_menu);
}

#[test]
fn snapping_choices_last_from_run_to_run() {
    use wavcraft_core::snap::SnapSettings;
    let dir = TempDir::new("snap");
    let file = dir.0.join("settings.txt");
    assert_eq!(Settings::load(&file).snap, SnapSettings::default());
    let snap = SnapSettings { enabled: false, zero_crossings: true, ruler_coarse: false, ruler_fine: true, edges: false, markers: false };
    Settings { snap, ..Settings::default() }.save(&file).unwrap();
    assert_eq!(Settings::load(&file).snap, snap);
    // A value that is not a switch leaves the default; a file without the keys is the defaults.
    let s = Settings::parse("snap_ruler_fine = maybe\nsnap_edges = off\n");
    assert_eq!((s.snap.ruler_fine, s.snap.edges), (false, false));
    assert_eq!(Settings::parse("session_folder = /x\n").snap, SnapSettings::default());
    // A default file keeps the old format: no snap lines are written.
    assert!(!Settings::default().to_text().contains("snap_"));
}

#[test]
fn f68_the_channel_layout_is_kept_in_the_settings_file() {
    use wavcraft_core::ChannelLayout;
    assert_eq!(Settings::default().channel_layout, ChannelLayout::Stacked);
    let dir = TempDir::new("f68-layout");
    let file = dir.0.join("settings.txt");
    let s = Settings { channel_layout: ChannelLayout::Layered, ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file).channel_layout, ChannelLayout::Layered);
    assert_eq!(Settings::parse("channel_layout = nonsense").channel_layout, ChannelLayout::Stacked);
}

#[test]
fn f69_zoom_presets_are_saved_and_bad_values_fall_back() {
    let dir = TempDir::new("zoom-presets");
    let file = dir.0.join("settings.txt");
    let mut s = Settings::default();
    s.zoom_presets.set(3, 42.0);
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file), s);
    assert_eq!(Settings::default().zoom_presets, wavcraft_core::view::ZoomPresets::default());
    assert_eq!(Settings::parse("zoom_presets = 3,4\n").zoom_presets.0[..2], [3.0, 4.0]);
    assert_eq!(Settings::parse("zoom_presets = banana\n").zoom_presets, wavcraft_core::view::ZoomPresets::default());
}

#[test]
fn spectral_divider_share_survives_a_save_and_a_load() {
    use wavcraft_core::settings::{DEFAULT_SPEC_SPLIT_PCT, MAX_SPEC_SPLIT_PCT, MIN_SPEC_SPLIT_PCT};
    let dir = TempDir::new("spectral-split");
    let file = dir.0.join("settings.txt");
    assert_eq!(Settings::default().spec_split_pct, DEFAULT_SPEC_SPLIT_PCT);
    assert!(!Settings::default().to_text().contains("spec_split_pct"), "the default is not written");
    let s = Settings { spec_split_pct: 35, ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file), s);
    // Out of the divider's limits, or not a number: the default stays, the rest of the file reads.
    for bad in [MIN_SPEC_SPLIT_PCT - 1, MAX_SPEC_SPLIT_PCT + 1] {
        assert_eq!(
            Settings::parse(&format!(
                "spec_split_pct = {bad}
"
            ))
            .spec_split_pct,
            DEFAULT_SPEC_SPLIT_PCT
        );
    }
    assert_eq!(
        Settings::parse(
            "spec_split_pct = lots
spec_db_range = 120
"
        )
        .spec_db_range,
        120
    );
    assert!(Settings::default().set("spec_split_pct", "95").is_err());
}

#[test]
fn the_status_bar_fields_and_the_spectral_band_are_kept_in_the_file() {
    use wavcraft_core::freq_axis::FreqAxis;
    use wavcraft_core::statusbar::{StatusField, StatusFields};
    let dir = TempDir::new("view");
    let file = dir.0.join("settings.txt");
    let mut fields = StatusFields::default();
    fields.set(StatusField::FileSize, true);
    fields.set(StatusField::Channels, false);
    let axis = FreqAxis { min_hz: 250.5, max_hz: 8_000.0, log: false, partial: false };
    let s = Settings { status_fields: fields, spectral_view: Some(axis), ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file), s);
    // No key: the defaults. An empty list: nothing shown. A band that makes no sense: ignored.
    assert_eq!(Settings::parse("session_folder =\n").status_fields, StatusFields::default());
    assert!(Settings::parse("status_fields =\n").status_fields.shown().is_empty());
    for bad in ["spectral_view = 500 100 log", "spectral_view = x y", "spectral_view = 1 2 sideways", "spectral_view = -5 10"] {
        assert_eq!(Settings::parse(bad).spectral_view, None, "{bad}");
    }
    let partly = Settings::parse("spectral_view = 0 8000 log_partial").spectral_view.unwrap();
    assert!(partly.log && partly.partial);
    assert!(Settings::parse(&Settings { spectral_view: Some(partly), ..Settings::default() }.to_text()).spectral_view == Some(partly));
    assert_eq!(
        Settings::parse("spectral_view = 100 2000").spectral_view,
        Some(FreqAxis { min_hz: 100.0, max_hz: 2000.0, log: true, partial: false })
    );
}

#[test]
fn sync_options_round_trip_and_default_off() {
    use wavcraft_core::SyncOptions;
    assert_eq!(Settings::parse("").sync, SyncOptions::default());
    let s = Settings { sync: SyncOptions { selection: true, zoom: false, playhead: true }, ..Default::default() };
    assert_eq!(Settings::parse(&s.to_text()), s);
}

#[test]
fn the_language_is_saved_and_read_back_and_unset_means_english() {
    let dir = TempDir::new("language");
    let file = dir.0.join("settings.txt");
    assert_eq!(Settings::load(&file).language, None);
    let s = Settings { language: Some("pseudo".into()), ..Settings::default() };
    s.save(&file).unwrap();
    assert_eq!(Settings::load(&file).language.as_deref(), Some("pseudo"));
    Settings::default().save(&file).unwrap();
    assert_eq!(Settings::load(&file).language, None);
}

/// The settings text for `folder` after a save and a load back, with the loaded value.
fn save_and_load(name: &str, folder: PathBuf) -> (String, Settings) {
    let dir = TempDir::new(name);
    let file = dir.0.join("settings.txt");
    let s = Settings { session_folder: Some(folder), ..Settings::default() };
    s.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let loaded = Settings::load(&file);
    assert_eq!(loaded, s, "the folder must come back exactly");
    (text, s)
}

/// The file has a hex line for the folder and no plain `session_folder = ` line.
fn assert_hex_only(text: &str) {
    assert!(text.lines().any(|l| l.starts_with("session_folder_hex = ")), "no hex line in {text:?}");
    assert!(!text.lines().any(|l| l.starts_with("session_folder = ")), "a plain session_folder line in {text:?}");
}

#[test]
fn a_session_folder_with_a_newline_round_trips_through_hex() {
    let (text, loaded) = save_and_load("newline", PathBuf::from("/tmp/a\nb"));
    assert_hex_only(&text);
    assert_eq!(loaded.session_folder, Some(PathBuf::from("/tmp/a\nb")));
}

#[test]
fn a_session_folder_with_a_trailing_space_round_trips_through_hex() {
    let (text, _) = save_and_load("trailing", PathBuf::from("/tmp/a "));
    assert_hex_only(&text);
}

#[test]
fn a_session_folder_with_a_leading_space_round_trips_through_hex() {
    let (text, _) = save_and_load("leading", PathBuf::from(" /tmp/x"));
    assert_hex_only(&text);
}

#[test]
fn a_session_folder_with_a_carriage_return_round_trips_through_hex() {
    let (text, _) = save_and_load("cr", PathBuf::from("/tmp/a\rb"));
    assert_hex_only(&text);
}

#[cfg(unix)]
#[test]
fn a_session_folder_with_non_utf8_bytes_round_trips_through_hex() {
    use std::os::unix::ffi::OsStringExt;
    let folder = PathBuf::from(OsString::from_vec(vec![b'/', b't', 0xff, b'f']));
    let (text, _) = save_and_load("bytes", folder);
    assert_hex_only(&text);
    assert!(text.contains("session_folder_hex = 2f74ff66"), "{text:?}");
}

#[cfg(windows)]
#[test]
fn a_session_folder_with_an_unpaired_surrogate_round_trips_through_hex() {
    use std::os::windows::ffi::OsStringExt;
    let folder = PathBuf::from(OsString::from_wide(&[0x2f, 0xd800, 0x66]));
    let (text, _) = save_and_load("surrogate", folder);
    assert_hex_only(&text);
}

#[test]
fn a_plain_session_folder_with_spaces_inside_keeps_the_plain_line() {
    let (text, loaded) = save_and_load("inside", PathBuf::from("/scratch/wavcraft sessions"));
    assert!(text.lines().any(|l| l == "session_folder = /scratch/wavcraft sessions"), "{text:?}");
    assert!(!text.contains("session_folder_hex"));
    assert_eq!(loaded.session_folder, Some(PathBuf::from("/scratch/wavcraft sessions")));
}

#[test]
fn switching_from_a_hex_folder_to_a_plain_one_drops_the_hex_line() {
    let dir = TempDir::new("switch");
    let file = dir.0.join("settings.txt");
    Settings { session_folder: Some(PathBuf::from("/tmp/a\nb")), ..Settings::default() }.save(&file).unwrap();
    Settings { session_folder: Some(PathBuf::from("/w-new")), ..Settings::default() }.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("session_folder_hex"), "{text:?}");
    assert_eq!(Settings::load(&file).session_folder, Some(PathBuf::from("/w-new")));
}

#[test]
fn a_bad_session_folder_hex_value_is_refused_and_ignored() {
    let mut s = Settings::default();
    assert!(s.set("session_folder_hex", "zz").is_err());
    assert!(s.set("session_folder_hex", "abc").is_err(), "an odd number of digits");
    assert!(s.set("session_folder_hex", "").is_err(), "an empty path");
    assert_eq!(s.session_folder, None, "a refused value changes nothing");
    assert_eq!(Settings::parse("session_folder_hex = zz\n").session_folder, None);
    assert!(Settings::validate("session_folder_hex = zz\n").is_err());
    assert!(Settings::validate("session_folder_hex = 2f74\n").is_ok());
}

#[test]
fn the_session_folder_set_by_key_keeps_its_spaces() {
    let mut s = Settings::default();
    s.set("session_folder", "/tmp/a ").unwrap();
    assert_eq!(s.session_folder, Some(PathBuf::from("/tmp/a ")));
    s.set("session_folder", "").unwrap();
    assert_eq!(s.session_folder, None);
}
