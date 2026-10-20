//! f94: the Appearance page's colour model: `#rrggbb` parsing, the five colour items that can be
//! overridden, the spectrogram palettes (our own stops) and named presets.

/// The colour items the Appearance page can override (`color.<item>`).
pub const ITEMS: [&str; 5] = ["waveform", "selection", "cursor", "playhead", "background"];

/// `"#rrggbb"` (either case, surrounding blanks ignored) as red, green, blue.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let part = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

/// `[r, g, b]` as `"#rrggbb"`.
pub fn format_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// The overridable colours; `None` is the theme's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors {
    pub waveform: Option<[u8; 3]>,
    pub selection: Option<[u8; 3]>,
    pub cursor: Option<[u8; 3]>,
    pub playhead: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
}

impl Colors {
    pub fn slot(&mut self, item: &str) -> Option<&mut Option<[u8; 3]>> {
        Some(match item {
            "waveform" => &mut self.waveform,
            "selection" => &mut self.selection,
            "cursor" => &mut self.cursor,
            "playhead" => &mut self.playhead,
            "background" => &mut self.background,
            _ => return None,
        })
    }

    pub fn get(&self, item: &str) -> Option<Option<[u8; 3]>> {
        let mut copy = *self;
        copy.slot(item).map(|s| *s)
    }
}

/// A colour setting as text: `#rrggbb` or `default`.
pub fn colour_text(c: Option<[u8; 3]>) -> String {
    c.map_or_else(|| "default".to_string(), format_hex)
}

/// A colour setting from text.
pub fn parse_colour(key: &str, v: &str) -> Result<Option<[u8; 3]>, String> {
    if v == "default" {
        return Ok(None);
    }
    parse_hex(v).map(Some).ok_or_else(|| format!("{key} is #rrggbb (six hex digits), or default"))
}

use crate::settings::SpecPalette;

impl SpecPalette {
    /// The ramp as `(position 0..=1, [r, g, b])` stops.
    pub fn stops(self) -> &'static [(f32, [f32; 3])] {
        match self {
            SpecPalette::Ember => &[
                (0.0, [0.0, 0.0, 0.0]),
                (0.3, [20.0, 12.0, 80.0]),
                (0.55, [140.0, 30.0, 120.0]),
                (0.75, [235.0, 90.0, 40.0]),
                (0.9, [250.0, 200.0, 60.0]),
                (1.0, [255.0, 250.0, 215.0]),
            ],
            SpecPalette::Glacier => &[(0.0, [0.0, 0.0, 0.0]), (0.35, [10.0, 40.0, 110.0]), (0.7, [30.0, 150.0, 170.0]), (1.0, [235.0, 250.0, 255.0])],
            SpecPalette::Mono => &[(0.0, [0.0, 0.0, 0.0]), (1.0, [255.0, 255.0, 255.0])],
            SpecPalette::Forest => &[(0.0, [0.0, 0.0, 0.0]), (0.4, [12.0, 70.0, 40.0]), (0.75, [90.0, 170.0, 60.0]), (1.0, [235.0, 255.0, 190.0])],
            SpecPalette::Dusk => &[(0.0, [10.0, 12.0, 40.0]), (0.4, [80.0, 40.0, 110.0]), (0.75, [215.0, 90.0, 120.0]), (1.0, [255.0, 235.0, 205.0])],
        }
    }
}

/// The longest preset name, in characters.
pub const MAX_PRESET_NAME_CHARS: usize = 32;
/// The most presets kept.
pub const MAX_PRESETS: usize = 32;

/// A named set of appearance settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub colors: Colors,
    pub spec_palette: SpecPalette,
    pub brightness: f32,
    pub contrast: f32,
}

/// Check a preset name: 1 to 32 characters, no `;`, `=` or control characters.
pub fn check_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_PRESET_NAME_CHARS || n.chars().any(|c| c.is_control() || c == ';' || c == '=') {
        return Err(format!("a preset name is 1 to {MAX_PRESET_NAME_CHARS} characters, without ; or ="));
    }
    Ok(n.to_string())
}

impl Preset {
    /// The settings-file value: `name;color.waveform=#..;...;spec_palette=..;ui_brightness=..;ui_contrast=..`.
    pub fn to_value(&self) -> String {
        let mut out = self.name.clone();
        let mut c = self.colors;
        for item in ITEMS {
            let v = c.slot(item).map(|s| *s).unwrap_or_default();
            out.push_str(&format!(";color.{item}={}", colour_text(v)));
        }
        out.push_str(&format!(";spec_palette={};ui_brightness={};ui_contrast={}", self.spec_palette.name(), self.brightness, self.contrast));
        out
    }

    /// A preset from its file value; `None` when anything in it is wrong.
    pub fn from_value(value: &str, brightness_ok: impl Fn(f32) -> bool, contrast_ok: impl Fn(f32) -> bool) -> Option<Preset> {
        let mut parts = value.split(';');
        let name = check_name(parts.next()?).ok()?;
        let mut p = Preset { name, colors: Colors::default(), spec_palette: SpecPalette::Ember, brightness: 1.0, contrast: 1.0 };
        for part in parts {
            let (k, v) = part.split_once('=')?;
            let v = v.trim();
            match k.trim() {
                "spec_palette" => p.spec_palette = SpecPalette::from_name(v)?,
                "ui_brightness" => p.brightness = v.parse().ok().filter(|b| brightness_ok(*b))?,
                "ui_contrast" => p.contrast = v.parse().ok().filter(|c| contrast_ok(*c))?,
                other => {
                    let item = other.strip_prefix("color.")?;
                    *p.colors.slot(item)? = parse_colour(other, v).ok()?;
                }
            }
        }
        Some(p)
    }
}
