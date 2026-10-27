//! Edit while playing (f48, A208): where the playhead goes when the document changes under it.
//! `EditSpan::between` finds what an edit replaced by comparing the pieces of two states;
//! `map_position` moves a play position across it.

use std::path::PathBuf;

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::doc::span::EditSpan;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;
const NONE: Crossfades = Crossfades::NONE;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-span-test-{}-{name}", std::process::id()));
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

/// `channels` channels of `frames` frames, in blocks of 1000 frames.
fn doc(dir: &TempDir, channels: usize, frames: usize) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions { pack_bytes: 1 << 24, cache_bytes: 1 << 24, ..StoreOptions::default() }).unwrap();
    let data: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
    let chans: Vec<Channel> =
        (0..channels).map(|_| Channel::from_pieces(data.chunks(1000).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, channels as u16)).unwrap()
}

#[test]
fn a_delete_before_the_playhead_shifts_it_back_by_the_deleted_length() {
    let d = TempDir::new("del-before");
    let a = doc(&d, 2, 10_000);
    let b = edit::delete(&a, 2_000..3_500, ChannelMask::ALL, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!((span.at, span.removed, span.inserted), (2_000, 1_500, 0));
    assert_eq!(span.map_position(6_000.0), 4_500.0);
    assert_eq!(span.map_position(6_000.5), 4_500.5, "the fraction survives");
    assert_eq!(span.map_position(1_999.0), 1_999.0, "before the cut: unchanged");
    assert_eq!(span.map_position(3_500.0), 2_000.0, "the end of the cut is the cut point");
    assert_eq!(span.map_position(10_000.0), 8_500.0, "the end of the file moves with it");
}

#[test]
fn a_playhead_inside_the_deleted_range_jumps_to_the_cut_point() {
    let d = TempDir::new("del-inside");
    let a = doc(&d, 1, 10_000);
    let b = edit::delete(&a, 2_300..4_700, ChannelMask::ALL, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!((span.at, span.removed, span.inserted), (2_300, 2_400, 0));
    for p in [2_300.5, 3_000.0, 4_699.9] {
        assert_eq!(span.map_position(p), 2_300.0, "{p}");
    }
}

#[test]
fn an_insert_before_the_playhead_pushes_it_forward() {
    let d = TempDir::new("insert");
    let a = doc(&d, 2, 10_000);
    let b = edit::insert_silence(&a, 1_500, 700, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!((span.at, span.removed, span.inserted), (1_500, 0, 700));
    assert_eq!(span.map_position(5_000.0), 5_700.0);
    assert_eq!(span.map_position(1_500.0), 1_500.0, "at the insert point: stays before it");
    assert_eq!(span.map_position(900.0), 900.0);
}

#[test]
fn same_length_edits_and_one_channel_edits_do_not_move_the_playhead() {
    let d = TempDir::new("same");
    let a = doc(&d, 2, 10_000);
    let b = edit::silence(&a, 3_000..4_000, ChannelMask::ALL, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!(span.removed, span.inserted);
    for p in [0.0, 2_999.0, 3_500.25, 4_000.0, 9_999.0] {
        assert_eq!(span.map_position(p), p, "{p}");
    }
    let c = edit::silence(&a, 3_000..4_000, ChannelMask::only(1), NONE).unwrap().state;
    let span = EditSpan::between(&a, &c);
    assert_eq!((span.at, span.removed, span.inserted), (3_000, 1_000, 1_000));
    assert_eq!(span.map_position(8_000.0), 8_000.0);
}

#[test]
fn nothing_changed_is_the_identity() {
    let d = TempDir::new("none");
    let a = doc(&d, 2, 5_000);
    let span = EditSpan::between(&a, &a.clone());
    assert_eq!((span.removed, span.inserted), (0, 0));
    assert_eq!(span.map_position(1234.5), 1234.5);
}

#[test]
fn crop_replaces_the_whole_file_so_the_playhead_keeps_its_offset_clamped() {
    // Known limit: a crop is seen as "all replaced by the kept part", not "head and tail cut",
    // so the playhead keeps its offset from the start (clamped to the new length).
    let d = TempDir::new("crop");
    let a = doc(&d, 1, 10_000);
    let b = edit::crop(&a, 2_000..6_000, ChannelMask::ALL, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!((span.at, span.removed, span.inserted), (0, 10_000, 4_000));
    assert_eq!(span.map_position(3_000.0), 3_000.0);
    assert_eq!(span.map_position(9_000.0), 4_000.0, "past the new end: the end");
}

#[test]
fn undo_of_a_delete_is_an_insert_and_the_mapping_round_trips() {
    let d = TempDir::new("undo");
    let a = doc(&d, 2, 10_000);
    let b = edit::delete(&a, 2_000..3_000, ChannelMask::ALL, NONE).unwrap().state;
    let fwd = EditSpan::between(&a, &b);
    let back = EditSpan::between(&b, &a);
    assert_eq!((back.at, back.removed, back.inserted), (2_000, 0, 1_000));
    assert_eq!(back.map_position(fwd.map_position(7_000.0)), 7_000.0);
}

#[test]
fn a_replaced_range_clamps_into_the_new_audio() {
    let d = TempDir::new("paste");
    let a = doc(&d, 1, 10_000);
    let clip = edit::copy(&a, 0..300, ChannelMask::ALL).unwrap();
    let b = edit::paste(&a, PasteAt::Replace(4_000..4_800), &clip, ChannelMask::ALL, NONE).unwrap().state;
    let span = EditSpan::between(&a, &b);
    assert_eq!((span.at, span.removed, span.inserted), (4_000, 800, 300));
    assert_eq!(span.map_position(4_100.0), 4_100.0);
    assert_eq!(span.map_position(4_700.0), 4_300.0, "past the new audio: its end");
    assert_eq!(span.map_position(6_000.0), 5_500.0);
}

#[test]
fn bad_positions_never_panic() {
    let span = EditSpan { at: 10, removed: 5, inserted: 0 };
    assert_eq!(span.map_position(f64::NAN), 0.0);
    assert_eq!(span.map_position(-4.0), 0.0);
    assert!(span.map_position(f64::INFINITY).is_infinite());
}
