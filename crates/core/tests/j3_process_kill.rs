//! J3.3: exercise take recovery after the recording process itself is killed.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use wavcraft_core::Document;
use wavcraft_core::io::OpenOptions;
use wavcraft_core::record::{FakeInput, FakeSpec, RecFormat, Recorder, Ring, create_take, marker_path};
use wavcraft_core::take_recovery::{repair_take, unfinished_takes};

const CHILD_DIR: &str = "WAVCRAFT_J3_PROCESS_KILL_DIR";
const CHILD_MODE: &str = "WAVCRAFT_J3_PROCESS_KILL_CHILD";
const TEST_NAME: &str = "a_killed_process_leaves_a_take_that_repairs";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-j3-process-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The child test never returns: the parent terminates this process after the WAV header reaches
/// two seconds. Keeping the recorder and fake input alive is the point of this helper.
fn record_in_child() {
    let dir = PathBuf::from(std::env::var_os(CHILD_DIR).expect("child directory is set"));
    let fmt = RecFormat { rate: 48_000, channels: 1, bits: 24, inputs: 0 };
    let ring = Ring::new(96_000, 1);
    let writer = create_take(&dir, fmt, 1).unwrap();
    let _recorder = Recorder::start(writer, ring.clone()).unwrap();
    let _fake = FakeInput::spawn(FakeSpec::parse(Some("sine:440")).unwrap().unwrap(), fmt, ring).unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn header_frames(path: &Path) -> u64 {
    let mut header = [0u8; 44];
    let Ok(mut file) = File::open(path) else { return 0 };
    if file.read_exact(&mut header).is_err() || &header[..4] != b"RIFF" || &header[36..40] != b"data" {
        return 0;
    }
    u64::from(u32::from_le_bytes([header[40], header[41], header[42], header[43]])) / 3
}

fn kill_when_header_reaches(child: &mut Child, path: &Path, frames: u64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if header_frames(path) >= frames {
            child.kill().unwrap();
            let _ = child.wait().unwrap();
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("recording child exited before the header reached {frames} frames: {status}");
        }
        assert!(Instant::now() < deadline, "timed out waiting for the recording header to reach {frames} frames");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_killed_process_leaves_a_take_that_repairs() {
    if std::env::var_os(CHILD_MODE).is_some() {
        record_in_child();
        return;
    }

    let dir = scratch();
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(exe)
        .arg("--exact")
        .arg(TEST_NAME)
        .arg("--nocapture")
        .env(CHILD_MODE, "1")
        .env(CHILD_DIR, &dir)
        .env("WAVCRAFT_FAKE_INPUT", "sine:440")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // The header updates every second; wait for the second update, which claims 96,000 frames.
    // If child setup fails, the loop reports its exit rather than hanging indefinitely.
    let mut path = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while path.is_none() {
        path =
            std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|entry| entry.path()).find(|p| p.extension().is_some_and(|ext| ext == "wav"));
        if child.try_wait().unwrap().is_some() {
            panic!("recording child exited before creating its take");
        }
        assert!(Instant::now() < deadline, "timed out waiting for the child to create its take");
        std::thread::sleep(Duration::from_millis(20));
    }
    let path = path.unwrap();
    kill_when_header_reaches(&mut child, &path, 96_000);

    let found = unfinished_takes(&dir);
    assert_eq!(found.len(), 1, "one killed take should be offered for recovery");
    let take = &found[0];
    let len = std::fs::metadata(&take.path).unwrap().len();
    let repaired = repair_take(take).unwrap();
    assert!(repaired >= 96_000);
    assert_eq!(repaired, (len - 44) / 3);
    assert!(!marker_path(&take.path).exists());

    let sessions = dir.join("sessions");
    let opts = OpenOptions { sessions_root: Some(sessions), ..OpenOptions::default() };
    let doc = Document::open_in(&take.path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(doc.sample_rate(), 48_000);
    assert_eq!(doc.channels(), 1);
    drop(doc);
    let _ = std::fs::remove_dir_all(&dir);
}
