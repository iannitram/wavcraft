//! Roadmap C202 (core only): the count-in before a Record. The pre-roll length, the frame the
//! recording really begins at, and the clicks to sound, all from the click grid's own maths.

use wavcraft_core::metronome::record_count_in::{
    CountInError, MAX_COUNT_IN_BARS, RecordCountIn, RecordPlan, check_track, plan_record_count_in, pre_roll_frames,
};
use wavcraft_core::metronome::{Click, ClickTrack};
use wavcraft_core::tempo::Tempo;

fn track(bpm: f64, n: u32, d: u32, rate: u32) -> ClickTrack {
    ClickTrack::new(Tempo::new(bpm, n, d).unwrap(), rate)
}

fn on(bars: u32) -> RecordCountIn {
    RecordCountIn { bars, on_record: true }
}

fn frames(p: &RecordPlan) -> Vec<u64> {
    p.clicks.iter().map(|c| c.frame).collect()
}

fn accents(p: &RecordPlan) -> Vec<bool> {
    p.clicks.iter().map(|c| c.accent).collect()
}

#[test]
fn the_default_setting_is_no_count_in() {
    let s = RecordCountIn::default();
    assert_eq!((s.bars, s.on_record), (0, false));
    let p = plan_record_count_in(&track(120.0, 4, 4, 44_100), &s, 5_000).unwrap();
    assert_eq!(p, RecordPlan { pre_roll_frames: 0, record_from: 5_000, clicks: vec![] });
}

#[test]
fn one_bar_of_four_four_at_120_is_four_clicks_and_the_pre_roll_is_one_bar() {
    let p = plan_record_count_in(&track(120.0, 4, 4, 44_100), &on(1), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 88_200);
    assert_eq!(p.record_from, 88_200);
    assert_eq!(frames(&p), [0, 22_050, 44_100, 66_150]);
    assert_eq!(accents(&p), [true, false, false, false]);
    assert_eq!(p.clicks.iter().map(|c| c.beat).collect::<Vec<_>>(), [0, 1, 2, 3]);
}

#[test]
fn two_bars_accent_the_first_beat_of_each_bar() {
    let p = plan_record_count_in(&track(120.0, 4, 4, 44_100), &on(2), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 176_400);
    assert_eq!(p.clicks.len(), 8);
    assert_eq!(accents(&p), [true, false, false, false, true, false, false, false]);
    assert_eq!(p.clicks[7].frame, 154_350);
}

#[test]
fn the_start_moves_the_recording_and_the_clicks_but_not_the_pre_roll() {
    let t = track(120.0, 4, 4, 44_100);
    let p = plan_record_count_in(&t, &on(1), 1_000).unwrap();
    assert_eq!(p.pre_roll_frames, 88_200);
    assert_eq!(p.record_from, 89_200);
    assert_eq!(frames(&p), [1_000, 23_050, 45_100, 67_150]);
    assert_eq!(p.clicks[0].beat, 0);
    assert!(p.clicks[0].accent);
}

#[test]
fn three_four_at_90_and_six_eight_at_120_follow_their_meters() {
    let p = plan_record_count_in(&track(90.0, 3, 4, 48_000), &on(1), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 96_000);
    assert_eq!(frames(&p), [0, 32_000, 64_000]);
    assert_eq!(accents(&p), [true, false, false]);
    let p = plan_record_count_in(&track(120.0, 6, 8, 48_000), &on(1), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 72_000);
    assert_eq!(frames(&p), [0, 12_000, 24_000, 36_000, 48_000, 60_000]);
    assert_eq!(accents(&p), [true, false, false, false, false, false]);
    let p = plan_record_count_in(&track(100.0, 5, 4, 44_100), &on(1), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 132_300);
    assert_eq!(frames(&p), [0, 26_460, 52_920, 79_380, 105_840]);
}

#[test]
fn fractional_beats_use_the_click_tracks_rounding() {
    let t = track(93.0, 4, 4, 44_100);
    let p = plan_record_count_in(&t, &on(1), 0).unwrap();
    assert_eq!(p.pre_roll_frames, 113_806);
    assert_eq!(frames(&p), [0, 28_452, 56_903, 85_355]);
    assert_eq!(plan_record_count_in(&t, &on(2), 0).unwrap().pre_roll_frames, 227_613);
    assert_eq!(plan_record_count_in(&t, &on(3), 0).unwrap().pre_roll_frames, 341_419);
    let p = plan_record_count_in(&t, &on(3), 0).unwrap();
    assert_eq!(p.clicks.len(), 12);
    assert_eq!(p.clicks[11].frame, 312_968);
}

