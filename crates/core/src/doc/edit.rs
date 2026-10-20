//! Edits (task 04): pure functions from one [`DocState`] to the next.
//!
//! Every edit takes the state it starts from and returns an [`Edit`]: the new state, the label
//! the History panel shows ("Delete", "Delete Audio", …) and the [`Change`] it made (what the
//! views must redraw, task 05). The old state is untouched, and the new one shares every block
//! and every channel the edit did not change, so undo is keeping the old state
//! ([`super::history`]).
//!
//! Rules:
//!
//! - A range is a half-open `Range<u64>` of frames. An empty range is a no-op: the edit returns
//!   the same state, with [`Change::None`], and the history records nothing (Audition greys
//!   Crop, Cut and Delete out with nothing selected).
//! - Channels never differ in length, so an edit that changes the length acts on all channels,
//!   and with only some channels in its mask an edit keeps the length, as the installed Audition
//!   does (26.5.0.82, `plan/tasks/audition-check.md`, 2026-10-16 Result): [`delete`] (and so
//!   [`cut`]) silences the range in those channels (History: "Delete Audio"); [`crop`] silences
//!   everything outside the range in those channels; [`paste`] overwrites in those channels from
//!   its start for the clip's length instead of inserting, stopping at the end of the file.
//! - Samples are copied only where new audio is made: [`replace_range`], a stereo clip mixed into
//!   a mono document, a clip from another document's store, and the crossfades.
//! - **Crossfades** ([`Crossfades`], on by default at 2 ms and 5 ms, as Audition's Preferences >
//!   Data): `cut_join` where Delete or Cut closes a gap, `edge` at every other boundary an edit
//!   makes (both ends of a paste, of a silence, of an inserted silence, of a `replace_range`, of
//!   the silences a one-channel crop makes). A crop of all channels has none. Each crossfade is
//!   one small new block, built from the audio that runs past the boundary on each side; where
//!   there isn't enough (the start or end of the file, a short paste) it shortens to what there
//!   is. Zero turns a crossfade off, and tests for exact content
//!   use [`Crossfades::NONE`].
//! - The crossfade curve is a raised cosine with the two gains summing to 1 (see
//!   [`crossfade_gain`]), not equal-power: equal-power lifts correlated audio by up to 3 dB in
//!   the middle of the fade, which clicks at the edges of an effect (the same audio on both
//!   sides), while this curve never steps by more than the audio itself at any phase once the
//!   fade is longer than half a period.

use std::fmt;
use std::io;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use soundcraft_audio_io::SampleFormat;

use super::{Channel, DocError, DocState, Piece};
use crate::markers::Shift;
use crate::store::{BLOCK_FRAMES, Store};

/// History labels.
pub const DELETE: &str = "Delete";
/// A Delete with only some channels active: a silence in those channels (Audition's label).
pub const DELETE_AUDIO: &str = "Delete Audio";
pub const CUT: &str = "Cut";
pub const PASTE: &str = "Paste";
/// The step Insert > Audio from File makes (Audition's wording was not seen; our choice).
pub const INSERT_FILE: &str = "Insert Audio from File";
/// Edit › Mix Paste (every mode).
pub const MIX_PASTE: &str = "Mix Paste";
pub const SILENCE: &str = "Silence";
pub const INSERT_SILENCE: &str = "Insert Silence";
pub const CROP: &str = "Crop";
/// Cut, Paste and Crop with only some channels active (Audition's labels).
pub const CUT_AUDIO: &str = "Cut Audio";
pub const PASTE_AUDIO: &str = "Paste Audio";
pub const CROP_AUDIO: &str = "Crop Audio";

/// Which channels an edit acts on: bit `n` is channel `n` (up to 64 channels).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelMask(u64);

impl ChannelMask {
    /// Every channel, whatever the count.
    pub const ALL: ChannelMask = ChannelMask(u64::MAX);
    pub const NONE: ChannelMask = ChannelMask(0);

    /// Only channel `ch`.
    pub fn only(ch: usize) -> ChannelMask {
        ChannelMask(if ch < 64 { 1 << ch } else { 0 })
    }

    pub fn from_bits(bits: u64) -> ChannelMask {
        ChannelMask(bits)
    }

    pub fn bits(self) -> u64 {
        self.0
    }

    /// This mask plus channel `ch`.
    pub fn with(self, ch: usize) -> ChannelMask {
        ChannelMask(self.0 | ChannelMask::only(ch).0)
    }

    pub fn contains(self, ch: usize) -> bool {
        ch < 64 && self.0 & (1 << ch) != 0
    }

    /// The channels of a `channels`-channel document in this mask, in order.
    pub fn active(self, channels: usize) -> Vec<usize> {
        (0..channels).filter(|&c| self.contains(c)).collect()
    }

    /// Every channel of a `channels`-channel document is in the mask.
    pub fn covers_all(self, channels: usize) -> bool {
        (0..channels).all(|c| self.contains(c))
    }

    /// This mask limited to a `channels`-channel document (no bits past its last channel).
    pub fn within(self, channels: usize) -> ChannelMask {
        let keep = if channels >= 64 { u64::MAX } else { (1u64 << channels) - 1 };
        ChannelMask(self.0 & keep)
    }
}

impl fmt::Debug for ChannelMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChannelMask({:#b})", self.0)
    }
}

