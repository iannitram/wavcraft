//! File metadata a WAV carries and a save keeps (roadmap A265, A272, A256, A036): the Broadcast
//! Wave `bext` chunk and the `LIST`/`INFO` tags. Both are read on open into
//! [`SourceFormat::meta`](crate::doc::SourceFormat), edited as one undoable step
//! ([`DocState::with_metadata`](crate::doc::DocState::with_metadata)), and written fresh on
//! save. Every other chunk of the source is copied through by the save (see `save.rs`).
//!
//! The `bext` bytes the app has no field for (version, UMID, loudness, reserved) are kept as
//! they were and written back unchanged.

use std::fmt::Write as _;

/// Size of the fixed part of `bext` (EBU Tech 3285): description 256, originator 32, reference
/// 32, date 10, time 8, time reference 8, then version, UMID, loudness and reserved bytes.
pub const BEXT_FIXED: usize = 602;
/// Where the time reference starts in the chunk.
const TIME_REF_AT: usize = 338;
/// Where the bytes the app does not edit start (the version field).
const TAIL_AT: usize = 346;
/// A `bext` or `INFO` chunk bigger than this is left alone (copied through, not parsed).
pub const MAX_META_BYTES: u64 = 1 << 20;

/// The fields of the Broadcast Wave `bext` chunk.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bext {
    pub description: String,
    pub originator: String,
    pub originator_reference: String,
    /// `yyyy-mm-dd`.
    pub origination_date: String,
    /// `hh:mm:ss`.
    pub origination_time: String,
    /// Samples since midnight of the first sample.
    pub time_reference: u64,
    /// Bytes 346 to 602 of the chunk (version, UMID, loudness, reserved), as read; may be short
    /// in an old file.
    pub tail: Vec<u8>,
    /// The coding history: lines ending in CR LF.
    pub coding_history: String,
}

/// The tags of a `LIST`/`INFO` chunk, in file order. Values are the raw bytes without the
/// terminating zero.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoTags(pub Vec<([u8; 4], Vec<u8>)>);

/// What the app reads, shows and writes of a file's metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    pub bext: Option<Bext>,
    pub info: InfoTags,
    /// An INFO tag was changed: the save writes the `LIST`/`INFO` chunk fresh; until then it is
    /// copied through byte for byte.
    pub info_edited: bool,
}

/// The fields `META get|set` and the panel know, in the panel's order.
pub const FIELDS: [&str; 10] = [
    "description",
    "originator",
    "originator_reference",
    "origination_date",
    "origination_time",
    "time_reference",
    "coding_history",
    "title",
    "artist",
    "comment",
];

