//! f30: new documents (empty, and from a clip), and the actions that make them.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use soundcraft_audio_io::SampleFormat;
use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::{self, ChannelMask};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::newdoc::{self, NewDocError, NewDocOptions};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-core-newdoc-{}-{name}", std::process::id()));
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

fn sample(c: usize, i: u64) -> f32 {
    ((i * 31 + c as u64 * 7_919) % 20_011) as f32 / 10_005.5 - 1.0
}

/// A document of `frames` frames, `channels` channels, in a store under `root`.
fn make(root: &std::path::Path, frames: u64, channels: usize, fmt: SampleFormat) -> DocState {
    let store = Store::create_session(Some(root), StoreOptions::default()).unwrap();
    let mut chans = Vec::new();
    for c in 0..channels {
        let mut ch = Channel::new();
        let mut at = 0;
        while at < frames {
            let m = (frames - at).min(BLOCK_FRAMES as u64);
            let v: Vec<f32> = (at..at + m).map(|i| sample(c, i)).collect();
            ch.push(Piece::whole(store.append(&v).unwrap()).unwrap());
            at += m;
        }
        chans.push(ch);
    }
    let mut src = SourceFormat::synthetic(44_100, channels as u16);
    src.sample_format = fmt;
    DocState::new(store, chans, 44_100, src).unwrap()
}

fn opts(root: &std::path::Path) -> NewDocOptions {
    NewDocOptions { sessions_root: Some(root.to_path_buf()), ..NewDocOptions::default() }
}

fn read_all(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut out = vec![0.0; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut out).unwrap(), out.len());
    out
}

#[test]
fn an_empty_document_has_the_chosen_format_and_no_audio() {
    let t = TempDir::new("empty");
    let d = newdoc::empty(&opts(&t.0), 48_000, 2, SampleFormat::Float32).unwrap();
    assert_eq!((d.len_frames(), d.channel_count(), d.sample_rate()), (0, 2, 48_000));
    assert_eq!(d.source().sample_format, SampleFormat::Float32);
    assert!(d.source().path.as_os_str().is_empty(), "unsaved: no path");
    let d16 = newdoc::empty(&opts(&t.0), 22_050, 1, SampleFormat::Int16).unwrap();
    assert_eq!((d16.channel_count(), d16.source().sample_format), (1, SampleFormat::Int16));
}

#[test]
fn an_empty_document_refuses_nonsense() {
    let t = TempDir::new("nonsense");
    for (rate, ch, fmt) in [
        (0, 2, SampleFormat::Float32),
        (9_000_000, 2, SampleFormat::Float32),
        (48_000, 0, SampleFormat::Float32),
        (48_000, 99, SampleFormat::Float32),
        (48_000, 2, SampleFormat::Int8),
    ] {
        assert!(matches!(newdoc::empty(&opts(&t.0), rate, ch, fmt), Err(NewDocError::Invalid(_))), "{rate} {ch} {fmt:?}");
    }
}

#[test]
fn a_new_document_from_a_clip_has_the_same_audio_in_its_own_store() {
    let t = TempDir::new("clip");
    // Three and a bit blocks, and a range that starts and ends inside blocks.
    let src = make(&t.0, 3 * BLOCK_FRAMES as u64 + 1_000, 2, SampleFormat::Int24);
    let range = 40_000..(2 * BLOCK_FRAMES as u64 + 500);
    let clip = edit::copy(&src, range.clone(), ChannelMask::ALL).unwrap();
    let mut last = 0.0;
    let d = newdoc::from_clip(&opts(&t.0), &clip, &mut |p| last = p, &AtomicBool::new(false)).unwrap();
    assert!((last - 1.0).abs() < 1e-9);
    assert_eq!((d.len_frames(), d.channel_count(), d.sample_rate()), (range.end - range.start, 2, 44_100));
    assert_eq!(d.source().sample_format, SampleFormat::Int24, "inherits the sample type");
    assert!(!d.store().same_store(src.store()), "its own store");
    for c in 0..2 {
        let want: Vec<f32> = range.clone().map(|i| sample(c, i)).collect();
        assert_eq!(read_all(&d, c), want);
    }
    // The source is still intact and independent: dropping it does not hurt the copy.
    drop(src);
    drop(clip);
    assert_eq!(read_all(&d, 1)[0], sample(1, 40_000));
}

#[test]
fn a_one_channel_clip_makes_a_mono_document() {
    let t = TempDir::new("mono");
    let src = make(&t.0, 5_000, 2, SampleFormat::Float32);
    let clip = edit::copy(&src, 0..5_000, ChannelMask::only(1)).unwrap();
    let d = newdoc::from_clip(&opts(&t.0), &clip, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(d.channel_count(), 1);
    assert_eq!(read_all(&d, 0)[123], sample(1, 123));
}

#[test]
fn cancel_and_a_full_disk_leave_nothing_behind() {
    let t = TempDir::new("fail");
    let src = make(&t.0, 10_000, 1, SampleFormat::Float32);
    let clip = edit::copy(&src, 0..10_000, ChannelMask::ALL).unwrap();
    let sessions = t.0.join("new");
    let o = opts(&sessions);
    assert!(matches!(newdoc::from_clip(&o, &clip, &mut |_| {}, &AtomicBool::new(true)), Err(NewDocError::Cancelled)));
    let tiny = NewDocOptions { free_space: Some(|_| Some(10)), ..o };
    assert!(matches!(newdoc::from_clip(&tiny, &clip, &mut |_| {}, &AtomicBool::new(false)), Err(NewDocError::NoSpace { .. })));
    let left = std::fs::read_dir(&sessions).unwrap().count();
    assert_eq!(left, 0, "the half-made session folders are removed");
}

#[test]
fn the_new_actions_are_in_the_table_and_follow_the_clipboard() {
    for (name, id) in [
        ("edit.copy_to_new", ActionId::CopyToNew),
        ("edit.paste_to_new", ActionId::PasteToNew),
        ("file.close_all", ActionId::CloseAll),
        ("file.save_all", ActionId::SaveAll),
    ] {
        assert_eq!(by_name(name).map(|a| a.id), Some(id));
    }
    let t = TempDir::new("actions");
    let src = make(&t.0, 1_000, 1, SampleFormat::Float32);
    let mut e = Editor::new(History::new(src, 1 << 30));
    assert!(e.enabled(ActionId::CopyToNew, &(0..0)));
    assert!(!e.enabled(ActionId::PasteToNew, &(0..0)), "nothing on the clipboard");
    e.run(ActionId::Copy, 0..100).unwrap();
    assert!(e.enabled(ActionId::PasteToNew, &(0..0)));
    assert_eq!(e.clipboard.as_ref().unwrap().len_frames(), 100);
}
