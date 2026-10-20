//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds one key, `session_folder`.
//!
//! The **session folder** is where opening a file writes its session store (one subfolder per
//! open file). The first of these that is set wins: the `--session-dir` command-line flag, the
//! [`ENV_SESSION_DIR`] environment variable, the settings file, the default
//! ([`default_sessions_root`](crate::store::default_sessions_root)). A folder that can't be made
//! or written falls back to the default, with a message saying why.
//!
//! Preferences (A278, A281; older files without these keys keep working, each key missing gives
//! its default): `autosave_enabled` (default on), `autosave_interval_secs` (how often the journal
//! may be written, default 2), `autosave_keep_days` (how many days "Clean up old sessions" keeps
//! a session, default 30), and `cache_mb` (the block cache's RAM budget; empty: the default,
//! [`default_cache_bytes`](crate::store::default_cache_bytes)).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The environment variable that sets the session folder (over the settings file).
pub const ENV_SESSION_DIR: &str = "WAVCRAFT_SESSION_DIR";
/// How a person changes the session folder, for messages.
pub const HOW_TO_CHANGE: &str = "File › Session Folder… in WavCraft, or the WAVCRAFT_SESSION_DIR environment variable";

const KEY_SESSION_FOLDER: &str = "session_folder";
const KEY_AUTOSAVE: &str = "autosave_enabled";
const KEY_INTERVAL: &str = "autosave_interval_secs";
const KEY_KEEP_DAYS: &str = "autosave_keep_days";
const KEY_CACHE_MB: &str = "cache_mb";

/// The journal is written at most this often by default (seconds).
pub const DEFAULT_INTERVAL_SECS: u64 = 2;
/// Allowed journal intervals, seconds.
pub const INTERVAL_RANGE: std::ops::RangeInclusive<u64> = 1..=3600;
/// The default age, in days, at which "Clean up old sessions" may remove a session.
pub const DEFAULT_KEEP_DAYS: u32 = 30;
/// Allowed values for the keep-days setting.
pub const KEEP_DAYS_RANGE: std::ops::RangeInclusive<u32> = 1..=3650;
/// Allowed block cache budgets, megabytes.
pub const CACHE_MB_RANGE: std::ops::RangeInclusive<u64> = 64..=65_536;

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Auto Save writes a journal while the document has unsaved work.
    pub autosave_enabled: bool,
    /// The most often a journal is written, in seconds.
    pub autosave_interval_secs: u64,
    /// "Clean up old sessions" removes a session only when it is older than this many days.
    pub autosave_keep_days: u32,
    /// The block cache's RAM budget in megabytes (`None`: the default).
    pub cache_mb: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_folder: None,
            autosave_enabled: true,
            autosave_interval_secs: DEFAULT_INTERVAL_SECS,
            autosave_keep_days: DEFAULT_KEEP_DAYS,
            cache_mb: None,
        }
    }
}