/// Edit crossfade lengths (Audition's Preferences > Data). 0 turns one off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crossfades {
    /// Where Delete or Cut closes a gap ("Smooth delete/cut boundaries with crossfades of 2 ms").
    pub cut_join: Duration,
    /// Every other boundary an edit makes ("Smooth all edit boundaries by crossfading 5 ms").
    pub edge: Duration,
}

impl Crossfades {
    /// No crossfades: every splice is exact.
    pub const NONE: Crossfades = Crossfades { cut_join: Duration::ZERO, edge: Duration::ZERO };

    /// A duration in frames at `sample_rate`, to the nearest frame.
    pub fn frames(d: Duration, sample_rate: u32) -> u64 {
        (d.as_secs_f64() * f64::from(sample_rate)).round() as u64
    }
}

impl Default for Crossfades {
    fn default() -> Self {
        Crossfades { cut_join: Duration::from_millis(2), edge: Duration::from_millis(5) }
    }
}

/// The two gains of a crossfade `n` frames long at frame `i`: `(outgoing, incoming)`. A raised
/// cosine whose gains sum to 1, so audio that is the same on both sides passes unchanged.
pub fn crossfade_gain(i: u64, n: u64) -> (f64, f64) {
    let t = (i as f64 + 0.5) / n as f64;
    let incoming = 0.5 - 0.5 * (std::f64::consts::PI * t).cos();
    (1.0 - incoming, incoming)
}

/// What an edit changed, for the views (task 05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Nothing (an empty range, an empty clip).
    None,
    /// The length is the same; only `range` of `channels` changed.
    SameLength { range: Range<u64>, channels: ChannelMask },
    /// Everything from `frame` on may have moved.
    LengthFrom { frame: u64 },
}

/// The result of an edit.
#[derive(Debug, Clone)]
pub struct Edit {
    pub state: DocState,
    pub label: String,
    pub change: Change,
}

impl Edit {
    /// Nothing changed: the state is the one the edit started from.
    pub fn is_noop(&self) -> bool {
        self.change == Change::None
    }

    pub(crate) fn noop(doc: &DocState, label: &str) -> Edit {
        Edit { state: doc.clone(), label: label.to_string(), change: Change::None }
    }
}

/// Why an edit could not be made. The document is unchanged.
#[derive(Debug)]
pub enum EditError {
    /// The range runs past the end (or starts after it ends).
    OutOfRange {
        range: Range<u64>,
        len: u64,
    },
    /// A clip at another sample rate (Audition converts; WavCraft refuses for now).
    SampleRate {
        clip: u32,
        doc: u32,
    },
    /// A clip whose channels don't map onto the document's (equal counts, mono into any, or any
    /// into mono are the cases that do).
    Channels {
        clip: usize,
        doc: usize,
    },
    /// The cancel flag was set. Blocks already written die with the pieces that held them.
    Cancelled,
    /// A conversion that can't be made (a setting out of range, a channel layout with no mapping).
    Unsupported(String),
    Io(io::Error),
    Doc(DocError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfRange { range, len } => write!(f, "frames {}..{} are outside a document of {len} frames", range.start, range.end),
            EditError::SampleRate { clip, doc } => {
                write!(f, "the clipboard is at {clip} Hz and the document at {doc} Hz; converting sample rates is not supported yet")
            }
            EditError::Channels { clip, doc } => write!(f, "a {clip}-channel clip can't be pasted into a {doc}-channel document"),
            EditError::Cancelled => write!(f, "cancelled"),
            EditError::Unsupported(why) => write!(f, "{why}"),
            EditError::Io(e) => write!(f, "{e}"),
            EditError::Doc(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EditError {}

impl From<io::Error> for EditError {
    fn from(e: io::Error) -> Self {
        EditError::Io(e)
    }
}

impl From<DocError> for EditError {
    fn from(e: DocError) -> Self {
        EditError::Doc(e)
    }
}

/// Audio copied out of a document: pieces per channel (sharing the document's blocks, no sample
/// copy) and the sample rate.
#[derive(Debug, Clone)]
pub struct Clip {
    store: Store,
    channels: Vec<Arc<Channel>>,
    sample_rate: u32,
    /// The sample type of the document it was copied from (a new document made from the clip
    /// saves in it).
    sample_format: SampleFormat,
}

impl Clip {
    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    pub fn store(&self) -> &Store {
        &self.store
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

    pub fn len_frames(&self) -> u64 {
        self.channels.first().map_or(0, |c| c.len())
    }

    /// The clip as a document state sharing its blocks (no sample copy), for writing it out.
    pub fn to_doc(&self) -> Result<DocState, crate::doc::DocError> {
        let source = std::sync::Arc::new(crate::doc::SourceFormat::synthetic(self.sample_rate, self.channels.len() as u16));
        DocState::from_shared(self.store.clone(), self.channels.clone(), self.sample_rate, source)
    }

    /// All of `doc` as a clip (shares its blocks; no sample copy).
    pub fn of_document(doc: &DocState) -> Clip {
        Clip {
            store: doc.store().clone(),
            channels: doc.channels().to_vec(),
            sample_rate: doc.sample_rate(),
            sample_format: doc.source().sample_format,
        }
    }

    /// Copy frames of channel `ch` from `start` into `out`, as [`DocState::read`].
    pub fn read(&self, ch: usize, start: u64, out: &mut [f32]) -> io::Result<usize> {
        let channel = self.channels.get(ch).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("no channel {ch}")))?;
        channel.read(&self.store, start, out)
    }
}

/// Where a paste goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteAt {
    /// Insert at this frame; the rest moves later.
    Insert(u64),
    /// Replace this range.
    Replace(Range<u64>),
}

// ---------------------------------------------------------------------------------------------
// The edits

/// Delete `range`. With every channel in `channels`: remove it and close the gap (a `cut_join`
/// crossfade at the join). With only some: silence it in those channels and keep the length
/// (label [`DELETE_AUDIO`], `edge` crossfades, as [`silence`]).
pub fn delete(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades) -> Result<Edit, EditError> {
    check_range(doc, &range)?;
    let n = doc.channel_count();
    if range.is_empty() || channels.within(n) == ChannelMask::NONE {
        return Ok(Edit::noop(doc, DELETE));
    }
    if !channels.covers_all(n) {
        return silence_labelled(doc, range, channels, xf, DELETE_AUDIO);
    }
    let len = doc.len_frames();
    let join = Crossfades::frames(xf.cut_join, doc.sample_rate());
    let mut out = Vec::with_capacity(n);
    for ch in doc.channels() {
        let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(ch, range.end, len - range.end, join)];
        out.push(Arc::new(build(doc.store(), &segs)?));
    }
    finish(doc, out, DELETE, Change::LengthFrom { frame: range.start }, Shift::Delete(range.clone()))
}

