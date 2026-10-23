//! Track colour and notes (roadmap C105). Renaming a track and drawing its colour already exist
//! (`rename_track`; the model's `Track::color`). What is added here, in core only, is the
//! validated way to change a track's colour and to read and change its notes, plus a small named
//! palette for default colours.
//!
//! Colour: the model keeps a track's colour as `[u8; 3]` (red, green, blue), so every value of
//! that type is in range. Text is the place where input can be malformed, so [`parse_color`]
//! accepts only `#RRGGBB` (the `#` optional, six hex digits, either case) and refuses all else.
//!
//! Notes: the model's `Track::comments`. At most [`TRACK_NOTES_MAX_CHARS`] characters (Unicode
//! scalar values, not bytes) counted after trimming; longer text is REFUSED, never cut, because
//! the model cuts a longer note when it loads a file and the user would lose text silently.
//! Leading and trailing whitespace is trimmed; control characters are REFUSED except newline and
//! tab; blank text clears the notes; accents, CJK and emoji are kept as typed.
//!
//! Every call that fails changes nothing. The setters return the old value.

use soundcraft_model::TrackId;

use super::{MtError, Multitrack, err};

/// The most characters a track's notes may hold (the model keeps the same number on load).
pub const TRACK_NOTES_MAX_CHARS: usize = 4096;

/// The named default colours, in the order they are handed out.
const PALETTE: [(&str, [u8; 3]); 8] = [
    ("Red", [220, 60, 60]),
    ("Orange", [240, 140, 40]),
    ("Yellow", [235, 205, 60]),
    ("Green", [90, 180, 80]),
    ("Teal", [60, 170, 160]),
    ("Blue", [70, 120, 210]),
    ("Purple", [140, 90, 200]),
    ("Pink", [220, 100, 160]),
];

/// The grey used if the palette were ever empty.
const FALLBACK_COLOR: [u8; 3] = [128, 128, 128];

/// The named palette: eight distinct colours.
pub fn palette() -> &'static [(&'static str, [u8; 3])] {
    &PALETTE
}

/// The default colour for the track at position `n` (0 is the first): the palette cycles, so no
/// colour repeats within any run of palette-length neighbours.
pub fn default_color(n: usize) -> [u8; 3] {
    PALETTE.get(n % PALETTE.len()).map_or(FALLBACK_COLOR, |p| p.1)
}

/// Read a colour typed as `#RRGGBB` (the `#` optional, surrounding whitespace ignored, six hex
/// digits in either case). Anything else is refused.
pub fn parse_color(text: &str) -> Result<[u8; 3], MtError> {
    let bad = || MtError(format!("a colour is written #RRGGBB (6 hex digits), not {text:?}"));
    let t = text.trim();
    let digits = t.strip_prefix('#').unwrap_or(t);
    if digits.chars().count() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let byte = |i: usize| digits.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => Ok([r, g, b]),
        _ => Err(bad()),
    }
}

/// The colour as `#RRGGBB` (upper-case hex, always six digits).
pub fn format_color(color: [u8; 3]) -> String {
    let [r, g, b] = color;
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Check and normalise notes text: trim, refuse control characters other than newline and tab,
/// refuse more than [`TRACK_NOTES_MAX_CHARS`] characters. Blank text gives an empty string.
pub fn clean_track_notes(text: &str) -> Result<String, MtError> {
    let t = text.trim();
    if let Some(c) = t.chars().find(|c| c.is_control() && *c != '\n' && *c != '\t') {
        return Err(MtError(format!("notes cannot contain control characters (found U+{:04X})", c as u32)));
    }
    let n = t.chars().count();
    if n > TRACK_NOTES_MAX_CHARS {
        return Err(MtError(format!("notes can hold at most {TRACK_NOTES_MAX_CHARS} characters; this text has {n}")));
    }
    Ok(t.to_string())
}

impl Multitrack {
    /// A track's colour.
    pub fn track_color(&self, id: TrackId) -> Result<[u8; 3], MtError> {
        match self.session.track(id) {
            Some(t) => Ok(t.color),
            None => err("no such track"),
        }
    }

    /// Set a track's colour; returns the colour it had. An unknown track is an error.
    pub fn set_track_color(&mut self, id: TrackId, color: [u8; 3]) -> Result<[u8; 3], MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        let old = t.color;
        t.color = color;
        Ok(old)
    }

    /// Set a track's colour from `#RRGGBB` text; returns the colour it had. Malformed text or an
    /// unknown track is an error and changes nothing.
    pub fn set_track_color_hex(&mut self, id: TrackId, text: &str) -> Result<[u8; 3], MtError> {
        let color = parse_color(text)?;
        self.set_track_color(id, color)
    }

    /// A track's notes ("" when there are none).
    pub fn track_notes(&self, id: TrackId) -> Result<&str, MtError> {
        match self.session.track(id) {
            Some(t) => Ok(t.comments.as_str()),
            None => err("no such track"),
        }
    }

    /// Set a track's notes after [`clean_track_notes`]; blank text clears them. Returns the notes
    /// it had. An unknown track or refused text is an error and changes nothing.
    pub fn set_track_notes(&mut self, id: TrackId, text: &str) -> Result<String, MtError> {
        let clean = clean_track_notes(text)?;
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        Ok(std::mem::replace(&mut t.comments, clean))
    }
}
