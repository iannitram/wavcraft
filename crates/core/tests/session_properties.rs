//! C078: change a multitrack session's name, tempo, time signature, rate and bit depth later
//! (`wavcraft_core::multitrack::properties`).

use soundcraft_audio_io::AudioBuffer;
use soundcraft_model::{BitDepthSetting, Playlist, TrackId};
use wavcraft_core::multitrack::{bits_of, Multitrack};

const RATE: u32 = 48_000;

fn new_session() -> Multitrack {
    Multitrack::new("Mix", RATE, BitDepthSetting::Int24).unwrap()
}

/// A session with one mono track "A" and one clip "c" (two samples) on it.
fn with_clip() -> (Multitrack, TrackId) {
    let mut m = new_session();
    let t = m.add_track(Some("A"), 1).unwrap();
    let audio = AudioBuffer { sample_rate: RATE, channels: vec![vec![0.5, 0.25]] };
    m.add_clip_buffer(t, "c", "c.wav", audio, 0.0).unwrap();
    (m, t)
}

fn message<T: std::fmt::Debug>(r: Result<T, wavcraft_core::multitrack::MtError>) -> String {
    r.unwrap_err().0
}

// ---- holds_audio ----

#[test]
fn an_empty_session_holds_no_audio_even_with_tracks() {
    let mut m = new_session();
    assert!(!m.holds_audio());
    m.add_track(Some("A"), 1).unwrap();
    m.add_track(Some("B"), 2).unwrap();
    assert!(!m.holds_audio());
}

#[test]
fn a_clip_with_its_source_is_audio() {
    let (m, _) = with_clip();
    assert!(m.holds_audio());
}

#[test]
fn a_source_alone_is_audio() {
    let (mut m, t) = with_clip();
    m.session.track_mut(t).unwrap().playlist_mut().unwrap().clips.clear();
    assert_eq!(m.session.sources.len(), 1);
    assert!(m.holds_audio());
}

#[test]
fn a_clip_alone_is_audio() {
    let (mut m, _) = with_clip();
    m.session.sources.clear();
    assert!(m.holds_audio());
}

#[test]
fn a_clip_on_an_inactive_playlist_is_audio() {
    let (mut m, t) = with_clip();
    m.session.sources.clear();
    let track = m.session.track_mut(t).unwrap();
    track.playlists.push(Playlist::new("alt"));
    track.active_playlist = 1;
    assert!(track.clips().is_empty());
    assert!(m.holds_audio());
}

#[test]
fn a_clip_on_the_second_track_only_is_audio() {
    let mut m = new_session();
    m.add_track(Some("A"), 1).unwrap();
    let b = m.add_track(Some("B"), 1).unwrap();
    let audio = AudioBuffer { sample_rate: RATE, channels: vec![vec![0.5, 0.25]] };
    m.add_clip_buffer(b, "c", "c.wav", audio, 0.0).unwrap();
    m.session.sources.clear();
    assert!(m.holds_audio());
}

#[test]
fn removing_the_track_empties_the_session_again() {
    let (mut m, t) = with_clip();
    m.remove_track(t).unwrap();
    assert!(!m.holds_audio());
}

// ---- name ----

#[test]
fn rename_returns_the_old_name() {
    let mut m = new_session();
    assert_eq!(m.set_name("Take 2"), Ok("Mix".to_string()));
    assert_eq!(m.name(), "Take 2");
    assert_eq!(m.set_name("Take 3"), Ok("Take 2".to_string()));
    assert_eq!(m.name(), "Take 3");
}

#[test]
fn rename_trims_the_name() {
    let mut m = new_session();
    assert_eq!(m.set_name("  Final  "), Ok("Mix".to_string()));
    assert_eq!(m.name(), "Final");
}

#[test]
fn an_empty_name_is_refused_and_changes_nothing() {
    let mut m = new_session();
    assert_eq!(message(m.set_name("")), "a name cannot be empty");
    assert_eq!(message(m.set_name("   ")), "a name cannot be empty");
    assert_eq!(m.name(), "Mix");
}

