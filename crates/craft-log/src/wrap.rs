//! The window app's logger: upstream's own logger (its file, folder, rotation and format kept
//! exactly), wrapped to add the repeat collapse and the ring of recent lines.

use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use crate::collapse::{self, Collapser};
use crate::line::{Line, timestamp};

/// A logger around `inner`: `inner` keeps deciding what is enabled and where lines go; this adds
/// the repeat collapse ([`Collapser`]) and keeps every line it passes on in the ring.
pub struct Wrapped {
    inner: &'static dyn log::Log,
    collapse: Mutex<Collapser>,
}

impl Wrapped {
    pub fn new(inner: &'static dyn log::Log) -> Wrapped {
        Wrapped { inner, collapse: Mutex::new(Collapser::default()) }
    }

    fn note(&self, (times, level, target, so_far): (u64, log::Level, String, bool)) {
        let text = collapse::note_text(times, so_far);
        remember(level, &target, &text);
        self.inner.log(&log::Record::builder().level(level).target(&target).args(format_args!("{text}")).build());
    }
}

fn remember(level: log::Level, target: &str, message: &str) {
    let thread = std::thread::current();
    let line = Line { ts: &timestamp(SystemTime::now()), level, pid: None, thread: thread.name().unwrap_or("?"), target, message };
    collapse::remember(&line.text());
}

impl log::Log for Wrapped {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.inner.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let action = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).observe(record.level(), record.target(), &message);
        if let Some(note) = action.note {
            self.note(note);
        }
        if action.swallow {
            return;
        }
        remember(record.level(), record.target(), &message);
        self.inner.log(record);
    }

    fn flush(&self) {
        let pending = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).pending();
        if let Some(note) = pending {
            self.note(note);
        }
        self.inner.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;
    use std::sync::Mutex;

    struct Sink(Mutex<Vec<String>>);
    impl log::Log for Sink {
        fn enabled(&self, m: &log::Metadata<'_>) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record<'_>) {
            self.0.lock().unwrap().push(format!("{} {}", r.level(), r.args()));
        }
        fn flush(&self) {}
    }

    #[test]
    fn repeats_reach_the_inner_logger_once_with_a_count() {
        let sink: &'static Sink = Box::leak(Box::new(Sink(Mutex::new(Vec::new()))));
        let w = Wrapped::new(sink);
        let rec = |lvl, msg: &str| {
            w.log(&log::Record::builder().level(lvl).target("t").args(format_args!("{msg}")).build());
        };
        rec(log::Level::Warn, "wrap-test same");
        rec(log::Level::Warn, "wrap-test same");
        rec(log::Level::Warn, "wrap-test same");
        rec(log::Level::Debug, "wrap-test filtered by the inner logger");
        rec(log::Level::Warn, "wrap-test other");
        rec(log::Level::Warn, "wrap-test other");
        w.flush();
        let got = sink.0.lock().unwrap().clone();
        assert_eq!(got, ["WARN wrap-test same", "WARN previous line repeated 2 times", "WARN wrap-test other", "WARN previous line repeated 1 times"]);
        assert!(collapse::recent(collapse::RING_LINES).iter().any(|l| l.ends_with("t: wrap-test other")));
    }
}
