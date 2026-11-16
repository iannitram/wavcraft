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
fn the_menu_entry_runs_the_picked_script() {
    let scratch = Scratch::new("menu-entry");
    let mut app = a_window(&scratch);
    open(&mut app, &scratch);
    let five = script(&scratch, "five.txt", &["SELECT 1000 2000", "ACTION edit.delete", "SELECT 0 500", "EFFECT invert", "SELECT 0 10"]);
    let bad3 = script(&scratch, "bad3.txt", &["SELECT 1000 2000", "ACTION edit.delete", "ACTION edit.nope"]);
    let ctx = app.ctx.clone();
    assert_eq!(steps(&mut app), 1);

    app.scripted_pick = Some(None);
    app.pick_run_script(&ctx);
    assert!(app.error.is_none(), "a cancelled pick sets no error");
    assert_eq!(steps(&mut app), 1);
    assert_eq!(field(&ask(&mut app, "STATE"), "frames"), "10000");
    assert!(app.scripted_pick.is_none(), "the pick was taken");

    app.scripted_pick = Some(Some(bad3.clone()));
    app.pick_run_script(&ctx);
    assert!(app.error.as_deref().is_some_and(|e| e.starts_with("line 3:")), "{:?}", app.error);
    assert_eq!(steps(&mut app), 1);
    assert_eq!(field(&ask(&mut app, "STATE"), "frames"), "10000");

    app.scripted_pick = Some(Some(five.clone()));
    app.pick_run_script(&ctx);
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(steps(&mut app), 2, "the script added exactly one step");
    assert_eq!(field(&ask(&mut app, "STATE"), "frames"), "9000");
}

#[test]
fn the_menu_entry_needs_an_open_file() {
    let scratch = Scratch::new("menu-no-doc");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    app.scripted_pick = Some(Some(scratch.0.join("x.txt")));
    app.pick_run_script(&ctx);
    assert_eq!(app.error.as_deref(), Some("no file is open"));
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

#[test]
fn five_steps_give_the_same_file_both_ways() {
    use wavcraft_core::doc::edit::Crossfades;

    let scratch = Scratch::new("both-ways");
    const LINES: [&str; 5] = ["SELECT 1000 2000", "ACTION edit.delete", "SELECT 0 500", "EFFECT invert", "EFFECT gain gain_db=-6.0206"];
    let n = 20_000usize;
    let l: Vec<i32> = (0..n).map(|i| ((i * 7919) % 20_001) as i32 - 10_000).collect();
    let r: Vec<i32> = (0..n).map(|i| ((i * 104_729) % 20_011) as i32 - 10_005).collect();
    let source = vec![l, r];
    let win_file = scratch.0.join("window.wav");
    let head_file = scratch.0.join("headless.wav");
    write_wav(&win_file, &source);
    write_wav(&head_file, &source);

    // The window, with the same crossfades as Crossfades::default() (2 ms and 5 ms).
    let mut app = a_window(&scratch);
    for pref in ["PREF set crossfade_cut_join_ms 2", "PREF set crossfade_edge_ms 5"] {
        let reply = ask(&mut app, pref);
        assert!(reply.starts_with("OK") || reply.contains("lasts until WavCraft closes"), "{pref}: {reply}");
    }
    let (cut_join, edge) = app.settings.crossfades();
    assert_eq!((cut_join.as_millis(), edge.as_millis()), (2, 5), "the window's crossfades are set");
    assert_eq!(ask(&mut app, &format!("OPEN {}", win_file.display())), "OK");
    settle(&mut app);
    let file = script(&scratch, "five.txt", &LINES);
    assert_eq!(ask(&mut app, &format!("RUN_SCRIPT {}", file.display())), "OK replaced=3");
    let win_out = scratch.0.join("window-out.wav");
    assert_eq!(ask(&mut app, &format!("SAVE {}", win_out.display())), "OK");
    settle(&mut app);
    assert_eq!(ask(&mut app, "LASTERROR"), "OK -", "the window's save ended well");

    // Headless, with Crossfades::default().
    let sessions = scratch.0.join("headless-sessions");
    std::fs::create_dir_all(&sessions).expect("a sessions folder");
    let mut head = Headless::with_crossfades(&sessions, Crossfades::default());
    assert_eq!(head.answer_line(&format!("OPEN {}", head_file.display())), "OK");
    assert_eq!(head.run_script(&LINES.join("\n")), Ok(()));
    let head_out = scratch.0.join("headless-out.wav");
    assert_eq!(head.answer_line(&format!("SAVE {}", head_out.display())), "OK");

    // Compare the two saved files.
    let from_window = read_file(&sessions, &win_out);
    let from_headless = read_file(&sessions, &head_out);
    assert_eq!(from_window.len(), 2, "both channels");
    for c in 0..2 {
        assert_eq!(from_window[c].len(), 19_000, "channel {c}: the window's file is 19000 frames");
        assert_eq!(from_headless[c].len(), 19_000, "channel {c}: the headless file is 19000 frames");
        assert_ne!(from_window[c][..500], source[c][..500], "channel {c}: frames 0 to 499 were changed");
    }
    let first_diff = (0..2).find_map(|c| from_window[c].iter().zip(&from_headless[c]).position(|(a, b)| a != b).map(|f| (c, f)));
    assert!(from_window == from_headless, "the window and headless saved different samples; first difference (channel, frame): {first_diff:?}");

    // A script one line shorter must give a different file, so removing a line on one side fails.
    let sessions4 = scratch.0.join("headless-sessions-4");
    std::fs::create_dir_all(&sessions4).expect("a sessions folder");
    let head4_file = scratch.0.join("headless4.wav");
    write_wav(&head4_file, &source);
    let mut head4 = Headless::with_crossfades(&sessions4, Crossfades::default());
    assert_eq!(head4.answer_line(&format!("OPEN {}", head4_file.display())), "OK");
    assert_eq!(head4.run_script(&LINES[..4].join("\n")), Ok(()));
    let head4_out = scratch.0.join("headless4-out.wav");
    assert_eq!(head4.answer_line(&format!("SAVE {}", head4_out.display())), "OK");
    let four = read_file(&sessions4, &head4_out);
    assert!(four != from_window, "a script one line shorter must give a different file");
}
