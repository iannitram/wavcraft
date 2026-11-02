//! Locked tests for #1009. Open Append must refuse a W64, CAF or AU file whose sample rate
//! is not the document's before anything is written to the store.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::io::{self, OpenError};
use wavcraft_core::store::{Store, StoreOptions};

const FRAMES: usize = 100;
const FILE_RATE: u32 = 44_100;
const DOC_RATE: u32 = 48_000;

const W64_RIFF: [u8; 16] = [0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00];
const W64_WAVE: [u8; 16] = [0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
const W64_FMT: [u8; 16] = [0x66, 0x6D, 0x74, 0x20, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
const W64_DATA: [u8; 16] = [0x64, 0x61, 0x74, 0x61, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-core-issue1009-{}-{name}", std::process::id()));
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

/// 100 mono 16-bit frames of a ramp, little-endian or big-endian.
fn pcm16(big_endian: bool) -> Vec<u8> {
    (0..FRAMES as i16).flat_map(|i| if big_endian { (i * 100).to_be_bytes() } else { (i * 100).to_le_bytes() }).collect()
}

fn w64(rate: u32) -> Vec<u8> {
    let data = pcm16(false);
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * 2).to_le_bytes());
    fmt.extend_from_slice(&2u16.to_le_bytes());
    fmt.extend_from_slice(&16u16.to_le_bytes());
    let data_chunk = 24 + data.len() as u64;
    let total = 40 + 24 + fmt.len() as u64 + data_chunk;
    let mut b = Vec::new();
    b.extend_from_slice(&W64_RIFF);
    b.extend_from_slice(&total.to_le_bytes());
    b.extend_from_slice(&W64_WAVE);
    b.extend_from_slice(&W64_FMT);
    b.extend_from_slice(&(24 + fmt.len() as u64).to_le_bytes());
    b.extend_from_slice(&fmt);
    b.extend_from_slice(&W64_DATA);
    b.extend_from_slice(&data_chunk.to_le_bytes());
    b.extend_from_slice(&data);
    b
}

fn caf(rate: u32) -> Vec<u8> {
    let data = pcm16(true);
    let mut b = Vec::new();
    b.extend_from_slice(b"caff");
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&0u16.to_be_bytes());
    b.extend_from_slice(b"desc");
    b.extend_from_slice(&32i64.to_be_bytes());
    b.extend_from_slice(&f64::from(rate).to_be_bytes());
    b.extend_from_slice(b"lpcm");
    for v in [0u32, 2, 1, 1, 16] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(4 + data.len() as i64).to_be_bytes());
    b.extend_from_slice(&0u32.to_be_bytes());
    b.extend_from_slice(&data);
    b
}

fn au(rate: u32) -> Vec<u8> {
    let data = pcm16(true);
    let mut b = Vec::new();
    b.extend_from_slice(b".snd");
    for v in [24u32, data.len() as u32, 3, rate, 1] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(&data);
    b
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn store_in(dir: &Path) -> Store {
    Store::create_session(Some(&dir.join("s")), StoreOptions::default()).unwrap()
}

/// Opens `path` as an Open Append of a `DOC_RATE` document; returns the result and whether any block id was taken.
fn append(path: &Path, store: &Store) -> (Result<wavcraft_core::doc::DocState, OpenError>, bool) {
    let before = store.next_block_id();
    let r = io::open_into(path, store, DOC_RATE, None, &mut |_| {}, &AtomicBool::new(false));
    (r, store.next_block_id() != before)
}

fn mismatch() -> OpenError {
    OpenError::RateMismatch { file: FILE_RATE, expected: DOC_RATE }
}

#[test]
fn a_w64_file_with_a_different_rate_is_refused_with_rate_mismatch() {
    let t = TempDir::new("w64");
    let store = store_in(&t.0);
    let (r, _) = append(&write(&t.0, "a.w64", &w64(FILE_RATE)), &store);
    assert_eq!(r.unwrap_err(), mismatch());
}

#[test]
fn a_caf_file_with_a_different_rate_is_refused_with_rate_mismatch() {
    let t = TempDir::new("caf");
    let store = store_in(&t.0);
    let (r, _) = append(&write(&t.0, "a.caf", &caf(FILE_RATE)), &store);
    assert_eq!(r.unwrap_err(), mismatch());
}

#[test]
fn an_au_file_with_a_different_rate_is_refused_with_rate_mismatch() {
    let t = TempDir::new("au");
    let store = store_in(&t.0);
    let (r, _) = append(&write(&t.0, "a.au", &au(FILE_RATE)), &store);
    assert_eq!(r.unwrap_err(), mismatch());
}

#[test]
fn a_refused_w64_caf_or_au_file_writes_no_block_to_the_store() {
    let t = TempDir::new("noblocks");
    let store = store_in(&t.0);
    for (name, bytes) in [("a.w64", w64(FILE_RATE)), ("a.caf", caf(FILE_RATE)), ("a.au", au(FILE_RATE))] {
        let (_, wrote) = append(&write(&t.0, name, &bytes), &store);
        assert!(!wrote, "{name}: the store took a block id before the rate was refused");
    }
}

#[test]
fn w64_caf_and_au_files_at_the_documents_rate_still_open_into_the_store() {
    let t = TempDir::new("same");
    let store = store_in(&t.0);
    for (name, bytes) in [("a.w64", w64(DOC_RATE)), ("a.caf", caf(DOC_RATE)), ("a.au", au(DOC_RATE))] {
        let (r, _) = append(&write(&t.0, name, &bytes), &store);
        let state = r.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(state.len_frames(), FRAMES as u64, "{name}");
        assert!(state.store().same_store(&store), "{name}");
    }
}
