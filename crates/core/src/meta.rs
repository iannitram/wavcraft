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
    /// The `iXML` chunk body as the file had it (trailing zeros included), if any.
    pub ixml: Option<Vec<u8>>,
    /// One name per channel (empty = none): from iXML `TRACK_LIST`, else the channel-mask labels.
    pub chnames: Vec<String>,
    /// A channel name was changed: the save rewrites the names in the iXML (the rest of it
    /// byte for byte); until then the chunk is copied through.
    pub chnames_edited: bool,
}

/// The fields `META get|set` and the panel know, in the panel's order.
pub const FIELDS: [&str; 14] = [
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
    "date",
    "genre",
    "copyright",
    "software",
];

/// The RIFF INFO tag a field name stands for: the friendly names above, or `INFO.XXXX` for any
/// four-character tag.
fn info_id(field: &str) -> Option<[u8; 4]> {
    match field {
        "title" => Some(*b"INAM"),
        "artist" => Some(*b"IART"),
        "comment" => Some(*b"ICMT"),
        "date" => Some(*b"ICRD"),
        "genre" => Some(*b"IGNR"),
        "copyright" => Some(*b"ICOP"),
        "software" => Some(*b"ISFT"),
        _ => {
            let id: [u8; 4] = field.strip_prefix("INFO.")?.as_bytes().try_into().ok()?;
            id.iter().all(|c| c.is_ascii_alphanumeric() || *c == b' ').then_some(id)
        }
    }
}

/// `chname.N` (N counts from 1) to a channel index.
fn chname_index(field: &str) -> Option<usize> {
    field.strip_prefix("chname.")?.parse::<usize>().ok()?.checked_sub(1)
}

/// Speaker labels of a `WAVE_FORMAT_EXTENSIBLE` channel mask, in bit order.
const MASK_LABELS: [&str; 18] = ["L", "R", "C", "LFE", "BL", "BR", "FLC", "FRC", "BC", "SL", "SR", "TC", "TFL", "TFC", "TFR", "TBL", "TBC", "TBR"];

