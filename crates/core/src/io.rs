//! Streamed open: a file goes into a new session store a chunk at a time, never whole into RAM.
//!
//! - **WAV / BWF / RF64** (PCM 8/16/24/32-bit, float 32/64, `WAVE_FORMAT_EXTENSIBLE`) and
//!   **AIFF / AIFF-C** (`NONE`, `twos`, `sowt`, `fl32`, `fl64`): our own chunk parser, then the
//!   data in 64 K-frame chunks, converted to `f32` exactly as `soundcraft-audio-io` converts
//!   (its decoder is the reference; a test compares every sample of every kit file), one block
//!   per channel per chunk.
//! - **Everything else** (MP3, FLAC, Ogg, AAC/ALAC, CAF, ADPCM WAV, …): `symphonia`, packet by
//!   packet, set up as `audio-io` sets it up, so both decode alike.
//!
//! The rules of the chunk parser follow `audio-io`'s on purpose (the same lenient reading of
//! truncated files, a data size of 0, RF64's `ds64`), so a file opens to the same samples either
//! way. What was wrong with a file that still opened is listed in
//! [`SourceFormat::warnings`](crate::doc::SourceFormat).

pub mod channel_mask;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use soundcraft_audio_io::{BwfInfo, FileFormat, SampleFormat};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{self as sc, CodecParameters, CodecType, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

mod containers;
pub use containers::{RAW_MAX_CHANNELS, RAW_MAX_RATE_HZ, RawEncoding, RawSpec};

use crate::doc::{Channel, ChunkInfo, DocState, Piece, SourceFormat};
use crate::markers::Markers;
use crate::settings::HOW_TO_CHANGE;
use crate::space::{FreeSpaceFn, format_bytes, frames_from_file_size, store_bytes};
use crate::store::{BLOCK_FRAMES, BlockHandle, Encoding, Store, StoreOptions, f32_to_i16_exact, f32_to_i24_exact};

/// Why a file could not be opened.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenError {
    /// Reading the file (or writing the session store) failed.
    Io(String),
    /// The bytes are not audio we can decode.
    Decode(String),
    /// The file decoded to no audio at all.
    Empty,
    /// The cancel flag was set. The session folder is gone.
    Cancelled,
    /// Checked before writing: the disk holding the session folder `folder` has `free` bytes,
    /// and the store needs about `needed` (`estimated`: guessed from a compressed file's size).
    /// Nothing was written.
    NoSpace { needed: u64, free: u64, folder: PathBuf, estimated: bool },
    /// The disk holding the session folder `folder` filled up while the store was written. The
    /// partial session is gone.
    DiskFull { folder: PathBuf },
    /// The session folder `folder` (the one session folders are made in) could not be made or
    /// written to (`reason`, the system's words). Nothing was read past the file's start.
    SessionFolder { folder: PathBuf, reason: String },
    /// An append (f57): the file's sample rate is not the one the document has. Nothing was written.
    RateMismatch { file: u32, expected: u32 },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "cannot read the file: {e}"),
            OpenError::Decode(e) => write!(f, "cannot decode the file: {e}"),
            OpenError::Empty => write!(f, "the file holds no audio"),
            OpenError::Cancelled => write!(f, "opening was cancelled"),
            OpenError::NoSpace { needed, free, folder, estimated } => write!(
                f,
                "not enough disk space to open it: it needs about {}{} in the session folder {}, and that disk has {} free. \
                 Free some space there, or choose another session folder ({}).",
                format_bytes(*needed),
                if *estimated { " (estimated from the file's size)" } else { "" },
                folder.display(),
                format_bytes(*free),
                HOW_TO_CHANGE
            ),
            OpenError::DiskFull { folder } => write!(
                f,
                "the disk holding the session folder {} filled up while opening it; nothing was kept. \
                 Free some space there, or choose another session folder ({}).",
                folder.display(),
                HOW_TO_CHANGE
            ),
            OpenError::SessionFolder { folder, reason } => write!(
                f,
                "the session folder {} can't be written to ({reason}), so the file was not opened. \
                 Choose another session folder ({}).",
                folder.display(),
                HOW_TO_CHANGE
            ),
            OpenError::RateMismatch { file, expected } => {
                write!(f, "its sample rate is {file} Hz and the document's is {expected} Hz; convert one of them first")
            }
        }
    }
}

impl std::error::Error for OpenError {}

impl From<io::Error> for OpenError {
    fn from(e: io::Error) -> Self {
        OpenError::Io(e.to_string())
    }
}

pub(crate) fn malformed(what: impl std::fmt::Display) -> OpenError {
    OpenError::Decode(format!("malformed audio: {what}"))
}

pub(crate) fn unsupported(what: impl std::fmt::Display) -> OpenError {
    OpenError::Decode(format!("unsupported audio: {what}"))
}

/// The highest sample rate a file may declare (Hz). 768 kHz is the top of what converters and
/// DXD work use; a header above it is damaged or hostile, and is refused before anything is
/// written (the README's rule: numbers from a file are checked before use).
pub const MAX_SAMPLE_RATE_HZ: u32 = 768_000;

/// Refuse a sample rate of 0 or above [`MAX_SAMPLE_RATE_HZ`]; `what` names the file's kind.
pub(crate) fn check_rate(rate: u32, what: &str) -> Result<(), OpenError> {
    if rate == 0 {
        return Err(malformed(format!("{what} declares a zero sample rate")));
    }
    if rate > MAX_SAMPLE_RATE_HZ {
        return Err(malformed(format!("{what} declares a sample rate of {rate} Hz; WavCraft opens up to {MAX_SAMPLE_RATE_HZ} Hz")));
    }
    Ok(())
}

/// The warning for a sample size under 8 bits (`bits`), which is read as whole bytes: the WAV and
/// AIFF formats store every sample left-justified in whole bytes, so the low bits are padding.
/// Sizes of 9 bits or more that aren't whole bytes (12- or 20-bit recorders) are stored the same
/// way and are not warned about.
fn small_sample_warning(bits: u16) -> Option<String> {
    (bits < 8).then(|| format!("the header gives {bits} bits per sample; read as 8-bit samples, the whole byte each is stored in"))
}

/// Where the session store goes and how it is set up.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// The folder new session folders are made in (the system cache's `wavcraft/sessions` when
    /// `None`).
    pub sessions_root: Option<PathBuf>,
    pub store: StoreOptions,
    /// How free space is measured before writing ([`space::free_space`](crate::space::free_space)
    /// when `None`; tests pass their own). When it returns `None` the check is skipped.
    pub free_space: Option<FreeSpaceFn>,
    /// Stream the file into this existing store instead of making a new session (Insert > Audio
    /// from File: the inserted audio then shares the open document's store, with no copy).
    /// `sessions_root` is not used then. A cancelled or failed open leaves the store alone; the
    /// blocks it had written are freed when the dropped pieces release them.
    pub into_store: Option<Store>,
    /// The folder for saved waveform overviews ([`crate::peakcache`]); `None` leaves the cache
    /// off. Used by [`Document::open_in`](crate::Document::open_in).
    pub peak_cache: Option<PathBuf>,
}

