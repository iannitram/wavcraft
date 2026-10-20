//! Save (task 06, first part: WAV). The document is streamed out 64 K frames at a time with
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
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use soundcraft_audio_io::SampleFormat;

use crate::dither::Ditherer;
use crate::doc::{DocState, SourceFormat};
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
    let file = match write_wav(doc, file, ty, progress, fail_after) {
        Ok(file) => file,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log::warn!(target: "wavcraft_core::save", "save to {} failed, the file is unchanged: {e}", target.display());
            return Err(SaveError::Io(e));
        }
    };
    if let Err(e) = std::fs::rename(&tmp, target) {
        drop(file);
        // Hyperion's review (2026-10-19): a failed save leaves nothing behind. The target is
        // untouched (the rename did not happen) and this save's temporary file goes too.
        let _ = std::fs::remove_file(&tmp);
        log::warn!(target: "wavcraft_core::save", "could not replace {}, the file is unchanged: {e}", target.display());
        return Err(SaveError::Io(io::Error::new(e.kind(), format!("could not replace the file ({e}); it is unchanged"))));
    }
    drop(file);
    log::info!(target: "wavcraft_core::save", "saved {} frames as {} WAV to {}", frames, ty.describe(), target.display());
    Ok(())
}

fn write_wav(doc: &DocState, file: File, ty: SampleType, progress: &mut dyn FnMut(f64), fail_after: Option<u64>) -> io::Result<File> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels for WAV"))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let data_len = write_header(&mut out, ch16, doc.sample_rate(), ty, frames)?;
    let mut bufs = vec![vec![0f32; BLOCK_FRAMES]; channels];
    // One quantiser per channel for the whole file: its noise and filter state carry across blocks.
    let mut dithers: Vec<Ditherer> = (0..channels).map(|ch| Ditherer::new(&doc.source().dither, ch)).collect();
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
            for (buf, dither) in bufs.iter().zip(dithers.iter_mut()) {
                let x = buf[i];
                match ty {
                    SampleType::Int16 => bytes.extend_from_slice(&(dither.quantize(x, 16) as i16).to_le_bytes()),
                    SampleType::Int24 => bytes.extend_from_slice(&dither.quantize(x, 24).to_le_bytes()[..3]),
                    SampleType::Int32 => bytes.extend_from_slice(&dither.quantize(x, 32).to_le_bytes()),
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
    if data_len & 1 == 1 {
        out.write_all(&[0])?;
    }
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok(file)
}

/// The RIFF (or, above 4 GiB, RF64) header and the `data` chunk's header; returns the data size.
fn write_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let bytes = ty.bytes() as u64;
    let data_len = frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("WAV too long"))?;
    let float = ty == SampleType::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let fact = if float { 12 } else { 0 };
    let riff_len = 4 + (8 + u64::from(fmt_len)) + fact + 8 + data_len + (data_len & 1);
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
    out.write_all(b"data")?;
    out.write_all(&u32::try_from(data_len).unwrap_or(u32::MAX).to_le_bytes())?;
    Ok(data_len)
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
