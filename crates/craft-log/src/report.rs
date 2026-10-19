//! The diagnostic report (contract B): ordered `(section, key, value)` lines, rendered as text and
//! as JSON, written at the top of a log and printed by `<app>-cli diagnostics`.
//!
//! craft-log fills the fields every app shares, from what Linux shows in `/proc`, `/etc` and the
//! environment (no dependencies, nothing run but `ffmpeg -version`); the app adds its own sections
//! (graphics, video, screens, folders, the previous run) in between. Every value is a string, and
//! a field nobody could read says "unknown" rather than going missing. The home folder is shown as
//! `~` everywhere ([`mask_home`]). On other systems the Linux-only fields read "unknown".
//!
//! The order, which the app keeps by calling these in turn:
//! [`Report::new`] (app, system, processor, memory) → the app's graphics, video, screens and
//! folders → [`Report::add_switches`] → [`Report::add_helpers`] → the app's previous run →
//! [`Report::add_logs`], last, so the report ends with where the logs are.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::App;
use crate::line::escape;

/// What a field says when it could not be read.
pub const UNKNOWN: &str = "unknown";

/// How long `ffmpeg -version` may take before it is given up on.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(3);

/// One field: its section, its name and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub section: String,
    pub key: String,
    pub value: String,
}

/// The report: fields in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    fields: Vec<Field>,
}

impl Report {
    /// An empty report (the app's tests use this; programs use [`Report::new`]).
    pub fn empty() -> Report {
        Report::default()
    }

