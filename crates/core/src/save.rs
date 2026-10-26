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
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use soundcraft_audio_io::SampleFormat;

use crate::doc::edit::ChannelMask;
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
    /// The document has no audio.
    Empty,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
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
    fn create(target: &Path, owner_only: bool) -> io::Result<TempFile> {
        let prefix = temp_prefix(target);
        for _ in 0..16 {
            let path = target.with_file_name(format!("{prefix}{}", crate::random::hex(8)?));
            // create_new: O_CREAT|O_EXCL, which also refuses a link at that name.
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            if owner_only {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            #[cfg(not(unix))]
            let _ = owner_only; // Windows: the file inherits its folder's access list
            match opts.open(&path) {
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
    save_wav_inner(doc, target, ty, &Part::whole(doc), progress, None, true)
}

/// A frame range and channel subset to write from a document.
#[derive(Debug, Clone)]
pub struct Part {
    pub frames: Range<u64>,
    pub channels: Vec<usize>,
}

impl Part {
    pub fn whole(doc: &DocState) -> Part {
        Part { frames: 0..doc.len_frames(), channels: (0..doc.channel_count()).collect() }
    }
    pub fn range(doc: &DocState, frames: Range<u64>, mask: ChannelMask) -> Part {
        let end = frames.end.min(doc.len_frames());
        Part { frames: frames.start.min(end)..end, channels: mask.active(doc.channel_count()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Wav(SampleType),
}
impl FileKind {
    pub fn describe(self) -> String {
        match self {
            FileKind::Wav(ty) => format!("{} WAV", ty.describe()),
        }
    }
}

/// Write a document part without changing the open document's save path.
pub fn write_file(doc: &DocState, target: &Path, kind: FileKind, part: &Part, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    match kind {
        FileKind::Wav(ty) => save_wav_inner(doc, target, ty, part, progress, None, false),
    }
}

#[doc(hidden)]
pub fn write_failing_after(doc: &DocState, target: &Path, ty: SampleType, part: &Part, fail_after: u64) -> Result<(), SaveError> {
    save_wav_inner(doc, target, ty, part, &mut |_| {}, Some(fail_after), false)
}

/// [`save_wav`] that fails with an I/O error once `fail_after` frames are out (tests: a save
/// that dies halfway must leave the target whole).
#[doc(hidden)]
pub fn save_wav_failing_after(doc: &DocState, target: &Path, ty: SampleType, fail_after: u64) -> Result<(), SaveError> {
    save_wav_inner(doc, target, ty, &Part::whole(doc), &mut |_| {}, Some(fail_after), false)
}

fn save_wav_inner(
    doc: &DocState,
    target: &Path,
    ty: SampleType,
    part: &Part,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
    relocate: bool,
) -> Result<(), SaveError> {
    if part.frames.start > part.frames.end
        || part.frames.end > doc.len_frames()
        || part.channels.iter().any(|&ch| ch >= doc.channel_count())
    {
        return Err(SaveError::Io(io::Error::new(io::ErrorKind::InvalidInput, "invalid document part")));
    }
    let frames = part.frames.end.saturating_sub(part.frames.start);
    if frames == 0 || part.channels.is_empty() {
        return Err(SaveError::Empty);
    }
    remove_leftover_temps(target);
    let tmp = TempFile::create(target, false).map_err(SaveError::Io)?;
    let TempFile { path: tmp, file } = tmp;
    // Written through the handle made by the exclusive create, never reopened by name.
    let (file, written) = match write_wav(doc, file, ty, part, progress, fail_after) {
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
    if relocate {
        doc.source().relocated.set(target.to_path_buf(), written);
    }
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
    part: &Part,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> io::Result<(File, Vec<ChunkInfo>)> {
    let frames = part.frames.end - part.frames.start;
    let channels = part.channels.len();
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels for WAV"))?;
    let whole = part.frames.start == 0
        && part.frames.end == doc.len_frames()
        && part.channels.iter().copied().eq(0..doc.channel_count());
    let (plan, mut source) = if whole { plan_chunks(doc, ty) } else { (vec![Item::Data], None) };
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
                write_samples(&mut out, doc, ty, part, progress, fail_after)?;
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
fn write_samples(
    out: &mut impl Write,
    doc: &DocState,
    ty: SampleType,
    part: &Part,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> io::Result<()> {
    let frames = part.frames.end - part.frames.start;
    let channels = part.channels.len();
    let mut bufs = vec![vec![0f32; BLOCK_FRAMES]; channels];
    let mut bytes = Vec::with_capacity(BLOCK_FRAMES * channels * ty.bytes());
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(BLOCK_FRAMES as u64) as usize;
        for (&ch, buf) in part.channels.iter().zip(bufs.iter_mut()) {
            let got = doc.read(ch, part.frames.start + at, &mut buf[..n])?;
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

/// Write `bytes` to `target` with the same care as a WAV save (f117, the session file): a fresh
/// exclusive temporary file beside it (owner-only on Unix when `owner_only`), written, synced to
/// disk, then renamed over the target; the folder is synced after. Any failure removes the
/// temporary file and leaves the target as it was.
pub fn write_file_atomic(target: &Path, bytes: &[u8], owner_only: bool) -> io::Result<()> {
    remove_leftover_temps(target);
    let TempFile { path: tmp, mut file } = TempFile::create(target, owner_only)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        return Err(io::Error::new(e.kind(), format!("could not replace {} ({e}); it is unchanged", target.display())));
    }
    drop(file);
    #[cfg(unix)]
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all(); // best effort: the rename itself is done
    }
    Ok(())
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
