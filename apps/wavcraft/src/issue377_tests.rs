//! Tests for #377: Play pressed right after an edit must play the edited audio, not the buffer the
//! player held before the asynchronous rebuild finished.
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
        let dir = std::env::temp_dir().join(format!("wavcraft-issue377-{}-{name}", std::process::id()));
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

fn ramp(n: usize) -> DocState {
    Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| i as f32).collect()] }).unwrap().state
}

/// The issue's case: the old 100-frame buffer is held, the edit shortens it to 60 frames, Play
/// pressed while the rebuild is still running must not start on the pre-edit audio, and starts on
/// the 60 edited frames once `finish_rebuild` hands them over.
#[test]
fn a_play_on_the_edited_audio_plays_the_sixty_edited_frames() {
    if silent_child("app::issue377_tests::a_play_on_the_edited_audio_plays_the_sixty_edited_frames") {
        return;
    }
    let scratch = Scratch::new("play-edited");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    app.add_document(ramp(100), &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.doc.as_ref().map(Document::frames), Some(100));

    // Shorten the audio to 60 frames by deleting frames 60..100.
    app.selection = Selection { anchor: 60, head: 100 };
    app.run_action(ActionId::Delete, &ctx).expect("delete frames 60..100");
    assert!(app.rebuilding.is_some(), "rebuilding after edit");

    // Press Play at the start during the rebuild: it must wait for the edited audio, not start on pre-edit audio.
    app.selection = Selection { anchor: 0, head: 0 };
    app.start_play();
    assert!(!app.player.is_playing(), "play must not start on pre-edit audio");
    assert_eq!(app.transport.mode, Mode::Stopped);

    // finish_rebuild hands over the edited audio and starts the waiting play.
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rebuilding.is_some() && Instant::now() < deadline {
        app.finish_rebuild();
        if app.rebuilding.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert!(app.rebuilding.is_none(), "rebuild finished");
    assert_eq!(app.doc.as_ref().map(Document::frames), Some(60));
    assert_eq!(app.transport.mode, Mode::Playing, "the play starts on the edited audio");

    let deadline = Instant::now() + Duration::from_secs(5);
    while app.player.is_playing() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.player.is_playing(), "the 60 frames run out and the play ends");
    assert_eq!(app.player.position(), 60, "the play went over the 60 edited frames");
}

/// The app defers a Play pressed while the rebuild is pending (it must not start on the old audio)
/// and `finish_rebuild` starts it once the edited audio is in place.
#[test]
fn a_play_pressed_during_a_rebuild_waits_for_the_edited_audio() {
    let app = include_str!("app.rs");
    let start = app.find("    fn start_play(&mut self) {").expect("start_play");
    let body = &app[start..start + 400];
    assert!(body.contains("if self.rebuilding.is_some()"), "start_play does not wait for the rebuild:\n{body}");
    assert!(body.contains("self.play_after_edit = true;"), "start_play does not remember the play:\n{body}");

    let finish = app.find("    fn finish_rebuild(&mut self) {").expect("finish_rebuild");
    let fin = &app[finish..finish + 1600];
    assert!(fin.contains("std::mem::take(&mut self.play_after_edit)"), "finish_rebuild does not take the pending play");
    assert!(fin.contains("self.start_play();"), "finish_rebuild does not start the pending play");
}
