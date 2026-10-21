//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds `session_folder` and `hidden_panels`.
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

/// The environment variable that sets the session folder (over the settings file).
pub const ENV_SESSION_DIR: &str = "WAVCRAFT_SESSION_DIR";
/// How a person changes the session folder, for messages.
pub const HOW_TO_CHANGE: &str = "File › Session Folder… in WavCraft, or the WAVCRAFT_SESSION_DIR environment variable";

const KEY_SESSION_FOLDER: &str = "session_folder";
const KEY_HIDDEN_PANELS: &str = "hidden_panels";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// What the settings file holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Ids of the panels the person hid (`crate::panels`); a panel not listed is shown.
    pub hidden_panels: Vec<String>,
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
                KEY_HIDDEN_PANELS => s.hidden_panels = value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect(),
                _ => {}
            }
        }
        s
    }

    /// The file's text for these settings.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft settings. One `key = value` per line.\n");
        out.push_str("# session_folder: where opened files are kept while you work on them (empty: the default).\n");
        let folder = self.session_folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        out.push_str(&format!("{KEY_SESSION_FOLDER} = {folder}\n"));
        out.push_str("# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\n");
        out.push_str(&format!("{KEY_HIDDEN_PANELS} = {}\n", self.hidden_panels.join(",")));
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
        let settings = Settings { session_folder: Some(PathBuf::from("/w-new")) };
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
        let settings = Settings { session_folder: Some(PathBuf::from("/w-new")) };
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
}
