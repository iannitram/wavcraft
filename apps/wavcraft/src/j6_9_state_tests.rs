//! J6.9: the control channel's STATE reports the take's recording fields with no document open.
//!
//! The no-document arm of STATE used to leave out the `recording=` and `rec_*` fields, so a script
//! that records into an empty editor (as tools/alpha/f25.py does) could not see the take. The test
//! runs the real window headless: STATE before a take says `recording=0`; during a fake-input take
//! it says `recording=1` with `rec_frames`, `rec_dropped` and `rec_bytes`.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "j6_9_state_tests.rs"] mod j6_9_state_tests;`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;

/// The fake input and the record folder must be in the environment before the app starts. Setting
/// them here would be `set_var`, which is unsafe while other test threads may read the environment,
/// so the test runs again in a child process that has them (as c5_space_return_tests does).
const CHILD_MARK: &str = "sine:440";

fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("wavcraft-j6-9-state-{}", std::process::id()))
}

#[test]
fn a_take_is_in_state_with_no_document_open() {
    if std::env::var("WAVCRAFT_FAKE_INPUT").as_deref() != Ok(CHILD_MARK) {
        let dir = scratch();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let out = std::process::Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", "app::j6_9_state_tests::a_take_is_in_state_with_no_document_open"])
            .env("WAVCRAFT_FAKE_INPUT", CHILD_MARK)
            .env("WAVCRAFT_RECORD_DIR", &dir)
            .output()
            .expect("run the test with a fake input");
        let _ = std::fs::remove_dir_all(&dir);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && stdout.contains("1 passed"), "the child run failed:\n{stdout}{}", String::from_utf8_lossy(&out.stderr));
        return;
    }

    let sessions = scratch().join("sessions");
    let _ = std::fs::create_dir_all(&sessions);
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(sessions), false);
    app.settings_path = None;
    app.recent_path = None;
    let ctx = app.ctx.clone();

    assert!(app.editor.is_none(), "the app starts with no document open");
    let before = app.control_request("STATE", &ctx);
    assert_eq!(field(&before, "recording").as_deref(), Some("0"), "before a take: {before}");

    let start = app.control_request("RECORD start", &ctx);
    assert!(start.starts_with("OK path="), "RECORD start: {start}");

    let deadline = Instant::now() + Duration::from_secs(5);
    let during = loop {
        let state = app.control_request("STATE", &ctx);
        let recording = field(&state, "recording").as_deref() == Some("1");
        let frames = field(&state, "rec_frames").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let bytes = field(&state, "rec_bytes").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        if recording && frames > 0 && bytes > 44 {
            break state;
        }
        assert!(Instant::now() < deadline, "timed out waiting for recording frames and bytes: {state}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(field(&during, "recording").as_deref(), Some("1"), "during a take: {during}");
    let frames = field(&during, "rec_frames").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    assert!(frames > 0, "rec_frames grows during a take: {during}");
    assert_eq!(field(&during, "rec_dropped").as_deref(), Some("0"), "no frames dropped: {during}");
    let bytes = field(&during, "rec_bytes").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    assert!(bytes > 44, "rec_bytes grows past the header: {during}");

    let stop = app.control_request("RECORD stop", &ctx);
    assert!(stop.starts_with("OK frames="), "RECORD stop: {stop}");
    let after = app.control_request("STATE", &ctx);
    assert_eq!(field(&after, "recording").as_deref(), Some("0"), "after the stop: {after}");
}

/// The value of a `key=value` word in a STATE answer.
fn field(state: &str, key: &str) -> Option<String> {
    state.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string))
}
