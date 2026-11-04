//! The one theme (M3): every colour the UI draws comes from here, as a named design value.
//!
//! There is a dark set and a light set of [`Palette`]. `WaveApp::apply_look` picks the set
//! each frame with [`set_dark`]; every panel reads [`pal`] when it paints, so switching between
//! dark and light changes every panel. Outside this file, UI code names a role
//! (`theme::pal().alert`), never a colour value. User-chosen colours (track, marker and wave
//! colours from the settings) go through [`rgb`] and [`rgba`]; the test at the end of this file
//! fails on any other `Color32` constructor or named colour in the UI sources.
//!
//! The fixed values at the bottom (`white`, `black`, `CHANNEL`) are the same in both sets on purpose.

use egui::Color32;
use std::sync::atomic::{AtomicBool, Ordering};

/// One set of design values. Field docs say what the role is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The editor and frequency-panel ground.
    pub background: Color32,
    /// Dark wells: the navigator strip, the amplitude ruler and the timeline lane.
    pub well: Color32,
    /// Ruler lanes and the amplitude ruler column.
    pub ruler: Color32,
    /// An off push button or key cap.
    pub button_off: Color32,
    /// The level meter's track.
    pub meter_track: Color32,
    /// A multitrack track header, and the selected one.
    pub track_header: Color32,
    pub track_header_selected: Color32,
    /// A track header with no track in it.
    pub track_header_idle: Color32,
    /// A multitrack track's lane.
    pub track_lane: Color32,
    /// Grid lines in the frequency and spectral displays.
    pub grid: Color32,
    /// Hairlines: frame edges and the lines under the waveform.
    pub hairline: Color32,
    /// Dividers between lanes and the tick marks of the rulers.
    pub divider: Color32,
    /// Primary text.
    pub text: Color32,
    /// Emphasised text and the meter's peak hold.
    pub text_strong: Color32,
    /// Secondary text, labels and rulers' numbers.
    pub text_dim: Color32,
    /// Faint text and zero labels.
    pub text_faint: Color32,
    /// Pure white, for text and marks on a coloured fill.
    pub white: Color32,
    /// Pure black, for text on a bright fill.
    pub black: Color32,
    /// Errors and clipping.
    pub alert: Color32,
    /// The text cursor and the warning colour of the status line.
    pub cursor: Color32,
    /// Caution: amber meter zone, scan limits and stale results.
    pub caution: Color32,
    /// Good: green meter zone and the input check that passes.
    pub ok: Color32,
    /// A dimmer green for the spectral lane's minimum.
    pub ok_dim: Color32,
    /// The accent: the navigator's view handle and selection strip, links and key outlines.
    pub accent: Color32,
    /// The fill of the navigator's view handle.
    pub accent_fill: Color32,
    /// The fill of a time range (a loop or punch range in the multitrack view).
    pub range_fill: Color32,
    /// The selection in the editor.
    pub selection: Color32,
    /// The playhead line.
    pub playhead: Color32,
    /// The spectral band's fill and outline.
    pub band_fill: Color32,
    pub band_outline: Color32,
    /// Markers, and the default marker colour.
    pub marker: Color32,
}

/// The dark set: the values the app drew before M3, so a dark session looks as it did.
pub static DARK: Palette = Palette {
    background: Color32::from_rgb(18, 20, 24),
    well: Color32::from_gray(24),
    ruler: Color32::from_gray(28),
    button_off: Color32::from_gray(48),
    meter_track: Color32::from_rgb(30, 33, 40),
    track_header: Color32::from_rgb(32, 36, 44),
    track_header_selected: Color32::from_rgb(44, 52, 66),
    track_header_idle: Color32::from_rgb(40, 40, 40),
    track_lane: Color32::from_rgb(14, 16, 20),
    grid: Color32::from_gray(45),
    hairline: Color32::from_gray(55),
    divider: Color32::from_gray(90),
    text: Color32::from_rgb(230, 230, 230),
    text_strong: Color32::from_gray(235),
    text_dim: Color32::from_gray(170),
    text_faint: Color32::from_gray(110),
    white: Color32::WHITE,
    black: Color32::BLACK,
    alert: Color32::from_rgb(235, 70, 60),
    cursor: Color32::from_rgb(235, 80, 70),
    caution: Color32::from_rgb(225, 180, 60),
    ok: Color32::from_rgb(70, 190, 120),
    ok_dim: Color32::from_rgb(40, 110, 80),
    accent: Color32::from_rgb(90, 140, 230),
    accent_fill: Color32::from_rgba_premultiplied(60, 80, 120, 70),
    range_fill: Color32::from_rgba_premultiplied(24, 31, 50, 50),
    selection: Color32::from_rgba_premultiplied(70, 90, 130, 90),
    playhead: Color32::from_rgb(250, 250, 250),
    band_fill: Color32::from_rgba_premultiplied(60, 70, 20, 60),
    band_outline: Color32::from_rgb(240, 220, 90),
    marker: Color32::from_rgb(240, 190, 70),
};