/// Copy `range` of the channels in `channels` (in channel order). Shares blocks; no sample copy.
pub fn copy(doc: &DocState, range: Range<u64>, channels: ChannelMask) -> Result<Clip, EditError> {
    check_range(doc, &range)?;
    let chans =
        channels.active(doc.channel_count()).into_iter().filter_map(|c| doc.channel(c)).map(|ch| Arc::new(slice(ch, range.clone()))).collect();
    Ok(Clip { store: doc.store().clone(), channels: chans, sample_rate: doc.sample_rate(), sample_format: doc.source().sample_format })
}

/// Copy, then [`delete`]: the clip holds the channels in `channels`, and the document loses
/// `range` (all channels) or has it silenced (some channels).
pub fn cut(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades) -> Result<(Edit, Clip), EditError> {
    let clip = copy(doc, range.clone(), channels)?;
    let mut edit = delete(doc, range, channels, xf)?;
    edit.label = if channels.covers_all(doc.channel_count()) { CUT } else { CUT_AUDIO }.to_string();
    Ok((edit, clip))
}

/// Paste `clip` at a frame or over a range, into the channels in `channels`.
///
/// Equal channel counts paste channel to channel; a mono clip goes to every channel it pastes
/// into; a clip of several channels into a mono document is mixed to mono (their mean, (L+R)/2
/// for stereo, as Audition writes: the only case that makes new blocks, apart from a clip from
/// another document's store, which is copied in). A clip at another sample rate is refused.
///
/// With every channel in `channels`, `Insert` puts the clip in and `Replace` puts it in place of
/// the range. With only some (Audition: "Paste Audio", seen at a cursor mid-file) the length
/// stays the same: the clip overwrites those channels from the insert frame, or from the start
/// of a `Replace` range whatever that range's length, for the clip's length, and the other
/// channels are untouched. Clip frames that would run past the end of the file are dropped (our
/// choice: Audition was not seen pasting near the end). The overwritten stretch is the edit's
/// [`Change::SameLength`] range, the one to select afterwards.
pub fn paste(doc: &DocState, at: PasteAt, clip: &Clip, channels: ChannelMask, xf: Crossfades) -> Result<Edit, EditError> {
    let range = match &at {
        PasteAt::Insert(f) => *f..*f,
        PasteAt::Replace(r) => r.clone(),
    };
    check_range(doc, &range)?;
    if clip.sample_rate != doc.sample_rate() {
        return Err(EditError::SampleRate { clip: clip.sample_rate, doc: doc.sample_rate() });
    }
    let n = doc.channel_count();
    let k = clip.channel_count();
    let clip_len = clip.len_frames();
    let mask = channels.within(n);
    if clip_len == 0 || k == 0 || mask == ChannelMask::NONE {
        return Ok(Edit::noop(doc, PASTE));
    }
    let store = doc.store();
    let len = doc.len_frames();
    let edge = Crossfades::frames(xf.edge, doc.sample_rate());
    if !mask.covers_all(n) {
        // Overwrite in place: `[range.start, range.start + written)` of the active channels.
        let written = clip_len.min(len - range.start);
        if written == 0 {
            return Ok(Edit::noop(doc, PASTE));
        }
        let end = range.start + written;
        let mut out: Vec<Arc<Channel>> = doc.channels().to_vec();
        for (c, src) in paste_sources(store, clip, n, &mask.active(n))? {
            let Some(ch) = doc.channel(c) else { continue };
            let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(&src, 0, written, edge), Seg::new(ch, end, len - end, edge)];
            out[c] = Arc::new(build(store, &segs)?);
        }
        return finish(doc, out, PASTE_AUDIO, Change::SameLength { range: range.start..end, channels: mask }, Shift::None);
    }
    let mut out = Vec::with_capacity(n);
    for (c, src) in paste_sources(store, clip, n, &mask.active(n))? {
        let Some(ch) = doc.channel(c) else { continue };
        let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(&src, 0, clip_len, edge), Seg::new(ch, range.end, len - range.end, edge)];
        out.push(Arc::new(build(store, &segs)?));
    }
    let change = if range.end - range.start == clip_len {
        Change::SameLength { range: range.start..range.end, channels: mask }
    } else {
        Change::LengthFrom { frame: range.start }
    };
    let shift = if range.end - range.start == clip_len { Shift::None } else { Shift::Replace { range: range.clone(), len: clip_len } };
    finish(doc, out, PASTE, change, shift)
}

