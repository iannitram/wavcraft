//! Batch writes of parts of a document (f79): **Extract Channels to Mono Files** (A137) and
//! **Export Audio of Range Markers** (A185). Both go through [`crate::save::save_slice`], so the
//! samples are streamed out of the store 64 K frames at a time (never a whole channel in RAM),
//! each file is written to its own temporary name and renamed in only when complete, and a
//! failed write leaves no temporary file. The open document is never touched.
//!
//! **Careful with the owner's files:** these never replace a file that is already there. A name
//! in use gets `_2`, `_3`… (so running an export twice keeps both sets). File names are made
//! from marker names and patterns by [`sanitize_name`]: no path separators, no characters
//! Windows refuses, no reserved device names, never empty, so a marker called `../x` stays
//! inside the output folder.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::doc::DocState;
use crate::markers::Marker;
use crate::save::{SampleType, SaveError, Slice, save_slice};

/// The longest file name stem we make, in characters.
const MAX_STEM: usize = 80;

/// Default pattern for extracted channels: `<name>_L`.
pub const DEFAULT_CHANNEL_PATTERN: &str = "{name}_{channel}";
/// Default pattern for exported ranges: the marker's name.
pub const DEFAULT_RANGE_PATTERN: &str = "{name}";

/// Why a batch write failed or was refused.
#[derive(Debug)]
pub enum ExportError {
    /// Nothing to write (one channel already, or no range markers); the message says what.
    Nothing(String),
    /// The output folder could not be made.
    Folder(io::Error),
    /// One file failed; the files named in `written` are complete, this one left nothing behind.
    Save { path: PathBuf, error: SaveError, written: Vec<PathBuf> },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Nothing(m) => write!(f, "{m}"),
            ExportError::Folder(e) => write!(f, "the output folder can't be used: {e}"),
            ExportError::Save { path, error, written } => {
                write!(f, "could not write {}: {error} ({} file(s) before it were written)", path.display(), written.len())
            }
        }
    }
}

impl std::error::Error for ExportError {}

/// Options for the batch writes.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub ty: SampleType,
    /// File name pattern without the extension: `{name}`, `{index}` (1-based, 3 digits),
    /// `{channel}` (extract only) and `{base}` (the document's name).
    pub pattern: String,
    /// Tests: each file fails once this many frames are out.
    #[doc(hidden)]
    pub fail_after: Option<u64>,
}

impl ExportOptions {
    pub fn new(ty: SampleType, pattern: &str) -> ExportOptions {
        ExportOptions { ty, pattern: pattern.to_string(), fail_after: None }
    }
}

/// `name` as a safe file name stem: letters, digits, space, `-`, `_`, `.`, `(`, `)` stay; anything
/// else (separators, `:`, `*`, control characters…) becomes `_`. Leading and trailing spaces and
/// dots go, the length is capped, a Windows device name (`CON`, `NUL`, `COM1`…) gets a `_` in
/// front, and an empty result is `untitled`.
pub fn sanitize_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' })
        .take(MAX_STEM * 2)
        .collect();
    out = out.trim_matches([' ', '.']).to_string();
    if out.chars().count() > MAX_STEM {
        out = out.chars().take(MAX_STEM).collect::<String>().trim_end_matches([' ', '.']).to_string();
    }
    if out.is_empty() {
        return "untitled".to_string();
    }
    let device = out.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4 && (device.starts_with("COM") || device.starts_with("LPT")) && device.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{out}") } else { out }
}

/// Fill in a pattern. The values are sanitised first, then the whole result again, so neither a
/// marker's name nor the pattern can make a separator.
pub fn expand_pattern(pattern: &str, name: &str, index: usize, channel: &str, base: &str) -> String {
    let text = pattern
        .replace("{name}", &sanitize_name(name))
        .replace("{index}", &format!("{index:03}"))
        .replace("{channel}", &sanitize_name(channel))
        .replace("{base}", &sanitize_name(base));
    sanitize_name(&text)
}

/// The suffix for channel `i` of `count`: `L` and `R` for two, otherwise `C1`, `C2`… (1-based).
pub fn channel_suffix(i: usize, count: usize) -> String {
    match (count, i) {
        (2, 0) => "L".to_string(),
        (2, 1) => "R".to_string(),
        _ => format!("C{}", i + 1),
    }
}

/// `folder/stem.wav`, or `stem_2.wav`, `stem_3.wav`… when that exists or was just chosen.
fn free_path(folder: &Path, stem: &str, taken: &mut HashSet<String>) -> PathBuf {
    let mut n = 1;
    loop {
        let file = if n == 1 { format!("{stem}.wav") } else { format!("{stem}_{n}.wav") };
        let key = file.to_lowercase();
        let path = folder.join(&file);
        if !taken.contains(&key) && std::fs::symlink_metadata(&path).is_err() {
            taken.insert(key);
            return path;
        }
        n += 1;
    }
}

fn make_folder(folder: &Path) -> Result<(), ExportError> {
    std::fs::create_dir_all(folder).map_err(ExportError::Folder)
}

