//! L3.3 (#1614): deleting transcript words is one ordinary, undoable edit.

use std::path::PathBuf;
use std::sync::Arc;

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::transcript::{self, Transcript, Word};

const RATE: u32 = 48_000;
const FRAMES: usize = 1_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-transcript-edit-test-{}-{name}", std::process::id()));
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

fn fixture(dir: &TempDir) -> (DocState, Vec<f32>) {
    let samples: Vec<f32> = (0..FRAMES).map(|i| i as f32 / FRAMES as f32).collect();
    let store = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let block = store.append(&samples).unwrap();
    let channel = Channel::from_pieces([Piece::whole(block).unwrap()]);
    let doc = DocState::new(store, vec![channel], RATE, SourceFormat::synthetic(RATE, 1)).unwrap();
    let words = (0..10).map(|i| Word { text: format!("word{i}"), start: i * 100, end: i * 100 + 80 }).collect();
    (doc.with_transcript(Some(Arc::new(Transcript::new(RATE, words)))), samples)
}

fn samples(doc: &DocState) -> Vec<f32> {
    let mut out = vec![0.0; doc.len_frames() as usize];
    assert_eq!(doc.read(0, 0, &mut out).unwrap(), out.len());
    out
}

fn words(doc: &DocState) -> Vec<Word> {
    doc.transcript().unwrap().words.clone()
}

// Catches: word deletion choosing the wrong frame range or crossfades, or removing the wrong
// number of words compared with a hand-cut selection.
#[test]
fn deleting_a_sentence_equals_the_hand_cut() {
    let dir = TempDir::new("sentence");
    let (doc, _) = fixture(&dir);
    let crossfades = Crossfades::default();
    let range = doc.transcript().unwrap().range_of(3, 7);
    let expected = edit::delete(&doc, range, ChannelMask::ALL, crossfades).unwrap();
    let actual = transcript::delete_words(&doc, 3, 7, crossfades).unwrap();

    let expected_samples = samples(&expected.state);
    let actual_samples = samples(&actual.state);
    assert_eq!(
        actual_samples.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>(),
        expected_samples.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>()
    );
    assert_eq!(actual.state.transcript().unwrap().words.len(), doc.transcript().unwrap().words.len() - 5);
}

// Catches: deleting words taking more than one History step or undo failing to restore audio and
// transcript state.
#[test]
fn undo_restores_audio_and_transcript() {
    let dir = TempDir::new("undo");
    let (doc, original_samples) = fixture(&dir);
    let original_words = words(&doc);
    let mut history = History::new(doc, u64::MAX);
    let before = history.steps().len();
    let edit = transcript::delete_words(history.current(), 3, 7, Crossfades::NONE).unwrap();
    history.push_edit(edit);
    assert_eq!(history.steps().len(), before + 1);

    let undone = history.undo().unwrap();
    assert_eq!(samples(&undone), original_samples);
    assert_eq!(words(&undone), original_words);
}

// Catches: a length-preserving edit dropping or moving the transcript.
#[test]
fn a_same_length_edit_keeps_the_transcript() {
    let dir = TempDir::new("same-length");
    let (doc, _) = fixture(&dir);
    let original_words = words(&doc);
    let edited = edit::silence(&doc, 250..300, ChannelMask::ALL, Crossfades::NONE).unwrap();
    assert_eq!(words(&edited.state), original_words);
}
