//! Save (task 06, first part: WAV). The document is streamed out 64 K frames at a time with
//! [`DocState::read`], never flattened, so a three-hour file saves on an 8 GB machine. Integer
//! samples are `round(x · 2^(bits−1))`, clamped, with no dither: the exact inverse of how they
//! are read, so untouched audio saves bit for bit. Above 4 GiB the file is RF64.
//!
//! **A save never damages the target:** it writes a temporary file in the same folder, syncs it,
//! and renames it over the target only when it is complete. Any failure removes that temporary
//! file and leaves the target as it was.
//!
//! **The temporary file is the save's own** (Codex review CX-W2/W6, 2026-10-19): a new name each
//! time, `.<name>.wavcraft-tmp-<16 random hex>`, made with exclusive create (never an existing
//! file, never through a link), locked and kept open while it is written, and removed only by
//! the save that made it. One left by a save that was killed (unlocked, untouched for a minute)
//! is removed by the next save of the same file; one whose save is still running stays.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use soundcraft_audio_io::SampleFormat;

use crate::doc::{ChunkInfo, DocState, SourceFormat};
use crate::meta::Bext;
use crate::store::BLOCK_FRAMES;

/// How samples are stored in the saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    Int16,
    Int24,
    Int32,
    Float32,
}

impl SampleType {
    fn bytes(self) -> usize {
        match self {
            SampleType::Int16 => 2,
            SampleType::Int24 => 3,
            SampleType::Int32 | SampleType::Float32 => 4,
        }
    }

    /// "16-bit", "32-bit float"…
    pub fn describe(self) -> &'static str {
        match self {
            SampleType::Int16 => "16-bit",
            SampleType::Int24 => "24-bit",
            SampleType::Int32 => "32-bit integer",
            SampleType::Float32 => "32-bit float",
        }
    }

    /// The type a save keeps for a file opened from `src`. A 32-bit integer source saves as
    /// 32-bit float: samples are held as `f32`, which can't carry all 32 bits (task 06).
    pub fn for_source(src: &SourceFormat) -> SampleType {
        match src.sample_format {
            SampleFormat::Int8 | SampleFormat::Int16 => SampleType::Int16,
            SampleFormat::Int24 => SampleType::Int24,
            SampleFormat::Int32 | SampleFormat::Float32 | SampleFormat::Float64 | SampleFormat::Compressed => SampleType::Float32,
        }
    }
}

/// Why a save failed. The target is untouched in every case.
#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    /// The user stopped the save before replacement.
    Cancelled,
    /// The document has no audio.
    Empty,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::Cancelled => write!(f, "save cancelled"),
            SaveError::Empty => write!(f, "the file has no audio to save"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(e: io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// The fixed temporary name saves used before the Codex review (`<name>.wavcraft-tmp`). Saves no
/// longer write or remove it: a file there may be another program's (CX-W2).
pub fn temp_path(target: &Path) -> PathBuf {
    let name = target.file_name().map_or_else(|| "untitled".into(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!("{name}.wavcraft-tmp"))
}

/// The start of every temporary file name a save of `target` uses.
fn temp_prefix(target: &Path) -> String {
    let name = target.file_name().map_or_else(|| "untitled".into(), |n| n.to_string_lossy().into_owned());
    format!(".{name}.wavcraft-tmp-")
}

/// The temporary files of saves of `target` now beside it (running, or left by a kill).
pub fn temp_files(target: &Path) -> Vec<PathBuf> {
    let (dir, prefix) = (target.parent().unwrap_or(Path::new(".")), temp_prefix(target));
    let Ok(entries) = std::fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) else { return Vec::new() };
    entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&prefix)).map(|e| e.path()).collect()
}

/// A leftover is removed only when nothing has written it for this long, as well as unlocked.
pub const LEFTOVER_AGE: Duration = Duration::from_secs(60);

/// A save's own temporary file: a fresh name, exclusive create, locked while it is written.
struct TempFile {
    path: PathBuf,
    file: File,
}

