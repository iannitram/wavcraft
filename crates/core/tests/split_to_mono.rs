//! C111: split a stereo track into two mono tracks (`Multitrack::split_to_mono`).

use soundcraft_model::{BitDepthSetting, ChannelFormat, Clip, ClipContent, MAX_NAME_CHARS, Playlist, SourceId, Track, TrackId, TrackKind};
use soundcraft_model_audio::AudioBuffer;
use wavcraft_core::multitrack::Multitrack;
use wavcraft_core::multitrack::split::split_name;

const RATE: u32 = 48_000;

const A_LEFT: [f32; 4] = [0.5, 0.25, -0.5, 0.125];
const A_RIGHT: [f32; 4] = [-0.25, 1.0, 0.0, -0.125];
const B_LEFT: [f32; 2] = [0.75, 0.5];
const B_RIGHT: [f32; 2] = [0.5, 0.75];

fn stereo(left: &[f32], right: &[f32]) -> AudioBuffer {
    AudioBuffer { sample_rate: RATE, channels: vec![left.to_vec(), right.to_vec()] }
}

fn new_session() -> Multitrack {
    Multitrack::new("Split", RATE, BitDepthSetting::Int24).unwrap()
}

/// A stereo track "Drums" with clip "Kick" at 0 s and clip "Snare" at 1 s.
fn drums(m: &mut Multitrack) -> TrackId {
    let t = m.add_track_format(Some("Drums"), ChannelFormat::Stereo).unwrap();
    m.add_clip_buffer(t, "Kick", "kick.wav", stereo(&A_LEFT, &A_RIGHT), 0.0).unwrap();
    m.add_clip_buffer(t, "Snare", "snare.wav", stereo(&B_LEFT, &B_RIGHT), 1.0).unwrap();
    t
}

fn track(m: &Multitrack, id: TrackId) -> &Track {
    m.session.track(id).unwrap()
}

fn names(m: &Multitrack) -> Vec<String> {
    m.session.tracks.iter().map(|t| t.name.clone()).collect()
}

fn source_of(c: &Clip) -> SourceId {
    c.source().unwrap()
}

fn offset_of(c: &Clip) -> i64 {
    match c.content {
        ClipContent::Audio { offset, .. } => offset,
        _ => -1,
    }
}

/// The channels of the audio a clip plays.
fn audio_of(m: &Multitrack, c: &Clip) -> Vec<Vec<f32>> {
    m.session.pool.get(source_of(c)).unwrap().buffer.channels.clone()
}

/// What an edit that is refused must not change.
fn snapshot(m: &Multitrack) -> (String, usize, usize) {
    (m.state_json(), m.session.sources.len(), m.session.pool.len())
}

#[test]
fn each_channel_goes_to_its_own_mono_track() {
    let mut m = new_session();
    let t = drums(&mut m);
    let (l, r) = m.split_to_mono(t).unwrap();
    let (lt, rt) = (track(&m, l), track(&m, r));
    assert_eq!((lt.clips().len(), rt.clips().len()), (2, 2));
    assert_eq!(audio_of(&m, &lt.clips()[0]), vec![A_LEFT.to_vec()]);
    assert_eq!(audio_of(&m, &rt.clips()[0]), vec![A_RIGHT.to_vec()]);
    assert_eq!(audio_of(&m, &lt.clips()[1]), vec![B_LEFT.to_vec()]);
    assert_eq!(audio_of(&m, &rt.clips()[1]), vec![B_RIGHT.to_vec()]);
    assert_eq!(m.session.pool.get(source_of(&lt.clips()[0])).unwrap().buffer.sample_rate, RATE);
    assert_eq!((lt.format, rt.format), (ChannelFormat::Mono, ChannelFormat::Mono));
    assert_eq!((lt.kind, rt.kind), (TrackKind::Audio, TrackKind::Audio));
}