    /// A report holding the fields craft-log gathers first: the app and its build, the system,
    /// the processor and memory.
    pub fn new(app: &App) -> Report {
        let mut r = Report::empty();
        r.add("app", "name", app.name);
        r.add("app", "version", app.version);
        r.add("app", "commit", format!("{}{}", crate::BUILD_COMMIT, crate::BUILD_TREE));
        r.add("app", "build profile", crate::BUILD_PROFILE);
        r.add("app", "built on", crate::BUILD_DATE);
        r.add("app", "runs from", std::env::current_exe().map(|p| mask_home(&p.display().to_string())).unwrap_or_default());
        let windows = cfg!(windows).then(|| windows_version_in_background(app.name)).flatten();
        r.add("system", "operating system", read("/etc/os-release").map(|t| os_name(&t)).or(windows).unwrap_or_default());
        r.add("system", "kernel", read("/proc/sys/kernel/osrelease").map(|t| t.trim().to_owned()).unwrap_or_default());
        r.add("system", "architecture", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));
        r.add("system", "session", session(&|k| std::env::var(k).ok()));
        r.add("system", "locale", locale(&|k| std::env::var(k).ok()));
        r.add("processor", "model", read("/proc/cpuinfo").map(|t| cpu_model(&t)).unwrap_or_else(|| windows_processor(&|k| std::env::var(k).ok())));
        r.add("processor", "threads", std::thread::available_parallelism().map(|n| n.to_string()).unwrap_or_default());
        let mem = read("/proc/meminfo").map(|t| meminfo(&t)).or_else(|| MEMORY.get().and_then(|f| f()).map(|(t, a)| (gib(t), gib(a)))).unwrap_or_default();
        r.add("memory", "total", mem.0);
        r.add("memory", "available", mem.1);
        r
    }

    /// Add a field. An empty value reads "unknown"; a key already in the section gets " 2", " 3"…
    pub fn add(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let value = value.into();
        let value = if value.trim().is_empty() { UNKNOWN.to_owned() } else { value };
        let taken = |k: &str| self.fields.iter().any(|f| f.section == section && f.key == k);
        let mut unique = key.to_owned();
        let mut n = 1;
        while taken(&unique) {
            n += 1;
            unique = format!("{key} {n}");
        }
        self.fields.push(Field { section: section.to_owned(), key: unique, value });
    }

    /// The fields, in order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The value of `section`/`key`, if present.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.fields.iter().find(|f| f.section == section && f.key == key).map(|f| f.value.as_str())
    }

    /// The switches in force: every environment variable starting with `CRAFT_`, `RUST_LOG`, and
    /// those starting with one of the app's `prefixes` (`FILMCRAFT_`, `WGPU_`), the home folder
    /// masked. A switch named in `secret` (`PHOTOCRAFT_CONTROL_TOKEN`), or whose name says it holds
    /// a secret ([`is_secret_name`]), shows only "set (***)".
    pub fn add_switches(&mut self, prefixes: &[&str], secret: &[&str]) {
        let vars: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.to_string_lossy().into_owned()))).collect();
        let found = switches(&vars, prefixes, secret);
        if found.is_empty() {
            self.add("switches", "environment", "none");
        }
        for (k, v) in found {
            self.add("switches", &k, mask_home(&v));
        }
    }

    /// Helper programs: ffmpeg (where it is on `PATH`, and its first version line). Waits for
    /// `ffmpeg -version` (up to 3 s): for `<app>-cli diagnostics`. A window app uses
    /// [`Report::add_helpers_later`] and [`log_helpers_in_background`] so its window never waits.
    pub fn add_helpers(&mut self) {
        self.add("helpers", "ffmpeg", ffmpeg_helper(find_on_path("ffmpeg")));
    }

    /// The helpers' field for a report logged before they are probed: it points at the line
    /// [`log_helpers_in_background`] writes when the answer arrives.
    pub fn add_helpers_later(&mut self) {
        self.add("helpers", "ffmpeg", "probed in the background: the \"report: [helpers] ffmpeg:\" line after this report");
    }

    /// Where the logs are: the folder, each named log (`("window log", path)`), and the newest
    /// crash file. Last, by the contract, so whoever reads the report's end finds them.
    pub fn add_logs(&mut self, folder: Option<&Path>, logs: &[(&str, Option<&Path>)], newest_crash: Option<&Path>) {
        let show = |p: Option<&Path>, none: &str| p.map_or_else(|| none.to_owned(), |p| mask_home(&p.display().to_string()));
        self.add("logs", "folder", show(folder, UNKNOWN));
        for (name, path) in logs {
            self.add("logs", name, show(*path, "none"));
        }
        self.add("logs", "newest crash file", show(newest_crash, "none"));
    }

    /// The report as text: a title, then each section's fields.
    pub fn text(&self) -> String {
        let mut s = String::from("diagnostic report\n");
        let mut section = "";
        for f in &self.fields {
            if f.section != section {
                section = &f.section;
                let _ = writeln!(s, "[{section}]");
            }
            let _ = writeln!(s, "  {}: {}", f.key, f.value);
        }
        s
    }

    /// The report as one JSON object: `{"<section>": {"<key>": "<value>", …}, …}`, in order.
    pub fn json(&self) -> String {
        let mut s = String::from("{");
        let mut section: Option<&str> = None;
        for f in &self.fields {
            if section != Some(f.section.as_str()) {
                if section.is_some() {
                    s.push_str("},");
                }
                section = Some(&f.section);
                let _ = write!(s, "\"{}\":{{", escape(&f.section));
            } else {
                s.push(',');
            }
            let _ = write!(s, "\"{}\":\"{}\"", escape(&f.key), escape(&f.value));
        }
        if section.is_some() {
            s.push('}');
        }
        s.push('}');
        s
    }

    /// Write the report into the log, one info line per field (`report: [section] key: value`),
    /// under `target`.
    pub fn log(&self, target: &str) {
        for f in &self.fields {
            log::info!(target: target, "report: [{}] {}: {}", f.section, f.key, f.value);
        }
    }
}

/// The ffmpeg helper's value: where `found` is (home masked) and its first version line, or why
/// not.
pub fn ffmpeg_helper(found: Option<PathBuf>) -> String {
    match found {
        None => "not found on PATH".to_owned(),
        Some(p) => {
            let version = first_line_of(&p, &["-version"], FFMPEG_TIMEOUT).unwrap_or_else(|why| format!("{UNKNOWN} ({why})"));
            format!("{} ({version})", mask_home(&p.display().to_string()))
        }
    }
}

/// Probe the helpers on a background thread and log `report: [helpers] ffmpeg: …` at info under
/// `target` when the answer arrives (after [`Report::log`], so it follows the report). For a
/// window app, whose window must not wait for `ffmpeg -version`. The handle gives the value
/// (tests); `None` when no thread could be started (nothing is logged then).
pub fn log_helpers_in_background(target: &'static str) -> Option<std::thread::JoinHandle<String>> {
    probe_in_background(target, || ffmpeg_helper(find_on_path("ffmpeg")))
}

