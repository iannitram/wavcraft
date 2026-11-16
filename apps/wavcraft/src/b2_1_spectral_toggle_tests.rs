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

/// A short mono 16-bit 48 kHz WAV, enough to open.
fn write_wav(path: &Path) {
    let frames: u32 = 4_800;
    let data_len = frames * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&(48_000u32 * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for f in 0..frames {
        let s = ((f * 97) % 2000) as i16 - 1000;
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// Run frames until the open has finished, as the window does while it loads.
fn settle(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.finish_opening();
            app.finish_saving(ui.ctx());
            app.pump_open_queue(ui.ctx());
        });
        output.textures_delta.clear();
        if app.opening.is_none() && app.saving.is_none() && app.open_queue.is_empty() && !app.open_after_save {
            return;
        }
        assert!(Instant::now() < deadline, "the window did not settle");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The FFT size `SPECTRAL` reports.
fn fft_of(app: &mut WaveApp, ctx: &egui::Context) -> u32 {
    app.control_request("SPECTRAL", ctx)
        .split_whitespace()
        .find_map(|w| w.strip_prefix("fft="))
        .and_then(|v| v.parse().ok())
        .expect("SPECTRAL reports fft=")
}

/// With a file open, `ACTION view.spectral.resolutionDown` then `resolutionUp` each answer OK, and
/// each changes the spectral FFT size (default 2048): down to 1024, then back to 2048.
#[test]
fn spectral_resolution_actions_answer_ok() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::b2_1_spectral_toggle_tests::spectral_resolution_actions_answer_ok", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless spectral resolution test with silent output");
        assert!(status.success(), "the headless spectral resolution test passed");
        return;
    }
    let scratch = Scratch::new("spectral-resolution");
    let path = scratch.0.join("tone.wav");
    write_wav(&path);
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    assert_eq!(app.control_request(&format!("OPEN {}", path.display()), &ctx), "OK");
    settle(&mut app);
    let before = fft_of(&mut app, &ctx);

    let down = app.control_request("ACTION view.spectral.resolutionDown", &ctx);
    assert!(down.starts_with("OK"), "resolutionDown answers OK, not {down}");
    let lower = fft_of(&mut app, &ctx);
    assert!(lower < before, "resolutionDown shortens the FFT: {before} -> {lower}");

    let up = app.control_request("ACTION view.spectral.resolutionUp", &ctx);
    assert!(up.starts_with("OK"), "resolutionUp answers OK, not {up}");
    assert_eq!(fft_of(&mut app, &ctx), before, "resolutionUp goes back to the FFT size it started at");
}
