//! Save (task 06; WAV, AIFF and FLAC). The document is streamed out 64 K frames at a time with
//! [`DocState::read`], never flattened, so a three-hour file saves on an 8 GB machine. Integer
//! samples are `round(x · 2^(bits−1))`, clamped: the exact inverse of how they are read, so
//! untouched audio saves bit for bit. Dither and noise shaping ([`crate::dither`]) are applied
//! only when the document's [`SourceFormat::dither`] asks for them (Convert Sample Type), and
//! only to the integer depths. Above 4 GiB the file is RF64.
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

use crate::dither::Ditherer;
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
    /// The format cannot hold what was asked (FLAC and 32-bit samples).
    Unsupported(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::Empty => write!(f, "the file has no audio to save"),
            SaveError::Unsupported(why) => write!(f, "{why}"),
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

/// What a write takes from the document: a frame range and some channels (f43: Save Selection
/// As and Export write part of a document; Save writes all of it).
#[derive(Debug, Clone)]
pub struct Part {
    pub frames: Range<u64>,
    pub channels: Vec<usize>,
}

impl Part {
    /// All frames and channels of `doc`.
    pub fn whole(doc: &DocState) -> Part {
        Part { frames: 0..doc.len_frames(), channels: (0..doc.channel_count()).collect() }
    }

    /// `frames` of the channels in `mask` (clamped to the document).
    pub fn range(doc: &DocState, frames: Range<u64>, mask: ChannelMask) -> Part {
        let end = frames.end.min(doc.len_frames());
        Part { frames: frames.start.min(end)..end, channels: mask.active(doc.channel_count()) }
    }
}

/// The file formats a write can produce. **The one place the format is chosen**: a new format
/// is a variant here and an arm in [`write_file`].
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

impl Part {
    /// Whether this is all frames and all channels of `doc` (a plain Save).
    pub fn is_whole(&self, doc: &DocState) -> bool {
        self.frames == (0..doc.len_frames()) && self.channels.iter().copied().eq(0..doc.channel_count())
    }
}

/// The file formats a save can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    Wav,
    /// Big-endian PCM; 32-bit float is AIFF-C `fl32`.
    Aiff,
    /// 16 or 24 bit only.
    Flac,
}

impl SaveFormat {
    /// The format a file name's extension asks for (`wav`, `aif`, `aiff`, `flac`; any case).
    pub fn from_path(path: &Path) -> Option<SaveFormat> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "wav" => Some(SaveFormat::Wav),
            "aif" | "aiff" => Some(SaveFormat::Aiff),
            "flac" => Some(SaveFormat::Flac),
            _ => None,
        }
    }

    /// The format of a source file, when a save can write it.
    pub fn from_file_format(f: soundcraft_audio_io::FileFormat) -> Option<SaveFormat> {
        match f {
            soundcraft_audio_io::FileFormat::Wav => Some(SaveFormat::Wav),
            soundcraft_audio_io::FileFormat::Aiff => Some(SaveFormat::Aiff),
            soundcraft_audio_io::FileFormat::Flac => Some(SaveFormat::Flac),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SaveFormat::Wav => "WAV",
            SaveFormat::Aiff => "AIFF",
            SaveFormat::Flac => "FLAC",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Wav => "wav",
            SaveFormat::Aiff => "aiff",
            SaveFormat::Flac => "flac",
        }
    }

    /// Whether this format can hold `ty` samples.
    pub fn accepts(self, ty: SampleType) -> bool {
        match self {
            SaveFormat::Wav | SaveFormat::Aiff => true,
            SaveFormat::Flac => matches!(ty, SampleType::Int16 | SampleType::Int24),
        }
    }

    /// `ty` if this format holds it, else the nearest type it does (FLAC: 24-bit).
    pub fn fit(self, ty: SampleType) -> SampleType {
        if self.accepts(ty) { ty } else { SampleType::Int24 }
    }
}

