//! Locked tests for #366 (opening another file asks before discarding unsaved edits).
//!
//! The open path needs a running window, so these check the wiring in the source: the
//! interactive entries (Ctrl+O / File › Open and a dropped file) go through `request_open`, the
//! prompt is drawn every frame, and the control channel exposes `OPEN_PICK` / `OPEN_ANSWER` so a
//! scripted run can drive the same prompt.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "issue366_tests.rs"] mod issue366_tests;`

/// The app source, with comments and test code cut off the end so only the live code is searched.
fn app_src() -> &'static str {
    let src = include_str!("app.rs");
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

/// The body of the function whose signature starts with `signature`, up to the next `\n    fn `.
fn body_of<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = src.find(signature).unwrap_or_else(|| panic!("{signature} is missing"));
    let rest = &src[start + signature.len()..];
    let end = rest.find("\n    fn ").or_else(|| rest.find("\n    /// ")).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn ctrl_o_and_the_open_dialog_go_through_the_prompt() {
    let src = app_src();
    let pick = body_of(src, "fn pick_file(&mut self, _ctx: &egui::Context) {");
    assert!(pick.contains("self.request_open("), "File > Open must ask first: {pick}");
    assert!(!pick.contains("self.open_many("), "File > Open must not queue files without the prompt");
}

#[test]
fn a_dropped_file_goes_through_the_prompt() {
    let src = app_src();
    assert!(src.contains("self.request_open(vec![path]);"), "a dropped file must go through request_open");
    assert!(!src.contains("self.open(path, &ctx);"), "a dropped file must not open without the prompt");
}

#[test]
fn the_prompt_only_comes_up_for_unsaved_edits_of_the_current_document() {
    let src = app_src();
    let request = body_of(src, "fn request_open(&mut self, paths: Vec<PathBuf>) {");
    assert!(request.contains("if self.unsaved() {"), "the prompt is for the current document's unsaved edits");
    assert!(request.contains("self.open_prompt = true;"));
}

#[test]
fn the_prompt_offers_save_dont_save_and_cancel_as_close_does() {
    let src = app_src();
    let dialog = body_of(src, "fn open_prompt_dialog(&mut self, ctx: &egui::Context) {");
    for label in ["button.save", "button.dont_save", "button.cancel"] {
        assert!(dialog.contains(label), "the open prompt needs {label}");
    }
    assert!(dialog.contains("tr(\"dialog.unsaved\")"), "same title as the close prompt");
}

#[test]
fn the_prompt_is_drawn_every_frame() {
    let src = app_src();
    assert!(src.contains("self.open_prompt_dialog(&ctx);"));
}

#[test]
fn the_queue_waits_for_the_prompt_and_for_a_save() {
    let src = app_src();
    let pump = body_of(src, "fn pump_open_queue(&mut self, ctx: &egui::Context) {");
    assert!(pump.contains("self.open_prompt"), "no file opens while the prompt is up");
    assert!(pump.contains("self.saving.is_some()"), "the file opens after a save, not during it");
}

#[test]
fn a_failed_or_cancelled_save_from_the_prompt_drops_the_files() {
    let src = app_src();
    let finish = body_of(src, "fn finish_saving(");
    assert!(finish.matches("std::mem::take(&mut self.open_after_save)").count() >= 2, "both failure arms must drop the queued files");
}

#[test]
fn the_control_channel_has_open_pick_and_open_answer() {
    let control = include_str!("control.rs");
    assert!(control.contains("`OPEN_PICK <path>"), "OPEN_PICK is documented in the protocol header");
    assert!(control.contains("`OPEN_ANSWER save|discard|cancel`"), "OPEN_ANSWER is documented in the protocol header");
    let src = app_src();
    assert!(src.contains("\"OPEN_PICK\" if"));
    assert!(src.contains("\"OPEN_ANSWER\" =>"));
}

/// The same prompt driven through the control channel's own lines (`OPEN_PICK`, `OPEN_ANSWER`,
/// `STATE`) on a real window (`WaveApp::new` on eframe's headless creation context), asserting what
/// each answer does to the document that has unsaved edits.
mod control_port {
    use super::super::*;

    /// A folder for one test, removed when it ends. Declare it before the window.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("wavcraft-issue366-{}-{name}", std::process::id()));
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

