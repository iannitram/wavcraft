//! N4.5: the `RUN_SCRIPT <path>` control verb on the real window code (`WaveApp::control_request`, on
//! eframe's headless creation context): a script is one undo step, and a bad line puts everything back.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "n4_run_script_tests.rs"] mod n4_run_script_tests;`

use super::*;

use wavcraft_automation::Backend;
use wavcraft_automation::headless::Headless;

/// Frames in the source file.
const FRAMES: usize = 10_000;

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-n4-run-script-{}-{name}", std::process::id()));
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

/// The text of `key=` in a STATE answer (up to the next space).
fn field(answer: &str, key: &str) -> String {
    let pat = format!(" {key}=");
    let from = answer.find(&pat).map_or(answer.len(), |i| i + pat.len());
    answer[from..].split(' ').next().unwrap_or("").to_string()
}

/// How many History steps STATE reports.
fn steps(app: &mut WaveApp) -> usize {
    field(&ask(app, "STATE"), "steps").parse().expect("steps is a number")
}

/// Write source.wav in the scratch folder and open it.
fn open(app: &mut WaveApp, scratch: &Scratch) -> PathBuf {
    let path = scratch.0.join("source.wav");
    write_wav(&path, &source_samples());
    assert_eq!(ask(app, &format!("OPEN {}", path.display())), "OK");
    settle(app);
    path
}

/// A script file in the scratch folder.
fn script(scratch: &Scratch, name: &str, lines: &[&str]) -> PathBuf {
    let path = scratch.0.join(name);
    std::fs::write(&path, lines.join("\n")).expect("write the script");
    path
}

#[test]
fn one_undo_reverts_the_script() {
    let scratch = Scratch::new("one-undo");
    let mut app = a_window(&scratch);
    open(&mut app, &scratch);
    assert_eq!(steps(&mut app), 1);
    let file = script(&scratch, "script.txt", &["SELECT 1000 2000", "ACTION edit.delete", "SELECT 0 500", "EFFECT invert"]);
    assert_eq!(ask(&mut app, &format!("RUN_SCRIPT {}", file.display())), "OK replaced=2");
    let state = ask(&mut app, "STATE");
    assert_eq!(field(&state, "steps"), "2", "the script grew History by exactly one step: {state}");
    assert_eq!(field(&state, "frames"), "9000");
    assert_eq!(field(&state, "unsaved"), "1");
    assert!(ask(&mut app, "ACTION edit.undo").starts_with("OK"));
    let state = ask(&mut app, "STATE");
    assert_eq!(field(&state, "steps"), "2");
    assert_eq!(field(&state, "cursor"), "0");
    assert_eq!(field(&state, "frames"), "10000");
    let after = scratch.0.join("after.wav");
    assert_eq!(ask(&mut app, &format!("SAVE {}", after.display())), "OK");
    settle(&mut app);
    let saved = read_file(&scratch.0.join("reader"), &after);
    let source = source_samples();
    assert_eq!(saved.len(), 2);
    for c in 0..2 {
        assert_eq!(saved[c].len(), FRAMES);
        assert!(saved[c] == source[c], "channel {c} is the source again");
    }
}

#[test]
fn a_bad_line_3_changes_nothing() {
    let scratch = Scratch::new("bad-line");
    let mut app = a_window(&scratch);
    open(&mut app, &scratch);
    assert!(ask(&mut app, "SELECT 300 400").starts_with("OK"));
    let before = ask(&mut app, "STATE");
    let want = (field(&before, "steps"), field(&before, "selection"), field(&before, "unsaved"), field(&before, "frames"));
    assert_eq!((want.1.as_str(), want.2.as_str(), want.3.as_str()), ("300-400", "0", "10000"));
    let file = script(&scratch, "bad3.txt", &["SELECT 1000 2000", "ACTION edit.delete", "ACTION edit.nope"]);
    assert_eq!(ask(&mut app, &format!("RUN_SCRIPT {}", file.display())), "ERR line 3: no action named \"edit.nope\"");
    let after = ask(&mut app, "STATE");
    assert_eq!((field(&after, "steps"), field(&after, "selection"), field(&after, "unsaved"), field(&after, "frames")), want);
    assert_eq!(field(&after, "steps"), "1");
    assert_eq!(field(&after, "cursor"), "0");
}

#[test]
fn a_missing_file_and_no_path_are_refused() {
    let scratch = Scratch::new("refused");
    let mut app = a_window(&scratch);
    assert_eq!(ask(&mut app, "RUN_SCRIPT"), "ERR RUN_SCRIPT <path>");
    assert_eq!(ask(&mut app, "RUN_SCRIPT x.txt"), "ERR no file is open");
    open(&mut app, &scratch);
    let before = steps(&mut app);
    let missing = scratch.0.join("nothing.txt");
    let answer = ask(&mut app, &format!("RUN_SCRIPT {}", missing.display()));
    assert!(answer.starts_with("ERR cannot read the script:"), "{answer}");
    assert_eq!(steps(&mut app), before);
}

#[test]
fn a_failed_script_keeps_the_redo_steps() {
    let scratch = Scratch::new("keeps-redo");
    let mut app = a_window(&scratch);
    open(&mut app, &scratch);
    assert!(ask(&mut app, "SELECT 1000 2000").starts_with("OK"));
    assert!(ask(&mut app, "ACTION edit.delete").starts_with("OK"));
    assert!(ask(&mut app, "ACTION edit.undo").starts_with("OK"));
    let before = ask(&mut app, "STATE");
    assert_eq!((field(&before, "steps").as_str(), field(&before, "cursor").as_str()), ("2", "0"));
    let want = (field(&before, "selection"), field(&before, "unsaved"), field(&before, "frames"));

    let bad = script(&scratch, "bad2.txt", &["SELECT 0 10", "ACTION edit.nope"]);
    let not_allowed = script(&scratch, "open2.txt", &["SELECT 0 10", "OPEN x.wav"]);
    let cases = [(bad, "ERR line 2: no action named \"edit.nope\""), (not_allowed, "ERR line 2: OPEN is not allowed in a script")];
    for (file, answer) in cases {
        assert_eq!(ask(&mut app, &format!("RUN_SCRIPT {}", file.display())), answer);
        let after = ask(&mut app, "STATE");
        assert_eq!((field(&after, "steps").as_str(), field(&after, "cursor").as_str()), ("2", "0"), "{after}");
        assert_eq!((field(&after, "selection"), field(&after, "unsaved"), field(&after, "frames")), want);
    }
    assert!(ask(&mut app, "ACTION edit.redo").starts_with("OK"));
    assert_eq!(field(&ask(&mut app, "STATE"), "frames"), "9000");
}