/// Insert > Audio from File: put all of `file` (opened into the same store as `doc`, so its blocks
/// are shared and nothing is copied) at `at`, into every channel, as one step labelled
/// [`INSERT_FILE`]. Channels and sample rate follow [`paste`]: a mono file goes to every channel,
/// a stereo file into a mono document is mixed to mono, and another sample rate is refused
/// ([`EditError::SampleRate`]; converting is another feature).
pub fn insert_file(doc: &DocState, at: PasteAt, file: &DocState, xf: Crossfades) -> Result<Edit, EditError> {
    let mut edit = paste(doc, at, &Clip::of_document(file), ChannelMask::ALL, xf)?;
    if !edit.is_noop() {
        edit.label = INSERT_FILE.to_string();
    }
    Ok(edit)
}

/// The clip channel each of the document channels in `active` gets, as `(channel, source)`:
/// channel to channel for equal counts, the one channel of a mono clip to each, or the clip's
/// channels mixed to their mean for a mono document. A clip from another store is copied in
/// (only the channels used; a mono clip once).
fn paste_sources(store: &Store, clip: &Clip, n: usize, active: &[usize]) -> Result<Vec<(usize, Arc<Channel>)>, EditError> {
    let k = clip.channels.len();
    let foreign = !store.same_store(&clip.store);
    let bring =
        |c: &Arc<Channel>| -> io::Result<Arc<Channel>> { if foreign { Ok(Arc::new(import(store, &clip.store, &[c])?)) } else { Ok(c.clone()) } };
    if k == n {
        let mut out = Vec::with_capacity(active.len());
        for &c in active {
            if let Some(src) = clip.channels.get(c) {
                out.push((c, bring(src)?));
            }
        }
        Ok(out)
    } else if k == 1 {
        let Some(first) = clip.channels.first() else { return Ok(Vec::new()) };
        let src = bring(first)?;
        Ok(active.iter().map(|&c| (c, src.clone())).collect())
    } else if n == 1 {
        let all: Vec<&Arc<Channel>> = clip.channels.iter().collect();
        let mixed = Arc::new(import(store, &clip.store, &all)?);
        Ok(active.iter().map(|&c| (c, mixed.clone())).collect())
    } else {
        Err(EditError::Channels { clip: k, doc: n })
    }
}

/// Silence `range` in `channels`: the zero block, same length, nothing written to disk (apart
/// from the `edge` crossfades, which fade the old audio out and back in inside the range).
pub fn silence(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades) -> Result<Edit, EditError> {
    silence_labelled(doc, range, channels, xf, SILENCE)
}

fn silence_labelled(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades, label: &str) -> Result<Edit, EditError> {
    check_range(doc, &range)?;
    let n = doc.channel_count();
    let mask = channels.within(n);
    if range.is_empty() || mask == ChannelMask::NONE {
        return Ok(Edit::noop(doc, label));
    }
    let len = doc.len_frames();
    let edge = Crossfades::frames(xf.edge, doc.sample_rate());
    let zeros = zeros(range.end - range.start);
    let mut out = Vec::with_capacity(n);
    for (c, ch) in doc.channels().iter().enumerate() {
        if mask.contains(c) {
            let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(&zeros, 0, zeros.len(), edge), Seg::new(ch, range.end, len - range.end, edge)];
            out.push(Arc::new(build(doc.store(), &segs)?));
        } else {
            out.push(ch.clone());
        }
    }
    finish(doc, out, label, Change::SameLength { range, channels: mask }, Shift::None)
}

/// Insert `frames` of silence at `at` on every channel; the rest moves later.
pub fn insert_silence(doc: &DocState, at: u64, frames: u64, xf: Crossfades) -> Result<Edit, EditError> {
    check_range(doc, &(at..at))?;
    if frames == 0 {
        return Ok(Edit::noop(doc, INSERT_SILENCE));
    }
    let len = doc.len_frames();
    let edge = Crossfades::frames(xf.edge, doc.sample_rate());
    let zeros = zeros(frames);
    let mut out = Vec::with_capacity(doc.channel_count());
    for ch in doc.channels() {
        let segs = [Seg::new(ch, 0, at, 0), Seg::new(&zeros, 0, frames, edge), Seg::new(ch, at, len - at, edge)];
        out.push(Arc::new(build(doc.store(), &segs)?));
    }
    finish(doc, out, INSERT_SILENCE, Change::LengthFrom { frame: at }, Shift::Insert { at, len: frames })
}

