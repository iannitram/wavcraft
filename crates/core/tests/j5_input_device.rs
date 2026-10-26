//! Locked J5 persistence contract. No audio hardware or process-global settings are used.

use std::path::PathBuf;
use wavcraft_core::settings::Settings;

const LEGACY: &str = "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = \n";

#[test]
fn j5_input_device_reads_trimmed_name_and_preserves_other_settings() {
    let settings = Settings::parse("session_folder = scratch/sessions\nhidden_panels = history,spectral\n  input_device =  USB Mic  \n");
    assert_eq!(settings.input_device.as_deref(), Some("USB Mic"));
    assert_eq!(settings.session_folder, Some(PathBuf::from("scratch/sessions")));
    assert_eq!(settings.hidden_panels, vec!["history", "spectral"]);
}

#[test]
fn j5_input_device_preserves_unicode_equals_and_hash_in_name() {
    let settings = Settings::parse("# input_device = ignored\ninput_device = Mic = Café #2\n");
    assert_eq!(settings.input_device.as_deref(), Some("Mic = Café #2"));
}

#[test]
fn j5_input_device_last_assignment_wins() {
    let settings = Settings::parse("input_device = Built-in Mic\ninput_device = USB Mic\n");
    assert_eq!(settings.input_device.as_deref(), Some("USB Mic"));
}

#[test]
fn j5_input_device_empty_assignment_restores_default() {
    assert_eq!(Settings::parse("input_device = USB Mic\n").input_device.as_deref(), Some("USB Mic"));
    assert_eq!(Settings::parse("input_device = USB Mic\ninput_device =  \n").input_device, None);
    assert_eq!(Settings::parse("session_folder = scratch/sessions\n").input_device, None);
    assert_eq!(Settings::default().input_device, None);
    assert_eq!(Settings::default().to_text(), LEGACY);
}

#[test]
fn j5_input_device_writes_exact_text_and_round_trips() {
    let settings = Settings { input_device: Some("Mic = Café #2".into()), ..Default::default() };
    assert_eq!(
        settings.to_text(),
        "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = \ninput_device = Mic = Café #2\n"
    );
    assert_eq!(Settings::parse(&settings.to_text()).input_device.as_deref(), Some("Mic = Café #2"));
}

#[test]
fn j5_input_device_appends_after_hidden_panels() {
    let settings =
        Settings { session_folder: Some(PathBuf::from("scratch")), hidden_panels: vec!["history".into()], input_device: Some("USB Mic".into()) };
    assert_eq!(
        settings.to_text(),
        "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = scratch\n# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\nhidden_panels = history\ninput_device = USB Mic\n"
    );
}

#[test]
fn j5_input_device_survives_disk_reload_and_can_be_cleared() {
    let path = std::env::temp_dir().join(format!("wavcraft-j5-input-device-{}.txt", std::process::id()));
    let settings = Settings { input_device: Some("USB Mic".into()), ..Default::default() };
    settings.save(&path).unwrap();
    let loaded = Settings::load(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded.input_device.as_deref(), Some("USB Mic"));

    let cleared = Settings { input_device: None, ..loaded };
    cleared.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let reloaded = Settings::load(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(text, LEGACY);
    assert_eq!(reloaded.input_device, None);
}