/// The light set: the same roles, with values that read on a light ground.
pub static LIGHT: Palette = Palette {
    background: Color32::from_rgb(246, 247, 249),
    well: Color32::from_rgb(226, 229, 234),
    ruler: Color32::from_rgb(222, 225, 230),
    button_off: Color32::from_rgb(208, 212, 218),
    meter_track: Color32::from_rgb(214, 218, 226),
    track_header: Color32::from_rgb(226, 229, 235),
    track_header_selected: Color32::from_rgb(205, 213, 228),
    track_header_idle: Color32::from_rgb(200, 200, 200),
    track_lane: Color32::from_rgb(236, 238, 242),
    grid: Color32::from_gray(214),
    hairline: Color32::from_gray(200),
    divider: Color32::from_gray(150),
    text: Color32::from_rgb(28, 31, 36),
    text_strong: Color32::from_rgb(10, 10, 12),
    text_dim: Color32::from_gray(100),
    text_faint: Color32::from_gray(140),
    white: Color32::WHITE,
    black: Color32::BLACK,
    alert: Color32::from_rgb(190, 40, 34),
    cursor: Color32::from_rgb(200, 40, 30),
    caution: Color32::from_rgb(150, 100, 0),
    ok: Color32::from_rgb(30, 130, 70),
    ok_dim: Color32::from_rgb(150, 200, 170),
    accent: Color32::from_rgb(40, 100, 210),
    accent_fill: Color32::from_rgba_premultiplied(40, 80, 160, 60),
    range_fill: Color32::from_rgba_premultiplied(6, 16, 33, 40),
    selection: Color32::from_rgba_premultiplied(40, 70, 140, 70),
    playhead: Color32::from_rgb(20, 20, 20),
    band_fill: Color32::from_rgba_premultiplied(40, 60, 20, 50),
    band_outline: Color32::from_rgb(150, 120, 0),
    marker: Color32::from_rgb(170, 110, 0),
};

/// The four channel colours of the frequency display (same in both sets).
pub const CHANNEL: [Color32; 4] =
    [Color32::from_rgb(64, 200, 140), Color32::from_rgb(90, 160, 235), Color32::from_rgb(235, 190, 80), Color32::from_rgb(220, 110, 200)];

/// Whether the dark set is showing. The app sets it every frame.
static DARK_SHOWING: AtomicBool = AtomicBool::new(true);

/// Show the dark set (`true`) or the light set (`false`).
pub fn set_dark(dark: bool) {
    DARK_SHOWING.store(dark, Ordering::Relaxed);
}

/// The set that is showing now. Panels call this when they paint.
pub fn pal() -> &'static Palette {
    if DARK_SHOWING.load(Ordering::Relaxed) { &DARK } else { &LIGHT }
}