#[test]
fn the_pair_takes_the_place_of_the_stereo_track_and_is_named_from_it() {
    let mut m = new_session();
    let intro = m.add_track_format(Some("Intro"), ChannelFormat::Mono).unwrap();
    m.add_clip_buffer(intro, "Hello", "hello.wav", AudioBuffer { sample_rate: RATE, channels: vec![vec![0.5, -0.5]] }, 0.5).unwrap();
    let t = drums(&mut m);
    let outro = m.add_track_format(Some("Outro"), ChannelFormat::Stereo).unwrap();
    m.add_clip_buffer(outro, "Bye", "bye.wav", stereo(&B_LEFT, &B_RIGHT), 2.0).unwrap();
    let (intro_before, outro_before) = (track(&m, intro).clone(), track(&m, outro).clone());

    let (l, r) = m.split_to_mono(t).unwrap();

    assert_eq!(names(&m), ["Intro", "Drums L", "Drums R", "Outro"]);
    assert_eq!((m.session.tracks[1].id, m.session.tracks[2].id), (l, r));
    assert!(l != r && l != t && r != t);
    assert!(m.session.track(t).is_none());
    assert_eq!(track(&m, intro), &intro_before);
    assert_eq!(track(&m, outro), &outro_before);
    assert_eq!(m.track_count(), 4);
}

#[test]
fn clips_keep_their_timing_and_settings() {
    let mut m = new_session();
    let t = drums(&mut m);
    {
        let tr = m.session.track_mut(t).unwrap();
        let pl = tr.playlist_mut().unwrap();
        pl.clips[0].gain_db = -3.0;
        pl.clips[0].color = Some([1, 2, 3]);
        pl.clips[0].rating = 4;
        pl.clips[1].length = 1;
        let src = source_of(&pl.clips[1]);
        pl.clips[1].content = ClipContent::Audio { source: src, offset: 1 };
    }
    let before: Vec<Clip> = track(&m, t).clips().to_vec();
    let (l, r) = m.split_to_mono(t).unwrap();
    for id in [l, r] {
        let clips = track(&m, id).clips();
        assert_eq!(clips.len(), 2);
        for (new, old) in clips.iter().zip(&before) {
            assert_eq!(new.name, old.name);
            assert_eq!((new.start, new.length), (old.start, old.length));
            assert_eq!((new.gain_db, new.color, new.rating), (old.gain_db, old.color, old.rating));
            assert_eq!(offset_of(new), offset_of(old));
            assert!(new.id != old.id);
        }
        assert_eq!((clips[0].start, clips[0].length), (0, 4));
        assert_eq!((clips[1].start, clips[1].length), (48_000, 1));
        assert_eq!(offset_of(&clips[1]), 1);
        assert_eq!((clips[0].gain_db, clips[0].color, clips[0].rating), (-3.0, Some([1, 2, 3]), 4));
    }
    let ids: Vec<_> = [l, r].iter().flat_map(|i| track(&m, *i).clips().iter().map(|c| c.id)).collect();
    assert!((0..ids.len()).all(|i| (i + 1..ids.len()).all(|j| ids[i] != ids[j])));
    assert_eq!(m.clip_count(), 4);
}

#[test]
fn the_mixer_settings_are_copied_except_pan_and_record_arm() {
    let mut m = new_session();
    let t = drums(&mut m);
    {
        let tr = m.session.track_mut(t).unwrap();
        tr.mixer.volume_db = -6.0;
        tr.mixer.trim_db = 2.5;
        tr.mixer.mute = true;
        tr.mixer.solo = true;
        tr.mixer.record_arm = true;
        tr.mixer.input_monitor = true;
        tr.color = [10, 20, 30];
        tr.comments = "take 3".to_string();
    }
    let (l, r) = m.split_to_mono(t).unwrap();
    for id in [l, r] {
        let tr = track(&m, id);
        assert_eq!((tr.mixer.volume_db, tr.mixer.trim_db), (-6.0, 2.5));
        assert_eq!((tr.mixer.mute, tr.mixer.solo), (true, true));
        assert_eq!((tr.mixer.record_arm, tr.mixer.input_monitor), (false, false));
        assert_eq!(tr.color, [10, 20, 30]);
        assert_eq!(tr.comments, "take 3");
    }
    assert_eq!(track(&m, l).mixer.pan, vec![-1.0]);
    assert_eq!(track(&m, r).mixer.pan, vec![1.0]);
}

