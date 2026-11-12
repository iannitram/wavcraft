//! L1.3 (#1057): the transcript is part of a document state, so undo and redo carry it.

use std::path::PathBuf;
use std::sync::Arc;

use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::transcript::{Transcript, Word};

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

#[test]
fn a_same_length_edit_keeps_the_transcript_and_a_delete_drops_it() {
    use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades};
    let dir = TempDir::new("edits");
    let mut h = History::new(silent_doc(&dir, 48_000), u64::MAX);
    h.push_transcript("Transcribe", three_words());
    // Silence moves no audio: the word times still hold.
    let e = edit::silence(h.current(), 100..200, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(e.state.transcript().map(|t| t.words.len()), Some(3));
    h.push_edit(e);
    // A delete moves later audio: the words cannot follow yet (L3), so the transcript goes.
    let e = edit::delete(h.current(), 0..150, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert!(e.state.transcript().is_none());
    h.push_edit(e);
    // Undo brings it back with the audio.
    assert_eq!(h.undo().unwrap().transcript().map(|t| t.words.len()), Some(3));
}
