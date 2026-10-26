//! Reorder tracks (roadmap C104, core only): `Multitrack::move_track(from, to)`.
//!
//! Positions are 0-based among the audio tracks. Ids, names, clips and mixer state belong to the
//! track and travel with it; the selection is kept by id; non-audio tracks keep their slots.

use wavcraft_core::multitrack::soundcraft_model::{BitDepthSetting, ChannelFormat, TrackId, TrackKind};
use wavcraft_core::multitrack::{MtError, Multitrack};

use soundcraft_audio_io::AudioBuffer as ModelBuffer;

/// A session with four audio tracks named T1 to T4; returns their ids in order.
fn four() -> (Multitrack, Vec<TrackId>) {
    let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
    let ids = ["T1", "T2", "T3", "T4"].iter().map(|n| m.add_track(Some(n), 2).unwrap_or_else(|e| panic!("{e}"))).collect();
    (m, ids)
}

fn mv(m: &mut Multitrack, from: usize, to: usize) {
    m.move_track(from, to).unwrap_or_else(|e| panic!("{e}"));
}

fn tone(secs: f64) -> ModelBuffer {
    let n = (48_000.0 * secs) as usize;
    ModelBuffer { sample_rate: 48_000, channels: vec![vec![0.25; n]] }
}

fn order(m: &Multitrack) -> Vec<TrackId> {
    m.track_ids()
}

fn pick(ids: &[TrackId], idx: &[usize]) -> Vec<TrackId> {
    idx.iter().map(|i| ids[*i]).collect()
}

fn msg(r: Result<(), MtError>) -> String {
    match r {
        Ok(()) => String::new(),
        Err(e) => e.0,
    }
}

#[test]
fn moving_forward_shifts_the_tracks_in_between_back() {
    let (mut m, ids) = four();
    mv(&mut m, 0, 2);
    assert_eq!(order(&m), pick(&ids, &[1, 2, 0, 3]));
}

#[test]
fn moving_backward_shifts_the_tracks_in_between_forward() {
    let (mut m, ids) = four();
    mv(&mut m, 3, 1);
    assert_eq!(order(&m), pick(&ids, &[0, 3, 1, 2]));
}

#[test]
fn moving_to_a_neighbour_swaps_the_pair_and_back_again() {
    let (mut m, ids) = four();
    mv(&mut m, 0, 1);
    assert_eq!(order(&m), pick(&ids, &[1, 0, 2, 3]));
    mv(&mut m, 1, 0);
    assert_eq!(order(&m), ids);
}

#[test]
fn moving_to_the_ends_works() {
    let (mut m, ids) = four();
    mv(&mut m, 0, 3);
    assert_eq!(order(&m), pick(&ids, &[1, 2, 3, 0]));
    mv(&mut m, 3, 0);
    assert_eq!(order(&m), ids);
    mv(&mut m, 3, 0);
    assert_eq!(order(&m), pick(&ids, &[3, 0, 1, 2]));
}

#[test]
fn the_same_position_changes_nothing() {
    let (mut m, _) = four();
    m.set_mute(order(&m)[1], true).unwrap_or_else(|e| panic!("{e}"));
    let before = m.session.tracks.clone();
    mv(&mut m, 2, 2);
    assert_eq!(m.session.tracks, before);
}

#[test]
fn a_position_past_the_last_track_is_refused_and_nothing_changes() {
    let (mut m, _) = four();
    let before = m.session.tracks.clone();
    let from = msg(m.move_track(4, 0));
    assert!(from.contains("track position 4"), "{from}");
    assert!(from.contains("4 audio tracks"), "{from}");
    let to = msg(m.move_track(0, 7));
    assert!(to.contains("track position 7"), "{to}");
    assert!(to.contains("4 audio tracks"), "{to}");
    assert!(m.move_track(0, 4).is_err(), "the first position past the end is refused");
    assert!(m.move_track(9, 9).is_err(), "the same bad position is still refused");
    assert!(m.move_track(usize::MAX, 0).is_err());
    assert_eq!(m.session.tracks, before);
}

#[test]
fn the_from_position_is_reported_first_when_both_are_bad() {
    let (mut m, _) = four();
    let text = msg(m.move_track(5, 9));
    assert!(text.contains("track position 5"), "{text}");
    assert!(!text.contains("position 9"), "{text}");
}

