//! Tests for M2.4: the navigator paints each column of the Episode Map in its class colour
//! (silence, speech or loud, from `episode_map::columns`), and the `EPISODEMAP` control verb
//! reports how many columns each class has.
//!
//! The test runs on the headless window (`WaveApp::new` on eframe's headless creation context)
//! with the silent output: a subprocess receives `WAVCRAFT_NO_AUDIO=1` before its threads start,
//! so the test never plays through a real device.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-m2.4-{}-{name}", std::process::id()));
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

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// A `key=value` field of a control answer.
fn field(answer: &str, key: &str) -> String {
    answer.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string)).unwrap_or_default()
}

/// Run frames until nothing is opening, saving or queued.
fn settle(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let deadline = Instant::now() + Duration::from_secs(60);
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
        assert!(Instant::now() < deadline, "the open did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A mono 16-bit 48 kHz WAV of 3 s: 1 s of digital silence, 1 s of a 1 kHz tone at -20 dBFS and
/// 1 s of a 1 kHz tone at -3 dBFS.
fn write_three_second_wav(path: &Path) {
    const RATE: u32 = 48_000;
    let frames = 3 * RATE;
    let data = frames * 2;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for n in 0..frames {
        let db = match n / RATE {
            0 => None,
            1 => Some(-20.0_f64),
            _ => Some(-3.0_f64),
        };
        let sample = db.map_or(0.0, |db| {
            let phase = 2.0 * std::f64::consts::PI * 1_000.0 * f64::from(n) / f64::from(RATE);
            10f64.powf(db / 20.0) * phase.sin()
        });
        let v = (sample * 32_767.0).round() as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).expect("a WAV file");
}

/// The step's case: open the 3 s file, draw one frame of the window (the navigator paints the
/// map), then ask EPISODEMAP. It answers with the columns of the strip, and silence, speech and
/// loud each hold one third of them, within 1.
#[test]
fn the_map_classes_silence_speech_and_loud() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::m2_map_tests::the_map_classes_silence_speech_and_loud", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless map test with silent output");
        assert!(status.success(), "the headless map test passed");
        return;
    }
    let scratch = Scratch::new("classes");
    let wav = scratch.0.join("three-seconds.wav");
    write_three_second_wav(&wav);
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();
    app.open(wav, &ctx);
    settle(&mut app);
    assert_eq!(app.doc.as_ref().map(|d| d.frames()), Some(3 * 48_000), "the file is 3 s");

    // One frame of the window, on a 1400 x 800 screen: the navigator paints the whole file.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 800.0));
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| app.navigator(ui));
    output.textures_delta.clear();

    let answer = ask(&mut app, "EPISODEMAP");
    assert!(answer.starts_with("OK "), "EPISODEMAP answers OK: {answer}");
    let cols: usize = field(&answer, "cols").parse().expect("cols is a count");
    assert!(cols > 0, "the strip has columns: {answer}");
    let third = cols as f64 / 3.0;
    for class in ["silence", "speech", "loud"] {
        let n: usize = field(&answer, class).parse().expect("a class count");
        assert!((n as f64 - third).abs() <= 1.0, "{class} is one third of {cols} columns (within 1): {answer}");
    }
}
