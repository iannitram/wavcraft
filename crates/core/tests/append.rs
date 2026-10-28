//! f57: Open Append (to the current document, to a new one) and the rate check of `open_into`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::DocState;
use wavcraft_core::doc::append::{APPEND, append_files, new_from_files};
use wavcraft_core::doc::history::History;
use wavcraft_core::io::{self, OpenError, OpenOptions};
use wavcraft_core::store::{Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-core-append-{}-{name}", std::process::id()));
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

/// The 16-bit value of frame `i` of channel `c` of file number `seed`.
fn value(seed: i16, c: usize, i: u64) -> i16 {
    ((i as i64 * 37 + c as i64 * 1_001 + i64::from(seed) * 5_003) % 20_000) as i16 - 10_000
}

fn write_wav(path: &Path, rate: u32, channels: u16, frames: u64, seed: i16) {
    let data_len = frames as u32 * u32::from(channels) * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        for c in 0..channels as usize {
            b.extend_from_slice(&value(seed, c, i).to_le_bytes());
        }
    }
    std::fs::write(path, b).unwrap();
}

fn opts(root: &Path) -> OpenOptions {
    OpenOptions { sessions_root: Some(root.to_path_buf()), ..OpenOptions::default() }
}

fn open(root: &Path, path: &Path) -> DocState {
    io::open(path, &opts(root), &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

fn read_all(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut out = vec![0.0; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut out).unwrap(), out.len());
    out
}

fn f(seed: i16, c: usize, i: u64) -> f32 {
    f32::from(value(seed, c, i)) / 32_768.0
}

const BLOCKS: u64 = 70_000; // more than one 65,536-frame block

#[test]
fn appended_files_follow_the_document_back_to_back_in_one_edit() {
    let t = TempDir::new("current");
    let (a, b, c) = (t.0.join("a.wav"), t.0.join("b.wav"), t.0.join("c.wav"));
    write_wav(&a, 44_100, 2, 1_000, 1);
    write_wav(&b, 44_100, 2, BLOCKS, 2);
    write_wav(&c, 44_100, 2, 500, 3);
    let base = open(&t.0, &a);
    let r = append_files(&base, &[b.clone(), c.clone()], None, &mut |_| {}, &AtomicBool::new(false));
    assert!(r.refused.is_empty() && !r.cancelled);
    assert_eq!(r.added.len(), 2);
    let edit = r.edit.unwrap();
    assert_eq!(edit.label, APPEND);
    let doc = &edit.state;
    assert_eq!(doc.len_frames(), 1_000 + BLOCKS + 500);
    for ch in 0..2 {
        let got = read_all(doc, ch);
        assert_eq!(got[0], f(1, ch, 0));
        assert_eq!(got[999], f(1, ch, 999));
        assert_eq!(got[1_000], f(2, ch, 0), "exact join, no crossfade");
        assert_eq!(got[1_000 + BLOCKS as usize - 1], f(2, ch, BLOCKS - 1));
        assert_eq!(got[1_000 + BLOCKS as usize], f(3, ch, 0));
        assert_eq!(*got.last().unwrap(), f(3, ch, 499));
    }
    // It is one history step, and undo gives the document back.
    let mut h = History::new(base.clone(), 1 << 30);
    h.push_edit(edit);
    assert_eq!(h.steps().len(), 2);
    h.undo();
    assert_eq!(h.current().len_frames(), 1_000);
}

#[test]
fn a_file_at_another_rate_is_refused_before_it_is_written_and_the_rest_go_on() {
    let t = TempDir::new("rate");
    let (a, bad, ok) = (t.0.join("a.wav"), t.0.join("bad.wav"), t.0.join("ok.wav"));
    write_wav(&a, 44_100, 1, 100, 1);
    write_wav(&bad, 48_000, 1, 100_000, 2);
    write_wav(&ok, 44_100, 1, 200, 3);
    let base = open(&t.0, &a);
    let before = base.store().bytes_on_disk();
    let r = append_files(&base, &[bad.clone(), ok.clone()], None, &mut |_| {}, &AtomicBool::new(false));
    assert_eq!(r.refused.len(), 1);
    assert_eq!(r.refused[0].0, bad);
    assert!(r.refused[0].1.contains("48000") && r.refused[0].1.contains("44100"), "{}", r.refused[0].1);
    assert_eq!(r.edit.as_ref().unwrap().state.len_frames(), 300);
    assert!(base.store().bytes_on_disk() < before + 100_000 * 4, "the refused file wrote nothing");
    // Only a refused file: no edit at all.
    let only = append_files(&base, &[bad], None, &mut |_| {}, &AtomicBool::new(false));
    assert!(only.edit.is_none() && only.added.is_empty());
}

#[test]
fn channels_are_duplicated_or_mixed_as_paste_does() {
    let t = TempDir::new("channels");
    let (st, mo) = (t.0.join("st.wav"), t.0.join("mo.wav"));
    write_wav(&st, 48_000, 2, 100, 1);
    write_wav(&mo, 48_000, 1, 100, 2);
    // Mono onto stereo: both channels get it.
    let stereo = open(&t.0, &st);
    let d = append_files(&stereo, std::slice::from_ref(&mo), None, &mut |_| {}, &AtomicBool::new(false)).edit.unwrap().state;
    assert_eq!(d.channel_count(), 2);
    assert_eq!(read_all(&d, 0)[100], f(2, 0, 0));
    assert_eq!(read_all(&d, 1)[100], f(2, 0, 0));
    // Stereo onto mono: the mean.
    let mono = open(&t.0, &mo);
    let d = append_files(&mono, std::slice::from_ref(&st), None, &mut |_| {}, &AtomicBool::new(false)).edit.unwrap().state;
    assert_eq!(d.channel_count(), 1);
    let want = (f(1, 0, 5) + f(1, 1, 5)) / 2.0;
    assert!((read_all(&d, 0)[105] - want).abs() < 1e-6);
    // Six channels onto stereo do not map.
    let six = t.0.join("six.wav");
    write_wav(&six, 48_000, 6, 10, 4);
    let r = append_files(&stereo, &[six], None, &mut |_| {}, &AtomicBool::new(false));
    assert!(r.edit.is_none() && r.refused.len() == 1, "{:?}", r.refused);
}

#[test]
fn a_missing_or_broken_file_is_reported_and_the_others_still_go_in() {
    let t = TempDir::new("broken");
    let (a, junk, b) = (t.0.join("a.wav"), t.0.join("junk.wav"), t.0.join("b.wav"));
    write_wav(&a, 48_000, 1, 100, 1);
    std::fs::write(&junk, b"not audio at all").unwrap();
    write_wav(&b, 48_000, 1, 50, 2);
    let base = open(&t.0, &a);
    let r = append_files(&base, &[t.0.join("missing.wav"), junk, b], None, &mut |_| {}, &AtomicBool::new(false));
    assert_eq!(r.refused.len(), 2);
    assert_eq!(r.edit.unwrap().state.len_frames(), 150);
}

#[test]
fn a_new_document_holds_the_files_back_to_back() {
    let t = TempDir::new("new");
    let (a, bad, b, c) = (t.0.join("a.wav"), t.0.join("bad.wav"), t.0.join("b.wav"), t.0.join("c.wav"));
    write_wav(&bad, 22_050, 2, 100, 9);
    write_wav(&a, 44_100, 2, 300, 1);
    write_wav(&b, 44_100, 1, 200, 2);
    write_wav(&c, 44_100, 2, 100, 3);
    let sessions = t.0.join("s");
    let r = new_from_files(&opts(&sessions), &[a.clone(), bad.clone(), b, c], &mut |_| {}, &AtomicBool::new(false));
    assert_eq!(r.added.len(), 3);
    assert_eq!(r.refused.len(), 1, "the rate that differs from the first file's");
    let doc = r.state.unwrap();
    assert_eq!((doc.len_frames(), doc.channel_count(), doc.sample_rate()), (600, 2, 44_100));
    assert_eq!(read_all(&doc, 1)[300], f(2, 0, 0));
    assert_eq!(read_all(&doc, 1)[599], f(3, 1, 99));
    // A first file that fails does not stop the next one from starting the document.
    let r = new_from_files(&opts(&sessions), &[t.0.join("missing.wav"), a], &mut |_| {}, &AtomicBool::new(false));
    assert_eq!(r.state.unwrap().len_frames(), 300);
    let none = new_from_files(&opts(&sessions), &[bad], &mut |_| {}, &AtomicBool::new(true));
    assert!(none.state.is_none());
}

#[test]
fn open_into_checks_the_rate_and_cancel() {
    let t = TempDir::new("into");
    let a = t.0.join("a.wav");
    write_wav(&a, 44_100, 1, 100, 1);
    let store = Store::create_session(Some(&t.0.join("s")), StoreOptions::default()).unwrap();
    let r = io::open_into(&a, &store, 48_000, None, &mut |_| {}, &AtomicBool::new(false));
    assert_eq!(r.unwrap_err(), OpenError::RateMismatch { file: 44_100, expected: 48_000 });
    let r = io::open_into(&a, &store, 44_100, None, &mut |_| {}, &AtomicBool::new(true));
    assert_eq!(r.unwrap_err(), OpenError::Cancelled);
    let ok = io::open_into(&a, &store, 44_100, None, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(ok.store().same_store(&store));
}