/// Test hooks: stop with an I/O error after some frames, and the RIFF size above which a WAV
/// becomes RF64 (4 GiB; tests lower it).
#[derive(Clone, Copy)]
struct Hooks {
    fail_after: Option<u64>,
    rf64_over: u64,
    /// Later saves copy the source's chunks from the file just written (false for a copy of the
    /// document written elsewhere: Export, Save Selection As).
    relocate: bool,
}

const NO_HOOKS: Hooks = Hooks { fail_after: None, rf64_over: u32::MAX as u64, relocate: true };

/// Save `doc` to `target` as a WAV file of `ty` samples, replacing it atomically.
/// `progress` gets 0 to 1 as the samples go out.
pub fn save_wav(doc: &DocState, target: &Path, ty: SampleType, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    save(doc, target, SaveFormat::Wav, ty, progress)
}

/// Save `doc` to `target` in `format` with `ty` samples, replacing it atomically. A failure of
/// any kind removes this save's temporary file and leaves `target` as it was.
pub fn save(doc: &DocState, target: &Path, format: SaveFormat, ty: SampleType, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    save_inner(doc, target, format, ty, &Part::whole(doc), progress, NO_HOOKS)
}

/// [`save_wav`] that fails with an I/O error once `fail_after` frames are out (tests: a save
/// that dies halfway must leave the target whole).
#[doc(hidden)]
pub fn save_wav_failing_after(doc: &DocState, target: &Path, ty: SampleType, fail_after: u64) -> Result<(), SaveError> {
    save_failing_after(doc, target, SaveFormat::Wav, ty, fail_after)
}

/// Write `part` of `doc` to `target` as `kind`, through the save path's safety: an exclusive
/// owner-only temporary file beside the target, synced before the rename, the target never
/// lost. The document itself is only read, and what was saved is not remembered as the source
/// of later saves (a copy, not the document's file).
pub fn write_file(doc: &DocState, target: &Path, kind: FileKind, part: &Part, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    match kind {
        FileKind::Wav(ty) => save_inner(doc, target, SaveFormat::Wav, ty, part, progress, Hooks { relocate: false, ..NO_HOOKS }),
    }
}

/// [`write_file`] for WAV that fails with an I/O error once `fail_after` frames are out (tests).
#[doc(hidden)]
pub fn write_failing_after(doc: &DocState, target: &Path, ty: SampleType, part: &Part, fail_after: u64) -> Result<(), SaveError> {
    save_inner(doc, target, SaveFormat::Wav, ty, part, &mut |_| {}, Hooks { fail_after: Some(fail_after), relocate: false, ..NO_HOOKS })
}

/// [`save`] that fails with an I/O error once `fail_after` frames are out.
#[doc(hidden)]
pub fn save_failing_after(doc: &DocState, target: &Path, format: SaveFormat, ty: SampleType, fail_after: u64) -> Result<(), SaveError> {
    save_inner(doc, target, format, ty, &Part::whole(doc), &mut |_| {}, Hooks { fail_after: Some(fail_after), ..NO_HOOKS })
}

/// [`save_wav`] with the RIFF size above which the file is written as RF64 set to `threshold`
/// bytes instead of 4 GiB (tests: the RF64 writer without an 8 GB file).
#[doc(hidden)]
pub fn save_wav_rf64_over(doc: &DocState, target: &Path, ty: SampleType, threshold: u64) -> Result<(), SaveError> {
    save_inner(doc, target, SaveFormat::Wav, ty, &Part::whole(doc), &mut |_| {}, Hooks { rf64_over: threshold, ..NO_HOOKS })
}

