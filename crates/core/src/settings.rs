//! The app's few lasting settings, and where the session folder is.
//!
//! Settings live in a small text file, `settings.txt` in the system's config folder under
//! `wavcraft` (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on a
//! Mac, `~/.config/wavcraft` on Linux): one `key = value` per line, `#` for comments. Today it
//! holds the session folder and the preferences of the Preferences dialog (see [`KEYS`]); a file
//! from an older build, with fewer keys, loads with the defaults for the rest, and an older build
//! reading a newer file ignores the keys it doesn't know.
//!
//! The **session folder** is where opening a file writes its session store (one subfolder per
//! open file). The first of these that is set wins: the `--session-dir` command-line flag, the
//! [`ENV_SESSION_DIR`] environment variable, the settings file, the default
//! ([`default_sessions_root`](crate::store::default_sessions_root)). A folder that can't be made
//! or written falls back to the default, with a message saying why.

use crate::appearance::{self, Colors, Preset};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The environment variable that sets the session folder (over the settings file).
pub const ENV_SESSION_DIR: &str = "WAVCRAFT_SESSION_DIR";
/// How a person changes the session folder, for messages.
pub const HOW_TO_CHANGE: &str = "File › Session Folder… in WavCraft, or the WAVCRAFT_SESSION_DIR environment variable";

const KEY_SESSION_FOLDER: &str = "session_folder";
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

const KEY_SPEC_WINDOW: &str = "spec_window";
const KEY_SPEC_RESOLUTION: &str = "spec_resolution";
const KEY_SPEC_DB_RANGE: &str = "spec_db_range";
const KEY_SPEC_PLAY_FREQS: &str = "spec_play_selected_freqs";
const KEY_TIME_FORMAT: &str = "time_format";
const KEY_FRAME_RATE: &str = "frame_rate";
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

/// Every preference key, in the order the file lists them.
pub const KEYS: [&str; 35] = [
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
    KEY_SPEC_WINDOW,
    KEY_SPEC_RESOLUTION,
    KEY_SPEC_DB_RANGE,
    KEY_SPEC_PLAY_FREQS,
    KEY_TIME_FORMAT,
    KEY_FRAME_RATE,
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
];

