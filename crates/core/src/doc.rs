//! The document model: a piece table over the store's unchanging blocks.
//!
//! - A [`Piece`] is part of one block: `(block, offset, len)`.
//! - A [`Channel`] is an ordered list of pieces plus the running total of their lengths, so the
//!   piece holding frame `f` is a binary search ([`Channel::locate`]).
//! - A [`DocState`] is one `Arc<Channel>` per channel, all the same length, plus the sample rate
//!   and what the source file was. It is cheap to clone, and an edit (task 04) makes a new one that
//!   shares almost everything with the old.
//!
//! Rules: blocks never change, only pieces point into them; frame positions are `u64`
//! everywhere; every sample read goes through one path, [`DocState::read`].

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use soundcraft_audio_io::{AudioBuffer, BwfInfo, FileFormat, SampleFormat};

use crate::markers::Markers;
use crate::store::{BLOCK_FRAMES, BlockHandle, BlockId, Store};

pub mod edit;
pub mod history;

/// Part of one block.
#[derive(Debug, Clone)]
pub struct Piece {
    block: Arc<BlockHandle>,
    offset: u32,
    len: u32,
}

impl Piece {
    /// `len` frames of `block` from `offset`. `None` if that is empty or runs past the block.
    pub fn new(block: Arc<BlockHandle>, offset: u32, len: u32) -> Option<Piece> {
        let end = u64::from(offset) + u64::from(len);
        (len > 0 && end <= u64::from(block.frames())).then_some(Piece { block, offset, len })
    }

    /// The whole of `block`.
    pub fn whole(block: Arc<BlockHandle>) -> Option<Piece> {
        let len = block.frames();
        Piece::new(block, 0, len)
    }

    /// `len` frames of silence (the zero block). `None` for 0.
    pub fn zeros(len: u32) -> Option<Piece> {
        Piece::new(Store::zero(), 0, len)
    }

    pub fn block(&self) -> &Arc<BlockHandle> {
        &self.block
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    /// Never true: a piece holds at least one frame.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The frames `[from, to)` of this piece (relative to its start), as a piece of the same
    /// block. `None` if empty or out of range.
    pub fn sub(&self, from: u32, to: u32) -> Option<Piece> {
        if from >= to || to > self.len {
            return None;
        }
        Piece::new(self.block.clone(), self.offset + from, to - from)
    }
}

/// One channel: pieces in order, and `ends[i]` = frames up to and including piece `i`.
#[derive(Debug, Clone, Default)]
pub struct Channel {
    pieces: Vec<Piece>,
    ends: Vec<u64>,
}

impl Channel {
    pub fn new() -> Channel {
        Channel::default()
    }

    pub fn from_pieces(pieces: impl IntoIterator<Item = Piece>) -> Channel {
        let mut ch = Channel::new();
        for p in pieces {
            ch.push(p);
        }
        ch
    }

    /// Add a piece at the end.
    pub fn push(&mut self, piece: Piece) {
        if piece.len == 0 {
            return;
        }
        self.ends.push(self.len() + u64::from(piece.len));
        self.pieces.push(piece);
    }

    /// Add a piece at the end, joining it to the last piece when it continues that piece in the
    /// same block (so cutting and pasting back does not leave the piece list longer).
    pub fn push_merged(&mut self, piece: Piece) {
        if let (Some(last), Some(end)) = (self.pieces.last_mut(), self.ends.last_mut())
            && Arc::ptr_eq(&last.block, &piece.block)
            && u64::from(last.offset) + u64::from(last.len) == u64::from(piece.offset)
            && let Some(len) = last.len.checked_add(piece.len)
        {
            last.len = len;
            *end += u64::from(piece.len);
            return;
        }
        self.push(piece);
    }

    /// Add `frames` of silence at the end (zero-block pieces of at most `u32::MAX` frames).
    pub fn push_zeros(&mut self, mut frames: u64) {
        while frames > 0 {
            let n = frames.min(u64::from(u32::MAX)) as u32;
            if let Some(p) = Piece::zeros(n) {
                self.push(p);
            }
            frames -= u64::from(n);
        }
    }

