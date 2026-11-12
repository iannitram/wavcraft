//! Tests for A1.1 (cancelling a running open): `cancel_opening` stops the file that is opening,
//! drops the files queued behind it, adds no document, leaves no session folder behind, and says so.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "a1_open_cancel_tests.rs"] mod a1_open_cancel_tests;`

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-a1-open-cancel-{}-{name}", std::process::id()));
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

/// A mono 16-bit 44.1 kHz WAV of `seconds` of silence.
fn write_wav(path: &Path, seconds: u32) {
    let data = 44_100 * seconds * 2;
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

/// The window as the program builds it, with its session folder in `scratch`. Its settings file and
/// recent-files list are cut loose at once, so nothing a test does is written to the user's own.
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// Cancelling a 60 s file the moment it starts to open: the job is dropped, the queue is cleared,
/// no document is added, the notice says the open was cancelled, and the session folder ends empty.
#[test]
fn cancel_opening_drops_the_job_and_adds_no_document() {
    let scratch = Scratch::new("cancel");
    let wav = scratch.0.join("long.wav");
    write_wav(&wav, 60);
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();

    app.open(wav, &ctx);
    assert!(app.opening.is_some(), "the 60 s file is opening");
    // A file queued behind it: it must not open once the cancel is done.
    app.open_queue.push_back(scratch.0.join("queued-but-missing.wav"));

    app.cancel_opening();

    assert!(app.opening.is_none(), "the job is dropped");
    assert!(app.open_queue.is_empty(), "the open queue is cleared");
    assert_eq!(app.notice.as_deref(), Some("Open cancelled"));
    assert!(app.file_rows().is_empty(), "no document is added");

    let deadline = Instant::now() + Duration::from_secs(5);
    let left = |app: &WaveApp| std::fs::read_dir(&app.sessions.path).map(|d| d.count()).unwrap_or(0);
    while left(&app) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(left(&app), 0, "the session folder is empty within 5 s");
    assert!(app.file_rows().is_empty(), "still no document after the worker has stopped");
}

/// OPEN_CANCEL over the control channel (A1.6): a 60 s file is opening, OPEN_CANCEL answers OK and
/// the open is gone (no document, STATE says busy=0); a second OPEN_CANCEL answers ERR.
#[test]
fn open_cancel_over_the_control_channel() {
    let scratch = Scratch::new("control");
    let wav = scratch.0.join("long.wav");
    write_wav(&wav, 60);
    let mut app = a_window(&scratch);
    let ctx = app.ctx.clone();

    app.open(wav, &ctx);
    assert!(app.opening.is_some(), "the 60 s file is opening");

    let answer = app.control_request("OPEN_CANCEL", &ctx);
    assert_eq!(answer, "OK");
    assert!(app.opening.is_none(), "opening is None");
    assert!(app.file_rows().is_empty(), "file_rows() is empty");
    let state = app.control_request("STATE", &ctx);
    assert!(state.split_whitespace().any(|w| w == "busy=0"), "STATE has busy=0, got {state}");

    let again = app.control_request("OPEN_CANCEL", &ctx);
    assert!(again.starts_with("ERR"), "a second OPEN_CANCEL answers ERR, got {again}");
}
