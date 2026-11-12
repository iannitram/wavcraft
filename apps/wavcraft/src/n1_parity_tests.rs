//! N1.5: the same steps through the real window code (`WaveApp::control_request`, on eframe's
//! headless creation context, as issue366_tests.rs builds it) and through `Headless`, then the
//! two saved files compared sample for sample.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "n1_parity_tests.rs"] mod n1_parity_tests;`

use super::*;

use wavcraft_automation::Backend;
use wavcraft_automation::headless::Headless;

/// Frames in the source file.
const FRAMES: usize = 10_000;

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-n1-parity-{}-{name}", std::process::id()));
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

/// Two channels of distinct, deterministic 16-bit samples (never -32768, so inverting is exact).
fn source_samples() -> Vec<Vec<i32>> {
    let l = (0..FRAMES).map(|i| ((i * 7919) % 20_001) as i32 - 10_000).collect();
    let r = (0..FRAMES).map(|i| ((i * 104_729) % 20_011) as i32 - 10_005).collect();
    vec![l, r]
}

/// A plain PCM WAV, 16-bit, 48 kHz, interleaved.
fn write_wav(path: &Path, channels: &[Vec<i32>]) {
    let ch = channels.len() as u16;
    let frames = channels[0].len();
    let data_len = (frames * usize::from(ch) * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&ch.to_le_bytes());
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&(48_000 * u32::from(ch) * 2).to_le_bytes());
    out.extend_from_slice(&(ch * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for f in 0..frames {
        for c in channels {
            out.extend_from_slice(&(c[f] as i16).to_le_bytes());
        }
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// Every channel of the file at `path`, as 16-bit sample values, read by a fresh `Headless`.
fn read_file(sessions: &Path, path: &Path) -> Vec<Vec<i32>> {
    let mut reader = Headless::new(sessions);
    assert_eq!(reader.answer_line(&format!("OPEN {}", path.display())), "OK");
    let doc = reader.document().expect("a document is open");
    (0..doc.channel_count())
        .map(|ch| {
            let mut buf = vec![0f32; doc.len_frames() as usize];
            let n = doc.read(ch, 0, &mut buf).expect("read a channel");
            assert_eq!(n, buf.len(), "a whole channel reads back");
            buf.iter().map(|v| (v * 32_768.0).round() as i32).collect()
        })
        .collect()
}

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

/// Run frames until nothing is opening, saving or queued.
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

/// The four steps between OPEN and SAVE (six in all). Each answer must start with OK.
const STEPS: [&str; 4] = ["SELECT 1000 2000", "ACTION edit.delete", "SELECT 0 500", "EFFECT invert"];

#[test]
fn the_window_and_headless_save_the_same_samples() {
    let scratch = Scratch::new("same");
    let (win_file, head_file) = (scratch.0.join("window.wav"), scratch.0.join("headless.wav"));
    write_wav(&win_file, &source_samples());
    write_wav(&head_file, &source_samples());

    // The window, with the default crossfades set by name. With no settings file (the test
    // window has none) PREF answers that the setting lasts until close; it is set all the same.
    let mut app = a_window(&scratch);
    for pref in ["PREF set crossfade_cut_join_ms 2", "PREF set crossfade_edge_ms 5"] {
        let reply = ask(&mut app, pref);
        assert!(reply.starts_with("OK") || reply.contains("lasts until WavCraft closes"), "{pref}: {reply}");
    }
    let (cut_join, edge) = app.settings.crossfades();
    assert_eq!((cut_join.as_millis(), edge.as_millis()), (2, 5), "the window's crossfades are set");
    assert_eq!(ask(&mut app, &format!("OPEN {}", win_file.display())), "OK");
    settle(&mut app);
    for step in STEPS {
        let reply = ask(&mut app, step);
        assert!(reply.starts_with("OK"), "window {step}: {reply}");
    }
    assert_eq!(ask(&mut app, "SAVE"), "OK");
    settle(&mut app);
    assert_eq!(ask(&mut app, "LASTERROR"), "OK -", "the window's save ended well");

    // Headless, with the same crossfades.
    let head_sessions = scratch.0.join("headless-sessions");
    std::fs::create_dir_all(&head_sessions).expect("a sessions folder");
    let mut head = Headless::with_crossfades(&head_sessions, wavcraft_core::doc::edit::Crossfades::default());
    assert_eq!(head.answer_line(&format!("OPEN {}", head_file.display())), "OK");
    for step in STEPS {
        let reply = head.answer_line(step);
        assert!(reply.starts_with("OK"), "headless {step}: {reply}");
    }
    assert_eq!(head.answer_line("SAVE"), "OK");

    let source = source_samples();
    let from_window = read_file(&head_sessions, &win_file);
    let from_headless = read_file(&head_sessions, &head_file);
    assert_eq!(from_window.len(), 2, "both channels");
    for c in 0..2 {
        assert_eq!(from_window[c].len(), FRAMES - 1000, "channel {c}: the window's file is 1000 frames shorter");
        assert_eq!(from_headless[c].len(), FRAMES - 1000, "channel {c}: the headless file is 1000 frames shorter");
        assert_ne!(from_window[c][..500], source[c][..500], "channel {c}: frames 0 to 499 were inverted");
    }
    assert!(from_window == from_headless, "the window and headless saved different samples");
}