/// Frames per chunk read (and per block written).
const CHUNK_FRAMES: usize = BLOCK_FRAMES;
/// How much of the file's start is read to tell its format.
const HEAD_BYTES: u64 = 1 << 16;
/// Maximum memory to allocate for scratch buffers in bytes (1 GiB).
const MAX_SCRATCH_BYTES: usize = 1 << 30;

/// Open `path` into a new session store. `progress` gets the fraction of the file read so far
/// (0 to 1), now and then. If `cancel` becomes true, opening stops, the store and its folder are
/// removed, and the result is [`OpenError::Cancelled`].
///
/// Before the first sample is written, the store's size is estimated
/// ([`space::store_bytes`](crate::space::store_bytes)) and checked against the free space on
/// the session folder's disk: too little gives [`OpenError::NoSpace`]. A disk that fills up
/// anyway gives [`OpenError::DiskFull`]. Either way the session folder is removed.
pub fn open(path: &Path, opts: &OpenOptions, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<DocState, OpenError> {
    open_inner(path, opts, None, progress, cancel)
}

/// File > Import > Raw Data: open the headerless `path` as `spec` says, streamed into a new
/// session store exactly like [`open`] (same space check, cancel and progress).
pub fn open_raw(path: &Path, spec: &RawSpec, opts: &OpenOptions, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<DocState, OpenError> {
    open_inner(path, opts, Some(spec), progress, cancel)
}

fn open_inner(
    path: &Path,
    opts: &OpenOptions,
    raw: Option<&RawSpec>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<DocState, OpenError> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head)?;

    // The file was read above, so an error from here on is the session folder's, not the file's.
    let store = match &opts.into_store {
        Some(store) => store.clone(),
        None => {
            let root = opts.sessions_root.clone().unwrap_or_else(crate::store::default_sessions_root);
            Store::create_session(Some(root.as_path()), opts.store.clone())
                .map_err(|e| OpenError::SessionFolder { folder: root.clone(), reason: e.to_string() })?
        }
    };
    stream_file(path, file, file_len, head, &store, opts.free_space, None, raw, progress, cancel)
}

/// Open `path` into an existing store (f57, Open Append): its blocks are written to `store`, next
/// to the document's own, and the returned state holds only the file's audio. The file's sample
/// rate must be `expect_rate` ([`OpenError::RateMismatch`], before any sample is written). On an
/// error the store is left as it was, apart from blocks no step refers to.
pub fn open_into(
    path: &Path,
    store: &Store,
    expect_rate: u32,
    free_space: Option<FreeSpaceFn>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<DocState, OpenError> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head)?;
    stream_file(path, file, file_len, head, store, free_space, Some(expect_rate), None, progress, cancel)
}

#[allow(clippy::too_many_arguments)]
fn stream_file(
    path: &Path,
    mut file: File,
    file_len: u64,
    head: Vec<u8>,
    store: &Store,
    free_space: Option<FreeSpaceFn>,
    expect_rate: Option<u32>,
    raw: Option<&RawSpec>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<DocState, OpenError> {
    let ext = path.extension().and_then(|e| e.to_str());
    let mut src = SourceFormat::synthetic(0, 0);
    src.path = path.to_path_buf();
    let free_space = free_space.unwrap_or(crate::space::free_space);
    let mut job = Job { file: &mut file, file_len, progress, cancel, free_space, expect_rate };
    // Raw PCM (main's `raw` spec) is never a WAV, so no `cue `/`LIST` chunks are read from it.
    let is_wav_file = raw.is_none() && is_wav(&head);
    let is_aiff_file = raw.is_none() && !is_wav_file && is_aiff(&head);
    let built = if let Some(spec) = raw {
        containers::open_raw(&mut job, store, &mut src, spec)
    } else if is_wav_file {
        open_wav(&mut job, store, &mut src)
    } else if is_aiff_file {
        open_aiff(&mut job, store, &mut src)
    } else if containers::is_w64(&head) {
        containers::open_w64(&mut job, store, &mut src)
    } else if containers::is_caf(&head) {
        containers::open_caf(&mut job, store, &mut src)
    } else if containers::is_au(&head) {
        containers::open_au(&mut job, store, &mut src)
    } else {
        let detected = soundcraft_audio_io::detect_format(&head, ext);
        drop(head);
        open_symphonia(path, ext, detected, None, &mut job, store, &mut src)
    }?;
    if cancel.load(Ordering::Relaxed) {
        return Err(OpenError::Cancelled);
    }
    if built.channels.is_empty() || built.channels.iter().all(Channel::is_empty) || src.sample_rate == 0 {
        return Err(OpenError::Empty);
    }
    // The last writes may still sit in the store's buffer: a full disk shows up here too.
    store.flush().map_err(|e| write_error(store, e))?;
    (job.progress)(1.0);
    for w in &src.warnings {
        log::warn!("{}: {w}", path.display());
    }
    let rate = src.sample_rate;
    if let Some(first) = built.channels.first() {
        let mut at = 0u64;
        let mut origin = Vec::new();
        for p in first.pieces() {
            origin.push((p.block().block().id, at));
            at += u64::from(p.len());
        }
        src.origin = Arc::new(origin);
    }
    let frames = built.channels.first().map_or(0, Channel::len);
    let markers = if is_wav_file {
        read_wav_markers(&mut job, &src, frames)
    } else if is_aiff_file {
        read_aiff_markers(&mut job, &src, frames)
    } else {
        Markers::new()
    };
    DocState::new(store.clone(), built.channels, rate, src)
        .map(|d| if markers.is_empty() { d } else { d.with_markers(Arc::new(markers)) })
        .map_err(|e| malformed(e.to_string()))
}

/// The markers a WAV file carries (A183): its `cue ` chunk and `LIST`/`adtl` chunks. A file
/// without them, or with damaged ones, gives none; the audio opens either way.
fn read_wav_markers(job: &mut Job<'_>, src: &SourceFormat, frames: u64) -> Markers {
    const MAX_CHUNK: u64 = 16 << 20;
    let file_len = job.file_len;
    let fits = |c: &&ChunkInfo| c.size <= MAX_CHUNK && c.offset.saturating_add(8).saturating_add(c.size) <= file_len;
    let wanted: Vec<(ChunkInfo, bool)> =
        src.chunks.iter().filter(|c| &c.id == b"cue " || &c.id == b"LIST").filter(fits).map(|c| (c.clone(), &c.id == b"cue ")).collect();
    let mut cue = Vec::new();
    let mut lists = Vec::new();
    for (c, is_cue) in wanted {
        let Ok(body) = job.read_at(c.offset + 8, c.size) else { continue };
        if is_cue && cue.is_empty() {
            cue = body;
        } else if !is_cue {
            lists.push(body);
        }
    }
    let lists: Vec<&[u8]> = lists.iter().map(Vec::as_slice).collect();
    Markers::from_wav_chunks(&cue, &lists, frames)
}

/// The markers an AIFF file carries (A184): its `MARK` chunk. None if it has none or it is damaged.
fn read_aiff_markers(job: &mut Job<'_>, src: &SourceFormat, frames: u64) -> Markers {
    let file_len = job.file_len;
    let Some(c) =
        src.chunks.iter().find(|c| &c.id == b"MARK" && c.size <= (16 << 20) && c.offset.saturating_add(8).saturating_add(c.size) <= file_len)
    else {
        return Markers::new();
    };
    match job.read_at(c.offset + 8, c.size) {
        Ok(body) => Markers::from_aiff_mark(&body, frames),
        Err(_) => Markers::new(),
    }
}

struct Job<'a> {
    file: &'a mut File,
    file_len: u64,
    progress: &'a mut dyn FnMut(f64),
    cancel: &'a AtomicBool,
    free_space: FreeSpaceFn,
    /// Open Append: the only sample rate the file may have.
    expect_rate: Option<u32>,
}