#[test]
fn a_long_name_is_cut_to_256_characters() {
    let mut m = new_session();
    assert_eq!(m.set_name(&"x".repeat(300)), Ok("Mix".to_string()));
    assert_eq!(m.name().chars().count(), 256);
    let mut m = new_session();
    m.set_name(&"y".repeat(256)).unwrap();
    assert_eq!(m.name().chars().count(), 256);
}

#[test]
fn rename_is_allowed_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    assert_eq!(m.set_name("Later"), Ok("Mix".to_string()));
    assert_eq!(m.name(), "Later");
}

// ---- tempo ----

#[test]
fn the_default_tempo_is_120() {
    assert_eq!(new_session().tempo_bpm(), 120.0);
}

#[test]
fn set_tempo_returns_the_old_tempo() {
    let mut m = new_session();
    assert_eq!(m.set_tempo_bpm(98.5), Ok(120.0));
    assert_eq!(m.tempo_bpm(), 98.5);
    assert_eq!(m.set_tempo_bpm(140.0), Ok(98.5));
    assert_eq!(m.tempo_bpm(), 140.0);
}

#[test]
fn tempo_bounds_are_5_and_1000() {
    let mut m = new_session();
    assert!(m.set_tempo_bpm(4.999).is_err());
    assert_eq!(m.tempo_bpm(), 120.0);
    assert!(m.set_tempo_bpm(5.0).is_ok());
    assert_eq!(m.tempo_bpm(), 5.0);
    assert!(m.set_tempo_bpm(1000.0).is_ok());
    assert_eq!(m.tempo_bpm(), 1000.0);
    assert!(m.set_tempo_bpm(1000.001).is_err());
    assert_eq!(m.tempo_bpm(), 1000.0);
}

#[test]
fn a_bad_tempo_is_refused_with_the_range_and_changes_nothing() {
    let mut m = new_session();
    for bpm in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -120.0] {
        let text = message(m.set_tempo_bpm(bpm));
        assert!(text.contains("5 to 1000 BPM"), "{text}");
        assert_eq!(m.tempo_bpm(), 120.0);
    }
    assert_eq!(message(m.set_tempo_bpm(2000.0)), "a tempo must be 5 to 1000 BPM, not 2000");
}

#[test]
fn set_tempo_changes_only_the_opening_tempo() {
    let mut m = new_session();
    m.session.tempo.set_tempo(960, 90.0).unwrap();
    assert_eq!(m.set_tempo_bpm(100.0), Ok(120.0));
    assert_eq!(m.session.tempo.tempo_at_tick(0), 100.0);
    assert_eq!(m.session.tempo.tempo_at_tick(960), 90.0);
    assert_eq!(m.session.tempo.tempos().len(), 2);
}

#[test]
fn tempo_may_change_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    assert_eq!(m.set_tempo_bpm(90.0), Ok(120.0));
    assert_eq!(m.tempo_bpm(), 90.0);
}

// ---- time signature ----

#[test]
fn the_default_time_signature_is_4_4() {
    assert_eq!(new_session().time_signature(), (4, 4));
}

#[test]
fn set_time_signature_returns_the_old_one() {
    let mut m = new_session();
    assert_eq!(m.set_time_signature(3, 4), Ok((4, 4)));
    assert_eq!(m.time_signature(), (3, 4));
    assert_eq!(m.set_time_signature(6, 8), Ok((3, 4)));
    assert_eq!(m.time_signature(), (6, 8));
}

#[test]
fn the_beat_count_is_1_to_64() {
    let mut m = new_session();
    assert!(m.set_time_signature(0, 4).is_err());
    assert_eq!(m.time_signature(), (4, 4));
    assert!(m.set_time_signature(1, 4).is_ok());
    assert_eq!(m.time_signature(), (1, 4));
    assert!(m.set_time_signature(64, 4).is_ok());
    assert_eq!(m.time_signature(), (64, 4));
    assert!(m.set_time_signature(65, 4).is_err());
    assert_eq!(m.time_signature(), (64, 4));
}