fn info_id(field: &str) -> Option<[u8; 4]> {
    match field {
        "title" => Some(*b"INAM"),
        "artist" => Some(*b"IART"),
        "comment" => Some(*b"ICMT"),
        _ => None,
    }
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn fixed_str(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(b.get(..end).unwrap_or_default()).trim_end().to_string()
}

impl Bext {
    /// Parse a `bext` chunk body. `None` when it is shorter than the fixed fields up to the
    /// time reference.
    pub fn parse(c: &[u8]) -> Option<Bext> {
        if c.len() < TAIL_AT {
            return None;
        }
        let (lo, hi) = (u64::from(le_u32(c, TIME_REF_AT)?), u64::from(le_u32(c, TIME_REF_AT + 4)?));
        let tail_end = c.len().min(BEXT_FIXED);
        let history = c.get(BEXT_FIXED..).unwrap_or_default();
        let end = history.iter().position(|&b| b == 0).unwrap_or(history.len());
        Some(Bext {
            description: fixed_str(c.get(0..256)?),
            originator: fixed_str(c.get(256..288)?),
            originator_reference: fixed_str(c.get(288..320)?),
            origination_date: fixed_str(c.get(320..330)?),
            origination_time: fixed_str(c.get(330..338)?),
            time_reference: (hi << 32) | lo,
            tail: c.get(TAIL_AT..tail_end)?.to_vec(),
            coding_history: String::from_utf8_lossy(history.get(..end).unwrap_or_default()).into_owned(),
        })
    }

    /// The chunk body, with `time_reference` and `history` as given (a save passes its own).
    pub fn to_bytes(&self, time_reference: u64, history: &str) -> Vec<u8> {
        fn put(out: &mut Vec<u8>, s: &str, width: usize) {
            let b = s.as_bytes();
            let n = b.len().min(width);
            out.extend_from_slice(&b[..n]);
            out.resize(out.len() + (width - n), 0);
        }
        let mut out = Vec::with_capacity(BEXT_FIXED + history.len());
        put(&mut out, &self.description, 256);
        put(&mut out, &self.originator, 32);
        put(&mut out, &self.originator_reference, 32);
        put(&mut out, &self.origination_date, 10);
        put(&mut out, &self.origination_time, 8);
        out.extend_from_slice(&time_reference.to_le_bytes());
        out.extend_from_slice(&self.tail);
        if !history.is_empty() {
            out.resize(BEXT_FIXED, 0);
        }
        out.extend_from_slice(history.as_bytes());
        out
    }

    /// `history` with `line` added: each line ends in CR LF, and a history that lacks the
    /// final one gets it first.
    pub fn with_history_line(history: &str, line: &str) -> String {
        let mut h = history.to_string();
        if !h.is_empty() && !h.ends_with('\n') {
            h.push_str("\r\n");
        }
        h.push_str(line);
        h.push_str("\r\n");
        h
    }
}

impl InfoTags {
    /// Parse a `LIST` chunk body that starts with `INFO`; `None` for any other list.
    pub fn parse(body: &[u8]) -> Option<InfoTags> {
        if body.get(0..4)? != b"INFO" {
            return None;
        }
        let mut tags = Vec::new();
        let mut at = 4usize;
        while let (Some(id), Some(size)) = (body.get(at..at + 4), le_u32(body, at + 4)) {
            let start = at + 8;
            let end = start.saturating_add(size as usize).min(body.len());
            let (Some(raw), Ok(id)) = (body.get(start..end), <[u8; 4]>::try_from(id)) else { break };
            let mut value = raw.to_vec();
            while value.last() == Some(&0) {
                value.pop();
            }
            tags.push((id, value));
            at = start.saturating_add(size as usize).saturating_add(size as usize & 1);
        }
        Some(InfoTags(tags))
    }

    /// The `LIST` chunk body (`INFO`, then each tag with its zero and pad byte).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = b"INFO".to_vec();
        for (id, value) in &self.0 {
            let len = value.len() + 1;
            out.extend_from_slice(id);
            out.extend_from_slice(&(len as u32).to_le_bytes());
            out.extend_from_slice(value);
            out.push(0);
            if len & 1 == 1 {
                out.push(0);
            }
        }
        out
    }

    fn get(&self, id: [u8; 4]) -> Option<String> {
        self.0.iter().find(|(i, _)| *i == id).map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    }

    fn set(&mut self, id: [u8; 4], value: &str) {
        if value.is_empty() {
            self.0.retain(|(i, _)| *i != id);
        } else if let Some(entry) = self.0.iter_mut().find(|(i, _)| *i == id) {
            entry.1 = value.as_bytes().to_vec();
        } else {
            self.0.push((id, value.as_bytes().to_vec()));
        }
    }
}

/// `\r` and `\n` as the two characters `\r`, `\n` (one line for the control channel).
pub fn escape_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

/// The inverse of [`escape_lines`].
pub fn unescape_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

impl Metadata {
    /// The value of `field` (see [`FIELDS`]); empty when the file has none. `None` for a name
    /// that is not a field.
    pub fn get(&self, field: &str) -> Option<String> {
        if let Some(id) = info_id(field) {
            return Some(self.info.get(id).unwrap_or_default());
        }
        let b = self.bext.as_ref();
        Some(
            match field {
                "description" => b.map(|b| b.description.clone()),
                "originator" => b.map(|b| b.originator.clone()),
                "originator_reference" => b.map(|b| b.originator_reference.clone()),
                "origination_date" => b.map(|b| b.origination_date.clone()),
                "origination_time" => b.map(|b| b.origination_time.clone()),
                "time_reference" => b.map(|b| b.time_reference.to_string()),
                "coding_history" => b.map(|b| b.coding_history.clone()),
                _ => return None,
            }
            .unwrap_or_default(),
        )
    }

