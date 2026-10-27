//! f09: snapping (A123, A127) and channel enable (A132), without a window.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, action, by_name, id_by_name};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::snap::{SnapSettings, major_step, minor_divisions, minor_step, ruler_minor_tick_near, ruler_step, ruler_tick_near, snap_frame};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-snap-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

/// Left flips from +1 to -1 at frame 5000; right is a constant 0.25.
fn flip_stereo() -> Vec<Vec<f32>> {
    vec![(0..20_000).map(|k| if k < 5000 { 1.0 } else { -1.0 }).collect(), vec![0.25; 20_000]]
}

fn on() -> SnapSettings {
    SnapSettings::default()
}

#[test]
fn ruler_ticks_are_the_ones_the_ruler_draws() {
    assert_eq!(ruler_step(0.0004), 0.001);
    assert_eq!(ruler_step(0.7), 1.0);
    // 100 frames a pixel at 48 kHz: 11,000 frames = 0.229 s, so 0.5 s ticks.
    assert_eq!(ruler_tick_near(24_300, 100.0, RATE), 24_000);
    assert_eq!(ruler_tick_near(47_000, 100.0, RATE), 48_000);
}

#[test]
fn snapping_off_or_nothing_near_changes_nothing() {
    let t = TempDir::new("off");
    let doc = doc_from(&t.0, &flip_stereo());
    let off = SnapSettings { enabled: false, zero_crossings: true, ..on() };
    assert_eq!(snap_frame(&doc, &off, ChannelMask::ALL, 100.0, &[10_000], 10_100), 10_100);
    // Reach is 800 frames at 100 frames a pixel; the tick at 0 and the target are farther.
    assert_eq!(snap_frame(&doc, &on(), ChannelMask::ALL, 100.0, &[1000], 10_000), 10_000);
    // Past the end is brought back inside.
    assert_eq!(snap_frame(&doc, &SnapSettings { enabled: false, ..on() }, ChannelMask::ALL, 1.0, &[], 99_999), 20_000);
}

#[test]
fn a_frame_snaps_to_the_ruler_a_target_and_the_nearest_wins() {
    let t = TempDir::new("targets");
    let doc = doc_from(&t.0, &flip_stereo());
    // Tick at 12,000 (0.25 s steps? 0.5 s = 24,000): at 100 frames a pixel the ticks are 24,000 apart, so use 0.
    assert_eq!(snap_frame(&doc, &on(), ChannelMask::ALL, 100.0, &[], 600), 0);
    // A selection edge at 10,000 pulls 10,500 (500 away, reach 800).
    assert_eq!(snap_frame(&doc, &on(), ChannelMask::ALL, 100.0, &[10_000], 10_500), 10_000);
    // Two targets: the nearer one.
    assert_eq!(snap_frame(&doc, &on(), ChannelMask::ALL, 100.0, &[10_000, 10_400], 10_300), 10_400);
}

