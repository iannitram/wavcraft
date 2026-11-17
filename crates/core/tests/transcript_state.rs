//! L1.3 (#1057): the transcript is part of a document state, so undo and redo carry it.
//! L3.3a (#1059): an edit that moves audio moves the words with it, as it moves the markers.

use std::path::PathBuf;
use std::sync::Arc;

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::transcript::{self, Transcript, Word};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-transcript-state-test-{}-{name}", std::process::id()));
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

fn silent_doc(dir: &TempDir, frames: u64) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let mut ch = Channel::new();
    ch.push_zeros(frames);
    DocState::new(s, vec![ch], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap()
}

fn stereo_silent_doc(dir: &TempDir, frames: u64) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let chans = (0..2)
        .map(|_| {
            let mut ch = Channel::new();
            ch.push_zeros(frames);
            ch
        })
        .collect();
    DocState::new(s, chans, 48_000, SourceFormat::synthetic(48_000, 2)).unwrap()
}

/// The words of a state's transcript as (text, start, end); panics when it has none.
fn words_of(d: &DocState) -> Vec<(String, u64, u64)> {
    d.transcript().expect("the state has a transcript").words.iter().map(|w| (w.text.clone(), w.start, w.end)).collect()
}

fn w3(a: (&str, u64, u64), b: (&str, u64, u64), c: (&str, u64, u64)) -> Vec<(String, u64, u64)> {
    vec![(a.0.to_string(), a.1, a.2), (b.0.to_string(), b.1, b.2), (c.0.to_string(), c.1, c.2)]
}

fn three_words() -> Arc<Transcript> {
    let w = |t: &str, s, e| Word { text: t.to_string(), start: s, end: e };
    Arc::new(Transcript::new(48_000, vec![w("one", 0, 100), w("two", 200, 300), w("three", 400, 500)]))
}

#[test]
fn undo_of_transcribe_removes_the_transcript() {
    let dir = TempDir::new("undo");
    let mut h = History::new(silent_doc(&dir, 48_000), u64::MAX);
    assert!(h.current().transcript().is_none(), "a new document has no transcript");

    assert_eq!(h.push_transcript("Transcribe", three_words()), 0);
    assert_eq!(h.steps().len(), 2);
    assert_eq!(h.steps()[1].label(), "Transcribe");
    assert_eq!(h.steps()[1].made_bytes(), 0, "a transcript makes no audio");
    assert_eq!(h.current().transcript().map(|t| t.words.len()), Some(3));

    let undone = h.undo().unwrap();
    assert!(undone.transcript().is_none());
    assert!(h.current().transcript().is_none());

    let redone = h.redo().unwrap();
    assert_eq!(redone.transcript().map(|t| t.words.len()), Some(3));
    assert_eq!(redone.transcript().unwrap().words[2].text, "three");
}

#[test]
fn the_same_transcript_again_is_no_step() {
    let dir = TempDir::new("same");
    let mut h = History::new(silent_doc(&dir, 1_000), u64::MAX);
    let t = three_words();
    h.push_transcript("Transcribe", t.clone());
    h.push_transcript("Transcribe", Arc::new((*t).clone()));
    assert_eq!(h.steps().len(), 2, "an equal transcript records nothing");
}

#[test]
fn a_rate_change_moves_the_word_frames() {
    let dir = TempDir::new("rate");
    let mut d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    d.rescale_positions(48_000, 24_000);
    let t = d.transcript().unwrap();
    assert_eq!(t.sample_rate, 24_000);
    let frames: Vec<(u64, u64)> = t.words.iter().map(|w| (w.start, w.end)).collect();
    assert_eq!(frames, vec![(0, 50), (100, 150), (200, 250)]);
}

