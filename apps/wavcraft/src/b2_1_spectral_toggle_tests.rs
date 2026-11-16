//! Tests for B2.1: the action `view.spectral.toggle` (Spectral Frequency Display, Shift+D and F9)
//! flips the 'spectral' panel. It works with no file open, and a second run flips it back.
//!
//! These run on the headless window (`WaveApp::new` on eframe's headless creation context) with the
//! silent output: a subprocess receives `WAVCRAFT_NO_AUDIO=1` before its threads start, so the
//! test never plays through a real device.
use std::process::Command;

use super::*;

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-b2.1-{}-{name}", std::process::id()));
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
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// No file is opened. The action flips the spectral panel, `PANELS` shows the change, and running
/// the action again puts the panel back.
#[test]
fn view_spectral_toggle_flips_the_panel() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::b2_1_spectral_toggle_tests::view_spectral_toggle_flips_the_panel", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless spectral toggle test with silent output");
        assert!(status.success(), "the headless spectral toggle test passed");
        return;
    }
    let scratch = Scratch::new("spectral-toggle");
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();
    let before = app.panels.is_shown("spectral");

    assert!(app.control_request("ACTION view.spectral.toggle", &ctx).starts_with("OK"));
    assert_eq!(app.panels.is_shown("spectral"), !before, "the first toggle flips the spectral panel");
    let want = if before { "spectral=0" } else { "spectral=1" };
    assert!(app.control_request("PANELS", &ctx).contains(want), "PANELS shows {want} after the first toggle");

    assert!(app.control_request("ACTION view.spectral.toggle", &ctx).starts_with("OK"));
    assert_eq!(app.panels.is_shown("spectral"), before, "the second toggle puts the panel back");
}
