//! The Video preferences (the "Video" page of the Preferences dialog): whether a timecode is drawn
//! over the picture, which corner it sits in, how big it is, and whether scrubbing shows the spot
//! frame. They live in the same `settings.txt` as [`Settings`](super::Settings), as four more
//! `key = value` lines; [`Settings`](super::Settings) ignores keys it doesn't know, and
//! [`VideoPrefs::serialize`] keeps every line that is not one of its own keys. A missing or bad
//! value is never an error on load: that preference keeps its default.

pub const KEY_OVERLAY_TIMECODE: &str = "video_overlay_timecode";
pub const KEY_OVERLAY_CORNER: &str = "video_overlay_corner";
pub const KEY_OVERLAY_SIZE: &str = "video_overlay_size";
pub const KEY_SPOT_FRAME_ON_SCRUB: &str = "video_spot_frame_on_scrub";

/// Every Video preference key, in the order the file lists them.
pub const KEYS: [&str; 4] = [KEY_OVERLAY_TIMECODE, KEY_OVERLAY_CORNER, KEY_OVERLAY_SIZE, KEY_SPOT_FRAME_ON_SCRUB];

/// The overlay text height range, in percent of the picture height.
pub const MIN_OVERLAY_SIZE: u8 = 2;
pub const MAX_OVERLAY_SIZE: u8 = 20;
pub const DEFAULT_OVERLAY_SIZE: u8 = 5;

/// The comment line [`VideoPrefs::to_text`] writes above the four keys.
pub const COMMENT: &str =
    "# video_*: Video preferences (timecode overlay on or off, its corner, its size as a percent of the picture height, spot frame while scrubbing).";

/// The picture corner the timecode overlay sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayCorner {
    TopLeft,
    TopRight,
    #[default]
    BottomLeft,
    BottomRight,
}

impl OverlayCorner {
    /// The word written in the settings file: `top-left`, `top-right`, `bottom-left` or `bottom-right`.
    pub fn as_str(self) -> &'static str {
        match self {
            OverlayCorner::TopLeft => "top-left",
            OverlayCorner::TopRight => "top-right",
            OverlayCorner::BottomLeft => "bottom-left",
            OverlayCorner::BottomRight => "bottom-right",
        }
    }

    /// The corner a word names (any letter case, surrounding spaces ignored); anything else is `None`.
    pub fn from_text(text: &str) -> Option<OverlayCorner> {
        match text.trim().to_ascii_lowercase().as_str() {
            "top-left" => Some(OverlayCorner::TopLeft),
            "top-right" => Some(OverlayCorner::TopRight),
            "bottom-left" => Some(OverlayCorner::BottomLeft),
            "bottom-right" => Some(OverlayCorner::BottomRight),
            _ => None,
        }
    }
}

/// Parse `true` or `false` (any letter case, surrounding spaces ignored); anything else is `None`.
fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// The Video preferences. The fields are private so every value was checked: change them only
/// through [`VideoPrefs::set`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPrefs {
    overlay_timecode: bool,
    overlay_corner: OverlayCorner,
    overlay_size: u8,
    spot_frame_on_scrub: bool,
}

impl Default for VideoPrefs {
    fn default() -> Self {
        VideoPrefs {
            overlay_timecode: false,
            overlay_corner: OverlayCorner::BottomLeft,
            overlay_size: DEFAULT_OVERLAY_SIZE,
            spot_frame_on_scrub: true,
        }
    }
}

impl VideoPrefs {
    /// Whether the timecode is drawn over the picture.
    pub fn overlay_timecode(&self) -> bool {
        self.overlay_timecode
    }

    /// The corner the timecode sits in.
    pub fn overlay_corner(&self) -> OverlayCorner {
        self.overlay_corner
    }

    /// The timecode text height, in percent of the picture height.
    pub fn overlay_size_percent(&self) -> u8 {
        self.overlay_size
    }

    /// Whether scrubbing shows the spot frame.
    pub fn spot_frame_on_scrub(&self) -> bool {
        self.spot_frame_on_scrub
    }

    /// The value of preference `key` as text.
    pub fn get(&self, key: &str) -> Result<String, String> {
        Ok(match key {
            KEY_OVERLAY_TIMECODE => self.overlay_timecode.to_string(),
            KEY_OVERLAY_CORNER => self.overlay_corner.as_str().to_string(),
            KEY_OVERLAY_SIZE => self.overlay_size.to_string(),
            KEY_SPOT_FRAME_ON_SCRUB => self.spot_frame_on_scrub.to_string(),
            _ => return Err(format!("unknown video preference {key:?}; the keys are {}", KEYS.join(", "))),
        })
    }

    /// Set preference `key` from text. A value that doesn't fit, or an unknown key, is an `Err`
    /// and changes nothing.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        match key {
            KEY_OVERLAY_TIMECODE => match parse_bool(value) {
                Some(b) => self.overlay_timecode = b,
                None => return Err(format!("{key} is true or false")),
            },
            KEY_SPOT_FRAME_ON_SCRUB => match parse_bool(value) {
                Some(b) => self.spot_frame_on_scrub = b,
                None => return Err(format!("{key} is true or false")),
            },
            KEY_OVERLAY_CORNER => match OverlayCorner::from_text(value) {
                Some(c) => self.overlay_corner = c,
                None => return Err(format!("{key} is top-left, top-right, bottom-left or bottom-right")),
            },
            KEY_OVERLAY_SIZE => match value.parse::<u8>() {
                Ok(n) if (MIN_OVERLAY_SIZE..=MAX_OVERLAY_SIZE).contains(&n) => self.overlay_size = n,
                _ => return Err(format!("{key} is a whole number of percent of the picture height, {MIN_OVERLAY_SIZE} to {MAX_OVERLAY_SIZE}")),
            },
            _ => return Err(format!("unknown video preference {key:?}; the keys are {}", KEYS.join(", "))),
        }
        Ok(())
    }

    /// Preferences from the settings file's text: lines for other keys are ignored, and a
    /// missing or bad value leaves that preference at its default.
    pub fn load_from(text: &str) -> VideoPrefs {
        let mut p = VideoPrefs::default();
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

    /// The comment line and the four `key = value` lines, each ending in a newline.
    pub fn to_text(&self) -> String {
        format!(
            "{}\nvideo_overlay_timecode = {}\nvideo_overlay_corner = {}\nvideo_overlay_size = {}\nvideo_spot_frame_on_scrub = {}\n",
            COMMENT,
            self.overlay_timecode,
            self.overlay_corner.as_str(),
            self.overlay_size,
            self.spot_frame_on_scrub
        )
    }

    /// `existing` (a settings file's text) with these preferences in it: every line that is not
    /// one of ours is kept exactly as it was, in order; ours are written once, at the end.
    pub fn serialize(&self, existing: &str) -> String {
        let mut out = String::new();
        for segment in existing.split_inclusive('\n') {
            let t = segment.trim();
            let ours = !t.starts_with('#') && t.split_once('=').is_some_and(|(k, _)| KEYS.contains(&k.trim()));
            if t == COMMENT || ours {
                continue;
            }
            out.push_str(segment); // other lines are kept byte for byte, line ending included
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&self.to_text());
        out
    }
}