impl TempFile {
    fn create(target: &Path) -> io::Result<TempFile> {
        let prefix = temp_prefix(target);
        for _ in 0..16 {
            let path = target.with_file_name(format!("{prefix}{}", crate::random::hex(8)?));
            // create_new: O_CREAT|O_EXCL, which also refuses a link at that name.
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    file.try_lock().map_err(|e| io::Error::other(format!("could not lock the temporary file: {e}")))?;
                    return Ok(TempFile { path, file });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for the temporary file"))
    }
}

/// Remove the temporary files that saves of `target` left when they were killed: unlocked (the
/// system drops a dead program's locks) and untouched for [`LEFTOVER_AGE`]. Anything uncertain
/// (a link, locked, recent, unreadable) stays. Returns how many went.
pub fn remove_leftover_temps(target: &Path) -> usize {
    let mut removed = 0;
    for path in temp_files(target) {
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        let old = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).is_some_and(|age| age >= LEFTOVER_AGE);
        if !meta.is_file() || !old {
            continue;
        }
        let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else { continue };
        if file.try_lock().is_err() {
            continue; // a save is still writing it
        }
        // Holding the lock, so no save can be using it; on Windows the handle must close first.
        drop(file);
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
            log::warn!(target: "wavcraft_core::save", "decision: removed a temporary file left by a save that was stopped: {}", path.display());
        }
    }
    removed
}

/// Save `doc` to `target` as a WAV file of `ty` samples, replacing it atomically.
/// `progress` gets 0 to 1 as the samples go out.
pub fn save_wav(doc: &DocState, target: &Path, ty: SampleType, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    save_wav_inner(doc, target, ty, progress, None)
}

/// [`save_wav`] that fails with an I/O error once `fail_after` frames are out (tests: a save
/// that dies halfway must leave the target whole).
#[doc(hidden)]
pub fn save_wav_failing_after(doc: &DocState, target: &Path, ty: SampleType, fail_after: u64) -> Result<(), SaveError> {
    save_wav_inner(doc, target, ty, &mut |_| {}, Some(fail_after))
}

fn save_wav_inner(doc: &DocState, target: &Path, ty: SampleType, progress: &mut dyn FnMut(f64), fail_after: Option<u64>) -> Result<(), SaveError> {
    let frames = doc.len_frames();
    if frames == 0 {
        return Err(SaveError::Empty);
    }
    remove_leftover_temps(target);
    let tmp = TempFile::create(target).map_err(SaveError::Io)?;
    let TempFile { path: tmp, file } = tmp;
    // Written through the handle made by the exclusive create, never reopened by name.
    let (file, written) = match write_wav(doc, file, ty, progress, fail_after) {
        Ok(done) => done,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log::warn!(target: "wavcraft_core::save", "save to {} failed, the file is unchanged: {e}", target.display());
            return Err(SaveError::Io(e));
        }
    };
    if let Err(e) = std::fs::rename(&tmp, target) {
        drop(file);
        // The Windows dev machine's review (2026-10-19): a failed save leaves nothing behind. The target is
        // untouched (the rename did not happen) and this save's temporary file goes too.
        let _ = std::fs::remove_file(&tmp);
        log::warn!(target: "wavcraft_core::save", "could not replace {}, the file is unchanged: {e}", target.display());
        return Err(SaveError::Io(io::Error::new(e.kind(), format!("could not replace the file ({e}); it is unchanged"))));
    }
    drop(file);
    // The file on disk now holds the chunks at new places: later saves copy from there.
    doc.source().relocated.set(target.to_path_buf(), written);
    log::info!(target: "wavcraft_core::save", "saved {} frames as {} WAV to {}", frames, ty.describe(), target.display());
    Ok(())
}

/// One chunk of the saved file after `fmt `/`fact`, in the order it is written.
enum Item {
    /// The samples (written fresh from the document).
    Data,
    /// A chunk the app owns and writes fresh: `bext`, `LIST`/`INFO`.
    Fresh { id: [u8; 4], body: Vec<u8> },
    /// A chunk of the source file, streamed through unchanged.
    Copy(ChunkInfo),
}