/// Keep only `range`. With every channel in `channels`: everything else goes, on every channel,
/// with no crossfades (its ends become the file's). With only some (Audition: "Crop Audio"):
/// everything outside `range` is silenced in those channels and the length stays the same; the
/// other channels are untouched, and an `edge` crossfade inside each silence fades the audio out
/// and in, as [`silence`] does. An empty range is no edit (Audition greys Crop out with nothing
/// selected), and so is a range that already covers the whole file.
pub fn crop(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades) -> Result<Edit, EditError> {
    check_range(doc, &range)?;
    let n = doc.channel_count();
    let mask = channels.within(n);
    let len = doc.len_frames();
    if range.is_empty() || (range.start == 0 && range.end == len) || mask == ChannelMask::NONE {
        return Ok(Edit::noop(doc, CROP));
    }
    if mask.covers_all(n) {
        let out = doc.channels().iter().map(|ch| Arc::new(slice(ch, range.clone()))).collect();
        return finish(doc, out, CROP, Change::LengthFrom { frame: 0 }, Shift::Crop(range.clone()));
    }
    let edge = Crossfades::frames(xf.edge, doc.sample_rate());
    let before = zeros(range.start);
    let after = zeros(len - range.end);
    let mut out = Vec::with_capacity(n);
    for (c, ch) in doc.channels().iter().enumerate() {
        if mask.contains(c) {
            let segs = [
                Seg::new(&before, 0, before.len(), 0),
                Seg::new(ch, range.start, range.end - range.start, edge),
                Seg::new(&after, 0, after.len(), edge),
            ];
            out.push(Arc::new(build(doc.store(), &segs)?));
        } else {
            out.push(ch.clone());
        }
    }
    finish(doc, out, CROP_AUDIO, Change::SameLength { range: 0..len, channels: mask }, Shift::None)
}

/// The general path effects use: new audio for `range` of `channels`, made chunk by chunk.
///
/// `producer(start, input, output)` is called for each chunk of at most [`BLOCK_FRAMES`] frames
/// in order: `start` is the chunk's first frame, `input` holds the original samples of each
/// channel in the mask (in channel order) and `output` the buffers to fill, all the chunk's
/// length. The same two sets of buffers are reused for every chunk, so a 3-hour effect never
/// holds more than these in RAM. Each output chunk is appended to the store as a new block per
/// channel; the rest of the document is shared. Same length in and out, `edge` crossfades inside
/// both ends of the range.
///
/// Checks `cancel` before each chunk: when set, returns [`EditError::Cancelled`], and the blocks
/// already written die with their pieces. A producer error stops the edit the same way.
pub fn replace_range<F>(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    label: &str,
    xf: Crossfades,
    cancel: &AtomicBool,
    mut producer: F,
) -> Result<Edit, EditError>
where
    F: FnMut(u64, &[&[f32]], &mut [&mut [f32]]) -> io::Result<()>,
{
    check_range(doc, &range)?;
    let active = channels.active(doc.channel_count());
    if range.is_empty() || active.is_empty() {
        return Ok(Edit::noop(doc, label));
    }
    let store = doc.store();
    let k = active.len();
    let mut input: Vec<Vec<f32>> = vec![vec![0.0; BLOCK_FRAMES]; k];
    let mut output: Vec<Vec<f32>> = vec![vec![0.0; BLOCK_FRAMES]; k];
    let mut made: Vec<Channel> = vec![Channel::new(); k];
    let mut at = range.start;
    while at < range.end {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        let m = (range.end - at).min(BLOCK_FRAMES as u64) as usize;
        for (buf, &c) in input.iter_mut().zip(&active) {
            let got = doc.read(c, at, &mut buf[..m])?;
            if got != m {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
        }
        {
            let ins: Vec<&[f32]> = input.iter().map(|b| &b[..m]).collect();
            let mut outs: Vec<&mut [f32]> = output.iter_mut().map(|b| &mut b[..m]).collect();
            producer(at, &ins, &mut outs)?;
        }
        for (buf, ch) in output.iter().zip(made.iter_mut()) {
            let block = store.append(&buf[..m])?;
            if let Some(p) = Piece::whole(block) {
                ch.push(p);
            }
        }
        at += m as u64;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(EditError::Cancelled);
    }
    let len = doc.len_frames();
    let edge = Crossfades::frames(xf.edge, doc.sample_rate());
    let mut out: Vec<Arc<Channel>> = doc.channels().to_vec();
    for (new, &c) in made.iter().zip(&active) {
        let Some(ch) = doc.channel(c) else { continue };
        let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(new, 0, new.len(), edge), Seg::new(ch, range.end, len - range.end, edge)];
        out[c] = Arc::new(build(store, &segs)?);
    }
    let mask = active.iter().fold(ChannelMask::NONE, |m, &c| m.with(c));
    finish(doc, out, label, Change::SameLength { range, channels: mask }, Shift::None)
}

// ---------------------------------------------------------------------------------------------
// Mix Paste (f22: A149-A151)

/// How Mix Paste combines the pasted audio with what is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixMode {
    /// Insert at the cursor; the rest moves later (like Paste). With only some channels active it
    /// overwrites in them instead, as [`paste`] does.
    Insert,
    /// Sum the pasted audio into the existing audio.
    Overlap,
    /// Replace the existing audio from the cursor for the pasted length.
    Overwrite,
    /// Multiply the existing audio by the pasted audio.
    Modulate,
}