fn save_inner(
    doc: &DocState,
    target: &Path,
    format: SaveFormat,
    ty: SampleType,
    part: &Part,
    progress: &mut dyn FnMut(f64),
    hooks: Hooks,
) -> Result<(), SaveError> {
    let frames = part.frames.end.saturating_sub(part.frames.start);
    if frames == 0 || part.channels.is_empty() {
        return Err(SaveError::Empty);
    }
    if !format.accepts(ty) {
        return Err(SaveError::Unsupported(format!("{} cannot hold {} samples", format.name(), ty.describe())));
    }
    remove_leftover_temps(target);
    let tmp = TempFile::create(target).map_err(SaveError::Io)?;
    let TempFile { path: tmp, file } = tmp;
    // Written through the handle made by the exclusive create, never reopened by name.
    if format != SaveFormat::Wav && !part.is_whole(doc) {
        let _ = std::fs::remove_file(&tmp);
        return Err(SaveError::Unsupported(format!("{} can only be written whole", format.name())));
    }
    let (file, written) = match if format == SaveFormat::Wav {
        write_wav(doc, file, ty, part, progress, hooks).map(|(f, table)| (f, Some(table)))
    } else {
        write_stream(doc, file, format, ty, progress, hooks).map(|f| (f, None))
    } {
        Ok(done) => done,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log::warn!(target: "wavcraft_core::save", "save to {} failed, the file is unchanged: {e}", target.display());
            return Err(SaveError::Io(e));
        }
    };
    if let Err(e) = std::fs::rename(&tmp, target) {
        drop(file);
        // A failed save leaves nothing behind. The target is untouched (the rename did not
        // happen) and this save's temporary file goes too.
        let _ = std::fs::remove_file(&tmp);
        log::warn!(target: "wavcraft_core::save", "could not replace {}, the file is unchanged: {e}", target.display());
        return Err(SaveError::Io(io::Error::new(e.kind(), format!("could not replace the file ({e}); it is unchanged"))));
    }
    drop(file);
    if let (Some(table), true) = (written, hooks.relocate && part.is_whole(doc)) {
        // The file on disk now holds the chunks at new places: later saves copy from there.
        doc.source().relocated.set(target.to_path_buf(), table);
    }
    log::info!(target: "wavcraft_core::save", "saved {} frames as {} {} to {}", frames, ty.describe(), format.name(), target.display());
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
    hooks: Hooks,
) -> io::Result<(File, Vec<ChunkInfo>)> {
    // A part (Save Selection As) carries none of the source's other chunks or markers: they
    // describe the whole file.
    let whole = part.is_whole(doc);
    let (frames, channels) = (part.frames.end - part.frames.start, part.channels.len());
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels for WAV"))?;
    let (plan, mut source) = if whole { plan_chunks(doc, ty) } else { (vec![Item::Data], None) };
    // Markers go after everything else as `cue ` and `LIST`/`adtl` chunks (A183); the source's own
    // copies are not carried over (see `NOT_COPIED`, `plan_chunks`).
    let marker_chunks = if whole { doc.markers().to_wav_chunks() } else { Vec::new() };
    let extra: u64 = marker_chunks.len() as u64
        + plan
            .iter()
            .map(|i| match i {
                Item::Data => 0,
                Item::Fresh { body, .. } => 8 + body.len() as u64 + (body.len() as u64 & 1),
                Item::Copy(c) => 8 + c.size + (c.size & 1),
            })
            .sum::<u64>();
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let (data_len, mut pos) = write_header(&mut out, ch16, doc.sample_rate(), ty, frames, extra, hooks.rf64_over)?;
    let mut table = Vec::new();
    for item in &plan {
        match item {
            Item::Data => {
                table.push(ChunkInfo { id: *b"data", offset: pos, size: data_len });
                out.write_all(b"data")?;
                out.write_all(&u32::try_from(data_len).unwrap_or(u32::MAX).to_le_bytes())?;
                write_samples(&mut out, doc, ty, part, SaveFormat::Wav, None, progress, hooks.fail_after)?;
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
    out.write_all(&marker_chunks)?;
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok((file, table))
}

/// Stream the document into `file` as AIFF or FLAC; durable (synced) before it returns. (A WAV is
/// written by [`write_wav`], which also copies the source's other chunks through.)
fn write_stream(doc: &DocState, file: File, format: SaveFormat, ty: SampleType, progress: &mut dyn FnMut(f64), hooks: Hooks) -> io::Result<File> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let mut flac = None;
    let data_len = match format {
        SaveFormat::Wav => return Err(invalid("a WAV is written by write_wav")),
        SaveFormat::Aiff => {
            let ch16 = i16::try_from(channels).map_err(|_| invalid("too many channels for AIFF"))?;
            write_aiff_header(&mut out, ch16, doc.sample_rate(), ty, frames)?
        }
        SaveFormat::Flac => {
            let bps = if ty == SampleType::Int16 { 16 } else { 24 };
            flac = Some(crate::flac_enc::Encoder::new(&mut out, channels, doc.sample_rate(), bps, frames)?);
            0
        }
    };
    write_samples(&mut out, doc, ty, &Part::whole(doc), format, flac.as_mut(), progress, hooks.fail_after)?;
    if data_len & 1 == 1 {
        out.write_all(&[0])?;
    }
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok(file)
}

/// The samples, 64 K frames at a time, in `format`'s byte order (FLAC through `flac`).
fn write_samples(
    out: &mut impl Write,
    doc: &DocState,
    ty: SampleType,
    part: &Part,
    format: SaveFormat,
    mut flac: Option<&mut crate::flac_enc::Encoder>,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> io::Result<()> {
    let (first, frames, chans) = (part.frames.start, part.frames.end - part.frames.start, &part.channels);
    let channels = chans.len();
    let mut bufs = vec![vec![0f32; BLOCK_FRAMES]; channels];
    // One quantiser per channel for the whole file: its noise and filter state carry across blocks.
    let mut dithers: Vec<Ditherer> = (0..channels).map(|ch| Ditherer::new(&doc.source().dither, ch)).collect();
    let mut bytes = Vec::with_capacity(BLOCK_FRAMES * channels * ty.bytes());
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(BLOCK_FRAMES as u64) as usize;
        for (&ch, buf) in chans.iter().zip(bufs.iter_mut()) {
            let got = doc.read(ch, first + at, &mut buf[..n])?;
            if got != n {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("channel {ch} gave {got} of {n} frames at {at}")));
            }
        }
        if let Some(enc) = flac.as_deref_mut() {
            let bits = if ty == SampleType::Int16 { 16 } else { 24 };
            for start in (0..n).step_by(crate::flac_enc::BLOCK_SIZE) {
                let end = (start + crate::flac_enc::BLOCK_SIZE).min(n);
                let block: Vec<Vec<i64>> = bufs
                    .iter()
                    .zip(dithers.iter_mut())
                    .map(|(b, d)| b[start..end].iter().map(|&x| i64::from(d.quantize(x, bits))).collect())
                    .collect();
                enc.write_block(&mut *out, &block)?;
            }
        } else {
            bytes.clear();
            let big = format == SaveFormat::Aiff;
            for i in 0..n {
                for (buf, dither) in bufs.iter().zip(dithers.iter_mut()) {
                    push_sample(&mut bytes, buf[i], ty, big, dither);
                }
            }
            out.write_all(&bytes)?;
        }
        at += n as u64;
        if fail_after.is_some_and(|f| at >= f && at < frames) {
            return Err(io::Error::other("test: the disk went away"));
        }
        progress(at as f64 / frames as f64);
    }
    Ok(())
}