impl Job<'_> {
    /// Refuse a file whose rate is not the one an append needs.
    fn check_expected_rate(&self, rate: u32) -> Result<(), OpenError> {
        match self.expect_rate {
            Some(expected) if expected != rate => Err(OpenError::RateMismatch { file: rate, expected }),
            _ => Ok(()),
        }
    }

    fn check(&mut self, at: u64) -> Result<(), OpenError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(OpenError::Cancelled);
        }
        (self.progress)(at as f64 / self.file_len.max(1) as f64);
        Ok(())
    }

    /// Refuse to go on if the disk holding `store` has less room than `frames` frames of
    /// `channels` channels need. An unknown free space passes.
    fn check_space(&self, store: &Store, frames: u64, channels: usize, encoding: Encoding, estimated: bool) -> Result<(), OpenError> {
        let needed = store_bytes(frames, channels as u64, encoding.width());
        match (self.free_space)(store.dir()) {
            Some(free) if free < needed => Err(OpenError::NoSpace { needed, free, folder: sessions_folder(store), estimated }),
            Some(_) => Ok(()),
            None => {
                log::warn!("free space on {} unknown; opening without checking it", store.dir().display());
                Ok(())
            }
        }
    }

    /// Up to `len` bytes at `offset` (fewer at the end of the file).
    fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(offset))?;
        let mut out = Vec::new();
        (&mut *self.file).take(len).read_to_end(&mut out)?;
        Ok(out)
    }
}

/// The folder session folders are made in (what a person sets), for messages.
fn sessions_folder(store: &Store) -> PathBuf {
    store.dir().parent().unwrap_or(store.dir()).to_path_buf()
}

/// A write to the store failed: a full disk (or quota) is [`OpenError::DiskFull`].
fn write_error(store: &Store, e: io::Error) -> OpenError {
    match e.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => OpenError::DiskFull { folder: sessions_folder(store) },
        _ => OpenError::Io(format!("writing the session store in {}: {e}", store.dir().display())),
    }
}

/// What open decodes a source as, which decides how its blocks are stored ([`store_encoding`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSamples {
    /// Read by our own WAV/RF64/AIFF reader: integers (`float: false`) or floats in containers of
    /// `bytes` bytes per sample.
    Pcm { bytes: usize, float: bool },
    /// Decoded by symphonia: `flac` when the codec is FLAC, and the sample format open reports.
    Decoded { flac: bool, sample_format: SampleFormat },
}

/// How a source's blocks are stored: 16-bit containers as [`Encoding::I16`], 24-bit containers
/// (any 17 to 24 valid bits) as [`Encoding::I24`], FLAC of 9 to 16 or 17 to 24 bits likewise,
/// and everything else (8-bit, 32-bit integers, floats, lossy codecs) as [`Encoding::F32`].
///
/// Each block is still checked as it is written: if any sample of it would not read back with
/// the same bits from the integer encoding, that block is written as `F32` instead.
pub fn store_encoding(source: SourceSamples) -> Encoding {
    match source {
        SourceSamples::Pcm { bytes: 2, float: false } => Encoding::I16,
        SourceSamples::Pcm { bytes: 3, float: false } => Encoding::I24,
        SourceSamples::Decoded { flac: true, sample_format: SampleFormat::Int16 } => Encoding::I16,
        SourceSamples::Decoded { flac: true, sample_format: SampleFormat::Int24 } => Encoding::I24,
        _ => Encoding::F32,
    }
}

/// Channels being filled a block at a time.
struct Builder {
    store: Store,
    channels: Vec<Channel>,
    planes: Vec<Vec<f32>>,
    /// How blocks are written (a block that doesn't fit it exactly is written as `F32`).
    encoding: Encoding,
    i16s: Vec<i16>,
    i24s: Vec<i32>,
}

impl Builder {
    fn new(store: &Store, channels: usize, encoding: Encoding) -> Result<Builder, OpenError> {
        // Check if the memory usage would be excessive
        let estimated_size = channels
            .checked_mul(CHUNK_FRAMES)
            .and_then(|n| n.checked_mul(4)) // 4 bytes per f32 sample
            .ok_or_else(|| unsupported(format!("{channels} channels at once is more than WavCraft opens")))?;

        if estimated_size > MAX_SCRATCH_BYTES {
            return Err(unsupported(format!("{channels} channels at once is more than WavCraft opens")));
        }

        let mut planes = Vec::new();
        for _ in 0..channels {
            let mut v = Vec::new();
            v.try_reserve_exact(CHUNK_FRAMES).map_err(|_| OpenError::Io("not enough memory to open a file with this many channels".into()))?;
            planes.push(v);
        }

        Ok(Builder {
            store: store.clone(),
            channels: (0..channels).map(|_| Channel::new()).collect(),
            planes,
            encoding,
            i16s: Vec::new(),
            i24s: Vec::new(),
        })
    }

    /// Append what the planes hold as one block per channel, and empty them.
    fn flush(&mut self) -> Result<(), OpenError> {
        for (plane, ch) in self.planes.iter_mut().zip(self.channels.iter_mut()) {
            if plane.is_empty() {
                continue;
            }
            let block = append_as(&self.store, self.encoding, plane, &mut self.i16s, &mut self.i24s).map_err(|e| write_error(&self.store, e))?;
            if let Some(p) = Piece::whole(block) {
                ch.push(p);
            }
            plane.clear();
        }
        Ok(())
    }

    fn full(&self) -> bool {
        self.planes.first().is_some_and(|p| p.len() >= CHUNK_FRAMES)
    }
}

/// Append `plane` as one block in `encoding`, or as `F32` if a sample of it has no exact integer
/// in that encoding.
fn append_as(store: &Store, encoding: Encoding, plane: &[f32], i16s: &mut Vec<i16>, i24s: &mut Vec<i32>) -> io::Result<Arc<BlockHandle>> {
    match encoding {
        Encoding::I16 => {
            i16s.clear();
            i16s.extend(plane.iter().map_while(|&x| f32_to_i16_exact(x)));
            if i16s.len() == plane.len() {
                return store.append_i16(i16s);
            }
        }
        Encoding::I24 => {
            i24s.clear();
            i24s.extend(plane.iter().map_while(|&x| f32_to_i24_exact(x)));
            if i24s.len() == plane.len() {
                return store.append_i24(i24s);
            }
        }
        Encoding::F32 => {}
    }
    store.append(plane)
}