    /// A mono 16-bit 44.1 kHz WAV of `frames` silent frames.
    fn write_wav(path: &Path, frames: u32) {
        let data = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&44_100u32.to_le_bytes());
        b.extend_from_slice(&88_200u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        b.resize(b.len() + data as usize, 0);
        std::fs::write(path, b).expect("a WAV file");
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

    /// A `key=value` field of a STATE answer.
    fn field(state: &str, key: &str) -> String {
        state.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string)).unwrap_or_default()
    }

    /// Run `n` frames of the open queue's pump.
    fn frames(app: &mut WaveApp, n: usize) {
        let ctx = app.ctx.clone();
        for _ in 0..n {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.finish_opening();
                app.pump_open_queue(ui.ctx());
            });
            output.textures_delta.clear();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
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
            assert!(Instant::now() < deadline, "the open did not settle");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// File A (100 frames) open and saved at `a.wav`, then edited to 300 frames and unsaved; file B
    /// (1000 frames) beside it.
    fn a_edited_over_b_waiting(scratch: &Scratch) -> (WaveApp, PathBuf, PathBuf) {
        let (a, b) = (scratch.0.join("a.wav"), scratch.0.join("b.wav"));
        write_wav(&a, 100);
        write_wav(&b, 1000);
        let mut app = a_window(scratch);
        let ctx = app.ctx.clone();
        app.open(a.clone(), &ctx);
        settle(&mut app);
        app.editor.as_mut().expect("A is open").insert_silence(0, 200).expect("an edit");
        let state = ask(&mut app, "STATE");
        assert_eq!((field(&state, "frames").as_str(), field(&state, "unsaved").as_str()), ("300", "1"), "{state}");
        (app, a, b)
    }

    #[test]
    fn cancel_keeps_the_document_and_its_edits_and_drops_the_file() {
        let scratch = Scratch::new("cancel");
        let (mut app, a, b) = a_edited_over_b_waiting(&scratch);
        assert_eq!(ask(&mut app, &format!("OPEN_PICK {}", b.display())), "OK queued=1 prompt=1");
        frames(&mut app, 20);
        assert!(app.opening.is_none(), "nothing starts opening behind the prompt");
        let state = ask(&mut app, "STATE");
        assert_eq!((field(&state, "frames").as_str(), field(&state, "unsaved").as_str()), ("300", "1"), "nothing opens behind the prompt: {state}");

        assert_eq!(ask(&mut app, "OPEN_ANSWER cancel"), "OK prompt=0");
        settle(&mut app);
        let state = ask(&mut app, "STATE");
        assert_eq!((field(&state, "frames").as_str(), field(&state, "unsaved").as_str()), ("300", "1"), "Cancel keeps A and its edits: {state}");
        assert_eq!(app.doc_path, a, "still A's own file");
        assert!(app.editor.as_ref().is_some_and(|e| e.history.cursor() > 0), "A's undo history is kept");
        assert!(ask(&mut app, "OPEN_ANSWER cancel").starts_with("ERR"), "no prompt is left");
    }

    #[test]
    fn dont_save_drops_the_edits_and_opens_the_file() {
        let scratch = Scratch::new("discard");
        let (mut app, a, b) = a_edited_over_b_waiting(&scratch);
        assert_eq!(ask(&mut app, &format!("OPEN_PICK {}", b.display())), "OK queued=1 prompt=1");
        assert_eq!(ask(&mut app, "OPEN_ANSWER discard"), "OK prompt=0");
        settle(&mut app);
        let state = ask(&mut app, "STATE");
        assert_eq!((field(&state, "frames").as_str(), field(&state, "unsaved").as_str()), ("1000", "0"), "B is open, clean: {state}");
        assert_eq!(app.doc_path, b);
        let on_disk = std::fs::metadata(&a).expect("A's file").len();
        assert_eq!(on_disk, 44 + 200, "A's file was not written");
    }

    #[test]
    fn save_writes_the_edits_then_opens_the_file() {
        let scratch = Scratch::new("save");
        let (mut app, a, b) = a_edited_over_b_waiting(&scratch);
        assert_eq!(ask(&mut app, &format!("OPEN_PICK {}", b.display())), "OK queued=1 prompt=1");
        assert_eq!(ask(&mut app, "OPEN_ANSWER save"), "OK prompt=0");
        settle(&mut app);
        let state = ask(&mut app, "STATE");
        assert_eq!((field(&state, "frames").as_str(), field(&state, "unsaved").as_str()), ("1000", "0"), "B is open, clean: {state}");
        assert_eq!(app.doc_path, b);
        let on_disk = std::fs::metadata(&a).expect("A's file").len();
        assert_eq!(on_disk, 44 + 600, "A's file holds the 300 edited frames");
    }

    #[test]
    fn a_clean_document_opens_without_a_prompt() {
        let scratch = Scratch::new("clean");
        let (a, b) = (scratch.0.join("a.wav"), scratch.0.join("b.wav"));
        write_wav(&a, 100);
        write_wav(&b, 1000);
        let mut app = a_window(&scratch);
        let ctx = app.ctx.clone();
        app.open(a, &ctx);
        settle(&mut app);
        assert_eq!(ask(&mut app, &format!("OPEN_PICK {}", b.display())), "OK queued=1 prompt=0");
        settle(&mut app);
        assert_eq!(app.doc_path, b);
    }
}
