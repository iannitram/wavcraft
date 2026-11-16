//! Tests for E5.2 (#1419, #31): Repeat Last Command with its dialog (F2, Ctrl+R) in the window asks
//! `Editor::repeat_dialog` and opens the dialog from its answer. After Amplify at 3 dB it reopens
//! the Amplify dialog with those settings and changes no audio and no History step; with nothing
//! to repeat it says "not available now" and opens nothing.
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
        let dir = std::env::temp_dir().join(format!("wavcraft-e5-repeat-dialog-{}-{name}", std::process::id()));
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
        .args(["--exact", &format!("app::e5_repeat_dialog_tests::{name}"), "--nocapture"])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .status()
        .expect("run the silent-sink test subprocess");
    assert!(status.success(), "the silent-sink test subprocess must pass");
    true
}

/// 1_000 frames of 0.25 on channel 0 (and channel 1), the whole file selected by default.
fn a_thousand_frames(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let channels = vec![vec![0.25; 1_000], vec![0.25; 1_000]];
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels }).expect("a 1_000 frame document").state;
    app.add_document(state, &ctx);
    wait_for_rebuild(app);
}

fn wait_for_rebuild(app: &mut WaveApp) {
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// The current document's channel 0, all 1_000 frames.
fn channel_zero(app: &WaveApp) -> Vec<f32> {
    let editor = app.editor.as_ref().expect("an editor");
    let mut buf = vec![0.0f32; 1_000];
    editor.history.current().read(0, 0, &mut buf).expect("channel 0 reads");
    buf
}

/// Amplify at 3 dB, then F2 / Ctrl+R (the repeat-with-dialog action): the Amplify dialog is open
/// with left_db 3.0, and the audio and the History steps are the same as before the key press.
#[test]
fn e5_ctrl_r_reopens_dialog_with_last_params() {
    if in_silent_subprocess("e5_ctrl_r_reopens_dialog_with_last_params") {
        return;
    }
    let scratch = Scratch::new("reopen");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    a_thousand_frames(&mut app);
    assert!(app.effect_dialog.is_none(), "no effect dialog is open at the start");

    let i = wavcraft_core::effects::all().iter().position(|e| e.id() == "amplify").expect("amplify is registered") as u16;
    app.run_effect(i, vec![3.0, 0.0, 1.0], &ctx).expect("Amplify 3 dB");
    wait_for_rebuild(&mut app);

    let before_steps = app.editor.as_ref().expect("an editor").history.steps().len();
    let before = channel_zero(&app);
    assert!((before[0] - 0.25 * 10f32.powf(3.0 / 20.0)).abs() < 1e-3, "Amplify 3 dB really ran: {}", before[0]);

    app.run_action(ActionId::RepeatLastDialog, &ctx).expect("Repeat Last Command...");
    let dialog = app.effect_dialog.as_ref().expect("the effect dialog is open");
    assert_eq!(dialog.index, i, "the dialog is for Amplify");
    assert_eq!(dialog.id(), "amplify", "the dialog is Amplify");
    assert_eq!(dialog.values, vec![3.0, 0.0, 1.0], "the dialog starts from the last settings (left_db 3.0)");
    assert!(!dialog.generator, "an effect dialog, not a generator");
    assert!(dialog.rack_slot.is_none(), "not opened from the rack");

    assert_eq!(app.editor.as_ref().expect("an editor").history.steps().len(), before_steps, "opening the dialog adds no History step");
    assert_eq!(channel_zero(&app), before, "opening the dialog changes no audio");
}

/// Nothing has run yet: F2 / Ctrl+R answers "not available now" and opens no dialog.
#[test]
fn e5_ctrl_r_with_nothing_to_repeat_reports_not_available() {
    if in_silent_subprocess("e5_ctrl_r_with_nothing_to_repeat_reports_not_available") {
        return;
    }
    let scratch = Scratch::new("nothing");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    a_thousand_frames(&mut app);

    let err = app.run_action(ActionId::RepeatLastDialog, &ctx).expect_err("nothing to repeat");
    assert!(err.ends_with("is not available now"), "the message says not available now: {err}");
    assert!(app.effect_dialog.is_none(), "no effect dialog opens");
}