impl MixMode {
    /// The mode named `s` (`insert`, `overlap`, `overwrite`, `modulate`; any case).
    pub fn parse(s: &str) -> Option<MixMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "insert" => Some(MixMode::Insert),
            "overlap" => Some(MixMode::Overlap),
            "overwrite" => Some(MixMode::Overwrite),
            "modulate" => Some(MixMode::Modulate),
            _ => None,
        }
    }
}

/// The settings of one Mix Paste.
#[derive(Debug, Clone, PartialEq)]
pub struct MixOptions {
    pub mode: MixMode,
    /// The volume of the pasted audio in percent, per channel (the last entry serves any further
    /// channel; empty means 100).
    pub copied_pct: Vec<f64>,
    /// The volume of the existing audio in percent, per channel (as `copied_pct`). Used by
    /// Overlap and Modulate.
    pub existing_pct: Vec<f64>,
    /// Flip the sign of the pasted audio.
    pub invert: bool,
    /// A crossfade of this length at both edges of the pasted audio (zero: none).
    pub crossfade: Duration,
    /// Paste the clip this many times in a row (0 counts as 1).
    pub loops: u32,
}

impl MixOptions {
    /// 100 % of both, not inverted, no crossfade, once.
    pub fn new(mode: MixMode) -> MixOptions {
        MixOptions { mode, copied_pct: Vec::new(), existing_pct: Vec::new(), invert: false, crossfade: Duration::ZERO, loops: 1 }
    }

    fn gains(pct: &[f64], ch: usize) -> f32 {
        (pct.get(ch).or(pct.last()).copied().unwrap_or(100.0) / 100.0) as f32
    }
}

/// How many frames a Mix Paste at `at` with `clip` would write: the clip's length times the loop
/// count, cut at the end of the file for every mode but a whole-file Insert.
pub fn mix_paste_frames(doc: &DocState, at: u64, clip: &Clip, channels: ChannelMask, opts: &MixOptions) -> u64 {
    let total = clip.len_frames().saturating_mul(u64::from(opts.loops.max(1)));
    let grows = opts.mode == MixMode::Insert && channels.within(doc.channel_count()).covers_all(doc.channel_count());
    if grows { total } else { total.min(doc.len_frames().saturating_sub(at)) }
}

/// Edit › Mix Paste: paste `clip` at `at` with a mode, volumes, an optional inversion, edge
/// crossfades and a loop count. One step labelled [`MIX_PASTE`].
///
/// The new audio is made chunk by chunk ([`BLOCK_FRAMES`] frames at a time, reading the clip
/// and the document through the store), never a whole copy in memory, and spliced into the piece
/// table; the rest of the document is shared. Overlap, Overwrite and Modulate stop at the end of
/// the file (our choice, as one-channel [`paste`]); Insert grows it. Channels follow
/// [`paste`]'s rules. Checks `cancel` before each chunk.
pub fn mix_paste(doc: &DocState, at: u64, clip: &Clip, channels: ChannelMask, opts: &MixOptions, cancel: &AtomicBool) -> Result<Edit, EditError> {
    check_range(doc, &(at..at))?;
    if clip.sample_rate != doc.sample_rate() {
        return Err(EditError::SampleRate { clip: clip.sample_rate, doc: doc.sample_rate() });
    }
    let n = doc.channel_count();
    let mask = channels.within(n);
    let clip_len = clip.len_frames();
    let total = mix_paste_frames(doc, at, clip, mask, opts);
    if total == 0 || clip_len == 0 || clip.channel_count() == 0 || mask == ChannelMask::NONE {
        return Ok(Edit::noop(doc, MIX_PASTE));
    }
    let grows = opts.mode == MixMode::Insert && mask.covers_all(n);
    let store = doc.store();
    let len = doc.len_frames();
    let edge = Crossfades::frames(opts.crossfade, doc.sample_rate());
    let sign = if opts.invert { -1.0f32 } else { 1.0 };
    let mut out: Vec<Arc<Channel>> = doc.channels().to_vec();
    let mut clip_buf = vec![0.0f32; BLOCK_FRAMES];
    let mut old_buf = vec![0.0f32; BLOCK_FRAMES];
    for (c, src) in paste_sources(store, clip, n, &mask.active(n))? {
        let Some(ch) = doc.channel(c) else { continue };
        let (g_new, g_old) = (sign * MixOptions::gains(&opts.copied_pct, c), MixOptions::gains(&opts.existing_pct, c));
        let mut made = Channel::new();
        let mut done = 0u64;
        while done < total {
            if cancel.load(Ordering::Relaxed) {
                return Err(EditError::Cancelled);
            }
            let m = (total - done).min(BLOCK_FRAMES as u64) as usize;
            // The clip's frames for output frames done..done+m, wrapping for the loops.
            let mut filled = 0usize;
            while filled < m {
                let from = (done + filled as u64) % clip_len;
                let take = (m - filled).min((clip_len - from) as usize);
                if src.read(store, from, &mut clip_buf[filled..filled + take])? != take {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "clip shorter than its length").into());
                }
                filled += take;
            }
            let buf = &mut clip_buf[..m];
            if matches!(opts.mode, MixMode::Overlap | MixMode::Modulate) {
                let old = &mut old_buf[..m];
                if doc.read(c, at + done, old)? != m {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
                }
                for (b, o) in buf.iter_mut().zip(old.iter()) {
                    *b = if opts.mode == MixMode::Overlap { *o * g_old + *b * g_new } else { (*o * g_old) * (*b * g_new) };
                }
            } else {
                for b in buf.iter_mut() {
                    *b *= g_new;
                }
            }
            if let Some(p) = Piece::whole(store.append(buf)?) {
                made.push(p);
            }
            done += m as u64;
        }
        let end = if grows { at } else { at + total };
        let segs = [Seg::new(ch, 0, at, 0), Seg::new(&made, 0, total, edge), Seg::new(ch, end, len - end, edge)];
        out[c] = Arc::new(build(store, &segs)?);
    }
    // A grown file has every channel rebuilt (Insert on all of them); otherwise only the active ones changed.
    let change = if grows { Change::LengthFrom { frame: at } } else { Change::SameLength { range: at..at + total, channels: mask } };
    // A grown file inserted `total` frames at `at` (later markers move); otherwise nothing moved.
    let shift = if grows { Shift::Insert { at, len: total } } else { Shift::None };
    finish(doc, out, MIX_PASTE, change, shift)
}