/// [`log_helpers_in_background`] with the probe given.
pub fn probe_in_background(target: &'static str, probe: impl FnOnce() -> String + Send + 'static) -> Option<std::thread::JoinHandle<String>> {
    std::thread::Builder::new()
        .name("craft-helpers".into())
        .spawn(move || {
            let value = probe();
            log::info!(target: target, "report: [helpers] ffmpeg: {value}");
            value
        })
        .ok()
}

/// Where an app that knows the machine's memory without `/proc` (Windows, macOS) tells the report:
/// (total, available) in bytes.
static MEMORY: std::sync::OnceLock<MemoryReader> = std::sync::OnceLock::new();

/// Reads the machine's (total, available) memory in bytes ([`set_memory_reader`]).
pub type MemoryReader = fn() -> Option<(u64, u64)>;

/// Give the report a reader for the machine's (total, available) memory in bytes, for systems
/// without `/proc/meminfo` (EffectCraft: its sysmem crate; PhotoCraft: winsafe). Once; later calls
/// are ignored.
pub fn set_memory_reader(read: MemoryReader) {
    let _ = MEMORY.set(read);
}

/// Bytes as the report shows memory: `"62.5 GiB"`.
pub fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
}

/// Windows: the processor as the system names it (`PROCESSOR_IDENTIFIER`, for example "AMD64
/// Family 26 Model 112 Stepping 0, AuthenticAMD"), else empty (the report shows "unknown").
pub fn windows_processor(env: &dyn Fn(&str) -> Option<String>) -> String {
    env("PROCESSOR_IDENTIFIER").map(|v| v.trim().to_owned()).unwrap_or_default()
}

/// Windows: the version in `ver`'s output ("Microsoft Windows [Version 10.0.26100.4652]" gives
/// "Windows 10.0.26100.4652"), else `None`.
pub fn windows_version(ver: &str) -> Option<String> {
    let v = ver.split("[Version ").nth(1)?.split(']').next()?.trim();
    (!v.is_empty()).then(|| format!("Windows {v}"))
}