fn push_sample(bytes: &mut Vec<u8>, x: f32, ty: SampleType, big: bool, dither: &mut Ditherer) {
    match (ty, big) {
        (SampleType::Int16, false) => bytes.extend_from_slice(&(dither.quantize(x, 16) as i16).to_le_bytes()),
        (SampleType::Int16, true) => bytes.extend_from_slice(&(dither.quantize(x, 16) as i16).to_be_bytes()),
        (SampleType::Int24, false) => bytes.extend_from_slice(&dither.quantize(x, 24).to_le_bytes()[..3]),
        (SampleType::Int24, true) => bytes.extend_from_slice(&dither.quantize(x, 24).to_be_bytes()[1..]),
        (SampleType::Int32, false) => bytes.extend_from_slice(&dither.quantize(x, 32).to_le_bytes()),
        (SampleType::Int32, true) => bytes.extend_from_slice(&dither.quantize(x, 32).to_be_bytes()),
        (SampleType::Float32, false) => bytes.extend_from_slice(&x.to_le_bytes()),
        (SampleType::Float32, true) => bytes.extend_from_slice(&x.to_be_bytes()),
    }
}

/// The AIFF (or, for 32-bit float, AIFF-C `fl32`) header up to the first sample; returns the
/// data size. AIFF sizes are 32 bits: a longer file is refused (use WAV, which goes to RF64).
fn write_aiff_header(out: &mut impl Write, channels: i16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let bytes = ty.bytes() as u64;
    let data_len = frames.checked_mul(channels as u64).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("AIFF too long"))?;
    let aifc = ty == SampleType::Float32;
    let name: &[u8] = b"32-bit floating point"; // 21 bytes + length byte: even
    let comm_len: u32 = if aifc { 18 + 4 + 1 + name.len() as u32 } else { 18 };
    let ssnd_len = 8 + data_len;
    let form_len = 4 + if aifc { 12 } else { 0 } + (8 + u64::from(comm_len)) + 8 + ssnd_len + (data_len & 1);
    let form_len = u32::try_from(form_len).map_err(|_| invalid("too long for AIFF (4 GiB limit): save as WAV or FLAC"))?;
    let frames32 = u32::try_from(frames).map_err(|_| invalid("too long for AIFF"))?;
    out.write_all(b"FORM")?;
    out.write_all(&form_len.to_be_bytes())?;
    out.write_all(if aifc { b"AIFC" } else { b"AIFF" })?;
    if aifc {
        out.write_all(b"FVER")?;
        out.write_all(&4u32.to_be_bytes())?;
        out.write_all(&0xA280_5140u32.to_be_bytes())?;
    }
    out.write_all(b"COMM")?;
    out.write_all(&comm_len.to_be_bytes())?;
    out.write_all(&channels.to_be_bytes())?;
    out.write_all(&frames32.to_be_bytes())?;
    out.write_all(&(bytes as i16 * 8).to_be_bytes())?;
    out.write_all(&extended(sample_rate))?;
    if aifc {
        out.write_all(b"fl32")?;
        out.write_all(&[name.len() as u8])?;
        out.write_all(name)?;
    }
    out.write_all(b"SSND")?;
    out.write_all(&(u32::try_from(ssnd_len).map_err(|_| invalid("too long for AIFF"))?).to_be_bytes())?;
    out.write_all(&[0; 8])?; // data offset and block size
    Ok(data_len)
}