// ---------------------------------------------------------------------------------------------
// PCM conversion, exactly as soundcraft-audio-io's `pcm::decode_sample`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcmKind {
    /// Signed (unsigned for 1-byte WAV) integers in `bytes` bytes, left-justified.
    Int {
        bytes: usize,
        big_endian: bool,
        unsigned: bool,
    },
    Float {
        bytes: usize,
        big_endian: bool,
    },
}

impl PcmKind {
    fn bytes(self) -> usize {
        match self {
            PcmKind::Int { bytes, .. } | PcmKind::Float { bytes, .. } => bytes,
        }
    }
}

/// One sample from `kind.bytes()` bytes. Integers are left-justified into 32 bits and divided by
/// 2^31 in `f64`, then rounded to `f32` (exact for 8 to 24 bits); non-finite floats become 0.
fn decode_sample(s: &[u8], kind: PcmKind) -> f32 {
    match kind {
        PcmKind::Int { bytes, big_endian, unsigned } => {
            if bytes == 0 || bytes > 4 || s.len() < bytes {
                return 0.0;
            }
            let mut acc: u32 = 0;
            for i in 0..bytes {
                let idx = if big_endian { i } else { bytes - 1 - i };
                let mut byte = s.get(idx).copied().unwrap_or(0);
                if unsigned && i == 0 {
                    byte ^= 0x80;
                }
                acc = (acc << 8) | u32::from(byte);
            }
            let shifted = acc << (32 - 8 * bytes as u32);
            (f64::from(shifted as i32) / 2_147_483_648.0) as f32
        }
        PcmKind::Float { bytes, big_endian } => {
            let v = match bytes {
                4 => {
                    let Some(a) = s.get(..4).and_then(|x| <[u8; 4]>::try_from(x).ok()) else { return 0.0 };
                    if big_endian { f32::from_be_bytes(a) } else { f32::from_le_bytes(a) }
                }
                8 => {
                    let Some(a) = s.get(..8).and_then(|x| <[u8; 8]>::try_from(x).ok()) else { return 0.0 };
                    (if big_endian { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) }) as f32
                }
                _ => 0.0,
            };
            if v.is_finite() { v } else { 0.0 }
        }
    }
}

/// De-interleave whole frames of `data` onto the end of `planes`.
fn deinterleave(data: &[u8], kind: PcmKind, planes: &mut [Vec<f32>]) {
    let bytes = kind.bytes();
    let block = bytes * planes.len();
    if block == 0 {
        return;
    }
    match (kind, planes) {
        // The common cases, written out for speed. Both are exact: a 16- or 24-bit integer
        // divided by a power of two fits an f32, so they equal the general path bit for bit.
        (PcmKind::Int { bytes: 2, big_endian: false, unsigned: false }, [mono]) => {
            mono.extend(data.as_chunks::<2>().0.iter().map(|s| f32::from(i16::from_le_bytes(*s)) / 32_768.0));
        }
        (PcmKind::Int { bytes: 2, big_endian: false, unsigned: false }, [l, r]) => {
            for f in data.as_chunks::<4>().0 {
                l.push(f32::from(i16::from_le_bytes([f[0], f[1]])) / 32_768.0);
                r.push(f32::from(i16::from_le_bytes([f[2], f[3]])) / 32_768.0);
            }
        }
        (PcmKind::Int { bytes: 3, big_endian: false, unsigned: false }, planes) => {
            for f in data.chunks_exact(block) {
                for (p, s) in planes.iter_mut().zip(f.as_chunks::<3>().0) {
                    let v = i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8;
                    p.push(v as f32 / 8_388_608.0);
                }
            }
        }
        (kind, planes) => {
            for f in data.chunks_exact(block) {
                for (p, s) in planes.iter_mut().zip(f.chunks_exact(bytes)) {
                    p.push(decode_sample(s, kind));
                }
            }
        }
    }
}

/// Read `frames` frames of `kind` PCM from `data_start` into the store, a chunk at a time.
fn stream_pcm(job: &mut Job<'_>, store: &Store, data_start: u64, frames: u64, kind: PcmKind, channels: usize) -> Result<Builder, OpenError> {
    let encoding = store_encoding(SourceSamples::Pcm { bytes: kind.bytes(), float: matches!(kind, PcmKind::Float { .. }) });
    job.check_space(store, frames, channels, encoding, false)?;

    // Check buffer size before allocation
    let block = kind.bytes() * channels;
    let buf_len = CHUNK_FRAMES.checked_mul(block).filter(|n| *n <= MAX_SCRATCH_BYTES).ok_or_else(|| unsupported("too many channels"))?;

    let mut buf = Vec::new();
    buf.try_reserve_exact(buf_len).map_err(|_| OpenError::Io("not enough memory to open this file".into()))?;
    buf.resize(buf_len, 0u8);

    let mut b = Builder::new(store, channels, encoding)?;
    job.file.seek(SeekFrom::Start(data_start))?;
    let mut done = 0u64;
    while done < frames {
        job.check(data_start + done * block as u64)?;
        let n = (frames - done).min(CHUNK_FRAMES as u64) as usize;
        let bytes = buf.get_mut(..n * block).ok_or_else(|| OpenError::Io("chunk buffer".into()))?;
        job.file.read_exact(bytes)?;
        deinterleave(bytes, kind, &mut b.planes);
        b.flush()?;
        done += n as u64;
    }
    Ok(b)
}

// ---------------------------------------------------------------------------------------------
// WAV

pub(crate) const WAVE_FORMAT_PCM: u16 = 1;
pub(crate) const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

pub(crate) fn tag(b: &[u8], pos: usize) -> Option<[u8; 4]> {
    b.get(pos..pos.checked_add(4)?)?.try_into().ok()
}

pub(crate) fn le_u16(b: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(pos..pos.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn le_u32(b: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(pos..pos.checked_add(4)?)?.try_into().ok()?))
}

pub(crate) fn le_u64(b: &[u8], pos: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(pos..pos.checked_add(8)?)?.try_into().ok()?))
}

pub(crate) fn be_u16(b: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(pos..pos.checked_add(2)?)?.try_into().ok()?))
}

pub(crate) fn be_u32(b: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(pos..pos.checked_add(4)?)?.try_into().ok()?))
}

pub(crate) fn is_wav(b: &[u8]) -> bool {
    matches!(tag(b, 0), Some(t) if &t == b"RIFF" || &t == b"RF64" || &t == b"BW64") && tag(b, 8).is_some_and(|t| &t == b"WAVE")
}

pub(crate) fn is_aiff(b: &[u8]) -> bool {
    tag(b, 0).is_some_and(|t| &t == b"FORM") && tag(b, 8).is_some_and(|t| &t == b"AIFF" || &t == b"AIFC")
}

pub(crate) struct Fmt {
    pub(crate) tag: u16,
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    pub(crate) bits: u16,
    pub(crate) valid_bits: u16,
    /// `dwChannelMask` of a `WAVE_FORMAT_EXTENSIBLE` header (0 or absent: unlabelled).
    pub(crate) channel_mask: u32,
    /// Bytes a frame takes, as the header says (checked against the format before use).
    pub(crate) block_align: u16,
}

