//! The app's own log (`SELF-LOGGING.md`, contract A and D): every run writes
//! `<config folder>/logs/wavcraft.log`, a new file each start with the two before it kept as
//! `wavcraft.1.log` and `wavcraft.2.log`, capped at 16 MiB. craft-log wraps it: the build line
//! first, the `CRAFT_LOG` filter (`CRAFT_LOG=debug` is our own crates at debug, everything else at
//! warn), repeated lines collapsed, the last 200 lines kept for a crash file, and a panic writes
//! `crash-<UTC stamp>.txt` beside the log.
//!
//! Until this, the app had no logger: its `log::warn!` lines went nowhere.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use craft_log::filter::Filter;
use craft_log::line::{Line, timestamp};

pub const APP: craft_log::App =
    craft_log::App { name: "wavcraft", version: env!("CARGO_PKG_VERSION"), ours: "wavcraft", default_filter: "warn,wavcraft=info" };

pub const LOG_FILE: &str = "wavcraft.log";
/// Earlier runs' logs kept beside the current one.
const KEEP: usize = 2;
/// A log stops growing here (a last line says so).
const CAP: u64 = 16 << 20;

/// The folder the logs and crash files go in: `logs` in WavCraft's config folder.
pub fn log_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wavcraft").join("logs"))
}

/// Install the logger and the crash hook. Returns the log file's path; `None` when the folder
/// can't be made (the app then runs without a log, as before, and says so on standard error).
pub fn install() -> Option<PathBuf> {
    let dir = log_dir()?;
    let path = match open_fresh(&dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wavcraft: no log file in {}: {e}", dir.display());
            return None;
        }
    };
    let (spec, _) = APP.filter_spec();
    let filter = Filter::parse(&spec);
    let max = filter.max();
    let file = match File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("wavcraft: no log file at {}: {e}", path.display());
            return None;
        }
    };
    static LOGGER: OnceLock<FileLog> = OnceLock::new();
    let inner = LOGGER.get_or_init(|| FileLog { filter, out: Mutex::new(Out { file: BufWriter::new(file), len: 0, capped: false }) });
    if !craft_log::install_wrapped(&APP, inner, max) {
        return None;
    }
    craft_log::set_log_path(Some(path.clone()));
    craft_log::crash::install_hook(APP, dir);
    log::info!(target: "wavcraft", "log file {}", path.display());
    Some(path)
}

/// Rotate `wavcraft.log` → `wavcraft.1.log` → `wavcraft.2.log` (the oldest goes) and return the
/// path for this run's log.
pub fn open_fresh(dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let name = |i: usize| if i == 0 { dir.join(LOG_FILE) } else { dir.join(format!("wavcraft.{i}.log")) };
    for i in (0..KEEP).rev() {
        let from = name(i);
        if from.exists() {
            fs::rename(&from, name(i + 1))?;
        }
    }
    Ok(name(0))
}

struct Out {
    file: BufWriter<File>,
    len: u64,
    capped: bool,
}

struct FileLog {
    filter: Filter,
    out: Mutex<Out>,
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= self.filter.level_for(m.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let thread = std::thread::current();
        let message = record.args().to_string();
        let ts = timestamp(SystemTime::now());
        let line =
            Line { ts: &ts, level: record.level(), pid: None, thread: thread.name().unwrap_or("?"), target: record.target(), message: &message }
                .text();
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        if out.capped {
            return;
        }
        if out.len + line.len() as u64 > CAP {
            out.capped = true;
            let _ = writeln!(out.file, "{ts} WARN [{}] wavcraft: log reached 16 MiB; nothing more is written this run", thread.name().unwrap_or("?"));
            let _ = out.file.flush();
            return;
        }
        out.len += line.len() as u64;
        // `Line::text` ends with its own newline.
        let _ = write!(out.file, "{line}");
        // Warnings and errors reach the disk at once, so a crash right after still has them.
        if record.level() <= log::Level::Warn {
            let _ = out.file.flush();
        }
    }

    fn flush(&self) {
        let _ = self.out.lock().unwrap_or_else(PoisonError::into_inner).file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_log_each_start_keeps_the_two_before_it() {
        let dir = std::env::temp_dir().join(format!("wavcraft-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for run in 1..=4 {
            let path = open_fresh(&dir);
            assert!(path.is_ok(), "run {run}: {path:?}");
            if let Ok(path) = path {
                assert!(fs::write(&path, format!("run {run}")).is_ok());
            }
        }
        let read = |n: &str| fs::read_to_string(dir.join(n)).unwrap_or_default();
        assert_eq!((read("wavcraft.log"), read("wavcraft.1.log"), read("wavcraft.2.log")), ("run 4".into(), "run 3".into(), "run 2".into()));
        assert!(!dir.join("wavcraft.3.log").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn our_crates_log_info_and_others_warn_by_default() {
        let f = Filter::parse(APP.default_filter);
        assert_eq!(f.level_for("wavcraft_core::store"), log::LevelFilter::Info);
        assert_eq!(f.level_for("wavcraft"), log::LevelFilter::Info);
        assert_eq!(f.level_for("wgpu_core::device"), log::LevelFilter::Warn);
    }
}