/// Windows: the operating system's version is read by `cmd /c ver` on a background thread (a
/// child process, so never in the way of a start) and logged when it arrives as
/// `report: [system] operating system: …`; the report's field says so. `None` elsewhere, or when
/// no thread could be started.
fn windows_version_in_background(target: &'static str) -> Option<String> {
    std::thread::Builder::new()
        .name("craft-windows-version".into())
        .spawn(move || {
            let line = first_line_of(Path::new("cmd"), &["/c", "ver"], Duration::from_secs(3)).ok().and_then(|o| windows_version(&o));
            log::info!(target: target, "report: [system] operating system: {}", line.as_deref().unwrap_or(UNKNOWN));
        })
        .ok()?;
    Some("Windows (version read in the background: the \"report: [system] operating system:\" line after this report)".to_owned())
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// `~` in place of this machine's home folder ([`home_folders`]), wherever it is a whole path or
/// starts one in `text`. On Windows the match ignores ASCII case, as its file names do.
pub fn mask_home(text: &str) -> String {
    let homes = home_folders(&|k| std::env::var(k).ok(), cfg!(windows));
    mask_homes(text, &homes, cfg!(windows))
}

/// The home folders to mask, longest first: on Unix `HOME`; on Windows `USERPROFILE`,
/// `HOMEDRIVE` + `HOMEPATH`, and `HOME` when set (Git Bash and MSYS set it), each also written with
/// forward slashes. `env` reads a variable (tests pass their own).
pub fn home_folders(env: &dyn Fn(&str) -> Option<String>, windows: bool) -> Vec<String> {
    let mut homes: Vec<String> = Vec::new();
    let mut add = |h: Option<String>| {
        if let Some(h) = h.map(|h| h.trim_end_matches(['/', '\\']).to_owned()).filter(|h| h.len() > 1) {
            let forward = h.replace('\\', "/");
            homes.push(h);
            if windows {
                homes.push(forward);
            }
        }
    };
    if windows {
        add(env("USERPROFILE"));
        add(env("HOMEDRIVE").zip(env("HOMEPATH")).map(|(d, p)| format!("{d}{p}")));
    }
    add(env("HOME"));
    homes.sort_by_key(|h| std::cmp::Reverse(h.len()));
    homes.dedup();
    homes
}

/// [`mask_home`] with the home folder given (case as given).
pub fn mask_with(text: &str, home: Option<&str>) -> String {
    let homes: Vec<String> = home.map(|h| h.trim_end_matches(['/', '\\']).to_owned()).into_iter().collect();
    mask_homes(text, &homes, false)
}

/// Each occurrence of one of `homes` (tried in order) that is not followed by a character that
/// could continue a folder name (a letter, a digit, `_` or `-`) becomes `~`: `/home/deck/x`,
/// `/home/deck.`, `[/home/deck]`, `/home/deck&` and a home at the end of a line are masked;
/// `/home/deckard` is left alone when the home folder is `/home/deck`. A home of one character
/// (`/`) masks nothing.
pub fn mask_homes(text: &str, homes: &[String], ignore_ascii_case: bool) -> String {
    let mut out = text.to_owned();
    for home in homes.iter().filter(|h| h.len() > 1) {
        out = mask_one(&out, home, ignore_ascii_case);
    }
    out
}

fn mask_one(text: &str, home: &str, ignore_ascii_case: bool) -> String {
    // ASCII lower-casing keeps every byte offset, so positions found in the copy hold in `text`
    let (hay, needle) = if ignore_ascii_case { (text.to_ascii_lowercase(), home.to_ascii_lowercase()) } else { (text.to_owned(), home.to_owned()) };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(i) = hay[at..].find(&needle).map(|i| at + i) {
        let end = i + needle.len();
        let continues = text[end..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        out.push_str(&text[at..i]);
        out.push_str(if continues { &text[i..end] } else { "~" });
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// `PRETTY_NAME` from `/etc/os-release` text, else `NAME VERSION_ID`, with `BUILD_ID` and
/// `VARIANT_ID` added when present (SteamOS puts its build there).
pub fn os_name(os_release: &str) -> String {
    let field = |k: &str| {
        os_release.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_owned()).filter(|v| !v.is_empty())
    };
    let mut name = field("PRETTY_NAME").or_else(|| Some(format!("{} {}", field("NAME")?, field("VERSION_ID").unwrap_or_default()).trim().to_owned()));
    if let Some(n) = &mut name {
        if let Some(v) = field("VERSION_ID").filter(|v| !n.contains(v.as_str())) {
            let _ = write!(n, " {v}");
        }
        if let Some(b) = field("BUILD_ID") {
            let _ = write!(n, " (build {b})");
        }
        if let Some(v) = field("VARIANT_ID") {
            let _ = write!(n, " [{v}]");
        }
    }
    name.unwrap_or_default()
}

/// The first `model name` (x86) or `Model`/`Hardware` (ARM) in `/proc/cpuinfo` text.
pub fn cpu_model(cpuinfo: &str) -> String {
    ["model name", "Model", "Hardware"]
        .iter()
        .find_map(|k| cpuinfo.lines().find_map(|l| l.split_once(':').filter(|(a, _)| a.trim() == *k).map(|(_, v)| v.trim().to_owned())))
        .unwrap_or_default()
}

/// (total, available) from `/proc/meminfo` text, as `"62.5 GiB"`.
pub fn meminfo(text: &str) -> (String, String) {
    let kib =
        |k: &str| -> Option<u64> { text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix(':')).and_then(|v| v.split_whitespace().next()?.parse().ok()) };
    let gib = |v: Option<u64>| v.map(|k| format!("{:.1} GiB", k as f64 / 1_048_576.0)).unwrap_or_default();
    (gib(kib("MemTotal")), gib(kib("MemAvailable")))
}

/// The graphical session from the environment `env` reads: Game Mode (gamescope) when Steam's
/// game UI is running, else the session type and the desktop (`wayland, KDE`), else "unknown".
pub fn session(env: &dyn Fn(&str) -> Option<String>) -> String {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    let desktop = set("XDG_CURRENT_DESKTOP");
    let gamescope = desktop.as_deref().is_some_and(|d| d.to_ascii_lowercase().contains("gamescope")) || set("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    if set("SteamGamepadUI").is_some_and(|v| v != "0") || gamescope {
        return "SteamOS Game Mode (gamescope)".into();
    }
    let kind = set("XDG_SESSION_TYPE").or_else(|| set("WAYLAND_DISPLAY").map(|_| "wayland".into())).or_else(|| set("DISPLAY").map(|_| "x11".into()));
    match (kind, desktop) {
        (Some(k), Some(d)) => format!("{k}, {d}"),
        (Some(k), None) => k,
        (None, Some(d)) => d,
        (None, None) if cfg!(windows) => "Windows".into(),
        (None, None) => UNKNOWN.into(),
    }
}

/// `LC_ALL`, else `LANG`, else "unknown".
pub fn locale(env: &dyn Fn(&str) -> Option<String>) -> String {
    ["LC_ALL", "LANG"].iter().find_map(|k| env(k).filter(|v| !v.is_empty())).unwrap_or_else(|| UNKNOWN.into())
}

/// What a secret switch shows in place of its value.
pub const SECRET_SET: &str = "set (***)";

/// Whether a switch's name says it holds a secret (`…TOKEN…`, `…SECRET…`, `…PASSWORD…`,
/// `…PASSWD…`, `…CREDENTIAL…`, `…API_KEY…`, `…APIKEY…`, `…AUTH…`, `…BEARER…`, `…COOKIE…`,
/// `…PRIVATE_KEY…`, or ending `_KEY`, `_PASS`, `_PWD`), a guard for switches nobody listed.
pub fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "API_KEY", "APIKEY", "AUTH", "BEARER", "COOKIE", "PRIVATE_KEY"].iter().any(|w| n.contains(w))
        || ["_KEY", "_PASS", "_PWD"].iter().any(|w| n.ends_with(w))
}

