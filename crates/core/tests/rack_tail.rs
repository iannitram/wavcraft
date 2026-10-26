//! Effects Rack "Apply with tail" (B019): the rack rings out past the selection end.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::{Change, ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::rack::{RackConfig, apply_with_tail};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-rack-tail-test-{}-{name}", std::process::id()));
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

fn chan(e: &Editor, ch: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

/// Invert, then gain -6.0206 dB: 0.5 becomes -0.25.
fn two_slot() -> RackConfig {
    let mut r = RackConfig::new();
    r.set_slot(0, "invert", None).unwrap();
    r.set_slot(1, "gain", Some(vec![-6.0206])).unwrap();
    r
}

#[test]
fn the_tail_is_inserted_after_the_selection() {
    let tmp = TempDir::new("grow");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let out = e.run_rack_with_tail(&two_slot(), 100..300, 500).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 100..800, "the processed range plus the tail");
    let x = chan(&e, 0);
    assert_eq!(x.len(), 1500);
    assert_eq!(x[50], 0.5);
    assert!((x[200] + 0.25).abs() < 1e-4, "{}", x[200]);
    assert!(x[300..800].iter().all(|v| v.abs() < 1e-6), "the tail is what the rack makes of silence");
    assert_eq!((x[800], x[1499]), (0.5, 0.5), "the rest of the file moved later, unchanged");
}

#[test]
fn it_is_one_undo_step_and_undo_restores_the_length() {
    let tmp = TempDir::new("undo");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let steps = e.history.steps().len();
    e.run_rack_with_tail(&two_slot(), 100..300, 500).unwrap();
    assert_eq!(e.history.steps().len(), steps + 1);
    assert_eq!(e.history.undo_label(), Some("Effects Rack"));
    assert_eq!(chan(&e, 0).len(), 1500, "the one step added the tail");
    e.run(ActionId::Undo, 0..0).unwrap();
    let x = chan(&e, 0);
    assert_eq!(x.len(), 1000);
    assert!(x.iter().all(|v| *v == 0.5));
}

#[test]
fn an_empty_selection_means_the_whole_file_and_the_tail_goes_at_the_end() {
    let tmp = TempDir::new("whole");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let out = e.run_rack_with_tail(&two_slot(), 0..0, 200).unwrap();
    assert_eq!(out.selection, 0..1200, "an empty selection is the whole file, plus the tail");
    let x = chan(&e, 0);
    assert_eq!(x.len(), 1200);
    assert!((x[999] + 0.25).abs() < 1e-4 && x[1000..].iter().all(|v| v.abs() < 1e-6));
}

#[test]
fn a_zero_tail_is_a_plain_apply() {
    let tmp = TempDir::new("zero");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    e.run_rack_with_tail(&two_slot(), 100..300, 0).unwrap();
    let x = chan(&e, 0);
    assert_eq!(x.len(), 1000);
    assert!((x[200] + 0.25).abs() < 1e-4 && x[400] == 0.5);
}

#[test]
fn every_channel_grows_and_an_unselected_channel_gets_a_silent_tail() {
    let tmp = TempDir::new("stereo");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000], vec![-0.5; 1000]]);
    e.channels = ChannelMask::only(0);
    e.run_rack_with_tail(&two_slot(), 0..400, 200).unwrap();
    let (l, r) = (chan(&e, 0), chan(&e, 1));
    assert_eq!((l.len(), r.len()), (1200, 1200));
    assert!((l[100] + 0.25).abs() < 1e-4, "{}", l[100]);
    assert_eq!(r[100], -0.5, "channel 1 is not selected");
    assert!(l[400..600].iter().all(|v| v.abs() < 1e-6) && r[400..600].iter().all(|v| *v == 0.0));
    assert_eq!((l[700], r[700]), (0.5, -0.5));
}

#[test]
fn an_inactive_rack_adds_no_tail_and_no_step() {
    let tmp = TempDir::new("off");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let mut r = two_slot();
    r.bypass();
    let steps = e.history.steps().len();
    let out = e.run_rack_with_tail(&r, 100..300, 500).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(e.history.steps().len(), steps);
    assert_eq!(chan(&e, 0).len(), 1000);
}

#[test]
fn the_edit_reports_a_length_change_from_the_range_start() {
    let tmp = TempDir::new("change");
    let e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let cancel = AtomicBool::new(false);
    let edit = apply_with_tail(&two_slot(), e.history.current(), 100..300, ChannelMask::only(0), Crossfades::NONE, &cancel, 500).unwrap();
    assert_eq!(edit.change, Change::LengthFrom { frame: 100 });
    assert_eq!(edit.label, "Effects Rack");
    assert_eq!(edit.state.len_frames(), 1500);
}

#[test]
fn no_active_channel_changes_nothing() {
    let tmp = TempDir::new("nochan");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    e.channels = ChannelMask::NONE;
    let steps = e.history.steps().len();
    let out = e.run_rack_with_tail(&two_slot(), 100..300, 500).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(e.history.steps().len(), steps);
    assert_eq!(chan(&e, 0).len(), 1000);
}
