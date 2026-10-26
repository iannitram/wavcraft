//! wav-c-116-b (#116, v1 item J3): locked tests for `take_recover`. Do not edit.

use std::path::{Path, PathBuf};

use wavcraft_core::take_recover::{repair_take_file, takes_needing_repair};
use wavcraft_core::take_repair::HeaderRepair;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-take-recover-test-{}-{name}", std::process::id()));
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

/// A 48 kHz PCM take's 44-byte header with the given RIFF and data sizes.
fn header(channels: u16, bits: u16, riff: u32, data: u32) -> [u8; 44] {
    let block = channels * (bits / 8);
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&riff.to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    h[28..32].copy_from_slice(&(48_000 * u32::from(block)).to_le_bytes());
    h[32..34].copy_from_slice(&block.to_le_bytes());
    h[34..36].copy_from_slice(&bits.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// `n` bytes of a repeating pattern standing in for samples.
fn samples(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// Write the parts one after another to `path`; returns the bytes written.
fn write(path: &Path, parts: &[&[u8]]) -> Vec<u8> {
    let bytes = parts.concat();
    std::fs::write(path, &bytes).unwrap();
    bytes
}

#[test]
fn j3_recover_fixes_a_killed_take_in_place() {
    let dir = TempDir::new("fix");
    let path = &dir.0.join("Take 2026-10-25 07.05.12.wav");
    let body = samples(144_000);
    write(path, &[&header(1, 24, 36, 0), &body]);

    let want_header = header(1, 24, 144_036, 144_000);
    assert_eq!(repair_take_file(path).unwrap(), HeaderRepair::Fixed(want_header));

    let after = std::fs::read(path).unwrap();
    assert_eq!(after.len(), 144_044, "the file keeps its length");
    assert_eq!(after, [&want_header[..], &body[..]].concat(), "only the two sizes change; every sample is kept");
}

#[test]
fn j3_recover_leaves_an_intact_take_untouched() {
    let dir = TempDir::new("intact");
    let path = &dir.0.join("Take 2026-10-25 08.00.00.wav");
    let before = write(path, &[&header(2, 24, 6_036, 6_000), &samples(6_000), &[1, 2, 3]]);
    assert_eq!(repair_take_file(path).unwrap(), HeaderRepair::Intact);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn j3_recover_does_not_touch_a_file_that_is_not_a_take() {
    let dir = TempDir::new("not-a-take");
    let mp3 = &dir.0.join("Take 2026-10-25 09.00.00.wav");
    let before = write(mp3, &[b"ID3", &[0u8; 97]]);
    assert_eq!(repair_take_file(mp3).unwrap(), HeaderRepair::NotATake);
    assert_eq!(std::fs::read(mp3).unwrap(), before);

    let short = &dir.0.join("Take 2026-10-25 10.00.00.wav");
    let before = write(short, &[&header(1, 24, 36, 0)[..10]]);
    assert_eq!(repair_take_file(short).unwrap(), HeaderRepair::NotATake);
    assert_eq!(std::fs::read(short).unwrap(), before);
}

#[test]
fn j3_recover_missing_file_is_an_error() {
    let dir = TempDir::new("missing-file");
    let path = &dir.0.join("Take 2026-10-25 11.00.00.wav");
    let err = repair_take_file(path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(!path.exists(), "a missing take is not created");
}

#[test]
fn j3_recover_lists_only_takes_that_need_repair() {
    let dir = TempDir::new("list");
    let stale = header(1, 24, 36, 0);
    let body = samples(3_000);
    let late = write(&dir.0.join("Take 2026-10-25 07.05.12.wav"), &[&stale, &body]);
    write(&dir.0.join("Take 2026-10-24 21.30.00.wav"), &[&stale, &body]);
    write(&dir.0.join("Take 2026-10-25 08.00.00.wav"), &[&header(1, 24, 3_036, 3_000), &body]);
    write(&dir.0.join("notes.wav"), &[&stale, &body]);
    write(&dir.0.join("Take 2026-10-25 09.00.00.txt"), &[&stale, &body]);
    write(&dir.0.join("Take short.wav"), &[&stale[..10]]);
    write(&dir.0.join("Take not a wav.wav"), &[b"ID3", &[0u8; 97]]);
    std::fs::create_dir(dir.0.join("Take folder.wav")).unwrap();

    let found = takes_needing_repair(&dir.0).unwrap();
    assert_eq!(found, vec![dir.0.join("Take 2026-10-24 21.30.00.wav"), dir.0.join("Take 2026-10-25 07.05.12.wav")]);
    assert_eq!(std::fs::read(dir.0.join("Take 2026-10-25 07.05.12.wav")).unwrap(), late, "listing changes no file");
}

#[test]
fn j3_recover_missing_folder_lists_nothing() {
    let dir = TempDir::new("missing-folder");
    assert_eq!(takes_needing_repair(&dir.0.join("no such folder")).unwrap(), Vec::<PathBuf>::new());
}
