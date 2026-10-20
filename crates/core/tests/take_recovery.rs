//! C032: takes a crash left behind are found, repaired without losing audio, and never deleted
//! by anything but the owner's confirmed discard.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use wavcraft_core::Document;
use wavcraft_core::io::OpenOptions;
use wavcraft_core::record::{RecFormat, Recorder, Ring, TakeWriter, create_take, marker_path};
use wavcraft_core::take_recovery::{discard_take, repair_take, unfinished_takes};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-takerec-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tone(from: usize, n: usize) -> Vec<f32> {
    (from..from + n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
}

/// A take of `header_frames` frames the header knows and `extra` more written after the last
/// header update, whose writer then goes away without finishing: what a kill leaves.
fn crashed_take(dir: &Path, fmt: RecFormat, header_frames: usize, extra: usize) -> PathBuf {
    let mut w = create_take(dir, fmt, 100).unwrap();
    let path = w.path().to_path_buf();
    w.write_frames(&tone(0, header_frames * usize::from(fmt.channels))).unwrap();
    w.update_header().unwrap();
    w.write_frames(&tone(header_frames * usize::from(fmt.channels), extra * usize::from(fmt.channels))).unwrap();
    drop(w);
    path
}

fn open(path: &Path, sessions: &Path) -> Document {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    Document::open_in(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn a_take_has_a_marker_until_it_is_finished() {
    let dir = scratch("marker");
    let fmt = RecFormat { rate: 48_000, channels: 1, bits: 16 };
    let ring = Ring::new(48_000, 1);
    let rec = Recorder::start(create_take(&dir, fmt, 1).unwrap(), ring.clone()).unwrap();
    let marker = marker_path(rec.path());
    assert!(marker.exists(), "made with the take");
    ring.push(&[0.25; 480]);
    std::thread::sleep(Duration::from_millis(30));
    let take = rec.stop();
    assert!(!marker.exists(), "removed by a clean stop");
    assert!(take.path.exists());
    assert!(unfinished_takes(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dropped_recorder_finishes_cleanly_too() {
    let dir = scratch("dropped");
    let ring = Ring::new(4_800, 1);
    let rec = Recorder::start(create_take(&dir, RecFormat::default(), 2).unwrap(), ring.clone()).unwrap();
    let marker = marker_path(rec.path());
    ring.push(&[0.1; 100]);
    drop(rec);
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_live_take_is_not_offered_and_a_dead_one_is() {
    let dir = scratch("live");
    let fmt = RecFormat { rate: 48_000, channels: 1, bits: 24 };
    let mut live: TakeWriter = create_take(&dir, fmt, 5).unwrap();
    live.write_frames(&tone(0, 4_800)).unwrap();
    live.update_header().unwrap();
    assert!(unfinished_takes(&dir).is_empty(), "the marker is locked by the recorder");
    let path = live.path().to_path_buf();
    drop(live); // the process "dies": the lock goes, the marker stays
    let found = unfinished_takes(&dir);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, path);
    assert_eq!(found[0].frames, 4_800);
    assert!(found[0].header_ok);
    assert!(path.exists() && marker_path(&path).exists(), "listing deletes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn repair_recovers_every_frame_written_even_after_the_last_header_update() {
    let dir = scratch("repair");
    let fmt = RecFormat { rate: 48_000, channels: 2, bits: 24 };
    let path = crashed_take(&dir, fmt, 4_800, 2_400);
    let before = std::fs::read(&path).unwrap();
    let found = unfinished_takes(&dir);
    assert_eq!((found.len(), found[0].frames, found[0].header_ok), (1, 7_200, false));
    assert_eq!(repair_take(&found[0]).unwrap(), 7_200);
    let after = std::fs::read(&path).unwrap();
    assert_eq!(after.len(), before.len(), "never shortened");
    assert_eq!(after[44..], before[44..], "the audio bytes are untouched");
    assert!(!marker_path(&path).exists());
    assert!(unfinished_takes(&dir).is_empty(), "nothing left on the next start");
    let doc = open(&path, &dir);
    assert_eq!(doc.frames(), 7_200);
    assert_eq!(doc.audio.channels.len(), 2);
    let want = tone(0, 7_200 * 2);
    for i in (0..7_200).step_by(97) {
        assert!((doc.audio.channels[0][i] - want[i * 2]).abs() < 1e-4, "frame {i}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_partial_frame_at_the_end_is_kept_not_cut() {
    let dir = scratch("partial");
    let fmt = RecFormat { rate: 48_000, channels: 1, bits: 24 };
    let path = crashed_take(&dir, fmt, 1_000, 500);
    {
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&[1, 2]).unwrap();
    }
    let len = std::fs::metadata(&path).unwrap().len();
    let found = unfinished_takes(&dir);
    assert_eq!(found[0].frames, 1_500);
    repair_take(&found[0]).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), len, "the two stray bytes are still there");
    assert_eq!(open(&path, &dir).frames(), 1_500, "and the take still opens");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_header_is_rebuilt_from_the_marker() {
    let dir = scratch("damaged");
    let fmt = RecFormat { rate: 44_100, channels: 1, bits: 16 };
    let path = crashed_take(&dir, fmt, 2_000, 1_000);
    {
        use std::io::{Seek, Write};
        let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.seek(std::io::SeekFrom::Start(0)).unwrap();
        f.write_all(&[0u8; 44]).unwrap();
    }
    let found = unfinished_takes(&dir);
    assert_eq!((found.len(), found[0].fmt, found[0].frames), (1, fmt, 3_000));
    repair_take(&found[0]).unwrap();
    let doc = open(&path, &dir);
    assert_eq!((doc.frames(), doc.audio.sample_rate), (3_000, 44_100));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_take_with_no_audio_keeps_its_file_and_loses_only_the_marker() {
    let dir = scratch("empty");
    let w = create_take(&dir, RecFormat::default(), 7).unwrap();
    let path = w.path().to_path_buf();
    drop(w);
    assert!(unfinished_takes(&dir).is_empty());
    assert!(path.exists(), "the file is never deleted by a scan");
    assert!(!marker_path(&path).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_marker_without_its_take_is_left_alone() {
    let dir = scratch("orphan");
    let marker = dir.join("Take x.wav.recording");
    std::fs::write(&marker, "WavCraft unfinished take\nrate=48000\nch=1\nbits=24\n").unwrap();
    assert!(unfinished_takes(&dir).is_empty());
    assert!(marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discard_removes_the_take_and_its_marker_only_when_asked() {
    let dir = scratch("discard");
    let path = crashed_take(&dir, RecFormat::default(), 100, 10);
    let other = dir.join("keep me.wav");
    std::fs::write(&other, b"x").unwrap();
    let found = unfinished_takes(&dir);
    assert!(path.exists());
    discard_take(&found[0]).unwrap();
    assert!(!path.exists() && !marker_path(&path).exists());
    assert!(other.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
