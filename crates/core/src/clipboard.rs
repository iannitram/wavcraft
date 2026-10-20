//! The system clipboard's side of Copy and Paste (roadmap A153). The clipboards themselves are
//! the editor's five [`Clip`]s ([`crate::actions::Editor`]); this module turns a clip into a
//! WAV file another program accepts, and a WAV (or other audio) file another program put on the
//! clipboard into a clip. Both go through files and the session store, never RAM, and both
//! refuse anything bigger than [`MAX_FILE_BYTES`].

use std::fmt;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::doc::edit::{self, ChannelMask, Clip};
use crate::io::{self, OpenError, OpenOptions};
use crate::save::{self, SampleType};

/// The largest WAV file Copy puts on the system clipboard, or Paste takes from it: 256 MiB
/// (24-bit stereo at 48 kHz: about 31 minutes). Larger clips still copy to the internal
/// clipboard.
pub const MAX_FILE_BYTES: u64 = 256 << 20;

/// File extensions Paste takes from the system clipboard.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"];

/// Why a clip could not go to, or come from, the system clipboard.
#[derive(Debug)]
pub enum ClipboardError {
    /// The file would be (or is) `bytes` long; the limit is `limit`.
    TooLarge {
        bytes: u64,
        limit: u64,
    },
    /// The clip holds no audio.
    Empty,
    /// Writing the file failed.
    Save(String),
    /// Reading the file in failed.
    Open(OpenError),
    Io(std::io::Error),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::TooLarge { bytes, limit } => write!(
                f,
                "{} is over the {} limit for the system clipboard (it stays in the internal clipboard)",
                crate::space::format_bytes(*bytes),
                crate::space::format_bytes(*limit)
            ),
            ClipboardError::Empty => write!(f, "there is no audio to put on the system clipboard"),
            ClipboardError::Save(e) => write!(f, "could not write the clipboard file: {e}"),
            ClipboardError::Open(e) => write!(f, "could not read the clipboard audio: {e}"),
            ClipboardError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// How long a 24-bit WAV file of the clip would be.
pub fn wav_bytes(clip: &Clip) -> u64 {
    44 + clip.len_frames().saturating_mul(clip.channel_count() as u64).saturating_mul(3)
}

/// Write `clip` to `target` as a 24-bit WAV (the form most programs accept), streaming through
/// the store. Refuses a clip whose file would pass [`MAX_FILE_BYTES`] before writing anything.
pub fn write_clip_wav(clip: &Clip, target: &Path) -> Result<(), ClipboardError> {
    let bytes = wav_bytes(clip);
    if clip.len_frames() == 0 || clip.channel_count() == 0 {
        return Err(ClipboardError::Empty);
    }
    if bytes > MAX_FILE_BYTES {
        return Err(ClipboardError::TooLarge { bytes, limit: MAX_FILE_BYTES });
    }
    let doc = clip.to_doc().map_err(|e| ClipboardError::Save(e.to_string()))?;
    save::save_wav(&doc, target, SampleType::Int24, &mut |_| {}).map_err(|e| ClipboardError::Save(e.to_string()))
}

/// Whether `path` names a file Paste may take (by extension).
pub fn is_audio_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Read the audio file at `path` (written by another program) into a clip: the file is streamed
/// into a session store made with `opts`, and the clip refers to its blocks.
pub fn clip_from_file(path: &Path, opts: &OpenOptions) -> Result<Clip, ClipboardError> {
    let bytes = std::fs::metadata(path).map_err(ClipboardError::Io)?.len();
    if bytes > MAX_FILE_BYTES {
        return Err(ClipboardError::TooLarge { bytes, limit: MAX_FILE_BYTES });
    }
    let cancel = AtomicBool::new(false);
    let doc = io::open(path, opts, &mut |_| {}, &cancel).map_err(ClipboardError::Open)?;
    edit::copy(&doc, 0..doc.len_frames(), ChannelMask::ALL).map_err(|e| ClipboardError::Save(e.to_string()))
}