/// A colour from user settings (track, marker, wave and ground colours) or computed channels.
pub fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A colour from channel bytes, with alpha (not premultiplied).
pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// `c` with its alpha replaced by `a` (not premultiplied).
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Tests that switch the set showing hold this while they run: the switch is one flag for the whole
/// process and tests run on several threads.
#[cfg(test)]
pub(crate) static SET_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// The roles that must change between the dark and the light set. `white`, `black` and the
    /// fixed `CHANNEL` colours are not in this list: they are the same in both on purpose.
    fn themed(p: &Palette) -> Vec<(&'static str, Color32)> {
        vec![
            ("background", p.background),
            ("well", p.well),
            ("ruler", p.ruler),
            ("button_off", p.button_off),
            ("meter_track", p.meter_track),
            ("track_header", p.track_header),
            ("track_header_selected", p.track_header_selected),
            ("track_header_idle", p.track_header_idle),
            ("track_lane", p.track_lane),
            ("grid", p.grid),
            ("hairline", p.hairline),
            ("divider", p.divider),
            ("text", p.text),
            ("text_strong", p.text_strong),
            ("text_dim", p.text_dim),
            ("text_faint", p.text_faint),
            ("alert", p.alert),
            ("cursor", p.cursor),
            ("caution", p.caution),
            ("ok", p.ok),
            ("ok_dim", p.ok_dim),
            ("accent", p.accent),
            ("accent_fill", p.accent_fill),
            ("range_fill", p.range_fill),
            ("selection", p.selection),
            ("playhead", p.playhead),
            ("band_fill", p.band_fill),
            ("band_outline", p.band_outline),
            ("marker", p.marker),
        ]
    }

    #[test]
    fn every_themed_role_differs_between_dark_and_light() {
        for ((name, dark), (_, light)) in themed(&DARK).into_iter().zip(themed(&LIGHT)) {
            assert_ne!(dark, light, "role {name} is the same in both sets");
        }
    }

    #[test]
    fn switching_the_set_changes_what_panels_read() {
        let _guard = SET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_dark(true);
        assert_eq!(pal(), &DARK);
        assert_ne!(pal().background, LIGHT.background);
        set_dark(false);
        assert_eq!(pal(), &LIGHT);
        assert_ne!(pal().text, DARK.text);
        set_dark(true);
    }

    /// Sources of the UI, read from disk. Test modules and `*_tests.rs` files are not UI code.
    fn ui_sources() -> Vec<(String, String)> {
        let mut pending = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut out = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && name != "theme.rs" && !name.ends_with("_tests.rs") {
                    out.push((name, std::fs::read_to_string(&path).unwrap_or_default()));
                }
            }
        }
        out
    }

    /// Each line of `src` (outside test modules and comments) that names a colour value:
    /// a `Color32` constructor such as `from_rgb`, or a named colour such as `WHITE`.
    fn inline_colours(src: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        let mut skipping_test_mod: Option<i32> = None;
        let mut pending_cfg_test = false;
        for (i, line) in src.lines().enumerate() {
            let t = line.trim_start();
            if let Some(depth) = skipping_test_mod.as_mut() {
                *depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if *depth <= 0 {
                    skipping_test_mod = None;
                }
                continue;
            }
            if t.starts_with("#[cfg(test)]") {
                pending_cfg_test = true;
                continue;
            }
            if pending_cfg_test && (t.starts_with("mod ") || t.starts_with("pub mod ")) && line.contains('{') {
                let depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if depth > 0 {
                    skipping_test_mod = Some(depth);
                }
                pending_cfg_test = false;
                continue;
            }
            pending_cfg_test = false;
            if t.starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(at) = rest.find("Color32::") {
                let after = &rest[at + "Color32::".len()..];
                let named = after.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                if after.starts_with("from_") || named {
                    found.push((i + 1, line.trim().to_string()));
                    break;
                }
                rest = after;
            }
        }
        found
    }

    #[test]
    fn scanner_flags_inline_colour_values() {
        assert_eq!(inline_colours("let c = egui::Color32::from_rgb(1, 2, 3);").len(), 1);
        assert_eq!(inline_colours("ui.colored_label(Color32::LIGHT_RED, t);").len(), 1);
        assert_eq!(inline_colours("p.rect_filled(r, 0.0, Color32::from_gray(24));").len(), 1);
        assert!(inline_colours("let c = theme::pal().alert;").is_empty());
        assert!(inline_colours("let c = theme::rgb(1, 2, 3);").is_empty());
        assert!(inline_colours("// Color32::from_rgb(1, 2, 3) in a comment").is_empty());
        assert!(inline_colours("fn f(c: Color32) -> Color32 { c }").is_empty());
        assert!(inline_colours("#[cfg(test)]\nmod tests {\n    fn t() { Color32::from_rgb(1, 2, 3); }\n}\n").is_empty());
    }

    #[test]
    fn no_inline_colour_values_in_ui_sources() {
        let sources = ui_sources();
        assert!(!sources.is_empty(), "the UI sources were not found");
        let mut bad = Vec::new();
        for (file, src) in &sources {
            for (line, text) in inline_colours(src) {
                bad.push(format!("{file}:{line}: {text}"));
            }
        }
        assert!(bad.is_empty(), "inline colour values in the UI (use theme::pal() or theme::rgb):\n{}", bad.join("\n"));
    }
}