#[test]
fn zero_crossings_snap_only_when_asked_and_only_on_enabled_channels() {
    let t = TempDir::new("zero");
    let doc = doc_from(&t.0, &flip_stereo());
    let zero = SnapSettings { zero_crossings: true, ..on() };
    // 2 frames a pixel: reach 16 frames, ruler ticks every 240 (5040 is 37 away).
    assert_eq!(snap_frame(&doc, &on(), ChannelMask::ALL, 2.0, &[], 5003), 5003);
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::ALL, 2.0, &[], 5003), 5000);
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::ALL, 2.0, &[], 4995), 5000);
    // The right channel alone never crosses; left alone does.
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::only(1), 2.0, &[], 5003), 5003);
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::only(0), 2.0, &[], 5003), 5000);
    // Out of reach (16 frames) of the crossing and of the ticks at 4800 and 5040: stays.
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::ALL, 2.0, &[], 5020), 5020);
    // No channel enabled: nothing to look at, no crash.
    assert_eq!(snap_frame(&doc, &zero, ChannelMask::NONE, 2.0, &[], 5003), 5003);
}

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let mut e = Editor::new(History::new(doc_from(dir, data), u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

#[test]
fn snap_and_channel_actions_have_names_and_keys() {
    assert_eq!(by_name("view.snap.toggle").map(|a| a.id), Some(ActionId::SnapToggle));
    assert_eq!(by_name("view.snap.zero.toggle").map(|a| a.id), Some(ActionId::SnapZeroToggle));
    let keys: Vec<String> = wavcraft_core::shortcuts::BINDINGS
        .iter()
        .filter(|b| b.command == wavcraft_core::shortcuts::Command::Action(ActionId::SnapToggle))
        .map(|b| b.chord.text(false))
        .collect();
    assert_eq!(keys, ["S"]);
    assert_eq!(id_by_name("channel.toggle.1"), Some(ActionId::ToggleChannel(1)));
    assert_eq!(id_by_name("channel.toggle.64"), None);
    assert_eq!(id_by_name("channel.toggle.x"), None);
    assert_eq!(id_by_name("channel.toggle"), None);
}

#[test]
fn the_toggles_flip_the_flags_and_the_editor_snaps_with_them() {
    let t = TempDir::new("toggle");
    let mut e = editor(&t.0, &flip_stereo());
    assert!(e.snap.enabled && !e.snap.zero_crossings, "snapping starts on, zero crossings off");
    assert_eq!(e.snap(5003, 2.0, &[]), 5003);
    e.run(ActionId::SnapZeroToggle, 0..0).unwrap();
    assert!(e.snap.zero_crossings);
    assert_eq!(e.snap(5003, 2.0, &[]), 5000);
    e.run(ActionId::SnapToggle, 0..0).unwrap();
    assert!(!e.snap.enabled);
    assert_eq!(e.snap(5003, 2.0, &[]), 5003, "master switch off");
    e.run(ActionId::SnapToggle, 0..0).unwrap();
    assert!(e.snap.enabled);
    // Snapping is not an edit: no history step.
    assert_eq!(e.history.steps().len(), 1);
}

#[test]
fn channel_toggles_pick_the_channels_edits_and_snapping_act_on() {
    let t = TempDir::new("chan");
    let mut e = editor(&t.0, &flip_stereo());
    assert_eq!(e.enabled_flags(), "11");
    e.run(ActionId::ToggleChannel(0), 0..0).unwrap();
    assert_eq!(e.enabled_flags(), "01");
    // Zero crossings now look at the right channel only, which never crosses.
    e.snap.zero_crossings = true;
    assert_eq!(e.snap(5003, 2.0, &[]), 5003);
    // Silence acts on the enabled (right) channel only.
    e.run(ActionId::Silence, 100..200).unwrap();
    let doc = e.history.current();
    let (mut l, mut r) = ([9.0f32; 1], [9.0f32; 1]);
    doc.read(0, 150, &mut l).unwrap();
    doc.read(1, 150, &mut r).unwrap();
    assert_eq!((l[0], r[0]), (1.0, 0.0));
    // The last enabled channel cannot be switched off.
    let out = e.run(ActionId::ToggleChannel(1), 0..0).unwrap();
    assert_eq!(e.enabled_flags(), "01");
    assert!(out.message.unwrap().contains("At least one"));
    // Both again; a channel that does not exist is not available.
    e.run(ActionId::ToggleChannel(0), 0..0).unwrap();
    assert_eq!(e.enabled_flags(), "11");
    assert_eq!(e.channels, ChannelMask::ALL);
    assert!(!e.enabled(ActionId::ToggleChannel(2), &(0..0)));
    e.run(ActionId::ToggleChannel(7), 0..0).unwrap();
    assert_eq!(e.enabled_flags(), "11");
    // The Up / Down keys still work with the toggles.
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    assert_eq!(e.enabled_flags(), "10");
}

// ---- f49: the ruler's coarse and fine targets, and their switches ----

fn only(coarse: bool, fine: bool) -> SnapSettings {
    SnapSettings { ruler_coarse: coarse, ruler_fine: fine, edges: false, ..on() }
}

#[test]
fn minor_ticks_divide_the_major_step() {
    // 100 frames a pixel at 48 kHz: major 0.5 s, five minor ticks of 0.1 s.
    assert_eq!(major_step(100.0, RATE), 0.5);
    assert_eq!(minor_divisions(0.5), 5);
    assert!((minor_step(0.5) - 0.1).abs() < 1e-12);
    // 1 s -> ten of 0.1 s; 2 s -> four of 0.5 s; 60 s -> six of 10 s.
    assert!((minor_step(1.0) - 0.1).abs() < 1e-12);
    assert!((minor_step(2.0) - 0.5).abs() < 1e-12);
    assert!((minor_step(60.0) - 10.0).abs() < 1e-12);
    // Every step ruler_step can return divides into whole minor ticks (no zero, no overshoot).
    for s in [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0] {
        let n = minor_divisions(s);
        assert!(n >= 2 && (minor_step(s) * f64::from(n) - s).abs() < 1e-9, "step {s}");
    }
}

#[test]
fn coarse_and_fine_ticks_differ_at_each_zoom() {
    // Computed by hand. Zoom 100 fpp: major 0.5 s (24,000 frames), minor 0.1 s (4,800 frames).
    assert_eq!(ruler_tick_near(31_000, 100.0, RATE), 24_000);
    assert_eq!(ruler_minor_tick_near(31_000, 100.0, RATE), 28_800);
    assert_eq!(ruler_minor_tick_near(33_000, 100.0, RATE), 33_600);
    // Zoom 1000 fpp: 110,000 frames = 2.29 s -> major 5 s (240,000), minor 1 s (48,000).
    assert_eq!(major_step(1000.0, RATE), 5.0);
    assert_eq!(ruler_tick_near(130_000, 1000.0, RATE), 240_000);
    assert_eq!(ruler_minor_tick_near(130_000, 1000.0, RATE), 144_000);
    // Zoom 4 fpp: 440 frames = 9.2 ms -> major 10 ms (480), minor 1 ms (48).
    assert_eq!(major_step(4.0, RATE), 0.01);
    assert_eq!(ruler_tick_near(700, 4.0, RATE), 480);
    assert_eq!(ruler_minor_tick_near(700, 4.0, RATE), 720);
}

#[test]
fn coarse_only_snaps_to_major_ticks_fine_to_minor_ones() {
    let t = TempDir::new("ruler");
    let doc = doc_from(&t.0, &[vec![0.0; 300_000]]);
    let snap = |s: &SnapSettings, fpp: f64, frame: u64| snap_frame(&doc, s, ChannelMask::ALL, fpp, &[], frame);
    // 100 fpp: reach is 800 frames. Minor tick 28,800 is 800 from 28,000; the major 24,000 is not near.
    assert_eq!(snap(&only(true, false), 100.0, 28_000), 28_000, "coarse ignores a minor tick");
    assert_eq!(snap(&only(false, true), 100.0, 28_000), 28_800, "fine takes it");
    assert_eq!(snap(&only(true, false), 100.0, 24_500), 24_000, "coarse takes a major tick");
    assert_eq!(snap(&only(false, true), 100.0, 24_500), 24_000, "fine includes the major ticks");
    assert_eq!(snap(&only(true, true), 100.0, 24_500), 24_000);
    // Neither: the ruler is not a target.
    assert_eq!(snap(&only(false, false), 100.0, 24_500), 24_500);
    // Zoomed far out (1000 fpp, reach 8,000 frames): major 5 s = 240,000, minor 1 s = 48,000.
    assert_eq!(snap(&only(true, false), 1000.0, 236_000), 240_000);
    assert_eq!(snap(&only(true, false), 1000.0, 150_000), 150_000);
    assert_eq!(snap(&only(false, true), 1000.0, 150_000), 144_000);
    // Zoomed far in (4 fpp, reach 32 frames): major 480, minor 48.
    assert_eq!(snap(&only(true, false), 4.0, 700), 700);
    assert_eq!(snap(&only(false, true), 4.0, 700), 720);
}

#[test]
fn the_edges_switch_controls_listed_targets() {
    let t = TempDir::new("edges");
    let doc = doc_from(&t.0, &[vec![0.0; 100_000]]);
    let mut s = SnapSettings { ruler_coarse: false, ..on() };
    assert_eq!(snap_frame(&doc, &s, ChannelMask::ALL, 100.0, &[10_300], 10_000), 10_300);
    s.edges = false;
    assert_eq!(snap_frame(&doc, &s, ChannelMask::ALL, 100.0, &[10_300], 10_000), 10_000);
}

#[test]
fn the_snap_actions_flip_their_own_switch_only() {
    let t = TempDir::new("actions");
    let doc = doc_from(&t.0, &flip_stereo());
    let mut e = Editor::new(History::new(doc, u64::MAX));
    assert_eq!((e.snap.ruler_coarse, e.snap.ruler_fine, e.snap.edges), (true, false, true), "coarse and edges on, fine off by default");
    for (name, get) in [
        ("view.snap.ruler_coarse.toggle", (|s: &SnapSettings| s.ruler_coarse) as fn(&SnapSettings) -> bool),
        ("view.snap.ruler_fine.toggle", |s| s.ruler_fine),
        ("view.snap.edges.toggle", |s| s.edges),
    ] {
        let id = id_by_name(name).unwrap_or_else(|| panic!("{name} exists"));
        let before = e.snap;
        let out = e.run(id, 0..0).unwrap();
        assert!(out.message.is_some_and(|m| m.contains("on") || m.contains("off")));
        assert_ne!(get(&before), get(&e.snap), "{name} flips its flag");
        assert_eq!((before.enabled, before.zero_crossings), (e.snap.enabled, e.snap.zero_crossings), "{name} leaves the others");
        e.run(id, 0..0).unwrap();
        assert_eq!(e.snap, before, "{name} twice is a no-op");
    }
}
