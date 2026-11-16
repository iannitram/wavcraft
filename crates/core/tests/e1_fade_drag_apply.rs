//! Core checks for v1 E1.2 (issue #27): a fade handle drag, turned into a range by `drag_fade`,
//! applies the fade as one undo step.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::dsp::fade::FadeCurve;
use wavcraft_core::effects;
use wavcraft_core::fade_handles::{FadeGesture, FadeHandle, drag_fade};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::view::Viewport;

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-e1-fade-drag-test-{}-{name}", std::process::id()));
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
    std::fs::create_dir_all(dir).unwrap();
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn read_ch(e: &Editor, ch: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn effect_idx(id: &str) -> u16 {
    effects::all().iter().position(|e| e.id() == id).unwrap() as u16
}

#[test]
fn e1_drag_fade_in_to_frame_24001_fades_linearly_as_one_undo_step() {
    let dir = TempDir::new("in");
    let mut e = editor(&dir.0.join("s"), &[vec![1.0f32; 48_000]]);
    let view = Viewport { start: 0.0, frames_per_px: 1.0 };
    let g = drag_fade(FadeHandle::In, &view, 0..48_000, 24_001.0, 0.0, false).unwrap();
    assert_eq!(g, FadeGesture { handle: FadeHandle::In, range: 0..24_001, curve: FadeCurve::Linear });
    e.run_effect(effect_idx("fade_in"), vec![0.0], g.range.clone()).unwrap();
    let s = read_ch(&e, 0);
    assert_eq!(s[0], 0.0);
    assert!((s[12_000] - 0.5).abs() <= 1e-6);
    assert!((s[24_000] - 1.0).abs() <= 1e-6);
    assert_eq!(s[24_001], 1.0);
    assert_eq!(s[47_999], 1.0);
    assert_eq!(e.history.undo_label(), Some("Fade In"));
    e.run(ActionId::Undo, 0..0).unwrap();
    assert!(read_ch(&e, 0).iter().all(|&v| v == 1.0));
}

#[test]
fn e1_drag_fade_out_from_frame_24000_ends_at_zero() {
    let dir = TempDir::new("out");
    let mut e = editor(&dir.0.join("s"), &[vec![1.0f32; 48_000]]);
    let view = Viewport { start: 0.0, frames_per_px: 1.0 };
    let g = drag_fade(FadeHandle::Out, &view, 0..48_000, 24_000.0, 0.0, false).unwrap();
    assert_eq!(g.range, 24_000..48_000);
    e.run_effect(effect_idx("fade_out"), vec![0.0], g.range.clone()).unwrap();
    let s = read_ch(&e, 0);
    assert_eq!(s[23_999], 1.0);
    assert!((s[24_000] - 1.0).abs() <= 1e-6);
    assert!((s[36_000] - 0.5).abs() <= 1e-3);
    assert_eq!(s[47_999], 0.0);
}
