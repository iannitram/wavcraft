//! Tests for C3.4: a playhead dragged along the ruler snaps with the current snap settings, and the
//! SCRUB control verb still reports the exact frame it is given.
//!
//! The window is the headless one of issue377_tests.rs (`WaveApp::new` on eframe's headless
//! creation context), with its session folder in a scratch folder.
use std::time::Duration;

use super::*;
use wavcraft_core::{AudioBuffer, Document, Markers};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-c3-{}-{name}", std::process::id()));
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
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// A window with a 50_000-frame document open, a point marker at 48_000, and snapping on or off.
fn a_window_with_marker(scratch: &Scratch, snapping: bool) -> WaveApp {
    let mut app = a_window(scratch);
    let ctx = app.ctx.clone();
    let buffer = AudioBuffer { sample_rate: 48_000, channels: vec![(0..50_000).map(|i| i as f32).collect()] };
    let markers = Markers::from_items(vec![("Marker".to_string(), 48_000, 48_000)]);
    let state = Document::from_buffer(buffer).expect("a document").state.with_markers(Arc::new(markers));
    app.add_document(state, &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    let editor = app.editor.as_mut().expect("the document is open");
    assert_eq!(editor.history.current().markers().iter().map(|m| m.start).collect::<Vec<_>>(), vec![48_000]);
    editor.snap.enabled = snapping;
    app
}

/// A frame three pixels from the point marker (at one frame a pixel) is pulled onto it.
#[test]
fn a_playhead_dragged_a_few_pixels_from_a_marker_snaps_to_it() {
    let scratch = Scratch::new("drag-snaps");
    let app = a_window_with_marker(&scratch, true);
    assert_eq!(app.snap_playhead_drag(48_003, 1.0), 48_000);
    assert_eq!(app.snap_playhead_drag(47_997, 1.0), 48_000);
}

/// With snapping off the dragged frame is the frame it was given.
#[test]
fn with_snapping_off_the_dragged_frame_is_unchanged() {
    let scratch = Scratch::new("drag-off");
    let app = a_window_with_marker(&scratch, false);
    assert_eq!(app.snap_playhead_drag(48_003, 1.0), 48_003);
}

/// The SCRUB verb is not snapped: with snapping on, `SCRUB 47900` still reports 47_900.
#[test]
fn the_scrub_control_verb_reports_the_exact_frame() {
    let scratch = Scratch::new("scrub-exact");
    let mut app = a_window_with_marker(&scratch, true);
    let answer = ask(&mut app, "SCRUB 47900");
    assert!(answer.starts_with("OK pos=47900 "), "SCRUB must report the exact frame: {answer}");
    assert_eq!(ask(&mut app, "SCRUB end"), "OK");
}

/// The drag's snap is called from the pointer drag only: the ruler drag stores the snapped frame,
/// and the SCRUB verb keeps the raw one.
#[test]
fn the_pointer_drag_is_the_only_caller_of_the_snap() {
    let src = include_str!("app.rs");
    let src = src.split("#[cfg(test)]").next().unwrap_or(src);
    let calls: Vec<&str> = src.match_indices("snap_playhead_drag(").map(|(i, _)| &src[i.saturating_sub(60)..i]).collect();
    assert_eq!(calls.len(), 2, "the definition and one call in the pointer drag: {calls:?}");
    assert!(src.contains("self.head_scrub.pending = Some(self.snap_playhead_drag(frame_at(&self.view, pos), self.view.frames_per_px));"));
    let scrub = src.find("\"SCRUB\" => {").expect("the SCRUB verb");
    assert!(src[scrub..scrub + 400].contains("self.scrub_to(frame);"), "SCRUB must keep the exact frame");
}

/// C3.2: the control line `TOOLBAR run view.snap.toggle` replies OK, and the toolbar Snap button's
/// pressed state follows the switch it flips.
#[test]
fn the_toolbar_snap_toggle_replies_ok_and_flips_the_pressed_state() {
    let scratch = Scratch::new("toolbar-snap");
    let mut app = a_window(&scratch);
    let item = menus::toolbar_item("view.snap.toggle").expect("Snap is on the toolbar");
    let before = app.settings.snap.enabled;
    assert!(WaveApp::toolbar_item_on(&item, false, before) == before);

    let answer = ask(&mut app, "TOOLBAR run view.snap.toggle");
    assert!(answer.starts_with("OK"), "TOOLBAR run view.snap.toggle must reply OK: {answer}");
    assert_eq!(app.settings.snap.enabled, !before);
    assert_eq!(WaveApp::toolbar_item_on(&item, false, app.settings.snap.enabled), !before);
}
