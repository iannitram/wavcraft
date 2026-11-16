//! Tests for B2.3: the four spectral scale actions and keys move the frequency scale one step or
//! fully, and stop at each end. `ACTION view.spectral.scale…` and the Ctrl+Alt keys both reach
//! `run_window_action`, and the scale shows in the `freqband=` field of `STATE`.
//!
//! These run on the headless window with the silent output: a subprocess receives
//! `WAVCRAFT_NO_AUDIO=1` before its threads start, so the test never plays through a real device.
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;
use wavcraft_core::freq_axis::Scale;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-b2.3-{}-{name}", std::process::id()));
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

/// The scale word at the end of the `freqband=` field of `STATE` (`lin`, `log_partial` or `log`).
fn state_scale_word(app: &mut WaveApp, ctx: &egui::Context) -> String {
    let state = app.control_request("STATE", ctx);
    let field = state.split_whitespace().find(|t| t.starts_with("freqband=")).unwrap_or_else(|| panic!("STATE has freqband=: {state}"));
    field.rsplit(':').next().unwrap_or_default().to_string()
}

/// Runs one action or key, checks it answered `OK`, and checks the scale and the `STATE` word.
fn step(app: &mut WaveApp, ctx: &egui::Context, request: &str, expect: Scale) {
    let answer = app.control_request(request, ctx);
    assert!(answer.starts_with("OK"), "{request} answers OK: {answer}");
    assert_eq!(app.freq_axis().scale(), expect, "after {request}");
    assert_eq!(state_scale_word(app, ctx), expect.word(), "STATE after {request}");
}

/// The step's case: from linear, the two more-log steps reach partial then full log and stop there;
/// the two more-linear steps come back and stop at linear; the full keys jump to either end.
#[test]
fn the_four_keys_move_the_frequency_scale() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::b2_3_freq_scale_tests::the_four_keys_move_the_frequency_scale", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless scale test with silent output");
        assert!(status.success(), "the headless scale test passed");
        return;
    }
    let scratch = Scratch::new("scale-keys");
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();
    three_seconds(&mut app, &ctx);

    let answer = app.control_request("SPECTRAL scale lin", &ctx);
    assert!(answer.contains("lin"), "SPECTRAL scale lin answers the band: {answer}");
    assert_eq!(app.freq_axis().scale(), Scale::Linear, "the scale starts linear");

    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLog", Scale::LogPartial);
    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLog", Scale::LogFull);
    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLog", Scale::LogFull);

    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLinear", Scale::LogPartial);
    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLinear", Scale::Linear);
    step(&mut app, &ctx, "ACTION view.spectral.scaleMoreLinear", Scale::Linear);

    step(&mut app, &ctx, "ACTION view.spectral.scaleFullLog", Scale::LogFull);
    step(&mut app, &ctx, "ACTION view.spectral.scaleFullLinear", Scale::Linear);

    // The keys themselves: Ctrl+Alt+PageUp is fully log.
    step(&mut app, &ctx, "KEY Ctrl+Alt+PageUp", Scale::LogFull);
}
