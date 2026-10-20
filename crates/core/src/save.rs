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

use crate::doc::{DocState, SourceFormat};
use crate::store::BLOCK_FRAMES;

/// How samples are stored in the saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    Int16,
    Int24,
    Int32,
    Float32,
    /// 64-bit float (raw, W64, CAF and AU; the samples are held as `f32`, so it is exact).
    Float64,
}

/// The file layouts a save can write. `Wav` is the default; the others (item f91) write PCM
/// and float only: Sony Wave64, Apple CAF (big-endian) and Sun/NeXT AU (big-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Wav,
    W64,
    Caf,
    Au,
}

impl Container {
    /// The container for a file name's extension (`None` for one we do not write).
    pub fn from_path(path: &Path) -> Option<Container> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "wav" | "wave" | "bwf" => Some(Container::Wav),
            "w64" => Some(Container::W64),
            "caf" => Some(Container::Caf),
            "au" | "snd" => Some(Container::Au),
            _ => None,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Container::Wav => "WAV",
            Container::W64 => "Wave64",
            Container::Caf => "CAF",
            Container::Au => "AU",
        }
    }

    fn big_endian(self) -> bool {
        matches!(self, Container::Caf | Container::Au)
    }
}

impl SampleType {
    fn bytes(self) -> usize {
        match self {
            SampleType::Int16 => 2,
            SampleType::Int24 => 3,
            SampleType::Int32 | SampleType::Float32 => 4,
            SampleType::Float64 => 8,
        }
    }

    fn is_float(self) -> bool {
        matches!(self, SampleType::Float32 | SampleType::Float64)
    }

    /// "16-bit", "32-bit float"…
    pub fn describe(self) -> &'static str {
        match self {
            SampleType::Int16 => "16-bit",
            SampleType::Int24 => "24-bit",
            SampleType::Int32 => "32-bit integer",
            SampleType::Float32 => "32-bit float",
            SampleType::Float64 => "64-bit float",
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
    save_inner(doc, target, Container::Wav, ty, progress, None)
}

/// [`save_wav`] for any [`Container`]: same atomic temporary-file-and-rename, same streaming.
pub fn save_as(doc: &DocState, target: &Path, container: Container, ty: SampleType, progress: &mut dyn FnMut(f64)) -> Result<(), SaveError> {
    save_inner(doc, target, container, ty, progress, None)
}

/// [`save_wav`] that fails with an I/O error once `fail_after` frames are out (tests: a save
/// that dies halfway must leave the target whole).
#[doc(hidden)]
pub fn save_wav_failing_after(doc: &DocState, target: &Path, ty: SampleType, fail_after: u64) -> Result<(), SaveError> {
    save_inner(doc, target, Container::Wav, ty, &mut |_| {}, Some(fail_after))
}

fn save_inner(
    doc: &DocState,
    target: &Path,
    container: Container,
    ty: SampleType,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> Result<(), SaveError> {
    let frames = doc.len_frames();
    if frames == 0 {
        return Err(SaveError::Empty);
    }
    remove_leftover_temps(target);
    let tmp = TempFile::create(target).map_err(SaveError::Io)?;
    let TempFile { path: tmp, file } = tmp;
    // Written through the handle made by the exclusive create, never reopened by name.
    let file = match write_container(doc, file, container, ty, progress, fail_after) {
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
    log::info!(target: "wavcraft_core::save", "saved {} frames as {} {} to {}", frames, ty.describe(), container.describe(), target.display());
    Ok(())
}

fn write_container(
    doc: &DocState,
    file: File,
    container: Container,
    ty: SampleType,
    progress: &mut dyn FnMut(f64),
    fail_after: Option<u64>,
) -> io::Result<File> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels"))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let rate = doc.sample_rate();
    let data_len = match container {
        Container::Wav => write_header(&mut out, ch16, rate, ty, frames)?,
        Container::W64 => write_w64_header(&mut out, ch16, rate, ty, frames)?,
        Container::Caf => write_caf_header(&mut out, ch16, rate, ty, frames)?,
        Container::Au => write_au_header(&mut out, ch16, rate, ty, frames)?,
    };
    let be = container.big_endian();
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
                put_sample(&mut bytes, x, ty, be);
            }
        }
        out.write_all(&bytes)?;
        at += n as u64;
        if fail_after.is_some_and(|f| at >= f && at < frames) {
            return Err(io::Error::other("test: the disk went away"));
        }
        progress(at as f64 / frames as f64);
    }
    // WAV pads to an even length, Wave64 to a multiple of 8; CAF and AU do not pad.
    let pad = match container {
        Container::Wav => data_len & 1,
        Container::W64 => (8 - data_len % 8) % 8,
        Container::Caf | Container::Au => 0,
    };
    out.write_all(&[0u8; 8][..pad as usize])?;
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok(file)
}

/// The RIFF (or, above 4 GiB, RF64) header and the `data` chunk's header; returns the data size.
fn write_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let bytes = ty.bytes() as u64;
    let data_len = frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("WAV too long"))?;
    let float = ty.is_float();
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