/// Generated audio (f21): `frames` of new samples inserted at `range.start`, replacing `range`
/// when it is not empty. Every channel gets a signal; `producer(channel, out)` fills the next
/// `out.len()` frames of that channel's signal (called in order for each channel, in chunks of at
/// most [`BLOCK_FRAMES`], so a long tone is never held in RAM). One undoable edit named `label`.
/// `cancel` stops it between chunks ([`EditError::Cancelled`], nothing changed).
pub fn generate<F>(doc: &DocState, range: Range<u64>, frames: u64, label: &str, cancel: &AtomicBool, mut producer: F) -> Result<Edit, EditError>
where
    F: FnMut(usize, &mut [f32]) -> io::Result<()>,
{
    check_range(doc, &range)?;
    let n = doc.channel_count();
    if frames == 0 || n == 0 {
        return Ok(Edit::noop(doc, label));
    }
    let store = doc.store();
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut made: Vec<Channel> = vec![Channel::new(); n];
    let mut done = 0u64;
    while done < frames {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        let m = (frames - done).min(BLOCK_FRAMES as u64) as usize;
        for (c, ch) in made.iter_mut().enumerate() {
            producer(c, &mut buf[..m])?;
            if let Some(p) = Piece::whole(store.append(&buf[..m])?) {
                ch.push(p);
            }
        }
        done += m as u64;
    }
    let len = doc.len_frames();
    let mut out: Vec<Arc<Channel>> = Vec::with_capacity(n);
    for (c, new) in made.iter().enumerate() {
        let ch = doc.channel(c).ok_or_else(|| io::Error::other("channel missing"))?;
        let segs = [Seg::new(ch, 0, range.start, 0), Seg::new(new, 0, frames, 0), Seg::new(ch, range.end, len - range.end, 0)];
        out.push(Arc::new(build(store, &segs)?));
    }
    // The range was replaced by `frames` generated frames (an insert when the range is empty).
    let shift = Shift::Replace { range: range.clone(), len: frames };
    finish(doc, out, label, Change::LengthFrom { frame: range.start }, shift)
}

// ---------------------------------------------------------------------------------------------
// Building channels

fn check_range(doc: &DocState, range: &Range<u64>) -> Result<(), EditError> {
    let len = doc.len_frames();
    if range.start > range.end || range.end > len {
        return Err(EditError::OutOfRange { range: range.clone(), len });
    }
    Ok(())
}

fn finish(doc: &DocState, channels: Vec<Arc<Channel>>, label: &str, change: Change, shift: Shift) -> Result<Edit, EditError> {
    let mut state = DocState::from_shared(doc.store().clone(), channels, doc.sample_rate(), doc.source().clone())?;
    // Markers follow the audio (A182); the clamp is a safety net for a shift that left one past the end.
    let base = doc.markers().moved_by(&shift).map_or_else(|| doc.markers().clone(), Arc::new);
    let markers = base.clamped(state.len_frames()).map_or(base, Arc::new);
    state = state.with_markers(markers);
    Ok(Edit { state, label: label.to_string(), change })
}

/// A channel of `frames` of silence.
fn zeros(frames: u64) -> Channel {
    let mut ch = Channel::new();
    ch.push_zeros(frames);
    ch
}

/// Push the pieces of `ch` covering `range` onto `out`, joining pieces that continue each other.
fn push_range(out: &mut Channel, ch: &Channel, range: Range<u64>) {
    if range.start >= range.end {
        return;
    }
    let Some((mut i, mut off)) = ch.locate(range.start) else { return };
    let mut left = range.end - range.start;
    while left > 0 {
        let Some(p) = ch.pieces().get(i) else { break };
        let take = u64::from(p.len() - off).min(left) as u32;
        if let Some(sub) = p.sub(off, off + take) {
            out.push_merged(sub);
        }
        left -= u64::from(take);
        i += 1;
        off = 0;
    }
}

/// The pieces of `ch` covering `range`, as a channel of their own.
fn slice(ch: &Channel, range: Range<u64>) -> Channel {
    let mut out = Channel::new();
    push_range(&mut out, ch, range);
    out
}