/// Chunks the app writes itself or that a save makes stale, so never copied from the source:
/// the format and size chunks, padding, the peak envelope (the audio changed), and the cue list
/// and `adtl` labels (the marker code owns them).
const NOT_COPIED: [&[u8; 4]; 7] = [b"fmt ", b"fact", b"ds64", b"JUNK", b"PEAK", b"cue ", b"data"];

/// The EBU R98 coding-history line for a save.
fn coding_line(ty: SampleType, rate: u32, channels: usize) -> String {
    let mode = match channels {
        1 => "mono",
        2 => "stereo",
        _ => "multi",
    };
    format!("A=PCM,F={rate},W={},M={mode},T=WavCraft", ty.bytes() * 8)
}

/// A chunk of the source is copied only if the file still has it where the table says.
fn source_chunk_is_intact(f: &mut File, len: u64, c: &ChunkInfo) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let mut head = [0u8; 8];
    c.offset.checked_add(8).and_then(|b| b.checked_add(c.size)).is_some_and(|end| end <= len)
        && f.seek(SeekFrom::Start(c.offset)).is_ok()
        && f.read_exact(&mut head).is_ok()
        && head[..4] == c.id
        && u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])) == c.size
}

/// The chunks after `fmt `/`fact` (Roadmap A272, A265, A256): the source's chunks in their
/// original order, the app's own (`data`, `bext`, an edited `LIST`/`INFO`) written fresh in
/// their place, and the source file itself kept open to stream the rest from.
fn plan_chunks(doc: &DocState, ty: SampleType) -> (Vec<Item>, Option<File>) {
    let src = doc.source();
    let meta = &src.meta;
    let (path, chunks) = src.relocated.get().unwrap_or_else(|| (src.path.clone(), src.chunks.clone()));
    let usable = src.format == soundcraft_audio_io::FileFormat::Wav && !path.as_os_str().is_empty();
    let mut file = if usable { File::open(&path).ok() } else { None };
    let len = file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
    let bext = meta.bext.as_ref().map(|b| {
        let history = Bext::with_history_line(&b.coding_history, &coding_line(ty, doc.sample_rate(), doc.channel_count()));
        Item::Fresh { id: *b"bext", body: b.to_bytes(b.time_reference.saturating_add(doc.start_shift()), &history) }
    });
    let info = (!meta.info.0.is_empty()).then(|| Item::Fresh { id: *b"LIST", body: meta.info.to_bytes() });
    let has_ixml_chunk = chunks.iter().any(|c| c.id == *b"iXML");
    let mut ixml = (meta.chnames_edited && (meta.ixml.is_some() || !has_ixml_chunk))
        .then(|| Item::Fresh { id: *b"iXML", body: crate::meta::ixml_with_names(meta.ixml.as_deref(), &meta.chnames) });
    let (mut bext, mut info) = (bext, info);
    let mut plan = Vec::new();
    let (mut have_data, mut have_info) = (false, false);
    for c in &chunks {
        let Some(f) = file.as_mut() else { break };
        if c.id == *b"data" {
            if !have_data {
                plan.push(Item::Data);
                have_data = true;
            }
            continue;
        }
        if NOT_COPIED.contains(&&c.id) {
            continue;
        }
        if !source_chunk_is_intact(f, len, c) {
            log::warn!(target: "wavcraft_core::save", "the {} chunk is no longer in the source file where it was; it is not kept", c.id_str());
            continue;
        }
        if c.id == *b"bext" && meta.bext.is_some() {
            plan.extend(bext.take());
        } else if c.id == *b"iXML" && meta.ixml.is_some() {
            if meta.chnames_edited {
                plan.extend(ixml.take());
            } else {
                plan.push(Item::Copy(c.clone()));
            }
        } else if c.id == *b"LIST" && is_list(f, c, b"INFO") {
            if meta.info_edited {
                if !have_info {
                    plan.extend(info.take());
                }
            } else {
                plan.push(Item::Copy(c.clone()));
            }
            have_info = true;
        } else if c.id == *b"LIST" && is_list(f, c, b"adtl") {
            // The marker code owns the labels with the cue list.
        } else {
            plan.push(Item::Copy(c.clone()));
        }
    }
    if let Some(b) = bext.take() {
        plan.insert(0, b);
    }
    if !have_data {
        plan.push(Item::Data);
    }
    if !have_info {
        plan.extend(info.take());
    }
    plan.extend(ixml.take());
    (plan, file)
}

