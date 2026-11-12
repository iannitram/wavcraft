//! Tests for H3.1 (#1186): during playback, a Next or Previous Marker jump keeps the play going from
//! the marker. Play stops, as before, only when the marker is outside what is being played.
//!
//! Each test runs on the headless window (`WaveApp::new` on eframe's headless creation context)
//! with the silent sink (the tests set `WAVCRAFT_NO_AUDIO=1` themselves, once, so no device is opened).
use std::sync::Once;
use std::time::{Duration, Instant};

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-h3-marker-jump-{}-{name}", std::process::id()));
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
#[allow(unsafe_code)]
fn a_window(scratch: &Scratch) -> WaveApp {
    static SET: Once = Once::new();
    // SAFETY: set once, before any window of this module builds its player; nothing here changes it again.
    SET.call_once(|| unsafe { std::env::set_var("WAVCRAFT_NO_AUDIO", "1") });
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    assert!(app.player.info().test_sink, "the window must use the silent sink");
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// 3 s of 48 kHz audio on the window, with point markers at 24_000 and 72_000 and no selection.
fn three_seconds_with_two_markers(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 144_000]] }).expect("a 3 s document").state;
    app.add_document(state, &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    for frame in [24_000, 72_000] {
        app.selection = Selection { anchor: frame, head: frame };
        app.run_action(ActionId::MarkerAdd, &ctx).expect("a point marker");
    }
    app.selection = Selection { anchor: 0, head: 0 };
}

/// Wait (a few seconds at most) until the play has ended.
fn wait_for_the_play_to_end(app: &WaveApp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.player.is_playing() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Case 1: play from 0, Next Marker: the play goes on from the marker at 24_000.
#[test]
fn next_marker_during_a_play_from_the_start_carries_on_from_the_marker() {
    let scratch = Scratch::new("case1");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    three_seconds_with_two_markers(&mut app);

    app.start_play();
    assert!(app.player.is_playing(), "the play from 0 is running");

    app.run_action(ActionId::MarkerNext, &ctx).expect("Next Marker");
    assert!(app.player.is_playing(), "Next Marker during the play keeps it playing");
    assert_eq!(app.transport.mode, Mode::Playing);
    let at = app.player.position();
    assert!((24_000..48_000).contains(&at), "the playhead is in 24_000..48_000, not {at}");
}

/// Case 2: Next Marker again goes on to 72_000, and Previous Marker comes back into 24_000..48_000,
/// both still playing.
#[test]
fn next_then_previous_marker_keep_the_play_going() {
    let scratch = Scratch::new("case2");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    three_seconds_with_two_markers(&mut app);

    app.start_play();
    app.run_action(ActionId::MarkerNext, &ctx).expect("Next Marker");
    assert!(app.player.is_playing(), "the play is running after the first jump");

    app.run_action(ActionId::MarkerNext, &ctx).expect("Next Marker again");
    assert!(app.player.is_playing(), "the play is still running after the second jump");
    assert_eq!(app.transport.mode, Mode::Playing);
    let at = app.player.position();
    assert!(at >= 72_000, "the playhead is at or after 72_000, not {at}");

    app.run_action(ActionId::MarkerPrev, &ctx).expect("Previous Marker");
    assert!(app.player.is_playing(), "Previous Marker during the play keeps it playing");
    assert_eq!(app.transport.mode, Mode::Playing);
    let at = app.player.position();
    assert!((24_000..48_000).contains(&at), "the playhead is in 24_000..48_000, not {at}");
}

/// Case 3: a play of the selection 0..12_000 stops on Next Marker to 24_000, which is outside it.
#[test]
fn next_marker_outside_the_selection_being_played_stops_the_play() {
    let scratch = Scratch::new("case3");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    three_seconds_with_two_markers(&mut app);

    app.selection = Selection { anchor: 0, head: 12_000 };
    app.start_play();
    assert!(app.player.is_playing(), "the selection 0..12_000 is playing");

    app.run_action(ActionId::MarkerNext, &ctx).expect("Next Marker");
    assert!(!app.player.is_playing(), "Next Marker to 24_000 is outside 0..12_000, so the play stops");
    assert_eq!(app.transport.mode, Mode::Stopped);
    wait_for_the_play_to_end(&app);
    assert!(!app.player.is_playing(), "the play stays stopped");
}
