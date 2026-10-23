//! Invert phase per track (C110): the per-track flag on `Multitrack` and the sample flip.

use wavcraft_core::multitrack::Multitrack;
use wavcraft_core::multitrack::invert::apply_invert;
use wavcraft_core::multitrack::soundcraft_model::{BitDepthSetting, Session, TrackId};

fn two_tracks() -> (Multitrack, TrackId, TrackId) {
    let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).expect("new session");
    let a = m.add_track(Some("Guitar"), 2).expect("track a");
    let b = m.add_track(Some("Voice"), 1).expect("track b");
    (m, a, b)
}

fn gone_id(m: &mut Multitrack) -> TrackId {
    let id = m.add_track(Some("Temp"), 1).expect("temp track");
    m.remove_track(id).expect("remove temp");
    id
}

#[test]
fn a_new_track_is_not_inverted() {
    let (m, a, b) = two_tracks();
    assert_eq!(m.invert(a), Ok(false));
    assert_eq!(m.invert(b), Ok(false));
}

#[test]
fn set_invert_turns_one_track_on_and_leaves_the_other() {
    let (mut m, a, b) = two_tracks();
    m.set_invert(a, true).expect("set a");
    assert_eq!(m.invert(a), Ok(true));
    assert_eq!(m.invert(b), Ok(false));
    assert!(m.session.track(a).expect("track a").mixer.phase_invert);
    assert!(!m.session.track(b).expect("track b").mixer.phase_invert);
}

#[test]
fn set_invert_can_turn_it_off_again() {
    let (mut m, a, _b) = two_tracks();
    m.set_invert(a, true).expect("on");
    m.set_invert(a, false).expect("off");
    assert_eq!(m.invert(a), Ok(false));
}

#[test]
fn setting_the_same_value_twice_keeps_it() {
    let (mut m, a, _b) = two_tracks();
    m.set_invert(a, true).expect("on");
    m.set_invert(a, true).expect("on again");
    assert_eq!(m.invert(a), Ok(true));
}

#[test]
fn an_unknown_track_is_refused_by_both_calls() {
    let (mut m, _a, _b) = two_tracks();
    let gone = gone_id(&mut m);
    let set = m.set_invert(gone, true);
    assert_eq!(set.expect_err("set refused").0, "no such track");
    let get = m.invert(gone);
    assert_eq!(get.expect_err("get refused").0, "no such track");
}

#[test]
fn a_refused_set_changes_no_track() {
    let (mut m, a, b) = two_tracks();
    let gone = gone_id(&mut m);
    let _ = m.set_invert(gone, true);
    assert_eq!(m.invert(a), Ok(false));
    assert_eq!(m.invert(b), Ok(false));
}

#[test]
fn invert_leaves_mute_solo_and_volume_alone() {
    let (mut m, a, _b) = two_tracks();
    m.set_mute(a, true).expect("mute");
    m.set_solo(a, true).expect("solo");
    m.set_volume_db(a, -6.0).expect("volume");
    m.set_invert(a, true).expect("invert");
    let mixer = &m.session.track(a).expect("track a").mixer;
    assert!(mixer.mute && mixer.solo);
    assert_eq!(mixer.volume_db, -6.0);
}

#[test]
fn the_flag_survives_the_models_json_round_trip() {
    let (mut m, a, b) = two_tracks();
    m.set_invert(a, true).expect("invert a");
    let text = m.to_json().expect("to json");
    let back = Session::from_json(&text).expect("from json");
    assert!(back.track(a).expect("track a").mixer.phase_invert);
    assert!(!back.track(b).expect("track b").mixer.phase_invert);
}

#[test]
fn apply_invert_true_flips_every_sample() {
    let mut x = [0.5_f32, -0.25, 1.0, -1.0, 0.0, 0.125];
    apply_invert(&mut x, true);
    assert_eq!(x, [-0.5, 0.25, -1.0, 1.0, 0.0, -0.125]);
}

#[test]
fn apply_invert_false_changes_nothing() {
    let mut x = [0.5_f32, -0.25, 1.0, -1.0, 0.0];
    apply_invert(&mut x, false);
    assert_eq!(x.map(f32::to_bits), [0.5_f32, -0.25, 1.0, -1.0, 0.0].map(f32::to_bits));
}

#[test]
fn applying_twice_restores_the_original_bits() {
    let original = [0.3_f32, -0.7, 0.0, 1.0e-30, -123.456];
    let mut x = original;
    apply_invert(&mut x, true);
    apply_invert(&mut x, true);
    assert_eq!(x.map(f32::to_bits), original.map(f32::to_bits));
}

#[test]
fn an_inverted_copy_cancels_the_original() {
    let original: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin() * 0.8).collect();
    let mut flipped = original.clone();
    apply_invert(&mut flipped, true);
    assert!(original.iter().zip(&flipped).all(|(a, b)| a + b == 0.0));
    assert!(original.iter().zip(&flipped).any(|(a, b)| a != b));
}

#[test]
fn apply_invert_handles_an_empty_buffer_and_special_values() {
    let mut empty: [f32; 0] = [];
    apply_invert(&mut empty, true);
    let mut x = [f32::INFINITY, f32::NEG_INFINITY, f32::NAN];
    apply_invert(&mut x, true);
    assert_eq!(x[0], f32::NEG_INFINITY);
    assert_eq!(x[1], f32::INFINITY);
    assert!(x[2].is_nan());
}