#[test]
fn the_note_value_is_a_power_of_two_up_to_64() {
    let mut m = new_session();
    for d in [1, 2, 4, 8, 16, 32, 64] {
        assert!(m.set_time_signature(3, d).is_ok(), "{d}");
        assert_eq!(m.time_signature(), (3, d));
    }
    for d in [0, 3, 5, 6, 7, 9, 12, 128] {
        assert!(m.set_time_signature(3, d).is_err(), "{d}");
        assert_eq!(m.time_signature(), (3, 64));
    }
}

#[test]
fn a_bad_time_signature_is_refused_with_the_rule_and_changes_nothing() {
    let mut m = new_session();
    let text = message(m.set_time_signature(65, 4));
    assert_eq!(text, "a time signature needs 1 to 64 beats over a note value of 1, 2, 4, 8, 16, 32 or 64, not 65/4");
    let text = message(m.set_time_signature(4, 3));
    assert!(text.ends_with("not 4/3"), "{text}");
    assert_eq!(m.time_signature(), (4, 4));
}

#[test]
fn set_time_signature_keeps_later_meter_changes() {
    let mut m = new_session();
    m.session.tempo.set_meter(3840, 7, 8).unwrap();
    assert_eq!(m.set_time_signature(3, 4), Ok((4, 4)));
    assert_eq!(m.time_signature(), (3, 4));
    let meters = m.session.tempo.meters();
    assert_eq!(meters.len(), 2);
    assert_eq!((meters[1].numerator, meters[1].denominator), (7, 8));
}

#[test]
fn time_signature_may_change_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    assert_eq!(m.set_time_signature(5, 4), Ok((4, 4)));
    assert_eq!(m.time_signature(), (5, 4));
}

// ---- sample rate ----

#[test]
fn rate_changes_on_an_empty_session_and_returns_the_old_rate() {
    let mut m = new_session();
    m.add_track(Some("A"), 1).unwrap();
    assert_eq!(m.set_rate(44_100), Ok(48_000));
    assert_eq!(m.rate(), 44_100);
    assert_eq!(m.session.sample_rate.hz(), 44_100);
    assert_eq!(m.set_rate(96_000), Ok(44_100));
    assert_eq!(m.rate(), 96_000);
}

#[test]
fn rate_bounds_are_8000_and_768000() {
    let mut m = new_session();
    assert!(m.set_rate(7_999).is_err());
    assert_eq!(m.rate(), 48_000);
    assert!(m.set_rate(8_000).is_ok());
    assert_eq!(m.rate(), 8_000);
    assert!(m.set_rate(768_000).is_ok());
    assert_eq!(m.rate(), 768_000);
    assert!(m.set_rate(768_001).is_err());
    assert_eq!(m.rate(), 768_000);
    assert!(m.set_rate(0).is_err());
    assert!(m.set_rate(u32::MAX).is_err());
    assert_eq!(m.rate(), 768_000);
}

#[test]
fn a_bad_rate_is_refused_with_the_range() {
    let mut m = new_session();
    assert_eq!(message(m.set_rate(100)), "a session's sample rate must be 8000 to 768000 Hz, not 100");
    assert_eq!(m.rate(), 48_000);
}

#[test]
fn rate_is_refused_while_the_session_holds_a_clip() {
    let (mut m, _) = with_clip();
    let text = message(m.set_rate(44_100));
    assert_eq!(
        text,
        "the session already holds audio, so its sample rate cannot change from 48000 Hz to 44100 Hz without retiming it; remove every clip and source first"
    );
    assert_eq!(m.rate(), 48_000);
}

#[test]
fn rate_is_refused_for_a_source_alone_and_for_a_clip_alone() {
    let (mut m, t) = with_clip();
    m.session.track_mut(t).unwrap().playlist_mut().unwrap().clips.clear();
    assert!(m.set_rate(44_100).is_err());
    assert_eq!(m.rate(), 48_000);
    let (mut m, _) = with_clip();
    m.session.sources.clear();
    assert!(m.set_rate(44_100).is_err());
    assert_eq!(m.rate(), 48_000);
}

