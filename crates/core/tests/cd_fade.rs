//! C245: track fades for the CD layout (core only). Fade lengths and their limits, the linear
//! gain, applying fades to per-channel buffers (bit-identical when there are none), and where
//! each track starts with the pauses counted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wavcraft_core::cd::fade::{MAX_FADE_SECS, TrackFade, apply_fades, check_fit, fade_frames, fade_gain, track_starts};
use wavcraft_core::cd::{CdLayout, SAMPLES_PER_FRAME};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-cdfade-test-{}-{name}", std::process::id()));
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

/// A document of `frames` frames at `rate` with `channels` channels, each a constant `level`.
fn doc(dir: &Path, rate: u32, channels: usize, frames: usize, level: f32) -> Arc<DocState> {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 16 << 20, ..StoreOptions::default() }).unwrap();
    let data = vec![level; frames];
    let chans = (0..channels).map(|_| Channel::from_pieces(data.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    Arc::new(DocState::new(s.clone(), chans, rate, SourceFormat::synthetic(rate, channels as u16)).unwrap())
}

fn fade(i: f64, o: f64) -> TrackFade {
    TrackFade::new(i, o).unwrap()
}

fn bits(chs: &[Vec<f32>]) -> Vec<Vec<u32>> {
    chs.iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect()
}

fn near(got: &[f32], want: &[f32]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() < 1e-6, "frame {i}: {g} vs {w} in {got:?}");
    }
}

fn err_text<T>(r: Result<T, wavcraft_core::cd::CdError>) -> String {
    match r {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    }
}

#[test]
fn new_accepts_the_edges_of_the_range() {
    assert_eq!(MAX_FADE_SECS, 30.0);
    assert_eq!(TrackFade::new(0.0, 0.0).unwrap(), TrackFade::NONE);
    let f = TrackFade::new(30.0, 30.0).unwrap();
    assert_eq!((f.fade_in_secs, f.fade_out_secs), (30.0, 30.0));
    let g = TrackFade::new(0.5, 29.999).unwrap();
    assert_eq!((g.fade_in_secs, g.fade_out_secs), (0.5, 29.999));
}

#[test]
fn new_refuses_out_of_range_and_names_the_fade() {
    for bad in [-0.001, 30.001, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let e = err_text(TrackFade::new(bad, 1.0));
        assert!(e.contains("fade-in") && e.contains("0 to 30 seconds"), "{bad}: {e:?}");
        let e = err_text(TrackFade::new(1.0, bad));
        assert!(e.contains("fade-out") && e.contains("0 to 30 seconds"), "{bad}: {e:?}");
    }
    let e = err_text(TrackFade::new(31.0, -1.0));
    assert!(e.contains("fade-in") && !e.contains("fade-out"), "the fade-in is checked first: {e:?}");
}

#[test]
fn none_is_the_default_and_is_none_looks_at_both_lengths() {
    assert_eq!(TrackFade::default(), TrackFade::NONE);
    assert!(TrackFade::NONE.is_none());
    assert!(fade(0.0, 0.0).is_none());
    assert!(!fade(0.001, 0.0).is_none());
    assert!(!fade(0.0, 0.001).is_none());
    assert!(!fade(2.0, 3.0).is_none());
}

#[test]
fn fade_frames_rounds_to_the_nearest_frame_at_the_rate() {
    assert_eq!(fade_frames(1.0, 44_100), 44_100);
    assert_eq!(fade_frames(1.0, 48_000), 48_000);
    assert_eq!(fade_frames(0.5, 1000), 500);
    assert_eq!(fade_frames(2.5, 48_000), 120_000);
    assert_eq!(fade_frames(0.0004, 1000), 0, "0.4 frame rounds down");
    assert_eq!(fade_frames(0.0006, 1000), 1, "0.6 frame rounds up");
    assert_eq!(fade_frames(0.0, 44_100), 0);
}

#[test]
fn fade_gain_is_linear_from_zero_and_never_above_one() {
    let want = [0.0, 0.25, 0.5, 0.75, 1.0];
    for (i, w) in want.iter().enumerate() {
        assert_eq!(fade_gain(i as u64, 4), *w, "frame {i} of 4");
    }
    assert_eq!(fade_gain(9, 4), 1.0, "past the end of the fade");
    assert_eq!(fade_gain(0, 0), 1.0, "a fade of 0 frames is full gain");
    assert_eq!(fade_gain(5, 0), 1.0);
    assert_eq!(fade_gain(1, 1), 1.0);
    assert_eq!(fade_gain(0, 1), 0.0);
}