#[test]
fn the_plan_matches_the_click_tracks_own_clicks() {
    for (bpm, n, d, rate, bars, start) in [(93.0, 4, 4, 44_100, 2, 777), (90.0, 3, 4, 48_000, 3, 0), (120.0, 6, 8, 48_000, 1, 123_456)] {
        let t = track(bpm, n, d, rate);
        let p = plan_record_count_in(&t, &on(bars), start).unwrap();
        let want: Vec<Click> = t.clicks(0, t.bars_frames(bars)).into_iter().map(|c| Click { frame: c.frame + start, ..c }).collect();
        assert_eq!(p.clicks, want);
        assert_eq!(p.pre_roll_frames, t.bars_frames(bars));
        assert_eq!(p.record_from, start + t.bars_frames(bars));
    }
}

#[test]
fn every_click_sits_between_the_start_and_the_recording() {
    let p = plan_record_count_in(&track(93.0, 4, 4, 44_100), &on(3), 40_000).unwrap();
    assert!(p.clicks.iter().all(|c| c.frame >= 40_000 && c.frame < p.record_from));
    assert_eq!(p.record_from, 40_000 + p.pre_roll_frames);
}

#[test]
fn the_flag_off_or_zero_bars_means_no_count_in() {
    let t = track(120.0, 4, 4, 44_100);
    let off = RecordCountIn { bars: 2, on_record: false };
    let p = plan_record_count_in(&t, &off, 9_000).unwrap();
    assert_eq!(p, RecordPlan { pre_roll_frames: 0, record_from: 9_000, clicks: vec![] });
    assert_eq!(pre_roll_frames(&t, &off), Ok(0));
    let zero = RecordCountIn { bars: 0, on_record: true };
    let p = plan_record_count_in(&t, &zero, 9_000).unwrap();
    assert_eq!(p, RecordPlan { pre_roll_frames: 0, record_from: 9_000, clicks: vec![] });
    assert_eq!(pre_roll_frames(&t, &zero), Ok(0));
}

#[test]
fn pre_roll_frames_is_the_bars_in_frames() {
    let t = track(120.0, 4, 4, 44_100);
    assert_eq!(pre_roll_frames(&t, &on(1)), Ok(88_200));
    assert_eq!(pre_roll_frames(&t, &on(2)), Ok(176_400));
    assert_eq!(pre_roll_frames(&t, &on(16)), Ok(1_411_200));
}

#[test]
fn bars_are_limited_to_sixteen_at_n_minus_1_n_and_n_plus_1() {
    assert_eq!(MAX_COUNT_IN_BARS, 16);
    let t = track(120.0, 4, 4, 44_100);
    let p = plan_record_count_in(&t, &on(15), 0).unwrap();
    assert_eq!((p.pre_roll_frames, p.clicks.len()), (1_323_000, 60));
    let p = plan_record_count_in(&t, &on(16), 0).unwrap();
    assert_eq!((p.pre_roll_frames, p.clicks.len()), (1_411_200, 64));
    assert_eq!(plan_record_count_in(&t, &on(17), 0), Err(CountInError::TooManyBars));
    assert_eq!(pre_roll_frames(&t, &on(17)), Err(CountInError::TooManyBars));
    assert_eq!(plan_record_count_in(&t, &on(u32::MAX), 0), Err(CountInError::TooManyBars));
}

#[test]
fn too_many_bars_is_refused_even_when_the_flag_is_off() {
    let t = track(120.0, 4, 4, 44_100);
    let s = RecordCountIn { bars: 17, on_record: false };
    assert_eq!(plan_record_count_in(&t, &s, 0), Err(CountInError::TooManyBars));
    assert_eq!(pre_roll_frames(&t, &s), Err(CountInError::TooManyBars));
}

#[test]
fn a_zero_rate_is_refused() {
    let t = ClickTrack { tempo: Tempo::default(), rate: 0 };
    assert_eq!(check_track(&t), Err(CountInError::BadRate));
    assert_eq!(plan_record_count_in(&t, &on(1), 0), Err(CountInError::BadRate));
    assert_eq!(plan_record_count_in(&t, &RecordCountIn::default(), 0), Err(CountInError::BadRate));
    assert_eq!(pre_roll_frames(&t, &on(1)), Err(CountInError::BadRate));
    assert_eq!(check_track(&ClickTrack { tempo: Tempo::default(), rate: 1 }), Ok(()));
}

