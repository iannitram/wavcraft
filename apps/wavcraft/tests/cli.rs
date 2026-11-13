use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
