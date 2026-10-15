//! Item 03b: the store's size estimate, refusing an open when the session folder's disk is too
//! full (with an injected free-space function, so no real disk is filled), and a disk that fills
//! up part way through.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};
use wavcraft_core::io::{OpenError, OpenOptions, open};
use wavcraft_core::space::{self, MARGIN_BYTES, raw_store_bytes, store_bytes};
use wavcraft_core::store::StoreOptions;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-space-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn is_empty(&self) -> bool {
        std::fs::read_dir(&self.0).unwrap().count() == 0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

thread_local! {
    /// What the injected free-space function says, per test thread (`open` asks on the
    /// calling thread).
    static FREE: Cell<Option<u64>> = const { Cell::new(None) };
}

fn injected_free(_: &Path) -> Option<u64> {
    FREE.with(Cell::get)
}

fn open_with_free(path: &Path, root: &Path, free: Option<u64>) -> Result<wavcraft_core::DocState, OpenError> {
    FREE.with(|f| f.set(free));
    let opts = OpenOptions { sessions_root: Some(root.to_path_buf()), free_space: Some(injected_free), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false))
}

fn write_file(dir: &Path, name: &str, format: FileFormat, depth: BitDepth, channels: usize, frames: usize) -> PathBuf {
    let buf = AudioBuffer {
        sample_rate: 48_000,
        channels: (0..channels).map(|c| (0..frames).map(|n| ((n * (c + 3)) % 200) as f32 / 200.0 - 0.5).collect()).collect(),
    };
    let bytes = soundcraft_audio_io::encode(&buf, &EncodeOptions { format, bit_depth: depth, dither: false, bwf: None }).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn the_estimate_is_four_bytes_a_sample_plus_a_margin_whatever_the_source_depth() {
    // The arithmetic: 3 hours of 48 kHz stereo is 4,147,200,000 bytes of f32 (the 4.1 GB store
    // of a 2.1 GB 16-bit file), plus 1 % plus the margin.
    let frames = 3 * 3600 * 48_000;
    assert_eq!(raw_store_bytes(frames, 2), 4_147_200_000);
    assert_eq!(raw_store_bytes(frames, 1), 2_073_600_000);
    assert_eq!(store_bytes(frames, 2), 4_147_200_000 + 41_472_000 + MARGIN_BYTES);
    assert_eq!(store_bytes(0, 2), MARGIN_BYTES);
    assert_eq!(store_bytes(u64::MAX, 8), u64::MAX, "saturates, never wraps");
    // A compressed file of unknown length: an hour at the assumed 128 kbit/s is 57.6 MB.
    assert_eq!(space::frames_from_file_size(57_600_000, 48_000), 3600 * 48_000);
    assert_eq!(space::frames_from_file_size(0, 48_000), 0);

    // And what open asks for, per file: 16-bit, 24-bit and float WAV, mono and stereo, AIFF,
    // and FLAC (whose length its header gives). Asked with no room, each is refused with the
    // estimate for its own frames and channels; with room, the store it writes is the raw part
    // of that estimate exactly.
    let dir = TempDir::new("estimate");
    let root = TempDir::new("estimate-sessions");
    let frames = 100_000usize;
    let mut cases = Vec::new();
    for channels in [1usize, 2] {
        for (depth, tag) in [(BitDepth::Int16, "16"), (BitDepth::Int24, "24"), (BitDepth::Float32, "float")] {
            cases.push((format!("w{channels}-{tag}.wav"), FileFormat::Wav, depth, channels));
        }
        cases.push((format!("a{channels}-16.aiff"), FileFormat::Aiff, BitDepth::Int16, channels));
        cases.push((format!("f{channels}-16.flac"), FileFormat::Flac, BitDepth::Int16, channels));
    }
    for (name, format, depth, channels) in cases {
        let path = write_file(&dir.0, &name, format, depth, channels, frames);
        let want = store_bytes(frames as u64, channels as u64);
        match open_with_free(&path, &root.0, Some(0)) {
            Err(OpenError::NoSpace { needed, free, folder, estimated }) => {
                assert_eq!((needed, free, estimated), (want, 0, false), "{name}");
                assert_eq!(folder, root.0, "{name}: the message names the folder a person sets");
            }
            other => panic!("{name}: expected NoSpace, got {other:?}"),
        }
        assert!(root.is_empty(), "{name}: a refused open leaves no session folder");
        let doc = open_with_free(&path, &root.0, Some(want)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.disk_bytes_referenced(), raw_store_bytes(frames as u64, channels as u64), "{name}");
    }
}

#[test]
fn too_little_room_is_refused_with_a_clear_message_and_unknown_room_is_not() {
    let dir = TempDir::new("refuse");
    let root = TempDir::new("refuse-sessions");
    let path = write_file(&dir.0, "s.wav", FileFormat::Wav, BitDepth::Int16, 2, 50_000);
    let needed = store_bytes(50_000, 2);

    let err = open_with_free(&path, &root.0, Some(needed - 1)).unwrap_err();
    assert_eq!(err, OpenError::NoSpace { needed, free: needed - 1, folder: root.0.clone(), estimated: false });
    let msg = err.to_string();
    for part in [
        space::format_bytes(needed).as_str(),
        &space::format_bytes(needed - 1),
        &root.0.display().to_string(),
        "WAVCRAFT_SESSION_DIR",
        "Session Folder",
    ] {
        assert!(msg.contains(part), "the message names {part:?}: {msg}");
    }
    assert!(root.is_empty());

    // Exactly enough opens; an unknown free space opens (the check is skipped).
    assert!(open_with_free(&path, &root.0, Some(needed)).is_ok());
    assert!(open_with_free(&path, &root.0, None).is_ok());
    assert!(root.is_empty(), "documents dropped, their session folders gone");
}

#[test]
fn a_disk_that_fills_up_part_way_gives_an_error_and_leaves_nothing() {
    let dir = TempDir::new("full");
    let root = TempDir::new("full-sessions");
    // 300,000 stereo frames is 2.4 MB of f32; the store may take 1 MB.
    for (name, format) in [("big.wav", FileFormat::Wav), ("big.flac", FileFormat::Flac)] {
        let path = write_file(&dir.0, name, format, BitDepth::Int16, 2, 300_000);
        let opts = OpenOptions {
            sessions_root: Some(root.0.clone()),
            store: StoreOptions { max_bytes: Some(1 << 20), ..StoreOptions::default() },
            free_space: Some(|_| Some(u64::MAX)),
        };
        let err = open(&path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(err, OpenError::DiskFull { folder: root.0.clone() }, "{name}");
        assert!(err.to_string().contains("WAVCRAFT_SESSION_DIR"), "{name}: {err}");
        assert!(root.is_empty(), "{name}: the partial session is gone");
    }
}

#[test]
fn the_real_free_space_is_known_for_the_temp_folder() {
    let free = space::free_space(&std::env::temp_dir());
    assert!(free.is_some_and(|f| f > 0), "{free:?}");
}
