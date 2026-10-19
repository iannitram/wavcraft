//! craft-log: self-logging for the Craft apps. **Ours, never offered upstream** (its contract is
//! kept with our notes as `SELF-LOGGING.md`; every commit title starts `self-logging:`).
//!
//! The same crate is carried, unchanged, in FilmCraft, EffectCraft and PhotoCraft. An app supplies
//! an [`App`] (its name, version, its own crates' filter directive, its default filter) and calls,
//! in each program's `main`:
//!
//! - the window app: [`install_wrapped`] around the app's own logger, which keeps its file,
//!   folder, rotation and format exactly; this adds the build line, `CRAFT_LOG`, the repeat
//!   collapse, the ring of recent lines and the stall watch; then [`set_log_path`] once the app
//!   has opened its file;
//! - a command-line program: [`cli::install`], one shared `<log dir>/<app>-cli.log` appended to by
//!   every run, and [`cli::report_failure`] before exiting with an error, which prints
//!   `log: <path>` as the last line on standard error;
//! - a panic: [`crash::install_hook`] after the app's own hook, which writes
//!   `<dir>/crash-<UTC stamp>.txt` (the build line, the panic, a backtrace, the last 200 log
//!   lines) and ends the panic's output with `crash: <path>`;
//! - a test: `let _log = craft_log::testing::capture();`, which prints the test's last log lines
//!   when it fails.
//!
//! Switches: `CRAFT_LOG=debug|trace|info|warn|error` sets our own crates to that level and every
//! other crate to warn (a bare `RUST_LOG=debug` turns on every third-party crate); `RUST_LOG` is
//! still honoured when `CRAFT_LOG` is not set; `CRAFT_LOG_JSON=1` writes one JSON object per line
//! in the command-line log; `CRAFT_LOG_STDERR=1` copies command-line log lines to standard error.
//!
//! Nothing here sends anything anywhere: every line goes to a local file or standard error.

pub mod cli;
pub mod collapse;
pub mod crash;
pub mod filter;
pub mod line;
pub mod report;
pub mod stall;
pub mod testing;
pub mod wrap;

#[cfg(test)]
mod build_tree;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

pub use collapse::recent;

/// The commit the program was built from (first 8 hex digits), or "unknown".
pub const BUILD_COMMIT: &str = env!("CRAFT_LOG_COMMIT");
/// `+modified?` when the build looks made from uncommitted work (a source file newer than git's
/// index when it was built; an approximation, hence the question mark), else empty.
pub const BUILD_TREE: &str = env!("CRAFT_LOG_TREE");
/// The date it was built (UTC, `YYYY-MM-DD`).
pub const BUILD_DATE: &str = env!("CRAFT_LOG_BUILD_DATE");
/// `release` or `debug`.
pub const BUILD_PROFILE: &str = if cfg!(debug_assertions) { "debug" } else { "release" };

/// What craft-log needs to know about the app.
#[derive(Clone, Copy, Debug)]
pub struct App {
    /// Lower-case program family name: `filmcraft`. Names the log files and the build line.
    pub name: &'static str,
    /// The app's version (`env!("CARGO_PKG_VERSION")` of the program).
    pub version: &'static str,
    /// The filter directive naming the app's own crates, in the syntax its filter understands:
    /// `filmcraft*`, `effectcraft`, `photocraft`.
    pub ours: &'static str,
    /// The filter when neither `CRAFT_LOG` nor `RUST_LOG` is set (the app's own default).
    pub default_filter: &'static str,
}

impl App {
    /// The first line of every log, crash file and report: which build wrote it.
    /// `filmcraft 0.7.0 (commit a8657630, built 2026-10-13, release, linux x86_64)`, with
    /// `a8657630+modified?` for a build from uncommitted work ([`BUILD_TREE`]).
    pub fn build_line(&self) -> String {
        format!(
            "{} {} (commit {BUILD_COMMIT}{BUILD_TREE}, built {BUILD_DATE}, {BUILD_PROFILE}, {} {})",
            self.name,
            self.version,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }

    /// The filter directive string in force and where it came from (see [`filter::spec`]).
    pub fn filter_spec(&self) -> (String, filter::Source) {
        filter::spec_from_env(self.ours, self.default_filter)
    }
}

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Record where this process's log file is, for the lines that point at it (the stall warning,
/// a failing command's last line, the diagnostic report).
pub fn set_log_path(path: Option<PathBuf>) {
    *LOG_PATH.lock().unwrap_or_else(PoisonError::into_inner) = path;
}

/// This process's log file, when one was recorded.
pub fn log_path() -> Option<PathBuf> {
    LOG_PATH.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

static WRAPPED: OnceLock<wrap::Wrapped> = OnceLock::new();

/// Install `inner` (the app's own logger, built but not installed) wrapped by
/// [`wrap::Wrapped`], with `max` as the global level ceiling, and log the build line as the first
/// record. `false` when another logger was installed first (nothing changes then).
pub fn install_wrapped(app: &App, inner: &'static dyn log::Log, max: log::LevelFilter) -> bool {
    let wrapped = WRAPPED.get_or_init(|| wrap::Wrapped::new(inner));
    if log::set_logger(wrapped).is_err() {
        return false;
    }
    log::set_max_level(max);
    let (spec, source) = app.filter_spec();
    log::info!(target: app.name, "{}", app.build_line());
    log::info!(target: app.name, "log filter \"{spec}\" from {}", source.name());
    stall::start(app.name);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_line_names_the_build() {
        let app = App { name: "testapp", version: "1.2.3", ours: "testapp", default_filter: "warn,testapp=info" };
        let line = app.build_line();
        assert!(line.starts_with(&format!("testapp 1.2.3 (commit {BUILD_COMMIT}{BUILD_TREE}, ")), "{line}");
        assert!(BUILD_TREE.is_empty() || BUILD_TREE == "+modified?", "{BUILD_TREE}");
        assert!(line.contains(&format!("built {BUILD_DATE}, {BUILD_PROFILE}, ")), "{line}");
        assert!(BUILD_COMMIT == "unknown" || (BUILD_COMMIT.len() == 8 && BUILD_COMMIT.bytes().all(|b| b.is_ascii_hexdigit())));
        assert_eq!(BUILD_DATE.len(), 10);
    }
}