/// One sample of `ty` appended to `out`, little- or big-endian.
fn put_sample(out: &mut Vec<u8>, x: f32, ty: SampleType, be: bool) {
    match ty {
        SampleType::Int16 => {
            let v = quantize(x, 16) as i16;
            out.extend_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        }
        SampleType::Int24 => {
            let v = quantize(x, 24);
            if be {
                out.extend_from_slice(&v.to_be_bytes()[1..]);
            } else {
                out.extend_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        SampleType::Int32 => {
            let v = quantize(x, 32);
            out.extend_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        }
        SampleType::Float32 => out.extend_from_slice(&if be { x.to_be_bytes() } else { x.to_le_bytes() }),
        SampleType::Float64 => {
            let v = f64::from(x);
            out.extend_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        }
    }
}

/// Data size in bytes.
fn data_bytes(channels: u16, ty: SampleType, frames: u64) -> io::Result<u64> {
    frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(ty.bytes() as u64)).ok_or_else(|| invalid("file too long"))
}

/// Sony Wave64 header: the `riff` and `wave` GUIDs, a `fmt ` chunk (16 or 18 bytes of
/// WAVEFORMATEX) and the `data` chunk's header. Chunk sizes are 64-bit and include the chunk's
/// own 24-byte header; chunks are padded to 8 bytes (the padding is not in the size).
fn write_w64_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let data_len = data_bytes(channels, ty, frames)?;
    let float = ty.is_float();
    let fmt_body: u64 = if float { 18 } else { 16 };
    let fmt_size = 24 + fmt_body;
    let fmt_padded = fmt_size.div_ceil(8) * 8;
    let data_size = 24 + data_len;
    let total = 40 + fmt_padded + data_size.div_ceil(8) * 8;
    out.write_all(&W64_RIFF)?;
    out.write_all(&total.to_le_bytes())?;
    out.write_all(&W64_WAVE)?;
    out.write_all(&W64_FMT)?;
    out.write_all(&fmt_size.to_le_bytes())?;
    let bytes = ty.bytes() as u16;
    let block_align = channels.checked_mul(bytes).ok_or_else(|| invalid("too many channels"))?;
    out.write_all(&(if float { 3u16 } else { 1u16 }).to_le_bytes())?;
    out.write_all(&channels.to_le_bytes())?;
    out.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate.checked_mul(u32::from(block_align)).ok_or_else(|| invalid("sample rate too high"))?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&(bytes * 8).to_le_bytes())?;
    if float {
        out.write_all(&0u16.to_le_bytes())?;
    }
    out.write_all(&vec![0u8; (fmt_padded - fmt_size) as usize])?;
    out.write_all(&W64_DATA)?;
    out.write_all(&data_size.to_le_bytes())?;
    Ok(data_len)
}

pub(crate) const W64_RIFF: [u8; 16] = [0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00];
pub(crate) const W64_WAVE: [u8; 16] = [0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
pub(crate) const W64_FMT: [u8; 16] = [0x66, 0x6D, 0x74, 0x20, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
pub(crate) const W64_DATA: [u8; 16] = [0x64, 0x61, 0x74, 0x61, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];

/// Apple CAF header: `caff`, a `desc` chunk (linear PCM, big-endian, packed) and the `data`
/// chunk's header with its 4-byte edit count. Sizes are signed 64-bit big-endian.
fn write_caf_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let data_len = data_bytes(channels, ty, frames)?;
    let bytes = ty.bytes() as u32;
    out.write_all(b"caff")?;
    out.write_all(&1u16.to_be_bytes())?;
    out.write_all(&0u16.to_be_bytes())?;
    out.write_all(b"desc")?;
    out.write_all(&32i64.to_be_bytes())?;
    out.write_all(&f64::from(sample_rate).to_be_bytes())?;
    out.write_all(b"lpcm")?;
    // Flags: bit 0 float, bit 1 little-endian (not set: big-endian samples).
    out.write_all(&u32::from(ty.is_float()).to_be_bytes())?;
    out.write_all(&(bytes * u32::from(channels)).to_be_bytes())?;
    out.write_all(&1u32.to_be_bytes())?;
    out.write_all(&u32::from(channels).to_be_bytes())?;
    out.write_all(&(bytes * 8).to_be_bytes())?;
    out.write_all(b"data")?;
    let size = i64::try_from(data_len + 4).map_err(|_| invalid("file too long"))?;
    out.write_all(&size.to_be_bytes())?;
    out.write_all(&0u32.to_be_bytes())?;
    Ok(data_len)
}

/// Sun/NeXT AU header (24 bytes, big-endian). The data size is 32-bit: longer is refused.
fn write_au_header(out: &mut impl Write, channels: u16, sample_rate: u32, ty: SampleType, frames: u64) -> io::Result<u64> {
    let data_len = data_bytes(channels, ty, frames)?;
    let size = u32::try_from(data_len).ok().filter(|&n| n != u32::MAX).ok_or_else(|| invalid("too long for an AU file (4 GiB at most)"))?;
    let encoding: u32 = match ty {
        SampleType::Int16 => 3,
        SampleType::Int24 => 4,
        SampleType::Int32 => 5,
        SampleType::Float32 => 6,
        SampleType::Float64 => 7,
    };
    out.write_all(b".snd")?;
    out.write_all(&24u32.to_be_bytes())?;
    out.write_all(&size.to_be_bytes())?;
    out.write_all(&encoding.to_be_bytes())?;
    out.write_all(&sample_rate.to_be_bytes())?;
    out.write_all(&u32::from(channels).to_be_bytes())?;
    Ok(data_len)
}
