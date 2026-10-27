//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds `session_folder`, `hidden_panels`, `panel_layout`, `zone_sizes` and `workspace`, and the preferences of the Preferences dialog (see
//! [`KEYS`]; `input_device` is one of them); a file from an older build, with fewer keys, loads
//! with the defaults for the rest, and an older build reading a newer file ignores the keys it
//! doesn't know.
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

use crate::appearance::{self, Colors, Preset};
use crate::chanmap::ChannelMap;
use crate::snap::SnapSettings;
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
const KEY_SNAP: &str = "snap";
const KEY_SNAP_ZERO: &str = "snap_zero_crossings";
const KEY_SNAP_COARSE: &str = "snap_ruler_coarse";
const KEY_SNAP_FINE: &str = "snap_ruler_fine";
const KEY_SNAP_EDGES: &str = "snap_edges";
const KEY_RIGHT_CLICK: &str = "right_click";
const KEY_HIDDEN_PANELS: &str = "hidden_panels";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
const KEY_CUT_JOIN: &str = "crossfade_cut_join_ms";
const KEY_EDGE: &str = "crossfade_edge_ms";
const KEY_OUTPUT: &str = "output_device";
const KEY_INPUT: &str = "input_device";
const KEY_HOST: &str = "audio_host";
const KEY_BUFFER: &str = "buffer_frames";
const KEY_OUT_MAP: &str = "output_map";
const KEY_IN_MAP: &str = "input_map";

const KEY_SRC_QUALITY: &str = "src_quality";
const KEY_PLAYBACK_QUALITY: &str = "playback_quality";
const KEY_SAVE_MARKERS: &str = "save_markers";
const KEY_SAVE_METADATA: &str = "save_metadata";
const KEY_SAVE_META: &str = "save_include_metadata";
const KEY_MARKER_PATTERN: &str = "marker_name_pattern";
const KEY_AUTOSCROLL: &str = "autoscroll";
const KEY_JKL_STEPS: &str = "jkl_speed_steps";
const KEY_PREROLL: &str = "preroll_s";
const KEY_POSTROLL: &str = "postroll_s";
const KEY_RECORD_OFFSET: &str = "record_offset_ms";
const KEY_MONITOR_INPUT: &str = "monitor_input";

const KEY_SPEC_WINDOW: &str = "spec_window";
const KEY_SPEC_RESOLUTION: &str = "spec_resolution";
const KEY_SPEC_DB_RANGE: &str = "spec_db_range";
const KEY_SPEC_PLAY_FREQS: &str = "spec_play_selected_freqs";
const KEY_LOUDNESS_TARGET: &str = "loudness_target_lufs";
const KEY_TRUE_PEAK: &str = "true_peak_ceiling_dbtp";
const KEY_ZOOM_FACTOR: &str = "zoom_factor";
const KEY_EXTEND_SELECTION: &str = "extend_selection";
const KEY_CONTEXT_CHANNELS: &str = "context_channel_editing";
const KEY_THEME: &str = "theme";
const KEY_WAVE_COLOUR: &str = "waveform_colour";
const KEY_BRIGHTNESS: &str = "ui_brightness";
const KEY_SPEC_PALETTE: &str = "spec_palette";
const KEY_CONTRAST: &str = "ui_contrast";
/// The settings-file key that holds one appearance preset per line.
const KEY_PRESET: &str = "appearance_preset";
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

/// Every preference key, in the order the file lists them (`hidden_panels` belongs to the Window
/// menu and is not one of them).
pub const KEYS: [&str; 45] = [
    KEY_SESSION_FOLDER,
    KEY_CUT_JOIN,
    KEY_EDGE,
    KEY_OUTPUT,
    KEY_INPUT,
    KEY_SRC_QUALITY,
    KEY_SAVE_MARKERS,
    KEY_SAVE_METADATA,
    KEY_SAVE_META,
    KEY_MARKER_PATTERN,
    KEY_AUTOSCROLL,
    KEY_JKL_STEPS,
    KEY_PREROLL,
    KEY_POSTROLL,
    KEY_RECORD_OFFSET,
    KEY_MONITOR_INPUT,
    KEY_SPEC_WINDOW,
    KEY_SPEC_RESOLUTION,
    KEY_SPEC_DB_RANGE,
    KEY_SPEC_PLAY_FREQS,
    KEY_TIME_DISPLAY,
    KEY_LOUDNESS_TARGET,
    KEY_TRUE_PEAK,
    KEY_ZOOM_FACTOR,
    KEY_EXTEND_SELECTION,
    KEY_CONTEXT_CHANNELS,
    KEY_THEME,
    KEY_WAVE_COLOUR,
    KEY_BRIGHTNESS,
    "color.waveform",
    "color.selection",
    "color.cursor",
    "color.playhead",
    "color.background",
    KEY_SPEC_PALETTE,
    KEY_CONTRAST,
    KEY_AUTOSAVE,
    KEY_INTERVAL,
    KEY_KEEP_DAYS,
    KEY_CACHE_MB,
    KEY_PLAYBACK_QUALITY,
    KEY_HOST,
    KEY_BUFFER,
    KEY_OUT_MAP,
    KEY_IN_MAP,
];