/// Whether the `LIST` chunk `c` has the form type `form`.
fn is_list(f: &mut File, c: &ChunkInfo, form: &[u8; 4]) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let mut t = [0u8; 4];
    c.size >= 4 && f.seek(SeekFrom::Start(c.offset + 8)).is_ok() && f.read_exact(&mut t).is_ok() && &t == form
}

fn write_wav(
    doc: &DocState,
    file: File,
    ty: SampleType,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> io::Result<(File, Vec<ChunkInfo>)> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels for WAV"))?;
    let (plan, mut source) = plan_chunks(doc, ty);
    let extra: u64 = plan
        .iter()
        .map(|i| match i {
            Item::Data => 0,
            Item::Fresh { body, .. } => 8 + body.len() as u64 + (body.len() as u64 & 1),
            Item::Copy(c) => 8 + c.size + (c.size & 1),
        })
        .sum();
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let (data_len, mut pos) = write_header(&mut out, ch16, doc.sample_rate(), ty, frames, extra)?;
    let mut table = Vec::new();
    for item in &plan {
        match item {
            Item::Data => {
                table.push(ChunkInfo { id: *b"data", offset: pos, size: data_len });
                out.write_all(b"data")?;
                out.write_all(&u32::try_from(data_len).unwrap_or(u32::MAX).to_le_bytes())?;
                write_samples(&mut out, doc, ty, progress, fail_after)?;
                if data_len & 1 == 1 {
                    out.write_all(&[0])?;
                }
                pos += 8 + data_len + (data_len & 1);
            }
            Item::Fresh { id, body } => {
                table.push(ChunkInfo { id: *id, offset: pos, size: body.len() as u64 });
                out.write_all(id)?;
                out.write_all(&(body.len() as u32).to_le_bytes())?;
                out.write_all(body)?;
                if body.len() & 1 == 1 {
                    out.write_all(&[0])?;
                }
                pos += 8 + body.len() as u64 + (body.len() as u64 & 1);
            }
            Item::Copy(c) => {
                use std::io::{Read, Seek, SeekFrom};
                let f = source.as_mut().ok_or_else(|| io::Error::other("the source file went away"))?;
                f.seek(SeekFrom::Start(c.offset + 8))?;
                table.push(ChunkInfo { id: c.id, offset: pos, size: c.size });
                out.write_all(&c.id)?;
                out.write_all(&(c.size as u32).to_le_bytes())?;
                let copied = io::copy(&mut f.take(c.size), &mut out)?;
                if copied != c.size {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!("the {} chunk of the source file changed while saving", c.id_str()),
                    ));
                }
                if c.size & 1 == 1 {
                    out.write_all(&[0])?;
                }
                pos += 8 + c.size + (c.size & 1);
            }
        }
    }
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok((file, table))
}

/// The samples, 64 K frames at a time.
fn write_samples(out: &mut impl Write, doc: &DocState, ty: SampleType, progress: &mut dyn FnMut(f64), fail_after: Option<u64>) -> io::Result<()> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let mut bufs = vec![vec![0f32; BLOCK_FRAMES]; channels];
    let mut bytes = Vec::with_capacity(BLOCK_FRAMES * channels * ty.bytes());
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(BLOCK_FRAMES as u64) as usize;
        for (ch, buf) in bufs.iter_mut().enumerate() {
            let got = doc.read(ch, at, &mut buf[..n])?;
            if got != n {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("channel {ch} gave {got} of {n} frames at {at}")));
            }
        }
        bytes.clear();
        for i in 0..n {
            for buf in &bufs {
                let x = buf[i];
                match ty {
                    SampleType::Int16 => bytes.extend_from_slice(&(quantize(x, 16) as i16).to_le_bytes()),
                    SampleType::Int24 => bytes.extend_from_slice(&quantize(x, 24).to_le_bytes()[..3]),
                    SampleType::Int32 => bytes.extend_from_slice(&quantize(x, 32).to_le_bytes()),
                    SampleType::Float32 => bytes.extend_from_slice(&x.to_le_bytes()),
                }
            }
        }
        out.write_all(&bytes)?;
        at += n as u64;
        if fail_after.is_some_and(|f| at >= f && at < frames) {
            return Err(io::Error::other("test: the disk went away"));
        }
        progress(at as f64 / frames as f64);
    }
    Ok(())
}

