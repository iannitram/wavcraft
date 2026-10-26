//! Top-corner waveform fade handles (v1 E1, issue #27): hit-testing the Fade In and Fade Out
//! handles at the top corners of the active range and turning an inward drag into a fade range
//! and curve.

use std::ops::Range;

use crate::dsp::fade::{FadeCurve, curve_from_drag};
use crate::view::{Selection, Viewport};

/// How close (in pixels) the pointer must be to a top corner of the waveform to grab its fade handle.
pub const HANDLE_HIT_PX: f32 = 8.0;

/// Which top-corner fade handle of the waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeHandle {
    /// Top-left corner: fades in from the start of the active range.
    In,
    /// Top-right corner: fades out to the end of the active range.
    Out,
}

/// The fade edit produced by dragging a top-corner handle inward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeGesture {
    pub handle: FadeHandle,
    pub range: Range<u64>,
    pub curve: FadeCurve,
}

/// The frame range whose top corners carry the fade handles: the selection when non-empty
/// (clamped to `0..total_frames`), or the whole file `0..total_frames` when nothing is selected.
pub fn active_fade_range(selection: &Selection, total_frames: usize) -> Range<usize> {
    // STUB: card wav-c-27-b fills this.
    let _ = (selection, total_frames);
    0..0
}

/// The top-corner fade handle within [`HANDLE_HIT_PX`] of `(x_px, y_px)` (relative to the waveform
/// area's top-left), or `None`. When both corners are within reach, the nearer one wins (`In` on a tie).
pub fn handle_near(view: &Viewport, selection: &Selection, total_frames: usize, x_px: f32, y_px: f32) -> Option<FadeHandle> {
    // STUB: card wav-c-27-b fills this.
    let _ = (view, selection, total_frames, x_px, y_px);
    None
}

/// Turn a drag of `handle` at `x_px` (relative to the waveform area's left) and vertical offset `dy_px`
/// with modifier `ctrl` inside `target` into a [`FadeGesture`], or `None` when the resulting fade has
/// zero length or `x_px` is not finite.
pub fn drag_fade(handle: FadeHandle, view: &Viewport, target: Range<usize>, x_px: f32, dy_px: f32, ctrl: bool) -> Option<FadeGesture> {
    // STUB: card wav-c-27-b fills this.
    let _ = (handle, view, target, x_px, curve_from_drag(dy_px, ctrl));
    None
}