#[test]
fn check_fit_allows_up_to_half_the_track_and_no_more() {
    // 1000 frames at 1000 Hz: half is 500 frames. Test at n-1, n and n+1.
    for (secs, ok) in [(0.499, true), (0.5, true), (0.501, false)] {
        let (i, o) = (check_fit(&fade(secs, 0.0), 1000, 1000), check_fit(&fade(0.0, secs), 1000, 1000));
        assert_eq!((i.is_ok(), o.is_ok()), (ok, ok), "{secs} s");
        if !ok {
            let (ei, eo) = (err_text(i), err_text(o));
            assert!(ei.contains("fade-in") && ei.contains("longer than half"), "{ei:?}");
            assert!(eo.contains("fade-out") && eo.contains("longer than half"), "{eo:?}");
        }
    }
    assert!(check_fit(&fade(0.5, 0.5), 1000, 1000).is_ok(), "both at half");
    // An odd length rounds the half down: 1001 frames -> 500.
    assert!(check_fit(&fade(0.5, 0.5), 1001, 1000).is_ok());
    assert!(check_fit(&fade(0.501, 0.0), 1001, 1000).is_err());
    assert!(check_fit(&fade(0.0, 0.501), 1001, 1000).is_err());
    // Tiny and empty tracks.
    assert!(check_fit(&TrackFade::NONE, 0, 1000).is_ok());
    assert!(check_fit(&fade(0.001, 0.0), 1, 1000).is_err(), "one frame of fade needs two frames of track");
    assert!(check_fit(&fade(0.001, 0.0), 2, 1000).is_ok());
    assert!(check_fit(&fade(0.0, 0.001), 1, 1000).is_err());
    // Both too long: the fade-in is reported.
    let e = err_text(check_fit(&fade(0.9, 0.9), 1000, 1000));
    assert!(e.contains("fade-in"), "{e:?}");
}

#[test]
fn check_fit_refuses_a_zero_rate() {
    for f in [TrackFade::NONE, fade(0.1, 0.1)] {
        let e = err_text(check_fit(&f, 1000, 0));
        assert!(e.contains("sample rate must be above 0"), "{e:?}");
    }
}

#[test]
fn no_fade_leaves_every_sample_bit_identical() {
    let weird = vec![0.0, -0.0, 1.0, -1.0, f32::NAN, f32::INFINITY, f32::MIN_POSITIVE / 4.0, 1e30, -0.37, 0.123_456_79, 0.5, 0.25];
    let mut chs = vec![weird.clone(), weird.iter().rev().copied().collect()];
    let before = bits(&chs);
    apply_fades(&mut chs, 44_100, &TrackFade::NONE).unwrap();
    assert_eq!(bits(&chs), before);
    apply_fades(&mut chs, 1000, &TrackFade::default()).unwrap();
    assert_eq!(bits(&chs), before);
    // A fade that rounds to 0 frames changes nothing either.
    apply_fades(&mut chs, 1000, &fade(0.0004, 0.0004)).unwrap();
    assert_eq!(bits(&chs), before);
}

