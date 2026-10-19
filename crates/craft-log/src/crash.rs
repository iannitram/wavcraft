//! Crash files (contract D): a panic leaves `<dir>/crash-<UTC stamp>.txt` holding the build line,
//! the panic message and place, a backtrace and the last log lines kept in memory; the panic's
//! output on standard error then ends with `crash: <path>` (contract G).
//!
//! [`install_hook`] chains onto the hook already installed, which runs first: an app's own hook
//! that logs the panic puts its line among the recent ones, and the default hook prints the
//! message, so the `crash:` line comes last. At most [`MAX_FILES_PER_RUN`] files are written per
//! process: a panic that a last-resort guard catches and turns into an error still leaves a file
//! (it is a fault worth its backtrace), but a loop of them cannot fill the folder.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::App;
use crate::collapse::{self, RING_LINES};
use crate::line::timestamp;

/// Crash files written per process at most.
pub const MAX_FILES_PER_RUN: usize = 5;

/// `crash-2026-10-13T17-47-32.123Z.txt`: the UTC time, with no `:` (Windows forbids it in names).
pub fn file_name(at: SystemTime) -> String {
    format!("crash-{}.txt", timestamp(at).replace(':', "-"))
}

/// Whether `name` is a crash file of [`file_name`]'s shape.
pub fn is_crash_file(name: &str) -> bool {
    name.starts_with("crash-") && name.ends_with(".txt")
}

/// The crash file's text: the build line first, then the panic (`message`, as the hook's
/// `PanicHookInfo` displays it, with its place) and the thread, the backtrace, the recent lines.
pub fn text(app: &App, message: &str, thread: &str, backtrace: &str, recent: &[String]) -> String {
    let mut s = format!("{}\n\npanic in thread \"{thread}\": {message}\n\nbacktrace:\n{}\n", app.build_line(), backtrace.trim_end());
    s.push_str(&format!("\nthe last {} log lines (oldest first):\n", recent.len()));
    for line in recent {
        s.push_str(line);
        s.push('\n');
    }
    s
}

/// Write a crash file into `dir` (created if missing) and return its path. Never replaces a file:
/// two panics in the same millisecond get `-2`, `-3`… before the extension.
pub fn write(dir: &Path, app: &App, message: &str, thread: &str, backtrace: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    // a secret argument's value never reaches a file (the recent lines are already without it)
    let body = text(app, &crate::cli::hide_secrets(message), thread, backtrace, &collapse::recent(RING_LINES));
    let name = file_name(SystemTime::now());
    let stem = name.strip_suffix(".txt").unwrap_or(&name);
    let mut last = None;
    for n in 1..=10 {
        let path = if n == 1 { dir.join(&name) } else { dir.join(format!("{stem}-{n}.txt")) };
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(body.as_bytes())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free crash file name")))
}

static WRITTEN: AtomicUsize = AtomicUsize::new(0);

/// Chain a hook onto the one installed now: it runs the previous hook, then writes a crash file
/// into `dir` with a forced backtrace, logs its path at error and prints `crash: <path>` last on
/// standard error. Call once per process, after the app's own hook.
pub fn install_hook(app: App, dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if WRITTEN.fetch_add(1, Ordering::Relaxed) >= MAX_FILES_PER_RUN {
            let _ = writeln!(std::io::stderr(), "crash: no file ({MAX_FILES_PER_RUN} already written by this run)");
            return;
        }
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let thread = std::thread::current();
        match write(&dir, &app, &info.to_string(), thread.name().unwrap_or("?"), &backtrace) {
            Ok(path) => {
                log::error!(target: app.name, "crash file: {}", path.display());
                log::logger().flush();
                let _ = writeln!(std::io::stderr(), "crash: {}", path.display());
            }
            Err(e) => {
                log::error!(target: app.name, "no crash file: {}: {e}", dir.display());
                let _ = writeln!(std::io::stderr(), "crash: no file ({}: {e})", dir.display());
            }
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: App = App { name: "testapp", version: "1.2.3", ours: "testapp", default_filter: "warn,testapp=info" };

    fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("craft-log-crash-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_name_is_a_utc_stamp_without_colons() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_791_446_357_728);
        assert_eq!(file_name(at), "crash-2026-10-08T07-59-17.728Z.txt");
        assert!(is_crash_file(&file_name(at)));
        assert!(!is_crash_file("effectcraft.log"));
    }

    #[test]
    fn the_text_starts_with_the_build_and_holds_the_panic_the_backtrace_and_the_recent_lines() {
        let recent = vec!["line one".to_owned(), "line two".to_owned()];
        let t = text(&APP, "panicked at src/x.rs:1:2:\nboom", "main", "0: a\n1: b\n", &recent);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], APP.build_line());
        assert!(t.contains("panic in thread \"main\": panicked at src/x.rs:1:2:\nboom\n"), "{t}");
        assert!(t.contains("backtrace:\n0: a\n1: b\n"), "{t}");
        assert!(t.ends_with("the last 2 log lines (oldest first):\nline one\nline two\n"), "{t}");
    }

    #[test]
    fn a_second_crash_in_the_same_instant_gets_its_own_file() {
        // the ring test pushes 205 lines in parallel: it would push ours out
        let _turn = collapse::RING_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = temp_dir("write");
        collapse::remember("crash-test: a line before the panic");
        let a = write(&dir, &APP, "first", "t", "bt").unwrap();
        let b = write(&dir, &APP, "second", "t", "bt").unwrap();
        assert_ne!(a, b);
        let ta = std::fs::read_to_string(&a).unwrap();
        assert!(ta.starts_with(&APP.build_line()) && ta.contains(": first\n") && ta.contains("crash-test: a line before the panic"), "{ta}");
        assert!(std::fs::read_to_string(&b).unwrap().contains(": second\n"));
        assert!(a.file_name().and_then(|n| n.to_str()).is_some_and(is_crash_file));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unusable_folder_is_an_error_not_a_panic() {
        let dir = temp_dir("blocked");
        std::fs::write(&dir, "a file where the folder should be").unwrap();
        assert!(write(&dir, &APP, "m", "t", "bt").is_err());
        let _ = std::fs::remove_file(&dir);
    }
}
