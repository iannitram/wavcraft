//! Session title and notes (roadmap C094). The model's `Session` already carries both and saves
//! them in its JSON: the title is `Session::name`, the notes are `Session::comments` (an older
//! file without `comments` loads with empty notes). What is added here is the validated way to
//! read and change them.
//!
//! Rules:
//! - Notes: at most [`NOTES_MAX_CHARS`] characters (Unicode scalar values, not bytes), counted
//!   after trimming. Longer text is REFUSED, never cut, because the model cuts a longer note
//!   when it loads a file and the user would lose text silently. Leading and trailing whitespace
//!   is trimmed. Control characters are REFUSED (not stripped), except newline and tab. Empty
//!   (or blank) text clears the notes. Everything else, accents, CJK, emoji, is kept as typed.
//! - Title: at most [`TITLE_MAX_CHARS`] characters, trimmed, any control character refused
//!   (a title is one line), and empty is refused because a session always has a name.
//!
//! A refused call changes nothing.

use super::{MtError, Multitrack};

/// The most characters a session's notes may hold (the model keeps the same number on load).
pub const NOTES_MAX_CHARS: usize = 4096;
/// The most characters a session's title may hold (the model keeps the same number on load).
pub const TITLE_MAX_CHARS: usize = 256;

/// Check and normalise notes text: trim, refuse control characters other than newline and tab,
/// refuse more than [`NOTES_MAX_CHARS`] characters. Blank text gives an empty string.
pub fn clean_notes(text: &str) -> Result<String, MtError> {
    let t = text.trim();
    for c in t.chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            return Err(MtError(format!("notes cannot contain control characters (found U+{:04X})", c as u32)));
        }
    }
    let n = t.chars().count();
    if n > NOTES_MAX_CHARS {
        return Err(MtError(format!("notes can hold at most {NOTES_MAX_CHARS} characters; this text has {n}")));
    }
    Ok(t.to_string())
}

/// Check and normalise a title: trim, refuse any control character, refuse empty, refuse more
/// than [`TITLE_MAX_CHARS`] characters.
pub fn clean_title(text: &str) -> Result<String, MtError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(MtError(String::from("a title cannot be empty")));
    }
    for c in t.chars() {
        if c.is_control() {
            return Err(MtError(format!("a title cannot contain control characters (found U+{:04X})", c as u32)));
        }
    }
    let n = t.chars().count();
    if n > TITLE_MAX_CHARS {
        return Err(MtError(format!("a title can hold at most {TITLE_MAX_CHARS} characters; this text has {n}")));
    }
    Ok(t.to_string())
}

impl Multitrack {
    /// The session's notes ("" when there are none).
    pub fn notes(&self) -> &str {
        &self.session.comments
    }

    /// Set the session's notes after [`clean_notes`]; blank text clears them.
    pub fn set_notes(&mut self, text: &str) -> Result<(), MtError> {
        let clean = clean_notes(text)?;
        self.session.comments = clean;
        Ok(())
    }

    /// The session's title (the model's session name).
    pub fn title(&self) -> &str {
        &self.session.name
    }

    /// Set the session's title after [`clean_title`].
    pub fn set_title(&mut self, text: &str) -> Result<(), MtError> {
        let clean = clean_title(text)?;
        self.session.name = clean;
        Ok(())
    }
}