/// The RIFF (or, above 4 GiB, RF64) header, `fmt ` and (float) `fact`; returns the data size
/// and the file offset the next chunk starts at. `extra` is the bytes of every other chunk
/// (headers and pads included) that follow; the `data` chunk is counted here.
fn write_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64, extra: u64) -> io::Result<(u64, u64)> {
    let bytes = ty.bytes() as u64;
    let data_len = frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("WAV too long"))?;
    let float = ty == SampleType::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let fact = if float { 12 } else { 0 };
    let riff_len = 4 + (8 + u64::from(fmt_len)) + fact + 8 + data_len + (data_len & 1) + extra;
    let rf64 = u32::try_from(riff_len).is_err();
    let block_align = channels.checked_mul(bytes as u16).ok_or_else(|| invalid("too many channels"))?;
    if rf64 {
        // ds64 (EBU Tech 3306): RIFF size, data size, sample count (frames), table length.
        out.write_all(b"RF64")?;
        out.write_all(&u32::MAX.to_le_bytes())?;
        out.write_all(b"WAVEds64")?;
        out.write_all(&28u32.to_le_bytes())?;
        out.write_all(&(riff_len + 36).to_le_bytes())?;
        out.write_all(&data_len.to_le_bytes())?;
        out.write_all(&frames.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"fmt ")?;
    } else {
        out.write_all(b"RIFF")?;
        out.write_all(&(riff_len as u32).to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
    }
    out.write_all(&fmt_len.to_le_bytes())?;
    out.write_all(&(if float { 3u16 } else { 1u16 }).to_le_bytes())?;
    out.write_all(&channels.to_le_bytes())?;
    out.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate.checked_mul(u32::from(block_align)).ok_or_else(|| invalid("sample rate too high"))?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&(bytes as u16 * 8).to_le_bytes())?;
    if float {
        out.write_all(&0u16.to_le_bytes())?;
        out.write_all(b"fact")?;
        out.write_all(&4u32.to_le_bytes())?;
        out.write_all(&u32::try_from(frames).unwrap_or(u32::MAX).to_le_bytes())?;
    }
    let pos = if rf64 { 12 + 36 } else { 12 } + 8 + u64::from(fmt_len) + fact;
    Ok((data_len, pos))
}

/// `x` (full scale ±1) as a `bits`-bit integer, rounded and clamped; NaN is 0.
pub fn quantize(x: f32, bits: u32) -> i32 {
    let scale = (1u64 << (bits - 1)) as f64;
    let x = if x.is_nan() { 0.0 } else { f64::from(x) };
    (x * scale).round().clamp(-scale, scale - 1.0) as i32
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.to_string())
}

/// C186-a: cancellable whole-document WAV save; the builder fills this stub.
#[allow(clippy::todo)] // card wav-c-186-a removes this with the stub
pub fn save_wav_controlled(
    _doc: &DocState,
    _target: &Path,
    _ty: SampleType,
    _progress: &mut dyn FnMut(f64),
    _cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), SaveError> {
    todo!("C186-a")
}

/// C186-b: marker chunks are owned payloads, without RIFF headers or padding.
#[derive(Debug, Clone)]
pub struct SaveOptions {
    pub include_markers: bool,
    pub marker_chunks: Vec<([u8; 4], Vec<u8>)>,
}