pub(crate) fn parse_fmt(c: &[u8]) -> Result<Fmt, OpenError> {
    let short = || malformed("fmt chunk too short");
    let mut fmt = Fmt {
        tag: le_u16(c, 0).ok_or_else(short)?,
        channels: le_u16(c, 2).ok_or_else(short)?,
        sample_rate: le_u32(c, 4).ok_or_else(short)?,
        bits: le_u16(c, 14).unwrap_or(0),
        valid_bits: 0,
        channel_mask: 0,
        block_align: le_u16(c, 12).ok_or_else(short)?,
    };
    fmt.valid_bits = fmt.bits;
    if fmt.tag == WAVE_FORMAT_EXTENSIBLE {
        match (le_u16(c, 18), le_u16(c, 24)) {
            (Some(valid), Some(sub)) => {
                if valid != 0 {
                    fmt.valid_bits = valid;
                }
                fmt.tag = sub;
                fmt.channel_mask = le_u32(c, 20).unwrap_or(0);
            }
            _ => return Err(malformed("WAVE_FORMAT_EXTENSIBLE fmt chunk too short")),
        }
    }
    Ok(fmt)
}

pub(crate) fn fixed_str(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(b.get(..end).unwrap_or_default()).trim_end().to_string()
}

fn parse_bext(c: &[u8]) -> Option<BwfInfo> {
    if c.len() < 346 {
        return None;
    }
    let lo = u64::from(le_u32(c, 338)?);
    let hi = u64::from(le_u32(c, 342)?);
    Some(BwfInfo {
        description: fixed_str(c.get(0..256)?),
        originator: fixed_str(c.get(256..288)?),
        originator_reference: fixed_str(c.get(288..320)?),
        origination_date: fixed_str(c.get(320..330)?),
        origination_time: fixed_str(c.get(330..338)?),
        time_reference: (hi << 32) | lo,
    })
}

fn open_wav(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat) -> Result<Builder, OpenError> {
    let file_len = job.file_len;
    let first = job.read_at(0, 12)?;
    let is_rf64 = tag(&first, 0).is_some_and(|t| &t != b"RIFF");
    src.format = FileFormat::Wav;
    src.rf64 = is_rf64;
    let mut ds64_data: Option<u64> = None;
    let mut fmt: Option<Fmt> = None;
    // (start, length used, length declared, size field was 0)
    let mut data: Option<(u64, u64, u64, bool)> = None;
    let mut pos: u64 = 12;
    let mut chunks = 0usize;
    loop {
        let header = job.read_at(pos, 8)?;
        let (Some(id), Some(size32)) = (tag(&header, 0), le_u32(&header, 4)) else { break };
        chunks += 1;
        if chunks > 100_000 {
            break;
        }
        let body = pos + 8;
        let remaining = file_len.saturating_sub(body);
        let mut size = u64::from(size32);
        if &id == b"data" && is_rf64 && size32 == u32::MAX {
            size = ds64_data.unwrap_or(remaining);
        }
        let declared = size;
        src.chunks.push(ChunkInfo { id, offset: pos, size: declared });
        if &id == b"data" {
            let zero = size32 == 0 && !is_rf64;
            let len = if zero { remaining } else { declared.min(remaining) };
            if data.is_none() {
                data = Some((body, len, declared, zero));
            }
            if declared > remaining {
                break;
            }
        } else {
            if declared > remaining {
                break;
            }
            match &id {
                b"fmt " => fmt = Some(parse_fmt(&job.read_at(body, declared)?)?),
                b"bext" if declared <= crate::meta::MAX_META_BYTES => {
                    let raw = job.read_at(body, declared)?;
                    src.bwf = parse_bext(&raw);
                    src.meta.bext = crate::meta::Bext::parse(&raw);
                }
                b"_PMX" if src.meta.xmp.is_none() && src.meta.xmp_problem.is_none() => {
                    let read = if declared <= crate::xmp::MAX_XMP_BYTES {
                        crate::xmp::Xmp::parse(&job.read_at(body, declared)?)
                    } else {
                        Err("the packet is larger than 4 MiB".to_string())
                    };
                    match read {
                        Ok(x) => src.meta.xmp = Some(x),
                        Err(e) => {
                            src.warnings.push(format!("the XMP packet could not be read ({e}); it is kept as it is"));
                            src.meta.xmp_problem = Some(e);
                        }
                    }
                }
                b"LIST" if declared <= crate::meta::MAX_META_BYTES && src.meta.info.0.is_empty() => {
                    if let Some(tags) = crate::meta::InfoTags::parse(&job.read_at(body, declared)?) {
                        src.meta.info = tags;
                    }
                }
                b"iXML" if declared <= crate::meta::MAX_META_BYTES => src.meta.ixml = Some(job.read_at(body, declared)?),
                b"ds64" => ds64_data = le_u64(&job.read_at(body, declared.min(64))?, 8),
                _ => {}
            }
        }
        pos = body + declared + (declared & 1);
    }

    let fmt = fmt.ok_or_else(|| malformed("WAV has no fmt chunk"))?;
    if fmt.channels == 0 {
        return Err(malformed("WAV declares zero channels"));
    }
    check_rate(fmt.sample_rate, "WAV")?;
    job.check_expected_rate(fmt.sample_rate)?;
    src.sample_rate = fmt.sample_rate;
    src.channels = fmt.channels;
    let channels = usize::from(fmt.channels);
    src.meta.chnames = crate::meta::mask_names(fmt.channel_mask, channels);
    if let Some(x) = &src.meta.ixml {
        for (slot, n) in src.meta.chnames.iter_mut().zip(crate::meta::ixml_names(x, channels)) {
            if !n.is_empty() {
                *slot = n;
            }
        }
    }
    if channels > crate::channels::MAX_CHANNELS {
        return Err(unsupported(format!("WAV with {channels} channels (at most {} are supported)", crate::channels::MAX_CHANNELS)));
    }
    if fmt.channel_mask != 0 {
        if crate::channels::usable_mask(Some(fmt.channel_mask), channels).is_some() {
            src.channel_mask = Some(fmt.channel_mask);
        } else {
            src.warnings.push(format!(
                "the channel mask {:#x} names {} speakers but the file has {channels} channels: the channels are numbered 1 to {channels}",
                fmt.channel_mask,
                fmt.channel_mask.count_ones()
            ));
        }
    }
    let (kind, sample_format) = match (fmt.tag, fmt.bits) {
        (WAVE_FORMAT_PCM, bits @ 1..=32) => {
            let bytes = usize::from(bits.div_ceil(8));
            let sf = match fmt.valid_bits.min(bits) {
                0..=8 => SampleFormat::Int8,
                9..=16 => SampleFormat::Int16,
                17..=24 => SampleFormat::Int24,
                _ => SampleFormat::Int32,
            };
            (PcmKind::Int { bytes, big_endian: false, unsigned: bytes == 1 }, sf)
        }
        (WAVE_FORMAT_IEEE_FLOAT, 32) => (PcmKind::Float { bytes: 4, big_endian: false }, SampleFormat::Float32),
        (WAVE_FORMAT_IEEE_FLOAT, 64) => (PcmKind::Float { bytes: 8, big_endian: false }, SampleFormat::Float64),
        (WAVE_FORMAT_PCM | WAVE_FORMAT_IEEE_FLOAT, bits) => {
            return Err(unsupported(format!("WAV format {} with {bits} bits per sample", fmt.tag)));
        }
        _ => {
            // ADPCM, A-law, µ-law…: symphonia decodes these, as audio-io hands them over.
            let bwf = src.bwf.take();
            let path = src.path.clone();
            return open_symphonia(&path, Some("wav"), FileFormat::Wav, Some(bwf), job, store, src);
        }
    };
    src.sample_format = sample_format;
    // Only for the PCM and float layouts read here: other codecs (ADPCM…) use block-align for
    // their own packets and went to symphonia above.
    let block = (kind.bytes() * channels) as u64;
    if u64::from(fmt.block_align) != block {
        return Err(malformed(format!(
            "the WAV header's block-align is {} bytes, but {channels} channel(s) of {}-byte samples take {block} bytes a frame",
            fmt.block_align,
            kind.bytes()
        )));
    }
    if let PcmKind::Int { .. } = kind {
        src.warnings.extend(small_sample_warning(fmt.bits));
    }
    let (start, len, declared, zero) = data.ok_or_else(|| malformed("WAV has no data chunk"))?;
    let frames = len / block;
    if zero {
        src.warnings.push(format!("the data chunk's size is 0 (an unfinished recording?): opened the {frames} frames the file holds"));
    } else if declared > len {
        src.warnings.push(format!(
            "the file is shorter than its header says: the data chunk claims {declared} bytes but {len} are there; opened the {frames} whole frames present"
        ));
    }
    stream_pcm(job, store, start, frames, kind, channels)
}

