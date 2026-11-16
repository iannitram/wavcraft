#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! N1.7: the same steps through a running app and through `Headless`, then the two saved files
//! compared sample for sample.
//!
//! The running app is reached over its control channel with `Remote::from_env` (the environment
//! names the port and the token file; `bash tools/checks/1107-headless-window-parity.sh` starts
//! the app and runs this). Both sides do the same: OPEN a 16-bit WAV, `SELECT 1000 2000`,
//! `ACTION edit.delete`, `SELECT 0 500`, `EFFECT invert`, then a bare SAVE in place. The two
//! saved files are read back by a fresh `Headless` and compared; the in-process version of this
//! check is `apps/wavcraft/src/n1_parity_tests.rs`.
//!
//! Usage: `headless_window_parity <work-folder>` (the folder must exist). Stdout ends with
//! `same=1 frames=9000` and the exit code is 0 when the two files match.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use wavcraft_automation::headless::Headless;
use wavcraft_automation::remote::Remote;
use wavcraft_automation::{Backend, Request, SaveArgs};

/// Frames in the source file.
const SOURCE_FRAMES: usize = 10_000;
/// Frames left after the delete of 1000 frames.
const EXPECTED_FRAMES: usize = 9_000;
/// The steps between OPEN and SAVE.
const STEPS: [&str; 4] = ["SELECT 1000 2000", "ACTION edit.delete", "SELECT 0 500", "EFFECT invert"];

/// Two channels of distinct, deterministic 16-bit samples (never -32768, so inverting is exact).
/// The same samples as `apps/wavcraft/src/n1_parity_tests.rs`.
fn source_samples() -> Vec<Vec<i32>> {
    let l = (0..SOURCE_FRAMES).map(|i| ((i * 7919) % 20_001) as i32 - 10_000).collect();
    let r = (0..SOURCE_FRAMES).map(|i| ((i * 104_729) % 20_011) as i32 - 10_005).collect();
    vec![l, r]
}

/// A plain PCM WAV, 16-bit, 48 kHz, interleaved.
fn write_wav(path: &Path, channels: &[Vec<i32>]) -> std::io::Result<()> {
    let ch = channels.len() as u16;
    let frames = channels.first().map_or(0, Vec::len);
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
            let v = c.get(f).copied().unwrap_or(0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, out)
}

/// Connect to the running app, retrying for up to 20 seconds: it may not be listening yet.
fn connect_remote() -> Result<Remote, String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match Remote::from_env() {
            Ok(remote) => return Ok(remote),
            Err(why) if Instant::now() >= deadline => return Err(why.to_string()),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// Open `file` on `backend` and check it loaded: the remote's OPEN can answer OK for a cancelled
/// open, and a failed open can leave STATE at busy=0 before LASTERROR is set, so the reply to
/// STATE is checked too (`frames=` and `channels=`, and `path=` last).
fn open_checked(remote: &mut Remote, file: &Path) -> Result<(), String> {
    let open = Request::parse(&format!("OPEN {}", file.display()))?;
    let opened = remote.execute(&open);
    let state = remote.execute(&Request::State)?;
    let loaded = state.split_whitespace().any(|f| f == "frames=10000")
        && state.split_whitespace().any(|f| f == "channels=2")
        && state.ends_with(&file.display().to_string());
    if opened.is_err() || !loaded {
        let why = opened.err().unwrap_or_else(|| state.clone());
        return Err(format!("the open did not load window.wav: {why} (STATE: {state})"));
    }
    Ok(())
}

/// The four steps and the bare SAVE, on `backend`. Each step must answer OK.
fn edit_and_save(backend: &mut dyn Backend) -> Result<(), String> {
    for step in STEPS {
        backend.execute(&Request::parse(step)?).map_err(|why| format!("{step}: {why}"))?;
    }
    backend.execute(&Request::Save { path: None, args: SaveArgs::default() }).map_err(|why| format!("SAVE: {why}"))?;
    Ok(())
}

/// Every channel of the saved file at `file`, as 16-bit sample values, read by a fresh `Headless`.
fn read_saved(sessions: &Path, file: &Path) -> Result<Vec<Vec<i32>>, String> {
    let mut reader = Headless::new(sessions);
    let reply = reader.answer_line(&format!("OPEN {}", file.display()));
    if reply != "OK" {
        return Err(format!("reading {}: {reply}", file.display()));
    }
    let doc = reader.document().ok_or_else(|| "no document after OPEN".to_string())?;
    let frames = doc.len_frames() as usize;
    let mut channels = Vec::new();
    for ch in 0..doc.channel_count() {
        let mut buf = vec![0f32; frames];
        let n = doc.read(ch, 0, &mut buf).map_err(|e| format!("{e:?}"))?;
        if n != buf.len() {
            return Err(format!("reading {}: channel {ch} gave {n} of {frames} frames", file.display()));
        }
        channels.push(buf.iter().map(|v| (v * 32_768.0).round() as i32).collect());
    }
    Ok(channels)
}

/// Run both sides and compare. Returns whether the files match, and the line to print.
fn run(work: &Path) -> Result<(bool, String), String> {
    if !work.is_dir() {
        return Err(format!("the work folder {} does not exist", work.display()));
    }
    let source = source_samples();
    let win_file: PathBuf = work.join("window.wav");
    let head_file: PathBuf = work.join("headless.wav");
    write_wav(&win_file, &source).map_err(|e| format!("write window.wav: {e}"))?;
    write_wav(&head_file, &source).map_err(|e| format!("write headless.wav: {e}"))?;

    // Through the running app.
    let mut remote = connect_remote()?;
    open_checked(&mut remote, &win_file)?;
    edit_and_save(&mut remote)?;
    let state = remote.execute(&Request::State)?;
    if !state.split_whitespace().any(|f| f == "unsaved=0") {
        return Err(format!("the window's save left the document unsaved: {state}"));
    }

    // Through Headless, with the same steps.
    let head_sessions = work.join("headless-sessions");
    std::fs::create_dir_all(&head_sessions).map_err(|e| format!("make {}: {e}", head_sessions.display()))?;
    let mut head = Headless::new(&head_sessions);
    let head_open = head.execute(&Request::parse(&format!("OPEN {}", head_file.display()))?);
    head_open.map_err(|why| format!("the open did not load headless.wav: {why}"))?;
    edit_and_save(&mut head)?;

    // Compare the two saved files, read back by a fresh Headless each.
    let read_sessions = work.join("read-sessions");
    std::fs::create_dir_all(&read_sessions).map_err(|e| format!("make {}: {e}", read_sessions.display()))?;
    let from_window = read_saved(&read_sessions, &win_file)?;
    let from_headless = read_saved(&read_sessions, &head_file)?;
    let window_frames = from_window.first().map_or(0, Vec::len);
    let headless_frames = from_headless.first().map_or(0, Vec::len);
    let same = from_window == from_headless && from_window.len() == 2 && window_frames == EXPECTED_FRAMES && headless_frames == EXPECTED_FRAMES;
    if same {
        Ok((true, format!("same=1 frames={EXPECTED_FRAMES}")))
    } else {
        Ok((false, format!("same=0 frames={window_frames}/{headless_frames}")))
    }
}

fn main() -> ExitCode {
    let Some(work) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("error: usage: headless_window_parity <work-folder>");
        return ExitCode::from(1);
    };
    match run(&work) {
        Ok((true, line)) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Ok((false, line)) => {
            println!("{line}");
            ExitCode::from(1)
        }
        Err(why) => {
            eprintln!("error: {why}");
            ExitCode::from(1)
        }
    }
}
