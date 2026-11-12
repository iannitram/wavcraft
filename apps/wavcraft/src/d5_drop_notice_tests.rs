//! Tests for D5.2: the History panel's drop notice speaks only when the dropped count has risen.
//!
//! D5.3: the same notice reaches the status bar. The last test runs `check_drop_notice` (the
//! per-frame check) on the headless window: a drop by the byte cap sets the notice once, and a
//! switch to another document starts its count again with no notice.
//!
//! The window runs with the silent output: a subprocess receives `WAVCRAFT_NO_AUDIO=1` before its
//! threads start, so the test never plays through a real device.
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;
use wavcraft_core::doc::edit;
use wavcraft_core::{AudioBuffer, ChannelMask, Crossfades, Document};

#[test]
fn drop_notice_speaks_only_when_the_count_rises() {
    assert_eq!(drop_notice(0, 3, 4_000_000_000).as_deref(), Some("Oldest 3 steps removed to stay under 4.0 GB"));
    assert_eq!(drop_notice(3, 3, 4_000_000_000), None);
    assert_eq!(drop_notice(0, 0, 4_000_000_000), None);
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-d5.3-{}-{name}", std::process::id()));
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

/// A new mono document of `frames` frames (f32 in memory: 4 bytes a frame), opened in the window.
fn open_mono(app: &mut WaveApp, ctx: &egui::Context, frames: usize) {
    let audio = AudioBuffer { sample_rate: 44_100, channels: vec![vec![0.5; frames]] };
    let state = Document::from_buffer(audio).expect("a mono document").state;
    app.add_document(state, ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rebuilding.is_some() && Instant::now() < deadline {
        app.finish_rebuild();
        if app.rebuilding.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert!(app.rebuilding.is_none(), "the document finished opening");
    assert_eq!(app.doc.as_ref().map(|d| d.frames()), Some(frames), "the document is {frames} frames");
}

#[test]
fn a_drop_by_the_cap_reaches_the_status_bar() {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::d5_drop_notice_tests::a_drop_by_the_cap_reaches_the_status_bar", "--nocapture"])
            .env("WAVCRAFT_NO_AUDIO", "1")
            .status()
            .expect("run the headless drop notice test with silent output");
        assert!(status.success(), "the headless drop notice test passed");
        return;
    }
    let scratch = Scratch::new("drop-notice");
    let mut app = a_window(&scratch);
    assert!(app.player.info().test_sink, "the silent output is in use");
    let ctx = app.ctx.clone();

    // 262,144 frames of f32 is 1 MiB; the cap holds 1 MiB beyond the current state.
    let frames: usize = 262_144;
    open_mono(&mut app, &ctx, frames);
    app.editor.as_mut().expect("the document is open").history.set_cap_disk_bytes(1 << 20);
    // The frame before the edits: the document's count is seen, and there is no drop yet.
    app.check_drop_notice();
    assert_eq!(app.notice, None, "no drop yet, so no notice");

    for _ in 0..3 {
        let editor = app.editor.as_mut().expect("the document is open");
        let ed = edit::replace_range(
            editor.history.current(),
            0..frames as u64,
            ChannelMask::ALL,
            "Gain",
            Crossfades::NONE,
            &AtomicBool::new(false),
            |_, i, o| {
                for (a, b) in i[0].iter().zip(o[0].iter_mut()) {
                    *b = *a + 1.0;
                }
                Ok(())
            },
        )
        .expect("an edit over the whole file");
        editor.history.push_edit(ed);
    }
    let dropped = app.editor.as_ref().map_or(0, |e| e.history.dropped_total());
    assert!(dropped >= 1, "the cap dropped {dropped} steps, so at least 1");

    // The per-frame check, once: the drop reaches the status bar.
    app.check_drop_notice();
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.starts_with("Oldest "), "the notice starts with 'Oldest ': {notice:?}");
    assert!(notice.contains("removed to stay under"), "the notice says what was removed: {notice:?}");

    // The check again with no new drop: the notice is not set again.
    app.notice = None;
    app.check_drop_notice();
    assert_eq!(app.notice, None, "no new drop, so the notice is not set again");

    // Switch to another document: its count starts again and no notice appears.
    let first = app.doc_id;
    app.notice = None;
    open_mono(&mut app, &ctx, 1_000);
    assert_ne!(app.doc_id, first, "the other document is the open one");
    app.check_drop_notice();
    assert_eq!(app.notice, None, "a new document shows no drop notice");
    assert_eq!(app.drop_seen, Some((app.doc_id, 0)), "the seen count is reset for the new document");
}
