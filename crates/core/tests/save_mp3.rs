//! A3.3, A3.4 and A3.10: `SaveFormat::Mp3` is known by its path and its name; `save_mp3` writes it
//! through the craft-mp3 encoder, atomically, refusing what MP3 cannot hold before any file is made;
//! and its size is estimated at constant bit rate (`estimate_mp3_size`; `estimate_size` uses 192 kbps).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::mp3_options::Mp3Options;
use wavcraft_core::save::{
    AiffKind, SampleType, SaveError, SaveFormat, estimate_mp3_size, estimate_size, save, save_failing_after, save_mp3, temp_files,
};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-save-mp3-{}-{name}", std::process::id()));
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

/// A document at `rate` Hz with one channel per entry of `data`, its store in `dir/store`.
fn doc_at(dir: &Path, rate: u32, data: &[Vec<f32>]) -> DocState {
    let store = Store::create_with(&dir.join("store"), StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let channels =
        data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(store.append(b).unwrap()).unwrap()))).collect();
    DocState::new(store.clone(), channels, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap()
}

/// A sine at `freq` Hz and `amp` peak, `frames` long, the same on every channel of `channels`.
fn tone(rate: u32, freq: f32, amp: f32, frames: usize, channels: usize) -> Vec<Vec<f32>> {
    let one: Vec<f32> = (0..frames).map(|n| amp * (2.0 * std::f32::consts::PI * freq * n as f32 / rate as f32).sin()).collect();
    vec![one; channels]
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

/// A document of `frames` frames of 0.1 at `rate` Hz with `channels` channels, its store in a
/// folder under `dir` (made by the caller and removed by it).
fn silent_doc(dir: &Path, frames: u64, rate: u32, channels: usize) -> DocState {
    let folder = dir.join(format!("store-{rate}-{frames}-{channels}"));
    let store = Store::create_with(&folder, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let mut chans = Vec::with_capacity(channels);
    for _ in 0..channels {
        let mut pieces = Vec::new();
        let mut left = frames;
        while left > 0 {
            let n = left.min(BLOCK_FRAMES as u64);
            let samples = vec![0.1_f32; n as usize];
            pieces.push(Piece::whole(store.append(&samples).unwrap()).unwrap());
            left -= n;
        }
        chans.push(Channel::from_pieces(pieces));
    }
    DocState::new(store, chans, rate, SourceFormat::synthetic(rate, channels as u16)).unwrap()
}

#[test]
fn mp3_is_a_save_format() {
    assert_eq!(SaveFormat::from_path(Path::new("a.MP3")), Some(SaveFormat::Mp3));
    assert_eq!(SaveFormat::parse(" mp3 "), Some(SaveFormat::Mp3));
    assert_eq!(SaveFormat::Mp3.name(), "MP3");
    assert_eq!(SaveFormat::Mp3.extension(), "mp3");
    assert!(SaveFormat::Mp3.accepts(SampleType::Float32));
}

#[test]
fn a_stereo_tone_round_trips_at_192() {
    let dir = TempDir::new("tone");
    let doc = doc_at(&dir.0, 44_100, &tone(44_100, 1000.0, 0.5, 441_000, 2));
    let target = dir.0.join("tone.mp3");
    save_mp3(&doc, &target, Mp3Options::default(), &mut |_| {}).unwrap();
    let len = std::fs::metadata(&target).unwrap().len();
    assert!((238_000..=243_000).contains(&len), "file is {len} bytes");

    // The finished Info tag replaced the placeholder at the start: a stereo first frame has 32
    // bytes of side information after its 4-byte header, so "Info" sits at 36 and the frame count
    // (big-endian, after the id and flags) at 44. The placeholder has none, so this fails without the copy.
    let bytes = std::fs::read(&target).unwrap();
    assert_eq!(&bytes[36..40], b"Info", "no Info tag at the start of the file");
    let tag_frames = u32::from_be_bytes(bytes[44..48].try_into().unwrap());
    assert!(tag_frames > 0, "the Info tag still holds the placeholder (0 frames)");
    let tagged = u64::from(tag_frames) * 1152;
    assert!((441_000..=444_500).contains(&tagged), "the tag counts {tagged} samples");

    let target2 = dir.0.join("tone2.mp3");
    save(&doc, &target2, SaveFormat::Mp3, SampleType::Float32, &mut |_| {}).unwrap();
    let len2 = std::fs::metadata(&target2).unwrap().len();
    assert!((238_000..=243_000).contains(&len2), "save() file is {len2} bytes");

    let sessions = dir.0.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let back = reopen(&target, &sessions);
    assert_eq!(back.channel_count(), 2);
    assert_eq!(back.sample_rate(), 44_100);
    let frames = back.len_frames();
    assert!((441_000..=444_500).contains(&frames), "{frames} frames");

    // The middle second of channel 0: a 1 kHz sine at 0.5 peak has an RMS of -9.03 dBFS.
    let mut middle = vec![0f32; 44_100];
    assert_eq!(back.read(0, 220_500, &mut middle).unwrap(), middle.len());
    let mean_square = middle.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / middle.len() as f64;
    let rms_db = 10.0 * mean_square.log10();
    assert!((rms_db - (-9.03)).abs() <= 0.5, "middle-second RMS is {rms_db} dBFS");
}

#[test]
fn a_failed_mp3_save_leaves_the_target_whole() {
    let dir = TempDir::new("fail");
    let doc = doc_at(&dir.0, 44_100, &tone(44_100, 1000.0, 0.5, 441_000, 2));
    let target = dir.0.join("out.mp3");
    std::fs::write(&target, b"old mp3 bytes").unwrap();
    let err = save_failing_after(&doc, &target, SaveFormat::Mp3, SampleType::Float32, BLOCK_FRAMES as u64).unwrap_err();
    assert!(matches!(err, SaveError::Io(_)), "{err:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old mp3 bytes");
    assert!(temp_files(&target).is_empty());
}

#[test]
fn a_96k_file_is_refused() {
    let dir = TempDir::new("rate");
    let doc = doc_at(&dir.0, 96_000, &tone(96_000, 1000.0, 0.5, 96_000, 2));
    let target = dir.0.join("out.mp3");
    let err = save_mp3(&doc, &target, Mp3Options::default(), &mut |_| {}).unwrap_err();
    match err {
        SaveError::Unsupported(m) => assert!(m.contains("96000"), "{m}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
    assert!(!target.exists());
    assert!(temp_files(&target).is_empty());
}

#[test]
fn mono_option_on_stereo_is_refused_until_a35() {
    let dir = TempDir::new("mono");
    let doc = doc_at(&dir.0, 44_100, &tone(44_100, 1000.0, 0.5, 44_100, 2));
    let target = dir.0.join("out.mp3");
    let err = save_mp3(&doc, &target, Mp3Options { bitrate_kbps: 128, mono: true }, &mut |_| {}).unwrap_err();
    assert!(matches!(err, SaveError::Unsupported(_)), "{err:?}");
    assert!(!target.exists());
    assert!(temp_files(&target).is_empty());
}

#[test]
fn the_mp3_estimate_is_the_cbr_size() {
    let dir = std::env::temp_dir().join(format!("wavcraft-mp3-cbr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // 10 s, 44.1 kHz stereo: 192 kbps is 240000 bytes (estimate_size uses the default rate).
    let stereo = silent_doc(&dir, 441_000, 44_100, 2);
    let est = estimate_size(&stereo, SaveFormat::Mp3, SampleType::Int16, AiffKind::Aiff).unwrap();
    assert!((237_600..=242_400).contains(&est.bytes), "{}", est.bytes);
    assert!(!est.exact);
    // 5 s, 48 kHz, mono mix at 128 kbps: 80000 bytes.
    let mono = silent_doc(&dir, 240_000, 48_000, 2);
    let est = estimate_mp3_size(&mono, Mp3Options { bitrate_kbps: 128, mono: true }).unwrap();
    assert!((79_200..=80_800).contains(&est.bytes), "{}", est.bytes);
    assert!(!est.exact);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_mp3_estimate_refuses_what_the_save_would() {
    let dir = std::env::temp_dir().join(format!("wavcraft-mp3-refuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rate = silent_doc(&dir, 96_000, 96_000, 2);
    let err = estimate_mp3_size(&rate, Mp3Options::default()).unwrap_err();
    assert!(err.to_string().contains("96000"), "{err}");
    let bitrate = silent_doc(&dir, 44_100, 44_100, 2);
    let err = estimate_mp3_size(&bitrate, Mp3Options { bitrate_kbps: 100, mono: false }).unwrap_err();
    assert!(err.to_string().contains("100 kbps"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
