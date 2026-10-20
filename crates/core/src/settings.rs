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

const KEY_SRC_QUALITY: &str = "src_quality";
const KEY_SAVE_MARKERS: &str = "save_markers";
const KEY_SAVE_METADATA: &str = "save_metadata";
const KEY_MARKER_PATTERN: &str = "marker_name_pattern";
const KEY_AUTOSCROLL: &str = "autoscroll";
const KEY_JKL_STEPS: &str = "jkl_speed_steps";
const KEY_PREROLL: &str = "preroll_s";
const KEY_POSTROLL: &str = "postroll_s";
const KEY_RECORD_OFFSET: &str = "record_offset_ms";

/// Every preference key, in the order the file lists them.
pub const KEYS: [&str; 14] = [
    KEY_SESSION_FOLDER,
    KEY_CUT_JOIN,
    KEY_EDGE,
    KEY_OUTPUT,
    KEY_INPUT,
    KEY_SRC_QUALITY,
    KEY_SAVE_MARKERS,
    KEY_SAVE_METADATA,
    KEY_MARKER_PATTERN,
    KEY_AUTOSCROLL,
    KEY_JKL_STEPS,
    KEY_PREROLL,
    KEY_POSTROLL,
    KEY_RECORD_OFFSET,
];

/// The default marker name pattern (`{n}` is the marker's number).
pub const DEFAULT_MARKER_PATTERN: &str = "Marker {n}";
/// The longest marker name pattern accepted, in characters.
pub const MAX_MARKER_PATTERN_CHARS: usize = 64;
/// Playback speeds the J/K/L keys step through, by default.
pub const DEFAULT_JKL_STEPS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];
/// The most speeds, and the slowest and fastest speed, the JKL list accepts.
pub const MAX_JKL_STEPS: usize = 8;
pub const MIN_JKL_SPEED: f32 = 0.05;
pub const MAX_JKL_SPEED: f32 = 16.0;
/// Pre-roll and post-roll defaults and ceiling, in seconds.
pub const DEFAULT_ROLL_S: f32 = 2.0;
pub const MAX_ROLL_S: f32 = 60.0;
/// The record offset (latency compensation) is within plus or minus this many ms.
pub const MAX_RECORD_OFFSET_MS: i32 = 1000;

/// Sample rate conversion quality (the default for conversions that don't ask for one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrcQuality {
    Low,
    Medium,
    High,
    Best,
}

impl SrcQuality {
    pub const ALL: [SrcQuality; 4] = [SrcQuality::Low, SrcQuality::Medium, SrcQuality::High, SrcQuality::Best];

    pub fn name(self) -> &'static str {
        match self {
            SrcQuality::Low => "low",
            SrcQuality::Medium => "medium",
            SrcQuality::High => "high",
            SrcQuality::Best => "best",
        }
    }

    pub fn from_name(s: &str) -> Option<SrcQuality> {
        SrcQuality::ALL.into_iter().find(|q| q.name() == s)
    }
}

/// How the view follows the playhead while playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScroll {
    /// Jump a page when the playhead reaches the edge.
    Pagewise,
    /// Keep the playhead in the middle.
    Centred,
    Off,
}

impl AutoScroll {
    pub const ALL: [AutoScroll; 3] = [AutoScroll::Pagewise, AutoScroll::Centred, AutoScroll::Off];

    pub fn name(self) -> &'static str {
        match self {
            AutoScroll::Pagewise => "pagewise",
            AutoScroll::Centred => "centred",
            AutoScroll::Off => "off",
        }
    }

    pub fn from_name(s: &str) -> Option<AutoScroll> {
        AutoScroll::ALL.into_iter().find(|a| a.name() == s)
    }
}

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
const KEY_MARKER_COLUMNS: &str = "marker_columns";

/// Which columns the Markers panel shows besides the name (A181).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerColumns {
    pub start: bool,
    pub end: bool,
    pub duration: bool,
    pub kind: bool,
    pub description: bool,
}

