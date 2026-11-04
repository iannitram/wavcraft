//! #977: arming a track of n channels at input c sets the route "In c" for one channel and
//! "In c-(c+n-1)" for two or more, and the record plan still takes n consecutive inputs.

use soundcraft_model::{BitDepthSetting, ChannelFormat, TrackId};
use wavcraft_core::mtrecord::input_label;
use wavcraft_core::multitrack::Multitrack;

fn session() -> (Multitrack, TrackId, TrackId, TrackId) {
    let mut m = Multitrack::new("Rec", 48_000, BitDepthSetting::Int24).unwrap();
    let mono = m.add_track(Some("Vox"), 1).unwrap();
    let stereo = m.add_track(Some("Gtr"), 2).unwrap();
    let surround = m.add_track_format(Some("Sur"), ChannelFormat::Surround51).unwrap();
    (m, mono, stereo, surround)
}

fn label(m: &Multitrack, id: TrackId) -> String {
    input_label(&m.session.track(id).unwrap().mixer.input)
}

#[test]
fn a_mono_track_armed_at_3_is_in_3() {
    let (mut m, mono, ..) = session();
    m.arm_track(mono, Some(3)).unwrap();
    assert_eq!(label(&m, mono), "In 3");
}

#[test]
fn a_stereo_track_armed_at_5_is_in_5_6() {
    let (mut m, _, stereo, _) = session();
    m.arm_track(stereo, Some(5)).unwrap();
    assert_eq!(label(&m, stereo), "In 5-6");
}

#[test]
fn a_5_1_track_armed_at_3_is_in_3_8() {
    let (mut m, _, _, sur) = session();
    m.arm_track(sur, Some(3)).unwrap();
    assert_eq!(label(&m, sur), "In 3-8");
    assert!(m.session.track(sur).unwrap().mixer.record_arm, "the track is armed");
}

#[test]
fn a_5_1_track_armed_at_1_is_in_1_6() {
    let (mut m, _, _, sur) = session();
    m.arm_track(sur, Some(1)).unwrap();
    assert_eq!(label(&m, sur), "In 1-6");
}

#[test]
fn arming_a_5_1_track_with_no_input_and_no_route_is_in_1_6() {
    let (mut m, _, _, sur) = session();
    m.arm_track(sur, None).unwrap();
    assert_eq!(label(&m, sur), "In 1-6");
}

#[test]
fn rearming_a_5_1_track_with_no_input_keeps_its_first_channel() {
    let (mut m, _, _, sur) = session();
    m.arm_track(sur, Some(3)).unwrap();
    m.arm_track(sur, None).unwrap();
    assert_eq!(label(&m, sur), "In 3-8");
}

#[test]
fn the_5_1_plan_takes_six_consecutive_inputs_from_the_armed_one() {
    let (mut m, _, _, sur) = session();
    m.arm_track(sur, Some(3)).unwrap();
    let plan = m.plan_takes(&std::env::temp_dir(), 8);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].inputs, vec![2, 3, 4, 5, 6, 7]);
}

#[test]
fn the_session_state_reports_the_5_1_route_as_in_3_8() {
    let (mut m, mono, stereo, sur) = session();
    m.arm_track(mono, Some(3)).unwrap();
    m.arm_track(stereo, Some(5)).unwrap();
    m.arm_track(sur, Some(3)).unwrap();
    let state = m.state_value();
    let inputs: Vec<&str> = state["tracks"].as_array().unwrap().iter().map(|t| t["input"].as_str().unwrap()).collect();
    assert_eq!(inputs, vec!["In 3", "In 5-6", "In 3-8"]);
}

#[test]
fn the_docs_name_the_multichannel_form() {
    let src = include_str!("../src/mtrecord.rs");
    let module_doc: String = src.lines().take_while(|l| l.starts_with("//!")).collect::<Vec<_>>().join("\n");
    assert!(module_doc.contains("In 3-8"), "the module doc should give the multichannel form \"In 3-8\":\n{module_doc}");
    assert!(!src.contains("a stereo track takes `input` and the next one"), "the arm_track doc still describes only the stereo case");
}