#[test]
fn fade_in_scales_the_first_frames_only() {
    let mut chs = vec![vec![1.0f32; 10]];
    apply_fades(&mut chs, 10, &fade(0.4, 0.0)).unwrap();
    near(&chs[0], &[0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn fade_out_scales_the_last_frames_only_and_ends_at_zero() {
    let mut chs = vec![vec![1.0f32; 10]];
    apply_fades(&mut chs, 10, &fade(0.0, 0.4)).unwrap();
    near(&chs[0], &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.75, 0.5, 0.25, 0.0]);
}

#[test]
fn both_fades_scale_the_samples_they_meet() {
    let mut chs = vec![vec![0.8f32; 10]];
    apply_fades(&mut chs, 10, &fade(0.3, 0.2)).unwrap();
    near(&chs[0], &[0.0, 0.8 / 3.0, 1.6 / 3.0, 0.8, 0.8, 0.8, 0.8, 0.8, 0.4, 0.0]);
}

#[test]
fn every_channel_gets_the_same_gains() {
    let mut chs = vec![vec![1.0f32; 10], vec![-0.5f32; 10]];
    apply_fades(&mut chs, 10, &fade(0.4, 0.2)).unwrap();
    near(&chs[0], &[0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0]);
    near(&chs[1], &[-0.0, -0.125, -0.25, -0.375, -0.5, -0.5, -0.5, -0.5, -0.25, -0.0]);
}

#[test]
fn fades_of_half_the_track_each_meet_in_the_middle() {
    let mut chs = vec![vec![1.0f32; 10]];
    apply_fades(&mut chs, 10, &fade(0.5, 0.5)).unwrap();
    near(&chs[0], &[0.0, 0.2, 0.4, 0.6, 0.8, 0.8, 0.6, 0.4, 0.2, 0.0]);
    // An odd length: 11 frames, 5 each, the middle frame stays at full gain.
    let mut odd = vec![vec![1.0f32; 11]];
    apply_fades(&mut odd, 10, &fade(0.5, 0.5)).unwrap();
    near(&odd[0], &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 0.8, 0.6, 0.4, 0.2, 0.0]);
}

#[test]
fn a_fade_longer_than_half_is_refused_and_nothing_changes() {
    let start = vec![vec![1.0f32; 10], vec![0.5f32; 10]];
    // Test at n-1, n and n+1 frames (half of 10 is 5).
    for (secs, ok) in [(0.4, true), (0.5, true), (0.6, false)] {
        for (i, o) in [(secs, 0.0), (0.0, secs), (secs, secs)] {
            let mut chs = start.clone();
            let r = apply_fades(&mut chs, 10, &fade(i, o));
            assert_eq!(r.is_ok(), ok, "in {i} out {o}");
            if !ok {
                assert_eq!(bits(&chs), bits(&start), "nothing changes on a refusal");
                assert!(err_text(r).contains("longer than half"));
            }
        }
    }
    // The fade-out too long after a fine fade-in: still nothing changes.
    let mut chs = start.clone();
    assert!(apply_fades(&mut chs, 10, &fade(0.5, 0.6)).is_err());
    assert_eq!(bits(&chs), bits(&start));
    // 11 frames: half is 5.
    let mut odd = vec![vec![1.0f32; 11]];
    assert!(apply_fades(&mut odd, 10, &fade(0.6, 0.0)).is_err());
    assert!(apply_fades(&mut odd, 10, &fade(0.5, 0.0)).is_ok());
}

#[test]
fn bad_inputs_are_refused_and_nothing_changes() {
    let start = vec![vec![1.0f32; 100_000]];
    let mut chs = start.clone();
    // Fields set by hand around `new`: each is refused with its own name.
    let bad_in = |v: f64| TrackFade { fade_in_secs: v, fade_out_secs: 0.0 };
    let bad_out = |v: f64| TrackFade { fade_in_secs: 0.0, fade_out_secs: v };
    for v in [31.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(err_text(apply_fades(&mut chs, 1000, &bad_in(v))).contains("fade-in"), "{v}");
        assert!(err_text(apply_fades(&mut chs, 1000, &bad_out(v))).contains("fade-out"), "{v}");
    }
    assert_eq!(bits(&chs), bits(&start));
    // Zero rate.
    assert!(err_text(apply_fades(&mut chs, 0, &fade(0.1, 0.1))).contains("sample rate must be above 0"));
    // Channels of different lengths.
    let mut uneven = vec![vec![1.0f32; 10], vec![1.0f32; 9]];
    let e = err_text(apply_fades(&mut uneven, 10, &fade(0.2, 0.2)));
    assert!(e.contains("same length"), "{e:?}");
    assert_eq!(uneven[0], vec![1.0f32; 10]);
    assert_eq!(uneven[1], vec![1.0f32; 9]);
    let mut uneven = vec![vec![1.0f32; 10], vec![1.0f32; 11]];
    assert!(apply_fades(&mut uneven, 10, &TrackFade::NONE).is_err(), "even with no fade");
}

#[test]
fn empty_inputs() {
    let mut none: Vec<Vec<f32>> = Vec::new();
    assert!(apply_fades(&mut none, 44_100, &fade(1.0, 1.0)).is_ok());
    let mut empty = vec![Vec::<f32>::new(), Vec::new()];
    assert!(apply_fades(&mut empty, 1000, &TrackFade::NONE).is_ok());
    assert!(apply_fades(&mut empty, 1000, &fade(0.001, 0.0)).is_err(), "one frame of fade does not fit an empty track");
}

#[test]
fn fade_length_follows_the_sample_rate() {
    for (rate, n) in [(48_000u32, 480usize), (44_100, 441)] {
        let mut chs = vec![vec![1.0f32; 4800]];
        apply_fades(&mut chs, rate, &fade(0.01, 0.0)).unwrap();
        assert_eq!(chs[0][0], 0.0, "{rate}");
        assert_eq!(chs[0][n], 1.0, "{rate}: the fade is {n} frames");
        assert!(chs[0][n - 1] < 1.0, "{rate}");
        assert!((chs[0][n - 1] - (n - 1) as f32 / n as f32).abs() < 1e-6, "{rate}");
        assert_eq!(chs[0][n + 1], 1.0);
    }
}

#[test]
fn track_starts_follow_the_pauses_and_ignore_the_fades() {
    let d = TempDir::new("starts");
    let mut l = CdLayout::default();
    // 225, 76 and 75 CD frames; pauses 2 s (default), 0 and 1 s -> INDEX 01 at 150, 375, 526.
    l.add(doc(&d.0.join("a"), 44_100, 2, 132_300, 0.1), "one");
    l.add(doc(&d.0.join("b"), 44_100, 2, 44_100 + 100, 0.1), "two");
    l.add(doc(&d.0.join("c"), 44_100, 2, 44_100, 0.1), "three");
    l.set_pause(2, 1.0).unwrap();
    let want = vec![150 * SAMPLES_PER_FRAME, 375 * SAMPLES_PER_FRAME, 526 * SAMPLES_PER_FRAME];
    assert_eq!(want, vec![88_200, 220_500, 309_288]);
    assert_eq!(track_starts(&l, &[TrackFade::NONE; 3]).unwrap(), want, "no fades");
    assert_eq!(track_starts(&l, &[fade(1.0, 1.0), TrackFade::NONE, fade(0.5, 0.5)]).unwrap(), want, "fades do not move a start");
    l.set_pause(0, 0.0).unwrap();
    // Pauses now 0, 0 and 1 s: INDEX 01 at 0, 225 and 225 + 76 + 75 = 376 CD frames.
    assert_eq!(track_starts(&l, &[TrackFade::NONE; 3]).unwrap(), vec![0, 132_300, 221_088]);
}

#[test]
fn track_starts_of_an_empty_layout_is_empty() {
    assert_eq!(track_starts(&CdLayout::default(), &[]).unwrap(), Vec::<u64>::new());
    assert!(track_starts(&CdLayout::default(), &[TrackFade::NONE]).is_err());
}

#[test]
fn track_starts_needs_one_fade_per_track() {
    let d = TempDir::new("count");
    let mut l = CdLayout::default();
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.1), "one");
    l.add(doc(&d.0.join("b"), 44_100, 2, 44_100, 0.1), "two");
    let e = err_text(track_starts(&l, &[TrackFade::NONE]));
    assert!(e.contains("1 fades for 2 tracks"), "{e:?}");
    let e = err_text(track_starts(&l, &[TrackFade::NONE; 3]));
    assert!(e.contains("3 fades for 2 tracks"), "{e:?}");
    assert!(track_starts(&l, &[]).is_err());
    assert!(track_starts(&l, &[TrackFade::NONE; 2]).is_ok());
}

