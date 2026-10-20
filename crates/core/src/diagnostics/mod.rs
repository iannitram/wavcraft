//! Diagnostics (B045 and later): finding and mending faults in recorded audio. Each tool scans
//! a document read-only, reports [`Region`]s, and may offer a repair that goes through the
//! streamed [`replace_range`](crate::doc::edit::replace_range) path as one undo step.

pub mod declip;

/// A stretch of one channel a diagnostic flagged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    /// Channel index in the document.
    pub channel: usize,
    /// First frame of the stretch, in the document.
    pub start: u64,
    /// Number of frames.
    pub len: u64,
    /// The largest absolute sample value inside the stretch.
    pub peak: f32,
}

impl Region {
    /// One past the last frame.
    pub fn end(&self) -> u64 {
        self.start + self.len
    }
}
