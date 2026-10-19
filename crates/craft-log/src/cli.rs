//! The command-line programs' logger: one shared file per app, `<dir>/<app>-cli.log`, appended to
//! by every run (one check starts hundreds of short runs), with the process id in every line; a
//! 16 MiB cap with one earlier file kept. Standard error stays as quiet as it was unless
//! `CRAFT_LOG_STDERR=1`, so a command's output does not change.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use log::LevelFilter;

use crate::App;
use crate::collapse::{self, Collapser};
use crate::filter::{self, Filter};
use crate::line::{Line, timestamp};

/// The command-line log's cap: past it a run writes only error lines; at the next run it becomes
/// `<app>-cli.1.log`.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// A run starts a new file when the log has less room left than this, so the `log:` line of a
/// failing run points at a file that holds its lines.
pub const RUN_ROOM_BYTES: u64 = 64 * 1024;
/// Past the cap, error lines are still written, up to this much more (an error flood cannot fill
/// the disk).
pub const ERROR_ROOM_BYTES: u64 = 1024 * 1024;

/// Whether a log of `len` bytes has too little room for another run.
pub fn too_full(len: u64) -> bool {
    len > MAX_FILE_BYTES - RUN_ROOM_BYTES
}

/// `<app>-cli.log`
pub fn file_name(app: &str) -> String {
    format!("{app}-cli.log")
}

/// Open `<dir>/<app>-cli.log` for appending, first moving one without room for a run
/// ([`too_full`]) to `<app>-cli.1.log` (replacing the older one). Returns the path, the file and
/// the bytes already in it.
pub fn open(dir: &Path, app: &str) -> std::io::Result<(PathBuf, File, u64)> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(app));
    if std::fs::metadata(&path).is_ok_and(|m| too_full(m.len())) {
        roll_over(dir, app, &path);
    }
    let file = open_append(&path)?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok((path, file, len))
}

/// A roll-over lock older than this was left by a run that died holding it.
pub const LOCK_STALE: Duration = Duration::from_secs(10);
/// How long a run waits for another run's roll-over before appending without one.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Move a log without room for a run to `<app>-cli.1.log`, safely when many runs start at once
/// (one check starts hundreds): under `<app>-cli.lock`, and only if the log is still too full once
/// the lock is held, so a run that saw the full log before another run moved it does not then
/// move the fresh one over it. Without the lock (another run holds it past [`LOCK_WAIT`]) nothing
/// moves: the run appends to the full log, where its error lines still land.
fn roll_over(dir: &Path, app: &str, path: &Path) {
    let Some(_lock) = Lock::take(&dir.join(format!("{app}-cli.lock"))) else { return };
    if std::fs::metadata(path).is_ok_and(|m| m.is_file() && too_full(m.len())) {
        let _ = std::fs::rename(path, dir.join(format!("{app}-cli.1.log")));
    }
}

/// A lock file made with `create_new` (atomic everywhere, network folders included), removed when
/// dropped. One older than [`LOCK_STALE`] is taken over.
struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> Option<Lock> {
        let start = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Some(Lock(path.to_path_buf())),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if stale(path) {
                        // moved aside under a name of our own first: of two runs that both found it
                        // stale, only one moves it, and neither removes a lock just taken anew
                        let aside = path.with_extension(format!("stale-{}", std::process::id()));
                        if std::fs::rename(path, &aside).is_ok() {
                            let _ = std::fs::remove_file(&aside);
                        }
                        continue;
                    }
                    if start.elapsed() > LOCK_WAIT {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return None,
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn stale(path: &Path) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age > LOCK_STALE))
}

