//! Save (task 06, first part: WAV). The document is streamed out 64 K frames at a time with
//! [`DocState::read`], never flattened, so a three-hour file saves on an 8 GB machine. Integer
//! samples are `round(x · 2^(bits−1))`, clamped, with no dither: the exact inverse of how they
//! are read, so untouched audio saves bit for bit. Above 4 GiB the file is RF64.
//!
//! **A save never damages the target:** it writes `<name>.wavcraft-tmp` in the same folder,
//! syncs it, and renames it over the target only when it is complete. Any failure removes the
//! temporary file and leaves the target as it was.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

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

/// The temporary file a save of `target` writes first.
pub fn temp_path(target: &Path) -> PathBuf {
    let name = target.file_name().map_or_else(|| "untitled".into(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!("{name}.wavcraft-tmp"))
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
    let tmp = temp_path(target);
    if let Err(e) = write_wav(doc, &tmp, ty, progress, fail_after) {
        let _ = std::fs::remove_file(&tmp);
        log::warn!(target: "wavcraft_core::save", "save to {} failed, the file is unchanged: {e}", target.display());
        return Err(SaveError::Io(e));
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        // The new file is complete: keep it (on Windows the target may be open in another
        // program) and say where it is.
        log::warn!(target: "wavcraft_core::save", "could not replace {}: {e}; the saved audio is in {}", target.display(), tmp.display());
        return Err(SaveError::Io(io::Error::new(e.kind(), format!("could not replace the file ({e}); the saved audio is in {}", tmp.display()))));
    }
    log::info!(target: "wavcraft_core::save", "saved {} frames as {} WAV to {}", frames, ty.describe(), target.display());
    Ok(())
}

fn write_wav(doc: &DocState, path: &Path, ty: SampleType, progress: &mut dyn FnMut(f64), fail_after: Option<u64>) -> io::Result<()> {
    let (frames, channels) = (doc.len_frames(), doc.channel_count());
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels for WAV"))?;
    let file = File::create(path)?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let data_len = write_header(&mut out, ch16, doc.sample_rate(), ty, frames)?;
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
    if data_len & 1 == 1 {
        out.write_all(&[0])?;
    }
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()
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