impl MarkerColumns {
    /// The columns' settings names, in display order.
    pub const NAMES: [&'static str; 5] = ["start", "end", "duration", "type", "description"];

    pub fn get(&self, i: usize) -> bool {
        [self.start, self.end, self.duration, self.kind, self.description].get(i).copied().unwrap_or(false)
    }

    pub fn set(&mut self, i: usize, on: bool) {
        match i {
            0 => self.start = on,
            1 => self.end = on,
            2 => self.duration = on,
            3 => self.kind = on,
            4 => self.description = on,
            _ => {}
        }
    }

    /// `start,end,duration,type` style list of the columns that are on.
    pub fn to_list(&self) -> String {
        (0..Self::NAMES.len()).filter(|&i| self.get(i)).map(|i| Self::NAMES[i]).collect::<Vec<_>>().join(",")
    }

    /// The columns named in `list` (unknown names are ignored).
    pub fn from_list(list: &str) -> MarkerColumns {
        let mut c = MarkerColumns { start: false, end: false, duration: false, kind: false, description: false };
        for name in list.split(',').map(str::trim) {
            if let Some(i) = Self::NAMES.iter().position(|n| *n == name) {
                c.set(i, true);
            }
        }
        c
    }
}

impl Default for MarkerColumns {
    fn default() -> Self {
        MarkerColumns { start: true, end: true, duration: true, kind: true, description: false }
    }
}

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq)]
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
    /// The Markers panel's columns.
    pub marker_columns: MarkerColumns,
    /// Default sample rate conversion quality.
    pub src_quality: SrcQuality,
    /// Write the markers into a saved file by default.
    pub save_markers: bool,
    /// Write the metadata into a saved file by default.
    pub save_metadata: bool,
    /// The name a new marker gets (`{n}`: its number).
    pub marker_name_pattern: String,
    /// How the view follows the playhead.
    pub autoscroll: AutoScroll,
    /// The speeds J/K/L step through, ascending, as a multiple of normal speed.
    pub jkl_steps: Vec<f32>,
    /// Pre-roll and post-roll in seconds.
    pub preroll_s: f32,
    pub postroll_s: f32,
    /// Record offset in ms (positive: later). Stored only; recording applies it when it exists.
    pub record_offset_ms: i32,
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
            marker_columns: MarkerColumns::default(),
            src_quality: SrcQuality::High,
            save_markers: true,
            save_metadata: true,
            marker_name_pattern: DEFAULT_MARKER_PATTERN.to_string(),
            autoscroll: AutoScroll::Pagewise,
            jkl_steps: DEFAULT_JKL_STEPS.to_vec(),
            preroll_s: DEFAULT_ROLL_S,
            postroll_s: DEFAULT_ROLL_S,
            record_offset_ms: 0,
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
                KEY_MARKER_COLUMNS => s.marker_columns = MarkerColumns::from_list(value),
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

    /// The JKL playback speeds, ascending (1 to 8 of them).
    pub fn jkl_speed_steps(&self) -> &[f32] {
        &self.jkl_steps
    }

    /// Pre-roll: play this long before the cursor when previewing an edit point.
    pub fn preroll(&self) -> Duration {
        Duration::from_secs_f32(self.preroll_s)
    }

    /// Post-roll: play this long after the selection's end when previewing.
    pub fn postroll(&self) -> Duration {
        Duration::from_secs_f32(self.postroll_s)
    }

    /// The name for marker number `n` from the pattern (`{n}` replaced; a pattern without it
    /// gets the number appended).
    pub fn marker_name(&self, n: u32) -> String {
        marker_name(&self.marker_name_pattern, n)
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
            KEY_SRC_QUALITY => self.src_quality.name().to_string(),
            KEY_SAVE_MARKERS => self.save_markers.to_string(),
            KEY_SAVE_METADATA => self.save_metadata.to_string(),
            KEY_MARKER_PATTERN => self.marker_name_pattern.clone(),
            KEY_AUTOSCROLL => self.autoscroll.name().to_string(),
            KEY_JKL_STEPS => self.jkl_steps.iter().map(f32::to_string).collect::<Vec<_>>().join(","),
            KEY_PREROLL => self.preroll_s.to_string(),
            KEY_POSTROLL => self.postroll_s.to_string(),
            KEY_RECORD_OFFSET => self.record_offset_ms.to_string(),
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
            KEY_SRC_QUALITY => {
                self.src_quality = SrcQuality::from_name(value).ok_or_else(|| format!("{key} is one of low, medium, high, best"))?;
            }
            KEY_SAVE_MARKERS => self.save_markers = parse_bool(key, value)?,
            KEY_SAVE_METADATA => self.save_metadata = parse_bool(key, value)?,
            KEY_MARKER_PATTERN => {
                let n = value.chars().count();
                if n == 0 || n > MAX_MARKER_PATTERN_CHARS || value.chars().any(char::is_control) {
                    return Err(format!("{key} is 1 to {MAX_MARKER_PATTERN_CHARS} characters on one line, e.g. {DEFAULT_MARKER_PATTERN}"));
                }
                self.marker_name_pattern = value.to_string();
            }
            KEY_AUTOSCROLL => {
                self.autoscroll = AutoScroll::from_name(value).ok_or_else(|| format!("{key} is one of pagewise, centred, off"))?;
            }
            KEY_JKL_STEPS => self.jkl_steps = parse_steps(key, value)?,
            KEY_PREROLL => self.preroll_s = parse_roll(key, value)?,
            KEY_POSTROLL => self.postroll_s = parse_roll(key, value)?,
            KEY_RECORD_OFFSET => {
                self.record_offset_ms = match value.parse::<i32>() {
                    Ok(n) if n.abs() <= MAX_RECORD_OFFSET_MS => n,
                    _ => return Err(format!("{key} is a whole number of milliseconds, -{MAX_RECORD_OFFSET_MS} to {MAX_RECORD_OFFSET_MS}")),
                }
            }
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
        out.push_str("# time_display: decimal, compact, samples, smpte_24, smpte_25, smpte_2997df, smpte_2997ndf, smpte_30 or smpte_5994 (View > Time Display).\n");
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
        out.push_str("# marker_columns: the Markers panel's columns (start, end, duration, type, description).\n");
        out.push_str(&format!("{KEY_MARKER_COLUMNS} = {}\n", self.marker_columns.to_list()));
        out.push_str("# Data page: src_quality (low, medium, high, best) is the default sample rate conversion quality.\n");
        out.push_str(&format!("{KEY_SRC_QUALITY} = {}\n", self.get(KEY_SRC_QUALITY).unwrap_or_default()));
        out.push_str(
            "# Markers and Metadata page: write them into a saved file by default (true/false); the name a new marker gets ({n}: its number).\n",
        );
        for k in [KEY_SAVE_MARKERS, KEY_SAVE_METADATA, KEY_MARKER_PATTERN] {
            out.push_str(&format!("{k} = {}\n", self.get(k).unwrap_or_default()));
        }
        out.push_str("# Playback and Recording page: autoscroll (pagewise, centred, off); jkl_speed_steps (ascending, comma separated);\n");
        out.push_str("# preroll_s/postroll_s in seconds; record_offset_ms (-1000 to 1000).\n");
        for k in [KEY_AUTOSCROLL, KEY_JKL_STEPS, KEY_PREROLL, KEY_POSTROLL, KEY_RECORD_OFFSET] {
            out.push_str(&format!("{k} = {}\n", self.get(k).unwrap_or_default()));
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

fn parse_bool(key: &str, v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key} is true or false")),
    }
}