/// The switches among `vars`: `CRAFT_*`, `RUST_LOG`, and names starting with one of `prefixes`,
/// sorted by name; those named in `secret` or by [`is_secret_name`] show [`SECRET_SET`].
pub fn switches(vars: &[(String, String)], prefixes: &[&str], secret: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vars
        .iter()
        .filter(|(k, _)| k.starts_with("CRAFT_") || k == "RUST_LOG" || prefixes.iter().any(|p| k.starts_with(p)))
        .map(|(k, v)| if secret.contains(&k.as_str()) || is_secret_name(k) { (k.clone(), SECRET_SET.to_owned()) } else { (k.clone(), v.clone()) })
        .collect();
    out.sort();
    out
}

/// The first file named `program` in a folder on `PATH`.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) { vec![format!("{program}.exe")] } else { vec![program.to_owned()] };
    std::env::split_paths(&path).flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Run `program args` and return the first line with text it printed, killing it after `timeout`. On
/// Windows it runs without a console window (`CREATE_NO_WINDOW`), so a window app that asks
/// flashes none.
pub fn first_line_of(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no answer within {} s", timeout.as_secs()));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut out, &mut text);
    }
    // the first line with text (Windows' `ver` starts with an empty one)
    text.lines().map(str::trim_end).find(|l| !l.trim().is_empty()).map(str::to_owned).ok_or_else(|| "printed nothing".into())
}

/// The most recently modified file in `dir` whose name `wanted` accepts.
pub fn newest_file(dir: &Path, wanted: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(wanted) && e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// The last line of the file at `path` that contains `needle`, looking at its last `max_bytes`.
/// Only a regular file is read: opening a FIFO waits for a writer (see [`crate::cli::open_append`]).
pub fn last_line_with(path: &Path, needle: &str, max_bytes: u64) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    crate::cli::nonblocking(&mut options);
    let mut f = options.open(path).ok()?;
    let meta = f.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let len = meta.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max_bytes))).ok()?;
    let mut bytes = Vec::new();
    f.take(max_bytes).read_to_end(&mut bytes).ok()?;
    String::from_utf8_lossy(&bytes).lines().rev().find(|l| l.contains(needle)).map(str::to_owned)
}

/// When `path` was created, where the file system records it (a rename keeps it).
pub fn created(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.created().ok()
}

