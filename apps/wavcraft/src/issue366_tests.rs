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