/// Spectral display resolutions (FFT lengths in frames) the page offers, and the default.
pub const SPEC_RESOLUTIONS: [u32; 6] = [256, 512, 1024, 2048, 4096, 8192];
pub const DEFAULT_SPEC_RESOLUTION: u32 = 2048;
/// The spectral display's dB range (top is 0 dBFS): whole dB, limits and default.
pub const MIN_SPEC_DB_RANGE: u32 = 40;
pub const MAX_SPEC_DB_RANGE: u32 = 160;
pub const DEFAULT_SPEC_DB_RANGE: u32 = 100;
/// Default loudness target (LUFS) and true-peak ceiling (dBTP), and their limits.
pub const DEFAULT_LOUDNESS_TARGET_LUFS: f32 = -16.0;
pub const MIN_LOUDNESS_TARGET_LUFS: f32 = -70.0;
pub const MAX_LOUDNESS_TARGET_LUFS: f32 = 0.0;
pub const DEFAULT_TRUE_PEAK_DBTP: f32 = -1.0;
pub const MIN_TRUE_PEAK_DBTP: f32 = -20.0;
pub const MAX_TRUE_PEAK_DBTP: f32 = 0.0;
/// Zoom In/Out multiplies the view by this much: limits and default.
pub const MIN_ZOOM_FACTOR: f32 = 1.1;
pub const MAX_ZOOM_FACTOR: f32 = 8.0;
pub const DEFAULT_ZOOM_FACTOR: f32 = 2.0;
/// Interface brightness (1 is the theme as drawn): limits and default.
pub const MIN_BRIGHTNESS: f32 = 0.5;
pub const MAX_BRIGHTNESS: f32 = 1.5;
pub const DEFAULT_BRIGHTNESS: f32 = 1.0;
/// Interface contrast (1 is the theme as drawn): limits and default.
pub const MIN_CONTRAST: f32 = 0.5;
pub const MAX_CONTRAST: f32 = 1.5;
pub const DEFAULT_CONTRAST: f32 = 1.0;

/// A preference that is one of a few named choices: `ALL`, `name`, `from_name`, `NAMES`.
macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($(#[$vm])* $v),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$v),+];
            /// The choices as the settings file and `PREF` spell them, for messages.
            pub const NAMES: &'static str = concat!($($text, ", "),+);

            pub fn name(self) -> &'static str {
                match self { $($name::$v => $text),+ }
            }

            pub fn from_name(s: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|c| c.name() == s)
            }
        }
    };
}

choice! {
    /// The window function of the spectral display's FFT.
    SpecWindow {
        Hann = "hann",
        Hamming = "hamming",
        Blackman = "blackman",
        BlackmanHarris = "blackman-harris",
        Rectangular = "rectangular",
    }
}

choice! {
    /// Which edge Shift-click moves when it extends a selection.
    ExtendSelection {
        /// The edge nearest the click.
        Nearest = "nearest",
        /// The edge away from the anchor, so the anchor never moves.
        Anchor = "anchor",
    }
}

choice! {
    /// The interface theme.
    Theme {
        /// Follow the operating system (dark when it can't say).
        System = "system",
        Light = "light",
        Dark = "dark",
    }
}

choice! {
    /// The spectrogram's colour ramp (quiet to loud); the stops are in [`appearance`].
    SpecPalette {
        /// Black through violet and orange to pale yellow.
        Ember = "ember",
        /// Black through deep blue and teal to ice white.
        Glacier = "glacier",
        /// Black to white.
        Mono = "mono",
        /// Black through dark green to pale lime.
        Forest = "forest",
        /// Navy through plum and rose to cream.
        Dusk = "dusk",
    }
}

choice! {
    /// The waveform colour, from a small palette.
    WaveColour {
        Green = "green",
        Blue = "blue",
        Amber = "amber",
        Rose = "rose",
        Cyan = "cyan",
        White = "white",
    }
}

impl WaveColour {
    /// The colour as red, green, blue.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            WaveColour::Green => [64, 200, 140],
            WaveColour::Blue => [90, 150, 240],
            WaveColour::Amber => [235, 170, 60],
            WaveColour::Rose => [230, 100, 140],
            WaveColour::Cyan => [70, 200, 215],
            WaveColour::White => [235, 235, 235],
        }
    }
}

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
/// What the buffer size preference shows (and accepts) for "the device's own default".
pub const DEVICE_DEFAULT: &str = "Device default";
/// The largest buffer size (frames) the preference accepts.
pub const MAX_BUFFER_FRAMES: u32 = 65_536;

/// What a device preference shows (and accepts) for "no device chosen: use the system's".
pub const SYSTEM_DEFAULT: &str = "System default";

/// The edit crossfade defaults, in milliseconds (Audition's Preferences > Data).
pub const DEFAULT_CUT_JOIN_MS: u32 = 2;
pub const DEFAULT_EDGE_MS: u32 = 5;
/// The longest crossfade the preferences accept.
pub const MAX_CROSSFADE_MS: u32 = 1000;