    /// Length in frames.
    pub fn len(&self) -> u64 {
        self.ends.last().copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// The frame where piece `i` starts (the length, for `i == piece_count()`; `None` past that).
    pub fn piece_start(&self, i: usize) -> Option<u64> {
        if i == 0 { Some(0) } else { self.ends.get(i - 1).copied() }
    }

    /// Which piece holds `frame`, and how far into that piece it is. `None` when `frame` is at
    /// or past the end (`frame >= len()`): no piece holds it.
    pub fn locate(&self, frame: u64) -> Option<(usize, u32)> {
        let i = self.ends.partition_point(|&end| end <= frame);
        let start = self.piece_start(i)?;
        self.pieces.get(i)?;
        Some((i, (frame - start) as u32))
    }

    /// Make a piece start exactly at `frame`, splitting the piece that holds it in two if
    /// needed, and return that piece's index. `frame == len()` returns `piece_count()` (where a
    /// piece added at the end would go). `None` past the end. Never touches samples.
    pub fn split_at(&mut self, frame: u64) -> Option<usize> {
        if frame == self.len() {
            return Some(self.pieces.len());
        }
        let (i, off) = self.locate(frame)?;
        if off == 0 {
            return Some(i);
        }
        let piece = self.pieces.get(i)?;
        let (left, right) = (piece.sub(0, off)?, piece.sub(off, piece.len)?);
        if let Some(slot) = self.pieces.get_mut(i) {
            *slot = left;
        }
        self.pieces.insert(i + 1, right);
        self.ends.insert(i, frame);
        Some(i + 1)
    }

    /// Copy frames from `start` into `out` through the store's cache. Returns how many were
    /// read: `out.len()`, or fewer when the channel ends first (0 at or past the end).
    pub fn read(&self, store: &Store, start: u64, out: &mut [f32]) -> io::Result<usize> {
        let Some((mut i, mut off)) = self.locate(start) else { return Ok(0) };
        let want = (self.len() - start).min(out.len() as u64) as usize;
        let mut done = 0;
        while done < want {
            let Some(piece) = self.pieces.get(i) else { break };
            let n = ((piece.len - off) as usize).min(want - done);
            let dst = out.get_mut(done..done + n).ok_or_else(|| io::Error::other("read buffer"))?;
            store.read(piece.block.block(), piece.offset + off, dst)?;
            done += n;
            i += 1;
            off = 0;
        }
        Ok(done)
    }
}

/// A chunk of the source file (WAV/AIFF): its id, where its header starts, its declared size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    pub id: [u8; 4],
    /// Byte offset of the chunk's 8-byte header in the file.
    pub offset: u64,
    /// The size its header declares (the body may be shorter in a truncated file).
    pub size: u64,
}

impl ChunkInfo {
    pub fn id_str(&self) -> String {
        String::from_utf8_lossy(&self.id).into_owned()
    }
}

/// Chunks the opener reads (WAV `fmt `/`data`/`ds64`/`bext`, AIFF `COMM`/`SSND`/`FVER`) or
/// that are only padding.
const UNDERSTOOD: [&[u8; 4]; 8] = [b"fmt ", b"data", b"ds64", b"bext", b"COMM", b"SSND", b"FVER", b"JUNK"];
/// Chunks that hold markers or loop points (task 08 reads them).
const MARKERS: [&[u8; 4]; 6] = [b"cue ", b"LIST", b"smpl", b"MARK", b"INST", b"COMT"];

/// What the opened file was, and anything about it a save (task 06) or markers (task 08) need.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFormat {
    pub path: PathBuf,
    pub format: FileFormat,
    pub sample_format: SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
    /// The WAV header was RF64 / BW64.
    pub rf64: bool,
    pub bwf: Option<BwfInfo>,
    /// Every chunk of a WAV or AIFF file, in file order (empty for other formats).
    pub chunks: Vec<ChunkInfo>,
    /// What was wrong with the file but still opened: truncation, damaged packets.
    pub warnings: Vec<String>,
}

