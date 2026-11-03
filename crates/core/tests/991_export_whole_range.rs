//! #991: `save_slice` (Export Audio of Range Markers) writes `fmt `, `data` and the chunks the slice
//! hands it, whether or not the slice happens to cover the whole document.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavcraft_core::Markers;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::export::{DEFAULT_CHANNEL_PATTERN, DEFAULT_RANGE_PATTERN, ExportOptions, export_ranges, extract_channels};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, Slice, save_slice, save_wav};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;
const LEN: usize = 6_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-991-{}-{name}", std::process::id()));
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

/// A stereo document of LEN frames with a point marker, a range inside it and (when `whole`) a range
/// over the whole file.
fn doc(dir: &Path, whole: bool) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = (0..2usize)
        .map(|c| {
            let v: Vec<f32> = (0..LEN).map(|i| ((i * (c + 3)) % 200) as f32 / 256.0 - 0.4).collect();
            Channel::from_pieces([Piece::whole(s.append(&v).unwrap()).unwrap()])
        })
        .collect();
    let d = DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, 2)).unwrap();
    let mut m = Markers::new().with_added(50..50, Some("point")).unwrap().with_added(1_000..2_000, Some("inside")).unwrap();
    if whole {
        m = m.with_added(0..LEN as u64, Some("everything")).unwrap();
    }
    d.with_markers(Arc::new(m))
}

fn opts(pattern: &str) -> ExportOptions {
    ExportOptions::new(SampleType::Float32, pattern)
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let o = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &o, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

/// The ids of a RIFF file's chunks in order.
fn chunk_ids(path: &Path) -> Vec<String> {
    let b = std::fs::read(path).unwrap();
    let (mut at, mut ids) = (12, Vec::new());
    while at + 8 <= b.len() {
        ids.push(String::from_utf8_lossy(&b[at..at + 4]).into_owned());
        let size = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
        at += 8 + size + (size & 1);
    }
    ids
}

/// Export the range named `name` alone.
fn export_one(d: &DocState, out: &Path, name: &str) -> PathBuf {
    let id = d.markers().iter().find(|m| m.name == name).unwrap().id;
    export_ranges(d, out, Some(&[id]), "song", &opts(DEFAULT_RANGE_PATTERN), &mut |_| {}).unwrap().remove(0)
}

fn slice_of(markers: Vec<u8>) -> Slice {
    Slice { channels: vec![0, 1], range: 0..LEN as u64, markers }
}

#[test]
fn a_range_marker_spanning_the_whole_file_exports_with_no_cue_chunk() {
    let t = TempDir::new("nocue");
    let d = doc(&t.0.join("s"), true);
    let f = export_one(&d, &t.0.join("out"), "everything");
    let bytes = std::fs::read(&f).unwrap();
    assert!(!bytes.windows(4).any(|w| w == b"cue "), "chunks: {:?}", chunk_ids(&f));
    assert!(!bytes.windows(4).any(|w| w == b"LIST"), "chunks: {:?}", chunk_ids(&f));
}

#[test]
fn a_whole_file_range_exports_a_file_that_reopens_with_no_markers() {
    let t = TempDir::new("reopen");
    let d = doc(&t.0.join("s"), true);
    let f = export_one(&d, &t.0.join("out"), "everything");
    let back = reopen(&f, &t.0.join("r"));
    assert!(back.markers().is_empty(), "markers: {}", back.markers().len());
    assert_eq!((back.channel_count(), back.len_frames()), (2, LEN as u64));
}

#[test]
fn a_whole_file_range_exports_the_same_chunks_as_a_range_that_does_not_span_the_file() {
    let t = TempDir::new("layout");
    let d = doc(&t.0.join("s"), true);
    let whole = chunk_ids(&export_one(&d, &t.0.join("out"), "everything"));
    let part = chunk_ids(&export_one(&d, &t.0.join("out2"), "inside"));
    assert_eq!(part.first().map(String::as_str), Some("fmt "));
    assert_eq!(whole, part);
}

#[test]
fn save_slice_over_the_whole_document_writes_none_of_the_documents_own_markers() {
    let t = TempDir::new("slice");
    let d = doc(&t.0.join("s"), true);
    let target = t.0.join("slice.wav");
    save_slice(&d, &slice_of(Vec::new()), &target, SampleType::Float32, &mut |_| {}, None).unwrap();
    let ids = chunk_ids(&target);
    assert!(ids.iter().all(|i| i != "cue " && i != "LIST"), "chunks: {ids:?}");
    assert!(reopen(&target, &t.0.join("r")).markers().is_empty());
}

#[test]
fn save_slice_writes_the_marker_bytes_it_is_given_exactly_once() {
    let t = TempDir::new("given");
    let d = doc(&t.0.join("s"), true);
    let given = Markers::new().with_added(10..10, Some("kept")).unwrap();
    let target = t.0.join("slice.wav");
    save_slice(&d, &slice_of(given.to_wav_chunks()), &target, SampleType::Float32, &mut |_| {}, None).unwrap();
    assert_eq!(chunk_ids(&target).iter().filter(|i| *i == "cue ").count(), 1);
    let back = reopen(&target, &t.0.join("r"));
    assert_eq!(back.markers().iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["kept"], "only the given markers, not the document's");
}

#[test]
fn extracted_mono_files_still_carry_the_documents_markers() {
    let t = TempDir::new("extract");
    let d = doc(&t.0.join("s"), true);
    let files = extract_channels(&d, &t.0.join("out"), "song", &opts(DEFAULT_CHANNEL_PATTERN), &mut |_| {}).unwrap();
    assert_eq!(files.len(), 2);
    for f in &files {
        let back = reopen(f, &t.0.join("r"));
        assert_eq!(back.markers().len(), d.markers().len(), "{}", f.display());
    }
}

#[test]
fn a_whole_document_save_still_copies_the_documents_markers() {
    let t = TempDir::new("save");
    let d = doc(&t.0.join("s"), true);
    let target = t.0.join("whole.wav");
    save_wav(&d, &target, SampleType::Float32, &mut |_| {}).unwrap();
    assert!(chunk_ids(&target).iter().any(|i| i == "cue "));
    assert_eq!(reopen(&target, &t.0.join("r")).markers().len(), d.markers().len());
}