pub mod video;
const KEY_TIME_DISPLAY: &str = "time_display";
const KEY_TEMPO: &str = "tempo";
const KEY_CUSTOM_FPS: &str = "custom_fps";
const KEY_PANEL_LAYOUT: &str = "panel_layout";
const KEY_ZONE_SIZES: &str = "zone_sizes";
const KEY_WORKSPACE: &str = "workspace";
const KEY_METADATA_FIELDS: &str = "metadata_fields";
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
const KEY_AMP_SCALE: &str = "amp_scale";
const KEY_ZOOM_PRESETS: &str = "zoom_presets";
const KEY_CHANNEL_LAYOUT: &str = "channel_layout";

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Right-click in the waveform opens a context menu instead of moving the nearest selection
    /// edge to the click (Audition's preference; off is its default).
    pub right_click_menu: bool,
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
    /// View › Time Display.
    pub time_display: crate::view::TimeFormat,
    /// Tempo and time signature per document, keyed by the document's path (Bars and Beats, A101).
    pub tempos: std::collections::BTreeMap<String, crate::tempo::Tempo>,
    /// Where the panels are docked, as `crate::panels::Layout::encode` writes it (empty: default).
    pub panel_layout: String,
    /// The dock zones' sizes, as `Layout::encode` writes them (empty: default).
    pub zone_sizes: String,
    /// The workspace in use (`crate::workspaces`; empty: Default).
    pub workspace: String,
    /// Ids of the metadata fields file-listing panels show (`crate::panels::MetadataField`);
    /// `None`: never chosen, use the defaults.
    pub metadata_fields: Option<Vec<String>>,
    /// Default sample rate conversion quality.
    pub src_quality: SrcQuality,
    /// Sample-rate conversion quality for playback when the file's rate differs from the device's (`playback_quality = fast|high`).
    pub playback_quality: crate::resample::Quality,
    /// Write the markers into a saved file by default.
    pub save_markers: bool,
    /// Write the metadata into a saved file by default.
    pub save_metadata: bool,
    /// "Include markers and other metadata" when saving (A262): the default for each opened file.
    /// Off drops the markers and the INFO/BWF metadata, like Audition; only `fmt ` and `data` remain.
    pub save_include_metadata: bool,
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
    /// Hear the input through the output while recording (C025). Off by default.
    pub monitor_input: bool,
    /// Spectral Displays page: window function, FFT length, dB range (top is 0 dBFS).
    pub spec_window: SpecWindow,
    pub spec_resolution: u32,
    pub spec_db_range: u32,
    /// Placeholder: play only the selected frequencies. Stored only.
    pub spec_play_selected_freqs: bool,
    /// Loudness and Diagnostics page: the default target (LUFS) and true-peak ceiling (dBTP).
    pub loudness_target_lufs: f32,
    pub true_peak_ceiling_dbtp: f32,
    /// General page: how much Zoom In/Out changes the view; the extend-selection rule; whether
    /// an edit follows the channel the pointer is over.
    pub zoom_factor: f32,
    pub extend_selection: ExtendSelection,
    pub context_channel_editing: bool,
    /// Appearance page.
    pub theme: Theme,
    pub waveform_colour: WaveColour,
    pub ui_brightness: f32,
    /// Colour overrides (`color.<item>`), the spectrogram palette, contrast, and the named presets.
    pub colors: Colors,
    pub spec_palette: SpecPalette,
    pub ui_contrast: f32,
    pub presets: Vec<Preset>,
    /// Auto Save writes a journal while the document has unsaved work.
    pub autosave_enabled: bool,
    /// The most often a journal is written, in seconds.
    pub autosave_interval_secs: u64,
    /// "Clean up old sessions" removes a session only when it is older than this many days.
    pub autosave_keep_days: u32,
    /// The block cache's RAM budget in megabytes (`None`: the default).
    pub cache_mb: Option<u64>,
    /// The Markers panel's columns.
    pub marker_columns: MarkerColumns,
    /// The frame rate of the custom time format, 2 to 1000 (A102).
    pub custom_fps: u16,
    /// The audio host (driver API) by name: CoreAudio, WASAPI, ALSA… (`None`: the system's default).
    pub audio_host: Option<String>,
    /// The output buffer size in frames, applied when the stream reopens (`None`: the device's).
    pub buffer_frames: Option<u32>,
    /// Document channel → device output channel (empty: the automatic layout).
    pub output_map: ChannelMap,
    /// Device input channel → record channel (empty: the automatic layout). Kept for recording.
    pub input_map: ChannelMap,
    /// Which things snapping pulls to, and whether it is on (f49; it lasts from run to run).
    pub snap: SnapSettings,
    /// How the amplitude ruler labels its scale.
    pub amp_scale: crate::view::AmpScale,
    /// The five Zoom to Time slots, in seconds (f69).
    pub zoom_presets: crate::view::ZoomPresets,
    /// Whether the waveform shows channels layered in one lane or stacked in lanes.
    pub channel_layout: crate::channels::ChannelLayout,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_folder: None,
            right_click_menu: false,
            hidden_panels: Vec::new(),
            cut_join_ms: DEFAULT_CUT_JOIN_MS,
            edge_ms: DEFAULT_EDGE_MS,
            output_device: None,
            input_device: None,
            time_display: crate::view::TimeFormat::default(),
            tempos: std::collections::BTreeMap::new(),
            panel_layout: String::new(),
            zone_sizes: String::new(),
            workspace: String::new(),
            metadata_fields: None,
            src_quality: SrcQuality::High,
            playback_quality: crate::resample::Quality::default(),
            save_markers: true,
            save_metadata: true,
            save_include_metadata: true,
            marker_name_pattern: DEFAULT_MARKER_PATTERN.to_string(),
            autoscroll: AutoScroll::Pagewise,
            jkl_steps: DEFAULT_JKL_STEPS.to_vec(),
            preroll_s: DEFAULT_ROLL_S,
            postroll_s: DEFAULT_ROLL_S,
            record_offset_ms: 0,
            monitor_input: false,
            spec_window: SpecWindow::Hann,
            spec_resolution: DEFAULT_SPEC_RESOLUTION,
            spec_db_range: DEFAULT_SPEC_DB_RANGE,
            spec_play_selected_freqs: false,
            loudness_target_lufs: DEFAULT_LOUDNESS_TARGET_LUFS,
            true_peak_ceiling_dbtp: DEFAULT_TRUE_PEAK_DBTP,
            zoom_factor: DEFAULT_ZOOM_FACTOR,
            extend_selection: ExtendSelection::Nearest,
            context_channel_editing: true,
            theme: Theme::Dark,
            waveform_colour: WaveColour::Green,
            ui_brightness: DEFAULT_BRIGHTNESS,
            colors: Colors::default(),
            spec_palette: SpecPalette::Ember,
            ui_contrast: DEFAULT_CONTRAST,
            presets: Vec::new(),
            autosave_enabled: true,
            autosave_interval_secs: DEFAULT_INTERVAL_SECS,
            autosave_keep_days: DEFAULT_KEEP_DAYS,
            cache_mb: None,
            marker_columns: MarkerColumns::default(),
            custom_fps: crate::smpte::CUSTOM_DEFAULT_FPS,
            audio_host: None,
            buffer_frames: None,
            output_map: ChannelMap::default(),
            input_map: ChannelMap::default(),
            snap: SnapSettings::default(),
            amp_scale: crate::view::AmpScale::default(),
            zoom_presets: crate::view::ZoomPresets::default(),
            channel_layout: crate::channels::ChannelLayout::default(),
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
            if key.trim() == KEY_PRESET {
                if let Some(p) =
                    Preset::from_value(value, |b| (MIN_BRIGHTNESS..=MAX_BRIGHTNESS).contains(&b), |c| (MIN_CONTRAST..=MAX_CONTRAST).contains(&c))
                {
                    s.store_preset(p);
                }
                continue;
            }
            match key.trim() {
                KEY_HIDDEN_PANELS => s.hidden_panels = value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect(),
                KEY_PANEL_LAYOUT => s.panel_layout = value.to_string(),
                KEY_ZONE_SIZES => s.zone_sizes = value.to_string(),
                KEY_WORKSPACE => s.workspace = value.to_string(),
                // The snap switches (f49): a value that is not a switch leaves the default.
                KEY_SNAP | KEY_SNAP_ZERO | KEY_SNAP_COARSE | KEY_SNAP_FINE | KEY_SNAP_EDGES => {
                    if let Some(on) = parse_flag(value) {
                        match key.trim() {
                            KEY_SNAP => s.snap.enabled = on,
                            KEY_SNAP_ZERO => s.snap.zero_crossings = on,
                            KEY_SNAP_COARSE => s.snap.ruler_coarse = on,
                            KEY_SNAP_FINE => s.snap.ruler_fine = on,
                            _ => s.snap.edges = on,
                        }
                    }
                }
                KEY_ZOOM_PRESETS => s.zoom_presets = crate::view::ZoomPresets::parse(value),
                KEY_MARKER_COLUMNS => s.marker_columns = MarkerColumns::from_list(value),
                KEY_METADATA_FIELDS => {
                    s.metadata_fields = Some(value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect())
                }
                KEY_TEMPO => {
                    // `tempo = <bpm> <n>/<d> <document path>`: the path may hold spaces.
                    let mut it = value.splitn(3, char::is_whitespace);
                    let (bpm, meter, path) = (it.next(), it.next(), it.next().map(str::trim));
                    if let (Some(bpm), Some(meter), Some(path)) = (bpm, meter, path)
                        && let (Ok(bpm), Some((n, d))) = (bpm.parse::<f64>(), crate::tempo::Tempo::parse_meter(meter))
                        && let Some(t) = crate::tempo::Tempo::new(bpm, n, d)
                        && !path.is_empty()
                    {
                        s.tempos.insert(path.to_string(), t);
                    }
                }
                // A value that doesn't fit its key leaves the default, as an unknown key is ignored.
                other => {
                    let _ = s.set(other, value);
                }
            }
        }
        // The custom format carries the rate the file names, wherever the lines came.
        if let crate::view::TimeFormat::Custom(_) = s.time_display {
            s.time_display = crate::view::TimeFormat::Custom(s.custom_fps);
        }
        s
    }

    /// The block cache budget in bytes: the setting, else the default.
    pub fn cache_bytes(&self) -> u64 {
        self.cache_mb.map_or_else(crate::store::default_cache_bytes, |mb| mb << 20)
    }

    /// The crossfade lengths as durations: `(cut_join, edge)`.
    pub fn crossfades(&self) -> (Duration, Duration) {
        (Duration::from_millis(u64::from(self.cut_join_ms)), Duration::from_millis(u64::from(self.edge_ms)))
    }

    /// The tempo kept for the document at `path` (the default, 120 bpm in 4/4, when none).
    pub fn tempo_for(&self, path: &Path) -> crate::tempo::Tempo {
        self.tempos.get(path.to_string_lossy().as_ref()).copied().unwrap_or_default()
    }

    /// Keep `tempo` for the document at `path`.
    pub fn set_tempo_for(&mut self, path: &Path, tempo: crate::tempo::Tempo) {
        self.tempos.insert(path.to_string_lossy().into_owned(), tempo);
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

    /// The spectral display's FFT length and lowest level (dBFS) from the preferences, put into
    /// `params`. The window function is stored only: the analysis uses Hann.
    pub fn apply_to_spectrogram(&self, params: &mut crate::SpectrogramParams) {
        params.fft_size = self.spec_resolution as usize;
        params.floor_db = -(self.spec_db_range as f32);
        params.ceil_db = 0.0;
    }

    /// Default loudness target in LUFS (-16 unless changed).
    pub fn loudness_target(&self) -> f32 {
        self.loudness_target_lufs
    }

    /// Default true-peak ceiling in dBTP (-1 unless changed).
    pub fn true_peak_ceiling(&self) -> f32 {
        self.true_peak_ceiling_dbtp
    }

    /// The waveform colour as red, green, blue.
    pub fn waveform_rgb(&self) -> [u8; 3] {
        self.waveform_colour.rgb()
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
            KEY_TIME_DISPLAY => self.time_display.name().to_string(),
            KEY_AMP_SCALE => self.amp_scale.name().to_string(),
            KEY_CHANNEL_LAYOUT => self.channel_layout.name().to_string(),
            KEY_CUSTOM_FPS => self.custom_fps.to_string(),
            KEY_SRC_QUALITY => self.src_quality.name().to_string(),
            KEY_PLAYBACK_QUALITY => self.playback_quality.as_str().to_string(),
            KEY_SAVE_MARKERS => self.save_markers.to_string(),
            KEY_SAVE_METADATA => self.save_metadata.to_string(),
            KEY_SAVE_META => self.save_include_metadata.to_string(),
            KEY_MARKER_PATTERN => self.marker_name_pattern.clone(),
            KEY_AUTOSCROLL => self.autoscroll.name().to_string(),
            KEY_JKL_STEPS => self.jkl_steps.iter().map(f32::to_string).collect::<Vec<_>>().join(","),
            KEY_PREROLL => self.preroll_s.to_string(),
            KEY_POSTROLL => self.postroll_s.to_string(),
            KEY_RECORD_OFFSET => self.record_offset_ms.to_string(),
            KEY_MONITOR_INPUT => self.monitor_input.to_string(),
            KEY_SPEC_WINDOW => self.spec_window.name().to_string(),
            KEY_SPEC_RESOLUTION => self.spec_resolution.to_string(),
            KEY_SPEC_DB_RANGE => self.spec_db_range.to_string(),
            KEY_SPEC_PLAY_FREQS => self.spec_play_selected_freqs.to_string(),
            KEY_LOUDNESS_TARGET => self.loudness_target_lufs.to_string(),
            KEY_TRUE_PEAK => self.true_peak_ceiling_dbtp.to_string(),
            KEY_ZOOM_FACTOR => self.zoom_factor.to_string(),
            KEY_EXTEND_SELECTION => self.extend_selection.name().to_string(),
            KEY_CONTEXT_CHANNELS => self.context_channel_editing.to_string(),
            KEY_THEME => self.theme.name().to_string(),
            KEY_WAVE_COLOUR => self.waveform_colour.name().to_string(),
            KEY_BRIGHTNESS => self.ui_brightness.to_string(),
            KEY_SPEC_PALETTE => self.spec_palette.name().to_string(),
            KEY_CONTRAST => self.ui_contrast.to_string(),
            KEY_AUTOSAVE => (if self.autosave_enabled { "on" } else { "off" }).to_string(),
            KEY_INTERVAL => self.autosave_interval_secs.to_string(),
            KEY_KEEP_DAYS => self.autosave_keep_days.to_string(),
            KEY_CACHE_MB => self.cache_mb.map_or_else(|| "default".to_string(), |m| m.to_string()),
            _ if key.starts_with("color.") => match self.colors.get(&key[6..]) {
                Some(c) => appearance::colour_text(c),
                None => return Err(unknown_key(key)),
            },
            KEY_HOST => self.audio_host.clone().unwrap_or_else(|| SYSTEM_DEFAULT.to_string()),
            KEY_BUFFER => self.buffer_frames.map_or_else(|| DEVICE_DEFAULT.to_string(), |n| n.to_string()),
            KEY_OUT_MAP => self.output_map.to_string(),
            KEY_IN_MAP => self.input_map.to_string(),
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
            KEY_TIME_DISPLAY => {
                let f = crate::view::TimeFormat::from_name(value).ok_or_else(|| format!("{key} is one of decimal, compact, samples, smpte_24, smpte_25, smpte_2997df, smpte_2997ndf, smpte_30, smpte_5994, custom, cd75, bars_beats"))?;
                self.time_display = if let crate::view::TimeFormat::Custom(_) = f { crate::view::TimeFormat::Custom(self.custom_fps) } else { f };
            }
            KEY_AMP_SCALE => {
                self.amp_scale = crate::view::AmpScale::parse(value).ok_or_else(|| format!("{key} is one of db, percent, samples, normalized"))?;
            }
            KEY_CHANNEL_LAYOUT => {
                self.channel_layout = crate::channels::ChannelLayout::parse(value).ok_or_else(|| format!("{key} is stacked or layered"))?;
            }
            KEY_CUSTOM_FPS => {
                let rate = value.parse::<u32>().ok().and_then(crate::smpte::SmpteRate::custom).ok_or_else(|| format!("{key} is a whole number of frames a second, 2 to 1000"))?;
                self.custom_fps = rate.nominal() as u16;
                if let crate::view::TimeFormat::Custom(_) = self.time_display {
                    self.time_display = crate::view::TimeFormat::Custom(self.custom_fps);
                }
            }
            KEY_SRC_QUALITY => {
                self.src_quality = SrcQuality::from_name(value).ok_or_else(|| format!("{key} is one of low, medium, high, best"))?;
            }
            KEY_PLAYBACK_QUALITY => {
                self.playback_quality = crate::resample::Quality::parse(value).ok_or_else(|| format!("{key} is fast or high"))?;
            }
            KEY_SAVE_MARKERS => self.save_markers = parse_bool(key, value)?,
            KEY_SAVE_METADATA => self.save_metadata = parse_bool(key, value)?,
            KEY_SAVE_META => self.save_include_metadata = parse_on_off(key, value)?,
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
            KEY_MONITOR_INPUT => self.monitor_input = parse_bool(key, value)?,
            KEY_SPEC_WINDOW => self.spec_window = choose(key, value, SpecWindow::from_name, SpecWindow::NAMES)?,
            KEY_SPEC_RESOLUTION => {
                self.spec_resolution = match value.parse::<u32>() {
                    Ok(n) if SPEC_RESOLUTIONS.contains(&n) => n,
                    _ => return Err(format!("{key} is one of {}", SPEC_RESOLUTIONS.map(|n| n.to_string()).join(", "))),
                }
            }
            KEY_SPEC_DB_RANGE => {
                self.spec_db_range = match value.parse::<u32>() {
                    Ok(n) if (MIN_SPEC_DB_RANGE..=MAX_SPEC_DB_RANGE).contains(&n) => n,
                    _ => return Err(format!("{key} is a whole number of dB, {MIN_SPEC_DB_RANGE} to {MAX_SPEC_DB_RANGE}")),
                }
            }
            KEY_SPEC_PLAY_FREQS => self.spec_play_selected_freqs = parse_bool(key, value)?,
            KEY_LOUDNESS_TARGET => {
                self.loudness_target_lufs = parse_range(key, value, MIN_LOUDNESS_TARGET_LUFS, MAX_LOUDNESS_TARGET_LUFS, "LUFS")?;
            }
            KEY_TRUE_PEAK => self.true_peak_ceiling_dbtp = parse_range(key, value, MIN_TRUE_PEAK_DBTP, MAX_TRUE_PEAK_DBTP, "dBTP")?,
            KEY_ZOOM_FACTOR => self.zoom_factor = parse_range(key, value, MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR, "times")?,
            KEY_RIGHT_CLICK => self.right_click_menu = value.eq_ignore_ascii_case("menu"),
            KEY_EXTEND_SELECTION => self.extend_selection = choose(key, value, ExtendSelection::from_name, ExtendSelection::NAMES)?,
            KEY_CONTEXT_CHANNELS => self.context_channel_editing = parse_bool(key, value)?,
            KEY_THEME => self.theme = choose(key, value, Theme::from_name, Theme::NAMES)?,
            KEY_WAVE_COLOUR => self.waveform_colour = choose(key, value, WaveColour::from_name, WaveColour::NAMES)?,
            KEY_BRIGHTNESS => self.ui_brightness = parse_range(key, value, MIN_BRIGHTNESS, MAX_BRIGHTNESS, "(1 is normal)")?,
            KEY_SPEC_PALETTE => self.spec_palette = choose(key, value, SpecPalette::from_name, SpecPalette::NAMES)?,
            KEY_CONTRAST => self.ui_contrast = parse_range(key, value, MIN_CONTRAST, MAX_CONTRAST, "(1 is normal)")?,
            KEY_AUTOSAVE => self.autosave_enabled = parse_on_off(key, value)?,
            KEY_INTERVAL => self.autosave_interval_secs = parse_in(key, value, &INTERVAL_RANGE)?,
            KEY_KEEP_DAYS => self.autosave_keep_days = parse_in(key, value, &KEEP_DAYS_RANGE)?,
            KEY_CACHE_MB => {
                self.cache_mb = if value.is_empty() || value == "default" { None } else { Some(parse_in(key, value, &CACHE_MB_RANGE)?) };
            }
            _ if key.starts_with("color.") => {
                let colour = appearance::parse_colour(key, value)?;
                match self.colors.slot(&key[6..]) {
                    Some(slot) => *slot = colour,
                    None => return Err(unknown_key(key)),
                }
            }
            KEY_HOST => self.audio_host = device(value),
            KEY_BUFFER => {
                self.buffer_frames = match value {
                    "" | "0" | DEVICE_DEFAULT => None,
                    v => match v.parse::<u32>() {
                        Ok(n) if n <= MAX_BUFFER_FRAMES => Some(n),
                        _ => return Err(format!("{key} is a whole number of frames, 1 to {MAX_BUFFER_FRAMES} (0: the device default)")),
                    },
                }
            }
            KEY_OUT_MAP => self.output_map = ChannelMap::parse(value)?,
            KEY_IN_MAP => self.input_map = ChannelMap::parse(value)?,
            _ => return Err(unknown_key(key)),
        }
        Ok(())
    }

    fn store_preset(&mut self, p: Preset) {
        match self.presets.iter_mut().find(|q| q.name == p.name) {
            Some(q) => *q = p,
            None => {
                if self.presets.len() < appearance::MAX_PRESETS {
                    self.presets.push(p);
                }
            }
        }
    }

    /// The saved appearance presets' names, in the order saved.
    pub fn preset_names(&self) -> Vec<String> {
        self.presets.iter().map(|p| p.name.clone()).collect()
    }

    /// Save the current colours, palette, brightness and contrast as preset `name` (replacing one of that name).
    pub fn save_preset(&mut self, name: &str) -> Result<(), String> {
        let name = appearance::check_name(name)?;
        if self.presets.len() >= appearance::MAX_PRESETS && !self.presets.iter().any(|p| p.name == name) {
            return Err(format!("at most {} presets; delete one first", appearance::MAX_PRESETS));
        }
        let p = Preset { name, colors: self.colors, spec_palette: self.spec_palette, brightness: self.ui_brightness, contrast: self.ui_contrast };
        self.store_preset(p);
        Ok(())
    }

    /// Make preset `name` the current appearance.
    pub fn load_preset(&mut self, name: &str) -> Result<(), String> {
        let p = self.presets.iter().find(|p| p.name == name.trim()).ok_or_else(|| format!("no preset named {:?}", name.trim()))?.clone();
        self.colors = p.colors;
        self.spec_palette = p.spec_palette;
        self.ui_brightness = p.brightness;
        self.ui_contrast = p.contrast;
        Ok(())
    }

    /// Delete preset `name`.
    pub fn delete_preset(&mut self, name: &str) -> Result<(), String> {
        let at = self.presets.iter().position(|p| p.name == name.trim()).ok_or_else(|| format!("no preset named {:?}", name.trim()))?;
        self.presets.remove(at);
        Ok(())
    }

    /// The file's text for these settings.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft settings. One `key = value` per line.\n");
        out.push_str("# session_folder: where opened files are kept while you work on them (empty: the default).\n");
        let folder = self.session_folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        out.push_str(&format!("{KEY_SESSION_FOLDER} = {folder}\n"));
        if self.right_click_menu {
            out.push_str("# right_click: `menu` opens a context menu on right-click; left out (the default), a right-click moves the nearest selection edge to the click.\n");
            out.push_str(&format!("{KEY_RIGHT_CLICK} = menu\n"));
        }
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
        if !self.save_include_metadata {
            out.push_str("# save_include_metadata: 0 saves only the audio (no markers or metadata); 1, the default, includes them.\n");
            out.push_str(&format!("{KEY_SAVE_META} = 0\n"));
        }
        if let Some(output_device) = &self.output_device {
            out.push_str(&format!("{KEY_OUTPUT} = {output_device}
"));
        }
        if let Some(input_device) = &self.input_device {
            out.push_str(&format!("{KEY_INPUT} = {input_device}
"));
        }
        if self.time_display != crate::view::TimeFormat::default() {
            out.push_str("# time_display: decimal, compact, samples, smpte_24, smpte_25, smpte_2997df, smpte_2997ndf, smpte_30, smpte_5994, custom, cd75 or bars_beats (View > Time Display).
");
            out.push_str(&format!("{KEY_TIME_DISPLAY} = {}
", self.time_display.name()));
        }
        // Tempo lines are written only for documents that have one (Bars and Beats), so a file without them keeps its old format.
        if !self.tempos.is_empty() {
            out.push_str("# tempo: beats a minute, time signature and document, one line per document (Bars and Beats).\n");
            for (path, t) in &self.tempos {
                out.push_str(&format!("{KEY_TEMPO} = {} {} {path}\n", t.bpm_text(), t.meter_text()));
            }
        }
        // The rate is written only when it isn't the default or a custom format needs it, so a default file stays as it was.
        if self.amp_scale != crate::view::AmpScale::default() {
            out.push_str(&format!("{KEY_AMP_SCALE} = {}
", self.amp_scale.name()));
        }
        if self.zoom_presets != crate::view::ZoomPresets::default() {
            out.push_str("# zoom_presets: the five Zoom to Time slots, in seconds, comma separated.\n");
            out.push_str(&format!("{KEY_ZOOM_PRESETS} = {}\n", self.zoom_presets.to_text()));
        }
        if self.channel_layout != crate::channels::ChannelLayout::default() {
            out.push_str(&format!("{KEY_CHANNEL_LAYOUT} = {}
", self.channel_layout.name()));
        }
        if self.custom_fps != crate::smpte::CUSTOM_DEFAULT_FPS || matches!(self.time_display, crate::view::TimeFormat::Custom(_)) {
            out.push_str("# custom_fps: frames a second of the custom time display, 2 to 1000.\n");
            out.push_str(&format!("{KEY_CUSTOM_FPS} = {}\n", self.custom_fps));
        }
        if !self.panel_layout.is_empty() || !self.zone_sizes.is_empty() {
            out.push_str("# panel_layout: dock zones and tab order, `zone:id,id;zone:id` (a * marks the front tab); zone_sizes: points.\n");
            out.push_str(&format!("{KEY_PANEL_LAYOUT} = {}\n{KEY_ZONE_SIZES} = {}\n", self.panel_layout, self.zone_sizes));
        }
        if !self.workspace.is_empty() {
            out.push_str(&format!("# workspace: the workspace in use (Window › Workspace; empty: Default).\n{KEY_WORKSPACE} = {}\n", self.workspace));
        }
        // Written only when a switch differs from its default, so a default file keeps the old format.
        if self.snap != SnapSettings::default() {
            out.push_str("# snap_*: what snapping pulls to (1 on, 0 off): all, zero crossings, ruler major ticks, ruler minor ticks, selection edges and cursor.
");
            let n = &self.snap;
            for (key, on) in [
                (KEY_SNAP, n.enabled),
                (KEY_SNAP_ZERO, n.zero_crossings),
                (KEY_SNAP_COARSE, n.ruler_coarse),
                (KEY_SNAP_FINE, n.ruler_fine),
                (KEY_SNAP_EDGES, n.edges),
            ] {
                out.push_str(&format!("{key} = {}
", u8::from(on)));
            }
        }
        if self.marker_columns != MarkerColumns::default() {
            out.push_str("# marker_columns: the Markers panel's columns (start, end, duration, type, description).
");
            out.push_str(&format!("{KEY_MARKER_COLUMNS} = {}
", self.marker_columns.to_list()));
        }
        if let Some(fields) = &self.metadata_fields {
            out.push_str("# metadata_fields: what file lists show (Metadata Display), comma separated.\n");
            out.push_str(&format!("{KEY_METADATA_FIELDS} = {}\n", fields.join(",")));
        }
        // Preferences from the Preferences pages: a key is written only when it differs from its
        // default, so a default file keeps the old format (see KEYS).
        let defaults = Settings::default();
        let mut block = String::new();
        for k in KEYS[5..].iter().copied().filter(|k| *k != KEY_TIME_DISPLAY) {
            let v = self.get(k).unwrap_or_default();
            if v != defaults.get(k).unwrap_or_default() {
                block.push_str(&format!("{k} = {v}
"));
            }
        }
        if !block.is_empty() {
            out.push_str("# Preferences pages (non-default values only; the keys are listed in KEYS).
");
            out.push_str(&block);
        }
        for p in &self.presets {
            out.push_str(&format!("{KEY_PRESET} = {}
", p.to_value()));
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

fn parse_bool(key: &str, v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key} is true or false")),
    }
}

/// A snap switch's value: 1/0, on/off, true/false, yes/no. Anything else is no value (the default stays).
fn parse_flag(value: &str) -> Option<bool> {
    parse_on_off("snap", value).ok()
}

/// An on/off preference: `on`/`off` (also `true`/`false`, `yes`/`no`, `1`/`0`).
fn parse_on_off(key: &str, v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        _ => Err(format!("{key} is on or off, not {v:?}")),
    }
}

/// A whole number in `range`, or an error saying what is allowed.
fn parse_in<T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy>(key: &str, v: &str, range: &std::ops::RangeInclusive<T>) -> Result<T, String> {
    let n: T = v.parse().map_err(|_| format!("{key} wants a whole number, not {v:?}"))?;
    if range.contains(&n) { Ok(n) } else { Err(format!("{key} must be from {} to {}", range.start(), range.end())) }
}

fn choose<T>(key: &str, v: &str, from: fn(&str) -> Option<T>, names: &str) -> Result<T, String> {
    from(v).ok_or_else(|| format!("{key} is one of {}", names.trim_end_matches(", ")))
}

fn parse_range(key: &str, v: &str, lo: f32, hi: f32, unit: &str) -> Result<f32, String> {
    match v.parse::<f32>() {
        Ok(n) if n.is_finite() && (lo..=hi).contains(&n) => Ok(n),
        _ => Err(format!("{key} is a number, {lo} to {hi} {unit}")),
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

#[cfg(test)]
mod tempo_tests {
    use super::*;
    use crate::tempo::Tempo;

    #[test]
    fn a_documents_tempo_is_saved_and_read_back() {
        let mut s = Settings::default();
        let doc = Path::new("/tmp/some folder/a song.wav");
        assert_eq!(s.tempo_for(doc), Tempo::default());
        s.set_tempo_for(doc, Tempo::new(93.5, 3, 4).unwrap());
        s.time_display = crate::view::TimeFormat::BarsBeats;
        let back = Settings::parse(&s.to_text());
        assert_eq!(back, s);
        assert_eq!(back.tempo_for(doc), Tempo::new(93.5, 3, 4).unwrap());
        assert_eq!(back.tempo_for(Path::new("/tmp/other.wav")), Tempo::default());
    }

    #[test]
    fn a_bad_tempo_line_is_ignored() {
        let s = Settings::parse("tempo = 5 4/4 /a.wav\ntempo = 120 4/3 /b.wav\ntempo = abc 4/4 /c.wav\ntempo = 120 4/4\ntempo = 100 6/8 /d.wav\n");
        assert_eq!(s.tempos.len(), 1);
        assert_eq!(s.tempo_for(Path::new("/d.wav")), Tempo::new(100.0, 6, 8).unwrap());
    }
}
