use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("wavcraft-1299-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch directory");
    path
}

fn write_wav(path: &Path) -> Vec<i16> {
    let samples: Vec<i16> = (0..4096).map(|i| ((i * 7919) % 20001) as i16 - 10000).collect();
    let size = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&96_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in &samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("source WAV");
    samples
}

/// A 16-bit PCM WAV of silence with the given rate, channel count and frame count.
fn write_silent_wav(path: &Path, rate: u32, channels: u16, frames: u32) {
    let block = channels * 2;
    let size = frames * u32::from(block);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    bytes.extend_from_slice(&block.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.resize(bytes.len() + size as usize, 0);
    std::fs::write(path, bytes).expect("source WAV");
}

fn run(args: &[&str], sessions: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wavcraft"))
        .args(args)
        .arg("--session-dir")
        .arg(sessions)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("run wavcraft")
}

#[test]
fn wav_to_flac_and_back_is_identical() {
    let dir = scratch("roundtrip");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let src = dir.join("source.wav");
    let flac = dir.join("middle.flac");
    let out = dir.join("back.wav");
    let samples = write_wav(&src);
    assert!(run(&["convert", src.to_str().unwrap(), flac.to_str().unwrap()], &sessions).status.success());
    assert!(run(&["convert", flac.to_str().unwrap(), out.to_str().unwrap()], &sessions).status.success());
    let doc = wavcraft_core::Document::open(&out).expect("open result");
    let mut read = vec![0.0; samples.len()];
    doc.state.read(0, 0, &mut read).expect("read samples");
    assert_eq!(read.iter().map(|v| (v * 32768.0).round() as i16).collect::<Vec<_>>(), samples);
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bad_flag_exits_2_with_a_message() {
    let dir = scratch("flag");
    let out = run(&["convert", "a.wav", "b.flac", "--nope"], &dir);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--nope"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_extension_is_refused() {
    let dir = scratch("extension");
    let src = dir.join("source.wav");
    write_wav(&src);
    let out = run(&["convert", src.to_str().unwrap(), dir.join("out.xyz").to_str().unwrap()], &dir);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no save format"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_session_is_left_behind() {
    let dir = scratch("session");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let src = dir.join("source.wav");
    write_wav(&src);
    assert!(run(&["convert", src.to_str().unwrap(), dir.join("ok.flac").to_str().unwrap()], &sessions).status.success());
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let failed = run(&["convert", src.to_str().unwrap(), dir.join("bad.xyz").to_str().unwrap()], &sessions);
    assert_eq!(failed.status.code(), Some(2));
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_input_exits_2_with_a_message_and_leaves_no_session() {
    let dir = scratch("missing-input");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let out = run(&["convert", dir.join("no-such.wav").to_str().unwrap(), dir.join("out.flac").to_str().unwrap()], &sessions);
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).trim().is_empty());
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn inspect_prints_the_properties_rows() {
    let dir = scratch("inspect");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let src = dir.join("one-second.wav");
    write_silent_wav(&src, 48_000, 2, 48_000);
    let out = run(&["inspect", src.to_str().unwrap()], &sessions);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    for wanted in ["Sample rate: 48000 Hz", "Channels: 2", "Bit depth: 16-bit integer", "Format: WAV", "Duration: 0:01.000 (48000 frames)"] {
        assert!(lines.contains(&wanted), "missing line {wanted:?} in {text:?}");
    }
    assert!(lines.iter().any(|l| l.starts_with("Path: ")), "no Path line in {text:?}");
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn inspect_of_a_missing_file_exits_2_with_its_name() {
    let dir = scratch("inspect-missing");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let out = run(&["inspect", dir.join("no-such.wav").to_str().unwrap()], &sessions);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-such.wav"));
    assert!(out.stdout.is_empty());
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unwritable_output_exits_2_with_a_message_and_leaves_no_session() {
    let dir = scratch("unwritable-output");
    let sessions = dir.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let src = dir.join("source.wav");
    write_wav(&src);
    // The folder above the output does not exist, so the save cannot create the file.
    let target = dir.join("no-such-folder").join("out.flac");
    let out = run(&["convert", src.to_str().unwrap(), target.to_str().unwrap()], &sessions);
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).trim().is_empty());
    assert!(std::fs::read_dir(&sessions).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// `wavcraft` with `args` and `input` on standard input, the environment cleared of any window or control port.
fn mcp_session(args: &[&str], sessions: &Path, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wavcraft"))
        .args(args)
        .env("WAVCRAFT_SESSION_DIR", sessions)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAVCRAFT_CONTROL_PORT")
        .env_remove("WAVCRAFT_CONTROL_TOKEN_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run wavcraft");
    let mut stdin = child.stdin.take().expect("child stdin");
    stdin.write_all(input.as_bytes()).expect("write stdin");
    drop(stdin);
    child.wait_with_output().expect("wait for wavcraft")
}

#[test]
fn mcp_answers_tools_list_on_stdio() {
    let dir = scratch("mcp-tools");
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n"
    );
    let out = mcp_session(&["mcp"], &dir, input);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "stdout was: {text}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("first reply is JSON");
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("second reply is JSON");
    assert_eq!(first["id"], 1);
    assert_eq!(first["result"]["serverInfo"]["name"], "wavcraft");
    assert_eq!(second["id"], 2);
    assert_eq!(second["result"]["tools"].as_array().map(Vec::len), Some(7));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_runs_a_call_on_stdio() {
    let dir = scratch("mcp-call");
    let wav = dir.join("source.wav");
    write_wav(&wav);
    let open = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "open", "arguments": {"path": wav.to_str().expect("utf-8 path")}}
    });
    let select = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "select", "arguments": {"start": 10, "end": 20}}
    });
    let input = format!("{open}\n{select}\n");
    let out = mcp_session(&["mcp"], &dir, &input);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "stdout was: {text}");
    let reply: serde_json::Value = serde_json::from_str(lines[1]).expect("second reply is JSON");
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(reply["result"]["content"][0]["text"], "selection=10-20");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_attach_without_a_port_exits_2() {
    let dir = scratch("mcp-attach");
    let out = mcp_session(&["mcp", "--attach"], &dir, "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("WAVCRAFT_CONTROL_PORT"));
    assert!(out.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_refuses_a_bad_flag() {
    let dir = scratch("mcp-flag");
    let out = mcp_session(&["mcp", "--nope"], &dir, "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--nope"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: wavcraft mcp [--attach]"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
