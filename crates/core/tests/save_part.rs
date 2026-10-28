//! f43: Save Selection As and Export write part or all of a document without touching it.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::ChannelMask;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{FileKind, Part, SampleType, temp_files, write_failing_after, write_file};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-savepart-test-{}-{name}", std::process::id()));
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

/// Values exactly representable in 24 bits, different in each channel, over two blocks.
fn data(frames: usize) -> Vec<Vec<f32>> {
    (0..2).map(|c| (0..frames).map(|k| (((k * 7_919 + c * 104_729) % 16_000_000) as f32 - 8_000_000.0) / 8_388_608.0).collect()).collect()
}

fn doc_from(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

fn read(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

#[test]
fn saved_selection_samples_equal_the_source_range() {
    let t = TempDir::new("range");
    let src = data(2 * BLOCK_FRAMES + 1000);
    let doc = doc_from(&t.0.join("store"), &src);
    let (a, b) = (BLOCK_FRAMES as u64 - 17, 2 * BLOCK_FRAMES as u64 + 5);
    let out = t.0.join("sel.wav");
    write_file(&doc, &out, FileKind::Wav(SampleType::Int24), &Part::range(&doc, a..b, ChannelMask::ALL), &mut |_| {}).unwrap();
    let back = reopen(&out, &t.0.join("sessions"));
    assert_eq!((back.len_frames(), back.channel_count()), (b - a, 2));
    for (ch, s) in src.iter().enumerate() {
        assert_eq!(read(&back, ch), s[a as usize..b as usize], "channel {ch}");
    }
}

#[test]
fn only_the_enabled_channels_are_written() {
    let t = TempDir::new("mask");
    let src = data(5000);
    let doc = doc_from(&t.0.join("store"), &src);
    let out = t.0.join("right.wav");
    write_file(&doc, &out, FileKind::Wav(SampleType::Int24), &Part::range(&doc, 100..4000, ChannelMask::only(1)), &mut |_| {}).unwrap();
    let back = reopen(&out, &t.0.join("sessions"));
    assert_eq!(back.channel_count(), 1);
    assert_eq!(read(&back, 0), src[1][100..4000]);
}

#[test]
fn an_export_writes_the_whole_document_and_leaves_it_as_it_was() {
    let t = TempDir::new("export");
    let src = data(BLOCK_FRAMES + 321);
    let doc = doc_from(&t.0.join("store"), &src);
    let before: Vec<Vec<f32>> = (0..2).map(|c| read(&doc, c)).collect();
    let out = t.0.join("copy.wav");
    write_file(&doc, &out, FileKind::Wav(SampleType::Int24), &Part::whole(&doc), &mut |_| {}).unwrap();
    let back = reopen(&out, &t.0.join("sessions"));
    for (ch, s) in src.iter().enumerate() {
        assert_eq!(&read(&back, ch), s);
        assert_eq!(read(&doc, ch), before[ch], "the document is unchanged");
    }
}

#[test]
fn a_failed_part_write_leaves_no_temp_file_and_no_target() {
    let t = TempDir::new("fail");
    let doc = doc_from(&t.0.join("store"), &data(3 * BLOCK_FRAMES));
    let out = t.0.join("never.wav");
    let part = Part::range(&doc, 10..3 * BLOCK_FRAMES as u64, ChannelMask::ALL);
    assert!(write_failing_after(&doc, &out, SampleType::Int24, &part, BLOCK_FRAMES as u64).is_err());
    assert!(!out.exists(), "no target made");
    assert!(temp_files(&out).is_empty(), "no temp file left behind");
    let names: Vec<_> = std::fs::read_dir(&t.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!names.iter().any(|n| n.contains("wavcraft-tmp")), "{names:?}");
}

#[test]
fn an_empty_selection_writes_nothing() {
    let t = TempDir::new("empty");
    let doc = doc_from(&t.0.join("store"), &data(1000));
    let out = t.0.join("none.wav");
    assert!(write_file(&doc, &out, FileKind::Wav(SampleType::Int16), &Part::range(&doc, 500..500, ChannelMask::ALL), &mut |_| {}).is_err());
    assert!(!out.exists() && temp_files(&out).is_empty());
}

#[test]
fn invalid_public_parts_fail_without_creating_a_file() {
    let t = TempDir::new("invalid");
    let doc = doc_from(&t.0.join("store"), &data(1000));
    let out = t.0.join("invalid.wav");
    let (start, end) = (900, 100);
    for part in
        [Part { frames: start..end, channels: vec![0] }, Part { frames: 0..1001, channels: vec![0] }, Part { frames: 0..100, channels: vec![2] }]
    {
        assert!(write_file(&doc, &out, FileKind::Wav(SampleType::Int16), &part, &mut |_| {}).is_err());
        assert!(!out.exists());
        assert!(temp_files(&out).is_empty());
    }
}
