//! The filler finder (L4.1, #1060): which words of a [`Transcript`] are fillers such as "um"
//! or "uh". Case and punctuation are ignored, and only whole words match ("umbrella" is not "um").

use super::Transcript;

/// The filler words the finder looks for by default.
pub const DEFAULT_FILLERS: [&str; 8] = ["um", "uh", "er", "erm", "ah", "hm", "hmm", "mm"];

/// The letters and digits of `text`, lower-cased: the form two words are compared in.
fn key(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The index of every word in `t` whose key (see [`key`]) equals the key of one entry in
/// `list`, in order. A word that is only punctuation has an empty key and matches nothing.
pub fn fillers(t: &Transcript, list: &[&str]) -> Vec<usize> {
    let keys: Vec<String> = list.iter().map(|f| key(f)).filter(|k| !k.is_empty()).collect();
    t.words
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let k = key(&w.text);
            !k.is_empty() && keys.contains(&k)
        })
        .map(|(i, _)| i)
        .collect()
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
    fn finds_fillers_through_case_and_punctuation() {
        let t = transcript(&["Um,", "the", "UH.", "umbrella", "uh"]);
        assert_eq!(fillers(&t, &DEFAULT_FILLERS), vec![0, 2, 4]);
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
}
