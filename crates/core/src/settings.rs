//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds `session_folder`, `hidden_panels` and the preferences of the Preferences dialog (see
//! [`KEYS`]; `input_device` is one of them); a file from an older build, with fewer keys, loads
//! with the defaults for the rest, and an older build reading a newer file ignores the keys it
//! doesn't know.
//!
//! The **session folder** is where opening a file writes its session store (one subfolder per
//! open file). The first of these that is set wins: the `--session-dir` command-line flag, the
//! [`ENV_SESSION_DIR`] environment variable, the settings file, the default
//! ([`default_sessions_root`](crate::store::default_sessions_root)). A folder that can't be made
//! or written falls back to the default, with a message saying why.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub mod multitrack;

/// The environment variable that sets the session folder (over the settings file).
pub const ENV_SESSION_DIR: &str = "WAVCRAFT_SESSION_DIR";
/// How a person changes the session folder, for messages.
pub const HOW_TO_CHANGE: &str = "File › Session Folder… in WavCraft, or the WAVCRAFT_SESSION_DIR environment variable";

const KEY_SESSION_FOLDER: &str = "session_folder";
const KEY_HIDDEN_PANELS: &str = "hidden_panels";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
const KEY_CUT_JOIN: &str = "crossfade_cut_join_ms";
const KEY_EDGE: &str = "crossfade_edge_ms";
const KEY_OUTPUT: &str = "output_device";
const KEY_INPUT: &str = "input_device";

/// Every preference key the Preferences dialog and `PREF` use, in the order the file lists them
/// (`hidden_panels` belongs to the Window menu and is not one of them).
pub const KEYS: [&str; 5] = [KEY_SESSION_FOLDER, KEY_CUT_JOIN, KEY_EDGE, KEY_OUTPUT, KEY_INPUT];

/// What a device preference shows (and accepts) for "no device chosen: use the system's".
pub const SYSTEM_DEFAULT: &str = "System default";

/// The edit crossfade defaults, in milliseconds (Audition's Preferences > Data).
pub const DEFAULT_CUT_JOIN_MS: u32 = 2;
pub const DEFAULT_EDGE_MS: u32 = 5;
/// The longest crossfade the preferences accept.
pub const MAX_CROSSFADE_MS: u32 = 1000;

