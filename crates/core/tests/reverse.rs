//! Invert and Reverse (E4, #30): bit-exact polarity inversion and time reversal over a
//! selection or the whole file, across 64 K-frame block boundaries, per active channel,
//! with undo and Repeat Last Command.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-reverse-test-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn channel(e: &Editor, c: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
    v
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn reverse_id() -> ActionId {
    by_name("effect.reverse").expect("reverse is in the action table").id
}

fn invert_id() -> ActionId {
    by_name("effect.invert").expect("invert is in the action table").id
}

#[test]
fn reverse_whole_file_when_nothing_is_selected_is_bit_exact() {
    let tmp = TempDir::new("whole");
    let left = vec![0.0_f32, -0.0_f32, 0.25_f32, -0.5_f32, 0.75_f32, -1.0_f32];
    let right = vec![1.0_f32, 2.0_f32, 3.0_f32, 4.0_f32, 5.0_f32, 6.0_f32];
    let mut e = editor(&tmp.0, &[left, right]);
    let out = e.run(reverse_id(), 3..3).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("Reverse"));
    assert_eq!(e.label(ActionId::Undo), "Undo Reverse");
    assert_eq!(bits(&channel(&e, 0)), bits(&[-1.0_f32, 0.75_f32, -0.5_f32, 0.25_f32, -0.0_f32, 0.0_f32]));
    assert_eq!(bits(&channel(&e, 1)), bits(&[6.0_f32, 5.0_f32, 4.0_f32, 3.0_f32, 2.0_f32, 1.0_f32]));
}

#[test]
fn reverse_selection_across_block_boundaries_leaves_outside_untouched() {
    let tmp = TempDir::new("across-blocks");
    let frames = 2 * BLOCK_FRAMES + 9_000;
    let left: Vec<f32> = (0..frames).map(|k| (k + 1) as f32).collect();
    let right: Vec<f32> = (0..frames).map(|k| -((k + 1) as f32)).collect();
    let mut e = editor(&tmp.0, &[left.clone(), right.clone()]);
    let out = e.run(reverse_id(), 1_000..140_000).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 1_000..140_000);
    let got_l = channel(&e, 0);
    let got_r = channel(&e, 1);
    assert_eq!(bits(&got_l[..1_000]), bits(&left[..1_000]));
    assert_eq!(bits(&got_r[..1_000]), bits(&right[..1_000]));
    assert_eq!(bits(&got_l[140_000..]), bits(&left[140_000..]));
    assert_eq!(bits(&got_r[140_000..]), bits(&right[140_000..]));
    for k in [0usize, 1, 65_535, 65_536, 100_000, 131_071, 131_072, 138_999] {
        assert_eq!(got_l[1_000 + k].to_bits(), left[139_999 - k].to_bits(), "left offset {k}");
        assert_eq!(got_r[1_000 + k].to_bits(), right[139_999 - k].to_bits(), "right offset {k}");
    }
    e.run(ActionId::Undo, 1_000..140_000).unwrap();
    assert_eq!(bits(&channel(&e, 0)), bits(&left));
    assert_eq!(bits(&channel(&e, 1)), bits(&right));
}

#[test]
fn reverse_twice_restores_every_bit() {
    let tmp = TempDir::new("twice");
    let frames = 2 * BLOCK_FRAMES + 17;
    let mut left: Vec<f32> = (0..frames).map(|k| (k as f32 + 1.0) * 0.25).collect();
    left[100] = -0.0_f32;
    left[90_000] = 0.0_f32;
    let right: Vec<f32> = (0..frames).map(|k| -(k as f32 + 1.0) * 0.5).collect();
    let mut e = editor(&tmp.0, &[left.clone(), right.clone()]);
    let sel = 10..frames as u64 - 10;
    e.run(reverse_id(), sel.clone()).unwrap();
    e.run(reverse_id(), sel).unwrap();
    assert_eq!(bits(&channel(&e, 0)), bits(&left));
    assert_eq!(bits(&channel(&e, 1)), bits(&right));
}

#[test]
fn reverse_respects_the_active_channel_mask() {
    let tmp = TempDir::new("mask");
    let frames = BLOCK_FRAMES + 50;
    let left: Vec<f32> = (0..frames).map(|k| (k + 1) as f32).collect();
    let right: Vec<f32> = (0..frames).map(|k| -((k + 1) as f32)).collect();
    let mut e = editor(&tmp.0, &[left.clone(), right.clone()]);
    e.channels = ChannelMask::only(1);
    e.run(reverse_id(), 0..0).unwrap();
    let want_r: Vec<f32> = right.iter().copied().rev().collect();
    assert_eq!(bits(&channel(&e, 0)), bits(&left), "disabled left channel untouched");
    assert_eq!(bits(&channel(&e, 1)), bits(&want_r), "active right channel reversed");
}

#[test]
fn reverse_odd_and_single_frame_selections() {
    let tmp = TempDir::new("odd-single");
    let input = vec![10.0_f32, 20.0_f32, 30.0_f32, 40.0_f32, 50.0_f32, 60.0_f32, 70.0_f32];
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    e.run(reverse_id(), 1..6).unwrap();
    let after_odd = [10.0_f32, 60.0_f32, 50.0_f32, 40.0_f32, 30.0_f32, 20.0_f32, 70.0_f32];
    assert_eq!(bits(&channel(&e, 0)), bits(&after_odd));

    e.run(reverse_id(), 2..3).unwrap();
    assert_eq!(bits(&channel(&e, 0)), bits(&after_odd), "reversing one frame changes no sample");
}

#[test]
fn reverse_is_repeatable_with_repeat_last_command() {
    let tmp = TempDir::new("repeat");
    let input = vec![1.0_f32, 2.0_f32, 3.0_f32, 4.0_f32, 5.0_f32, 6.0_f32];
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    e.run(reverse_id(), 0..4).unwrap();
    assert_eq!(bits(&channel(&e, 0)), bits(&[4.0_f32, 3.0_f32, 2.0_f32, 1.0_f32, 5.0_f32, 6.0_f32]));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Reverse");
    e.run(ActionId::RepeatLast, 2..6).unwrap();
    assert_eq!(bits(&channel(&e, 0)), bits(&[4.0_f32, 3.0_f32, 6.0_f32, 5.0_f32, 1.0_f32, 2.0_f32]));
}

#[test]
fn reverse_and_invert_are_bit_exact_and_commute() {
    let input = vec![0.25_f32, -0.5_f32, 0.0_f32, -0.0_f32, 1.25_f32];
    let want = [-1.25_f32, 0.0_f32, -0.0_f32, 0.5_f32, -0.25_f32];

    let tmp1 = TempDir::new("inv-then-rev");
    let mut e1 = editor(&tmp1.0, std::slice::from_ref(&input));
    e1.run(invert_id(), 0..0).unwrap();
    e1.run(reverse_id(), 0..0).unwrap();
    assert_eq!(bits(&channel(&e1, 0)), bits(&want));

    let tmp2 = TempDir::new("rev-then-inv");
    let mut e2 = editor(&tmp2.0, std::slice::from_ref(&input));
    e2.run(reverse_id(), 0..0).unwrap();
    e2.run(invert_id(), 0..0).unwrap();
    assert_eq!(bits(&channel(&e2, 0)), bits(&want));
}
