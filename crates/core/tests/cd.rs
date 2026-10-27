//! f121: the CD layout. CUE times to the frame, ISRC checks, total time, and the exported WAV.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavcraft_core::cd::{CdLayout, DiscLimit, format_msf, isrc_normalize, isrc_valid, pause_frames};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-cd-test-{}-{name}", std::process::id()));
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

/// A document of `frames` frames at `rate` with `channels` channels, each a constant `level`.
fn doc(dir: &Path, rate: u32, channels: usize, frames: usize, level: f32) -> Arc<DocState> {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 16 << 20, ..StoreOptions::default() }).unwrap();
    let data = vec![level; frames];
    let chans = (0..channels).map(|_| Channel::from_pieces(data.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    Arc::new(DocState::new(s.clone(), chans, rate, SourceFormat::synthetic(rate, channels as u16)).unwrap())
}

#[test]
fn isrc_validation() {
    assert!(isrc_valid("USABC2400001"));
    assert!(isrc_valid("us-abc-24-00001"), "hyphens and lowercase are accepted");
    assert_eq!(isrc_normalize("gb-a1b-99-12345").as_deref(), Some("GBA1B9912345"));
    for bad in ["", "USABC240000", "USABC24000012", "U1ABC2400001", "USABC24A0001", "USAB!2400001", "USABC2400O01"] {
        assert!(!isrc_valid(bad), "{bad:?}");
    }
}

#[test]
fn msf_is_minutes_seconds_and_frames_at_75() {
    assert_eq!(format_msf(0), "00:00:00");
    assert_eq!(format_msf(74), "00:00:74");
    assert_eq!(format_msf(75), "00:01:00");
    assert_eq!(format_msf(75 * 60 + 76), "01:01:01");
    assert_eq!(pause_frames(2.0), 150);
    assert_eq!(pause_frames(0.5), 38, "37.5 rounds to 38");
}

#[test]
fn cue_times_are_exact_to_the_frame_and_pauses_default_as_asked() {
    let d = TempDir::new("cue");
    let mut l = CdLayout::default();
    // 3.0 s = 132300 samples = exactly 225 CD frames; 1.0 s plus 100 samples pads to 76 frames.
    l.add(doc(&d.0.join("a"), 44_100, 2, 132_300, 0.1), "one");
    l.add(doc(&d.0.join("b"), 44_100, 2, 44_100 + 100, 0.1), "two");
    l.add(doc(&d.0.join("c"), 44_100, 2, 44_100, 0.1), "three");
    assert_eq!((l.tracks[0].pause_secs, l.tracks[1].pause_secs), (2.0, 0.0), "2 s for the first, 0 for the others");
    l.set_pause(2, 1.0).unwrap();
    let t = l.timeline();
    assert_eq!((t[0].index00, t[0].index01, t[0].frames), (Some(0), 150, 225));
    assert_eq!((t[1].index00, t[1].index01, t[1].frames), (None, 375, 76));
    assert_eq!((t[2].index00, t[2].index01, t[2].frames), (Some(451), 526, 75));
    assert_eq!(l.total_frames(), 601);
    l.set_meta(0, "title", "First \"One\"").unwrap();
    l.set_meta(0, "performer", "Band").unwrap();
    l.set_meta(0, "isrc", "us-abc-24-00001").unwrap();
    let cue = l.cue_sheet("disc.wav").unwrap();
    let want = "FILE \"disc.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"First 'One'\"\n    PERFORMER \"Band\"\n    ISRC USABC2400001\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n  \
        TRACK 02 AUDIO\n    TITLE \"two\"\n    INDEX 01 00:05:00\n  \
        TRACK 03 AUDIO\n    TITLE \"three\"\n    INDEX 00 00:06:01\n    INDEX 01 00:07:01\n";
    assert_eq!(cue, want);
}

#[test]
fn a_bad_isrc_is_refused_when_set() {
    let d = TempDir::new("isrc");
    let mut l = CdLayout::default();
    l.add(doc(&d.0, 44_100, 2, 44_100, 0.0), "x");
    assert!(l.set_meta(0, "isrc", "nope").is_err());
    assert!(l.tracks[0].isrc.is_empty());
    assert!(l.set_meta(0, "isrc", "USABC2400001").is_ok());
    assert!(l.set_meta(0, "isrc", "").is_ok() && l.tracks[0].isrc.is_empty());
    assert!(l.set_meta(0, "colour", "red").is_err());
    assert!(l.set_pause(0, -1.0).is_err() && l.set_pause(0, f64::NAN).is_err() && l.set_pause(0, 99.0).is_err());
}

#[test]
fn total_time_against_the_limit() {
    let d = TempDir::new("limit");
    let mut l = CdLayout::default();
    l.add(doc(&d.0, 44_100, 2, 44_100 * 10, 0.0), "x");
    assert_eq!(l.total_frames(), 150 + 750);
    assert!((l.total_secs() - 12.0).abs() < 1e-9);
    assert!(!l.over_limit());
    assert_eq!(DiscLimit::Min74.frames(), 74 * 60 * 75);
    assert_eq!(DiscLimit::Min80.frames(), 80 * 60 * 75);
    // 75 minutes: a 1 kHz source of 4.5 M frames (only its length matters here; it converts to
    // 75 minutes at 44.1 kHz), and the pause is 0 so the total is just the audio.
    let mut long = CdLayout::default();
    long.add(doc(&d.0.join("long"), 1_000, 1, 4_500_000, 0.0), "long");
    long.tracks[0].pause_secs = 0.0;
    assert_eq!(long.total_frames(), 75 * 60 * 75);
    long.limit = DiscLimit::Min74;
    assert!(long.over_limit() && long.warnings().iter().any(|w| w.contains("over the 74-minute limit")));
    long.limit = DiscLimit::Min80;
    assert!(!long.over_limit());
}

#[test]
fn export_writes_one_wav_of_the_layouts_length_and_a_cue_and_converts() {
    let d = TempDir::new("export");
    let mut l = CdLayout::default();
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.5), "stereo");
    l.add(doc(&d.0.join("b"), 22_050, 1, 22_050 + 100, 0.25), "mono22");
    l.set_pause(1, 0.5).unwrap();
    let w = l.warnings();
    assert!(w.len() == 1 && w[0].contains("Track 2") && w[0].contains("22050 Hz") && w[0].contains("mono to stereo"), "{w:?}");
    let out = d.0.join("out");
    let r = l.export(&out, "disc", &mut |_| {}, &AtomicBool::new(false)).unwrap();
    // 150 + 75 + 38 + 76 frames (22150 source frames are 44300 samples, 75.3 CD frames).
    assert_eq!(r.total_frames, 339);
    let bytes = std::fs::read(&r.wav).unwrap();
    assert_eq!(bytes.len() as u64, 44 + 339 * 588 * 4);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 44_100);
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
    let sample = |frame: usize, ch: usize| i16::from_le_bytes(bytes[44 + (frame * 2 + ch) * 2..][..2].try_into().unwrap());
    assert_eq!(sample(100, 0), 0, "the first pause is silence");
    assert_eq!(sample(150 * 588 + 10, 0), 16384);
    let m = (150 + 75 + 38) * 588 + 100;
    assert_eq!((sample(m, 0), sample(m, 1)), (8192, 8192), "mono became stereo at the same level");
    assert_eq!(sample(338 * 588 + 587, 0), 0, "the padding after the audio is silence");
    let cue = std::fs::read_to_string(&r.cue).unwrap();
    assert!(cue.contains("INDEX 01 00:02:00") && cue.contains("INDEX 00 00:03:00") && cue.contains("INDEX 01 00:03:38"), "{cue}");
    // No temporary file is left behind.
    assert!(std::fs::read_dir(&out).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("wavcraft-tmp")));
}

#[test]
fn a_cancelled_or_invalid_export_leaves_nothing() {
    let d = TempDir::new("cancel");
    let mut l = CdLayout::default();
    assert!(l.export(&d.0.join("o"), "x", &mut |_| {}, &AtomicBool::new(false)).is_err(), "no tracks");
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.5), "a");
    l.tracks[0].isrc = "bad".into();
    assert!(l.export(&d.0.join("o"), "x", &mut |_| {}, &AtomicBool::new(false)).is_err(), "bad ISRC");
    l.tracks[0].isrc.clear();
    assert!(l.export(&d.0.join("o"), "x", &mut |_| {}, &AtomicBool::new(true)).is_err(), "cancelled");
    let left: Vec<_> = std::fs::read_dir(d.0.join("o")).map(|r| r.map(|e| e.unwrap().file_name()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn an_existing_wav_survives_a_failed_export() {
    let d = TempDir::new("keep");
    let out = d.0.join("o");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("x.wav"), b"old").unwrap();
    let mut l = CdLayout::default();
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.5), "a");
    assert!(l.export(&out, "x", &mut |_| {}, &AtomicBool::new(true)).is_err());
    assert_eq!(std::fs::read(out.join("x.wav")).unwrap(), b"old");
}
