//! New documents (f30): an empty one with a chosen format, and one made from a [`Clip`] (Copy to
//! New, Paste to New). Both get a session store of their own. A clip is streamed into it one
//! block at a time, so a long selection never sits in RAM whole.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_audio_io::SampleFormat;

use super::edit::Clip;
use super::{Channel, DocState, Piece, SourceFormat};
use crate::io::MAX_SAMPLE_RATE_HZ;
use crate::space::{FreeSpaceFn, format_bytes, free_space, store_bytes};
use crate::store::{BLOCK_FRAMES, Store, StoreOptions};

/// The most channels a new document may have.
pub const MAX_CHANNELS: usize = 32;

/// Where the new document's store goes and how it is set up.
#[derive(Debug, Clone, Default)]
pub struct NewDocOptions {
    /// The session folder's root (the default one when `None`).
    pub sessions_root: Option<PathBuf>,
    pub store: StoreOptions,
    /// How free space is measured (the real one when `None`; tests pass their own).
    pub free_space: Option<FreeSpaceFn>,
}

#[derive(Debug)]
pub enum NewDocError {
    /// A format the dialog should not have offered.
    Invalid(String),
    /// The session folder could not be made or written.
    Io(String),
    NoSpace {
        needed: u64,
        free: u64,
    },
    Cancelled,
}

impl fmt::Display for NewDocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NewDocError::Invalid(why) => write!(f, "{why}"),
            NewDocError::Io(why) => write!(f, "could not make the new document: {why}"),
            NewDocError::NoSpace { needed, free } => {
                write!(
                    f,
                    "not enough disk space for the new document: it needs about {}, and the session folder's disk has {} free",
                    format_bytes(*needed),
                    format_bytes(*free)
                )
            }
            NewDocError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for NewDocError {}

/// The sample types a new document may have (what a save can write).
pub fn supported(f: SampleFormat) -> bool {
    matches!(f, SampleFormat::Int16 | SampleFormat::Int24 | SampleFormat::Float32)
}

fn source(rate: u32, channels: usize, sample_format: SampleFormat) -> SourceFormat {
    let mut src = SourceFormat::synthetic(rate, u16::try_from(channels).unwrap_or(u16::MAX));
    src.sample_format = sample_format;
    src
}

fn store_for(opts: &NewDocOptions) -> Result<Store, NewDocError> {
    Store::create_session(opts.sessions_root.as_deref(), opts.store.clone()).map_err(|e| NewDocError::Io(e.to_string()))
}

/// A document with no audio: `channels` empty channels at `rate`, saved as `sample_format`.
pub fn empty(opts: &NewDocOptions, rate: u32, channels: usize, sample_format: SampleFormat) -> Result<DocState, NewDocError> {
    if rate == 0 || rate > MAX_SAMPLE_RATE_HZ {
        return Err(NewDocError::Invalid(format!("the sample rate must be between 1 and {MAX_SAMPLE_RATE_HZ} Hz")));
    }
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(NewDocError::Invalid(format!("the channel count must be between 1 and {MAX_CHANNELS}")));
    }
    if !supported(sample_format) {
        return Err(NewDocError::Invalid("the bit depth must be 16-bit, 24-bit or 32-bit float".to_string()));
    }
    let store = store_for(opts)?;
    let chans = (0..channels).map(|_| Channel::new()).collect();
    DocState::new(store, chans, rate, source(rate, channels, sample_format)).map_err(|e| NewDocError::Io(e.to_string()))
}

/// A document holding `clip`'s audio in a new store of its own, with the clip's channel count,
/// sample rate and sample type. The audio is read and written a block at a time. `progress`
/// gets 0 to 1; `cancel` stops it (the half-made store is removed).
pub fn from_clip(opts: &NewDocOptions, clip: &Clip, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<DocState, NewDocError> {
    let (frames, n) = (clip.len_frames(), clip.channel_count());
    if frames == 0 || n == 0 {
        return Err(NewDocError::Invalid("there is no audio to put in a new document".to_string()));
    }
    let store = store_for(opts)?;
    // The blocks are written as f32 (4 bytes a sample, [`Encoding::F32`]).
    let needed = store_bytes(frames, n as u64, 4);
    if let Some(free) = opts.free_space.unwrap_or(free_space)(store.dir())
        && free < needed
    {
        return Err(NewDocError::NoSpace { needed, free });
    }
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut channels = Vec::with_capacity(n);
    for c in 0..n {
        let mut ch = Channel::new();
        let mut at = 0u64;
        while at < frames {
            if cancel.load(Ordering::Relaxed) {
                return Err(NewDocError::Cancelled);
            }
            let m = (frames - at).min(BLOCK_FRAMES as u64) as usize;
            let got = clip.read(c, at, &mut buf[..m]).map_err(|e| NewDocError::Io(e.to_string()))?;
            if got != m {
                return Err(NewDocError::Io("the clip is shorter than its length".to_string()));
            }
            let block = store.append(&buf[..m]).map_err(|e| NewDocError::Io(e.to_string()))?;
            if let Some(p) = Piece::whole(block) {
                ch.push(p);
            }
            at += m as u64;
            progress(((c as u64 * frames + at) as f64) / (n as u64 * frames) as f64);
        }
        channels.push(ch);
    }
    store.flush().map_err(|e| NewDocError::Io(e.to_string()))?;
    let rate = clip.sample_rate();
    DocState::new(store, channels, rate, source(rate, n, clip.sample_format())).map_err(|e| NewDocError::Io(e.to_string()))
}
