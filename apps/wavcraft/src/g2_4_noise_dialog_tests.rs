//! Tests for G2.4 (#1396): Ctrl+Shift+P opens the Noise Reduction dialog, and the audio is unchanged
//! until its OK is pressed.
//!
//! Each test runs on the headless window (`WaveApp::new` on eframe's headless creation context)
//! with the silent sink in subprocesses given `WAVCRAFT_NO_AUDIO=1` before their threads start.
use std::process::Command as Process;

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-g2-4-noise-dialog-{}-{name}", std::process::id()));
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
    let status = Process::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", &format!("app::g2_4_noise_dialog_tests::{name}"), "--nocapture"])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .status()
        .expect("run the silent-sink test subprocess");
    assert!(status.success(), "the silent-sink test subprocess must pass");
    true
}

/// 10_000 frames of silence on the window.
fn ten_thousand_frames_of_silence(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 10_000]] }).expect("a 10_000 frame document").state;
    app.add_document(state, &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Ctrl+Shift+P, read as the window reads its keys each frame.
fn ctrl_shift_p_commands() -> Vec<Command> {
    let ctx = egui::Context::default();
    let keymap = Keymap::new();
    let press = egui::Event::Key {
        key: egui::Key::P,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    };
    let mut got = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput { events: vec![press], ..Default::default() }, |ui| {
        got = pressed_commands(ui.ctx(), &keymap, false);
    });
    output.textures_delta.clear();
    got
}

/// Ctrl+Shift+P opens the Noise Reduction dialog and does not run the effect: the audio is still
/// the 10_000 frames it was, with nothing to undo.
#[test]
fn ctrl_shift_p_opens_the_noise_reduction_dialog() {
    if in_silent_subprocess("ctrl_shift_p_opens_the_noise_reduction_dialog") {
        return;
    }
    let scratch = Scratch::new("opens");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    ten_thousand_frames_of_silence(&mut app);
    assert!(app.effect_dialog.is_none(), "no dialog is open before the key");

    assert_eq!(ctrl_shift_p_commands(), vec![Command::NoiseReductionDialog]);
    assert_eq!(app.run_command(Command::NoiseReductionDialog, &ctx), Ok(false), "the dialog opens and the document does not change");

    let dialog = app.effect_dialog.as_ref().expect("the Noise Reduction dialog is open");
    assert_eq!(dialog.id(), "noise_reduction");
    assert!(!dialog.generator, "Noise Reduction is an effect, not a generator");

    let editor = app.editor.as_ref().expect("the file is still open");
    assert_eq!(editor.history.current().len_frames(), 10_000, "the audio is still 10_000 frames");
    assert!(!editor.history.can_undo(), "nothing was run, so there is nothing to undo");
}

/// With no file open, the command is an error and no dialog opens.
#[test]
fn noise_reduction_dialog_command_with_no_file_open_is_an_error() {
    if in_silent_subprocess("noise_reduction_dialog_command_with_no_file_open_is_an_error") {
        return;
    }
    let scratch = Scratch::new("no-file");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    assert!(app.editor.is_none(), "no file is open");

    assert_eq!(app.run_command(Command::NoiseReductionDialog, &ctx), Err("no file is open".to_string()));
    assert!(app.effect_dialog.is_none(), "no dialog opens");
}
