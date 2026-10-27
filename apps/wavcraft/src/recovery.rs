//! Auto Save and crash recovery in the window (A239): keeps the session's journal current while
//! the owner edits, and, at start-up, offers to recover the work of a run that ended without
//! closing its session. The journal, the recovery and the safety rules are in
//! `wavcraft_core::autosave`; this file only drives them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant, SystemTime};

use wavcraft_core::actions::Editor;
use wavcraft_core::autosave::{Autosaver, Recoverable, Snapshot, recoverable_sessions};

/// The default for how often a journal is handed to the writer (the Auto Save preference
/// overrides it). Edits in between are in the next one.
const INTERVAL: Duration = Duration::from_secs(wavcraft_core::settings::DEFAULT_INTERVAL_SECS);

/// What the owner chose in the recovery window.
pub enum Choice {
    Recover(usize),
    Discard(usize),
}

pub struct Recovery {
    /// Sessions a crashed run left that hold a journal (found once, at start-up).
    pub offers: Vec<Recoverable>,
    /// The scan at start-up is done.
    pub scanned: bool,
    scan: Option<Receiver<Vec<Recoverable>>>,
    /// The window was closed with "Later".
    dismissed: bool,
    /// The offer being asked "discard for good?".
    confirming: Option<usize>,
    /// The open document's journal writer, and which session folder it is for.
    saver: Option<(PathBuf, Autosaver)>,
    last_sig: Option<(usize, usize, usize, usize, usize)>,
    last_offer: Option<Instant>,
    /// Auto Save preferences: on or off, and the most often a journal is handed over.
    enabled: bool,
    interval: Duration,
}

