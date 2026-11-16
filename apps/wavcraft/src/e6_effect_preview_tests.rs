//! Tests for E6.2: the `EFFECT_DIALOG PREVIEW` control verb, on the headless window with
//! `WAVCRAFT_NO_AUDIO=1` (the silent sink, 48 kHz).
use std::ffi::OsStr;
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;
use crate::player::{OutputChoice, Player};
use wavcraft_core::{AudioBuffer, Document};

/// Runs the test at `path` (its full path) again in a child process that has `WAVCRAFT_NO_AUDIO=1`
/// before its threads start, and returns true when it did, so the caller returns. Nothing sets the
/// variable in this process: `set_var` is unsafe while other test threads may read the environment.
fn silent_child(path: &str) -> bool {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() == Some(OsStr::new("1")) {
        return false;
    }
    let out = Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", path])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .output()
        .expect("run the test with silent output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{path} failed with silent output:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    true
}

/// A player on the silent sink (no test needs a device). A test that calls this, directly or through
/// `a_window`, starts with `silent_child`.
fn silent_player() -> Player {
    let p = Player::new(OutputChoice::default(), ChannelMap::default(), Quality::default());
    assert!(p.has_output() && p.info().test_sink, "the silent sink is the output");
    p
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-e6-{}-{name}", std::process::id()));
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
    let _ = silent_player();
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// A `key=value` field of a STATE answer.
fn field(state: &str, key: &str) -> String {
    state.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string)).unwrap_or_default()
}

const FRAMES: usize = 48_000;

/// Load a 48 kHz mono file of 0.5 and let its rebuild finish.
fn a_loaded_window(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; FRAMES]] }).expect("a document").state;
    app.add_document(state, &ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rebuilding.is_some() && Instant::now() < deadline {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.rebuilding.is_none(), "the file finished rebuilding");
}

#[test]
fn e6_effect_preview_starts_and_stops() {
    if silent_child("app::e6_effect_preview_tests::e6_effect_preview_starts_and_stops") {
        return;
    }
    let scratch = Scratch::new("starts");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    let before = ask(&mut app, "STATE");
    let (steps, undo) = (field(&before, "steps"), field(&before, "undo"));
    assert_eq!(ask(&mut app, "EFFECT_DIALOG amplify left_db=-6"), "OK");
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "OK preview=1");
    assert!(app.effect_preview.as_ref().is_some_and(|p| p.is_running()));
    assert_eq!(app.effect_preview.as_ref().and_then(|p| p.error()), None);
    let during = ask(&mut app, "STATE");
    assert_eq!(field(&during, "steps"), steps);
    assert_eq!(field(&during, "undo"), undo);
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "OK preview=0");
    assert!(app.effect_preview.is_none());
    assert_eq!(field(&ask(&mut app, "STATE"), "steps"), steps);
}

#[test]
fn e6_effect_preview_needs_an_open_effect_dialog() {
    if silent_child("app::e6_effect_preview_tests::e6_effect_preview_needs_an_open_effect_dialog") {
        return;
    }
    let scratch = Scratch::new("needs");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "ERR no effect dialog is open");
    assert!(app.effect_preview.is_none());
    assert_eq!(ask(&mut app, "GENERATE_DIALOG tone"), "OK");
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "ERR no effect dialog is open");
    assert!(app.effect_preview.is_none());
}

#[test]
fn e6_effect_preview_stops_when_the_dialog_closes() {
    if silent_child("app::e6_effect_preview_tests::e6_effect_preview_stops_when_the_dialog_closes") {
        return;
    }
    let scratch = Scratch::new("closes");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    assert_eq!(ask(&mut app, "EFFECT_DIALOG amplify left_db=-6"), "OK");
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "OK preview=1");
    assert!(app.effect_preview.is_some());
    assert!(ask(&mut app, "EFFECT_DIALOG CANCEL").starts_with("OK"));
    assert!(app.effect_preview.is_none());
}

#[test]
fn e6_effect_preview_stops_when_another_dialog_opens() {
    if silent_child("app::e6_effect_preview_tests::e6_effect_preview_stops_when_another_dialog_opens") {
        return;
    }
    let scratch = Scratch::new("another");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    assert_eq!(ask(&mut app, "EFFECT_DIALOG amplify left_db=-6"), "OK");
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "OK preview=1");
    assert_eq!(ask(&mut app, "GENERATE_DIALOG tone"), "OK");
    assert!(app.effect_preview.is_none(), "the Amplify worker is gone");
    assert!(!app.player.has_monitor());
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "ERR no effect dialog is open");
}

#[test]
fn e6_effect_preview_stops_on_a_document_switch() {
    if silent_child("app::e6_effect_preview_tests::e6_effect_preview_stops_on_a_document_switch") {
        return;
    }
    let scratch = Scratch::new("switch");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    assert_eq!(ask(&mut app, "EFFECT_DIALOG amplify left_db=-6"), "OK");
    assert_eq!(ask(&mut app, "EFFECT_DIALOG PREVIEW"), "OK preview=1");
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.1; 4_800]] }).expect("a document").state;
    app.add_document(state, &ctx);
    assert!(app.effect_preview.is_none());
    assert!(!app.player.has_monitor());
}

#[test]
fn e6_closing_a_dialog_keeps_a_recordings_monitor() {
    if silent_child("app::e6_effect_preview_tests::e6_closing_a_dialog_keeps_a_recordings_monitor") {
        return;
    }
    let scratch = Scratch::new("recmon");
    let mut app = a_window(&scratch);
    a_loaded_window(&mut app);
    // A recording's input monitor, as `attach_monitor` sets it.
    let ring = wavcraft_core::record::MonitorRing::new(4_800, 1);
    app.player.set_monitor(Some((ring, 48_000))).expect("the silent output takes a monitor");
    assert!(app.player.has_monitor());
    assert_eq!(ask(&mut app, "EFFECT_DIALOG amplify left_db=-6"), "OK");
    assert!(ask(&mut app, "EFFECT_DIALOG CANCEL").starts_with("OK"));
    assert!(app.effect_preview.is_none());
    assert!(app.player.has_monitor(), "no preview ran, so the recording's monitor stays");
}
