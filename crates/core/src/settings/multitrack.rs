//! The Multitrack preferences (the "Multitrack" and "Multitrack Clips" pages of the Preferences
//! dialog): the default track height, the default crossfade length for new clips, and how clips
//! are coloured. They live in the same `settings.txt` as [`Settings`](super::Settings), as three
//! more `key = value` lines; [`Settings`](super::Settings) ignores keys it doesn't know, and
//! [`MultitrackPrefs::save`] keeps every line it doesn't own. A missing or bad value is never an
//! error on load: that preference keeps its default.

use std::io;
use std::path::Path;

pub const KEY_TRACK_HEIGHT: &str = "multitrack_track_height";
pub const KEY_CLIP_CROSSFADE: &str = "multitrack_clip_crossfade_ms";
pub const KEY_CLIP_COLOR: &str = "multitrack_clip_color";

/// Every Multitrack preference key, in the order the file lists them.
pub const KEYS: [&str; 3] = [KEY_TRACK_HEIGHT, KEY_CLIP_CROSSFADE, KEY_CLIP_COLOR];

/// The track height range in points: SoundCraft's Micro (16) to Extreme (440) presets.
pub const MIN_TRACK_HEIGHT: u16 = 16;
pub const MAX_TRACK_HEIGHT: u16 = 440;
/// SoundCraft's Medium preset.
pub const DEFAULT_TRACK_HEIGHT: u16 = 88;
/// A new clip's crossfade, in ms: the same as the edit-boundary crossfade default.
pub const DEFAULT_CLIP_CROSSFADE_MS: u32 = super::DEFAULT_EDGE_MS;
/// The longest crossfade accepted: the same limit as the edit crossfades.
pub const MAX_CLIP_CROSSFADE_MS: u32 = super::MAX_CROSSFADE_MS;

/// The comment line [`MultitrackPrefs::to_text`] writes above the three keys.
pub const COMMENT: &str = "# multitrack_*: Multitrack preferences (track height in points, new-clip crossfade in ms, clip colour: track or clip).";

/// How a clip is coloured in the Multitrack view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipColorMode {
    /// Every clip takes its track's colour.
    #[default]
    Track,
    /// A clip uses its own colour when it has one, else its track's.
    Clip,
}

impl ClipColorMode {
    /// The word written in the settings file.
    pub fn as_str(self) -> &'static str {
        match self {
            ClipColorMode::Track => "track",
            ClipColorMode::Clip => "clip",
        }
    }

    /// The mode a word names (`track` or `clip`, any letter case, surrounding spaces ignored);
    /// anything else is `None`.
    pub fn from_text(text: &str) -> Option<ClipColorMode> {
        match text.trim().to_ascii_lowercase().as_str() {
            "track" => Some(ClipColorMode::Track),
            "clip" => Some(ClipColorMode::Clip),
            _ => None,
        }
    }
}

/// The Multitrack preferences. The fields are private so every value was checked: change them
/// only through [`MultitrackPrefs::set`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultitrackPrefs {
    track_height: u16,
    clip_crossfade_ms: u32,
    clip_color: ClipColorMode,
}

impl Default for MultitrackPrefs {
    fn default() -> Self {
        MultitrackPrefs { track_height: DEFAULT_TRACK_HEIGHT, clip_crossfade_ms: DEFAULT_CLIP_CROSSFADE_MS, clip_color: ClipColorMode::Track }
    }
}

impl MultitrackPrefs {
    /// The default height of a new track, in points.
    pub fn track_height(&self) -> u16 {
        self.track_height
    }

    /// The default crossfade of a new clip, in ms (0: none).
    pub fn clip_crossfade_ms(&self) -> u32 {
        self.clip_crossfade_ms
    }

    /// How clips are coloured.
    pub fn clip_color(&self) -> ClipColorMode {
        self.clip_color
    }

