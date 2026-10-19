//! The stall watch: listens to craft-stage scopes, warns when one passes [`STALL_AFTER`] (and
//! every [`STALL_EVERY`] after that) naming its current stage and the log file, and logs each
//! scope's total and per-stage times when it ends. It reports; it never cancels anything.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// A scope running this long gets its first warning.
pub const STALL_AFTER: Duration = Duration::from_secs(10);
/// Then one warning this often while it keeps running.
pub const STALL_EVERY: Duration = Duration::from_secs(30);
/// A scope that ends after at least this long logs its times at info (shorter ones at debug).
pub const INFO_AFTER: Duration = Duration::from_secs(1);
/// How often the watch thread looks.
const TICK: Duration = Duration::from_millis(500);

struct Active {
    name: &'static str,
    thread: String,
    start: Instant,
    stage: &'static str,
    stage_start: Instant,
    numbers: Vec<(&'static str, f64)>,
    times: Vec<(&'static str, Duration)>,
    last_warning: Option<Instant>,
}

/// The scopes in progress, and the clock-free logic around them (tests pass their own instants).
#[derive(Default)]
pub struct Watch {
    active: Mutex<HashMap<u64, Active>>,
}

fn secs(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

fn numbers_text(numbers: &[(&'static str, f64)]) -> String {
    if numbers.is_empty() {
        return String::new();
    }
    let mut s = String::from(" [");
    for (i, (k, v)) in numbers.iter().enumerate() {
        let _ = write!(s, "{}{k}={v}", if i == 0 { "" } else { " " });
    }
    s.push(']');
    s
}

impl Watch {
    pub fn enter_at(&self, id: u64, name: &'static str, thread: &str, now: Instant) {
        let a = Active {
            name,
            thread: thread.to_owned(),
            start: now,
            stage: "start",
            stage_start: now,
            numbers: Vec::new(),
            times: Vec::new(),
            last_warning: None,
        };
        self.active.lock().unwrap_or_else(PoisonError::into_inner).insert(id, a);
    }

    pub fn stage_at(&self, id: u64, stage: &'static str, numbers: &[(&'static str, f64)], now: Instant) {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(a) = active.get_mut(&id) else { return };
        let spent = now.saturating_duration_since(a.stage_start);
        add_time(&mut a.times, a.stage, spent);
        a.stage = stage;
        a.stage_start = now;
        a.numbers = numbers.to_vec();
    }

    /// The scope ended: its closing line (level, text), or `None` for an unknown id.
    /// `stage: import took 12.4 s (start 0.0 s, parse 0.3 s, relink 12.1 s)`
    pub fn leave_at(&self, id: u64, now: Instant) -> Option<(log::Level, String)> {
        let mut a = self.active.lock().unwrap_or_else(PoisonError::into_inner).remove(&id)?;
        let spent = now.saturating_duration_since(a.stage_start);
        add_time(&mut a.times, a.stage, spent);
        let total = now.saturating_duration_since(a.start);
        let mut text = format!("stage: {} took {} (", a.name, secs(total));
        for (i, (stage, t)) in a.times.iter().enumerate() {
            let _ = write!(text, "{}{stage} {}", if i == 0 { "" } else { ", " }, secs(*t));
        }
        text.push(')');
        text.push_str(&numbers_text(&a.numbers));
        let level = if a.last_warning.is_some() {
            log::Level::Warn
        } else if total >= INFO_AFTER {
            log::Level::Info
        } else {
            log::Level::Debug
        };
        Some((level, text))
    }

    /// Warnings due at `now` for scopes past [`STALL_AFTER`]; `log` names the log file.
    pub fn check_at(&self, now: Instant, log: Option<&std::path::Path>) -> Vec<String> {
        let mut out = Vec::new();
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        let mut ids: Vec<u64> = active.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(a) = active.get_mut(&id) else { continue };
            let running = now.saturating_duration_since(a.start);
            let due = match a.last_warning {
                None => running >= STALL_AFTER,
                Some(t) => now.saturating_duration_since(t) >= STALL_EVERY,
            };
            if !due {
                continue;
            }
            a.last_warning = Some(now);
            let in_stage = now.saturating_duration_since(a.stage_start);
            let log = log.map(|p| format!("; log: {}", p.display())).unwrap_or_default();
            out.push(format!(
                "stage: {} still running after {}, in stage \"{}\" for {}{} (thread {}{log})",
                a.name,
                secs(running),
                a.stage,
                secs(in_stage),
                numbers_text(&a.numbers),
                a.thread
            ));
        }
        out
    }
}

fn add_time(times: &mut Vec<(&'static str, Duration)>, stage: &'static str, d: Duration) {
    match times.iter_mut().find(|(s, _)| *s == stage) {
        Some((_, t)) => *t = t.saturating_add(d),
        None => times.push((stage, d)),
    }
}

/// The watch registered with craft-stage, logging with the app's name as the target (so the
/// app's default filter lets it through).
struct Listener {
    target: &'static str,
    watch: Watch,
}

impl craft_stage::Hook for Listener {
    fn enter(&self, id: u64, name: &'static str) {
        let thread = std::thread::current();
        self.watch.enter_at(id, name, thread.name().unwrap_or("?"), Instant::now());
    }

    fn stage(&self, id: u64, stage: &'static str, numbers: &[(&'static str, f64)]) {
        self.watch.stage_at(id, stage, numbers, Instant::now());
    }

    fn leave(&self, id: u64) {
        if let Some((level, text)) = self.watch.leave_at(id, Instant::now()) {
            log::log!(target: self.target, level, "{text}");
        }
    }
}

static LISTENER: OnceLock<Listener> = OnceLock::new();

/// Register the stall watch with craft-stage and start its thread (once per process).
pub fn start(target: &'static str) {
    let listener = LISTENER.get_or_init(|| Listener { target, watch: Watch::default() });
    if !craft_stage::set_hook(listener) {
        return;
    }
    let spawned = std::thread::Builder::new().name("craft-stall".into()).spawn(move || {
        loop {
            std::thread::sleep(TICK);
            for line in listener.watch.check_at(Instant::now(), crate::log_path().as_deref()) {
                log::warn!(target: listener.target, "{line}");
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!(target: target, "stage: no stall watch ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn a_short_scope_logs_its_stage_table_at_debug() {
        let w = Watch::default();
        let t0 = Instant::now();
        w.enter_at(1, "import", "main", t0);
        w.stage_at(1, "parse", &[], t0 + S / 10);
        w.stage_at(1, "relink", &[("clips", 12.0)], t0 + S / 2);
        let (level, text) = w.leave_at(1, t0 + S * 9 / 10).unwrap();
        assert_eq!(level, log::Level::Debug);
        assert_eq!(text, "stage: import took 0.9 s (start 0.1 s, parse 0.4 s, relink 0.4 s) [clips=12]");
        assert!(w.leave_at(1, t0).is_none(), "gone after leaving");
    }

    #[test]
    fn a_stall_is_warned_at_10_s_then_every_30_s_naming_stage_and_log() {
        let w = Watch::default();
        let t0 = Instant::now();
        w.enter_at(7, "select.subject", "worker-2", t0);
        w.stage_at(7, "grabcut", &[("iteration", 3.0)], t0 + S * 2);
        assert!(w.check_at(t0 + S * 9, None).is_empty());
        let log = Path::new("/tmp/app-cli.log");
        let first = w.check_at(t0 + S * 10, Some(log));
        assert_eq!(
            first,
            ["stage: select.subject still running after 10.0 s, in stage \"grabcut\" for 8.0 s [iteration=3] (thread worker-2; log: /tmp/app-cli.log)"]
        );
        assert!(w.check_at(t0 + S * 30, Some(log)).is_empty());
        assert_eq!(w.check_at(t0 + S * 40, None).len(), 1);
        // a stage visited twice adds up, and a scope that stalled closes at warn
        w.stage_at(7, "load", &[], t0 + S * 41);
        w.stage_at(7, "grabcut", &[], t0 + S * 42);
        let (level, text) = w.leave_at(7, t0 + S * 45).unwrap();
        assert_eq!(level, log::Level::Warn);
        assert_eq!(text, "stage: select.subject took 45.0 s (start 2.0 s, grabcut 42.0 s, load 1.0 s)");
    }

    #[test]
    fn a_second_long_scope_logs_at_info() {
        let w = Watch::default();
        let t0 = Instant::now();
        w.enter_at(3, "export", "main", t0);
        assert_eq!(w.leave_at(3, t0 + S * 3).map(|(l, _)| l), Some(log::Level::Info));
    }
}
