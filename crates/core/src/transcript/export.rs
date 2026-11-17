//! Text and SubRip export of a transcript (L5.2 and L5.3, #1271 and #1061).
//!
//! [`plain_text`] writes the words in order, joined by single spaces, with one trailing newline.
//! [`srt`] writes the words as SubRip subtitle cues, their times [`srt_time`]. An empty transcript
//! gives an empty string in both.

use super::{Transcript, Word};

/// The transcript as plain text: each word's text, joined by single spaces, then one `\n`.
/// No words gives an empty string.
pub fn plain_text(t: &Transcript) -> String {
    let words: &[Word] = &t.words;
    if words.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&w.text);
    }
    out.push('\n');
    out
}

/// One frame at `sample_rate` as a SubRip time `HH:MM:SS,mmm`: the milliseconds floored, the hours
/// not wrapped at 24 and keeping every digit past 99. A zero `sample_rate` gives `00:00:00,000`.
pub fn srt_time(frame: u64, sample_rate: u32) -> String {
    let ms = if sample_rate == 0 { 0 } else { frame * 1000 / u64::from(sample_rate) };
    let h = ms / 3_600_000;
    let m = (ms / 60_000) % 60;
    let s = (ms / 1_000) % 60;
    let mmm = ms % 1_000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, mmm)
}

/// The transcript as SubRip (SRT) subtitles: cues numbered from 1, each closed right after a word
/// whose last character is `.`, `?` or `!` or once it holds 10 words, its start from the first
/// word's and its end from the last word's frame through [`srt_time`]. Cues are separated by one
/// blank line and the string ends with the last cue's newline. No words gives an empty string.
pub fn srt(t: &Transcript) -> String {
    if t.words.is_empty() {
        return String::new();
    }
    let mut blocks: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut count = 0usize;
    let mut start = 0u64;
    let mut end = 0u64;
    for w in &t.words {
        if count == 0 {
            start = w.start;
        } else {
            text.push(' ');
        }
        text.push_str(&w.text);
        end = w.end;
        count += 1;
        if matches!(w.text.chars().next_back(), Some('.' | '?' | '!')) || count == 10 {
            let n = blocks.len() + 1;
            blocks.push(format!("{n}\n{} --> {}\n{}\n", srt_time(start, t.sample_rate), srt_time(end, t.sample_rate), text));
            text.clear();
            count = 0;
        }
    }
    if count > 0 {
        let n = blocks.len() + 1;
        blocks.push(format!("{n}\n{} --> {}\n{}\n", srt_time(start, t.sample_rate), srt_time(end, t.sample_rate), text));
    }
    blocks.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: u64, end: u64) -> Word {
        Word { text: text.to_string(), start, end }
    }

    #[test]
    fn plain_text_is_exact() {
        let t = Transcript::new(48_000, vec![word("Hello", 0, 10), word("world.", 10, 20), word("This", 20, 30), word("works.", 30, 40)]);
        assert_eq!(plain_text(&t), "Hello world. This works.\n");
    }

    #[test]
    fn empty_transcript_is_empty_string() {
        let t = Transcript::new(48_000, Vec::new());
        assert_eq!(plain_text(&t), "");
    }
}