// ---------------------------------------------------------------------------------------------
// AIFF

const MAX_SAMPLE_RATE: f64 = MAX_SAMPLE_RATE_HZ as f64;

/// An 80-bit IEEE 754 extended float.
pub(crate) fn extended_to_f64(b: &[u8]) -> Option<f64> {
    let se = be_u16(b, 0)?;
    let hi = u64::from(be_u32(b, 2)?);
    let lo = u64::from(be_u32(b, 6)?);
    let mant = (hi << 32) | lo;
    if mant == 0 {
        return Some(0.0);
    }
    let exp = i32::from(se & 0x7FFF);
    if exp == 0x7FFF {
        return None;
    }
    let v = mant as f64 * 2f64.powi(exp - 16383 - 63);
    Some(if se & 0x8000 != 0 { -v } else { v })
}

fn open_aiff(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat) -> Result<Builder, OpenError> {
    let file_len = job.file_len;
    let first = job.read_at(0, 12)?;
    let is_aifc = tag(&first, 8).is_some_and(|t| &t == b"AIFC");
    src.format = FileFormat::Aiff;
    let mut comm: Option<(i16, u32, i16, f64, [u8; 4])> = None;
    // (start, length, declared body length)
    let mut ssnd: Option<(u64, u64, u64)> = None;
    let mut pos: u64 = 12;
    let mut chunks = 0usize;
    loop {
        let header = job.read_at(pos, 8)?;
        let (Some(id), Some(size32)) = (tag(&header, 0), be_u32(&header, 4)) else { break };
        chunks += 1;
        if chunks > 100_000 {
            break;
        }
        let body = pos + 8;
        let remaining = file_len.saturating_sub(body);
        let declared = u64::from(size32);
        let len = declared.min(remaining);
        src.chunks.push(ChunkInfo { id, offset: pos, size: declared });
        match &id {
            b"COMM" => {
                let c = job.read_at(body, len.min(4096))?;
                let short = || malformed("COMM chunk too short");
                let channels = be_u16(&c, 0).ok_or_else(short)? as i16;
                let frames = be_u32(&c, 2).ok_or_else(short)?;
                let bits = be_u16(&c, 6).ok_or_else(short)? as i16;
                let rate = extended_to_f64(c.get(8..18).ok_or_else(short)?).ok_or_else(|| malformed("invalid sample rate"))?;
                let compression = if is_aifc { tag(&c, 18).ok_or_else(short)? } else { *b"NONE" };
                comm = Some((channels, frames, bits, rate, compression));
            }
            b"SSND" if ssnd.is_none() => {
                let c = job.read_at(body, len.min(4))?;
                let offset = u64::from(be_u32(&c, 0).unwrap_or(0));
                let start = (body + 8).saturating_add(offset).min(file_len);
                let data_len = len.saturating_sub(8).saturating_sub(offset);
                ssnd = Some((start, data_len, declared.saturating_sub(8).saturating_sub(offset)));
            }
            _ => {}
        }
        if declared > remaining {
            break;
        }
        pos = body + declared + (declared & 1);
    }

    let (channels, comm_frames, bits, rate, compression) = comm.ok_or_else(|| malformed("AIFF has no COMM chunk"))?;
    if channels <= 0 {
        return Err(malformed("AIFF declares zero or negative channels"));
    }
    if !(1.0..=MAX_SAMPLE_RATE).contains(&rate) {
        return Err(malformed(format!("AIFF sample rate {rate} out of range")));
    }
    job.check_expected_rate(rate.round() as u32)?;
    src.sample_rate = rate.round() as u32;
    src.channels = channels as u16;
    let int_kind = |big_endian: bool| -> Result<(PcmKind, SampleFormat), OpenError> {
        if !(1..=32).contains(&bits) {
            return Err(unsupported(format!("AIFF sample size {bits}")));
        }
        let bytes = (bits as usize).div_ceil(8);
        let sf = match bits {
            1..=8 => SampleFormat::Int8,
            9..=16 => SampleFormat::Int16,
            17..=24 => SampleFormat::Int24,
            _ => SampleFormat::Int32,
        };
        Ok((PcmKind::Int { bytes, big_endian, unsigned: false }, sf))
    };
    let (kind, sample_format) = match &compression {
        b"NONE" | b"twos" => int_kind(true)?,
        b"sowt" => int_kind(false)?,
        b"fl32" | b"FL32" => (PcmKind::Float { bytes: 4, big_endian: true }, SampleFormat::Float32),
        b"fl64" | b"FL64" => (PcmKind::Float { bytes: 8, big_endian: true }, SampleFormat::Float64),
        _ => {
            let path = src.path.clone();
            return open_symphonia(&path, Some("aiff"), FileFormat::Aiff, None, job, store, src);
        }
    };
    src.sample_format = sample_format;
    if let PcmKind::Int { .. } = kind {
        src.warnings.extend(small_sample_warning(bits as u16));
    }
    let (start, data_len, declared) = ssnd.unwrap_or((file_len, 0, 0));
    let block = (kind.bytes() * channels as usize).max(1) as u64;
    let frames = u64::from(comm_frames).min(data_len / block);
    if frames < u64::from(comm_frames) {
        src.warnings.push(format!(
            "the file is shorter than its header says: {comm_frames} frames declared, {frames} present{}",
            if declared > data_len { " (the sound data is cut off)" } else { "" }
        ));
    }
    stream_pcm(job, store, start, frames, kind, channels as usize)
}

// ---------------------------------------------------------------------------------------------
// Everything else: symphonia, set up as soundcraft-audio-io sets it up.