impl Recovery {
    /// Look for recoverable sessions under `roots` on a worker thread, then sweep the stale
    /// sessions that hold no journal (a sweep never removes a recoverable one).
    pub fn new(roots: Vec<PathBuf>, wake: impl Fn() + Send + 'static) -> Recovery {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("recovery scan".into()).spawn(move || {
            let found: Vec<Recoverable> = roots.iter().flat_map(|r| recoverable_sessions(r)).collect();
            let _ = tx.send(found);
            wake();
            // Then, in this same thread, the sweep of sessions with nothing to recover: two probes
            // of one session's lock at once would each see the other's as a running program.
            for root in &roots {
                wavcraft_core::store::sweep_stale_sessions(Some(root));
            }
        });
        Recovery {
            offers: Vec::new(),
            scanned: spawned.is_err(),
            scan: spawned.ok().map(|_| rx),
            dismissed: false,
            confirming: None,
            saver: None,
            last_sig: None,
            last_offer: None,
            enabled: true,
            interval: INTERVAL,
        }
    }

    /// Apply the Auto Save preferences (called every frame; cheap). Turning Auto Save off stops
    /// new journals; the one already on disk stays, as it is the newest durable state.
    pub fn configure(&mut self, enabled: bool, interval_secs: u64) {
        self.enabled = enabled;
        self.interval = Duration::from_secs(interval_secs.max(1));
    }

    /// Take the scan's answer when it arrives.
    pub fn poll(&mut self) {
        let Some(rx) = &self.scan else { return };
        match rx.try_recv() {
            Ok(found) => {
                if !found.is_empty() {
                    log::info!(target: "wavcraft", "recovery: {} session(s) from a run that ended without closing can be recovered", found.len());
                }
                self.offers = found;
                self.scanned = true;
                self.scan = None;
            }
            Err(TryRecvError::Disconnected) => {
                self.scanned = true;
                self.scan = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Called every frame: keep the journal of the open document current. `unsaved`: the current
    /// step differs from the file on disk. A document nobody has changed gets no journal (a crash
    /// then loses nothing); once there is one, it follows every change, undo included.
    pub fn track(&mut self, editor: Option<&Editor>, unsaved: bool, ctx: &egui::Context) {
        let Some(editor) = editor else { return };
        if !self.enabled {
            return;
        }
        let dir = editor.history.current().store().dir().to_path_buf();
        if self.saver.as_ref().is_none_or(|(d, _)| *d != dir) {
            // A new document: the old writer goes first (so its store can close), then the new one.
            self.saver = None;
            let saver = Autosaver::start(editor.history.current().store());
            // A recovered document is unsaved from the start: its journal is written at once, so
            // the blocks it names are held from now on.
            if unsaved {
                saver.offer(Snapshot::of(&editor.history));
                self.last_offer = Some(Instant::now());
            } else {
                self.last_offer = None;
            }
            self.saver = Some((dir, saver));
            self.last_sig = Some(Snapshot::signature(&editor.history));
            return;
        }
        let Some((_, saver)) = &self.saver else { return };
        let sig = Snapshot::signature(&editor.history);
        if Some(sig) == self.last_sig || !(unsaved || saver.stats().written > 0 || saver.stats().offered > 0) {
            return;
        }
        if let Some(wait) = self.last_offer.and_then(|t| self.interval.checked_sub(t.elapsed())) {
            ctx.request_repaint_after(wait);
            return;
        }
        saver.offer(Snapshot::of(&editor.history));
        self.last_sig = Some(sig);
        self.last_offer = Some(Instant::now());
    }

    /// `offered written failed pending` of the open document's writer.
    pub fn stats(&self) -> (u64, u64, u64, bool) {
        self.saver.as_ref().map_or((0, 0, 0, false), |(_, s)| {
            let st = s.stats();
            (st.offered, st.written, st.failed, st.pending())
        })
    }

    /// Drop an offer that was recovered or discarded.
    pub fn remove(&mut self, index: usize) {
        if index < self.offers.len() {
            self.offers.remove(index);
        }
        self.confirming = None;
    }

    /// The recovery window, while there is anything to offer. What the owner chose, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Choice> {
        if self.dismissed || self.offers.is_empty() {
            return None;
        }
        let mut choice = None;
        let mut later = false;
        egui::Window::new("Recover unsaved work").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.label("WavCraft ended without closing last time. This work was kept:");
                ui.add_space(6.0);
                for (i, o) in self.offers.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        match &o.info {
                            Ok(info) => {
                                let name = info.source_path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned());
                                ui.label(format!(
                                    "{name}: {} steps, {}, {}",
                                    info.labels.len().saturating_sub(1),
                                    wavcraft_core::format_time(info.frames as usize, info.sample_rate),
                                    ago(o.saved_at)
                                ));
                                if ui.button("Recover").clicked() {
                                    choice = Some(Choice::Recover(i));
                                }
                            }
                            Err(why) => {
                                ui.label(format!("Unreadable recovery data ({why}). It is kept, not deleted."));
                            }
                        }
                        if self.confirming == Some(i) {
                            if ui.button("Discard for good").clicked() {
                                choice = Some(Choice::Discard(i));
                            }
                            if ui.button("Keep").clicked() {
                                self.confirming = None;
                            }
                        } else if ui.button("Discard…").clicked() {
                            self.confirming = Some(i);
                        }
                    });
                }
                ui.add_space(6.0);
                if ui.button("Later").clicked() {
                    later = true;
                }
            },
        );
        self.dismissed |= later;
        choice
    }
}

/// "5 minutes ago" for a time the journal was written.
fn ago(t: Option<SystemTime>) -> String {
    let Some(secs) = t.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()) else { return "saved at an unknown time".to_string() };
    match secs {
        0..=59 => "saved moments ago".to_string(),
        60..=3599 => format!("saved {} min ago", secs / 60),
        3600..=86_399 => format!("saved {} h ago", secs / 3600),
        _ => format!("saved {} days ago", secs / 86_400),
    }
}

/// The folders to look in for sessions left behind: the chosen session folder and the default.
pub fn roots(chosen: &Path, default: &Path) -> Vec<PathBuf> {
    let mut v = vec![chosen.to_path_buf()];
    if default != chosen {
        v.push(default.to_path_buf());
    }
    v
}
