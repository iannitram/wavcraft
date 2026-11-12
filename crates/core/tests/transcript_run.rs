//! L1.6 (#1057): the transcription runner over a speech engine, with the fake engine.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use wavcraft_core::doc::{Channel, DocState, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::transcript::{FakeEngine, SpeechEngine, Word, transcribe};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-transcript-run-test-{}-{name}", std::process::id()));
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

/// `secs` of stereo silence at 48 kHz (zero-block pieces: nothing is written to disk).
fn silent_doc(dir: &TempDir, secs: u64) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let chans = (0..2)
        .map(|_| {
            let mut c = Channel::new();
            c.push_zeros(secs * 48_000);
            c
        })
        .collect();
    DocState::new(s, chans, 48_000, SourceFormat::synthetic(48_000, 2)).unwrap()
}

#[test]
fn fake_words_land_at_window_offsets() {
    let dir = TempDir::new("offsets");
    let doc = silent_doc(&dir, 65);
    let t = transcribe(&doc, &mut FakeEngine, &AtomicBool::new(false), &mut |_| {}).unwrap().unwrap();
    assert_eq!(t.sample_rate, 48_000);
    let got: Vec<(&str, u64, u64)> = t.words.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect();
    assert_eq!(got, vec![("w0", 48_000, 72_000), ("w1", 1_488_000, 1_512_000), ("w2", 2_928_000, 2_952_000)]);
    assert!(t.check().is_ok());
}

/// Sets `cancel` while it handles the first window, as the Cancel button would mid-run.
struct CancelsAfterOne<'a> {
    cancel: &'a AtomicBool,
    calls: &'a AtomicUsize,
}

impl SpeechEngine for CancelsAfterOne<'_> {
    fn window(&mut self, index: usize, samples: &[f32]) -> io::Result<Vec<Word>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
        FakeEngine.window(index, samples)
    }
}

#[test]
fn cancel_stops_before_the_next_window() {
    let dir = TempDir::new("cancel");
    let doc = silent_doc(&dir, 65);
    let (cancel, calls) = (AtomicBool::new(false), AtomicUsize::new(0));
    let mut engine = CancelsAfterOne { cancel: &cancel, calls: &calls };
    let got = transcribe(&doc, &mut engine, &cancel, &mut |_| {}).unwrap();
    assert_eq!(got, None);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no window after the cancel");

    // Cancelled before it starts: no window at all.
    let calls = AtomicUsize::new(0);
    let cancel = AtomicBool::new(true);
    let mut engine = CancelsAfterOne { cancel: &cancel, calls: &calls };
    assert_eq!(transcribe(&doc, &mut engine, &cancel, &mut |_| {}).unwrap(), None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn progress_reaches_one() {
    let dir = TempDir::new("progress");
    let doc = silent_doc(&dir, 65);
    let mut seen = Vec::new();
    transcribe(&doc, &mut FakeEngine, &AtomicBool::new(false), &mut |p| seen.push(p)).unwrap().unwrap();
    assert_eq!(seen.last().copied(), Some(1.0));
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "never goes back: {seen:?}");
    assert!(seen.iter().all(|p| (0.0..=1.0).contains(p)));
}

/// Returns words out of order, overlapping, empty and past the window, as a real model can.
struct Messy;

impl SpeechEngine for Messy {
    fn window(&mut self, _index: usize, _samples: &[f32]) -> io::Result<Vec<Word>> {
        let w = |t: &str, s, e| Word { text: t.to_string(), start: s, end: e };
        Ok(vec![w(" b ", 20_000, 30_000), w("a", 10_000, 21_000), w("", 40_000, 41_000), w("c", 25_000, 26_000), w("late", 470_000, 9_000_000)])
    }
}

#[test]
fn engine_words_are_cleaned_into_a_valid_transcript() {
    let dir = TempDir::new("messy");
    let doc = silent_doc(&dir, 10);
    let t = transcribe(&doc, &mut Messy, &AtomicBool::new(false), &mut |_| {}).unwrap().unwrap();
    assert!(t.check().is_ok());
    let got: Vec<(&str, u64, u64)> = t.words.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect();
    // Sorted by start, trimmed, empty text dropped, an overlap starts at the previous end, and a
    // word past the end of the 10 s document is dropped.
    assert_eq!(got, vec![("a", 30_000, 63_000), ("b", 63_000, 90_000)]);
}

struct Fails;

impl SpeechEngine for Fails {
    fn window(&mut self, _index: usize, _samples: &[f32]) -> io::Result<Vec<Word>> {
        Err(io::Error::other("model broke"))
    }
}

#[test]
fn an_engine_error_is_returned() {
    let dir = TempDir::new("fails");
    let doc = silent_doc(&dir, 5);
    let err = transcribe(&doc, &mut Fails, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("model broke"));
}
