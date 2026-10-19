//! A logger for tests: `let _log = craft_log::testing::capture();` at the top of a test keeps
//! that test's log lines, and when the test fails (panics) prints its last
//! [`FAILURE_LINES`] lines, so the reason is in the test's own output instead of nowhere.
//!
//! Lines are kept per thread: records logged on the test's own thread. Work a test hands to
//! other threads is not captured (those threads have no capture of their own).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::SystemTime;

use crate::line::{Line, timestamp};

/// Lines printed when a capturing test fails.
pub const FAILURE_LINES: usize = 50;
/// Lines kept per capturing thread.
const KEEP: usize = 500;

thread_local! {
    static LINES: RefCell<Option<VecDeque<String>>> = const { RefCell::new(None) };
}

struct TestLogger;

static LOGGER: TestLogger = TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        LINES.with(|l| l.borrow().is_some())
    }

    fn log(&self, record: &log::Record<'_>) {
        let thread = std::thread::current();
        let message = record.args().to_string();
        let line = Line {
            ts: &timestamp(SystemTime::now()),
            level: record.level(),
            pid: None,
            thread: thread.name().unwrap_or("?"),
            target: record.target(),
            message: &message,
        };
        let text = line.text();
        LINES.with(|l| {
            if let Some(lines) = l.borrow_mut().as_mut() {
                if lines.len() == KEEP {
                    lines.pop_front();
                }
                lines.push_back(text);
            }
        });
    }

    fn flush(&self) {}
}

/// Keeps the current test's log lines until dropped; prints the last ones if the test panicked.
#[must_use = "the capture ends when this is dropped"]
pub struct Capture(());

/// Start capturing this thread's log records at every level (installs the test logger the first
/// time; when the process already has another logger, nothing is captured and nothing breaks).
pub fn capture() -> Capture {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
    LINES.with(|l| *l.borrow_mut() = Some(VecDeque::new()));
    Capture(())
}

/// The lines captured so far on this thread (for tests that check what was logged).
pub fn captured() -> Vec<String> {
    LINES.with(|l| l.borrow().as_ref().map(|v| v.iter().cloned().collect()).unwrap_or_default())
}

impl Drop for Capture {
    fn drop(&mut self) {
        let lines = LINES.with(|l| l.borrow_mut().take()).unwrap_or_default();
        if std::thread::panicking() {
            let skip = lines.len().saturating_sub(FAILURE_LINES);
            let mut out = format!("--- the last {} log lines of this test ---\n", lines.len() - skip);
            if lines.is_empty() {
                out.push_str("(the test logged nothing)\n");
            }
            for l in lines.iter().skip(skip) {
                out.push_str(l);
            }
            // eprint! keeps the lines in the test harness's captured output, but panics when
            // standard error cannot be written; a second panic escaping this destructor while the
            // test unwinds would abort the whole test run, so it is caught here
            let _ = std::panic::catch_unwind(|| eprint!("{out}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_keeps_this_threads_lines() {
        let _log = capture();
        log::warn!(target: "testing-test", "kept {}", 1);
        log::debug!(target: "testing-test", "kept too");
        let got = captured();
        assert!(got.iter().any(|l| l.ends_with("testing-test: kept 1\n")), "{got:?}");
        assert!(got.iter().any(|l| l.contains(" DEBUG ") && l.ends_with("kept too\n")));
    }

    #[test]
    fn a_failing_test_prints_its_lines_and_the_capture_ends() {
        let r = std::thread::spawn(|| {
            let _log = capture();
            log::error!(target: "testing-test", "the reason it failed");
            panic!("deliberate");
        })
        .join();
        assert!(r.is_err());
        // a thread without a capture logs into nothing
        assert!(captured().is_empty());
    }
}
