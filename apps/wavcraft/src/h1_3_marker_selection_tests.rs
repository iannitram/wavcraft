//! Tests for H1.3: Add Marker during playback keeps the selection the user made. The marker goes
//! at the playhead (a point), the selection stays where it was, and playback goes on.
//!
//! These run on the headless window (`WaveApp::new` on eframe's headless creation context) with the
//! silent output: a subprocess receives `WAVCRAFT_NO_AUDIO=1` before its threads start, so the
//! test never plays through a real device.
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-h1.3-{}-{name}", std::process::id()));
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

/// Three seconds of 48 kHz audio, opened in the window.
fn three_seconds(app: &mut WaveApp, ctx: &egui::Context) {
    let n = 3 * 48_000;
    let audio = AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| (i % 100) as f32 / 100.0).collect()] };
    let state = Document::from_buffer(audio).expect("a 3 s document").state;
    app.add_document(state, ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rebuilding.is_some() && Instant::now() < deadline {
        app.finish_rebuild();
        if app.rebuilding.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert!(app.rebuilding.is_none(), "the document finished opening");
    assert_eq!(app.doc.as_ref().map(|d| d.frames()), Some(n), "the document is 3 s");
}

/// The step's case: loop on, selection 24_000..72_000, play from it, then Add Marker during play.
/// One point marker is added inside the selection, the selection stays 24_000..72_000 and playback
/// is still running.
#[test]
fn add_marker_during_playback_keeps_the_selection() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::h1_3_marker_selection_tests::add_marker_during_playback_keeps_the_selection", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless playback test with silent output");
        assert!(status.success(), "the headless playback test passed");
        return;
    }
    let scratch = Scratch::new("keeps-selection");
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();
    three_seconds(&mut app, &ctx);

    app.transport.looping = true;
    app.player.set_looping(true);
    app.selection = Selection { anchor: 24_000, head: 72_000 };
    app.start_play();
    assert_eq!(app.transport.mode, Mode::Playing, "playback started");
    assert!(app.player.is_playing(), "the player is running");

    app.run_action(ActionId::MarkerAdd, &ctx).expect("Add Marker during playback");

    // A marker edit changes the history only (no rebuild), so the markers are read from the editor.
    let editor = app.editor.as_ref().expect("the document is open");
    let markers = editor.history.current().markers();
    assert_eq!(markers.len(), 1, "one marker exists: {:?}", markers.as_slice());
    let marker = &markers.as_slice()[0];
    assert!(marker.is_point(), "the marker is a point: {marker:?}");
    assert!((24_000..72_000).contains(&marker.start), "the marker is inside 24_000..72_000: {marker:?}");
    assert_eq!(app.selection, Selection { anchor: 24_000, head: 72_000 }, "the selection is still 24_000..72_000");
    assert!(app.player.is_playing(), "playback is still running");
    assert_eq!(app.transport.mode, Mode::Playing, "the transport is still playing");
}
