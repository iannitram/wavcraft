//! Text export of a transcript (L5.2, #1271).
//!
//! [`plain_text`] writes the words in order, joined by single spaces, with one trailing newline.
//! An empty transcript gives an empty string.

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
