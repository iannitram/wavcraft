//! J4 (while recording): the numbers the recording status shows, as text: elapsed time, file
//! size, time left on disk and the dropped-sample count. The level meter and its clip light are
//! the Levels meter's (`meters`), not here.

use crate::space;

/// What the recorder knows about the take in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TakeStats {
    /// Frames written to the take so far.
    pub frames: u64,
    /// The take's sample rate in Hz.
    pub rate: u32,
    /// The take's channel count.
    pub channels: u16,
    /// Bits a sample in the file (16, 24 or 32).
    pub bits: u16,
    /// The take file's size in bytes, header included.
    pub file_bytes: u64,
    /// Free bytes on the recordings folder's disk, if known.
    pub free_bytes: Option<u64>,
    /// Frames lost because the writer fell behind.
    pub dropped_frames: u64,
}

/// The texts the recording status shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeReadout {
    /// Time recorded, `m:ss` or `h:mm:ss`.
    pub elapsed: String,
    /// The file's size, as `space::format_bytes` writes it.
    pub size: String,
    /// Recording time the free space holds, `m:ss` or `h:mm:ss`; `?` when the free space is unknown.
    pub left: String,
    /// The dropped-frame count.
    pub dropped: String,
}

/// The recording status texts for `stats`. A rate, channel count or bit depth of 0 counts as 1, so
/// nothing divides by zero.
pub fn take_readout(stats: &TakeStats) -> TakeReadout {
    let rate = u64::from(stats.rate.max(1));
    let block_align = u64::from(stats.channels.max(1)) * u64::from((stats.bits / 8).max(1));
    TakeReadout {
        elapsed: clock(stats.frames / rate),
        size: space::format_bytes(stats.file_bytes),
        left: match stats.free_bytes {
            Some(free) => clock(free / (block_align * rate)),
            None => "?".to_string(),
        },
        dropped: stats.dropped_frames.to_string(),
    }
}

/// `m:ss`, or `h:mm:ss` from one hour on, for a number of seconds.
fn clock(secs: u64) -> String {
    if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) } else { format!("{}:{:02}", secs / 60, secs % 60) }
}