impl SourceFormat {
    /// A source for audio made in memory (tests, new files).
    pub fn synthetic(sample_rate: u32, channels: u16) -> SourceFormat {
        SourceFormat {
            path: PathBuf::new(),
            format: FileFormat::Wav,
            sample_format: SampleFormat::Float32,
            sample_rate,
            channels,
            rf64: false,
            bwf: None,
            chunks: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Chunks the opener skipped without reading (what a save would drop).
    pub fn skipped_chunks(&self) -> Vec<&ChunkInfo> {
        self.chunks.iter().filter(|c| !UNDERSTOOD.contains(&&c.id)).collect()
    }

    /// Chunks that hold markers (`cue `, `LIST`, `smpl`, AIFF `MARK`, …), for task 08.
    pub fn marker_chunks(&self) -> Vec<&ChunkInfo> {
        self.chunks.iter().filter(|c| MARKERS.contains(&&c.id)).collect()
    }
}

/// Why a [`DocState`] could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    /// Channels of different lengths.
    LengthMismatch(Vec<u64>),
    NoChannels,
    ZeroSampleRate,
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::LengthMismatch(l) => write!(f, "channels differ in length: {l:?}"),
            DocError::NoChannels => write!(f, "a document needs at least one channel"),
            DocError::ZeroSampleRate => write!(f, "a document needs a sample rate"),
        }
    }
}

impl std::error::Error for DocError {}

fn check_parts(channels: &[Arc<Channel>], sample_rate: u32) -> Result<(), DocError> {
    if channels.is_empty() {
        return Err(DocError::NoChannels);
    }
    if sample_rate == 0 {
        return Err(DocError::ZeroSampleRate);
    }
    let lens: Vec<u64> = channels.iter().map(|c| c.len()).collect();
    if lens.windows(2).any(|w| w[0] != w[1]) {
        return Err(DocError::LengthMismatch(lens));
    }
    Ok(())
}

/// One state of a document: its channels (all the same length), sample rate and source.
#[derive(Debug, Clone)]
pub struct DocState {
    store: Store,
    channels: Vec<Arc<Channel>>,
    sample_rate: u32,
    source: Arc<SourceFormat>,
    markers: Arc<Markers>,
    /// Stand-in for the per-document state other rows add (markers on A170): positions in
    /// frames. Tests only; it proves [`DocState::with_parts`] carries state through and
    /// [`DocState::rescale_positions`] moves it.
    #[cfg(test)]
    standin: Vec<u64>,
}

/// `frame` re-expressed at `new_rate` so it keeps its place in time: `frame * new / old`,
/// rounded to the nearest frame (the rule [`crate::convert::convert`] uses for the length).
pub fn rescale_frame(frame: u64, old_rate: u32, new_rate: u32) -> u64 {
    let old = u128::from(old_rate.max(1));
    ((u128::from(frame) * u128::from(new_rate) + old / 2) / old) as u64
}

impl DocState {
    pub fn new(store: Store, channels: Vec<Channel>, sample_rate: u32, source: SourceFormat) -> Result<DocState, DocError> {
        DocState::from_shared(store, channels.into_iter().map(Arc::new).collect(), sample_rate, Arc::new(source))
    }

    /// From channels that may be shared with other states (edits keep the ones they don't change).
    pub fn from_shared(store: Store, channels: Vec<Arc<Channel>>, sample_rate: u32, source: Arc<SourceFormat>) -> Result<DocState, DocError> {
        check_parts(&channels, sample_rate)?;
        let doc = DocState {
            store,
            channels,
            sample_rate,
            source,
            markers: Arc::default(),
            #[cfg(test)]
            standin: Vec::new(),
        };
        debug_assert!(doc.channels.iter().all(|c| c.len() == doc.len_frames()), "all channels of a DocState are the same length");
        Ok(doc)
    }

    /// The markers of this state (A170); undo and redo restore them with the audio.
    pub fn markers(&self) -> &Arc<Markers> {
        &self.markers
    }

    /// This state with other markers (the audio is shared, not copied).
    pub fn with_markers(&self, markers: Arc<Markers>) -> DocState {
        DocState { markers, ..self.clone() }
    }

    /// This state with other audio parts and **every other field carried over** (markers and
    /// whatever later rows add). Use this, not [`DocState::from_shared`], to derive a state that
    /// is the same document with changed audio, so nothing per-document is dropped; positions
    /// in frames are not moved here (see [`DocState::rescale_positions`]).
    pub fn with_parts(&self, channels: Vec<Arc<Channel>>, sample_rate: u32, source: Arc<SourceFormat>) -> Result<DocState, DocError> {
        check_parts(&channels, sample_rate)?;
        Ok(DocState { channels, sample_rate, source, ..self.clone() })
    }

