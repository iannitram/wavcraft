//! Feature f99: the audio track of MOV, MP4 and M4A files (no video), through symphonia's
//! isomp4 demuxer. The files are made with ffmpeg; a test skips itself when it is absent.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::DocState;
use wavcraft_core::io::{OpenError, OpenOptions, open};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-mov-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn have_ffmpeg() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        eprintln!("skipped: ffmpeg is not installed");
    }
    ok
}

/// Run ffmpeg with `args` (inputs, maps and codecs) writing `out`.
fn ffmpeg(args: &[&str], out: &Path) {
    let status = Command::new("ffmpeg").args(["-loglevel", "error", "-y"]).args(args).arg(out).status().unwrap();
    assert!(status.success(), "ffmpeg failed: {args:?}");
}

fn stream(path: &Path, root: &Path) -> Result<DocState, OpenError> {
    let opts = OpenOptions { sessions_root: Some(root.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false))
}

fn sine(rate: u32, secs: u32) -> String {
    format!("sine=frequency=440:sample_rate={rate}:duration={secs}")
}

fn peak(doc: &DocState) -> f32 {
    let mut buf = vec![0.0f32; 4096];
    let n = doc.read(0, 0, &mut buf).unwrap();
    buf[..n].iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn a_mov_with_pcm_audio_and_a_video_track_opens_its_audio() {
    if !have_ffmpeg() {
        return;
    }
    let dir = TempDir::new("pcm");
    let path = dir.0.join("a.mov");
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            &sine(44_100, 2),
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=10:duration=2",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
            "-c:v",
            "mpeg4",
        ],
        &path,
    );
    let doc = stream(&path, &dir.0.join("s")).unwrap();
    assert_eq!(doc.sample_rate(), 44_100);
    assert_eq!(doc.channel_count(), 2);
    assert_eq!(doc.len_frames(), 88_200);
    assert!(peak(&doc) > 0.05, "the audio is not silent");
}

#[test]
fn an_mp4_with_aac_audio_opens() {
    if !have_ffmpeg() {
        return;
    }
    let dir = TempDir::new("aac");
    let path = dir.0.join("a.mp4");
    ffmpeg(&["-f", "lavfi", "-i", &sine(48_000, 2), "-c:a", "aac"], &path);
    let doc = stream(&path, &dir.0.join("s")).unwrap();
    assert_eq!(doc.sample_rate(), 48_000);
    assert_eq!(doc.channel_count(), 1);
    // AAC pads the start and end by a frame or two (1024 frames each).
    let n = doc.len_frames();
    assert!((95_000..=99_500).contains(&n), "length {n}");
    assert!(peak(&doc) > 0.05);
}

#[test]
fn an_m4a_with_alac_audio_opens() {
    if !have_ffmpeg() {
        return;
    }
    let dir = TempDir::new("alac");
    let path = dir.0.join("a.m4a");
    ffmpeg(&["-f", "lavfi", "-i", &sine(32_000, 1), "-ac", "2", "-c:a", "alac"], &path);
    let doc = stream(&path, &dir.0.join("s")).unwrap();
    assert_eq!((doc.sample_rate(), doc.channel_count(), doc.len_frames()), (32_000, 2, 32_000));
}

#[test]
fn the_first_audio_track_is_opened_and_the_others_are_reported() {
    if !have_ffmpeg() {
        return;
    }
    let dir = TempDir::new("two");
    let path = dir.0.join("two.mov");
    ffmpeg(&["-f", "lavfi", "-i", &sine(44_100, 1), "-f", "lavfi", "-i", &sine(22_050, 1), "-map", "0", "-map", "1", "-c:a", "pcm_s16le"], &path);
    let doc = stream(&path, &dir.0.join("s")).unwrap();
    assert_eq!((doc.sample_rate(), doc.len_frames()), (44_100, 44_100));
    let warnings = &doc.source().warnings;
    assert!(warnings.iter().any(|w| w.contains("audio tracks") && w.contains("first")), "warnings: {warnings:?}");
}

#[test]
fn an_unsupported_audio_codec_is_refused_with_a_clear_message() {
    if !have_ffmpeg() {
        return;
    }
    let dir = TempDir::new("ac3");
    let path = dir.0.join("x.mp4");
    ffmpeg(&["-f", "lavfi", "-i", &sine(48_000, 1), "-c:a", "ac3"], &path);
    let Err(err) = stream(&path, &dir.0.join("s")) else { panic!("must be refused") };
    let msg = err.to_string();
    assert!(matches!(err, OpenError::Decode(_)), "{err:?}");
    assert!(msg.contains("audio codec is not supported"), "{msg}");
}