fn parse_roll(key: &str, v: &str) -> Result<f32, String> {
    match v.parse::<f32>() {
        Ok(n) if n.is_finite() && (0.0..=MAX_ROLL_S).contains(&n) => Ok(n),
        _ => Err(format!("{key} is a number of seconds, 0 to {MAX_ROLL_S}")),
    }
}

fn parse_steps(key: &str, v: &str) -> Result<Vec<f32>, String> {
    let why = || format!("{key} is 1 to {MAX_JKL_STEPS} speeds, {MIN_JKL_SPEED} to {MAX_JKL_SPEED}, slowest first, comma separated");
    let steps: Vec<f32> = v.split(',').map(|p| p.trim().parse::<f32>().map_err(|_| why())).collect::<Result<_, _>>()?;
    let ok = !steps.is_empty()
        && steps.len() <= MAX_JKL_STEPS
        && steps.iter().all(|s| s.is_finite() && (MIN_JKL_SPEED..=MAX_JKL_SPEED).contains(s))
        && steps.windows(2).all(|w| w[0] < w[1]);
    if ok { Ok(steps) } else { Err(why()) }
}

/// Marker name `n` from `pattern`: `{n}` becomes the number; a pattern without it gets " n" appended.
pub fn marker_name(pattern: &str, n: u32) -> String {
    if pattern.contains("{n}") { pattern.replace("{n}", &n.to_string()) } else { format!("{pattern} {n}") }
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