/// Open `path` for appending without ever waiting on it. Opening a FIFO for writing waits for a
/// reader, so a FIFO in place of the log would hang every command: an existing file is opened only
/// when it is a regular file (a FIFO, a device or a folder is refused), and an absent one is made
/// with `create_new`, which never opens something put there in the meantime. On Linux the open
/// also carries `O_NONBLOCK`, which closes the gap between the check and the open (a FIFO swapped
/// in then fails at once instead of waiting); for a regular file it changes nothing.
pub fn open_append(path: &Path) -> std::io::Result<File> {
    use std::io::{Error, ErrorKind};
    for _ in 0..3 {
        let file = match std::fs::metadata(path) {
            Ok(m) if m.is_file() => append_options(false).open(path)?,
            Ok(_) => return Err(Error::other(format!("{} is not a regular file", path.display()))),
            Err(e) if e.kind() == ErrorKind::NotFound => match append_options(true).open(path) {
                Ok(f) => f,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue, // made by another run just now
                Err(e) => return Err(e),
            },
            Err(e) => return Err(e),
        };
        // what was opened, in case it was swapped between the check and the open
        return if file.metadata()?.is_file() { Ok(file) } else { Err(Error::other(format!("{} is not a regular file", path.display()))) };
    }
    Err(Error::other(format!("{} kept appearing and vanishing", path.display())))
}

/// Append (and create only when `create_new`), never blocking where the platform allows.
pub(crate) fn append_options(create_new: bool) -> OpenOptions {
    let mut o = OpenOptions::new();
    o.append(true).create_new(create_new);
    nonblocking(&mut o);
    o
}

/// `O_NONBLOCK` (0o4000 in Linux's generic flag values, which these architectures use; there is no
/// `libc` here to name it): a FIFO without a reader then fails to open instead of waiting.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64")))]
pub(crate) fn nonblocking(o: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    o.custom_flags(0o4000);
}

/// Elsewhere only the regular-file check guards the open.
#[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64"))))]
pub(crate) fn nonblocking(_: &mut OpenOptions) {}

struct Out {
    file: Option<File>,
    written: u64,
    /// The cap was reached and said so: only error lines from here.
    capped: bool,
    /// The file did not end in a newline (a run cut off mid-line): the first write starts with one.
    newline_first: bool,
}

/// Whether the file at `path` is not empty and does not end in a newline.
fn ends_mid_line(path: &Path) -> bool {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut options = OpenOptions::new();
    options.read(true);
    nonblocking(&mut options);
    let Ok(mut f) = options.open(path) else { return false };
    if !f.metadata().is_ok_and(|m| m.is_file() && m.len() > 0) || f.seek(SeekFrom::End(-1)).is_err() {
        return false;
    }
    let mut last = [0u8; 1];
    f.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

/// The installed command-line logger.
pub struct CliLogger {
    filter: Filter,
    json: bool,
    stderr: bool,
    /// Values of secret arguments, longest first: shown as [`HIDDEN`] in every line.
    secrets: Vec<String>,
    path: Option<PathBuf>,
    out: Mutex<Out>,
    collapse: Mutex<Collapser>,
}

impl CliLogger {
    /// A logger writing to `file` (at `path`, already holding `written` bytes).
    pub fn new(filter: Filter, json: bool, stderr: bool, file: Option<(PathBuf, File, u64)>) -> CliLogger {
        let (path, file, written) = match file {
            Some((p, f, w)) => (Some(p), Some(f), w),
            None => (None, None, 0),
        };
        let newline_first = path.as_deref().is_some_and(ends_mid_line);
        let out = Out { file, written, capped: false, newline_first };
        CliLogger { filter, json, stderr, secrets: Vec::new(), path, out: Mutex::new(out), collapse: Mutex::new(Collapser::default()) }
    }

    /// The same logger, showing each of `values` (secret arguments' values) as [`HIDDEN`]
    /// wherever it appears in a line, an error's text included.
    #[must_use]
    pub fn with_secrets(mut self, values: Vec<String>) -> CliLogger {
        self.secrets = values.into_iter().filter(|v| !v.is_empty()).collect();
        self.secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
        self.secrets.dedup();
        self
    }

    /// `text` with every secret value shown as [`HIDDEN`].
    pub fn hide_secrets<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        hide(text, &self.secrets)
    }

    /// The log file, when there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn write(&self, level: log::Level, target: &str, message: &str) {
        let message = &*self.hide_secrets(message);
        let thread = std::thread::current();
        let line = Line { ts: &timestamp(SystemTime::now()), level, pid: Some(std::process::id()), thread: thread.name().unwrap_or("?"), target, message };
        let text = line.text();
        collapse::remember(&text);
        let formatted = if self.json { line.json() } else { text };
        if self.stderr {
            let _ = std::io::stderr().write_all(formatted.as_bytes());
        }
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let formatted = if std::mem::take(&mut out.newline_first) { format!("\n{formatted}") } else { formatted };
        let len = u64::try_from(formatted.len()).unwrap_or(u64::MAX);
        let end = out.written.saturating_add(len);
        let Out { file, written, capped, .. } = &mut *out;
        let Some(f) = file.as_mut() else { return };
        if end > MAX_FILE_BYTES {
            // capped: say so once, then write only errors (the reason a command failed must be in
            // the file its `log:` line names), up to ERROR_ROOM_BYTES more; never fail the command
            if !*capped {
                *capped = true;
                let note = format!("log file reached {MAX_FILE_BYTES} bytes; only errors are written from here, and the next run starts a new one\n");
                if f.write_all(note.as_bytes()).is_ok() {
                    *written = written.saturating_add(u64::try_from(note.len()).unwrap_or(u64::MAX));
                }
            }
            if level != log::Level::Error || written.saturating_add(len) > MAX_FILE_BYTES + ERROR_ROOM_BYTES {
                return;
            }
        }
        // One write per line, so lines from concurrent runs appending to the same file stay whole.
        if f.write_all(formatted.as_bytes()).is_ok() {
            *written = written.saturating_add(len);
        } else {
            // a full disk or a vanished file: stop writing, never fail the command
            *file = None;
        }
    }

    fn note(&self, (times, level, target, so_far): (u64, log::Level, String, bool)) {
        self.write(level, &target, &collapse::note_text(times, so_far));
    }
}

