//! L1.1 and L1.2 (#1057): the word-timed transcript, its JSON form, and the sidecar file saved
//! beside the audio.

use std::path::PathBuf;

use wavcraft_core::transcript::{self, Transcript, Word};

fn word(text: &str, start: u64, end: u64) -> Word {
    Word { text: text.to_string(), start, end }
}

fn three_words() -> Transcript {
    Transcript::new(48_000, vec![word("hello", 4_800, 26_400), word("there", 28_000, 51_123), word("world", 51_123, 96_001)])
}

#[test]
fn round_trips_through_json() {
    let t = three_words();
    assert_eq!(t.version, 1);
    let text = t.to_json();
    let back = Transcript::from_json(&text).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.sample_rate, 48_000);
    let frames: Vec<(u64, u64)> = back.words.iter().map(|w| (w.start, w.end)).collect();
    assert_eq!(frames, vec![(4_800, 26_400), (28_000, 51_123), (51_123, 96_001)]);
    assert_eq!(back.words[1].text, "there");
}

#[test]
fn rejects_unknown_version() {
    let text = three_words().to_json().replacen("\"version\":1", "\"version\":2", 1);
    assert!(text.contains("\"version\":2"), "the test edits the version: {text}");
    let err = Transcript::from_json(&text).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn rejects_overlapping_words() {
    let text = r#"{"version":1,"sample_rate":48000,"words":[
        {"text":"a","start":0,"end":1000},
        {"text":"b","start":999,"end":2000}]}"#;
    let err = Transcript::from_json(text).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    // Touching is fine.
    assert!(Transcript::from_json(&text.replace("999", "1000")).is_ok());
}

#[test]
fn rejects_other_bad_data() {
    for bad in [
        "",
        "[]",
        r#"{"version":1,"sample_rate":0,"words":[]}"#,
        r#"{"version":1,"sample_rate":48000}"#,
        r#"{"version":1,"sample_rate":48000,"words":[{"text":"a","start":5,"end":5}]}"#,
        r#"{"version":1,"sample_rate":48000,"words":[{"text":"a","start":-5,"end":5}]}"#,
        r#"{"version":1,"sample_rate":48000,"words":[{"text":"a","start":1.5,"end":5}]}"#,
        r#"{"version":1,"sample_rate":48000,"words":[{"text":7,"start":1,"end":5}]}"#,
        r#"{"version":1,"sample_rate":4294967296,"words":[]}"#,
    ] {
        assert!(Transcript::from_json(bad).is_err(), "accepted {bad}");
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-transcript-test-{}-{name}", std::process::id()));
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

#[test]
fn sidecar_of_episode_wav() {
    let p = transcript::sidecar_path(std::path::Path::new("/some/folder/episode.wav"));
    assert_eq!(p, PathBuf::from("/some/folder/episode.wav.transcript.json"));
    assert_eq!(p.file_name().unwrap(), "episode.wav.transcript.json");
}

#[test]
fn transcript_survives_save_and_reload() {
    let dir = TempDir::new("reload");
    let audio = dir.0.join("episode.wav");
    let t = three_words();
    transcript::save_beside(&audio, &t).unwrap();
    assert!(dir.0.join("episode.wav.transcript.json").is_file());
    assert_eq!(transcript::load_beside(&audio).unwrap(), Some(t.clone()));
    // A second save replaces the first.
    let shorter = Transcript::new(48_000, vec![word("only", 0, 10)]);
    transcript::save_beside(&audio, &shorter).unwrap();
    assert_eq!(transcript::load_beside(&audio).unwrap(), Some(shorter));
    let names: Vec<_> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
}

#[test]
fn a_missing_sidecar_is_none() {
    let dir = TempDir::new("missing");
    assert_eq!(transcript::load_beside(&dir.0.join("episode.wav")).unwrap(), None);
    // A damaged one is an error, not None.
    std::fs::write(dir.0.join("bad.wav.transcript.json"), "{not json").unwrap();
    assert!(transcript::load_beside(&dir.0.join("bad.wav")).is_err());
}

#[test]
fn word_at_start_end_and_gap() {
    let t = Transcript::new(48_000, vec![word("first", 48_000, 72_000), word("second", 1_488_000, 1_512_000)]);
    assert_eq!(t.word_at(48_000), Some(0));
    assert_eq!(t.word_at(71_999), Some(0));
    assert_eq!(t.word_at(72_000), None);
    assert_eq!(t.word_at(0), None);
    assert_eq!(t.word_at(1_488_000), Some(1));
    assert_eq!(t.word_at(1_511_999), Some(1));
    assert_eq!(t.word_at(1_512_000), None);
    // A gap between words, and the frame just before the first word.
    assert_eq!(t.word_at(900_000), None);
    assert_eq!(t.word_at(47_999), None);
    // Touching words: the shared frame belongs to the later word.
    let touching = Transcript::new(48_000, vec![word("a", 0, 10), word("b", 10, 20)]);
    assert_eq!(touching.word_at(9), Some(0));
    assert_eq!(touching.word_at(10), Some(1));
    assert_eq!(touching.word_at(20), None);
    // An empty transcript has no word anywhere.
    let empty = Transcript::new(48_000, Vec::new());
    assert_eq!(empty.word_at(0), None);
    assert_eq!(empty.word_at(u64::MAX), None);
}

#[test]
fn range_of_words() {
    let t = Transcript::new(48_000, vec![word("first", 48_000, 72_000), word("second", 80_000, 90_000), word("third", 1_488_000, 1_512_000)]);
    assert_eq!(t.range_of(0, 1), 48_000..90_000);
    // Dragging the other way gives the same range.
    assert_eq!(t.range_of(1, 0), 48_000..90_000);
    assert_eq!(t.range_of(2, 2), 1_488_000..1_512_000);
    assert_eq!(t.range_of(0, 2), 48_000..1_512_000);
    // An index past the last word gives the empty range, not a panic.
    assert_eq!(t.range_of(0, 3), 0..0);
}
