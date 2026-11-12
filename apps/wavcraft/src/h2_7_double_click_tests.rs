//! Tests for H2.7: a double-click on a Markers row goes to the marker. The playhead moves to it and
//! a range marker's span becomes the selection. The control line `MARKER select <i>` does the same.
//! A single click only picks the row and leaves the selection as it was.
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
        let dir = std::env::temp_dir().join(format!("wavcraft-h2.7-{}-{name}", std::process::id()));
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

/// The step's case: a point marker at 10_000 and a range marker 30_000..40_000. `MARKER select 1`
/// replies with the range's span and the playhead goes to 30_000; `MARKER select 0` shows a cursor
/// at 10_000; a single click on a row leaves the selection where it was.
#[test]
fn double_click_goes_to_the_marker_span() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::h2_7_double_click_tests::double_click_goes_to_the_marker_span", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless marker test with silent output");
        assert!(status.success(), "the headless marker test passed");
        return;
    }
    let scratch = Scratch::new("double-click");
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();
    three_seconds(&mut app, &ctx);

    // Two markers, made the way the control channel makes them: the point first (index 0).
    assert!(app.control_request("SELECT 10000 10000", &ctx).starts_with("OK"));
    assert!(app.control_request("ACTION marker.add", &ctx).starts_with("OK"));
    assert!(app.control_request("SELECT 30000 40000", &ctx).starts_with("OK"));
    assert!(app.control_request("ACTION marker.add", &ctx).starts_with("OK"));
    let markers = app.editor.as_ref().expect("the document is open").history.current().markers().clone();
    let spans: Vec<(u64, u64)> = markers.iter().map(|m| (m.start, m.end)).collect();
    assert_eq!(spans, vec![(10_000, 10_000), (30_000, 40_000)], "a point at 10_000 and a range 30_000..40_000");

    // A single click on the range's row picks it and leaves the selection where it is.
    app.selection = Selection { anchor: 5_000, head: 6_000 };
    let picked = app.markers_ui.row_click(&markers, 1, false, false, false, false);
    assert_eq!(picked, None, "a single click only picks");
    assert_eq!(app.selection, Selection { anchor: 5_000, head: 6_000 }, "the selection is unchanged by a single click");
    assert_eq!(app.markers_ui.pick.ids, vec![markers.id_at(1).expect("the range")], "the range is picked");

    // The double-click's control line: the span is the selection and the playhead is at its start.
    assert_eq!(app.control_request("MARKER select 1", &ctx), "OK selection=30000-40000");
    assert_eq!(app.selection, Selection { anchor: 30_000, head: 40_000 });
    let state = app.control_request("STATE", &ctx);
    assert!(state.contains(" cursor_frame=30000 "), "the playhead is at 30_000: {state}");
    assert_eq!(app.playhead_secs(), Some(30_000.0 / 48_000.0), "the playhead is at 30_000 frames");

    // A point marker: the reply shows a cursor at 10_000.
    assert_eq!(app.control_request("MARKER select 0", &ctx), "OK selection=10000-10000");
    assert!(app.control_request("STATE", &ctx).contains(" cursor_frame=10000 "), "the cursor is at 10_000");
}