/// A whole number as an 80-bit IEEE 754 extended float (AIFF's sample rate).
fn extended(rate: u32) -> [u8; 10] {
    let mut b = [0u8; 10];
    if rate == 0 {
        return b;
    }
    let lz = u64::from(rate).leading_zeros();
    let exp = 16383 + (63 - lz) as u16;
    b[..2].copy_from_slice(&exp.to_be_bytes());
    b[2..].copy_from_slice(&(u64::from(rate) << lz).to_be_bytes());
    b
}

/// The RIFF (or, above 4 GiB, RF64) header and the `data` chunk's header; returns the data size.
/// The RIFF (or, above 4 GiB, RF64) header, `fmt ` and (float) `fact`; returns the data size
/// and the file offset the next chunk starts at. `extra` is the bytes of every other chunk
/// (headers and pads included) that follow; the `data` chunk is counted here.
fn write_header(
    out: &mut impl Write,
    channels: u16,
    sample_rate: u32,
    ty: SampleType,
    frames: u64,
    extra: u64,
    rf64_over: u64,
) -> io::Result<(u64, u64)> {
    let bytes = ty.bytes() as u64;
    let data_len = frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("WAV too long"))?;
    let float = ty == SampleType::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let fact = if float { 12 } else { 0 };
    let riff_len = 4 + (8 + u64::from(fmt_len)) + fact + 8 + data_len + (data_len & 1) + extra;
    let rf64 = riff_len > rf64_over;
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
