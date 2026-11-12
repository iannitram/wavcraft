//! The word-timed transcript of a document (v1 section L, roadmap L1, #1057).
//!
//! A [`Transcript`] is the words in order, each with its start and end **frame** at the
//! document's sample rate (end exclusive). Words never overlap: each starts at or after the
//! previous one's end. It is written as JSON ([`Transcript::to_json`]) and read back strictly
//! ([`Transcript::from_json`]): an unknown version, a missing field, a number that is not a
//! whole frame, an empty or backwards word or an overlap is an error, never a guess.
//!
//! The transcript is kept beside the audio file as `<file name>.transcript.json`
//! ([`sidecar_path`], [`save_beside`], [`load_beside`]).

use std::ffi::OsString;
use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::markers::Shift;

pub mod export;
pub mod filler;
pub mod mel;
mod run;

pub use run::{FakeEngine, SpeechEngine, transcribe};

/// The JSON layout this build writes and reads.
pub const VERSION: u32 = 1;

/// One word: its text and its frames `start..end` (end exclusive, `start < end`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub start: u64,
    pub end: u64,
}

/// The words of a document, in order, with frame times at `sample_rate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// Always [`VERSION`] for a transcript this build made or read.
    pub version: u32,
    pub sample_rate: u32,
    pub words: Vec<Word>,
}

/// The frames of `w` once `range` is replaced by `len` frames; None when the word lies wholly
/// inside a non-empty `range`. An empty `range` is an insert at its start: a word starting there
/// moves, and a word spanning it grows.
fn after_edit(w: &Word, range: &Range<u64>, len: u64) -> Option<(u64, u64)> {
    let n = range.end - range.start;
    if n == 0 {
        let start = if w.start < range.start { w.start } else { w.start + len };
        let end = if w.end <= range.start { w.end } else { w.end + len };
        return Some((start, end));
    }
    if w.start >= range.start && w.end <= range.end {
        return None;
    }
    let map = |p: u64, is_start: bool| {
        if p <= range.start {
            p
        } else if p >= range.end {
            p - n + len
        } else if is_start {
            range.start + len
        } else {
            range.start
        }
    };
    Some((map(w.start, true), map(w.end, false)))
}

fn bad(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("transcript: {}", what.into()))
}

impl Transcript {
    /// A transcript of the current version.
    pub fn new(sample_rate: u32, words: Vec<Word>) -> Transcript {
        Transcript { version: VERSION, sample_rate, words }
    }

    /// The index of the word that covers `frame`: its start is at or before `frame` and its end
    /// after it (start inclusive, end exclusive). `None` for a frame in a gap between words, before
    /// the first word, after the last, or in an empty transcript. Found by binary search, so the
    /// words must be in order and not overlap, as [`Transcript::check`] requires.
    pub fn word_at(&self, frame: u64) -> Option<usize> {
        // The words starting at or before `frame` form a prefix; the last of them is the only
        // candidate, since no earlier word can reach past a later word's start.
        let after = self.words.partition_point(|w| w.start <= frame);
        let i = after.checked_sub(1)?;
        (frame < self.words[i].end).then_some(i)
    }

    /// A copy with the words an edit leaves at the frames it moves them to, in the same rules as
    /// [`crate::markers::Markers::moved_by`]: a word wholly inside a removed range goes, one that
    /// straddles an edge is cut to what is left, a later word moves with the audio. An insert at a
    /// word's start moves that word, and a word spanning the insert grows. A crop keeps only the
    /// words inside its range, cut to it.
    pub fn moved_by(&self, shift: &Shift) -> Transcript {
        let words = self
            .words
            .iter()
            .filter_map(|w| {
                let (start, end) = match shift {
                    Shift::None => (w.start, w.end),
                    Shift::Delete(r) => after_edit(w, r, 0)?,
                    Shift::Insert { at, len } => after_edit(w, &(*at..*at), *len)?,
                    Shift::Replace { range, len } => after_edit(w, range, *len)?,
                    Shift::Crop(r) => {
                        if w.end <= r.start || w.start >= r.end {
                            return None;
                        }
                        (w.start.max(r.start) - r.start, w.end.min(r.end) - r.start)
                    }
                };
                Some(Word { text: w.text.clone(), start, end })
            })
            .collect();
        Transcript { version: self.version, sample_rate: self.sample_rate, words }
    }

