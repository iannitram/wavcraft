//! #981: a lane click may only pick a clip the type filter shows. The hit test is
//! `Multitrack::visible_clip_at`, and the Multitrack view must use it.

use soundcraft_model::{BitDepthSetting, ClipId, TrackId};
use wavcraft_core::multitrack::{AudioType, ModelBuffer, Multitrack};

fn tone(secs: f32) -> ModelBuffer {
    let n = (48_000.0 * secs) as usize;
    ModelBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()] }
}

/// Clip "1" at 0.0..0.1 s (type Dialogue) and clip "2" at 1.0..1.1 s (no type), on track `a`.
fn session() -> (Multitrack, TrackId, ClipId, ClipId) {
    let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
    let a = m.add_track(None, 1).unwrap_or_else(|e| panic!("{e}"));
    let c1 = m.add_clip_buffer(a, "1", "1.wav", tone(0.1), 0.0).unwrap_or_else(|e| panic!("{e}"));
    let c2 = m.add_clip_buffer(a, "2", "2.wav", tone(0.1), 1.0).unwrap_or_else(|e| panic!("{e}"));
    m.set_clip_type(&[c1], Some(AudioType::Dialogue)).unwrap_or_else(|e| panic!("{e}"));
    (m, a, c1, c2)
}

#[test]
fn without_a_filter_a_click_finds_the_clip_under_it() {
    let (m, a, c1, c2) = session();
    assert_eq!(m.visible_clip_at(a, 0.05), Some(c1));
    assert_eq!(m.visible_clip_at(a, 1.05), Some(c2));
}

#[test]
fn a_click_on_empty_lane_finds_no_clip() {
    let (m, a, _, _) = session();
    assert_eq!(m.visible_clip_at(a, 0.5), None);
    assert_eq!(m.visible_clip_at(a, 2.0), None);
}

#[test]
fn a_filter_that_hides_the_clips_makes_a_click_find_nothing() {
    let (mut m, a, _, _) = session();
    m.type_filter = Some(AudioType::Music);
    assert_eq!(m.visible_clip_at(a, 0.05), None);
    assert_eq!(m.visible_clip_at(a, 1.05), None);
}

#[test]
fn a_filter_that_shows_a_clip_still_finds_it_and_hides_the_others() {
    let (mut m, a, c1, _) = session();
    m.type_filter = Some(AudioType::Dialogue);
    assert_eq!(m.visible_clip_at(a, 0.05), Some(c1));
    assert_eq!(m.visible_clip_at(a, 1.05), None);
}

#[test]
fn a_track_that_does_not_exist_finds_no_clip() {
    let (mut m, _, _, _) = session();
    let gone = m.add_track(Some("extra"), 1).unwrap_or_else(|e| panic!("{e}"));
    m.remove_track(gone).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.visible_clip_at(gone, 0.05), None);
}

#[test]
fn the_lane_click_handler_uses_the_core_hit_test() {
    let ui = include_str!("../../../apps/wavcraft/src/multitrack_ui.rs");
    assert!(ui.contains("mt.visible_clip_at(id, sec)"), "the lane click handler must call mt.visible_clip_at(id, sec)");
    assert!(!ui.contains("clips.iter().find(|c| (c.start as f64 / rate.."), "the unfiltered clips.iter().find hit test must be gone");
}