/// The labels of the set bits of `mask`, one per channel (empty strings where the mask has fewer
/// speakers than channels, or no mask).
pub fn mask_names(mask: u32, channels: usize) -> Vec<String> {
    let mut out: Vec<String> = (0..MASK_LABELS.len()).filter(|b| mask >> b & 1 == 1).map(|b| MASK_LABELS[b].to_string()).collect();
    out.resize(channels, String::new());
    out.truncate(channels);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text between `<tag>` and `</tag>` in `s`, as byte offsets of the inside.
fn element(s: &str, tag: &str) -> Option<(usize, usize)> {
    let open = format!("<{tag}>");
    let start = s.find(&open)? + open.len();
    let end = start + s[start..].find(&format!("</{tag}>"))?;
    Some((start, end))
}

/// The `TRACK` elements of the iXML `TRACK_LIST`: (inside start, inside end, channel index).
fn ixml_tracks(s: &str) -> Vec<(usize, usize, usize)> {
    let Some((ls, le)) = element(s, "TRACK_LIST") else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = ls;
    while let Some(o) = s[at..le].find("<TRACK>") {
        let start = at + o + 7;
        let Some(c) = s[start..le].find("</TRACK>") else { break };
        let end = start + c;
        let inner = &s[start..end];
        let idx = element(inner, "INTERLEAVE_INDEX").and_then(|(a, b)| inner[a..b].trim().parse::<usize>().ok()).and_then(|i| i.checked_sub(1));
        out.push((start, end, idx.unwrap_or(out.len())));
        at = end;
    }
    out
}

/// The body without its trailing zero bytes as text, and those zeros.
fn ixml_text(raw: &[u8]) -> Option<(&str, &[u8])> {
    let n = raw.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    Some((std::str::from_utf8(&raw[..n]).ok()?, &raw[n..]))
}

/// The channel names an iXML body holds (index = channel; empty where none).
pub fn ixml_names(raw: &[u8], channels: usize) -> Vec<String> {
    let mut names = vec![String::new(); channels];
    let Some((s, _)) = ixml_text(raw) else { return names };
    for (a, b, ch) in ixml_tracks(s) {
        if let (Some(slot), Some((x, y))) = (names.get_mut(ch), element(&s[a..b], "NAME")) {
            *slot = xml_unescape(s[a + x..a + y].trim());
        }
    }
    names
}

fn track_xml(i: usize, name: &str) -> String {
    format!(
        "<TRACK>\n<CHANNEL_INDEX>{0}</CHANNEL_INDEX>\n<INTERLEAVE_INDEX>{0}</INTERLEAVE_INDEX>\n<NAME>{1}</NAME>\n</TRACK>\n",
        i + 1,
        xml_escape(name)
    )
}

/// `raw` (the iXML a file had, if any) with the channel names set to `names`: only the `NAME`
/// elements (and what a missing track needs) change; every other byte stays. A name that
/// already reads the same is not touched. An iXML that is not UTF-8 is returned as it was.
pub fn ixml_with_names(raw: Option<&[u8]>, names: &[String]) -> Vec<u8> {
    let Some(raw) = raw else {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<BWFXML>\n<IXML_VERSION>1.5</IXML_VERSION>\n<TRACK_LIST>\n");
        s.push_str(&format!("<TRACK_COUNT>{}</TRACK_COUNT>\n", names.len()));
        for (i, n) in names.iter().enumerate() {
            s.push_str(&track_xml(i, n));
        }
        s.push_str("</TRACK_LIST>\n</BWFXML>\n");
        return s.into_bytes();
    };
    let Some((s, zeros)) = ixml_text(raw) else { return raw.to_vec() };
    let tracks = ixml_tracks(s);
    // (position, replaced length, new text), applied from the end so offsets stay true.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut seen = vec![false; names.len()];
    for &(a, b, ch) in &tracks {
        let Some(want) = names.get(ch) else { continue };
        seen[ch] = true;
        let inner = &s[a..b];
        match element(inner, "NAME") {
            Some((x, y)) if xml_unescape(inner[x..y].trim()) != *want => edits.push((a + x, y - x, xml_escape(want))),
            Some(_) => {}
            None if !want.is_empty() => edits.push((b, 0, format!("<NAME>{}</NAME>\n", xml_escape(want)))),
            None => {}
        }
    }
    let missing: String = names.iter().enumerate().filter(|(i, n)| !seen[*i] && !n.is_empty()).map(|(i, n)| track_xml(i, n)).collect();
    if !missing.is_empty() {
        if let Some((_, le)) = element(s, "TRACK_LIST") {
            edits.push((le, 0, missing));
            if let Some((a, b)) = element(s, "TRACK_COUNT") {
                let count = names.len().max(tracks.iter().map(|t| t.2 + 1).max().unwrap_or(0));
                edits.push((a, b - a, count.to_string()));
            }
        } else {
            let block = format!("<TRACK_LIST>\n<TRACK_COUNT>{}</TRACK_COUNT>\n{missing}</TRACK_LIST>\n", names.len());
            edits.push((s.rfind("</BWFXML>").unwrap_or(s.len()), 0, block));
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = s.to_string();
    for (at, len, text) in edits {
        out.replace_range(at..at + len, &text);
    }
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(zeros);
    bytes
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
        if let Some(ch) = chname_index(field) {
            return self.chnames.get(ch).cloned();
        }
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
        if let Some(ch) = chname_index(field) {
            if value.contains(['\0', '\r', '\n']) {
                return Err("a channel name is one line".to_string());
            }
            let n = self.chnames.len();
            let slot = self.chnames.get_mut(ch).ok_or_else(|| format!("no channel {} (the file has {n})", ch + 1))?;
            *slot = value.trim().to_string();
            self.chnames_edited = true;
            return Ok(());
        }
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

    /// The field names the panel shows: [`FIELDS`], then `chname.N` for each channel.
    pub fn keys(&self) -> Vec<String> {
        FIELDS.iter().map(|f| f.to_string()).chain((1..=self.chnames.len()).map(|n| format!("chname.{n}"))).collect()
    }

    /// `name=value` lines for every field, for the control channel's `META get`.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for f in self.keys() {
            let _ = write!(s, "{}{f}={}", if s.is_empty() { "" } else { " " }, escape_lines(&self.get(&f).unwrap_or_default()).replace(' ', "_"));
        }
        s
    }
}

fn is_pattern(s: &str, pattern: &[u8]) -> bool {
    s.len() == pattern.len() && s.bytes().zip(pattern).all(|(c, p)| if *p == b'd' { c.is_ascii_digit() } else { c == *p })
}

/// The body of a Vorbis comment (the FLAC VORBIS_COMMENT block body; for Opus, the bytes after `OpusTags`).
/// Layout: u32 LE vendor length, vendor UTF-8, u32 LE comment count, then per comment u32 LE length and
/// the UTF-8 bytes of `KEY=value`. Entries whose key is empty or has a byte outside 0x20..=0x7D or is `=`
/// are skipped and not counted. Order and duplicates are kept.
pub fn vorbis_comment_body(vendor: &str, comments: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();

    // Step 1: Add vendor length and UTF-8 bytes
    let vendor_bytes = vendor.as_bytes();
    let vendor_len = u32::try_from(vendor_bytes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&vendor_len.to_le_bytes());
    out.extend_from_slice(vendor_bytes);

    // Step 2 & 3: Filter comments and add them
    let mut comment_count = 0u32;
    let mut filtered_comments = Vec::new();

    for &(key, value) in comments {
        // Skip empty keys or keys with invalid bytes
        if key.is_empty() || key.bytes().any(|b| !(0x20..=0x7D).contains(&b) || b == b'=') {
            continue;
        }

        filtered_comments.push((key, value));
        comment_count += 1;
    }

    // Add comment count
    out.extend_from_slice(&comment_count.to_le_bytes());

    // Add each valid comment
    for (key, value) in filtered_comments {
        let key_value = format!("{}={}", key, value);
        let key_value_bytes = key_value.as_bytes();
        let len = u32::try_from(key_value_bytes.len()).unwrap_or(u32::MAX);

        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(key_value_bytes);
    }

    out
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
    fn info_fields_and_generic_ids() {
        let mut m = Metadata::default();
        for (f, id) in [("date", *b"ICRD"), ("genre", *b"IGNR"), ("copyright", *b"ICOP"), ("software", *b"ISFT"), ("INFO.IKEY", *b"IKEY")] {
            m.set(f, "v").unwrap();
            assert_eq!(m.info.get(id).as_deref(), Some("v"), "{f}");
            assert_eq!(m.get(f).as_deref(), Some("v"));
        }
        assert!(m.set("INFO.TOOLONG", "x").is_err());
    }

    #[test]
    fn channel_names_from_ixml_and_mask() {
        let x = b"<BWFXML><TRACK_LIST><TRACK_COUNT>2</TRACK_COUNT><TRACK><INTERLEAVE_INDEX>2</INTERLEAVE_INDEX><NAME>Boom &amp; Lav</NAME></TRACK><TRACK><INTERLEAVE_INDEX>1</INTERLEAVE_INDEX><NAME>Mix</NAME></TRACK></TRACK_LIST></BWFXML>\0\0";
        assert_eq!(ixml_names(x, 3), ["Mix", "Boom & Lav", ""]);
        assert_eq!(mask_names(0x3, 2), ["L", "R"]);
        assert_eq!(mask_names(0x4, 2), ["C", ""]);
    }

    #[test]
    fn ixml_rewrite_changes_only_the_names() {
        let x = b"<?xml version=\"1.0\"?>\n<BWFXML>\n <UNKNOWN a=\"1\">keep &#39; me</UNKNOWN>\n<TRACK_LIST><TRACK_COUNT>2</TRACK_COUNT><TRACK><INTERLEAVE_INDEX>1</INTERLEAVE_INDEX><NAME>Mix</NAME></TRACK><TRACK><INTERLEAVE_INDEX>2</INTERLEAVE_INDEX></TRACK></TRACK_LIST>\n</BWFXML>\n\0";
        let same = ixml_with_names(Some(x), &["Mix".into(), String::new()]);
        assert_eq!(same, x, "unchanged names leave every byte");
        let out = ixml_with_names(Some(x), &["Mix".into(), "B<2>".into()]);
        let want = String::from_utf8_lossy(x).replace("</INTERLEAVE_INDEX></TRACK>", "</INTERLEAVE_INDEX><NAME>B&lt;2&gt;</NAME>\n</TRACK>");
        assert_eq!(String::from_utf8(out).unwrap(), want);
        let three = ixml_with_names(Some(x), &["Mix".into(), String::new(), "Z".into()]);
        let t = String::from_utf8(three).unwrap();
        assert!(t.contains("<TRACK_COUNT>3</TRACK_COUNT>") && t.contains("<NAME>Z</NAME>") && t.contains("keep &#39; me"));
        assert_eq!(ixml_names(t.as_bytes(), 3), ["Mix", "", "Z"]);
        let none = ixml_with_names(None, &["A".into(), "B".into()]);
        assert_eq!(ixml_names(&none, 2), ["A", "B"]);
        let no_list = ixml_with_names(Some(b"<BWFXML><X>1</X></BWFXML>"), &["A".into()]);
        let t = String::from_utf8(no_list).unwrap();
        assert!(t.starts_with("<BWFXML><X>1</X><TRACK_LIST>") && t.ends_with("</TRACK_LIST>\n</BWFXML>"));
        assert_eq!(ixml_with_names(Some(&[0xff, 0xfe]), &["A".into()]), [0xff, 0xfe]);
    }

    #[test]
    fn history_lines_end_in_crlf() {
        assert_eq!(Bext::with_history_line("", "A"), "A\r\n");
        assert_eq!(Bext::with_history_line("X\r\n", "A"), "X\r\nA\r\n");
        assert_eq!(Bext::with_history_line("X", "A"), "X\r\nA\r\n");
        assert_eq!(unescape_lines(&escape_lines("a\r\nb\\c")), "a\r\nb\\c");
    }

    #[test]
    fn vorbis_body_with_no_comments() {
        assert_eq!(vorbis_comment_body("wavcraft", &[]), vec![8, 0, 0, 0, 119, 97, 118, 99, 114, 97, 102, 116, 0, 0, 0, 0]);
    }

    #[test]
    fn vorbis_body_one_comment_layout() {
        assert_eq!(
            vorbis_comment_body("wavcraft", &[("TITLE", "Hi")]),
            vec![8, 0, 0, 0, 119, 97, 118, 99, 114, 97, 102, 116, 1, 0, 0, 0, 8, 0, 0, 0, 84, 73, 84, 76, 69, 61, 72, 105]
        );
    }

    #[test]
    fn vorbis_body_length_counts_utf8_bytes() {
        assert_eq!(
            vorbis_comment_body("x", &[("ARTIST", "Zoë")]),
            vec![1, 0, 0, 0, 120, 1, 0, 0, 0, 11, 0, 0, 0, 65, 82, 84, 73, 83, 84, 61, 90, 111, 195, 171]
        );
    }

    #[test]
    fn vorbis_body_skips_invalid_keys_and_does_not_count_them() {
        assert_eq!(
            vorbis_comment_body("x", &[("", "a"), ("A=B", "a"), ("T\u{7f}", "a"), ("BAD\tKEY", "a"), ("ALBUM", "A")]),
            vec![1, 0, 0, 0, 120, 1, 0, 0, 0, 7, 0, 0, 0, 65, 76, 66, 85, 77, 61, 65]
        );
    }

    #[test]
    fn vorbis_body_keeps_order_and_duplicates() {
        assert_eq!(
            vorbis_comment_body("", &[("GENRE", "Pop"), ("GENRE", "Rock")]),
            vec![0, 0, 0, 0, 2, 0, 0, 0, 9, 0, 0, 0, 71, 69, 78, 82, 69, 61, 80, 111, 112, 10, 0, 0, 0, 71, 69, 78, 82, 69, 61, 82, 111, 99, 107]
        );
    }
}
