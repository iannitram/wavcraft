//! Effects › Run Episode Check (K1): the scan runs on a worker thread, streamed from the
//! document's store (never the whole file in memory), with progress and Cancel. The detectors are
//! in `wavcraft_core::check`. The window drives a [`CheckRun`] the way it drives the Sample
//! Statistics scan in statspanel.rs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wavcraft_core::DocState;
use wavcraft_core::check::{self, Report};

/// Progress is kept in thousandths.
const STEPS: u32 = 1000;

struct Job {
    result: Receiver<Result<Option<Report>, String>>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    state: Arc<DocState>,
}

/// A finished check, with the document step it scanned.
pub struct Checked {
    pub state: Arc<DocState>,
    pub report: Report,
}

/// The Episode Check run: at most one scan at a time, and the last report.
#[derive(Default)]
pub struct CheckRun {
    job: Option<Job>,
    checked: Option<Checked>,
    error: Option<String>,
}

impl CheckRun {
    /// Start a scan of the whole of `state` on a worker thread. A running scan is cancelled and
    /// the last report is dropped: the new run replaces it.
    pub fn start(&mut self, state: Arc<DocState>, ctx: &egui::Context) {
        self.cancel();
        self.checked = None;
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (s, p, c, ctx) = (state.clone(), progress.clone(), cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("episode check".into()).spawn(move || {
            let mut report = |f: f64| {
                let step = (f.clamp(0.0, 1.0) * f64::from(STEPS)) as u32;
                // The passes report once per short window: repaint only when the bar would move.
                if p.swap(step, Ordering::Relaxed) != step {
                    ctx.request_repaint();
                }
            };
            let out = check::scan_doc(&s, &c, &mut report).map_err(|e| e.to_string());
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.error = None;
                self.job = Some(Job { result: rx, progress, cancel, state });
            }
            Err(e) => self.error = Some(format!("could not start Episode Check: {e}")),
        }
    }

    /// Stop the running scan, if any. Its result is never taken: the run keeps no report.
    pub fn cancel(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// How far the running scan is, 0 to 1 (0 when none runs).
    pub fn progress(&self) -> f32 {
        self.job
            .as_ref()
            .map_or_else(|| if self.checked.is_some() { 1.0 } else { 0.0 }, |j| j.progress.load(Ordering::Relaxed).min(STEPS) as f32 / STEPS as f32)
    }

    /// Take a finished scan's result (call every frame). True when a run ended this call.
    pub fn poll(&mut self) -> bool {
        let Some(job) = &self.job else { return false };
        match job.result.try_recv() {
            Ok(Ok(Some(report))) => {
                if let Some(j) = self.job.take() {
                    log::info!(target: "wavcraft", "episode check: {} findings{}", report.findings.len(), if report.truncated { " (truncated)" } else { "" });
                    self.checked = Some(Checked { state: j.state, report });
                }
            }
            Ok(Ok(None)) => self.job = None,
            Ok(Err(e)) => {
                self.job = None;
                self.error = Some(format!("Episode Check: {e}"));
            }
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("Episode Check: the scan stopped unexpectedly".to_string());
            }
        }
        true
    }

    /// The last finished check.
    pub fn checked(&self) -> Option<&Checked> {
        self.checked.as_ref()
    }

    /// The last finished check's report.
    pub fn report(&self) -> Option<&Report> {
        self.checked.as_ref().map(|c| &c.report)
    }

    /// Why the last run failed, if it did.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl Drop for CheckRun {
    /// A run dropped with the window or the document stops its worker instead of scanning on.
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use wavcraft_core::check::Kind;
    use wavcraft_core::doc::{Channel, Piece, SourceFormat};
    use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

    const RATE: u32 = 48_000;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("wavcraft-check-panel-test-{}-{name}", std::process::id()));
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

    /// `seconds` of a quiet stereo 220 Hz tone, with 20 ms of full scale on the left from 3.0 s.
    fn clipped_doc(dir: &TempDir, seconds: usize) -> Arc<DocState> {
        let n = seconds * RATE as usize;
        let tone: Vec<f32> = (0..n).map(|i| (0.25 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / f64::from(RATE)).sin()) as f32).collect();
        let mut left = tone.clone();
        left[144_000..144_960].fill(1.0);
        let store = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
        let channels = [left, tone]
            .iter()
            .map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(store.append(b).unwrap()).unwrap())))
            .collect();
        Arc::new(DocState::new(store, channels, RATE, SourceFormat::synthetic(RATE, 2)).unwrap())
    }

    fn wait(run: &mut CheckRun) {
        let until = Instant::now() + Duration::from_secs(120);
        while !run.poll() {
            assert!(Instant::now() < until, "the check did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_run_ends_with_the_report() {
        let dir = TempDir::new("report");
        let state = clipped_doc(&dir, 20);
        let mut run = CheckRun::default();
        run.start(state.clone(), &egui::Context::default());
        assert!(run.running());
        wait(&mut run);
        assert!(!run.running());
        assert_eq!(run.error(), None);
        let report = run.report().unwrap();
        let clips: Vec<_> = report.findings.iter().filter(|f| f.kind == Kind::Clipping).collect();
        assert_eq!(clips.len(), 1, "{clips:?}");
        assert_eq!((clips[0].start, clips[0].channel), (144_000, Some(0)));
        assert!(run.checked().is_some_and(|c| Arc::ptr_eq(&c.state, &state)));
    }

    #[test]
    fn a_cancelled_run_keeps_no_report() {
        let dir = TempDir::new("cancel");
        let state = clipped_doc(&dir, 20);
        let mut run = CheckRun::default();
        run.start(state, &egui::Context::default());
        run.cancel();
        assert!(!run.running());
        assert_eq!(run.progress(), 0.0);
        // Whatever the worker had done by then, its result never becomes the report.
        std::thread::sleep(Duration::from_millis(200));
        assert!(!run.poll());
        assert!(run.report().is_none());
        assert_eq!(run.error(), None);
    }
}
