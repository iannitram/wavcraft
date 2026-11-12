//! Background transcription worker (L1.11): runs `wavcraft_core::transcript::transcribe` on a
//! worker thread with progress and Cancel, streamed from the document (statspanel.rs pattern).
//! `WAVCRAFT_FAKE_SPEECH=1` selects the fake speech engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wavcraft_core::DocState;
use wavcraft_core::transcript::{FakeEngine, Transcript, transcribe};

/// Progress is kept in thousandths.
const STEPS: u32 = 1000;

struct Job {
    result: Receiver<Result<Option<Transcript>, String>>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    state: Arc<DocState>,
}

/// The transcription run: at most one scan at a time.
#[derive(Default)]
pub struct TranscriptRun {
    job: Option<Job>,
    error: Option<String>,
}

impl TranscriptRun {
    /// Start transcribing `state` on a worker thread. A running scan is cancelled.
    pub fn start(&mut self, state: Arc<DocState>, ctx: &egui::Context) {
        let fake = std::env::var("WAVCRAFT_FAKE_SPEECH").as_deref() == Ok("1");
        self.start_with(state, ctx, fake);
    }

    /// [`TranscriptRun::start`] with the engine chosen by the caller (`fake` selects the fake engine).
    pub fn start_with(&mut self, state: Arc<DocState>, ctx: &egui::Context, fake: bool) {
        self.cancel();
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (s, p, c, ctx) = (state.clone(), progress.clone(), cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("transcribe".into()).spawn(move || {
            let mut report = |f: f64| {
                let step = (f.clamp(0.0, 1.0) * f64::from(STEPS)) as u32;
                if p.swap(step, Ordering::Relaxed) != step {
                    ctx.request_repaint();
                }
            };
            let out = if fake {
                let mut engine = FakeEngine;
                transcribe(&s, &mut engine, &c, &mut report).map_err(|e| e.to_string())
            } else {
                Err("no speech engine available; set WAVCRAFT_FAKE_SPEECH=1".to_string())
            };
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.error = None;
                self.job = Some(Job { result: rx, progress, cancel, state });
            }
            Err(e) => self.error = Some(format!("could not start transcription: {e}")),
        }
    }

    /// Stop the running transcription, if any.
    pub fn cancel(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// How far the running scan is, 0 to 1.
    pub fn progress(&self) -> f32 {
        self.job.as_ref().map_or(0.0, |j| j.progress.load(Ordering::Relaxed).min(STEPS) as f32 / STEPS as f32)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Take a finished scan's result. Returns `Some((scanned_state, transcript))` when a run ended successfully with a transcript.
    pub fn poll(&mut self) -> Option<(Arc<DocState>, Transcript)> {
        let job = self.job.as_ref()?;
        match job.result.try_recv() {
            Ok(Ok(Some(transcript))) => {
                if let Some(j) = self.job.take() {
                    log::info!(target: "wavcraft", "transcription finished: {} words", transcript.words.len());
                    return Some((j.state, transcript));
                }
                None
            }
            Ok(Ok(None)) => {
                self.job = None;
                None
            }
            Ok(Err(e)) => {
                self.job = None;
                self.error = Some(format!("Transcription: {e}"));
                None
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("Transcription: the worker stopped unexpectedly".to_string());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use wavcraft_core::doc::{Channel, Piece, SourceFormat};
    use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

    const RATE: u32 = 48_000;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!("wavcraft-transcript-run-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_doc(dir: &TempDir, seconds: usize) -> Arc<DocState> {
        let n = seconds * RATE as usize;
        let tone: Vec<f32> = (0..n).map(|i| (0.25 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / f64::from(RATE)).sin()) as f32).collect();
        let store = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
        let channels = [tone.clone(), tone]
            .iter()
            .map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(store.append(b).unwrap()).unwrap())))
            .collect();
        Arc::new(DocState::new(store, channels, RATE, SourceFormat::synthetic(RATE, 2)).unwrap())
    }

    #[test]
    fn a_run_with_fake_engine_ends_with_transcript() {
        let dir = TempDir::new("fake");
        let state = test_doc(&dir, 65);
        let mut run = TranscriptRun::default();
        run.start_with(state.clone(), &egui::Context::default(), true);
        assert!(run.running());
        let until = Instant::now() + Duration::from_secs(60);
        let mut result = None;
        while result.is_none() {
            assert!(Instant::now() < until, "the transcription did not finish");
            result = run.poll();
            if result.is_none() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(!run.running());
        assert_eq!(run.error(), None);
        let (done_state, transcript) = result.unwrap();
        assert!(Arc::ptr_eq(&done_state, &state));
        assert_eq!(transcript.words.len(), 3);
        assert_eq!(transcript.words[0].start, 48_000);
        assert_eq!(transcript.words[0].end, 72_000);
    }

    #[test]
    fn a_cancelled_run_returns_none() {
        let dir = TempDir::new("cancel");
        let state = test_doc(&dir, 65);
        let mut run = TranscriptRun::default();
        run.start_with(state, &egui::Context::default(), true);
        run.cancel();
        assert!(!run.running());
        assert_eq!(run.progress(), 0.0);
        std::thread::sleep(Duration::from_millis(100));
        assert!(run.poll().is_none());
        assert_eq!(run.error(), None);
    }
}
