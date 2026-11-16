//! The filler finder (L4.1, #1060): which words of a [`Transcript`] are fillers such as "um"
//! or "uh". Case and punctuation are ignored, and only whole words match ("umbrella" is not "um").
//! A stretched filler ("umm", "uhhh") matches its short form, and [`repeats`] finds a word said
//! twice in a row ("the the").

use std::ops::Range;

use super::Transcript;

/// The filler words the finder looks for by default.
pub const DEFAULT_FILLERS: [&str; 8] = ["um", "uh", "er", "erm", "ah", "hm", "hmm", "mm"];

/// `text` with a trailing run of one repeated character collapsed to a single one: "uhhh" is
/// "uh", while "um" and a one-letter word are unchanged.
fn collapse(text: &str) -> String {
    let mut out = text.to_string();
    if let Some(last) = out.chars().next_back() {
        while out.chars().rev().nth(1) == Some(last) {
            out.pop();
        }
    }
    out
}

/// The letters and digits of `text`, lower-cased: the form words are compared in.
fn word_key(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The filler form of a word: its key with a trailing run of one repeated letter collapsed.
/// "Ummm," gives "um".
fn filler_key(text: &str) -> String {
    collapse(&word_key(text))
}

/// The index of every word in `t` whose filler key (see [`filler_key`]) equals the key of one entry in
/// `list`, in order. A word that is only punctuation has an empty key and matches nothing.
pub fn fillers(t: &Transcript, list: &[&str]) -> Vec<usize> {
    let keys: Vec<String> = list.iter().map(|f| filler_key(f)).filter(|k| !k.is_empty()).collect();
    t.words
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let k = filler_key(&w.text);
            !k.is_empty() && keys.contains(&k)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The index of every word in `t` that repeats the word before it: the second word of each
/// adjacent pair whose keys (see [`word_key`]) are equal, in order. Case and punctuation are ignored,
/// so "The" followed by "the," is a repeat; a word that is only punctuation is not.
pub fn repeats(t: &Transcript) -> Vec<usize> {
    t.words
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| {
            let k = word_key(&pair[0].text);
            !k.is_empty() && k == word_key(&pair[1].text)
        })
        .map(|(i, _)| i + 1)
        .collect()
}

/// The default shortest gap that counts as a pause: 1.5 seconds at `sample_rate`.
pub fn default_pause_frames(sample_rate: u32) -> u64 {
    u64::from(sample_rate) * 3 / 2
}

/// Every gap between one word's end and the next word's start that is at least `min_frames`
/// long, in order. Gaps before the first word and after the last are not pauses. A gap of zero
/// frames is never a pause, whatever `min_frames` is.
pub fn pauses(t: &Transcript, min_frames: u64) -> Vec<Range<u64>> {
    t.words
        .windows(2)
        .filter_map(|pair| {
            let (start, end) = (pair[0].end, pair[1].start);
            let len = end.saturating_sub(start);
            (len > 0 && len >= min_frames).then_some(start..end)
        })
        .collect()
}

/// The middle of `pause` to remove so that `keep_frames` of it stay: the kept frames are split
/// between the start and the end of the pause (the start gets the half that rounds down). A pause
/// no longer than `keep_frames` gives an empty range at its start.
pub fn shorten_cut(pause: Range<u64>, keep_frames: u64) -> Range<u64> {
    let len = pause.end.saturating_sub(pause.start);
    if keep_frames >= len {
        return pause.start..pause.start;
    }
    let front = keep_frames / 2;
    let back = keep_frames - front;
    (pause.start + front)..(pause.end - back)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Word;

    fn word(text: &str, start: u64, end: u64) -> Word {
        Word { text: text.to_string(), start, end }
    }

    fn transcript(texts: &[&str]) -> Transcript {
        let words = texts.iter().enumerate().map(|(i, t)| Word { text: (*t).to_string(), start: i as u64 * 10, end: i as u64 * 10 + 5 }).collect();
        Transcript::new(48_000, words)
    }

    #[test]
    fn finds_fillers_through_case_and_punctuation() {
        let t = transcript(&["Um,", "the", "UH.", "umbrella", "uh"]);
        assert_eq!(fillers(&t, &DEFAULT_FILLERS), vec![0, 2, 4]);
    }

    #[test]
    fn stretched_fillers_match_their_short_form() {
        let t = transcript(&["Umm,", "the", "UHHH.", "umbrella", "hmmm"]);
        assert_eq!(fillers(&t, &DEFAULT_FILLERS), vec![0, 2, 4]);
    }

    #[test]
    fn immediate_repeats_are_found_at_the_second_word() {
        let t = transcript(&["the", "the", "best", "best", "part"]);
        assert_eq!(repeats(&t), vec![1, 3]);
    }

    #[test]
    fn repeats_do_not_collapse_stretched_words() {
        let t = transcript(&["well", "wel"]);
        assert!(repeats(&t).is_empty());
    }

    #[test]
    fn collapsing_leaves_um_and_a_one_letter_word_alone() {
        assert_eq!(collapse("um"), "um");
        assert_eq!(collapse("a"), "a");
        assert_eq!(collapse("uhhh"), "uh");
    }

    #[test]
    fn default_list_is_the_eight_fillers() {
        let t = transcript(&["um", "uh", "er", "erm", "ah", "hm", "hmm", "mm", "hum", "ok"]);
        assert_eq!(fillers(&t, &DEFAULT_FILLERS), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_custom_list_is_used_as_given() {
        let t = transcript(&["like", "um", "Like."]);
        assert_eq!(fillers(&t, &["LIKE"]), vec![0, 2]);
    }

    #[test]
    fn punctuation_only_words_are_not_fillers() {
        let t = transcript(&["...", "--", ""]);
        assert!(fillers(&t, &DEFAULT_FILLERS).is_empty());
    }

    #[test]
    fn a_1_6_s_gap_is_a_pause_a_1_4_s_gap_is_not() {
        // 48 kHz: 1.6 s is 76,800 frames, 1.4 s is 67,200 frames; the default is 72,000.
        let min = default_pause_frames(48_000);
        assert_eq!(min, 72_000);
        let long = Transcript::new(48_000, vec![word("a", 0, 48_000), word("b", 48_000 + 76_800, 120_000 + 76_800)]);
        assert_eq!(pauses(&long, min), vec![48_000..124_800]);
        let short = Transcript::new(48_000, vec![word("a", 0, 48_000), word("b", 48_000 + 67_200, 120_000 + 67_200)]);
        assert!(pauses(&short, min).is_empty());
    }

    #[test]
    fn pauses_are_found_between_words_only() {
        let t = Transcript::new(
            48_000,
            vec![word("a", 0, 10_000), word("b", 100_000, 110_000), word("c", 110_000, 120_000), word("d", 200_000, 210_000)],
        );
        assert_eq!(pauses(&t, 1), vec![10_000..100_000, 120_000..200_000]);
        assert_eq!(pauses(&t, 90_000), vec![10_000..100_000]);
        assert!(pauses(&Transcript::new(48_000, Vec::new()), 1).is_empty());
    }

    #[test]
    fn a_zero_gap_is_never_a_pause() {
        let t = Transcript::new(48_000, vec![word("a", 0, 10_000), word("b", 10_000, 20_000)]);
        assert!(pauses(&t, 0).is_empty());
    }

    #[test]
    fn shortening_a_2_s_pause_to_half_a_second() {
        // 2 s is 96,000 frames; keeping 0.5 s (24,000) leaves 12,000 at each end.
        assert_eq!(shorten_cut(100_000..196_000, 24_000), 112_000..184_000);
    }

    #[test]
    fn shortening_keeps_the_given_frames_even_when_odd() {
        let cut = shorten_cut(100..110, 3);
        assert_eq!(cut, 101..108);
        assert_eq!(10 - (cut.end - cut.start), 3);
    }

    #[test]
    fn a_pause_no_longer_than_the_keep_is_not_cut() {
        assert_eq!(shorten_cut(100..110, 10), 100..100);
        assert_eq!(shorten_cut(100..110, 50), 100..100);
    }
}
