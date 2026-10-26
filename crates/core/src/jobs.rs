//! The background job runner (roadmap B003): a long `replace_range`-style operation on a worker
//! thread, with progress, cancel and a result for the window's thread.
//!
//! The worker only reads the state the job started from (an `Arc<DocState>`) and writes new
//! blocks; it never touches the [`History`](crate::doc::history::History). The window's thread
//! polls [`Job::poll`] each frame and, on `Done(Ok(edit))`, pushes the undo step itself. If the
//! history has moved since the job started ([`Job::base`] is no longer its current state) the
//! window should drop the result: the app does not allow edits while a job runs, so this is a
//! guard, not a path.
//!
//! Float headroom (B012) needs nothing here: the store holds `f32`, and no edit clamps, so
//! samples above 0 dBFS stay as they are until Save converts them (`save::quantize` clamps for
//! integer formats; 32-bit float keeps them).

use std::io;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError};

/// Progress is kept in thousandths.
const FULL: u32 = 1000;

/// What [`Job::poll`] found.
pub enum JobPoll {
    Running,
    /// The job ended. This is the only delivery: keep it (a later poll reports an error).
    Done(Result<Edit, EditError>),
}

/// One running (or just finished) background edit.
pub struct Job {
    label: String,
    base: Arc<DocState>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Edit, EditError>>,
}

impl Job {
    /// Start `replace_range(base, range, channels, label, xf, …, producer)` on a worker thread.
    /// `wake` is called when progress moves by a percent and when the job ends (the window's
    /// `request_repaint`).
    pub fn spawn_replace<F>(
        base: Arc<DocState>,
        range: Range<u64>,
        channels: ChannelMask,
        label: &str,
        xf: Crossfades,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
        mut producer: F,
    ) -> Job
    where
        F: FnMut(u64, &[&[f32]], &mut [&mut [f32]]) -> io::Result<()> + Send + 'static,
    {
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (w_base, w_progress, w_cancel, w_label) = (base.clone(), progress.clone(), cancel.clone(), label.to_string());
        let spawned = std::thread::Builder::new().name("job".into()).spawn(move || {
            let (start, total) = (range.start, range.end.saturating_sub(range.start).max(1));
            let mut last = 0;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                edit::replace_range(&w_base, range.clone(), channels, &w_label, xf, &w_cancel, |at, input, output| {
                    producer(at, input, output)?;
                    let done = at + input.first().map_or(0, |c| c.len() as u64) - start;
                    let permille = (((done as f64 / total as f64) * f64::from(FULL)) as u32).min(FULL);
                    // A finished producer is not a finished job: the last 0.1 % is the join.
                    w_progress.fetch_max(permille.min(FULL - 1), Ordering::Relaxed);
                    if permille >= last + 10 {
                        last = permille;
                        if let Some(w) = &wake {
                            w();
                        }
                    }
                    Ok(())
                })
            }));
            let result = outcome.unwrap_or_else(|_| Err(EditError::Io(io::Error::other("the job stopped unexpectedly"))));
            if result.is_ok() {
                w_progress.store(FULL, Ordering::Relaxed);
            }
            let _ = tx.send(result);
            if let Some(w) = &wake {
                w();
            }
        });
        if let Err(e) = spawned {
            // No thread: the first poll reports it.
            let (tx, rx) = channel();
            let _ = tx.send(Err(EditError::Io(e)));
            return Job { label: label.to_string(), base, progress, cancel, result: rx };
        }
        Job { label: label.to_string(), base, progress, cancel, result: rx }
    }

    /// The History label the edit will have.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The state the job started from.
    pub fn base(&self) -> &Arc<DocState> {
        &self.base
    }

    /// 0.0 to 1.0.
    pub fn progress(&self) -> f64 {
        f64::from(self.progress.load(Ordering::Relaxed)) / f64::from(FULL)
    }

    /// Progress in thousandths (0 to 1000).
    pub fn permille(&self) -> u32 {
        self.progress.load(Ordering::Relaxed)
    }

    /// Ask the worker to stop at its next chunk; the result is then `Err(EditError::Cancelled)`.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Whether the worker has an answer yet; take it when it does.
    pub fn poll(&mut self) -> JobPoll {
        match self.result.try_recv() {
            Ok(r) => JobPoll::Done(r),
            Err(TryRecvError::Empty) => JobPoll::Running,
            Err(TryRecvError::Disconnected) => JobPoll::Done(Err(EditError::Io(io::Error::other("the job stopped unexpectedly")))),
        }
    }
}
