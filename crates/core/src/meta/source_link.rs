//! A link from a mixed-down file back to the session that made it (roadmap A326, "Edit
//! Original"). The link is one `LIST`/`INFO` tag (`LINK_FIELD`) of the file's [`Metadata`]; it is
//! written and read through the same `Metadata::set` / `Metadata::get` the other INFO fields use,
//! and touches no other field. Writing the metadata into a WAV or FLAC file and the menu action
//! are not here.
//!
//! The tag value is one ASCII line: `WCLINK1;path=<p>;name=<n>;id=<i>;at=<seconds>` (`id` is left
//! out when there is none). `%`, `;`, control bytes and every non-ASCII byte in a value are
//! written as `%XX` (upper-case hex of the UTF-8 byte), so a path with any characters comes back
//! unchanged. Anything that does not read back exactly (unknown version, missing or repeated
//! field, bad escape, a value over its limit) is `None`, never an error.

use crate::meta::Metadata;

/// The INFO field (see `Metadata::get`) that holds the link.
pub const LINK_FIELD: &str = "INFO.WCSL";
/// Longest session path, in bytes of UTF-8.
pub const MAX_PATH_BYTES: usize = 512;
/// Longest session name, in bytes of UTF-8.
pub const MAX_NAME_BYTES: usize = 128;
/// Longest track or clip id, in bytes of UTF-8.
pub const MAX_ID_BYTES: usize = 64;
/// The first part of every tag value; the digit is the format version.
const VERSION_TAG: &str = "WCLINK1";

/// Where a mixed-down file came from. The fields are private: [`SourceLink::new`] is the only
/// way to build one, so every value has passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLink {
    session_path: String,
    session_name: String,
    item_id: Option<String>,
    created_at: u64,
}

impl SourceLink {
    /// A link to the session file `session_path` (as the caller wants it stored), its display
    /// name, an optional track or clip id, and `created_at` (seconds since the Unix epoch, passed
    /// in so the result is the same every time). Errors name what is wrong.
    pub fn new(session_path: &str, session_name: &str, item_id: Option<&str>, created_at: u64) -> Result<SourceLink, String> {
        for (field, value, max) in [
            ("session_path", Some(session_path), MAX_PATH_BYTES),
            ("session_name", Some(session_name), MAX_NAME_BYTES),
            ("item_id", item_id, MAX_ID_BYTES),
        ] {
            let Some(v) = value else { continue };
            if v.is_empty() {
                return Err(format!("{field} cannot be empty"));
            }
            if v.contains('\0') {
                return Err(format!("{field} cannot hold a zero byte"));
            }
            if v.len() > max {
                return Err(format!("{field} takes at most {max} bytes"));
            }
        }
        Ok(SourceLink {
            session_path: session_path.to_string(),
            session_name: session_name.to_string(),
            item_id: item_id.map(str::to_string),
            created_at,
        })
    }

    /// The session file path as given.
    pub fn session_path(&self) -> &str {
        &self.session_path
    }

    /// The session name as given.
    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    /// The track or clip id, if any.
    pub fn item_id(&self) -> Option<&str> {
        self.item_id.as_deref()
    }

    /// The caller's timestamp.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// The tag value for this link (see the module text).
    pub fn encode(&self) -> String {
        let mut out = format!("{VERSION_TAG};path={};name={}", pct_encode(&self.session_path), pct_encode(&self.session_name));
        if let Some(id) = &self.item_id {
            out.push_str(";id=");
            out.push_str(&pct_encode(id));
        }
        out.push_str(&format!(";at={}", self.created_at));
        out
    }

    /// The link a tag value holds; `None` for anything that is not exactly what `encode` makes
    /// (and for a link that `new` would refuse).
    pub fn parse(value: &str) -> Option<SourceLink> {
        if !value.is_ascii() {
            return None;
        }
        let mut parts = value.split(';');
        if parts.next()? != VERSION_TAG {
            return None;
        }
        let (mut path, mut name, mut id, mut at) = (None, None, None, None);
        for part in parts {
            let (key, val) = part.split_once('=')?;
            let slot = match key {
                "path" => &mut path,
                "name" => &mut name,
                "id" => &mut id,
                "at" => &mut at,
                _ => return None,
            };
            if slot.is_some() {
                return None;
            }
            *slot = Some(val);
        }
        let at = at?;
        if at.is_empty() || !at.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let at: u64 = at.parse().ok()?;
        let path = pct_decode(path?)?;
        let name = pct_decode(name?)?;
        let id = match id {
            Some(v) => Some(pct_decode(v)?),
            None => None,
        };
        SourceLink::new(&path, &name, id.as_deref(), at).ok()
    }
}

/// `s` with every byte that is not printable ASCII, and `%` and `;`, written as `%XX`.
fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if (0x20..=0x7e).contains(&b) && b != b'%' && b != b';' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The inverse of [`pct_encode`]; `None` for a `%` not followed by two hex digits or bytes that
/// are not UTF-8.
fn pct_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = *bytes.get(i)?;
        if b == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Store `link` in `meta` under [`LINK_FIELD`], replacing an earlier link; no other field changes.
pub fn write_source_link(meta: &mut Metadata, link: &SourceLink) -> Result<(), String> {
    meta.set(LINK_FIELD, &link.encode())
}

/// The link `meta` holds under [`LINK_FIELD`]; `None` when there is none or it is not readable.
pub fn read_source_link(meta: &Metadata) -> Option<SourceLink> {
    let value = meta.get(LINK_FIELD)?;
    SourceLink::parse(&value)
}