pub mod video;

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Ids of the panels the person hid (`crate::panels`); a panel not listed is shown.
    pub hidden_panels: Vec<String>,
    /// Crossfade where Delete or Cut closes a gap, in ms (0: none).
    pub cut_join_ms: u32,
    /// Crossfade at every other edit boundary, in ms (0: none).
    pub edge_ms: u32,
    /// The output device's name (`None`: the system default).
    pub output_device: Option<String>,
    /// Preferred recording input by device name (`None`: the system default).
    pub input_device: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_folder: None,
            hidden_panels: Vec::new(),
            cut_join_ms: DEFAULT_CUT_JOIN_MS,
            edge_ms: DEFAULT_EDGE_MS,
            output_device: None,
            input_device: None,
        }
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
                KEY_HIDDEN_PANELS => s.hidden_panels = value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect(),
                // A value that doesn't fit its key leaves the default, as an unknown key is ignored.
                other => {
                    let _ = s.set(other, value);
                }
            }
        }
        s
    }

    /// The crossfade lengths as durations: `(cut_join, edge)`.
    pub fn crossfades(&self) -> (Duration, Duration) {
        (Duration::from_millis(u64::from(self.cut_join_ms)), Duration::from_millis(u64::from(self.edge_ms)))
    }

    /// The value of preference `key` as text (a device none is chosen for reads
    /// [`SYSTEM_DEFAULT`], an unset session folder reads empty).
    pub fn get(&self, key: &str) -> Result<String, String> {
        Ok(match key {
            KEY_SESSION_FOLDER => self.session_folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            KEY_CUT_JOIN => self.cut_join_ms.to_string(),
            KEY_EDGE => self.edge_ms.to_string(),
            KEY_OUTPUT => self.output_device.clone().unwrap_or_else(|| SYSTEM_DEFAULT.to_string()),
            KEY_INPUT => self.input_device.clone().unwrap_or_else(|| SYSTEM_DEFAULT.to_string()),
            _ => return Err(unknown_key(key)),
        })
    }

    /// Set preference `key` from text. A value that doesn't fit changes nothing.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        let device = |v: &str| (!v.is_empty() && v != SYSTEM_DEFAULT).then(|| v.to_string());
        let ms = |v: &str| match v.parse::<u32>() {
            Ok(n) if n <= MAX_CROSSFADE_MS => Ok(n),
            _ => Err(format!("{key} is a whole number of milliseconds, 0 to {MAX_CROSSFADE_MS}")),
        };
        match key {
            KEY_SESSION_FOLDER => self.session_folder = (!value.is_empty()).then(|| PathBuf::from(value)),
            KEY_CUT_JOIN => self.cut_join_ms = ms(value)?,
            KEY_EDGE => self.edge_ms = ms(value)?,
            KEY_OUTPUT => self.output_device = device(value),
            KEY_INPUT => self.input_device = device(value),
            _ => return Err(unknown_key(key)),
        }
        Ok(())
    }

    /// The file's text for these settings.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft settings. One `key = value` per line.\n");
        out.push_str("# session_folder: where opened files are kept while you work on them (empty: the default).\n");
        let folder = self.session_folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        out.push_str(&format!("{KEY_SESSION_FOLDER} = {folder}\n"));
        if !self.hidden_panels.is_empty() {
            out.push_str("# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).
");
            out.push_str(&format!("{KEY_HIDDEN_PANELS} = {}
", self.hidden_panels.join(",")));
        }
        // Only what differs from the defaults is written, so a default file keeps the old format.
        if self.cut_join_ms != DEFAULT_CUT_JOIN_MS || self.edge_ms != DEFAULT_EDGE_MS {
            out.push_str("# crossfade_*_ms: edit crossfades in milliseconds (Delete/Cut join, every other boundary); 0 turns one off.
");
            out.push_str(&format!("{KEY_CUT_JOIN} = {}
{KEY_EDGE} = {}
", self.cut_join_ms, self.edge_ms));
        }
        if let Some(output_device) = &self.output_device {
            out.push_str(&format!("{KEY_OUTPUT} = {output_device}
"));
        }
        if let Some(input_device) = &self.input_device {
            out.push_str(&format!("{KEY_INPUT} = {input_device}
"));
        }
        out
    }

    /// Write the settings to `path` (its folder made if missing), replacing the file whole.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        self.save_with_names(path, || {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            format!("{file_name}.{}.{nanos}.{}.tmp", std::process::id(), NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed))
        })
    }

    fn save_with_names(&self, path: &Path, mut next_name: impl FnMut() -> String) -> io::Result<()> {
        // Create the temp file exclusively (O_EXCL: never follows a symlink, never truncates)
        // under a unique name; if the name is taken, try another.
        for _ in 0..16 {
            let tmp = path.with_file_name(next_name());
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
                Ok(mut file) => {
                    let written = io::Write::write_all(&mut file, self.to_text().as_bytes());
                    drop(file);
                    if let Err(error) = written {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(error);
                    }
                    if let Err(error) = std::fs::rename(&tmp, path) {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(error);
                    }
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no unused temporary file name found"))
    }
}

fn unknown_key(key: &str) -> String {
    format!("unknown preference {key:?}; the keys are {}", KEYS.join(", "))
}

/// Reset the preferences: move the settings file at `path` aside (to `settings.txt.reset-1`,
/// `-2`, … in the same folder, never over an earlier one) so the next load gives the defaults.
/// Returns where it went, or `None` when there was no file.
pub fn reset_aside(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "settings.txt".into());
    for n in 1..10_000u32 {
        let aside = path.with_file_name(format!("{name}.reset-{n}"));
        if !aside.exists() {
            std::fs::rename(path, &aside)?;
            return Ok(Some(aside));
        }
    }
    Err(io::Error::other("too many earlier reset copies"))
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
    // Create the probe exclusively (O_EXCL: never follows a symlink, never truncates) under a
    // random name; if the name is taken, try another.
    for _ in 0..16 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let probe = path.join(format!(".wavcraft-write-test-{}-{nanos}-{}", std::process::id(), NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(mut file) => {
                let written = io::Write::write_all(&mut file, b"x");
                drop(file);
                let _ = std::fs::remove_file(&probe);
                return written.map_err(|e| format!("it can't be written to ({e})"));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("it can't be written to ({e})")),
        }
    }
    Err("it can't be written to (no unused probe file name found)".into())
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
    pub fn describe(self) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::{Settings, check_folder};
    use std::path::PathBuf;

    fn unique_folder(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("wavcraft-q371-{tag}-{}-{nanos}", std::process::id()))
    }

    #[cfg(unix)]
    #[test]
    fn q371_symlink_at_old_probe_name_keeps_its_target() -> Result<(), String> {
        let folder = unique_folder("symlink");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let victim = folder.join("recording.wav");
        std::fs::write(&victim, b"KEEP-ME-0123456789").map_err(|e| e.to_string())?;
        let probe = folder.join(format!(".wavcraft-write-test-{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &probe).map_err(|e| e.to_string())?;

        let meta = std::fs::symlink_metadata(&probe).map_err(|e| e.to_string())?;
        assert!(meta.file_type().is_symlink(), "FIXTURE CHECK: probe path is not a symlink");
        let target = std::fs::read_link(&probe).map_err(|e| e.to_string())?;
        assert_eq!(target, victim, "FIXTURE CHECK: symlink does not point to the victim");
        let before = std::fs::read(&victim).map_err(|e| e.to_string())?;
        assert_eq!(before, b"KEEP-ME-0123456789".to_vec(), "FIXTURE CHECK: victim does not hold the 18 bytes");

        assert_eq!(check_folder(&folder), Ok(()));
        let after = std::fs::read(&victim).map_err(|e| e.to_string())?;
        assert_eq!(after, b"KEEP-ME-0123456789".to_vec());
        let _ = std::fs::remove_dir_all(&folder);
        Ok(())
    }

    #[test]
    fn q371_regular_file_at_old_probe_name_is_not_overwritten() -> Result<(), String> {
        let folder = unique_folder("regular");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let file = folder.join(format!(".wavcraft-write-test-{}", std::process::id()));
        std::fs::write(&file, b"KEEP-ME-0123456789").map_err(|e| e.to_string())?;
        let before = std::fs::read(&file).map_err(|e| e.to_string())?;
        assert_eq!(before, b"KEEP-ME-0123456789".to_vec(), "FIXTURE CHECK: file does not hold the 18 bytes");

        assert_eq!(check_folder(&folder), Ok(()));
        let after = std::fs::read(&file).map_err(|e| e.to_string())?;
        assert_eq!(after, b"KEEP-ME-0123456789".to_vec());
        let _ = std::fs::remove_dir_all(&folder);
        Ok(())
    }

    #[test]
    fn q371_empty_folder_passes_and_leaves_no_probe() -> Result<(), String> {
        let folder = unique_folder("empty");
        assert_eq!(check_folder(&folder), Ok(()));
        let mut count = 0;
        for entry in std::fs::read_dir(&folder).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name().to_string_lossy().starts_with(".wavcraft-write-test-") {
                count += 1;
            }
        }
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(&folder);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn q431_symlink_at_predicted_temp_name_keeps_its_target() -> Result<(), String> {
        let folder = unique_folder("q431-symlink");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let victim = folder.join("recording.wav");
        std::fs::write(&victim, b"KEEP-ME-0123456789").map_err(|e| e.to_string())?;
        let before = std::fs::read(&victim).map_err(|e| e.to_string())?;
        assert_eq!(before, b"KEEP-ME-0123456789".to_vec(), "FIXTURE CHECK: victim does not hold the 18 bytes");

        let decoy = folder.join("settings.txt.decoy.tmp");
        std::os::unix::fs::symlink(&victim, &decoy).map_err(|e| e.to_string())?;
        let second = folder.join("settings.txt.retry.tmp");
        let mut calls = 0;
        let settings = Settings { session_folder: Some(PathBuf::from("/w-new")), ..Default::default() };
        let saved = settings.save_with_names(&folder.join("settings.txt"), || {
            calls += 1;
            if calls == 1 { "settings.txt.decoy.tmp".into() } else { "settings.txt.retry.tmp".into() }
        });
        assert!(saved.is_ok(), "save failed: {saved:?}");
        assert_eq!(calls, 2);
        let after = std::fs::read(&victim).map_err(|e| e.to_string())?;
        assert_eq!(after, b"KEEP-ME-0123456789".to_vec());
        assert_eq!(std::fs::read_link(&decoy).map_err(|e| e.to_string())?, victim);
        assert_eq!(
            std::fs::read_to_string(folder.join("settings.txt")).map_err(|e| e.to_string())?,
            "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = /w-new\n"
        );
        assert!(!second.exists());
        let _ = std::fs::remove_dir_all(&folder);
        Ok(())
    }

    #[test]
    fn q431_regular_file_at_predicted_temp_name_is_not_overwritten() -> Result<(), String> {
        let folder = unique_folder("q431-regular");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let decoy = folder.join("settings.txt.decoy.tmp");
        std::fs::write(&decoy, b"KEEP-ME-0123456789").map_err(|e| e.to_string())?;
        let second = folder.join("settings.txt.retry.tmp");
        let mut calls = 0;
        let settings = Settings { session_folder: Some(PathBuf::from("/w-new")), ..Default::default() };
        let saved = settings.save_with_names(&folder.join("settings.txt"), || {
            calls += 1;
            if calls == 1 { "settings.txt.decoy.tmp".into() } else { "settings.txt.retry.tmp".into() }
        });
        assert!(saved.is_ok(), "save failed: {saved:?}");
        assert_eq!(calls, 2);
        assert_eq!(std::fs::read(&decoy).map_err(|e| e.to_string())?, b"KEEP-ME-0123456789");
        let text = std::fs::read_to_string(folder.join("settings.txt")).map_err(|e| e.to_string())?;
        assert_eq!(
            text,
            "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = /w-new\n"
        );
        assert!(!second.exists());
        let _ = std::fs::remove_dir_all(&folder);
        Ok(())
    }

    #[test]
    fn empty_hidden_panels_keep_the_old_file_format() {
        let settings = Settings { session_folder: Some(PathBuf::from("/w-new")), hidden_panels: Vec::new(), ..Default::default() };
        assert_eq!(
            settings.to_text(),
            "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = /w-new\n"
        );
    }

    #[test]
    fn hidden_panels_are_written_and_read_back() {
        let settings = Settings {
            session_folder: Some(PathBuf::from("/w-new")),
            hidden_panels: vec!["history".to_string(), "waveform".to_string()],
            ..Default::default()
        };
        let text = settings.to_text();
        assert_eq!(
            text,
            "# WavCraft settings. One `key = value` per line.\n# session_folder: where opened files are kept while you work on them (empty: the default).\nsession_folder = /w-new\n# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\nhidden_panels = history,waveform\n"
        );
        assert_eq!(Settings::parse(&text), settings);
    }
}
