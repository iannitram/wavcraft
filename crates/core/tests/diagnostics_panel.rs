//! Diagnostics panel (B050): repairing a chosen subset of the regions a scan found.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::Editor;
use wavcraft_core::diagnostics::{Tool, declick, declip};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-diagpanel-test-{}-{name}", std::process::id()));
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

fn now(e: &Editor) -> Vec<Vec<f32>> {
    let doc = e.history.current();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn clipped_sine() -> Vec<Vec<f32>> {
    let make = |phase: f64| -> Vec<f32> {
        (0..RATE as usize)
            .map(|n| ((1.4 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / f64::from(RATE) + phase).sin()) as f32).clamp(-1.0, 1.0))
            .collect()
    };
    vec![make(0.3), make(1.1)]
}

#[test]
fn repairing_a_subset_changes_only_those_regions_and_one_undo_restores() {
    let data = clipped_sine();
    let tmp = TempDir::new("subset");
    let mut e = editor(&tmp.0, &data);
    let doc = e.history.current().clone();
    let cancel = AtomicBool::new(false);
    let tool = Tool::DeClip(declip::Params::default());
    let found = tool.scan(&doc, 0..0, ChannelMask::ALL, &cancel).unwrap();
    assert!(found.regions.len() > 6);
    let chosen = vec![found.regions[1], found.regions[4]];
    let r = tool.repair(&doc, &chosen, Crossfades::NONE, &cancel).unwrap();
    assert_eq!((r.repaired, r.skipped), (2, 0));
    assert_eq!(r.edit.label, "DeClipper");
    let steps = e.history.steps().len();
    e.history.push_edit(r.edit);
    assert_eq!(e.history.steps().len(), steps + 1, "one undo step");
    let after = now(&e);
    for (ch, (a, b)) in data.iter().zip(&after).enumerate() {
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            let inside = chosen.iter().any(|c| c.channel == ch && (c.start..c.end()).contains(&(i as u64)));
            if inside {
                assert!(y.abs() >= x.abs(), "channel {ch} frame {i}: repaired sample is not lower");
            } else {
                assert_eq!(x.to_bits(), y.to_bits(), "channel {ch} frame {i} must not change");
            }
        }
    }
    assert!(chosen.iter().any(|c| (c.start..c.end()).any(|i| after[c.channel][i as usize] != data[c.channel][i as usize])));
    e.history.undo();
    assert_eq!(now(&e), data, "one undo restores every sample");
}

#[test]
fn repairing_every_region_equals_the_whole_file_repair() {
    let data = clipped_sine();
    let tmp = TempDir::new("all");
    let mut a = editor(&tmp.0.join("a"), &data);
    let mut b = editor(&tmp.0.join("b"), &data);
    let cancel = AtomicBool::new(false);
    let params = declip::Params::default();
    let whole = declip::declip(&a.history.current().clone(), 0..0, ChannelMask::ALL, &params, Crossfades::NONE, &cancel).unwrap();
    a.history.push_edit(whole.edit);
    let tool = Tool::DeClip(params);
    let doc = b.history.current().clone();
    let found = tool.scan(&doc, 0..0, ChannelMask::ALL, &cancel).unwrap();
    assert_eq!(found.regions.len(), whole.found);
    let r = tool.repair(&doc, &found.regions, Crossfades::NONE, &cancel).unwrap();
    b.history.push_edit(r.edit);
    assert_eq!(now(&a), now(&b));
}

#[test]
fn skipped_reports_the_runs_that_cannot_be_redrawn() {
    // A held level longer than the repair cap, and a run at the very start of the file.
    let mut ch: Vec<f32> = (0..20_000).map(|n| 0.3 * (n as f32 * 0.05).sin()).collect();
    for s in ch.iter_mut().take(5) {
        *s = 1.0;
    }
    for s in &mut ch[5_000..5_000 + 400] {
        *s = 1.0;
    }
    for s in &mut ch[10_000..10_006] {
        *s = 1.0;
    }
    let tmp = TempDir::new("skipped");
    let e = editor(&tmp.0, &[ch]);
    let doc = e.history.current().clone();
    let cancel = AtomicBool::new(false);
    let tool = Tool::DeClip(declip::Params::default());
    let found = tool.scan(&doc, 0..0, ChannelMask::ALL, &cancel).unwrap();
    assert_eq!(found.regions.len(), 3);
    assert_eq!(tool.skipped(&doc, &found.regions, &cancel).unwrap(), vec![true, true, false]);
    let r = tool.repair(&doc, &found.regions, Crossfades::NONE, &cancel).unwrap();
    assert_eq!((r.repaired, r.skipped), (1, 2));
}

#[test]
fn the_declicker_repairs_a_subset_too() {
    let mut ch: Vec<f32> = (0..30_000).map(|n| 0.4 * (n as f32 * 0.03).sin()).collect();
    for &at in &[5_000usize, 15_000, 25_000] {
        ch[at] += 0.5;
        ch[at + 1] -= 0.4;
    }
    let tmp = TempDir::new("declick");
    let mut e = editor(&tmp.0, &[ch.clone()]);
    let doc = e.history.current().clone();
    let cancel = AtomicBool::new(false);
    let tool = Tool::DeClick(declick::Params::default());
    let found = tool.scan(&doc, 0..0, ChannelMask::ALL, &cancel).unwrap();
    assert!(found.regions.len() >= 3, "found {}", found.regions.len());
    let one = vec![found.regions[1]];
    let r = tool.repair(&doc, &one, Crossfades::NONE, &cancel).unwrap();
    assert_eq!(r.edit.label, "DeClicker");
    e.history.push_edit(r.edit);
    let after = now(&e).remove(0);
    let (lo, hi) = (one[0].start as usize, one[0].end() as usize);
    for (i, (x, y)) in ch.iter().zip(&after).enumerate() {
        if i < lo || i >= hi {
            assert_eq!(x.to_bits(), y.to_bits(), "frame {i}");
        }
    }
    assert!(ch[lo..hi] != after[lo..hi]);
}