/// The file, counting how far symphonia has read it.
struct CountingFile {
    file: File,
    len: u64,
    pos: Arc<AtomicU64>,
}

impl Read for CountingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        self.pos.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for CountingFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = self.file.seek(to)?;
        self.pos.store(at, Ordering::Relaxed);
        Ok(at)
    }
}

impl MediaSource for CountingFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

pub(crate) fn format_from_codec(codec: CodecType, detected: FileFormat) -> FileFormat {
    if detected != FileFormat::Other {
        return detected;
    }
    match codec {
        c if c == sc::CODEC_TYPE_FLAC => FileFormat::Flac,
        c if c == sc::CODEC_TYPE_MP3 || c == sc::CODEC_TYPE_MP2 || c == sc::CODEC_TYPE_MP1 => FileFormat::Mp3,
        c if c == sc::CODEC_TYPE_AAC => FileFormat::Aac,
        c if c == sc::CODEC_TYPE_ALAC => FileFormat::Alac,
        c if c == sc::CODEC_TYPE_VORBIS => FileFormat::Ogg,
        _ => FileFormat::Other,
    }
}

pub(crate) fn sample_format_of(p: &CodecParameters) -> SampleFormat {
    let c = p.codec;
    if c == sc::CODEC_TYPE_PCM_F32LE || c == sc::CODEC_TYPE_PCM_F32BE {
        return SampleFormat::Float32;
    }
    if c == sc::CODEC_TYPE_PCM_F64LE || c == sc::CODEC_TYPE_PCM_F64BE {
        return SampleFormat::Float64;
    }
    let lossy = [
        sc::CODEC_TYPE_MP1,
        sc::CODEC_TYPE_MP2,
        sc::CODEC_TYPE_MP3,
        sc::CODEC_TYPE_AAC,
        sc::CODEC_TYPE_VORBIS,
        sc::CODEC_TYPE_OPUS,
        sc::CODEC_TYPE_ADPCM_MS,
        sc::CODEC_TYPE_ADPCM_IMA_WAV,
        sc::CODEC_TYPE_ADPCM_IMA_QT,
        sc::CODEC_TYPE_ADPCM_G722,
        sc::CODEC_TYPE_ADPCM_G726,
        sc::CODEC_TYPE_ADPCM_G726LE,
        sc::CODEC_TYPE_PCM_ALAW,
        sc::CODEC_TYPE_PCM_MULAW,
    ];
    match (!lossy.contains(&c), p.bits_per_sample) {
        (true, Some(1..=8)) => SampleFormat::Int8,
        (true, Some(9..=16)) => SampleFormat::Int16,
        (true, Some(17..=24)) => SampleFormat::Int24,
        (true, Some(25..=32)) => SampleFormat::Int32,
        _ => SampleFormat::Compressed,
    }
}

pub(crate) fn sym_err(e: SymError) -> OpenError {
    match e {
        SymError::Unsupported(m) => unsupported(m),
        SymError::LimitError(m) => OpenError::Decode(format!("audio too large: {m}")),
        other => malformed(other),
    }
}

/// `bwf`: `Some` when a WAV's own parse found its metadata (an ADPCM WAV, say).
fn open_symphonia(
    path: &Path,
    ext: Option<&str>,
    detected: FileFormat,
    bwf: Option<Option<BwfInfo>>,
    job: &mut Job<'_>,
    store: &Store,
    src: &mut SourceFormat,
) -> Result<Builder, OpenError> {
    let pos = Arc::new(AtomicU64::new(0));
    let source = CountingFile { file: File::open(path)?, len: job.file_len, pos: pos.clone() };
    let result = catch_unwind(AssertUnwindSafe(|| decode_symphonia(source, &pos, ext, detected, job, store, src)));
    let mut built = result.unwrap_or_else(|_| Err(malformed("decoder failed on malformed input")))?;
    if let Some(bwf) = bwf {
        src.bwf = bwf;
    }
    built.flush()?;
    Ok(built)
}

