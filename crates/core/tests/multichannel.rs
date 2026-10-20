//! A134: surround and many-channel files. Open and save keep the channel count, the samples and
//! the speaker mask; an edit on some channels leaves the others alone.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::channels;
use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenError, OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-mc-test-{}-{name}", std::process::id()));
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

/// Channel `c` of `n`: values exactly representable in 16 bits, different in every channel.
fn data(n: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..n).map(|c| (0..frames).map(|k| (((k * 31 + c * 977) % 65_536) as f32 - 32_768.0) / 32_768.0).collect()).collect()
}

fn doc_from(dir: &Path, data: &[Vec<f32>], mask: Option<u32>) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let mut src = SourceFormat::synthetic(RATE, data.len() as u16);
    src.channel_mask = mask;
    DocState::new(s.clone(), chans, RATE, src).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<f32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn a_five_one_file_round_trips_sample_exact_with_its_mask() {
    let t = TempDir::new("six");
    let d = data(6, 2 * BLOCK_FRAMES + 321);
    let doc = doc_from(&t.0.join("store"), &d, Some(0x3F));
    let path = t.0.join("six.wav");
    save_wav(&doc, &path, SampleType::Int16, &mut |_| {}).unwrap();
    let back = reopen(&path, &t.0.join("sessions"));
    assert_eq!(back.channel_count(), 6);
    assert_eq!(back.source().channel_mask, Some(0x3F));
    assert_eq!(channels::labels(back.source().channel_mask, 6), ["FL", "FR", "FC", "LFE", "BL", "BR"]);
    assert_eq!(content(&back), d, "sample for sample");
    // A second generation keeps it too.
    let again = t.0.join("again.wav");
    save_wav(&back, &again, SampleType::Int16, &mut |_| {}).unwrap();
    let back2 = reopen(&again, &t.0.join("sessions2"));
    assert_eq!((content(&back2), back2.source().channel_mask), (d, Some(0x3F)));
}

#[test]
fn the_extensible_header_is_the_standard_one() {
    let t = TempDir::new("hdr");
    let doc = doc_from(&t.0.join("store"), &data(6, 100), Some(0x3F));
    let path = t.0.join("h.wav");
    save_wav(&doc, &path, SampleType::Int24, &mut |_| {}).unwrap();
    let b = std::fs::read(&path).unwrap();
    assert_eq!(&b[12..16], b"fmt ");
    let u16_at = |p: usize| u16::from_le_bytes([b[p], b[p + 1]]);
    let u32_at = |p: usize| u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
    assert_eq!((u32_at(16), u16_at(20), u16_at(22)), (40, 0xFFFE, 6));
    assert_eq!((u16_at(32), u16_at(34), u16_at(36), u16_at(38)), (6 * 3, 24, 22, 24));
    assert_eq!((u32_at(40), u16_at(44)), (0x3F, 1), "mask, then the PCM sub-format");
    assert_eq!(&b[60..64], b"data");
}

#[test]
fn thirty_two_unlabelled_channels_round_trip_and_float_too() {
    let t = TempDir::new("thirty-two");
    let d = data(32, BLOCK_FRAMES + 99);
    let doc = doc_from(&t.0.join("store"), &d, None);
    for (ty, name) in [(SampleType::Int24, "a.wav"), (SampleType::Float32, "b.wav")] {
        let path = t.0.join(name);
        save_wav(&doc, &path, ty, &mut |_| {}).unwrap();
        let back = reopen(&path, &t.0.join(format!("s-{name}")));
        assert_eq!(back.channel_count(), 32, "{ty:?}");
        assert_eq!(back.source().channel_mask, None, "{ty:?}: unlabelled stays unlabelled");
        assert_eq!(channels::labels(None, 32)[31], "32");
        assert_eq!(content(&back), d, "{ty:?}");
    }
}

#[test]
fn an_edit_on_three_of_thirty_two_channels_leaves_the_others_identical() {
    let t = TempDir::new("edit");
    let d = data(32, 2 * BLOCK_FRAMES + 10);
    let doc = doc_from(&t.0.join("store"), &d, None);
    let mask = ChannelMask::only(2).with(17).with(30);
    let range = (BLOCK_FRAMES as u64 - 500)..(BLOCK_FRAMES as u64 + 700);
    let edited = edit::silence(&doc, range.clone(), mask, Crossfades::NONE).unwrap();
    assert!(!edited.is_noop());
    let path = t.0.join("edited.wav");
    save_wav(&edited.state, &path, SampleType::Int16, &mut |_| {}).unwrap();
    let back = content(&reopen(&path, &t.0.join("sessions")));
    for (c, (got, want)) in back.iter().zip(&d).enumerate() {
        if [2, 17, 30].contains(&c) {
            assert!(got[range.start as usize..range.end as usize].iter().all(|v| *v == 0.0), "channel {c} silenced");
            assert_eq!(got[..range.start as usize], want[..range.start as usize], "channel {c} before");
            assert_eq!(got[range.end as usize..], want[range.end as usize..], "channel {c} after");
        } else {
            assert_eq!(got, want, "channel {c} untouched");
        }
    }
    // A delete on some channels keeps the length and the other channels too.
    let del = edit::delete(&doc, 100..400, mask, Crossfades::NONE).unwrap();
    assert_eq!(del.state.len_frames(), doc.len_frames());
    assert_eq!(del.state.source().channel_mask, None);
}

#[test]
fn more_than_thirty_two_channels_are_refused_by_name() {
    let t = TempDir::new("many");
    let doc = doc_from(&t.0.join("store"), &data(33, 10), None);
    let path = t.0.join("many.wav");
    save_wav(&doc, &path, SampleType::Int16, &mut |_| {}).unwrap();
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    match open(&path, &opts, &mut |_| {}, &AtomicBool::new(false)) {
        Err(OpenError::Decode(m)) => assert!(m.contains("33 channels") && m.contains("32"), "{m}"),
        other => unreachable!("expected a refusal, got {:?}", other.map(|d| d.channel_count())),
    }
}

#[test]
fn a_mask_that_disagrees_with_the_channel_count_gives_numbers_and_a_warning() {
    let t = TempDir::new("badmask");
    let doc = doc_from(&t.0.join("store"), &data(6, 50), Some(0x3F));
    let path = t.0.join("bad.wav");
    save_wav(&doc, &path, SampleType::Int16, &mut |_| {}).unwrap();
    // Patch dwChannelMask to name 5 speakers.
    let mut b = std::fs::read(&path).unwrap();
    b[40..44].copy_from_slice(&0x1Fu32.to_le_bytes());
    std::fs::write(&path, b).unwrap();
    let back = reopen(&path, &t.0.join("sessions"));
    assert_eq!(back.source().channel_mask, None);
    assert!(back.source().warnings.iter().any(|w| w.contains("channel mask")), "{:?}", back.source().warnings);
    assert_eq!(content(&back), data(6, 50));
}