#[test]
fn zero_nan_negative_and_infinite_tempos_are_refused() {
    for bpm in [0.0, f64::NAN, -120.0, f64::INFINITY, f64::NEG_INFINITY] {
        let t = ClickTrack { tempo: Tempo { bpm, numerator: 4, denominator: 4 }, rate: 44_100 };
        assert_eq!(check_track(&t), Err(CountInError::BadTempo), "bpm {bpm}");
        assert_eq!(plan_record_count_in(&t, &on(1), 0), Err(CountInError::BadTempo), "bpm {bpm}");
        assert_eq!(plan_record_count_in(&t, &RecordCountIn::default(), 0), Err(CountInError::BadTempo), "bpm {bpm}");
        assert_eq!(pre_roll_frames(&t, &on(1)), Err(CountInError::BadTempo), "bpm {bpm}");
    }
}

#[test]
fn the_tempo_range_is_20_to_400_at_the_edges() {
    let at = |bpm: f64| ClickTrack { tempo: Tempo { bpm, numerator: 4, denominator: 4 }, rate: 48_000 };
    assert_eq!(check_track(&at(19.999)), Err(CountInError::BadTempo));
    assert_eq!(check_track(&at(20.0)), Ok(()));
    assert_eq!(check_track(&at(400.0)), Ok(()));
    assert_eq!(check_track(&at(400.001)), Err(CountInError::BadTempo));
    assert_eq!(plan_record_count_in(&at(20.0), &on(1), 0).unwrap().pre_roll_frames, 576_000);
    assert_eq!(plan_record_count_in(&at(400.0), &on(1), 0).unwrap().pre_roll_frames, 28_800);
}

#[test]
fn a_bad_meter_is_refused_at_its_edges() {
    let with = |n: u32, d: u32| ClickTrack { tempo: Tempo { bpm: 120.0, numerator: n, denominator: d }, rate: 48_000 };
    assert_eq!(check_track(&with(0, 4)), Err(CountInError::BadTempo));
    assert_eq!(check_track(&with(1, 4)), Ok(()));
    assert_eq!(check_track(&with(32, 4)), Ok(()));
    assert_eq!(check_track(&with(33, 4)), Err(CountInError::BadTempo));
    assert_eq!(check_track(&with(4, 0)), Err(CountInError::BadTempo));
    assert_eq!(check_track(&with(4, 3)), Err(CountInError::BadTempo));
    assert_eq!(check_track(&with(4, 32)), Ok(()));
    assert_eq!(plan_record_count_in(&with(0, 4), &on(1), 0), Err(CountInError::BadTempo));
}

#[test]
fn a_bad_rate_is_reported_before_a_bad_tempo_and_a_bad_tempo_before_too_many_bars() {
    let both = ClickTrack { tempo: Tempo { bpm: 0.0, numerator: 4, denominator: 4 }, rate: 0 };
    assert_eq!(check_track(&both), Err(CountInError::BadRate));
    let t = ClickTrack { tempo: Tempo { bpm: 0.0, numerator: 4, denominator: 4 }, rate: 44_100 };
    assert_eq!(plan_record_count_in(&t, &on(99), 0), Err(CountInError::BadTempo));
}

#[test]
fn the_start_plus_the_pre_roll_must_fit_in_a_frame_number() {
    let t = track(120.0, 4, 4, 44_100);
    let max = u64::MAX;
    // pre-roll is 88,200 frames: the largest start that fits is max - 88,200.
    let p = plan_record_count_in(&t, &on(1), max - 88_200 - 1).unwrap();
    assert_eq!(p.record_from, max - 1);
    let p = plan_record_count_in(&t, &on(1), max - 88_200).unwrap();
    assert_eq!(p.record_from, max);
    assert_eq!(p.clicks[3].frame, max - 88_200 + 66_150);
    assert_eq!(plan_record_count_in(&t, &on(1), max - 88_200 + 1), Err(CountInError::Overflow));
    assert_eq!(plan_record_count_in(&t, &on(1), max), Err(CountInError::Overflow));
    // no count-in adds nothing, so even the largest start is fine.
    let p = plan_record_count_in(&t, &RecordCountIn::default(), max).unwrap();
    assert_eq!(p.record_from, max);
}

#[test]
fn the_messages_state_their_ranges() {
    assert!(CountInError::TooManyBars.to_string().contains("0 to 16"));
    assert!(CountInError::BadTempo.to_string().contains("20 to 400"));
    assert!(CountInError::BadTempo.to_string().contains("1 to 32"));
    assert!(CountInError::BadRate.to_string().contains("above 0"));
    assert!(CountInError::Overflow.to_string().contains("largest frame"));
    let all = [CountInError::BadRate, CountInError::BadTempo, CountInError::TooManyBars, CountInError::Overflow];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.to_string(), b.to_string());
        }
    }
}

#[test]
fn a_valid_plan_is_the_same_each_time() {
    let t = track(93.0, 4, 4, 44_100);
    assert_eq!(plan_record_count_in(&t, &on(2), 321), plan_record_count_in(&t, &on(2), 321));
}