/// When `path` was last modified.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_replaces_the_home_folder_only_where_a_path_continues() {
        let home = Some("/home/deck");
        assert_eq!(mask_with("/home/deck/dev/x and /home/deck", home), "~/dev/x and ~");
        assert_eq!(mask_with("/home/deckard/x", home), "/home/deckard/x");
        assert_eq!(mask_with("FILMCRAFT_DATA_DIR=/home/deck/a:/home/deck/b", home), "FILMCRAFT_DATA_DIR=~/a:~/b");
        assert_eq!(mask_with("C:\\Users\\sam\\AppData", Some("C:\\Users\\sam\\")), "~\\AppData");
        assert_eq!(mask_with("/x", None), "/x");
        assert_eq!(mask_with("/x", Some("/")), "/x", "a root home masks nothing");
    }

    #[test]
    fn the_home_folder_is_masked_before_any_character_that_cannot_continue_its_name() {
        let home = Some("/home/deck");
        for (text, want) in [
            ("see /home/deck.", "see ~."),
            ("[/home/deck]", "[~]"),
            ("<path>/home/deck</path>", "<path>~</path>"),
            ("a=/home/deck&b=1", "a=~&b=1"),
            ("/home/deck\nnext", "~\nnext"),
            ("/home/deck\tx", "~\tx"),
            ("{\"dir\":\"/home/deck\"}", "{\"dir\":\"~\"}"),
            ("/home/deck|/home/deck=", "~|~="),
        ] {
            assert_eq!(mask_with(text, home), want, "{text:?}");
        }
        // another user's folder whose name starts the same is not this home
        assert_eq!(mask_with("/home/deck_2/x /home/deck-old /home/deck9", home), "/home/deck_2/x /home/deck-old /home/deck9");
    }

    #[test]
    fn windows_fields_come_from_the_environment_and_ver() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| (*v).to_owned());
        assert_eq!(
            windows_processor(&env(&[("PROCESSOR_IDENTIFIER", "AMD64 Family 26 Model 112 Stepping 0, AuthenticAMD ")])),
            "AMD64 Family 26 Model 112 Stepping 0, AuthenticAMD"
        );
        assert_eq!(windows_processor(&env(&[])), "");
        assert_eq!(windows_version("\r\nMicrosoft Windows [Version 10.0.26100.4652]\r\n").as_deref(), Some("Windows 10.0.26100.4652"));
        assert_eq!(windows_version("Microsoft Windows [Versión 10.0.19045.5965]"), None, "a translated word: no version");
        assert_eq!(windows_version(""), None);
        assert_eq!(gib(64 << 30), "64.0 GiB");
    }

    #[cfg(windows)]
    #[test]
    fn a_report_on_windows_names_the_processor_and_the_version_line() {
        let app = crate::App { name: "app", version: "1.0.0", ours: "app", default_filter: "warn" };
        let r = Report::new(&app);
        assert_ne!(r.get("processor", "model"), Some(UNKNOWN), "PROCESSOR_IDENTIFIER is always set on Windows");
        assert!(r.get("system", "operating system").is_some_and(|v| v.starts_with("Windows")), "{:?}", r.get("system", "operating system"));
    }

    #[test]
    fn os_release_names_steamos_with_its_build() {
        let steamos = "NAME=\"SteamOS\"\nPRETTY_NAME=\"SteamOS\"\nVERSION_ID=\"3.8\"\nBUILD_ID=\"20260930.1\"\nVARIANT_ID=steamdeck\nID=steamos\n";
        assert_eq!(os_name(steamos), "SteamOS 3.8 (build 20260930.1) [steamdeck]");
        assert_eq!(os_name("NAME=Fedora Linux\nVERSION_ID=41\n"), "Fedora Linux 41");
        assert_eq!(os_name("PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nVERSION_ID=\"24.04\"\n"), "Ubuntu 24.04.1 LTS");
        assert_eq!(os_name(""), "");
    }

    #[test]
    fn cpuinfo_and_meminfo() {
        let cpu = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S\nprocessor\t: 1\nmodel name\t: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S\n";
        assert_eq!(cpu_model(cpu), "AMD RYZEN AI MAX+ 395 w/ Radeon 8060S");
        assert_eq!(cpu_model("processor : 0\nHardware : BCM2835\n"), "BCM2835");
        let mem = "MemTotal:       65536000 kB\nMemFree:         1000 kB\nMemAvailable:   32768000 kB\n";
        assert_eq!(meminfo(mem), ("62.5 GiB".into(), "31.2 GiB".into()));
        assert_eq!(meminfo("nothing"), (String::new(), String::new()));
    }

    #[test]
    fn session_and_locale_from_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| (*v).to_owned());
        assert_eq!(session(&env(&[("XDG_SESSION_TYPE", "wayland"), ("XDG_CURRENT_DESKTOP", "KDE")])), "wayland, KDE");
        assert_eq!(session(&env(&[("XDG_SESSION_TYPE", "wayland"), ("SteamGamepadUI", "1")])), "SteamOS Game Mode (gamescope)");
        assert_eq!(session(&env(&[("XDG_CURRENT_DESKTOP", "gamescope")])), "SteamOS Game Mode (gamescope)");
        assert_eq!(session(&env(&[("DISPLAY", ":0")])), "x11");
        if !cfg!(windows) {
            assert_eq!(session(&env(&[])), "unknown");
        }
        assert_eq!(locale(&env(&[("LANG", "en_US.UTF-8")])), "en_US.UTF-8");
        assert_eq!(locale(&env(&[("LANG", "")])), "unknown");
    }

    #[test]
    fn switches_are_ours_and_sorted() {
        let vars: Vec<(String, String)> =
            [("PATH", "/bin"), ("FILMCRAFT_DATA_DIR", "/d"), ("CRAFT_LOG", "debug"), ("RUST_LOG", "info"), ("WGPU_BACKEND", "vulkan")]
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect();
        let got: Vec<String> = switches(&vars, &["FILMCRAFT_", "WGPU_"], &[]).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(got, ["CRAFT_LOG=debug", "FILMCRAFT_DATA_DIR=/d", "RUST_LOG=info", "WGPU_BACKEND=vulkan"]);
    }

    #[test]
    fn a_secret_switch_shows_only_that_it_is_set() {
        let vars: Vec<(String, String)> = [("APP_CONTROL_TOKEN", "s3cr3t-value"), ("APP_DATA_DIR", "/d"), ("APP_REMOTE", "hunter2")]
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect();
        let got: Vec<String> = switches(&vars, &["APP_"], &["APP_REMOTE"]).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert!(!got.iter().any(|l| l.contains("s3cr3t-value") || l.contains("hunter2")), "{got:?}");
        // named in the list, or by its name
        assert_eq!(got, ["APP_CONTROL_TOKEN=set (***)", "APP_DATA_DIR=/d", "APP_REMOTE=set (***)"]);
    }

    #[test]
    fn secret_names_beyond_token_and_password_are_caught() {
        for name in ["APP_APIKEY", "APP_SIGNING_KEY", "APP_DB_PASS", "APP_SMTP_PWD", "APP_BEARER", "APP_COOKIE", "APP_PRIVATE_KEY_PEM"] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["APP_DATA_DIR", "APP_KEYFRAMES", "APP_BYPASS_CACHE", "APP_PASSES", "WGPU_BACKEND"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn the_home_folder_comes_from_the_platforms_own_variables() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| (*v).to_owned());
        // Windows does not set HOME: USERPROFILE, HOMEDRIVE + HOMEPATH, in both slash styles
        let win = env(&[("USERPROFILE", "C:\\Users\\sam"), ("HOMEDRIVE", "D:"), ("HOMEPATH", "\\Users\\sam")]);
        let homes = home_folders(&win, true);
        assert_eq!(homes, ["C:\\Users\\sam", "C:/Users/sam", "D:\\Users\\sam", "D:/Users/sam"]);
        assert_eq!(
            mask_homes("C:\\Users\\sam\\AppData c:\\users\\SAM\\x C:/Users/sam/y D:\\Users\\sam. C:\\Users\\sama", &homes, true),
            "~\\AppData ~\\x ~/y ~. C:\\Users\\sama"
        );
        // Git Bash's HOME is masked too
        assert!(home_folders(&env(&[("USERPROFILE", "C:\\Users\\sam"), ("HOME", "/c/Users/sam")]), true).contains(&"/c/Users/sam".to_owned()));
        // Unix: HOME only
        assert_eq!(home_folders(&env(&[("HOME", "/home/deck/"), ("USERPROFILE", "x:\\y")]), false), ["/home/deck"]);
        assert!(home_folders(&env(&[("HOME", "/")]), false).is_empty());
    }

    #[test]
    fn a_report_renders_in_order_as_text_and_json_with_unknowns() {
        let mut r = Report::empty();
        r.add("app", "name", "filmcraft");
        r.add("app", "commit", "");
        r.add("graphics", "adapter", "Radeon \"8060S\"");
        r.add("graphics", "adapter", "llvmpipe");
        r.add_logs(Some(Path::new("/tmp/Logs")), &[("newest log", None)], None);
        assert_eq!(r.get("app", "commit"), Some("unknown"));
        assert_eq!(r.get("graphics", "adapter 2"), Some("llvmpipe"));
        assert_eq!(
            r.text(),
            "diagnostic report\n[app]\n  name: filmcraft\n  commit: unknown\n[graphics]\n  adapter: Radeon \"8060S\"\n  adapter 2: llvmpipe\n[logs]\n  folder: /tmp/Logs\n  newest log: none\n  newest crash file: none\n"
        );
        assert_eq!(
            r.json(),
            r#"{"app":{"name":"filmcraft","commit":"unknown"},"graphics":{"adapter":"Radeon \"8060S\"","adapter 2":"llvmpipe"},"logs":{"folder":"/tmp/Logs","newest log":"none","newest crash file":"none"}}"#
        );
        assert_eq!(Report::empty().json(), "{}");
    }

    #[test]
    fn the_gathered_fields_are_never_missing() {
        let app = App { name: "testapp", version: "1.0.0", ours: "testapp", default_filter: "info" };
        let mut r = Report::new(&app);
        r.add_switches(&["TESTAPP_"], &[]);
        r.add_helpers();
        for (s, k) in [("app", "commit"), ("system", "kernel"), ("system", "session"), ("processor", "threads"), ("memory", "total"), ("helpers", "ffmpeg")] {
            assert!(r.get(s, k).is_some_and(|v| !v.is_empty()), "{s}/{k} in {}", r.text());
        }
        if cfg!(target_os = "linux") {
            assert_ne!(r.get("system", "kernel"), Some(UNKNOWN));
            assert_ne!(r.get("memory", "total"), Some(UNKNOWN));
        }
    }

    #[test]
    fn a_program_that_does_not_answer_is_given_up_on() {
        if !cfg!(unix) || !Path::new("/bin/sleep").is_file() {
            return;
        }
        let t0 = Instant::now();
        let got = first_line_of(Path::new("/bin/sleep"), &["5"], Duration::from_millis(200));
        assert!(got.is_err_and(|e| e.contains("no answer")));
        assert!(t0.elapsed() < Duration::from_secs(3));
        assert_eq!(first_line_of(Path::new("/bin/echo"), &["hello"], FFMPEG_TIMEOUT).as_deref(), Ok("hello"));
    }

    #[test]
    fn a_fifo_in_place_of_a_log_is_not_waited_for() {
        let dir = std::env::temp_dir().join(format!("craft-log-report-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("app.log");
        if !Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
            return; // no mkfifo (Windows): nothing to check
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(last_line_with(&fifo, "x", 10)));
        let got = rx.recv_timeout(Duration::from_secs(5));
        assert_eq!(got, Ok(None), "reading the previous run's end waited on a FIFO");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_helpers_are_probed_without_waiting() {
        if !cfg!(unix) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("craft-log-report-helpers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // an ffmpeg that takes a second to answer, as a cold disk or a virus scanner makes it
        let slow = dir.join("ffmpeg");
        std::fs::write(&slow, "#!/bin/sh\nsleep 1\necho \"ffmpeg version 9.9-test\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let t0 = Instant::now();
        let probe = probe_in_background("report-test", move || ffmpeg_helper(Some(slow))).unwrap();
        assert!(t0.elapsed() < Duration::from_millis(300), "the caller waited {:?}", t0.elapsed());
        let value = probe.join().unwrap();
        assert!(value.ends_with("(ffmpeg version 9.9-test)"), "{value}");
        assert!(t0.elapsed() >= Duration::from_secs(1));
        let mut r = Report::empty();
        r.add_helpers_later();
        assert!(r.get("helpers", "ffmpeg").is_some_and(|v| v.contains("background")));
        assert_eq!(ffmpeg_helper(None), "not found on PATH");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_newest_file_is_found_by_name() {
        let dir = std::env::temp_dir().join(format!("craft-log-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crash-1.log"), "x").unwrap();
        std::fs::write(dir.join("app.log"), "x").unwrap();
        assert_eq!(newest_file(&dir, &|n| n.starts_with("crash-")), Some(dir.join("crash-1.log")));
        assert_eq!(newest_file(&dir, &|n| n.ends_with(".txt")), None);
        assert!(modified(&dir.join("app.log")).is_some());
        std::fs::write(dir.join("run.log"), "start\nrun ended: one\nmiddle\nrun ended: two\nlast\n").unwrap();
        assert_eq!(last_line_with(&dir.join("run.log"), "run ended:", 4096).as_deref(), Some("run ended: two"));
        assert_eq!(last_line_with(&dir.join("run.log"), "run ended:", 5), None, "only the tail is read");
        assert_eq!(last_line_with(&dir.join("missing.log"), "x", 10), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
