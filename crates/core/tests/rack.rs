//! The Effects Rack applied to a document (f31): one undo step, labelled, range and channels.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-rack-test-{}-{name}", std::process::id()));
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
use wavcraft_core::rack::RackConfig;

fn chan(e: &Editor, ch: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn two_slot() -> RackConfig {
    let mut r = RackConfig::new();
    r.set_slot(0, "invert", None).unwrap();
    r.set_slot(1, "gain", Some(vec![-6.0206])).unwrap();
    r
}

#[test]
fn apply_is_one_step_named_effects_rack() {
    let tmp = TempDir::new("one");
    let mut e = editor(&tmp.0, &[vec![0.5; 200_000]]);
    let steps = e.history.steps().len();
    let out = e.run_rack(&two_slot(), 0..0).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.steps().len(), steps + 1, "a single history step");
    assert_eq!(e.history.undo_label(), Some("Effects Rack"));
    assert!(chan(&e, 0).iter().all(|v| (v + 0.25).abs() < 1e-4), "inverted then halved, over the whole file");
    e.run(ActionId::Undo, 0..0).unwrap();
    assert!(chan(&e, 0).iter().all(|v| *v == 0.5));
}

#[test]
fn apply_touches_only_the_selection_and_the_active_channels() {
    let tmp = TempDir::new("sel");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000], vec![-0.5; 1000]]);
    e.run_rack(&two_slot(), 100..300).unwrap();
    let (l, r) = (chan(&e, 0), chan(&e, 1));
    assert_eq!((l[50], l[500]), (0.5, 0.5));
    assert!((l[200] + 0.25).abs() < 1e-4 && (r[200] - 0.25).abs() < 1e-4, "{} {}", l[200], r[200]);
}

#[test]
fn an_inactive_rack_adds_no_step() {
    let tmp = TempDir::new("off");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let mut r = two_slot();
    r.bypass();
    let steps = e.history.steps().len();
    let out = e.run_rack(&r, 0..0).unwrap();
    assert!(!out.doc_changed && e.history.steps().len() == steps);
}
