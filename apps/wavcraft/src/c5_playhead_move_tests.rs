//! Tests for C5.3 (#1296): Move Playhead to Previous and Next (Alt+Left, Alt+Right) run in the window.
//! Stopped, the drawn playhead moves and the selection stays; paused, the play seeks there.
//!
//! Each test runs on the headless window (`WaveApp::new` on eframe's headless creation context)
//! with the silent sink in subprocesses given `WAVCRAFT_NO_AUDIO=1` before their threads start.
use std::process::Command;

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-c5-playhead-move-{}-{name}", std::process::id()));
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
    assert!(app.player.info().test_sink, "the window must use the silent sink");
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// Set the silent-sink environment before the child process starts any threads.
fn in_silent_subprocess(name: &str) -> bool {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return false;
    }
    let status = Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", &format!("app::c5_playhead_move_tests::{name}"), "--nocapture"])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .status()
        .expect("run the silent-sink test subprocess");
    assert!(status.success(), "the silent-sink test subprocess must pass");
    true
}

/// 10_000 frames on the window, with a point marker at 1_000 and the selection 4_000..6_000.
fn ten_thousand_frames_with_a_marker_and_a_selection(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 10_000]] }).expect("a 10_000 frame document").state;
    app.add_document(state, &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.selection = Selection { anchor: 1_000, head: 1_000 };
    app.run_action(ActionId::MarkerAdd, &ctx).expect("a point marker at 1_000");
    app.selection = Selection { anchor: 4_000, head: 6_000 };
}

/// Stopped: Move Playhead to Next goes to the selection end at 6_000 and keeps the selection; Move
/// Playhead to Previous twice goes to the selection start at 4_000, then to the marker at 1_000.
#[test]
fn alt_right_moves_the_playhead_to_the_selection_end_and_keeps_the_selection() {
    if in_silent_subprocess("alt_right_moves_the_playhead_to_the_selection_end_and_keeps_the_selection") {
        return;
    }
    let scratch = Scratch::new("stopped");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    ten_thousand_frames_with_a_marker_and_a_selection(&mut app);
    assert_eq!(app.transport.mode, Mode::Stopped, "the window is stopped");

    app.run_action(ActionId::PlayheadNext, &ctx).expect("Move Playhead to Next");
    assert_eq!(app.head_scrub.pos, Some(6_000), "the drawn playhead is at the selection end");
    assert_eq!((app.selection.start(), app.selection.end()), (4_000, 6_000), "the selection is still 4_000..6_000");

    app.run_action(ActionId::PlayheadPrev, &ctx).expect("Move Playhead to Previous");
    assert_eq!(app.head_scrub.pos, Some(4_000), "the drawn playhead is at the selection start");
    assert_eq!((app.selection.start(), app.selection.end()), (4_000, 6_000), "the selection is still 4_000..6_000");

    app.run_action(ActionId::PlayheadPrev, &ctx).expect("Move Playhead to Previous again");
    assert_eq!(app.head_scrub.pos, Some(1_000), "the drawn playhead is at the marker");
    assert_eq!((app.selection.start(), app.selection.end()), (4_000, 6_000), "the selection is still 4_000..6_000");
    assert_eq!(app.transport.mode, Mode::Stopped, "still stopped");
}

/// Paused: Move Playhead to Next seeks the paused play to the selection end at 6_000, and the
/// selection is kept.
#[test]
fn paused_move_playhead_to_next_seeks_the_play_and_keeps_the_selection() {
    if in_silent_subprocess("paused_move_playhead_to_next_seeks_the_play_and_keeps_the_selection") {
        return;
    }
    let scratch = Scratch::new("paused");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    ten_thousand_frames_with_a_marker_and_a_selection(&mut app);

    app.start_play();
    app.pause_play();
    assert_eq!(app.transport.mode, Mode::Paused, "the play is paused");
    app.player.seek(5_000.0);

    app.run_action(ActionId::PlayheadNext, &ctx).expect("Move Playhead to Next while paused");
    assert_eq!(app.transport.mode, Mode::Paused, "the play stays paused");
    assert_eq!(app.player.position(), 6_000, "the paused playhead is at the selection end");
    assert_eq!((app.selection.start(), app.selection.end()), (4_000, 6_000), "the selection is still 4_000..6_000");
    app.stop_play();
}
