//! Stereo width control: scales the side signal of a left/right pair.

/// The largest width factor; larger requests are treated as this value.
pub const MAX_WIDTH: f32 = 4.0;

/// STUB: leaves the signal unchanged.
pub fn widen(_left: &mut [f32], _right: &mut [f32], _width: f32) {}

/// STUB: leaves the signal unchanged.
pub fn widen_interleaved(_samples: &mut [f32], _width: f32) {}

/// STUB: always returns 0.
pub fn measure_width(_left: &[f32], _right: &[f32]) -> f32 {
    0.0
}