#[test]
fn the_same_rate_is_accepted_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    assert_eq!(m.set_rate(48_000), Ok(48_000));
    assert_eq!(m.rate(), 48_000);
    assert!(m.holds_audio());
}

#[test]
fn a_bad_rate_on_a_session_with_audio_gets_the_range_message() {
    let (mut m, _) = with_clip();
    let text = message(m.set_rate(1));
    assert!(text.contains("8000 to 768000 Hz"), "{text}");
}

#[test]
fn rate_changes_again_once_the_audio_is_removed() {
    let (mut m, t) = with_clip();
    assert!(m.set_rate(44_100).is_err());
    m.remove_track(t).unwrap();
    assert_eq!(m.set_rate(44_100), Ok(48_000));
    assert_eq!(m.rate(), 44_100);
}

#[test]
fn changing_the_rate_leaves_the_other_properties_alone() {
    let mut m = new_session();
    m.set_tempo_bpm(100.0).unwrap();
    m.set_time_signature(3, 4).unwrap();
    m.set_rate(96_000).unwrap();
    assert_eq!(m.name(), "Mix");
    assert_eq!(m.tempo_bpm(), 100.0);
    assert_eq!(m.time_signature(), (3, 4));
    assert_eq!(bits_of(m.session.bit_depth), 24);
}

// ---- bit depth ----

#[test]
fn bit_depth_changes_on_an_empty_session_and_returns_the_old_one() {
    let mut m = new_session();
    m.add_track(Some("A"), 1).unwrap();
    assert_eq!(bits_of(m.session.bit_depth), 24);
    assert_eq!(m.set_bit_depth(16), Ok(24));
    assert_eq!(bits_of(m.session.bit_depth), 16);
    assert_eq!(m.set_bit_depth(32), Ok(16));
    assert_eq!(bits_of(m.session.bit_depth), 32);
    assert_eq!(m.set_bit_depth(24), Ok(32));
    assert_eq!(bits_of(m.session.bit_depth), 24);
}

#[test]
fn a_bad_bit_depth_is_refused_with_the_choices_and_changes_nothing() {
    let mut m = new_session();
    for bits in [0, 8, 12, 15, 17, 20, 23, 25, 31, 33, 64] {
        let text = message(m.set_bit_depth(bits));
        assert_eq!(text, format!("a bit depth must be 16, 24 or 32 (float), not {bits}"));
        assert_eq!(bits_of(m.session.bit_depth), 24);
    }
}

#[test]
fn bit_depth_is_refused_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    let text = message(m.set_bit_depth(16));
    assert_eq!(text, "the session already holds audio, so its bit depth cannot change from 24 to 16 bits; remove every clip and source first");
    assert_eq!(bits_of(m.session.bit_depth), 24);
}

#[test]
fn bit_depth_is_refused_for_a_source_alone_and_for_a_clip_alone() {
    let (mut m, t) = with_clip();
    m.session.track_mut(t).unwrap().playlist_mut().unwrap().clips.clear();
    assert!(m.set_bit_depth(16).is_err());
    let (mut m, _) = with_clip();
    m.session.sources.clear();
    assert!(m.set_bit_depth(16).is_err());
    assert_eq!(bits_of(m.session.bit_depth), 24);
}

#[test]
fn the_same_bit_depth_is_accepted_while_the_session_holds_audio() {
    let (mut m, _) = with_clip();
    assert_eq!(m.set_bit_depth(24), Ok(24));
    assert_eq!(bits_of(m.session.bit_depth), 24);
}

#[test]
fn a_bad_bit_depth_on_a_session_with_audio_gets_the_choices_message() {
    let (mut m, _) = with_clip();
    let text = message(m.set_bit_depth(20));
    assert!(text.contains("16, 24 or 32 (float)"), "{text}");
}

#[test]
fn bit_depth_changes_again_once_the_audio_is_removed() {
    let (mut m, t) = with_clip();
    assert!(m.set_bit_depth(16).is_err());
    m.remove_track(t).unwrap();
    assert_eq!(m.set_bit_depth(16), Ok(24));
    assert_eq!(bits_of(m.session.bit_depth), 16);
}
