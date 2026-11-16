//! Tests for J1.3 (the Recordings folder row in Preferences, Playback and Recording): the row's
//! preference goes through `pref_set`, sets the folder a take is saved in, and the Default button
//! clears it again.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "j1_recordings_row_tests.rs"] mod j1_recordings_row_tests;`

use super::*;
use std::path::PathBuf;

/// A folder for one test, removed when it ends. Declare it before the window, so the window is
/// dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-j1-row-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The window as the program builds it, with its session folder in `scratch`. Its settings file and
/// recent-files list are cut loose at once, so nothing a test does is written to the user's own.
/// (With no settings file, `pref_set` changes the setting and then returns an Err: tests ignore it.)
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// The row's Choose and Default buttons set the preference: a folder is stored, and an empty value
/// (what Default sends) clears it back to none.
#[test]
fn the_recordings_folder_row_sets_and_clears_the_preference() {
    let scratch = Scratch::new("sets-clears");
    let mut app = a_window(&scratch);
    assert_eq!(app.settings.recordings_folder, None);

    let _ = app.pref_set("recordings_folder", &scratch.0.join("takes").to_string_lossy());
    assert_eq!(app.settings.recordings_folder, Some(scratch.0.join("takes")));

    let _ = app.pref_set("recordings_folder", "");
    assert_eq!(app.settings.recordings_folder, None);
}

/// A take goes to the chosen folder while the preference is set, and to the default folder again
/// once it is cleared.
#[test]
fn a_take_goes_to_the_chosen_recordings_folder() {
    let scratch = Scratch::new("take-goes");
    let mut app = a_window(&scratch);

    let _ = app.pref_set("recordings_folder", &scratch.0.join("takes").to_string_lossy());
    assert_eq!(
        crate::recording::record_dir_from(None, app.settings.recordings_folder.as_deref(), Some(PathBuf::from("/d"))),
        Ok(scratch.0.join("takes"))
    );

    let _ = app.pref_set("recordings_folder", "");
    assert_eq!(
        crate::recording::record_dir_from(None, app.settings.recordings_folder.as_deref(), Some(PathBuf::from("/d"))),
        Ok(PathBuf::from("/d"))
    );
}
