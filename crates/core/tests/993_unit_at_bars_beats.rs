//! #993: dragging a digit group of a Bars and Beats time steps one bar, one beat or one tick at the
//! document's tempo (`TimeFormat::unit_at_with`), and `unit_at` is that with the default tempo.

use wavcraft_core::tempo::Tempo;
use wavcraft_core::view::TimeFormat;

const R: u32 = 48_000;
/// "2:3.480": the bar at 0, the beat at 2, the tick group at 4 to 6.
const T: &str = "2:3.480";

fn waltz() -> Tempo {
    Tempo::new(60.0, 3, 4).unwrap()
}

#[test]
fn the_bar_group_steps_one_bar() {
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 0, R, &Tempo::default()), Some(96_000), "120 bpm 4/4: a bar is 2 s");
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("3:2.000", 0, R, &waltz()), Some(144_000), "60 bpm 3/4: a bar is 3 s");
}

#[test]
fn the_beat_group_steps_one_beat() {
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 2, R, &Tempo::default()), Some(24_000), "120 bpm: a beat is 0.5 s");
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("3:2.000", 2, R, &waltz()), Some(48_000), "60 bpm: a beat is 1 s");
}

#[test]
fn the_tick_group_steps_one_tick() {
    let (def, w) = (Tempo::default(), waltz());
    for i in 4..=6 {
        assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, i, R, &def), Some(25), "120 bpm: 24,000 frames over 960 ticks, digit {i}");
    }
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("3:2.000", 5, R, &w), Some(50), "60 bpm 3/4: 48,000 frames over 960 ticks");
}

#[test]
fn an_eighth_note_meter_has_480_ticks_to_the_beat() {
    let t = Tempo::new(90.0, 7, 8).unwrap();
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("1:1.000", 0, R, &t), Some(112_000), "7 beats of 16,000 frames");
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("1:1.000", 2, R, &t), Some(16_000));
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("1:1.000", 4, R, &t), Some(33), "16,000 / 480 = 33.3 frames");
}

#[test]
fn a_separator_belongs_to_the_group_on_its_right() {
    let d = Tempo::default();
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 1, R, &d), Some(24_000), "the colon is the beat's");
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 3, R, &d), Some(25), "the dot is the tick group's");
}

#[test]
fn a_step_is_at_least_one_frame_and_text_or_rate_missing_gives_none() {
    let d = Tempo::default();
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 5, 1_000, &d), Some(1), "500 frames over 960 ticks rounds to 0, so 1");
    assert_eq!(TimeFormat::BarsBeats.unit_at_with("", 0, R, &d), None);
    assert_eq!(TimeFormat::BarsBeats.unit_at_with(T, 0, 0, &d), None);
}

#[test]
fn unit_at_is_unit_at_with_the_default_tempo_for_bars_and_beats() {
    for i in 0..7 {
        assert_eq!(TimeFormat::BarsBeats.unit_at(T, i, R), TimeFormat::BarsBeats.unit_at_with(T, i, R, &Tempo::default()), "char {i}");
    }
    assert_eq!(TimeFormat::BarsBeats.unit_at(T, 0, R), Some(96_000));
}

#[test]
fn the_other_formats_give_what_they_gave_before_whatever_the_tempo() {
    let w = waltz();
    let cases: [(TimeFormat, &str); 5] = [
        (TimeFormat::Decimal, "1:02:03.500"),
        (TimeFormat::Compact, "0:03"),
        (TimeFormat::Smpte30, "01:02:03:15"),
        (TimeFormat::Smpte25, "01:02:03:15"),
        (TimeFormat::Samples, "123456"),
    ];
    for (f, text) in cases {
        for i in 0..text.chars().count() {
            assert_eq!(f.unit_at_with(text, i, R, &w), f.unit_at(text, i, R), "{f:?} {text} char {i}");
        }
    }
    assert_eq!(TimeFormat::Decimal.unit_at_with("1:02:03.500", 0, R, &w), Some(3600 * 48_000));
    assert_eq!(TimeFormat::Smpte30.unit_at_with("01:02:03:15", 10, R, &w), Some(1600));
    assert_eq!(TimeFormat::Samples.unit_at_with("123456", 2, R, &w), Some(1000));
}
