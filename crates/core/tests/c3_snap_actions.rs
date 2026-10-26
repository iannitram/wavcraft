//! Locked tests for wav-c-22-c (v1 C3): Snapping actions, key S, menu labels and status messages.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, Kind, Repeat, action, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::snap::SnapSettings;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-c3-actions-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, frames: usize) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 16 << 20, ..StoreOptions::default() }).unwrap();
    let samples: Vec<f32> = (0..frames).map(|k| k as f32).collect();
    let ch = Channel::from_pieces(samples.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()));
    let doc = DocState::new(s, vec![ch], RATE, SourceFormat::synthetic(RATE, 1)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

#[test]
fn c3_actions_table_binds_s_to_enable_snapping_and_enables_all_four_switches() {
    let t = TempDir::new("table");
    let e = editor(&t.0, 0);
    assert_eq!(e.snap, SnapSettings { enabled: true, markers: true, ruler: true, zero_crossings: false });
    for (id, name, menu) in [
        (ActionId::SnapToggle, "edit.snapping.enabled", "Enable Snapping"),
        (ActionId::SnapMarkersToggle, "edit.snapping.markers", "Snap to Markers"),
        (ActionId::SnapRulerToggle, "edit.snapping.ruler", "Snap to Ruler"),
        (ActionId::SnapZeroToggle, "edit.snapping.zeroCrossings", "Snap to Zero Crossings"),
    ] {
        let row = action(id);
        assert_eq!(row.name, name);
        assert_eq!(row.menu, menu);
        assert_eq!(row.kind, Kind::Edit);
        assert_eq!(by_name(name).map(|a| a.id), Some(id));
        assert!(e.enabled(id, &(0..0)), "{id:?} must be enabled even on an empty document");
    }
    let keys: Vec<String> = action(ActionId::SnapToggle).keys.iter().map(|k| k.text(false)).collect();
    assert_eq!(keys, ["S"]);
    assert!(action(ActionId::SnapMarkersToggle).keys.is_empty());
    assert!(action(ActionId::SnapRulerToggle).keys.is_empty());
    assert!(action(ActionId::SnapZeroToggle).keys.is_empty());
}

#[test]
fn c3_actions_labels_show_on_and_off_for_each_snapping_switch() {
    let t = TempDir::new("labels");
    let mut e = editor(&t.0, 100);
    assert_eq!(e.label(ActionId::SnapToggle), "Enable Snapping (on)");
    assert_eq!(e.label(ActionId::SnapMarkersToggle), "Snap to Markers (on)");
    assert_eq!(e.label(ActionId::SnapRulerToggle), "Snap to Ruler (on)");
    assert_eq!(e.label(ActionId::SnapZeroToggle), "Snap to Zero Crossings (off)");
    e.snap = SnapSettings { enabled: false, markers: false, ruler: false, zero_crossings: true };
    assert_eq!(e.label(ActionId::SnapToggle), "Enable Snapping (off)");
    assert_eq!(e.label(ActionId::SnapMarkersToggle), "Snap to Markers (off)");
    assert_eq!(e.label(ActionId::SnapRulerToggle), "Snap to Ruler (off)");
    assert_eq!(e.label(ActionId::SnapZeroToggle), "Snap to Zero Crossings (on)");
}

#[test]
fn c3_actions_toggle_snap_flips_master_switch_and_reports_status() {
    let t = TempDir::new("master");
    let mut e = editor(&t.0, 1_000);
    let out = e.run(ActionId::SnapToggle, 10..20).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(out.selection, 10..20);
    assert_eq!(out.message.as_deref(), Some("Snapping off"));
    assert!(!e.snap.enabled);
    let out = e.run(ActionId::SnapToggle, 10..20).unwrap();
    assert!(!out.doc_changed);
    assert_eq!(out.selection, 10..20);
    assert_eq!(out.message.as_deref(), Some("Snapping on"));
    assert!(e.snap.enabled);
}

#[test]
fn c3_actions_toggle_snap_flips_markers_ruler_and_zero_crossings_independently() {
    let t = TempDir::new("targets");
    let mut e = editor(&t.0, 1_000);
    let out = e.run(ActionId::SnapMarkersToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Markers off"));
    assert_eq!(e.snap, SnapSettings { enabled: true, markers: false, ruler: true, zero_crossings: false });
    let out = e.run(ActionId::SnapMarkersToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Markers on"));
    assert!(e.snap.markers);

    let out = e.run(ActionId::SnapRulerToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Ruler off"));
    assert!(!e.snap.ruler);
    let out = e.run(ActionId::SnapRulerToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Ruler on"));
    assert!(e.snap.ruler);

    let out = e.run(ActionId::SnapZeroToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Zero Crossings on"));
    assert!(e.snap.zero_crossings);
    let out = e.run(ActionId::SnapZeroToggle, 5..15).unwrap();
    assert_eq!(out.message.as_deref(), Some("Snap to Zero Crossings off"));
    assert!(!e.snap.zero_crossings);
}

#[test]
fn c3_actions_snapping_toggles_add_no_history_step_and_do_not_replace_repeat_last() {
    let t = TempDir::new("history");
    let mut e = editor(&t.0, 1_000);
    e.run(ActionId::Silence, 10..20).unwrap();
    assert_eq!(e.history.steps().len(), 2);
    assert_eq!(e.last, Some(Repeat::Action(ActionId::Silence)));
    for id in [ActionId::SnapToggle, ActionId::SnapMarkersToggle, ActionId::SnapRulerToggle, ActionId::SnapZeroToggle] {
        let out = e.run(id, 10..20).unwrap();
        assert!(!out.doc_changed);
    }
    assert_eq!(e.history.steps().len(), 2, "snapping switches never record an undo step");
    assert_eq!(e.last, Some(Repeat::Action(ActionId::Silence)), "snapping switches are not repeatable");
    assert_eq!(e.snap, SnapSettings { enabled: false, markers: false, ruler: false, zero_crossings: true });
}

#[test]
fn c3_actions_toggle_snap_clamps_selection_through_run() {
    let t = TempDir::new("clamp");
    let mut e = editor(&t.0, 1_000);
    let out = e.run(ActionId::SnapToggle, 500..2_000).unwrap();
    assert_eq!(out.selection, 500..1_000);
    assert_eq!(out.message.as_deref(), Some("Snapping off"));
    assert_eq!(e.label(ActionId::SnapToggle), "Enable Snapping (off)");
}
