//! L5.3 (#1061): `transcript::export::srt` writes a transcript as SubRip subtitles, and
//! `transcript::export::srt_time` writes one frame as an SRT time `HH:MM:SS,mmm`.

use wavcraft_core::markers::Shift;
use wavcraft_core::transcript::export::{srt, srt_time};
use wavcraft_core::transcript::{Transcript, Word};

fn word(text: &str, start: u64, end: u64) -> Word {
    Word { text: text.to_string(), start, end }
}

fn at_48k(words: Vec<Word>) -> Transcript {
    Transcript::new(48_000, words)
}

// Catches: a wrong cue layout (number line, " --> " arrow, text line, blank line between cues)
// or a cue that does not end at a word ending in '.'.
#[test]
fn srt_is_exact() {
    let t = at_48k(vec![word("Hello", 0, 24_000), word("world.", 24_000, 48_000), word("This", 72_000, 96_000), word("works.", 96_000, 120_000)]);
    assert_eq!(srt(&t), "1\n00:00:00,000 --> 00:00:01,000\nHello world.\n\n2\n00:00:01,500 --> 00:00:02,500\nThis works.\n");
}

// Catches: an empty transcript giving a header, a number or a newline instead of nothing.
#[test]
fn an_empty_transcript_gives_an_empty_string() {
    assert_eq!(srt(&at_48k(Vec::new())), "");
}

// Catches: cues numbered from 0, or a cue start taken from the wrong word.
#[test]
fn cues_are_numbered_from_1() {
    let t = at_48k(vec![word("One.", 48_000, 96_000), word("Two.", 96_000, 144_000), word("Three.", 144_000, 192_000)]);
    let out = srt(&t);
    let numbers: Vec<&str> = out.split("\n\n").map(|cue| cue.lines().next().unwrap()).collect();
    assert_eq!(numbers, vec!["1", "2", "3"]);
    assert!(out.starts_with("1\n00:00:01,000 --> 00:00:02,000\nOne.\n"), "{out}");
}

// Catches: '?' or '!' not ending a cue (only '.' checked).
#[test]
fn a_question_mark_and_an_exclamation_mark_end_a_cue() {
    let t = at_48k(vec![word("Why?", 0, 4_800), word("Yes!", 4_800, 9_600), word("ok", 9_600, 14_400)]);
    assert_eq!(srt(&t), "1\n00:00:00,000 --> 00:00:00,100\nWhy?\n\n2\n00:00:00,100 --> 00:00:00,200\nYes!\n\n3\n00:00:00,200 --> 00:00:00,300\nok\n");
}

// Catches: a punctuation mark inside a word ending the cue (only the last character counts).
#[test]
fn a_full_stop_inside_a_word_does_not_end_a_cue() {
    let t = at_48k(vec![word("v1.5", 0, 4_800), word("ships", 4_800, 9_600)]);
    assert_eq!(srt(&t), "1\n00:00:00,000 --> 00:00:00,200\nv1.5 ships\n");
}

// Catches: no 10-word limit, a limit of 9 or 11, or the 11th word lost.
#[test]
fn a_cue_ends_after_10_words() {
    let words: Vec<Word> = (0..12u64).map(|i| word(&format!("w{}", i + 1), i * 4_800, (i + 1) * 4_800)).collect();
    assert_eq!(
        srt(&at_48k(words)),
        "1\n00:00:00,000 --> 00:00:01,000\nw1 w2 w3 w4 w5 w6 w7 w8 w9 w10\n\n2\n00:00:01,000 --> 00:00:01,200\nw11 w12\n"
    );
}

// Catches: a 10th word ending in '.' closing the cue twice (an empty second cue).
#[test]
fn a_tenth_word_with_a_full_stop_makes_one_cue() {
    let mut words: Vec<Word> = (0..9u64).map(|i| word("a", i * 4_800, (i + 1) * 4_800)).collect();
    words.push(word("end.", 43_200, 48_000));
    assert_eq!(srt(&at_48k(words)), "1\n00:00:00,000 --> 00:00:01,000\na a a a a a a a a end.\n");
}

// Catches: a last cue with no closing punctuation being dropped.
#[test]
fn the_last_open_cue_is_written() {
    let t = at_48k(vec![word("Done.", 0, 48_000), word("and", 48_000, 72_000), word("then", 72_000, 96_000)]);
    assert_eq!(srt(&t), "1\n00:00:00,000 --> 00:00:01,000\nDone.\n\n2\n00:00:01,000 --> 00:00:02,000\nand then\n");
}

// Catches: wrong hour, minute or second arithmetic (3661.5 s is 1 h 1 min 1.5 s).
#[test]
fn time_of_3661_5_s_is_01_01_01_500() {
    assert_eq!(srt_time(175_752_000, 48_000), "01:01:01,500");
}

// Catches: milliseconds rounded instead of floored (95 frames at 48 kHz is 1.979 ms).
#[test]
fn milliseconds_are_floored() {
    assert_eq!(srt_time(95, 48_000), "00:00:00,001");
    assert_eq!(srt_time(47, 48_000), "00:00:00,000");
}

// Catches: a hard-coded 48 kHz instead of the given rate.
#[test]
fn the_rate_is_the_one_given() {
    assert_eq!(srt_time(44_100 * 61 + 22_050, 44_100), "00:01:01,500");
    let t = Transcript::new(44_100, vec![word("Hi.", 44_100, 88_200)]);
    assert_eq!(srt(&t), "1\n00:00:01,000 --> 00:00:02,000\nHi.\n");
}

// Catches: hours wrapped at 24 or cut to two digits.
#[test]
fn hours_past_99_keep_every_digit() {
    assert_eq!(srt_time(48_000 * 3_600 * 100, 48_000), "100:00:00,000");
    assert_eq!(srt_time(48_000 * 3_600 * 25, 48_000), "25:00:00,000");
}

// Catches: a division by a zero rate panicking.
#[test]
fn a_zero_rate_gives_zero_time() {
    assert_eq!(srt_time(48_000, 0), "00:00:00,000");
}

// Catches: cue times read from somewhere other than the words' frames (a cut must move them).
#[test]
fn cue_times_follow_a_cut() {
    let t = at_48k(vec![word("One.", 48_000, 96_000), word("Two.", 144_000, 192_000)]);
    assert_eq!(srt(&t), "1\n00:00:01,000 --> 00:00:02,000\nOne.\n\n2\n00:00:03,000 --> 00:00:04,000\nTwo.\n");
    let cut = t.moved_by(&Shift::Delete(0..48_000));
    assert_eq!(srt(&cut), "1\n00:00:00,000 --> 00:00:01,000\nOne.\n\n2\n00:00:02,000 --> 00:00:03,000\nTwo.\n");
}