/// Write each channel of `doc` as its own mono WAV in `folder`, named by `opts.pattern` (default
/// [`DEFAULT_CHANNEL_PATTERN`], with `{base}` and `{name}` both the document's name). The
/// document's markers go into each file. `progress` gets 0 to 1 over the whole batch.
/// Returns the files, in channel order.
pub fn extract_channels(
    doc: &DocState,
    folder: &Path,
    base: &str,
    opts: &ExportOptions,
    progress: &mut dyn FnMut(f64),
) -> Result<Vec<PathBuf>, ExportError> {
    let count = doc.channel_count();
    if count < 2 {
        return Err(ExportError::Nothing("the file has one channel already; there is nothing to extract".to_string()));
    }
    if doc.len_frames() == 0 {
        return Err(ExportError::Nothing("the file has no audio".to_string()));
    }
    make_folder(folder)?;
    let markers = doc.markers().to_wav_chunks();
    let (mut taken, mut written) = (HashSet::new(), Vec::new());
    for ch in 0..count {
        let stem = expand_pattern(&opts.pattern, base, ch + 1, &channel_suffix(ch, count), base);
        let path = free_path(folder, &stem, &mut taken);
        let slice = Slice { channels: vec![ch], range: 0..doc.len_frames(), markers: markers.clone() };
        let mut report = |p: f64| progress((ch as f64 + p) / count as f64);
        if let Err(error) = save_slice(doc, &slice, &path, opts.ty, &mut report, opts.fail_after) {
            return Err(ExportError::Save { path, error, written });
        }
        written.push(path);
    }
    Ok(written)
}

/// The range markers an export covers: ranges only (a point has no audio), clipped to the
/// document, empty ones dropped; `only` limits it to those ids.
pub fn exportable_ranges<'a>(doc: &'a DocState, only: Option<&[u64]>) -> Vec<(&'a Marker, Range<u64>)> {
    let len = doc.len_frames();
    doc.markers()
        .iter()
        .filter(|m| !m.is_point() && only.is_none_or(|ids| ids.contains(&m.id)))
        .filter_map(|m| {
            let r = m.start.min(len)..m.end.min(len);
            (r.end > r.start).then_some((m, r))
        })
        .collect()
}

/// Write each range marker (see [`exportable_ranges`]) as its own WAV of all channels in
/// `folder`, named by `opts.pattern` (default [`DEFAULT_RANGE_PATTERN`]; `{index}` counts the
/// ranges in order from 1). Ranges may overlap. Returns the files in marker order.
pub fn export_ranges(
    doc: &DocState,
    folder: &Path,
    only: Option<&[u64]>,
    base: &str,
    opts: &ExportOptions,
    progress: &mut dyn FnMut(f64),
) -> Result<Vec<PathBuf>, ExportError> {
    let ranges = exportable_ranges(doc, only);
    if ranges.is_empty() {
        return Err(ExportError::Nothing("there are no range markers to export".to_string()));
    }
    make_folder(folder)?;
    let (mut taken, mut written) = (HashSet::new(), Vec::new());
    let all: Vec<usize> = (0..doc.channel_count()).collect();
    for (i, (m, range)) in ranges.iter().enumerate() {
        let stem = expand_pattern(&opts.pattern, &m.name, i + 1, "", base);
        let path = free_path(folder, &stem, &mut taken);
        let slice = Slice { channels: all.clone(), range: range.clone(), markers: Vec::new() };
        let mut report = |p: f64| progress((i as f64 + p) / ranges.len() as f64);
        if let Err(error) = save_slice(doc, &slice, &path, opts.ty, &mut report, opts.fail_after) {
            return Err(ExportError::Save { path, error, written });
        }
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitised() {
        assert_eq!(sanitize_name("Verse 1/Intro"), "Verse 1_Intro");
        assert_eq!(sanitize_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_name("a:b*c?d\"e<f>g|h\\i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(sanitize_name("  .. "), "untitled");
        assert_eq!(sanitize_name(""), "untitled");
        assert_eq!(sanitize_name("CON"), "_CON");
        assert_eq!(sanitize_name("com1.take"), "_com1.take");
        assert_eq!(sanitize_name("Café ré"), "Café ré");
        assert_eq!(sanitize_name("tab\there\nnl"), "tab_here_nl");
        assert!(sanitize_name(&"x".repeat(500)).chars().count() <= MAX_STEM);
    }

    #[test]
    fn patterns_fill_in_and_cannot_escape() {
        assert_eq!(expand_pattern("{index}-{name}", "Take/1", 7, "", "song"), "007-Take_1");
        assert_eq!(expand_pattern("{name}_{channel}", "song", 1, "L", "song"), "song_L");
        assert!(!expand_pattern("../{name}", "x", 1, "", "b").contains('/'));
    }

    #[test]
    fn channel_suffixes() {
        assert_eq!(channel_suffix(0, 2), "L");
        assert_eq!(channel_suffix(1, 2), "R");
        assert_eq!(channel_suffix(2, 6), "C3");
    }
}
