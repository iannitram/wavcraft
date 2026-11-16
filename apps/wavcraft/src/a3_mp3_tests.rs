//! A3.10: the MP3 size estimate through the window. `SAVEOPTS estimate mp3 16` answers the
//! constant-bit-rate size at the default 192 kbps, and the Save As size line says so.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "a3_mp3_tests.rs"] mod a3_mp3_tests;`

use super::*;

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-a3-mp3-{}-{name}", std::process::id()));
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

/// Two channels of `frames` deterministic 16-bit samples each (never -32768).
fn stereo_samples(frames: usize) -> Vec<Vec<i32>> {
    let l = (0..frames).map(|i| ((i * 7919) % 20_001) as i32 - 10_000).collect();
    let r = (0..frames).map(|i| ((i * 104_729) % 20_011) as i32 - 10_005).collect();
    vec![l, r]
}

/// A plain PCM WAV, 16-bit, 48 kHz, interleaved; the frame count is the channels' length.
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

#[test]
fn the_mp3_estimate_reaches_the_control_line() {
    let scratch = Scratch::new("estimate");
    let path = scratch.0.join("five.wav");
    // 5 s at 48 kHz, stereo: 192 kbps for 5 s is 120000 bytes.
    write_wav(&path, &stereo_samples(240_000));
    let mut app = a_window(&scratch);
    assert_eq!(ask(&mut app, &format!("OPEN {}", path.display())), "OK");
    settle(&mut app);
    let answer = ask(&mut app, "SAVEOPTS estimate mp3 16");
    assert!(answer.starts_with("OK bytes="), "{answer}");
    assert!(answer.ends_with(" exact=0"), "{answer}");
    let bytes = answer
        .strip_prefix("OK bytes=")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse::<u64>().ok())
        .expect("a byte count after bytes=");
    assert!((118_800..=121_200).contains(&bytes), "{answer}");
}

#[test]
fn the_size_line_says_constant_bit_rate() {
    let estimate = SizeEstimate { bytes: 120_000, exact: false };
    let mp3 = format_size_label(&estimate, SaveFormat::Mp3);
    assert!(mp3.contains("constant bit rate") && !mp3.contains("FLAC"), "{mp3}");
    let flac = format_size_label(&estimate, SaveFormat::Flac);
    assert!(flac.contains("FLAC"), "{flac}");
}