#[allow(clippy::todo)] // card wav-c-186-b removes this with the stub
impl Default for SaveOptions {
    fn default() -> Self {
        todo!("C186-b")
    }
}

/// C186-b: the same save, with an explicit marker-inclusion option.
#[allow(clippy::todo)] // card wav-c-186-b removes this with the stub
pub fn save_wav_with_options(
    _doc: &DocState,
    _target: &Path,
    _ty: SampleType,
    _options: &SaveOptions,
    _progress: &mut dyn FnMut(f64),
    _cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), SaveError> {
    todo!("C186-b")
}

#[cfg(test)]
mod c186_locked {
    use super::*;
    use crate::doc::{Channel, Piece};
    use crate::store::{Store, StoreOptions};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("wavcraft-c186-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn doc(&self, frames: usize) -> DocState {
            let store = Store::create_with(&self.0.join("store"), StoreOptions::default()).unwrap();
            let pieces = vec![0.5; frames]
                .chunks(BLOCK_FRAMES)
                .map(|b| Piece::whole(store.append(b).unwrap()).unwrap())
                .collect::<Vec<_>>();
            DocState::new(store, vec![Channel::from_pieces(pieces)], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn chunks(path: &Path) -> Vec<([u8; 4], Vec<u8>)> {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..12], b"WAVE");
        let mut at = 12;
        let mut out = Vec::new();
        while at < bytes.len() {
            let id = bytes[at..at + 4].try_into().unwrap();
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            out.push((id, bytes[at + 8..at + 8 + len].to_vec()));
            if len & 1 == 1 {
                assert_eq!(bytes[at + 8 + len], 0);
            }
            at += 8 + len + (len & 1);
        }
        assert_eq!(at, bytes.len());
        out
    }