impl log::Log for CliLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let action = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).observe(record.level(), record.target(), &message);
        if let Some(note) = action.note {
            self.note(note);
        }
        if !action.swallow {
            self.write(record.level(), record.target(), &message);
        }
    }

    fn flush(&self) {
        let pending = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).pending();
        if let Some(note) = pending {
            self.note(note);
        }
        if let Some(f) = self.out.lock().unwrap_or_else(PoisonError::into_inner).file.as_mut() {
            let _ = f.flush();
        }
    }
}

static CLI: OnceLock<CliLogger> = OnceLock::new();

/// What a secret argument's value shows as.
pub const HIDDEN: &str = "***";

fn hide<'a>(text: &'a str, secrets: &[String]) -> std::borrow::Cow<'a, str> {
    let mut out = std::borrow::Cow::Borrowed(text);
    for s in secrets {
        if out.contains(s.as_str()) {
            out = std::borrow::Cow::Owned(out.replace(s.as_str(), HIDDEN));
        }
    }
    out
}

/// `text` with the installed command-line logger's secret values hidden (for text that leaves
/// the process another way, a crash file's panic message). Unchanged without one.
pub fn hide_secrets(text: &str) -> String {
    installed().map_or_else(|| text.to_owned(), |l| l.hide_secrets(text).into_owned())
}

/// Install the command-line logger for `app`, writing to `<log_dir>/<app>-cli.log` (no file when
/// `log_dir` is `None` or unusable). Logs the build line and the command line first.
///
/// `secret_args` names the arguments whose values are never logged (`&["--control-token"]`):
/// `--control-token <value>` and `--control-token=<value>` show the value as `***`, and so does
/// every later line it appears in, in the file (text or JSON), in the copy on standard error, and in
/// a failing run's error. Returns `None` when another logger was installed first.
pub fn install(app: &App, log_dir: Option<&Path>, secret_args: &[&str]) -> Option<&'static CliLogger> {
    install_checked(app, log_dir, secret_args, &|_| false)
}