/// Part of a source channel placed in the output, and the crossfade wanted at its start.
struct Seg<'a> {
    src: &'a Channel,
    start: u64,
    len: u64,
    /// The crossfade wanted where this segment joins the one before it (0: a plain splice).
    fade: u64,
}

impl<'a> Seg<'a> {
    fn new(src: &'a Channel, start: u64, len: u64, fade: u64) -> Seg<'a> {
        Seg { src, start, len, fade }
    }
}

/// A crossfade in the output: `len` frames from `at`, mixing `a` from `a_from` out and `b` from
/// `b_from` in.
struct Fade<'a> {
    at: u64,
    len: u64,
    a: &'a Channel,
    a_from: u64,
    b: &'a Channel,
    b_from: u64,
}

/// The segments one after another, with a crossfade at each join that wants one.
///
/// A crossfade of `n` frames sits as close to centred on its join as the audio allows: it needs
/// the outgoing source to run on past its end and the incoming one to start before its start,
/// and it may not reach past the next join's half of a segment (or back past the previous
/// crossfade). Where the audio runs out it shortens.
fn build(store: &Store, segs: &[Seg<'_>]) -> io::Result<Channel> {
    let segs: Vec<&Seg<'_>> = segs.iter().filter(|s| s.len > 0).collect();
    let mut joined = Channel::new();
    for s in &segs {
        push_range(&mut joined, s.src, s.start..s.start + s.len);
    }
    let mut fades = Vec::new();
    let mut prev_end = 0u64;
    let mut p = segs.first().map_or(0, |s| s.len);
    for i in 1..segs.len() {
        let (a, b) = (segs[i - 1], segs[i]);
        let want = b.fade;
        if want > 0 {
            let a_end = a.start + a.len;
            let right_room = if segs.get(i + 1).is_some_and(|n| n.fade > 0) { b.len / 2 } else { b.len };
            let max_before = (p - prev_end).min(a_end).min(b.start);
            let max_after = right_room.min(a.src.len() - a_end).min(b.src.len() - b.start);
            let mut before = (want / 2).min(max_before);
            let after = (want - before).min(max_after);
            before = (want - after).min(max_before);
            if before + after > 0 {
                fades.push(Fade { at: p - before, len: before + after, a: a.src, a_from: a_end - before, b: b.src, b_from: b.start - before });
                prev_end = p + after;
            }
        }
        p += b.len;
    }
    if fades.is_empty() {
        return Ok(joined);
    }
    let mut out = Channel::new();
    let mut at = 0u64;
    for f in &fades {
        push_range(&mut out, &joined, at..f.at);
        for piece in fade_pieces(store, f)? {
            out.push(piece);
        }
        at = f.at + f.len;
    }
    push_range(&mut out, &joined, at..joined.len());
    Ok(out)
}

/// Write a crossfade's samples as new blocks.
fn fade_pieces(store: &Store, f: &Fade<'_>) -> io::Result<Vec<Piece>> {
    let chunk = (f.len as usize).min(BLOCK_FRAMES);
    let mut x = vec![0.0f32; chunk];
    let mut y = vec![0.0f32; chunk];
    let mut pieces = Vec::new();
    let mut done = 0u64;
    while done < f.len {
        let m = (f.len - done).min(BLOCK_FRAMES as u64) as usize;
        let (xs, ys) = (&mut x[..m], &mut y[..m]);
        if f.a.read(store, f.a_from + done, xs)? != m || f.b.read(store, f.b_from + done, ys)? != m {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "crossfade source shorter than expected"));
        }
        for (i, (o, v)) in xs.iter_mut().zip(ys.iter()).enumerate() {
            let (g_out, g_in) = crossfade_gain(done + i as u64, f.len);
            *o = (f64::from(*o) * g_out + f64::from(*v) * g_in) as f32;
        }
        if let Some(p) = Piece::whole(store.append(xs)?) {
            pieces.push(p);
        }
        done += m as u64;
    }
    Ok(pieces)
}

/// Copy channels of a clip (from `from`) into `store` as new blocks; several channels are mixed
/// to their mean.
fn import(store: &Store, from: &Store, chans: &[&Arc<Channel>]) -> io::Result<Channel> {
    let len = chans.first().map_or(0, |c| c.len());
    let mut acc = vec![0.0f32; BLOCK_FRAMES];
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut out = Channel::new();
    let mut at = 0u64;
    let scale = 1.0 / chans.len().max(1) as f32;
    while at < len {
        let m = (len - at).min(BLOCK_FRAMES as u64) as usize;
        let acc = &mut acc[..m];
        for (k, ch) in chans.iter().enumerate() {
            let dst = if k == 0 { &mut *acc } else { &mut buf[..m] };
            if ch.read(from, at, dst)? != m {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "clip shorter than its length"));
            }
            if k > 0 {
                for (a, b) in acc.iter_mut().zip(&buf[..m]) {
                    *a += *b;
                }
            }
        }
        if chans.len() > 1 {
            for a in acc.iter_mut() {
                *a *= scale;
            }
        }
        if let Some(p) = Piece::whole(store.append(acc)?) {
            out.push(p);
        }
        at += m as u64;
    }
    Ok(out)
}
