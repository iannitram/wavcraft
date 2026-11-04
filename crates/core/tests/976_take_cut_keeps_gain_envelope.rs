//! #976: the head and tail kept from a clip that a take covers in part play the gain the old clip
//! played at every point (the gain envelope's value at the cut is kept), punched or not.

use std::path::{Path, PathBuf};

use soundcraft_model::time::Range;
use soundcraft_model::{BitDepthSetting, Clip, ClipContent, TrackId};
use wavcraft_core::mtrecord::{TakeWriter, fake_samples};
use wavcraft_core::multitrack::Multitrack;

const TOL: f32 = 1e-5;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wavcraft-976-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_take(path: &Path, frames: usize) {
    let mut w = TakeWriter::create(path, 1, 48_000).unwrap();
    w.write(&[fake_samples(0, 0, frames, 48_000)]).unwrap();
    w.finish().unwrap();
}

/// A mono track holding one 48 000-frame clip at 0 with this gain envelope, and the old clip.
fn session_with(env: Vec<(i64, f32)>, dir: &Path) -> (Multitrack, TrackId, Clip) {
    let mut m = Multitrack::new("Rec", 48_000, BitDepthSetting::Int24).unwrap();
    let a = m.add_track(Some("Vox"), 1).unwrap();
    let p1 = dir.join("p1.wav");
    write_take(&p1, 48_000);
    m.place_take(a, 0, &p1).unwrap();
    m.session.track_mut(a).and_then(|t| t.playlist_mut()).unwrap().clips[0].gain_env = env;
    let old = m.session.track(a).unwrap().clips()[0].clone();
    (m, a, old)
}

fn clips(m: &Multitrack, a: TrackId) -> Vec<Clip> {
    let mut c = m.session.track(a).unwrap().clips().to_vec();
    c.sort_by_key(|c| c.start);
    c
}

/// Head and tail play what `old` played over the part each one kept.
fn assert_same_gain(c: &[Clip], old: &Clip) {
    assert_eq!(c.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(0, 12_000), (12_000, 12_000), (24_000, 24_000)]);
    for t in (0..12_000).step_by(1_000) {
        let (got, want) = (c[0].gain_at(t), old.gain_at(t));
        assert!((got - want).abs() < TOL, "head at {t}: {got} against the old clip's {want}");
    }
    for t in (0..24_000).step_by(1_000) {
        let (got, want) = (c[2].gain_at(t), old.gain_at(24_000 + t));
        assert!((got - want).abs() < TOL, "tail at {t}: {got} against the old clip's {want}");
    }
}

#[test]
fn a_punched_take_leaves_head_and_tail_playing_the_old_ramp() {
    let dir = scratch("ramp");
    let (mut m, a, old) = session_with(vec![(0, 0.0), (48_000, -12.0)], &dir);
    let p2 = dir.join("p2.wav");
    write_take(&p2, 48_000);
    m.place_take_in(a, 0, &p2, Some(Range::new(12_000, 24_000))).unwrap();
    assert_same_gain(&clips(&m, a), &old);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tail_after_a_punch_keeps_the_minus_12_db_the_old_clip_had_there() {
    let dir = scratch("early");
    let (mut m, a, old) = session_with(vec![(0, 0.0), (6_000, -12.0)], &dir);
    let p2 = dir.join("p2.wav");
    write_take(&p2, 48_000);
    m.place_take_in(a, 0, &p2, Some(Range::new(12_000, 24_000))).unwrap();
    let c = clips(&m, a);
    let want = 10f32.powf(-12.0 / 20.0);
    assert!((c[2].gain_at(0) - want).abs() < TOL, "tail gain at its first frame: {} against {want}", c[2].gain_at(0));
    assert_same_gain(&c, &old);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unpunched_take_over_the_middle_leaves_head_and_tail_playing_the_old_ramp() {
    let dir = scratch("plain");
    let (mut m, a, old) = session_with(vec![(0, 0.0), (48_000, -12.0)], &dir);
    let p2 = dir.join("p2.wav");
    write_take(&p2, 12_000);
    m.place_take(a, 12_000, &p2).unwrap();
    assert_same_gain(&clips(&m, a), &old);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_kept_parts_keep_their_source_offsets_and_get_their_own_ids() {
    let dir = scratch("offsets");
    let (mut m, a, _) = session_with(vec![(0, 0.0), (48_000, -12.0)], &dir);
    let p2 = dir.join("p2.wav");
    write_take(&p2, 12_000);
    m.place_take(a, 12_000, &p2).unwrap();
    let c = clips(&m, a);
    let offset = |c: &Clip| match &c.content {
        ClipContent::Audio { offset, .. } => *offset,
        other => panic!("{other:?}"),
    };
    assert_eq!((offset(&c[0]), offset(&c[2])), (0, 24_000), "head and tail play the audio the old clip did");
    let ids: std::collections::HashSet<_> = c.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), c.len(), "every clip has its own id");
    let _ = std::fs::remove_dir_all(&dir);
}