// Catches: a same-length edit (Silence) dropping or moving the words.
#[test]
fn a_same_length_edit_keeps_the_transcript() {
    let dir = TempDir::new("silence");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::silence(&d, 100..200, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(e.state.transcript().map(|t| (**t).clone()), Some((*three_words()).clone()));
}

// Catches: a delete still dropping the transcript, keeping a word the delete covers, or not
// moving later words earlier by the deleted length.
#[test]
fn a_delete_moves_later_words_and_drops_a_covered_one() {
    let dir = TempDir::new("delete");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::delete(&d, 0..150, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(words_of(&e.state), vec![("two".to_string(), 50, 150), ("three".to_string(), 250, 350)]);
}

// Catches: words that straddle a delete's edges being dropped instead of cut to what is left.
#[test]
fn a_delete_across_two_words_trims_both() {
    let dir = TempDir::new("straddle");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::delete(&d, 250..450, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(words_of(&e.state), w3(("one", 0, 100), ("two", 200, 250), ("three", 250, 300)));
}

// Catches: Insert Silence dropping the transcript, or not moving the word that starts at the
// insert point and the words after it.
#[test]
fn insert_silence_moves_the_words_at_and_after_the_insert() {
    let dir = TempDir::new("insert");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::insert_silence(&d, 200, 1_000, Crossfades::NONE).unwrap();
    assert_eq!(words_of(&e.state), w3(("one", 0, 100), ("two", 1_200, 1_300), ("three", 1_400, 1_500)));
}

// Catches: a crop (every channel) dropping the transcript, or keeping words outside the kept range.
#[test]
fn a_crop_keeps_only_the_words_inside_it() {
    let dir = TempDir::new("crop");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::crop(&d, 150..450, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(words_of(&e.state), vec![("two".to_string(), 50, 150), ("three".to_string(), 250, 300)]);
}

// Catches: a paste that lengthens the file (every channel) dropping the transcript or not moving
// later words by the clip's length.
#[test]
fn a_paste_moves_the_later_words() {
    let dir = TempDir::new("paste");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let clip = edit::copy(&d, 0..100, ChannelMask::ALL).unwrap();
    let e = edit::paste(&d, PasteAt::Insert(200), &clip, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(words_of(&e.state), w3(("one", 0, 100), ("two", 300, 400), ("three", 500, 600)));
}

// Catches: a delete on one channel of two (which keeps the length) moving or dropping the words.
#[test]
fn a_delete_on_one_channel_keeps_the_transcript() {
    let dir = TempDir::new("one-channel");
    let d = stereo_silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::delete(&d, 0..150, ChannelMask::only(0), Crossfades::NONE).unwrap();
    assert_eq!(e.state.len_frames(), 48_000);
    assert_eq!(words_of(&e.state), w3(("one", 0, 100), ("two", 200, 300), ("three", 400, 500)));
}

// Catches: an edit inventing an empty transcript for a document that had none.
#[test]
fn a_document_without_a_transcript_stays_without_one() {
    let dir = TempDir::new("none");
    let d = silent_doc(&dir, 48_000);
    let e = edit::delete(&d, 0..150, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert!(e.state.transcript().is_none());
}

// Catches: the moved transcript losing its sample rate or version.
#[test]
fn the_moved_transcript_keeps_its_rate_and_version() {
    let dir = TempDir::new("rate-version");
    let d = silent_doc(&dir, 48_000).with_transcript(Some(three_words()));
    let e = edit::delete(&d, 0..150, ChannelMask::ALL, Crossfades::NONE).unwrap();
    let t = e.state.transcript().unwrap();
    assert_eq!(t.sample_rate, 48_000);
    assert_eq!(t.version, transcript::VERSION);
}

// Catches: undo not restoring the original word frames after a delete moved them.
#[test]
fn undo_after_a_delete_restores_the_original_words() {
    let dir = TempDir::new("undo-delete");
    let mut h = History::new(silent_doc(&dir, 48_000), u64::MAX);
    h.push_transcript("Transcribe", three_words());
    let e = edit::delete(h.current(), 0..150, ChannelMask::ALL, Crossfades::NONE).unwrap();
    h.push_edit(e);
    assert_eq!(words_of(h.current()), vec![("two".to_string(), 50, 150), ("three".to_string(), 250, 350)]);
    let undone = h.undo().unwrap();
    assert_eq!(words_of(&undone), w3(("one", 0, 100), ("two", 200, 300), ("three", 400, 500)));
}
