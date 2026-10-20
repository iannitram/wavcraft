//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds the session folder and the preferences of the Preferences dialog (see [`KEYS`]); a file
//! from an older build, with fewer keys, loads with the defaults for the rest, and an older build
//! reading a newer file ignores the keys it doesn't know.
//! The panel layout, the zone sizes and the workspace in use (`workspace`) are kept too.
//!
//! The **session folder** is where opening a file writes its session store (one subfolder per
//! open file). The first of these that is set wins: the `--session-dir` command-line flag, the
//! [`ENV_SESSION_DIR`] environment variable, the settings file, the default
//! ([`default_sessions_root`](crate::store::default_sessions_root)). A folder that can't be made
//! or written falls back to the default, with a message saying why.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The environment variable that sets the session folder (over the settings file).
pub const ENV_SESSION_DIR: &str = "WAVCRAFT_SESSION_DIR";
/// How a person changes the session folder, for messages.
pub const HOW_TO_CHANGE: &str = "File › Session Folder… in WavCraft, or the WAVCRAFT_SESSION_DIR environment variable";

const KEY_SESSION_FOLDER: &str = "session_folder";
const KEY_HIDDEN_PANELS: &str = "hidden_panels";
const KEY_TIME_DISPLAY: &str = "time_display";
const KEY_RIGHT_CLICK: &str = "right_click";
const KEY_AMP_SCALE: &str = "amp_scale";
const KEY_CUT_JOIN: &str = "crossfade_cut_join_ms";
const KEY_EDGE: &str = "crossfade_edge_ms";
const KEY_OUTPUT: &str = "output_device";
const KEY_INPUT: &str = "input_device";

/// Every preference key, in the order the file lists them.
pub const KEYS: [&str; 5] = [KEY_SESSION_FOLDER, KEY_CUT_JOIN, KEY_EDGE, KEY_OUTPUT, KEY_INPUT];

/// What a device preference shows (and accepts) for "no device chosen: use the system's".
pub const SYSTEM_DEFAULT: &str = "System default";

/// The edit crossfade defaults, in milliseconds (Audition's Preferences > Data).
pub const DEFAULT_CUT_JOIN_MS: u32 = 2;
pub const DEFAULT_EDGE_MS: u32 = 5;
/// The longest crossfade the preferences accept.
pub const MAX_CROSSFADE_MS: u32 = 1000;
const KEY_PANEL_LAYOUT: &str = "panel_layout";
const KEY_ZONE_SIZES: &str = "zone_sizes";
const KEY_WORKSPACE: &str = "workspace";

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Ids of the panels the person hid (`crate::panels`); a panel not listed is shown.
    pub hidden_panels: Vec<String>,
    /// View › Time Display.
    pub time_display: crate::view::TimeFormat,
    /// Right-click in the waveform opens a context menu instead of extending the selection to
    /// the click (Audition's preference; off is its default).
    pub right_click_menu: bool,
    /// How the amplitude ruler labels its scale.
    pub amp_scale: crate::view::AmpScale,
    /// Crossfade where Delete or Cut closes a gap, in ms (0: none).
    pub cut_join_ms: u32,
    /// Crossfade at every other edit boundary, in ms (0: none).
    pub edge_ms: u32,
    /// The output device's name (`None`: the system default).
    pub output_device: Option<String>,
    /// The input device's name (`None`: the system default). Kept for recording.
    pub input_device: Option<String>,
    /// Where the panels are docked, as `crate::panels::Layout::encode` writes it (empty: default).
    pub panel_layout: String,
    /// The dock zones' sizes, as `Layout::encode` writes them (empty: default).
    pub zone_sizes: String,
    /// The workspace in use (`crate::workspaces`; empty: Default).
    pub workspace: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_folder: None,
            hidden_panels: Vec::new(),
            time_display: crate::view::TimeFormat::default(),
            right_click_menu: false,
            amp_scale: crate::view::AmpScale::default(),
            cut_join_ms: DEFAULT_CUT_JOIN_MS,
            edge_ms: DEFAULT_EDGE_MS,
            output_device: None,
            input_device: None,
            panel_layout: String::new(),
            zone_sizes: String::new(),
            workspace: String::new(),
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
                KEY_SESSION_FOLDER => s.session_folder = (!value.is_empty()).then(|| PathBuf::from(value)),
                KEY_HIDDEN_PANELS => s.hidden_panels = value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect(),
                KEY_TIME_DISPLAY => s.time_display = crate::view::TimeFormat::from_name(value).unwrap_or_default(),
                KEY_RIGHT_CLICK => s.right_click_menu = value.eq_ignore_ascii_case("menu"),
                KEY_PANEL_LAYOUT => s.panel_layout = value.to_string(),
                KEY_ZONE_SIZES => s.zone_sizes = value.to_string(),
                KEY_WORKSPACE => s.workspace = value.to_string(),
                _ => {}
            }
            if key.trim() == KEY_SESSION_FOLDER {
                s.session_folder = (!value.is_empty()).then(|| PathBuf::from(value));
            } else if key.trim() == KEY_AMP_SCALE {
                s.amp_scale = crate::view::AmpScale::parse(value).unwrap_or_default();
            }
            // A value that doesn't fit its key leaves the default, as an unknown key is ignored.
            let _ = s.set(key.trim(), value);
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
        out.push_str("# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\n");
        out.push_str(&format!("{KEY_HIDDEN_PANELS} = {}\n", self.hidden_panels.join(",")));
        out.push_str("# time_display: decimal, compact, samples or smpte30 (View > Time Display).\n");
        out.push_str(&format!("{KEY_TIME_DISPLAY} = {}\n", self.time_display.name()));
        out.push_str("# right_click: `extend` moves the nearest selection edge to the click, `menu` opens a context menu.\n");
        out.push_str(&format!("{KEY_RIGHT_CLICK} = {}\n", if self.right_click_menu { "menu" } else { "extend" }));
        out.push_str("# amp_scale: the amplitude ruler's scale: db, percent, samples or normalized.\n");
        out.push_str(&format!("{KEY_AMP_SCALE} = {}\n", self.amp_scale.name()));
        out.push_str("# crossfade_*_ms: edit crossfades in milliseconds (Delete/Cut join, every other boundary); 0 turns one off.\n");
        out.push_str(&format!("{KEY_CUT_JOIN} = {}\n{KEY_EDGE} = {}\n", self.cut_join_ms, self.edge_ms));
        out.push_str("# output_device, input_device: a device name from the Audio Hardware page (empty: the system default).\n");
        out.push_str(&format!("{KEY_OUTPUT} = {}\n", self.output_device.as_deref().unwrap_or_default()));
        out.push_str(&format!("{KEY_INPUT} = {}\n", self.input_device.as_deref().unwrap_or_default()));
        out.push_str("# panel_layout: dock zones and tab order, `zone:id,id;zone:id` (a * marks the front tab); zone_sizes: points.\n");
        out.push_str(&format!("{KEY_PANEL_LAYOUT} = {}\n{KEY_ZONE_SIZES} = {}\n", self.panel_layout, self.zone_sizes));
        out.push_str(&format!("# workspace: the workspace in use (Window › Workspace; empty: Default).\n{KEY_WORKSPACE} = {}\n", self.workspace));
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