/// [`install`], with the app's own test of an argument that is a secret wherever it stands
/// (PhotoCraft's control token: exactly 64 hex digits). Beyond `secret_args` and `looks_secret`,
/// every run also hides a password in a URL's user part (`https://sam:***@host/`) and any
/// argument or later text equal to the value of a secret environment switch ([`env_secret_values`]).
pub fn install_checked(app: &App, log_dir: Option<&Path>, secret_args: &[&str], looks_secret: &dyn Fn(&str) -> bool) -> Option<&'static CliLogger> {
    let (spec, source) = filter::spec_from_env(app.ours, app.default_filter);
    let filter = Filter::parse(&spec);
    let max = filter.max().max(LevelFilter::Info);
    let stderr = matches!(std::env::var("CRAFT_LOG_STDERR").ok().as_deref().map(str::trim), Some("1" | "true" | "yes"));
    let file = log_dir.and_then(|d| open(d, app.name).ok());
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let vars: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.to_string_lossy().into_owned()))).collect();
    let (shown, secrets) = mask_args_checked(&args, secret_args, looks_secret, &env_secret_values(&vars));
    let logger = CLI.get_or_init(|| CliLogger::new(filter, filter::json_from_env(), stderr, file).with_secrets(secrets));
    log::set_logger(logger).ok()?;
    log::set_max_level(max);
    logger.write(log::Level::Info, app.name, &app.build_line());
    let command_line = crate::report::mask_home(&shown.join(" "));
    logger.write(log::Level::Info, app.name, &format!("command line: {command_line}; log filter \"{spec}\" from {}", source.name()));
    crate::set_log_path(logger.path().map(Path::to_path_buf));
    crate::stall::start(app.name);
    Some(logger)
}

/// The installed command-line logger, if any.
pub fn installed() -> Option<&'static CliLogger> {
    CLI.get()
}

/// `args` as logged, and the secret values found: the value after an argument named in
/// `secret_args` (`--token <value>`), or after its `=` (`--token=<value>`), shows as [`HIDDEN`].
/// The caller reads the arguments with `args_os` and converts them lossily (`args` panics on an
/// argument that is not Unicode, a Latin-1 file name on Linux, turning a usage error into a
/// crash); the program's own handling of its arguments is not touched.
pub fn mask_args(args: &[String], secret_args: &[&str]) -> (Vec<String>, Vec<String>) {
    mask_args_checked(args, secret_args, &|_| false, &[])
}

/// [`mask_args`], also hiding: an argument (or a `--flag=` value) that `looks_secret` says is
/// one, wherever it stands; a URL's password (`scheme://user:password@`, or its whole user part
/// when it has no password, `https://<token>@host`); and every one of `env_secrets` (the values of
/// secret environment switches) wherever it appears. The secrets found come back with `env_secrets`.
pub fn mask_args_checked(args: &[String], secret_args: &[&str], looks_secret: &dyn Fn(&str) -> bool, env_secrets: &[String]) -> (Vec<String>, Vec<String>) {
    let mut shown = Vec::with_capacity(args.len());
    let mut secrets: Vec<String> = env_secrets.to_vec();
    let mut hide_next = false;
    for a in args {
        // the value after a secret flag is hidden, unless it is itself a secret flag
        // (`--token --token s3cr3t`, `--token --token=s3cr3t`): then it is read as one, so the
        // value after it is hidden too
        let is_secret_flag = secret_args.contains(&a.as_str()) || a.split_once('=').is_some_and(|(f, _)| secret_args.contains(&f));
        if std::mem::take(&mut hide_next) && !is_secret_flag {
            secrets.push(a.clone());
            shown.push(HIDDEN.to_owned());
            continue;
        }
        if secret_args.contains(&a.as_str()) {
            hide_next = true;
            shown.push(a.clone());
        } else if let Some((flag, value)) = a.split_once('=').filter(|(f, v)| secret_args.contains(f) || (f.starts_with('-') && looks_secret(v))) {
            secrets.push(value.to_owned());
            shown.push(format!("{flag}={HIDDEN}"));
        } else if looks_secret(a) {
            secrets.push(a.clone());
            shown.push(HIDDEN.to_owned());
        } else {
            secrets.extend(url_secret(a));
            shown.push(a.clone());
        }
    }
    secrets.retain(|s| !s.is_empty());
    secrets.dedup();
    (shown, secrets)
}

