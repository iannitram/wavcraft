//! Locked tests for wav-c-22-b (v1 C3): snapping a frame to markers, ruler ticks and zero crossings.

use std::path::{Path, PathBuf};

use wavcraft_core::doc::edit::ChannelMask;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::snap::{SNAP_PX, SnapSettings, snap_frame};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-c3-snap-{}-{name}", std::process::id()));
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
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 16 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

#[test]
fn c3_snap_default_settings_enable_markers_and_ruler_only() {
    let t = TempDir::new("defaults");
    let doc = doc_from(&t.0, &[vec![0.25f32; 96_000]]);
    let def = SnapSettings::default();
    assert_eq!(SNAP_PX, 8.0);
    assert_eq!(def, SnapSettings { enabled: true, markers: true, ruler: true, zero_crossings: false });
    // At 10 frames/px, reach is 80 frames; marker 10_000 is 50 frames away, ruler tick 9_600 is 450 away.
    let got = snap_frame(&doc, &def, ChannelMask::ALL, 10.0, &[10_000], 10_050);
    assert_eq!(got, 10_000);
    assert_eq!(got.abs_diff(10_000), 0);
}

#[test]
fn c3_snap_marker_edge_snaps_within_zero_samples_inside_reach_and_stays_outside() {
    let t = TempDir::new("markers");
    let doc = doc_from(&t.0, &[vec![0.25f32; 96_000]]);
    let on = SnapSettings { enabled: true, markers: true, ruler: false, zero_crossings: false };
    // At 10 frames/px, reach = (8.0 * 10.0).round() = 80 frames.
    let a = snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[12_345, 50_000], 12_425);
    let b = snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[12_345, 50_000], 12_265);
    assert_eq!(a, 12_345);
    assert_eq!(b, 12_345);
    assert_eq!(a.abs_diff(12_345), 0);
    assert_eq!(b.abs_diff(12_345), 0);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[12_345, 50_000], 12_426), 12_426);
    // Closer marker wins; equidistant tie goes to the earlier marker in the slice.
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[20_000, 20_100], 20_060), 20_100);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[20_000, 20_100], 20_050), 20_000);
    // A marker past the document's end is ignored.
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 10.0, &[96_050], 96_000), 96_000);
}

#[test]
fn c3_snap_ruler_snaps_to_nearest_tick_within_eight_pixels() {
    let t = TempDir::new("ruler");
    let doc = doc_from(&t.0, &[vec![0.25f32; 96_000]]);
    let on = SnapSettings { enabled: true, markers: false, ruler: true, zero_crossings: false };
    // At 300 frames/px (48 kHz), step is 1.0 s (48_000 frames) and reach is 2_400 frames.
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 300.0, &[], 46_000), 48_000);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 300.0, &[], 45_599), 45_599);
}

#[test]
fn c3_snap_zero_crossings_respect_active_channels_and_prefer_left_on_tie() {
    let t = TempDir::new("zero");
    // Left crosses at 1_000 and 1_100; right crosses only at 2_000.
    let left: Vec<f32> = (0..4_000).map(|k| if (1_000..1_100).contains(&k) { -0.5 } else { 0.5 }).collect();
    let right: Vec<f32> = (0..4_000).map(|k| if k < 2_000 { 0.5 } else { -0.5 }).collect();
    let doc = doc_from(&t.0, &[left, right]);
    let on = SnapSettings { enabled: true, markers: false, ruler: false, zero_crossings: true };
    // At 10 frames/px, reach is 80 frames.
    assert_eq!(snap_frame(&doc, &on, ChannelMask::only(0), 10.0, &[], 1_030), 1_000);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::only(0), 10.0, &[], 1_070), 1_100);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::only(0), 10.0, &[], 1_050), 1_000);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::only(1), 10.0, &[], 1_030), 1_030);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::only(1), 10.0, &[], 1_950), 2_000);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::NONE, 10.0, &[], 1_010), 1_010);
}

#[test]
fn c3_snap_chooses_nearest_across_sources_and_breaks_ties_markers_then_ruler_then_zero() {
    let t = TempDir::new("priority");
    // Mono doc of 10_000 frames with a single zero crossing at 2_450; at 10 frames/px (48 kHz),
    // the ruler step is 0.05 s (2_400 frames), so 2_400 is a ruler tick.
    let mono: Vec<f32> = (0..10_000).map(|k| if k < 2_450 { 0.5 } else { -0.5 }).collect();
    let doc = doc_from(&t.0, &[mono]);
    let all = SnapSettings { enabled: true, markers: true, ruler: true, zero_crossings: true };
    // At 2_440: marker 2_350 is 90 away (out of reach 80), ruler 2_400 is 40 away, zero 2_450 is 10 away.
    assert_eq!(snap_frame(&doc, &all, ChannelMask::ALL, 10.0, &[2_350], 2_440), 2_450);
    // At 2_375: marker 2_350 and ruler 2_400 are both 25 away -> marker wins.
    assert_eq!(snap_frame(&doc, &all, ChannelMask::ALL, 10.0, &[2_350], 2_375), 2_350);
    // At 2_425 with no markers: ruler 2_400 and zero 2_450 are both 25 away -> ruler wins.
    assert_eq!(snap_frame(&doc, &all, ChannelMask::ALL, 10.0, &[], 2_425), 2_400);
}

#[test]
fn c3_snap_disabled_or_invalid_scale_returns_clamped_frame() {
    let t = TempDir::new("disabled");
    let doc = doc_from(&t.0, &[vec![0.25f32; 10_000]]);
    let off = SnapSettings { enabled: false, ..SnapSettings::default() };
    let on = SnapSettings::default();
    assert_eq!(snap_frame(&doc, &off, ChannelMask::ALL, 10.0, &[2_400], 2_410), 2_410);
    assert_eq!(snap_frame(&doc, &off, ChannelMask::ALL, 10.0, &[2_400], 15_000), 10_000);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 0.0, &[2_400], 2_410), 2_410);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, -5.0, &[2_400], 2_410), 2_410);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, f64::NAN, &[2_400], 2_410), 2_410);
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, f64::INFINITY, &[2_400], 2_410), 2_410);
    // At 0.05 frames/px, (8.0 * 0.05).round() == 0, so reach is 0 and nothing snaps.
    assert_eq!(snap_frame(&doc, &on, ChannelMask::ALL, 0.05, &[2_400], 2_401), 2_401);
}