/// The names `PREF get|set` knows.
pub const PREF_NAMES: [&str; 4] = [KEY_AUTOSAVE, KEY_INTERVAL, KEY_KEEP_DAYS, KEY_CACHE_MB];

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// The settings file: `<config dir>/wavcraft/settings.txt`, when the system has a config dir.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("wavcraft").join("settings.txt"))
    }

    /// Read `path`. A missing or unreadable file gives the defaults; lines it doesn't know are
    /// ignored.
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text),
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("could not read the settings file {}: {e}", path.display());
                }
                Settings::default()
            }
        }
    }

    /// Settings from the file's text.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                KEY_SESSION_FOLDER => s.session_folder = (!value.is_empty()).then(|| PathBuf::from(value)),
                // A value that is missing, unreadable or out of range is not used: the default stays.
                k if PREF_NAMES.contains(&k) => {
                    let _ = s.set_pref(k, value);
                }
                _ => {}
            }
        }
        s
    }

    /// The block cache budget in bytes: the setting, else the default.
    pub fn cache_bytes(&self) -> u64 {
        self.cache_mb.map_or_else(crate::store::default_cache_bytes, |mb| mb << 20)
    }

    /// One preference as text (`on`/`off`, a number, `default` for an unset cache size), or
    /// `None` for a name this does not know.
    pub fn get_pref(&self, name: &str) -> Option<String> {
        Some(match name {
            KEY_AUTOSAVE => (if self.autosave_enabled { "on" } else { "off" }).to_string(),
            KEY_INTERVAL => self.autosave_interval_secs.to_string(),
            KEY_KEEP_DAYS => self.autosave_keep_days.to_string(),
            KEY_CACHE_MB => self.cache_mb.map_or_else(|| "default".to_string(), |m| m.to_string()),
            _ => return None,
        })
    }

    /// Set one preference from text; an unknown name, an unreadable value or one out of range is
    /// an error saying so and changes nothing.
    pub fn set_pref(&mut self, name: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        fn num<T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy>(
            name: &str,
            v: &str,
            range: &std::ops::RangeInclusive<T>,
        ) -> Result<T, String> {
            let n: T = v.parse().map_err(|_| format!("{name} wants a whole number, not {v:?}"))?;
            if range.contains(&n) { Ok(n) } else { Err(format!("{name} must be from {} to {}", range.start(), range.end())) }
        }
        match name {
            KEY_AUTOSAVE => self.autosave_enabled = parse_bool(value).ok_or_else(|| format!("{name} wants on or off, not {value:?}"))?,
            KEY_INTERVAL => self.autosave_interval_secs = num(name, value, &INTERVAL_RANGE)?,
            KEY_KEEP_DAYS => self.autosave_keep_days = num(name, value, &KEEP_DAYS_RANGE)?,
            KEY_CACHE_MB => {
                self.cache_mb = if value.is_empty() || value == "default" { None } else { Some(num(name, value, &CACHE_MB_RANGE)?) };
            }
            _ => return Err(format!("no preference named {name:?}")),
        }
        Ok(())
    }

    /// The file's text for these settings.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft settings. One `key = value` per line.\n");
        out.push_str("# session_folder: where opened files are kept while you work on them (empty: the default).\n");
        let folder = self.session_folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        out.push_str(&format!("{KEY_SESSION_FOLDER} = {folder}\n"));
        out.push_str("# autosave_enabled: on or off. autosave_interval_secs: the most often the journal is written.\n");
        out.push_str("# autosave_keep_days: Clean up old sessions keeps a session this many days.\n");
        out.push_str("# cache_mb: the block cache's RAM budget in megabytes (empty: the default).\n");
        for k in PREF_NAMES {
            let v = self.get_pref(k).unwrap_or_default();
            out.push_str(&format!("{k} = {}\n", if v == "default" { "" } else { &v }));
        }
        out
    }

    /// Write the settings to `path` (its folder made if missing), replacing the file whole.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.new");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }
}

/// Make `path` if it is missing and check that a file can be written in it. The error says why
/// not, in words.
pub fn check_folder(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("no folder given".into());
    }
    std::fs::create_dir_all(path).map_err(|e| format!("it can't be made ({e})"))?;
    if !path.is_dir() {
        return Err("it is not a folder".into());
    }
    let probe = path.join(format!(".wavcraft-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"x").map_err(|e| format!("it can't be written to ({e})"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Where the session folder setting came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderSource {
    CommandLine,
    Environment,
    SettingsFile,
    Default,
}

impl FolderSource {
    fn describe(self) -> &'static str {
        match self {
            FolderSource::CommandLine => "set by --session-dir",
            FolderSource::Environment => "set by WAVCRAFT_SESSION_DIR",
            FolderSource::SettingsFile => "set in WavCraft's settings",
            FolderSource::Default => "the default",
        }
    }
}

/// The session folder in use and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFolder {
    pub path: PathBuf,
    pub source: FolderSource,
    /// Set when the folder asked for could not be used and the default is used instead.
    pub warning: Option<String>,
}

/// Choose the session folder: the first of `command_line`, `env` (the value of
/// [`ENV_SESSION_DIR`]) and `settings` that is set and not empty, else `default`. If the chosen
/// folder fails `check` ([`check_folder`] outside tests), the default is used and `warning` says
/// why. Relative paths are taken from the current folder.
pub fn resolve_session_folder(
    command_line: Option<PathBuf>,
    env: Option<OsString>,
    settings: &Settings,
    default: PathBuf,
    check: impl Fn(&Path) -> Result<(), String>,
) -> SessionFolder {
    let asked = [
        (command_line, FolderSource::CommandLine),
        (env.map(PathBuf::from), FolderSource::Environment),
        (settings.session_folder.clone(), FolderSource::SettingsFile),
    ]
    .into_iter()
    .find_map(|(p, source)| p.filter(|p| !p.as_os_str().is_empty()).map(|p| (p, source)));
    let Some((path, source)) = asked else {
        return SessionFolder { path: default, source: FolderSource::Default, warning: None };
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    match check(&path) {
        Ok(()) => SessionFolder { path, source, warning: None },
        Err(why) => {
            let mut warning = format!(
                "The session folder {} ({}) can't be used: {why}. Using the default, {}, instead.",
                path.display(),
                source.describe(),
                default.display()
            );
            if let Err(also) = check(&default) {
                warning.push_str(&format!(" That one can't be used either: {also}."));
            }
            log::warn!("{warning}");
            SessionFolder { path: default, source: FolderSource::Default, warning: Some(warning) }
        }
    }
}