/// The secret part of the first URL in `arg` with a user part: its password
/// (`https://sam:hunter2@host` gives `hunter2`), or the whole user part when it has none
/// (`https://ghp_abc@host`, a token used as the user name).
fn url_secret(arg: &str) -> Option<String> {
    let rest = &arg[arg.find("://")? + 3..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let user = &authority[..authority.rfind('@')?];
    let secret = user.split_once(':').map_or(user, |(_, password)| password);
    (!secret.is_empty()).then(|| secret.to_owned())
}

/// The shortest environment value [`env_secret_values`] hides: a shorter one would rewrite
/// ordinary words in every line.
pub const MIN_ENV_SECRET: usize = 8;

/// The values of the environment's secret switches (named by [`crate::report::is_secret_name`],
/// whatever their prefix), so that one also passed as an argument, or echoed in an error, is hidden
/// in the log. Left alone: values shorter than [`MIN_ENV_SECRET`], and absolute paths
/// (`SSH_AUTH_SOCK=/run/…` names a socket, not a secret).
pub fn env_secret_values(vars: &[(String, String)]) -> Vec<String> {
    vars.iter()
        .filter(|(k, v)| crate::report::is_secret_name(k) && v.chars().count() >= MIN_ENV_SECRET && !Path::new(v).is_absolute())
        .map(|(_, v)| v.clone())
        .collect()
}

/// A command is about to exit with an error: log `reason` at error (so the log holds why), then
/// [`report_failure`].
pub fn failed(app: &App, reason: &str) {
    if installed().is_some() {
        log::error!(target: app.name, "{reason}");
    }
    report_failure();
}

/// For a command that is about to exit with an error: flush, and print `log: <path>` as the last
/// line on standard error, so whoever reads the failure knows where the reasons are. Prints
/// nothing when no log file is open.
pub fn report_failure() {
    let Some(logger) = installed() else { return };
    log::Log::flush(logger);
    if let Some(p) = logger.path() {
        // not eprintln!, which panics (exit 101) when standard error is full or closed
        let _ = writeln!(std::io::stderr(), "log: {}", p.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("craft-log-cli-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn logger_in(dir: &Path, json: bool) -> CliLogger {
        CliLogger::new(Filter::parse("warn,app=debug"), json, false, open(dir, "app").ok())
    }

    fn rec(l: &CliLogger, level: log::Level, target: &str, msg: &str) {
        l.log(&log::Record::builder().level(level).target(target).args(format_args!("{msg}")).build());
    }

    #[test]
    fn every_run_appends_to_one_file_with_its_process_id() {
        let dir = temp_dir("append");
        for run in 0..2 {
            let l = logger_in(&dir, false);
            rec(&l, log::Level::Warn, "app::x", &format!("run {run}"));
            rec(&l, log::Level::Debug, "other", "third-party debug is filtered");
            l.flush();
        }
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let pid = std::process::id();
        assert!(lines[0].contains(&format!(" WARN  [{pid} ")) && lines[0].ends_with("app::x: run 0"), "{}", lines[0]);
        assert!(lines[1].ends_with("run 1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_file_moves_aside_at_the_next_run() {
        let dir = temp_dir("rotate");
        std::fs::create_dir_all(&dir).unwrap();
        let full = dir.join("app-cli.log");
        std::fs::File::create(&full).unwrap().set_len(MAX_FILE_BYTES).unwrap();
        let (path, _, len) = open(&dir, "app").unwrap();
        assert_eq!((path, len), (full, 0));
        assert_eq!(std::fs::metadata(dir.join("app-cli.1.log")).unwrap().len(), MAX_FILE_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_without_room_for_a_run_moves_aside_at_open() {
        let dir = temp_dir("room");
        std::fs::create_dir_all(&dir).unwrap();
        let nearly = dir.join("app-cli.log");
        std::fs::File::create(&nearly).unwrap().set_len(MAX_FILE_BYTES - 1000).unwrap();
        let (path, _, len) = open(&dir, "app").unwrap();
        assert_eq!((path, len), (nearly, 0), "a run with 1000 bytes of room starts a new file");
        assert_eq!(std::fs::metadata(dir.join("app-cli.1.log")).unwrap().len(), MAX_FILE_BYTES - 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_error_line_is_written_past_the_cap() {
        let dir = temp_dir("cap");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-cli.log");
        let file = open_append(&path).unwrap();
        // as if the run had already written up to 10 bytes short of the cap
        let l = CliLogger::new(Filter::parse("warn,app=debug"), false, false, Some((path.clone(), file, MAX_FILE_BYTES - 10)));
        rec(&l, log::Level::Warn, "app", "a warning that does not fit");
        rec(&l, log::Level::Info, "app", "neither does this");
        rec(&l, log::Level::Error, "app", "the reason the command failed");
        l.flush();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("the reason the command failed"), "the log: line must point at a file holding the error: {text}");
        assert!(!text.contains("a warning that does not fit") && !text.contains("neither does this"), "{text}");
        assert!(text.contains(&format!("log file reached {MAX_FILE_BYTES} bytes")), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_roll_over_lock_is_taken_over_and_a_live_one_waited_for() {
        let dir = temp_dir("lock");
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("app-cli.lock");
        // a run died holding it a minute ago
        let f = std::fs::File::create(&lock).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(60)).unwrap();
        drop(f);
        let held = Lock::take(&lock).expect("a stale lock is taken over");
        assert!(lock.is_file());
        // held now: another taker waits LOCK_WAIT, then gives up
        let t0 = Instant::now();
        assert!(Lock::take(&lock).is_none());
        assert!(t0.elapsed() >= LOCK_WAIT);
        drop(held);
        assert!(!lock.exists(), "released");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_starts_on_a_new_line_after_a_cut_off_one() {
        let dir = temp_dir("newline");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app-cli.log"), "a line cut off by a run that died").unwrap();
        let l = logger_in(&dir, false);
        rec(&l, log::Level::Warn, "app", "first");
        rec(&l, log::Level::Warn, "app", "second");
        l.flush();
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text:?}");
        assert_eq!(lines[0], "a line cut off by a run that died");
        assert!(lines[1].ends_with("app: first") && lines[2].ends_with("app: second"), "{text:?}");
        // a file ending in a newline gets none extra
        let l = logger_in(&dir, false);
        rec(&l, log::Level::Warn, "app", "third");
        l.flush();
        assert_eq!(std::fs::read_to_string(dir.join("app-cli.log")).unwrap().lines().count(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_lines_and_collapsed_repeats() {
        let dir = temp_dir("json");
        let l = logger_in(&dir, true);
        for _ in 0..4 {
            rec(&l, log::Level::Warn, "app", "same");
        }
        rec(&l, log::Level::Info, "app", "next");
        l.flush();
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let msgs: Vec<_> = text.lines().map(|l| l.rsplit_once("\"msg\":").unwrap().1).collect();
        assert_eq!(msgs, ["\"same\"}", "\"previous line repeated 3 times\"}", "\"next\"}"]);
        assert!(text.lines().all(|l| l.starts_with("{\"ts\":\"") && l.contains(&format!("\"pid\":{}", std::process::id()))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fifo_swapped_in_after_the_check_fails_to_open_instead_of_waiting() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let dir = temp_dir("fifo-race");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("app-cli.log");
        if !std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
            return;
        }
        // the open itself, as if the regular-file check had passed just before the swap
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(append_options(false).open(&fifo).is_err()));
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(true));
        assert!(open(&dir, "app").is_err(), "a FIFO is never the log");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn secret_arguments_show_as_stars_wherever_they_appear() {
        let args: Vec<String> = ["prog", "--token", "s3cr3t", "--token=0th3r", "--name", "s3cr3t-ish", "--token"].iter().map(|s| (*s).to_owned()).collect();
        let (shown, secrets) = mask_args(&args, &["--token"]);
        assert_eq!(shown.join(" "), "prog --token *** --token=*** --name s3cr3t-ish --token");
        assert_eq!(secrets, ["s3cr3t", "0th3r"]);
        assert_eq!(mask_args(&args, &[]).0, args, "nothing is secret unless named");
        let dir = temp_dir("secret");
        let l = CliLogger::new(Filter::parse("info"), true, false, open(&dir, "app").ok()).with_secrets(secrets);
        rec(&l, log::Level::Error, "app", "login failed for s3cr3t (0th3r)");
        l.flush();
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        assert!(text.contains("\"msg\":\"login failed for *** (***)\"") && !text.contains("s3cr3t") && !text.contains("0th3r"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_unicode_argument_is_shown_lossily() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let arg = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'b']);
            let args = vec!["prog".to_owned(), arg.to_string_lossy().into_owned()];
            assert_eq!(mask_args(&args, &["--token"]).0.join(" "), "prog a\u{fffd}b");
        }
    }

    #[test]
    fn url_passwords_env_secrets_and_the_apps_own_test_are_hidden() {
        let args: Vec<String> = [
            "prog",
            "import",
            "https://sam:hunter22pw@example.com/a.mp4",
            "https://ghp_tokenuser1@example.com/b",
            "tok-envsecret-123",
            "--key=0123abcd",
            "0123abcd",
            "https://example.com/plain",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let hex = |a: &str| a.len() == 8 && a.bytes().all(|b| b.is_ascii_hexdigit());
        let (shown, secrets) = mask_args_checked(&args, &[], &hex, &["tok-envsecret-123".to_owned()]);
        assert_eq!(shown[5..], ["--key=***", "***", "https://example.com/plain"]);
        for s in ["hunter22pw", "ghp_tokenuser1", "tok-envsecret-123", "0123abcd"] {
            assert!(secrets.iter().any(|x| x == s), "{s} in {secrets:?}");
        }
        let dir = temp_dir("url-env");
        let l = CliLogger::new(Filter::parse("info"), false, false, open(&dir, "app").ok()).with_secrets(secrets);
        rec(&l, log::Level::Info, "app", &format!("command line: {}", shown.join(" ")));
        rec(&l, log::Level::Error, "app", "import: https://sam:hunter22pw@example.com/a.mp4 and tok-envsecret-123: not found");
        l.flush();
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        assert!(["hunter22pw", "ghp_tokenuser1", "tok-envsecret-123", "0123abcd"].iter().all(|s| !text.contains(s)), "{text}");
        assert!(text.contains("https://sam:***@example.com/a.mp4") && text.contains("https://***@example.com/b"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_long_non_path_values_of_secret_switches_are_hidden() {
        // an absolute path as this system writes one (Windows: a drive letter)
        let sock = if cfg!(windows) { r"C:\Users\sam\ssh-agent" } else { "/run/user/1000/ssh" };
        let vars: Vec<(String, String)> =
            [("APP_TOKEN", "tok-envsecret-123"), ("SSH_AUTH_SOCK", sock), ("APP_PASS", "short"), ("APP_DATA_DIR", "not-a-secret-at-all")]
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect();
        assert_eq!(env_secret_values(&vars), ["tok-envsecret-123"]);
    }

    #[test]
    fn a_secret_flag_where_its_value_should_be_does_not_hide_the_next_secret() {
        let args: Vec<String> = ["prog", "--token", "--token", "s3cr3t", "--token", "--token=0th3r"].iter().map(|s| (*s).to_owned()).collect();
        let (shown, secrets) = mask_args(&args, &["--token"]);
        assert_eq!(shown.join(" "), "prog --token --token *** --token --token=***");
        assert_eq!(secrets, ["s3cr3t", "0th3r"]);
        // the app's own test sees every argument, two tokens side by side included
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let args: Vec<String> = ["prog", "serve", "--token", &a, &b].iter().map(|s| (*s).to_owned()).collect();
        let hex64 = |v: &str| v.len() == 64 && v.bytes().all(|c| c.is_ascii_hexdigit());
        let (shown, secrets) = mask_args_checked(&args, &["--token"], &hex64, &[]);
        assert_eq!(shown.join(" "), "prog serve --token *** ***");
        assert_eq!(secrets, [a, b]);
    }

    #[test]
    fn no_file_is_not_an_error() {
        let l = CliLogger::new(Filter::parse("info"), false, false, None);
        rec(&l, log::Level::Error, "app", "nowhere to go, and that is fine");
        l.flush();
        assert!(l.path().is_none());
    }
}