    /// Set `field` to `value`. Setting a `bext` field on a file without one gives it a `bext`.
    /// Errors name what is wrong; nothing changes then.
    pub fn set(&mut self, field: &str, value: &str) -> Result<(), String> {
        if let Some(id) = info_id(field) {
            if value.contains('\0') {
                return Err(format!("{field} cannot hold a zero byte"));
            }
            self.info.set(id, value);
            self.info_edited = true;
            return Ok(());
        }
        if !FIELDS.contains(&field) {
            return Err(format!("no metadata field {field:?}; the fields are {}", FIELDS.join(", ")));
        }
        let limit = |max: usize| -> Result<(), String> {
            if !value.is_ascii() {
                return Err(format!("{field} takes ASCII text only"));
            }
            if value.len() > max {
                return Err(format!("{field} takes at most {max} characters"));
            }
            Ok(())
        };
        let mut b = self.bext.clone().unwrap_or_default();
        match field {
            "description" => {
                limit(256)?;
                b.description = value.to_string();
            }
            "originator" => {
                limit(32)?;
                b.originator = value.to_string();
            }
            "originator_reference" => {
                limit(32)?;
                b.originator_reference = value.to_string();
            }
            "origination_date" => {
                if !value.is_empty() && !is_pattern(value, b"dddd-dd-dd") {
                    return Err("origination_date is yyyy-mm-dd".to_string());
                }
                b.origination_date = value.to_string();
            }
            "origination_time" => {
                if !value.is_empty() && !is_pattern(value, b"dd:dd:dd") {
                    return Err("origination_time is hh:mm:ss".to_string());
                }
                b.origination_time = value.to_string();
            }
            "time_reference" => b.time_reference = value.trim().parse().map_err(|_| "time_reference is a whole number of samples".to_string())?,
            _ => {
                if !value.is_ascii() {
                    return Err("coding_history takes ASCII text only".to_string());
                }
                b.coding_history = value.to_string();
            }
        }
        self.bext = Some(b);
        Ok(())
    }

    /// `name=value` lines for every field, for the control channel's `META get`.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for f in FIELDS {
            let _ = write!(s, "{}{f}={}", if s.is_empty() { "" } else { " " }, escape_lines(&self.get(f).unwrap_or_default()).replace(' ', "_"));
        }
        s
    }
}

fn is_pattern(s: &str, pattern: &[u8]) -> bool {
    s.len() == pattern.len() && s.bytes().zip(pattern).all(|(c, p)| if *p == b'd' { c.is_ascii_digit() } else { c == *p })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bext_round_trips_byte_for_byte() {
        let mut raw = vec![0u8; BEXT_FIXED];
        raw[..4].copy_from_slice(b"take");
        raw[256..259].copy_from_slice(b"rec");
        raw[320..330].copy_from_slice(b"2026-10-20");
        raw[338..346].copy_from_slice(&123_456_789_000u64.to_le_bytes());
        raw[346..348].copy_from_slice(&1u16.to_le_bytes());
        raw[400] = 7;
        raw.extend_from_slice(b"A=PCM,F=48000\r\n");
        let b = Bext::parse(&raw).unwrap();
        assert_eq!((b.description.as_str(), b.originator.as_str(), b.time_reference), ("take", "rec", 123_456_789_000));
        assert_eq!(b.coding_history, "A=PCM,F=48000\r\n");
        assert_eq!(b.to_bytes(b.time_reference, &b.coding_history), raw);
    }

    #[test]
    fn a_short_old_bext_has_no_padding_added() {
        let raw = vec![0u8; 346];
        let b = Bext::parse(&raw).unwrap();
        assert_eq!(b.to_bytes(0, ""), raw);
        assert!(Bext::parse(&raw[..300]).is_none());
    }

    #[test]
    fn info_tags_round_trip_and_edit() {
        let mut body = b"INFO".to_vec();
        body.extend_from_slice(b"INAM\x05\0\0\0song\0\0ISFT\x03\0\0\0ab\0\0");
        let mut tags = InfoTags::parse(&body).unwrap();
        assert_eq!(tags.0.len(), 2);
        assert_eq!(tags.to_bytes(), body);
        tags.set(*b"ICMT", "hi");
        tags.set(*b"INAM", "");
        assert_eq!(tags.get(*b"ICMT").as_deref(), Some("hi"));
        assert_eq!(tags.get(*b"INAM"), None);
        assert!(InfoTags::parse(b"adtl....").is_none());
    }

    #[test]
    fn fields_validate_and_a_set_creates_bext() {
        let mut m = Metadata::default();
        assert_eq!(m.get("description").as_deref(), Some(""));
        assert!(m.get("nope").is_none());
        assert!(m.set("origination_date", "yesterday").is_err());
        assert!(m.set("time_reference", "-1").is_err());
        assert!(m.set("originator", &"x".repeat(33)).is_err());
        assert!(m.bext.is_none(), "a refused set changes nothing");
        m.set("description", "hello").unwrap();
        m.set("time_reference", "96000").unwrap();
        assert_eq!(m.bext.as_ref().unwrap().time_reference, 96_000);
        m.set("title", "T").unwrap();
        assert!(m.info_edited);
    }

    #[test]
    fn history_lines_end_in_crlf() {
        assert_eq!(Bext::with_history_line("", "A"), "A\r\n");
        assert_eq!(Bext::with_history_line("X\r\n", "A"), "X\r\nA\r\n");
        assert_eq!(Bext::with_history_line("X", "A"), "X\r\nA\r\n");
        assert_eq!(unescape_lines(&escape_lines("a\r\nb\\c")), "a\r\nb\\c");
    }
}