    /// Whether this transcript is one [`Transcript::from_json`] would accept: the current
    /// version, a sample rate above 0, every word `start < end`, none starting before the
    /// previous one's end.
    pub fn check(&self) -> io::Result<()> {
        if self.version != VERSION {
            return Err(bad(format!("version {} (this build reads {VERSION})", self.version)));
        }
        if self.sample_rate == 0 {
            return Err(bad("sample rate 0"));
        }
        let mut prev_end = 0u64;
        for (i, w) in self.words.iter().enumerate() {
            if w.start >= w.end {
                return Err(bad(format!("word {i} starts at {} and ends at {}", w.start, w.end)));
            }
            if w.start < prev_end {
                return Err(bad(format!("word {i} starts at {}, before the previous word ends at {prev_end}", w.start)));
            }
            prev_end = w.end;
        }
        Ok(())
    }

    /// The JSON form: `{"version":1,"sample_rate":…,"words":[{"text":…,"start":…,"end":…},…]}`.
    pub fn to_json(&self) -> String {
        let words: Vec<Value> = self
            .words
            .iter()
            .map(|w| {
                let mut m = Map::new();
                m.insert("text".into(), Value::from(w.text.as_str()));
                m.insert("start".into(), Value::from(w.start));
                m.insert("end".into(), Value::from(w.end));
                Value::Object(m)
            })
            .collect();
        let mut m = Map::new();
        m.insert("version".into(), Value::from(self.version));
        m.insert("sample_rate".into(), Value::from(self.sample_rate));
        m.insert("words".into(), Value::Array(words));
        Value::Object(m).to_string()
    }

    /// Read the JSON form back. Anything [`Transcript::check`] refuses, or that is not that
    /// layout, is an `InvalidData` error.
    pub fn from_json(text: &str) -> io::Result<Transcript> {
        let v: Value = serde_json::from_str(text).map_err(|e| bad(format!("not JSON ({e})")))?;
        let obj = v.as_object().ok_or_else(|| bad("not an object"))?;
        let version = u32_field(obj, "version")?;
        if version != VERSION {
            return Err(bad(format!("version {version} (this build reads {VERSION})")));
        }
        let sample_rate = u32_field(obj, "sample_rate")?;
        let list = obj.get("words").and_then(Value::as_array).ok_or_else(|| bad("no word list"))?;
        let mut words = Vec::new();
        words.try_reserve_exact(list.len()).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "transcript: too many words"))?;
        for (i, w) in list.iter().enumerate() {
            let w = w.as_object().ok_or_else(|| bad(format!("word {i} is not an object")))?;
            let text = w.get("text").and_then(Value::as_str).ok_or_else(|| bad(format!("word {i} has no text")))?;
            let frame = |key: &str| w.get(key).and_then(Value::as_u64).ok_or_else(|| bad(format!("word {i} has no whole-frame {key}")));
            words.push(Word { text: text.to_string(), start: frame("start")?, end: frame("end")? });
        }
        let t = Transcript { version, sample_rate, words };
        t.check()?;
        Ok(t)
    }
}

/// The largest sidecar [`load_beside`] reads: 64 MiB, hundreds of hours of words.
pub const MAX_SIDECAR_BYTES: u64 = 64 << 20;

/// The sidecar of `audio`: the same folder, its file name with `.transcript.json` added
/// (`episode.wav` gives `episode.wav.transcript.json`).
pub fn sidecar_path(audio: &Path) -> PathBuf {
    let mut name = audio.file_name().map(OsString::from).unwrap_or_default();
    name.push(".transcript.json");
    audio.with_file_name(name)
}

/// Write `transcript` as the sidecar of `audio`, atomically (a temporary file renamed over the
/// old one, [`crate::save::write_file_atomic`]). A transcript [`Transcript::check`] refuses is
/// not written.
pub fn save_beside(audio: &Path, transcript: &Transcript) -> io::Result<()> {
    transcript.check()?;
    crate::save::write_file_atomic(&sidecar_path(audio), transcript.to_json().as_bytes(), false)
}

/// Read the sidecar of `audio`. `Ok(None)` when there is none; an error when it exists but
/// cannot be read, is larger than [`MAX_SIDECAR_BYTES`], or is not a valid transcript.
pub fn load_beside(audio: &Path) -> io::Result<Option<Transcript>> {
    let path = sidecar_path(audio);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut text = String::new();
    file.take(MAX_SIDECAR_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_SIDECAR_BYTES {
        return Err(bad(format!("{} is larger than {MAX_SIDECAR_BYTES} bytes", path.display())));
    }
    Transcript::from_json(&text).map(Some)
}

fn u32_field(obj: &Map<String, Value>, key: &str) -> io::Result<u32> {
    let n = obj.get(key).and_then(Value::as_u64).ok_or_else(|| bad(format!("no whole-number {key}")))?;
    u32::try_from(n).map_err(|_| bad(format!("{key} {n} is too large")))
}
