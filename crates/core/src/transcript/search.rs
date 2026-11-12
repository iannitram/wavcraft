//! Phrase search over a [`Transcript`] (L5.1, #1270): where a query occurs in the words, as
//! word-index ranges. Case and punctuation are ignored, and a phrase must be consecutive words
//! ("quick brown" matches "The quick, brown fox" but not "quick fox brown").

use std::ops::Range;

use super::Transcript;

/// The letters and digits of `text`, lower-cased: the form two words are compared in.
fn key(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Every place `query` occurs in `t`, as word-index ranges in order. The query is split at
/// anything that is not a letter or digit, and each part is compared with a word's key (see
/// [`key`]); the parts must match consecutive words. An empty query, or one with no letters or
/// digits, matches nothing. Matches may overlap ("the the" in "the the the" gives 0..2 and 1..3).
pub fn find(t: &Transcript, query: &str) -> Vec<Range<usize>> {
    let want: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).map(key).filter(|k| !k.is_empty()).collect();
    if want.is_empty() || want.len() > t.words.len() {
        return Vec::new();
    }
    let keys: Vec<String> = t.words.iter().map(|w| key(&w.text)).collect();
    (0..=keys.len() - want.len()).filter(|&i| keys[i..i + want.len()] == want[..]).map(|i| i..i + want.len()).collect()
}

/// The index in `matches` of the first match that starts after word `from`, wrapping to the
/// first match when none does. `None` when there are no matches. `matches` is in the order
/// [`find`] gives.
pub fn next_match(matches: &[Range<usize>], from: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|m| m.start > from).unwrap_or(0))
}

/// The index in `matches` of the last match that starts before word `from`, wrapping to the
/// last match when none does. `None` when there are no matches.
pub fn prev_match(matches: &[Range<usize>], from: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().rposition(|m| m.start < from).unwrap_or(matches.len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Word;

    fn transcript(texts: &[&str]) -> Transcript {
        let words = texts.iter().enumerate().map(|(i, t)| Word { text: (*t).to_string(), start: i as u64 * 10, end: i as u64 * 10 + 5 }).collect();
        Transcript::new(48_000, words)
    }

    #[test]
    fn finds_every_match() {
        let t = transcript(&["The", "cat", "saw", "the", "dog", "and", "then", "a", "THE", "bird", "sang", "."]);
        assert_eq!(find(&t, "the"), vec![0..1, 3..4, 8..9]);
    }

    #[test]
    fn a_phrase_spans_words() {
        let t = transcript(&["the", "quick", "brown", "fox", "quick", "fox", "brown"]);
        assert_eq!(find(&t, "quick brown"), vec![1..3]);
        assert_eq!(find(&t, "Quick, brown!"), vec![1..3]);
    }

    #[test]
    fn next_wraps_to_the_first() {
        let m = find(&transcript(&["the", "a", "the", "b", "the"]), "the");
        assert_eq!(m, vec![0..1, 2..3, 4..5]);
        assert_eq!(next_match(&m, 0), Some(1));
        assert_eq!(next_match(&m, 4), Some(0));
        assert_eq!(next_match(&[], 0), None);
    }

    #[test]
    fn prev_wraps_to_the_last() {
        let m = find(&transcript(&["the", "a", "the", "b", "the"]), "the");
        assert_eq!(prev_match(&m, 4), Some(1));
        assert_eq!(prev_match(&m, 0), Some(2));
        assert_eq!(prev_match(&[], 0), None);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        let t = transcript(&["the", "..."]);
        assert!(find(&t, "").is_empty());
        assert!(find(&t, " ,. ").is_empty());
    }
}