#[test]
fn track_starts_checks_each_fade_against_its_own_track_at_44100() {
    let d = TempDir::new("fit");
    let mut l = CdLayout::default();
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.1), "one"); // 44100 samples: half is 22050
    l.add(doc(&d.0.join("b"), 22_050, 1, 22_050, 0.1), "two"); // 1 s at 22.05 kHz is 44100 at 44.1 kHz
    l.add(doc(&d.0.join("c"), 44_100, 2, 44_100 + 100, 0.1), "three"); // 44200 samples (not padded to 45 088): half is 22100
    for i in 0..2 {
        // n-1, n, n+1 frames: 22049, 22050, 22051.
        for (secs, ok) in [(0.49997, true), (0.5, true), (0.50003, false)] {
            let mut f = [TrackFade::NONE; 3];
            f[i] = fade(secs, 0.0);
            let r = track_starts(&l, &f);
            assert_eq!(r.is_ok(), ok, "track {} fade-in {secs}", i + 1);
            if !ok {
                let e = err_text(r);
                assert!(e.contains(&format!("track {}", i + 1)) && e.contains("longer than half"), "{e:?}");
            }
            f[i] = fade(0.0, secs);
            assert_eq!(track_starts(&l, &f).is_ok(), ok, "track {} fade-out {secs}", i + 1);
        }
    }
    // The audio length counts, not the padding up to a whole CD frame.
    let mut f = [TrackFade::NONE; 3];
    f[2] = fade(0.5025, 0.0);
    let e = err_text(track_starts(&l, &f));
    assert!(e.contains("track 3") && e.contains("longer than half"), "{e:?}");
    f[2] = fade(0.5, 0.0);
    assert!(track_starts(&l, &f).is_ok());
}

#[test]
fn track_starts_refuses_an_invalid_fade_with_its_track_number() {
    let d = TempDir::new("invalid");
    let mut l = CdLayout::default();
    l.add(doc(&d.0.join("a"), 44_100, 2, 44_100, 0.1), "one");
    l.add(doc(&d.0.join("b"), 44_100, 2, 44_100 * 100, 0.1), "two");
    let long = TrackFade { fade_in_secs: 31.0, fade_out_secs: 0.0 };
    let nan = TrackFade { fade_in_secs: 0.0, fade_out_secs: f64::NAN };
    let e = err_text(track_starts(&l, &[TrackFade::NONE, long]));
    assert!(e.contains("track 2") && e.contains("fade-in"), "{e:?}");
    let e = err_text(track_starts(&l, &[nan, TrackFade::NONE]));
    assert!(e.contains("track 1") && e.contains("fade-out"), "{e:?}");
}
