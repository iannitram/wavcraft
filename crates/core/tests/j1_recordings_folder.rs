//! Locked J1.1 persistence contract for the recordings folder preference.

use std::path::PathBuf;
use wavcraft_core::settings::{self, Settings};

#[test]
fn j1_recordings_folder_reads_writes_and_clears() {
    let parsed = Settings::parse("recordings_folder = /tmp/My Takes\n");
    assert_eq!(parsed.recordings_folder, Some(PathBuf::from("/tmp/My Takes")));

    let trimmed = Settings::parse("recordings_folder =   /tmp/My Takes   \n");
    assert_eq!(trimmed.recordings_folder, Some(PathBuf::from("/tmp/My Takes")));
    assert_eq!(Settings::parse("recordings_folder =  \n").recordings_folder, None);

    let default_text = Settings::default().to_text();
    assert!(!default_text.contains("recordings_folder"));

    let mut settings = Settings::default();
    settings.set("recordings_folder", "/tmp/x").unwrap();
    assert_eq!(settings.get("recordings_folder").unwrap(), "/tmp/x");
    assert!(settings::KEYS.contains(&"recordings_folder"));

    settings.set("recordings_folder", "  ").unwrap();
    assert_eq!(settings.recordings_folder, None);
    settings.set("recordings_folder", "/tmp/x").unwrap();

    let path = std::env::temp_dir().join(format!("wavcraft-j1-recordings-folder-{}.txt", std::process::id()));
    settings.save(&path).unwrap();
    let loaded = Settings::load(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(loaded.recordings_folder, Some(PathBuf::from("/tmp/x")));
}
