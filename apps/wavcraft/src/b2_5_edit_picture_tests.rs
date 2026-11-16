//! B2.5: the edit-to-picture timer. It reads `none` before any edit, `pending` from a document
//! change until the first spectral picture of the new state, then the milliseconds that took; the
//! SPECTRAL verb reports it as `edit_to_picture_ms=<none|pending|n>`.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "b2_5_edit_picture_tests.rs"] mod b2_5_edit_picture_tests;`
use std::process::Command;

use super::spectral::EditToPicture;
use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-b2-5-edit-picture-{}-{name}", std::process::id()));
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

fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder");
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// Run the test again in a child process with the silent sink.
fn in_silent_subprocess(name: &str) -> bool {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return false;
    }
    let status = Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", &format!("app::b2_5_edit_picture_tests::{name}"), "--nocapture"])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .status()
        .expect("run the silent-sink test subprocess");
    assert!(status.success(), "the silent-sink test subprocess must pass");
    true
}

fn finish(app: &mut WaveApp) {
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn a_new_timer_reads_none() {
    assert_eq!(EditToPicture::default().answer(), "edit_to_picture_ms=none");
}

#[test]
fn a_change_reads_pending_until_the_picture_of_the_new_document() {
    let mut t = EditToPicture::default();
    t.begin();
    assert_eq!(t.answer(), "edit_to_picture_ms=pending");
    t.picture(5);
    assert_eq!(t.answer(), "edit_to_picture_ms=pending", "no picture counts before the document is ready");
    t.document_ready(5);
    t.picture(4);
    assert_eq!(t.answer(), "edit_to_picture_ms=pending", "a picture of an older generation does not count");
    std::thread::sleep(std::time::Duration::from_millis(5));
    t.picture(5);
    let answer = t.answer();
    let n = answer.strip_prefix("edit_to_picture_ms=").expect("the answer starts with the key");
    assert_ne!(n, "pending");
    assert!(n.parse::<u64>().expect("a whole number") >= 4, "{answer}");
    t.picture(5);
    assert_eq!(t.answer(), answer, "a second picture leaves the number alone");
}

#[test]
fn a_second_change_starts_again() {
    let mut t = EditToPicture::default();
    t.begin();
    t.document_ready(1);
    t.picture(1);
    assert!(t.answer().parse_number().is_some());
    t.begin();
    assert_eq!(t.answer(), "edit_to_picture_ms=pending");
}

trait ParseNumber {
    fn parse_number(&self) -> Option<u64>;
}

impl ParseNumber for String {
    fn parse_number(&self) -> Option<u64> {
        self.strip_prefix("edit_to_picture_ms=")?.parse().ok()
    }
}

#[test]
fn delete_then_picture_reports_a_number() {
    if in_silent_subprocess("delete_then_picture_reports_a_number") {
        return;
    }
    let scratch = Scratch::new("delete");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();
    let state = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 10_000]] }).expect("a 10_000 frame document").state;
    app.add_document(state, &ctx);
    finish(&mut app);
    assert!(ask(&mut app, "SPECTRAL").ends_with("edit_to_picture_ms=none"));

    assert!(ask(&mut app, "SELECT 1000 2000").starts_with("OK"));
    assert!(ask(&mut app, "ACTION edit.delete").starts_with("OK"));
    assert!(ask(&mut app, "SPECTRAL").ends_with("edit_to_picture_ms=pending"));
    finish(&mut app);
    assert!(ask(&mut app, "SPECTRAL").ends_with("edit_to_picture_ms=pending"));

    app.edit_picture.picture(app.generation);
    let answer = ask(&mut app, "SPECTRAL");
    let n = answer.rsplit("edit_to_picture_ms=").next().expect("the key");
    assert!(!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()), "{answer}");

    assert!(ask(&mut app, "ACTION edit.undo").starts_with("OK"));
    assert!(ask(&mut app, "SPECTRAL").ends_with("edit_to_picture_ms=pending"));
}

#[test]
fn no_file_reads_none() {
    let scratch = Scratch::new("nofile");
    let mut app = a_window(&scratch);
    assert!(ask(&mut app, "SPECTRAL").ends_with("edit_to_picture_ms=none"));
    assert!(ask(&mut app, "SPECTRAL fft 4096").ends_with("edit_to_picture_ms=none"));
}
