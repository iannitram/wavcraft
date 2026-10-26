//! Snapping (v1 C3): snap a frame to markers, the time ruler and zero crossings when within
//! [`SNAP_PX`] pixels on screen.

// Stub for wav-c-22-b: remove the allow and the todo! when implementing.
#![allow(clippy::todo)]

use crate::doc::DocState;
use crate::doc::edit::ChannelMask;

/// How close (in pixels) a pointer position must be to a snap target to land on it.
pub const SNAP_PX: f64 = 8.0;

/// The `Edit › Snapping` switches: the master toggle (`S`) and which targets are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapSettings {
    pub enabled: bool,
    pub markers: bool,
    pub ruler: bool,
    pub zero_crossings: bool,
}

impl Default for SnapSettings {
    fn default() -> SnapSettings {
        SnapSettings { enabled: true, markers: true, ruler: true, zero_crossings: false }
    }
}

/// Snap `frame` to the nearest enabled target within [`SNAP_PX`] pixels at `frames_per_px`, or
/// leave it at `frame.min(doc.len_frames())` when snapping is off or no target is in reach.
pub fn snap_frame(doc: &DocState, settings: &SnapSettings, channels: ChannelMask, frames_per_px: f64, markers: &[u64], frame: u64) -> u64 {
    let _ = (doc, settings, channels, frames_per_px, markers, frame);
    todo!("wav-c-22-b: snap_frame")
}