    // Literal RIFF payloads: cue id 7 at sample 1, label A.
    fn marker_chunks() -> Vec<([u8; 4], Vec<u8>)> {
        vec![
            (*b"cue ", vec![1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 100, 97, 116, 97, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (*b"LIST", vec![97, 100, 116, 108, 108, 97, 98, 108, 6, 0, 0, 0, 7, 0, 0, 0, 65, 0]),
        ]
    }

    #[test]
    fn a_cancel_before_start_preserves_target_and_relocation() {
        let t = Scratch::new("pre");
        let doc = t.doc(4);
        let path = t.0.join("target.wav");
        std::fs::write(&path, b"original").unwrap();
        let mut progress = Vec::new();
        let result = save_wav_controlled(&doc, &path, SampleType::Int16, &mut |p| progress.push(p), &AtomicBool::new(true));
        assert!(matches!(result, Err(SaveError::Cancelled)));
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(progress, Vec::<f64>::new());
        assert_eq!(temp_files(&path), Vec::<PathBuf>::new());
        assert_eq!(doc.source().relocated.get(), None);
    }

    #[test]
    fn a_cancel_after_first_block_preserves_existing_target() {
        let t = Scratch::new("mid");
        let doc = t.doc(131_072);
        let path = t.0.join("target.wav");
        std::fs::write(&path, b"original").unwrap();
        let cancel = AtomicBool::new(false);
        let mut progress = Vec::new();
        let result = save_wav_controlled(&doc, &path, SampleType::Int16, &mut |p| {
            progress.push(p);
            cancel.store(true, Ordering::Relaxed);
        }, &cancel);
        assert!(matches!(result, Err(SaveError::Cancelled)));
        assert_eq!(progress, vec![0.5]);
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(temp_files(&path), Vec::<PathBuf>::new());
        assert_eq!(doc.source().relocated.get(), None);
    }

    #[test]
    fn a_cancel_at_last_callback_does_not_create_target() {
        let t = Scratch::new("last");
        let doc = t.doc(4);
        let path = t.0.join("target.wav");
        let cancel = AtomicBool::new(false);
        let mut progress = Vec::new();
        let result = save_wav_controlled(&doc, &path, SampleType::Int16, &mut |p| {
            progress.push(p);
            cancel.store(true, Ordering::Relaxed);
        }, &cancel);
        assert!(matches!(result, Err(SaveError::Cancelled)));
        assert_eq!(progress, vec![1.0]);
        assert!(!path.exists());
        assert_eq!(temp_files(&path), Vec::<PathBuf>::new());
        assert_eq!(doc.source().relocated.get(), None);
    }

    #[test]
    fn a_success_reports_each_block_and_writes_exact_pcm() {
        let t = Scratch::new("success");
        let doc = t.doc(131_072);
        let path = t.0.join("target.wav");
        let mut progress = Vec::new();
        save_wav_controlled(&doc, &path, SampleType::Int16, &mut |p| progress.push(p), &AtomicBool::new(false)).unwrap();
        assert_eq!(progress, vec![0.5, 1.0]);
        let out = chunks(&path);
        assert_eq!(out.iter().map(|c| c.0).collect::<Vec<_>>(), vec![*b"fmt ", *b"data"]);
        assert_eq!(out[1].1, [0, 64].repeat(131_072));
        assert_eq!(temp_files(&path), Vec::<PathBuf>::new());
        assert_eq!(doc.source().relocated.get().unwrap().0, path);
    }

    #[test]
    fn b_default_includes_markers() {
        let options = SaveOptions::default();
        assert!(options.include_markers);
        assert_eq!(options.marker_chunks, Vec::<([u8; 4], Vec<u8>)>::new());
        let t = Scratch::new("on");
        let doc = t.doc(4);
        let path = t.0.join("target.wav");
        let options = SaveOptions { marker_chunks: marker_chunks(), ..options };
        save_wav_with_options(&doc, &path, SampleType::Int16, &options, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let out = chunks(&path);
        assert_eq!(out.iter().map(|c| c.0).collect::<Vec<_>>(), vec![*b"fmt ", *b"data", *b"cue ", *b"LIST"]);
        assert_eq!(out[1].1, vec![0, 64, 0, 64, 0, 64, 0, 64]);
        assert_eq!(out[2].1, vec![1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 100, 97, 116, 97, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(out[3].1, vec![97, 100, 116, 108, 108, 97, 98, 108, 6, 0, 0, 0, 7, 0, 0, 0, 65, 0]);
    }

    #[test]
    fn b_off_keeps_info_audio_and_input_markers() {
        let t = Scratch::new("off");
        let doc = t.doc(4);
        let mut meta = crate::meta::Metadata::default();
        meta.info.0.push((*b"INAM", b"T".to_vec()));
        let doc = doc.with_metadata(meta);
        let path = t.0.join("target.wav");
        let options = SaveOptions { include_markers: false, marker_chunks: marker_chunks() };
        save_wav_with_options(&doc, &path, SampleType::Int16, &options, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let out = chunks(&path);
        assert_eq!(out.iter().map(|c| c.0).collect::<Vec<_>>(), vec![*b"fmt ", *b"data", *b"LIST"]);
        assert_eq!(out[1].1, vec![0, 64, 0, 64, 0, 64, 0, 64]);
        assert_eq!(out[2].1, vec![73, 78, 70, 79, 73, 78, 65, 77, 2, 0, 0, 0, 84, 0]);
        assert!(!options.include_markers);
        assert_eq!(options.marker_chunks, marker_chunks());
    }

    #[test]
    fn b_options_save_honors_cancellation() {
        let t = Scratch::new("options-cancel");
        let doc = t.doc(4);
        let path = t.0.join("target.wav");
        std::fs::write(&path, b"original").unwrap();
        let cancel = AtomicBool::new(false);
        let options = SaveOptions { include_markers: true, marker_chunks: marker_chunks() };
        let result = save_wav_with_options(&doc, &path, SampleType::Int16, &options, &mut |_| cancel.store(true, Ordering::Relaxed), &cancel);
        assert!(matches!(result, Err(SaveError::Cancelled)));
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(temp_files(&path), Vec::<PathBuf>::new());
        assert_eq!(doc.source().relocated.get(), None);
    }
}