#[test]
fn an_empty_session_refuses_every_move() {
    let mut m = Multitrack::new("Empty", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
    let text = msg(m.move_track(0, 0));
    assert!(text.contains("0 audio tracks"), "{text}");
}

#[test]
fn ids_names_and_the_other_tracks_are_untouched() {
    let (mut m, ids) = four();
    let before = m.session.tracks.clone();
    mv(&mut m, 0, 2);
    assert_eq!(m.track_count(), 4);
    for (i, id) in ids.iter().enumerate() {
        let now = m.session.track(*id).unwrap_or_else(|| panic!("track kept"));
        let was = before.iter().find(|t| t.id == *id).unwrap_or_else(|| panic!("track was there"));
        assert_eq!(now, was, "track {i} is the same track");
        assert_eq!(now.name, format!("T{}", i + 1));
    }
    assert_eq!(m.find_track("t3"), Ok(ids[2]));
}

#[test]
fn positions_in_find_track_follow_the_new_order() {
    let (mut m, ids) = four();
    mv(&mut m, 0, 1);
    assert_eq!(m.find_track("#1"), Ok(ids[1]));
    assert_eq!(m.find_track("#2"), Ok(ids[0]));
}

#[test]
fn clips_stay_with_their_track() {
    let (mut m, ids) = four();
    m.add_clip_buffer(ids[0], "A", "a.wav", tone(0.5), 0.0).unwrap_or_else(|e| panic!("{e}"));
    m.add_clip_buffer(ids[0], "A2", "a2.wav", tone(0.25), 1.0).unwrap_or_else(|e| panic!("{e}"));
    m.add_clip_buffer(ids[2], "C", "c.wav", tone(0.5), 2.0).unwrap_or_else(|e| panic!("{e}"));
    let sources = m.session.sources.len();
    mv(&mut m, 0, 3);
    assert_eq!((m.clip_count(), m.session.sources.len()), (3, sources));
    let names = |id: TrackId| m.session.track(id).map(|t| t.clips().iter().map(|c| c.name.clone()).collect::<Vec<_>>());
    assert_eq!(names(ids[0]), Some(vec!["A".to_string(), "A2".to_string()]));
    assert_eq!(names(ids[2]), Some(vec!["C".to_string()]));
    assert_eq!(names(ids[1]), Some(vec![]));
}

#[test]
fn mute_solo_arm_and_volume_travel_with_the_track() {
    let (mut m, ids) = four();
    m.set_mute(ids[0], true).unwrap_or_else(|e| panic!("{e}"));
    m.set_solo(ids[1], true).unwrap_or_else(|e| panic!("{e}"));
    m.set_volume_db(ids[3], -6.0).unwrap_or_else(|e| panic!("{e}"));
    if let Some(t) = m.session.track_mut(ids[2]) {
        t.mixer.record_arm = true;
    }
    mv(&mut m, 0, 2);
    let state: Vec<(bool, bool, bool, f32)> = order(&m)
        .iter()
        .map(|id| {
            let t = m.session.track(*id).unwrap_or_else(|| panic!("track"));
            (t.mixer.mute, t.mixer.solo, t.mixer.record_arm, t.mixer.volume_db)
        })
        .collect();
    assert_eq!(state, vec![(false, true, false, 0.0), (false, false, true, 0.0), (true, false, false, 0.0), (false, false, false, -6.0)]);
}

#[test]
fn the_selection_is_kept_by_id() {
    let (mut m, ids) = four();
    m.session.edit.selected_tracks = vec![ids[2], ids[0]];
    mv(&mut m, 0, 3);
    assert_eq!(m.session.edit.selected_tracks, vec![ids[2], ids[0]]);
    mv(&mut m, 2, 0);
    assert_eq!(m.session.edit.selected_tracks, vec![ids[2], ids[0]]);
}

#[test]
fn tracks_that_are_not_audio_keep_their_slots() {
    let (mut m, ids) = four();
    let aux = m.session.add_track(TrackKind::Aux, ChannelFormat::Stereo, None);
    let master = m.session.add_track(TrackKind::Master, ChannelFormat::Stereo, None);
    m.session.tracks.swap(1, 4);
    let all = |m: &Multitrack| m.session.tracks.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(all(&m), vec![ids[0], aux, ids[2], ids[3], ids[1], master]);
    mv(&mut m, 0, 2);
    assert_eq!(all(&m), vec![ids[2], aux, ids[3], ids[0], ids[1], master]);
    assert_eq!(order(&m), vec![ids[2], ids[3], ids[0], ids[1]]);
    assert!(m.move_track(4, 0).is_err(), "only the four audio tracks count");
    mv(&mut m, 3, 0);
    assert_eq!(all(&m), vec![ids[1], aux, ids[2], ids[3], ids[0], master]);
}
