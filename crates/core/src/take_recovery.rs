//! Recovering takes a crash left behind (roadmap C032). A take being recorded has a marker file
//! beside it (`record::marker_path`), made with the take and removed only once the header is
//! final. At start-up [`unfinished_takes`] lists the takes whose marker is still there and that
//! no live process holds; [`repair_take`] makes the file a plain, complete WAV again.
//!
//! Safety rules, all tested: nothing here deletes a take except [`discard_take`], which the
//! owner asks for after a second confirmation; repairing rewrites only the 44-byte header, never
//! the audio, and never shortens the file (bytes after the last whole frame stay where they are);
//! a take another running WavCraft is still recording (its marker is locked) is never listed.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::record::{HEADER_BYTES, MARKER_SUFFIX, MAX_DATA_BYTES, RecFormat, marker_path};

/// A take with a marker and no live recorder.
#[derive(Clone, Debug)]
pub struct UnfinishedTake {
    pub path: PathBuf,
    pub marker: PathBuf,
    pub fmt: RecFormat,
    /// Whole frames the file holds (what the repaired header will say).
    pub frames: u64,
    /// Whether the header on disk already says so.
    pub header_ok: bool,
}

impl UnfinishedTake {
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.fmt.rate.max(1))
    }

    /// The file's name without its folder.
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(|| "take".to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// `Take 2026-10-20 14.03.22.wav` shown as `Take 2026-10-20 14.03.22`.
    pub fn title(&self) -> String {
        self.path.file_stem().map_or_else(|| "take".to_string(), |n| n.to_string_lossy().into_owned())
    }
}

/// The format and data size a 44-byte header of our own layout holds, if it is one.
fn parse_header(b: &[u8]) -> Option<(RecFormat, u64)> {
    if b.len() < HEADER_BYTES as usize || &b[0..4] != b"RIFF" || &b[8..16] != b"WAVEfmt " || &b[36..40] != b"data" {
        return None;
    }
    let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (tag, bits) = (u16at(20), u16at(34));
    if u32at(16) != 16 || !((tag == 1 && bits != 32) || (tag == 3 && bits == 32)) {
        return None;
    }
    let fmt = RecFormat { rate: u32at(24), channels: u16at(22), bits }.validate().ok()?;
    (u64::from(u16at(32)) == fmt.block_align()).then_some((fmt, u64::from(u32at(40))))
}

/// The format written in a marker.
fn parse_marker(text: &str) -> Option<RecFormat> {
    RecFormat::parse_args(&text.lines().skip(1).collect::<Vec<_>>().join(" ")).ok()
}

/// Inspect one marker. `None` when there is nothing to offer: the take file is missing, the take
/// is live (its marker is locked), or the file is too damaged to say what it holds.
fn inspect(marker: &Path) -> Option<UnfinishedTake> {
    let name = marker.file_name()?.to_string_lossy().into_owned();
    let take = marker.with_file_name(name.strip_suffix(MARKER_SUFFIX)?);
    let mut mfile = OpenOptions::new().read(true).write(true).open(marker).or_else(|_| File::open(marker)).ok()?;
    if mfile.try_lock().is_err() {
        log::info!(target: "wavcraft", "take recovery: {} is still being recorded, left alone", take.display());
        return None;
    }
    let mut text = String::new();
    let _ = mfile.read_to_string(&mut text);
    let marker_fmt = parse_marker(&text);
    let mut f = File::open(&take).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = vec![0u8; HEADER_BYTES as usize];
    let got = f.read(&mut head).ok()?;
    let parsed = parse_header(&head[..got]);
    let fmt = parsed.map(|(f, _)| f).or(marker_fmt)?;
    let data = len.saturating_sub(HEADER_BYTES).min(MAX_DATA_BYTES);
    let whole = data - data % fmt.block_align();
    Some(UnfinishedTake {
        path: take,
        marker: marker.to_path_buf(),
        fmt,
        frames: whole / fmt.block_align(),
        header_ok: parsed.is_some_and(|(pf, d)| pf == fmt && d == whole),
    })
}

/// The takes in `dir` a crashed recording left, oldest name first. A take with no audio in it
/// only loses its marker (the file is never touched); one that cannot be read stays as it is.
pub fn unfinished_takes(dir: &Path) -> Vec<UnfinishedTake> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut markers: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(MARKER_SUFFIX)))
        .collect();
    markers.sort();
    let mut found = Vec::new();
    for m in markers {
        match inspect(&m) {
            Some(t) if t.frames == 0 && t.header_ok => {
                // Nothing was recorded: the take file stays, only the marker goes.
                let _ = std::fs::remove_file(&t.marker);
            }
            Some(t) => found.push(t),
            None => {}
        }
    }
    found
}

/// Make the take a complete WAV: the header's sizes (and format, if the header was damaged) come
/// from what is in the file, the marker goes. Only the first 44 bytes are written. Returns the
/// number of frames. Refuses a take that has become live again.
pub fn repair_take(t: &UnfinishedTake) -> io::Result<u64> {
    let again = inspect(&t.marker).ok_or_else(|| io::Error::other("the take is being recorded or can no longer be read"))?;
    let mut f = OpenOptions::new().read(true).write(true).open(&again.path)?;
    let header = again.fmt.header_for(again.frames * again.fmt.block_align());
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&header)?;
    f.sync_all()?;
    std::fs::remove_file(marker_path(&again.path))?;
    log::info!(target: "wavcraft", "take recovery: {} repaired, {} frames", again.path.display(), again.frames);
    Ok(again.frames)
}

/// Delete a take and its marker. Only for the owner's confirmed choice; never called by a sweep.
pub fn discard_take(t: &UnfinishedTake) -> io::Result<()> {
    std::fs::remove_file(&t.path)?;
    match std::fs::remove_file(&t.marker) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