/// Spectral display resolutions (FFT lengths in frames) the page offers, and the default.
pub const SPEC_RESOLUTIONS: [u32; 6] = [256, 512, 1024, 2048, 4096, 8192];
pub const DEFAULT_SPEC_RESOLUTION: u32 = 2048;
/// The spectral display's dB range (top is 0 dBFS): whole dB, limits and default.
pub const MIN_SPEC_DB_RANGE: u32 = 40;
pub const MAX_SPEC_DB_RANGE: u32 = 160;
pub const DEFAULT_SPEC_DB_RANGE: u32 = 100;
/// Frame rate limits (frames a second) and default.
pub const MIN_FRAME_RATE: f32 = 1.0;
pub const MAX_FRAME_RATE: f32 = 240.0;
pub const DEFAULT_FRAME_RATE: f32 = 30.0;
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
    /// How times are written by default.
    TimeFormat {
        /// Minutes, seconds and thousandths.
        Decimal = "decimal",
        /// Whole sample frames.
        Samples = "samples",
        /// Compact disc frames, 75 a second.
        Cd75 = "cd-75",
        /// Hours, minutes, seconds and frames at the frame rate preference.
        Smpte = "smpte",
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

/// What a device preference shows (and accepts) for "no device chosen: use the system's".
pub const SYSTEM_DEFAULT: &str = "System default";

/// The edit crossfade defaults, in milliseconds (Audition's Preferences > Data).
pub const DEFAULT_CUT_JOIN_MS: u32 = 2;
pub const DEFAULT_EDGE_MS: u32 = 5;
/// The longest crossfade the preferences accept.
pub const MAX_CROSSFADE_MS: u32 = 1000;

/// What the settings file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Where session stores go (`None`: the default).
    pub session_folder: Option<PathBuf>,
    /// Crossfade where Delete or Cut closes a gap, in ms (0: none).
    pub cut_join_ms: u32,
    /// Crossfade at every other edit boundary, in ms (0: none).
    pub edge_ms: u32,
    /// The output device's name (`None`: the system default).
    pub output_device: Option<String>,
    /// The input device's name (`None`: the system default). Kept for recording.
    pub input_device: Option<String>,
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
    /// Spectral Displays page: window function, FFT length, dB range (top is 0 dBFS).
    pub spec_window: SpecWindow,
    pub spec_resolution: u32,
    pub spec_db_range: u32,
    /// Placeholder: play only the selected frequencies. Stored only.
    pub spec_play_selected_freqs: bool,
    /// Time Display page: the default format and the frame rate that SMPTE uses.
    pub time_format: TimeFormat,
    pub frame_rate: f32,
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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_folder: None,
            cut_join_ms: DEFAULT_CUT_JOIN_MS,
            edge_ms: DEFAULT_EDGE_MS,
            output_device: None,
            input_device: None,
            src_quality: SrcQuality::High,
            save_markers: true,
            save_metadata: true,
            marker_name_pattern: DEFAULT_MARKER_PATTERN.to_string(),
            autoscroll: AutoScroll::Pagewise,
            jkl_steps: DEFAULT_JKL_STEPS.to_vec(),
            preroll_s: DEFAULT_ROLL_S,
            postroll_s: DEFAULT_ROLL_S,
            record_offset_ms: 0,
            spec_window: SpecWindow::Hann,
            spec_resolution: DEFAULT_SPEC_RESOLUTION,
            spec_db_range: DEFAULT_SPEC_DB_RANGE,
            spec_play_selected_freqs: false,
            time_format: TimeFormat::Decimal,
            frame_rate: DEFAULT_FRAME_RATE,
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

    /// The frame rate SMPTE times use.
    pub fn frame_rate(&self) -> f32 {
        self.frame_rate
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
            KEY_SRC_QUALITY => self.src_quality.name().to_string(),
            KEY_SAVE_MARKERS => self.save_markers.to_string(),
            KEY_SAVE_METADATA => self.save_metadata.to_string(),
            KEY_MARKER_PATTERN => self.marker_name_pattern.clone(),
            KEY_AUTOSCROLL => self.autoscroll.name().to_string(),
            KEY_JKL_STEPS => self.jkl_steps.iter().map(f32::to_string).collect::<Vec<_>>().join(","),
            KEY_PREROLL => self.preroll_s.to_string(),
            KEY_POSTROLL => self.postroll_s.to_string(),
            KEY_RECORD_OFFSET => self.record_offset_ms.to_string(),
            KEY_SPEC_WINDOW => self.spec_window.name().to_string(),
            KEY_SPEC_RESOLUTION => self.spec_resolution.to_string(),
            KEY_SPEC_DB_RANGE => self.spec_db_range.to_string(),
            KEY_SPEC_PLAY_FREQS => self.spec_play_selected_freqs.to_string(),
            KEY_TIME_FORMAT => self.time_format.name().to_string(),
            KEY_FRAME_RATE => self.frame_rate.to_string(),
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
            _ if key.starts_with("color.") => match self.colors.get(&key[6..]) {
                Some(c) => appearance::colour_text(c),
                None => return Err(unknown_key(key)),
            },
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
            KEY_TIME_FORMAT => self.time_format = choose(key, value, TimeFormat::from_name, TimeFormat::NAMES)?,
            KEY_FRAME_RATE => self.frame_rate = parse_range(key, value, MIN_FRAME_RATE, MAX_FRAME_RATE, "frames a second")?,
            KEY_LOUDNESS_TARGET => {
                self.loudness_target_lufs = parse_range(key, value, MIN_LOUDNESS_TARGET_LUFS, MAX_LOUDNESS_TARGET_LUFS, "LUFS")?;
            }
            KEY_TRUE_PEAK => self.true_peak_ceiling_dbtp = parse_range(key, value, MIN_TRUE_PEAK_DBTP, MAX_TRUE_PEAK_DBTP, "dBTP")?,
            KEY_ZOOM_FACTOR => self.zoom_factor = parse_range(key, value, MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR, "times")?,
            KEY_EXTEND_SELECTION => self.extend_selection = choose(key, value, ExtendSelection::from_name, ExtendSelection::NAMES)?,
            KEY_CONTEXT_CHANNELS => self.context_channel_editing = parse_bool(key, value)?,
            KEY_THEME => self.theme = choose(key, value, Theme::from_name, Theme::NAMES)?,
            KEY_WAVE_COLOUR => self.waveform_colour = choose(key, value, WaveColour::from_name, WaveColour::NAMES)?,
            KEY_BRIGHTNESS => self.ui_brightness = parse_range(key, value, MIN_BRIGHTNESS, MAX_BRIGHTNESS, "(1 is normal)")?,
            KEY_SPEC_PALETTE => self.spec_palette = choose(key, value, SpecPalette::from_name, SpecPalette::NAMES)?,
            KEY_CONTRAST => self.ui_contrast = parse_range(key, value, MIN_CONTRAST, MAX_CONTRAST, "(1 is normal)")?,
            _ if key.starts_with("color.") => {
                let colour = appearance::parse_colour(key, value)?;
                match self.colors.slot(&key[6..]) {
                    Some(slot) => *slot = colour,
                    None => return Err(unknown_key(key)),
                }
            }
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
        out.push_str("# crossfade_*_ms: edit crossfades in milliseconds (Delete/Cut join, every other boundary); 0 turns one off.\n");
        out.push_str(&format!("{KEY_CUT_JOIN} = {}\n{KEY_EDGE} = {}\n", self.cut_join_ms, self.edge_ms));
        out.push_str("# output_device, input_device: a device name from the Audio Hardware page (empty: the system default).\n");
        out.push_str(&format!("{KEY_OUTPUT} = {}\n", self.output_device.as_deref().unwrap_or_default()));
        out.push_str(&format!("{KEY_INPUT} = {}\n", self.input_device.as_deref().unwrap_or_default()));
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
        out.push_str("# Spectral Displays page: spec_window (");
        out.push_str(SpecWindow::NAMES.trim_end_matches(", "));
        out.push_str("), spec_resolution (FFT length), spec_db_range (dB below 0 dBFS), spec_play_selected_freqs (placeholder).\n");
        out.push_str("# Time Display page: time_format (");
        out.push_str(TimeFormat::NAMES.trim_end_matches(", "));
        out.push_str("), frame_rate (SMPTE frames a second).\n");
        out.push_str("# Loudness and Diagnostics page: default loudness_target_lufs and true_peak_ceiling_dbtp.\n");
        out.push_str("# General page: zoom_factor, extend_selection (nearest, anchor), context_channel_editing (true/false).\n");
        out.push_str("# Appearance page: theme (system, light, dark), waveform_colour (");
        out.push_str(WaveColour::NAMES.trim_end_matches(", "));
        out.push_str("), ui_brightness (0.5 to 1.5).\n");
        out.push_str("# Appearance colours: color.<waveform|selection|cursor|playhead|background> is #rrggbb or default;\n");
        out.push_str("# spec_palette (");
        out.push_str(SpecPalette::NAMES.trim_end_matches(", "));
        out.push_str("); ui_contrast (0.5 to 1.5). Each appearance_preset line is one named preset.\n");
        for k in &KEYS[14..] {
            out.push_str(&format!("{k} = {}\n", self.get(k).unwrap_or_default()));
        }
        for p in &self.presets {
            out.push_str(&format!("{KEY_PRESET} = {}\n", p.to_value()));
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