#[test]
fn each_source_becomes_two_mono_sources_and_the_old_one_goes() {
    let mut m = new_session();
    let t = drums(&mut m);
    let old: Vec<SourceId> = track(&m, t).clips().iter().map(source_of).collect();
    assert_eq!((m.session.sources.len(), m.session.pool.len()), (2, 2));
    let (l, r) = m.split_to_mono(t).unwrap();
    assert_eq!((m.session.sources.len(), m.session.pool.len()), (4, 4));
    for o in &old {
        assert!(m.session.source(*o).is_none());
        assert!(!m.session.pool.contains(*o));
    }
    let kick_l = m.session.source(source_of(&track(&m, l).clips()[0])).unwrap();
    let kick_r = m.session.source(source_of(&track(&m, r).clips()[0])).unwrap();
    assert_eq!((kick_l.name.as_str(), kick_l.path.as_str()), ("Kick L", "Kick L.wav"));
    assert_eq!((kick_r.name.as_str(), kick_r.path.as_str()), ("Kick R", "Kick R.wav"));
    assert!(kick_l.id != kick_r.id);
    for s in [kick_l, kick_r] {
        assert_eq!((s.channels, s.frames, s.sample_rate), (1, 4, RATE));
        assert!(s.unsaved);
        assert_eq!(s.format, soundcraft_model_audio::FileFormat::Wav);
        assert!(m.session.pool.contains(s.id));
    }
    let snare_l = m.session.source(source_of(&track(&m, l).clips()[1])).unwrap();
    assert_eq!((snare_l.name.as_str(), snare_l.frames), ("Snare L", 2));
    assert!(m.session.clips_using(snare_l.id).next().is_some());
}

#[test]
fn clips_that_shared_a_source_still_share_and_every_playlist_is_split() {
    let mut m = new_session();
    let t = drums(&mut m);
    {
        let tr = m.session.track_mut(t).unwrap();
        let mut again = tr.clips()[0].clone();
        again.start = 96_000;
        tr.playlist_mut().unwrap().clips.push(again);
        let mut take = Playlist::new("Take 2");
        take.clips.push(tr.clips()[1].clone());
        tr.playlists.push(take);
    }
    let fresh = m.session.new_clip_id();
    let fresh2 = m.session.new_clip_id();
    {
        let tr = m.session.track_mut(t).unwrap();
        tr.playlists[0].clips[2].id = fresh;
        tr.playlists[1].clips[0].id = fresh2;
    }
    let (l, r) = m.split_to_mono(t).unwrap();
    assert_eq!((m.session.sources.len(), m.session.pool.len()), (4, 4));
    for (id, kick, snare) in [(l, A_LEFT.to_vec(), B_LEFT.to_vec()), (r, A_RIGHT.to_vec(), B_RIGHT.to_vec())] {
        let tr = track(&m, id);
        assert_eq!(tr.playlists.len(), 2);
        assert_eq!(tr.playlists[0].name, "Drums");
        assert_eq!(tr.playlists[1].name, "Take 2");
        let c = tr.playlists[0].clips.clone();
        assert_eq!(c.len(), 3);
        assert_eq!(source_of(&c[0]), source_of(&c[2]));
        assert_eq!(audio_of(&m, &c[2]), vec![kick]);
        assert_eq!(c[2].start, 96_000);
        assert_eq!(tr.playlists[1].clips.len(), 1);
        assert_eq!(audio_of(&m, &tr.playlists[1].clips[0]), vec![snare]);
        assert_eq!(source_of(&tr.playlists[1].clips[0]), source_of(&c[1]));
    }
}

