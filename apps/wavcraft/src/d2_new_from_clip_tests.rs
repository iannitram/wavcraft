//! Tests for D2.1: Copy to New and Paste to New each add one untitled, unsaved row to the Files
//! panel's rows and make it the current document.
//!
//! Both build their document on a worker thread (`start_making`), then `finish_making` adds it.
//! The test runs on a real window (`WaveApp::new` on eframe's headless creation context) and
//! calls `finish_making` until the worker's result is taken.
use std::time::{Duration, Instant};

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-d2-new-from-clip-{}-{name}", std::process::id()));
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

/// A mono ramp of `n` frames, as a document state.
fn ramp(n: usize) -> DocState {
    Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| i as f32).collect()] }).expect("a ramp document").state
}

/// Run frames (`finish_making`) until the new document is added, for at most 10 seconds.
fn wait_for_making(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.making.is_some() {
        assert!(Instant::now() < deadline, "the new document was not made in 10 seconds");
        app.finish_making(&ctx);
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The row is an untitled, unsaved document of `frames` frames, and it is the current one.
fn assert_untitled_current(app: &WaveApp, row: &files::Row, frames: u64) {
    assert_eq!(row.name, "Untitled");
    assert_eq!(row.path, "");
    assert!(row.unsaved, "the new document is unsaved");
    assert_eq!(row.frames, frames);
    assert_eq!(row.doc, app.docs.current(), "the new document is the current one");
    assert_eq!(app.editor.as_ref().map(|e| e.history.current().len_frames()), Some(frames));
}

#[test]
fn copy_and_paste_to_new_add_untitled_rows() {
    let scratch = Scratch::new("rows");
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();

    // One 1000-frame document is open.
    app.add_document(ramp(1000), &ctx);
    while app.rebuilding.is_some() {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.file_rows().len(), 1);

    // Copy to New of frames 100..300 adds a 200-frame untitled document.
    app.selection = Selection { anchor: 100, head: 300 };
    app.copy_to_new(&ctx).expect("Copy to New starts");
    wait_for_making(&mut app);
    let rows = app.file_rows();
    assert_eq!(rows.len(), 2, "Copy to New adds one row");
    assert_untitled_current(&app, &rows[1], 200);

    // Copy frames 0..50 of that document, then Paste to New adds a 50-frame untitled document.
    app.selection = Selection { anchor: 0, head: 50 };
    app.run_action(ActionId::Copy, &ctx).expect("copy 0..50");
    app.paste_to_new(&ctx).expect("Paste to New starts");
    wait_for_making(&mut app);
    let rows = app.file_rows();
    assert_eq!(rows.len(), 3, "Paste to New adds one row");
    assert_untitled_current(&app, &rows[2], 50);
}