    /// The hook for per-document state stored as frame positions (markers, regions, loop
    /// points): re-express them for a change of sample rate from `old_rate` to `new_rate` with
    /// [`rescale_frame`]. A row that adds such state adds its line here. Selection and cursor
    /// belong to the window, not the document, and are rescaled by the caller.
    pub fn rescale_positions(&mut self, old_rate: u32, new_rate: u32) {
        if !self.markers.is_empty() {
            let scaled = self.markers.map_positions(|f| rescale_frame(f, old_rate, new_rate));
            self.markers = Arc::new(scaled);
        }
        #[cfg(test)]
        for p in &mut self.standin {
            *p = rescale_frame(*p, old_rate, new_rate);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_standin(&mut self, positions: Vec<u64>) {
        self.standin = positions;
    }

    #[cfg(test)]
    pub(crate) fn standin(&self) -> &[u64] {
        &self.standin
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn source(&self) -> &Arc<SourceFormat> {
        &self.source
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, ch: usize) -> Option<&Arc<Channel>> {
        self.channels.get(ch)
    }

    pub fn channels(&self) -> &[Arc<Channel>] {
        &self.channels
    }

    /// Length in frames (every channel has it).
    pub fn len_frames(&self) -> u64 {
        self.channels.first().map_or(0, |c| c.len())
    }

    pub fn piece_count(&self, ch: usize) -> usize {
        self.channels.get(ch).map_or(0, |c| c.piece_count())
    }

    /// Bytes of the distinct blocks this state uses, each block counted once, whole, and the
    /// zero block as 0.
    pub fn disk_bytes_referenced(&self) -> u64 {
        let mut seen: HashSet<BlockId> = HashSet::new();
        let mut bytes = 0;
        for ch in &self.channels {
            for p in ch.pieces() {
                let b = p.block().block();
                if !b.is_zero() && seen.insert(b.id) {
                    bytes += b.disk_bytes();
                }
            }
        }
        bytes
    }

    /// Copy frames of channel `ch` from `start` into `out`. Returns how many were read: all of
    /// `out` unless the document ends first (then fewer; 0 at or past the end). An error for a
    /// channel that doesn't exist or a store read that fails.
    pub fn read(&self, ch: usize, start: u64, out: &mut [f32]) -> io::Result<usize> {
        let channel = self.channels.get(ch).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("no channel {ch}")))?;
        channel.read(&self.store, start, out)
    }

    /// [`read`](Self::read) for every channel at once: `out` holds one buffer per channel, all
    /// the same length. Returns the frames read.
    pub fn read_frames(&self, start: u64, out: &mut [&mut [f32]]) -> io::Result<usize> {
        if out.len() != self.channels.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} buffers for {} channels", out.len(), self.channels.len())));
        }
        let mut frames = usize::MAX;
        for (ch, buf) in out.iter_mut().enumerate() {
            frames = frames.min(self.read(ch, start, buf)?);
        }
        Ok(if out.is_empty() { 0 } else { frames })
    }

    /// The whole document as one in-memory buffer. **Temporary** (task 03 → 05): the app still
    /// wants whole-channel buffers until its views read through [`read`](Self::read). For three
    /// hours of stereo this is 4.1 GB, which is what task 05 removes.
    pub fn to_audio_buffer(&self) -> io::Result<AudioBuffer> {
        let frames = usize::try_from(self.len_frames()).map_err(|_| io::Error::other("document too long for one buffer"))?;
        let mut channels = Vec::with_capacity(self.channels.len());
        for ch in 0..self.channels.len() {
            let mut samples = Vec::new();
            samples.try_reserve_exact(frames).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "no memory for the whole document"))?;
            samples.resize(frames, 0.0);
            for (i, chunk) in samples.chunks_mut(BLOCK_FRAMES).enumerate() {
                self.read(ch, (i * BLOCK_FRAMES) as u64, chunk)?;
            }
            channels.push(samples);
        }
        Ok(AudioBuffer { sample_rate: self.sample_rate, channels })
    }
}