#[test]
fn an_empty_stereo_track_splits_into_two_empty_mono_tracks() {
    let mut m = new_session();
    let t = m.add_track_format(Some("Bed"), ChannelFormat::Stereo).unwrap();
    let (l, r) = m.split_to_mono(t).unwrap();
    assert_eq!(names(&m), ["Bed L", "Bed R"]);
    assert_eq!((track(&m, l).clips().len(), track(&m, r).clips().len()), (0, 0));
    assert_eq!((m.session.sources.len(), m.session.pool.len()), (0, 0));
}

#[test]
fn an_unknown_track_a_mono_track_and_a_master_are_refused_and_nothing_changes() {
    let mut m = new_session();
    let _ = drums(&mut m);
    let voice = m.add_track_format(Some("Voice"), ChannelFormat::Mono).unwrap();
    let master = m.session.add_track(TrackKind::Master, ChannelFormat::Stereo, None);
    let before = snapshot(&m);
    let tracks_before = m.session.tracks.clone();

    let e = m.split_to_mono(TrackId(987_654)).unwrap_err();
    assert!(e.0.contains("no such track"), "{e}");
    let e = m.split_to_mono(voice).unwrap_err();
    assert!(e.0.contains("only a stereo track can be split") && e.0.contains("Voice"), "{e}");
    let e = m.split_to_mono(master).unwrap_err();
    assert!(e.0.contains("is not an audio track"), "{e}");

    assert_eq!(snapshot(&m), before);
    assert_eq!(m.session.tracks, tracks_before);
}

#[test]
fn a_name_another_track_already_has_is_refused_for_either_side_in_any_case() {
    for taken in ["Drums L", "drums r"] {
        let mut m = new_session();
        let t = drums(&mut m);
        m.add_track_format(Some(taken), ChannelFormat::Mono).unwrap();
        let before = snapshot(&m);
        let e = m.split_to_mono(t).unwrap_err();
        assert!(e.0.contains("already a track named"), "{taken}: {e}");
        assert_eq!(snapshot(&m), before, "{taken}");
    }
}

#[test]
fn clip_audio_that_is_missing_or_not_two_channels_is_refused_and_nothing_changes() {
    for mono_instead in [false, true] {
        let mut m = new_session();
        let t = drums(&mut m);
        let second = source_of(&track(&m, t).clips()[1]);
        m.session.pool.remove(second);
        if mono_instead {
            let one = AudioBuffer { sample_rate: RATE, channels: vec![vec![0.5, 0.5]] };
            m.session.pool.insert(second, std::sync::Arc::new(soundcraft_model::SourceAudio::new(one)));
        }
        let before = snapshot(&m);
        let tracks_before = m.session.tracks.clone();
        let e = m.split_to_mono(t).unwrap_err();
        assert!(e.0.contains("two channels"), "{mono_instead}: {e}");
        assert_eq!(snapshot(&m), before);
        assert_eq!(m.session.tracks, tracks_before);
    }
}

#[test]
fn a_long_name_is_cut_to_fit_and_counted_in_characters() {
    // Exactly at the limit: 254 characters plus " L" is 256. One more character is cut.
    let at = "a".repeat(MAX_NAME_CHARS - 2);
    let over = "a".repeat(MAX_NAME_CHARS - 1);
    assert_eq!(split_name(&at, 'L'), format!("{at} L"));
    assert_eq!(split_name(&over, 'L'), format!("{at} L"));
    assert_eq!(split_name(&"a".repeat(MAX_NAME_CHARS), 'R'), format!("{at} R"));
    assert_eq!(split_name("Drums", 'R'), "Drums R");
    assert_eq!(split_name(&at, 'L').chars().count(), MAX_NAME_CHARS);
    let wide = "\u{e9}".repeat(MAX_NAME_CHARS);
    let cut = split_name(&wide, 'L');
    assert_eq!(cut.chars().count(), MAX_NAME_CHARS);
    assert!(cut.ends_with("\u{e9} L"));

    let mut m = new_session();
    let t = m.add_track_format(Some(&over), ChannelFormat::Stereo).unwrap();
    m.split_to_mono(t).unwrap();
    assert_eq!(names(&m), [format!("{at} L"), format!("{at} R")]);
}