    /// The value of preference `key` as text.
    pub fn get(&self, key: &str) -> Result<String, String> {
        Ok(match key {
            KEY_TRACK_HEIGHT => self.track_height.to_string(),
            KEY_CLIP_CROSSFADE => self.clip_crossfade_ms.to_string(),
            KEY_CLIP_COLOR => self.clip_color.as_str().to_string(),
            _ => return Err(format!("unknown multitrack preference {key:?}; the keys are {}", KEYS.join(", "))),
        })
    }

    /// Set preference `key` from text. A value that doesn't fit, or an unknown key, is an `Err`
    /// and changes nothing.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        match key {
            KEY_TRACK_HEIGHT => {
                let n: u16 = value.parse().map_err(|_| format!("{key} is a whole number of points, {MIN_TRACK_HEIGHT} to {MAX_TRACK_HEIGHT}"))?;
                if (MIN_TRACK_HEIGHT..=MAX_TRACK_HEIGHT).contains(&n) {
                    self.track_height = n;
                } else {
                    return Err(format!("{key} is a whole number of points, {MIN_TRACK_HEIGHT} to {MAX_TRACK_HEIGHT}"));
                }
            }
            KEY_CLIP_CROSSFADE => {
                let n: u32 = value.parse().map_err(|_| format!("{key} is a whole number of milliseconds, 0 to {MAX_CLIP_CROSSFADE_MS}"))?;
                if n <= MAX_CLIP_CROSSFADE_MS {
                    self.clip_crossfade_ms = n;
                } else {
                    return Err(format!("{key} is a whole number of milliseconds, 0 to {MAX_CLIP_CROSSFADE_MS}"));
                }
            }
            KEY_CLIP_COLOR => {
                if let Some(m) = ClipColorMode::from_text(value) {
                    self.clip_color = m;
                } else {
                    return Err(format!("{key} is track or clip"));
                }
            }
            _ => return Err(format!("unknown multitrack preference {key:?}; the keys are {}", KEYS.join(", "))),
        }
        Ok(())
    }

    /// Preferences from the settings file's text: lines for other keys are ignored, and a
    /// missing or bad value leaves that preference at its default.
    pub fn parse(text: &str) -> MultitrackPrefs {
        let mut p = MultitrackPrefs::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let _ = p.set(key.trim(), value.trim());
        }
        p
    }

    /// The comment line and the three `key = value` lines, each ending in a newline.
    pub fn to_text(&self) -> String {
        format!(
            "{COMMENT}\n{KEY_TRACK_HEIGHT} = {}\n{KEY_CLIP_CROSSFADE} = {}\n{KEY_CLIP_COLOR} = {}\n",
            self.track_height,
            self.clip_crossfade_ms,
            self.clip_color.as_str()
        )
    }

    /// `existing` (a settings file's text) with these preferences in it: every line that is not
    /// one of ours is kept as it was, in order; ours are written once, at the end.
    pub fn merge_into(&self, existing: &str) -> String {
        let mut out = String::new();
        for line in existing.lines() {
            let t = line.trim();
            let ours = t.split_once('=').is_some_and(|(k, _)| KEYS.contains(&k.trim()));
            if t == COMMENT || (ours && !t.starts_with('#')) {
                // Skip our lines
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.to_text());
        out
    }

    /// Read `path`. A missing or unreadable file gives the defaults.
    pub fn load(path: &Path) -> MultitrackPrefs {
        match std::fs::read_to_string(path) {
            Ok(text) => MultitrackPrefs::parse(&text),
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("could not read the settings file {}: {e}", path.display());
                }
                MultitrackPrefs::default()
            }
        }
    }

    /// Write these preferences into the settings file at `path` (its folder made if missing),
    /// keeping the other lines the file has, replacing the file whole through a temporary file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let tmp = path.with_extension("txt.new");
        std::fs::write(&tmp, self.merge_into(&existing))?;
        std::fs::rename(&tmp, path)
    }
}