fn decode_symphonia(
    source: CountingFile,
    pos: &AtomicU64,
    ext: Option<&str>,
    detected: FileFormat,
    job: &mut Job<'_>,
    store: &Store,
    src: &mut SourceFormat,
) -> Result<Builder, OpenError> {
    let mss = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext.trim_start_matches('.'));
    }
    let fmt_opts = FormatOptions { enable_gapless: true, ..FormatOptions::default() };
    let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &MetadataOptions::default()).map_err(sym_err)?;
    let mut reader = probed.format;
    let Some(track) = reader.tracks().iter().find(|t| t.codec_params.codec != sc::CODEC_TYPE_NULL) else {
        // MOV/MP4 files list tracks whose codec symphonia does not know (AC-3, video...).
        return Err(if reader.tracks().is_empty() {
            unsupported("no decodable audio track")
        } else {
            unsupported("this file has no audio track in a codec WavCraft can read; the audio codec is not supported")
        });
    };
    let mut track_id = track.id;
    let others = reader.tracks().iter().filter(|t| t.id != track_id && t.codec_params.codec != sc::CODEC_TYPE_NULL).count();
    if others > 0 {
        src.warnings
            .push(format!("the file has {} audio tracks; opened the first one (track {track_id}) and left the other {others} unopened", others + 1));
    }
    let params = track.codec_params.clone();
    src.sample_rate = params.sample_rate.filter(|&r| r > 0).ok_or_else(|| malformed("unknown sample rate"))?;
    check_rate(src.sample_rate, "the file")?;
    job.check_expected_rate(src.sample_rate)?;
    src.channels = u16::try_from(params.channels.map(|c| c.count()).unwrap_or(0)).map_err(|_| unsupported("too many channels"))?;
    src.format = format_from_codec(params.codec, detected);
    src.sample_format = sample_format_of(&params);
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(sym_err)?;
    // The decoder's frame count when the container gives one (FLAC, most MP3s, M4A), else a
    // guess from the file's size. An unknown channel count is taken as stereo.
    let (expected_frames, estimated) = match params.n_frames {
        Some(n) if n > 0 => (n, false),
        _ => (frames_from_file_size(job.file_len, src.sample_rate), true),
    };
    let encoding = store_encoding(SourceSamples::Decoded { flac: params.codec == sc::CODEC_TYPE_FLAC, sample_format: src.sample_format });
    job.check_space(store, expected_frames, params.channels.map_or(2, |c| c.count()).max(1), encoding, estimated)?;

    let mut built: Option<Builder> = None;
    let mut frames: u64 = 0;
    let mut sbuf: Option<SampleBuffer<f32>> = None;
    let (mut errors, mut last_error) = (0usize, String::new());
    loop {
        job.check(pos.load(Ordering::Relaxed))?;
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => {
                sbuf = None;
                let Some(track) = reader.tracks().iter().find(|t| t.codec_params.codec != sc::CODEC_TYPE_NULL) else {
                    break;
                };
                let new_params = track.codec_params.clone();
                let Some(sample_rate) = new_params.sample_rate.filter(|&r| r > 0) else {
                    break;
                };
                let channels = new_params.channels.map(|c| c.count()).unwrap_or(0);
                if sample_rate != src.sample_rate {
                    src.warnings.push(format!(
                        "the sample rate changed from {} to {sample_rate} mid-stream; opened the {frames} frames before that",
                        src.sample_rate
                    ));
                    break;
                }
                if channels != usize::from(src.channels) {
                    src.warnings.push(format!(
                        "the channel count changed from {} to {channels} mid-stream; opened the {frames} frames before that",
                        src.channels
                    ));
                    break;
                }
                let Ok(new_decoder) = symphonia::default::get_codecs().make(&new_params, &DecoderOptions::default()) else {
                    break;
                };
                track_id = track.id;
                decoder = new_decoder;
                continue;
            }
            Err(e) => {
                if frames == 0 {
                    return Err(sym_err(e));
                }
                src.warnings.push(format!("stopped reading after a damaged part of the file ({e}); opened the {frames} frames before it"));
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(m)) => {
                errors += 1;
                last_error = m.to_string();
                if errors > 1000 && frames == 0 {
                    return Err(malformed(format!("no decodable packets: {m}")));
                }
                continue;
            }
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(sym_err(e)),
        };
        let spec = *decoded.spec();
        let ch = spec.channels.count();
        if ch == 0 {
            continue;
        }
        if built.is_none() {
            // The decoded stream is authoritative for the channel count.
            src.channels = u16::try_from(ch).map_err(|_| unsupported("too many channels"))?;
            built = Some(Builder::new(store, ch, encoding)?);
        }
        let Some(b) = built.as_mut() else { break };
        if b.planes.len() != ch {
            src.warnings
                .push(format!("the channel count changed from {} to {ch} mid-stream; opened the {frames} frames before that", b.planes.len()));
            break;
        }
        frames += decoded.frames() as u64;
        let sb = match &mut sbuf {
            Some(sb) if sb.capacity() >= decoded.capacity() * ch => sb,
            _ => sbuf.insert(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)),
        };
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks_exact(ch) {
            for (dst, &s) in b.planes.iter_mut().zip(frame) {
                dst.push(if s.is_finite() { s } else { 0.0 });
            }
            if b.full() {
                b.flush()?;
            }
        }
    }
    if params.codec == sc::CODEC_TYPE_FLAC && !estimated && frames < expected_frames {
        src.warnings.push(format!("the file is truncated: opened the {frames} of {expected_frames} frames it should hold"));
    }
    if errors > 0 {
        src.warnings.push(format!("{errors} damaged packet(s) skipped (last: {last_error})"));
    }
    if src.channels == 0 {
        return Err(malformed("stream contains no audio"));
    }
    Ok(match built {
        Some(b) => b,
        None => Builder::new(store, usize::from(src.channels), encoding)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    use super::{Encoding, OpenOptions, SampleFormat, SourceSamples, open, store_encoding};

    const FULL: &[u8] = include_bytes!("../tests/fixtures/q367-full.flac");
    const TRUNCATED: &[u8] = include_bytes!("../tests/fixtures/q367-truncated.flac");

    /// A folder in the system's temp directory, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("wavcraft-io-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Opens `bytes` as a FLAC file; returns its frame count and warnings.
    fn open_flac(name: &str, bytes: &[u8]) -> (u64, Vec<String>) {
        let t = TempDir::new(name);
        let path = t.0.join("input.flac");
        assert!(std::fs::write(&path, bytes).is_ok());
        let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
        let result = open(&path, &opts, &mut |_| {}, &AtomicBool::new(false));
        assert!(result.is_ok());
        match result {
            Ok(doc) => (doc.len_frames(), doc.source().warnings.clone()),
            Err(_) => (0, Vec::new()),
        }
    }

    /// Opens `bytes` as a PCM WAV file; returns its frame count and warnings.
    fn open_wav(name: &str, bytes: &[u8]) -> (u64, Vec<String>) {
        let t = TempDir::new(name);
        let path = t.0.join("input.wav");
        assert!(std::fs::write(&path, bytes).is_ok());
        let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
        let result = open(&path, &opts, &mut |_| {}, &AtomicBool::new(false));
        assert!(result.is_ok());
        match result {
            Ok(doc) => (doc.len_frames(), doc.source().warnings.clone()),
            Err(_) => (0, Vec::new()),
        }
    }

    #[test]
    fn q367_truncated_flac_warns_and_opens_fewer_frames() {
        let (frames, warnings) = open_flac("truncated", TRUNCATED);
        assert!(warnings.iter().any(|w| w.contains("truncated")), "warnings: {warnings:?}");
        assert!(frames > 0);
        assert!(frames < 44100, "frames: {frames}");
        // Complete frames decoded by the flac CLI (independent decoder).
        assert_eq!(frames, 16384);
    }

    #[test]
    fn q367_intact_flac_opens_fully_without_truncation_warning() {
        let (frames, warnings) = open_flac("intact", FULL);
        assert_eq!(frames, 44100);
        assert!(!warnings.iter().any(|w| w.contains("truncated")), "warnings: {warnings:?}");
    }

    #[test]
    fn q367_non_flac_count_mismatch_does_not_warn() {
        let channels = 2u16;
        let bits = 16u16;
        let sample_rate = 44_100u32;
        let block_align = channels * (bits / 8);
        let samples = vec![0u8; 32];
        let expected_data_len = samples.len() as u32 * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + expected_data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&expected_data_len.to_le_bytes());
        bytes.extend_from_slice(&samples);

        let actual_frames = samples.len() / usize::from(block_align);
        let (frames, warnings) = open_wav("non-flac-count-mismatch", &bytes);
        assert_eq!(frames, actual_frames as u64);
        assert!(!warnings.iter().any(|w| w.contains("truncated")), "warnings: {warnings:?}");
    }

    /// Test 2's table: which encoding each kind of source is stored in.
    #[test]
    fn each_source_gets_its_encoding() {
        use Encoding::*;
        use SampleFormat as S;
        let pcm = |bytes, float| SourceSamples::Pcm { bytes, float };
        let dec = |flac, sample_format| SourceSamples::Decoded { flac, sample_format };
        let table = [
            (pcm(1, false), F32, "8-bit WAV"),
            (pcm(2, false), I16, "16-bit WAV/AIFF (and 9 to 16 valid bits)"),
            (pcm(3, false), I24, "24-bit WAV/AIFF/RF64 (and 17 to 24 valid bits, e.g. 20)"),
            (pcm(4, false), F32, "32-bit integer"),
            (pcm(4, true), F32, "32-bit float"),
            (pcm(8, true), F32, "64-bit float"),
            (dec(true, S::Int8), F32, "8-bit FLAC"),
            (dec(true, S::Int16), I16, "16-bit FLAC"),
            (dec(true, S::Int24), I24, "24-bit FLAC"),
            (dec(true, S::Int32), F32, "32-bit FLAC"),
            (dec(false, S::Compressed), F32, "MP3, AAC, Vorbis, ADPCM"),
            (dec(false, S::Int16), F32, "16-bit ALAC or other symphonia PCM"),
            (dec(false, S::Float32), F32, "float through symphonia"),
        ];
        for (source, want, what) in table {
            assert_eq!(store_encoding(source), want, "{what}");
        }
    }
}
