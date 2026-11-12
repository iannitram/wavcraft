//! The transcription runner (L1.6): the document's 30-second windows ([`super::mel::windows`])
//! through a [`SpeechEngine`], one window at a time, the words mapped back to source frames.
//! A worker thread runs it; the cancel flag is read before each window.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use super::mel::{self, SAMPLE_RATE, WINDOW_SAMPLES};
use super::{Transcript, Word};
use crate::doc::DocState;

/// A speech-to-text model, fed one window at a time.
pub trait SpeechEngine {
    /// The words of window `index`: `samples` is [`WINDOW_SAMPLES`] samples of 16 kHz mono
    /// ([`mel::log_mel`] turns it into the model's input). Word times are 16 kHz samples from the
    /// window's start. The runner sorts, trims and clips what comes back, so an engine may return
    /// words in any order.
    fn window(&mut self, index: usize, samples: &[f32]) -> io::Result<Vec<Word>>;
}

/// A stand-in engine for tests and the app's fake mode (`WAVCRAFT_FAKE_SPEECH=1`): one word
/// `w<k>` per window `k`, from 1.0 s to 1.5 s into it. It ignores the audio.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeEngine;

impl SpeechEngine for FakeEngine {
    fn window(&mut self, index: usize, _samples: &[f32]) -> io::Result<Vec<Word>> {
        let rate = u64::from(SAMPLE_RATE);
        Ok(vec![Word { text: format!("w{index}"), start: rate, end: rate * 3 / 2 }])
    }
}

/// Transcribe `doc` with `engine`. `progress` gets 0..=1 (1 at the end); `Ok(None)` when `cancel`
/// was set before a window started. The result is at the document's sample rate and always
/// passes [`Transcript::check`]: words are sorted by start, their text trimmed, empty ones and
/// ones past the end of the document dropped, and a word that starts before the previous one
/// ends starts at that end instead (dropped if nothing is left of it).
pub fn transcribe(
    doc: &DocState,
    engine: &mut dyn SpeechEngine,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> io::Result<Option<Transcript>> {
    let rate = doc.sample_rate();
    let len = doc.len_frames();
    let windows = mel::windows(doc);
    let total = windows.total();
    let mut words: Vec<Word> = Vec::new();
    progress(0.0);
    for window in windows {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let window = window?;
        let base = window.index as u64 * WINDOW_SAMPLES as u64;
        let mut got = engine.window(window.index, &window.samples)?;
        got.sort_by_key(|w| (w.start, w.end));
        for w in got {
            let text = w.text.trim();
            if text.is_empty() || w.end <= w.start {
                continue;
            }
            let prev_end = words.last().map_or(0, |p| p.end);
            let start = mel::source_frame(base.saturating_add(w.start), rate).max(prev_end);
            let end = mel::source_frame(base.saturating_add(w.end), rate).min(len);
            if start < end {
                words.push(Word { text: text.to_string(), start, end });
            }
        }
        progress((window.index + 1) as f64 / total as f64);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    progress(1.0);
    Ok(Some(Transcript::new(rate, words)))
}
