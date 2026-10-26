//! Time-ruler tick step and nearest-tick frame arithmetic for snapping (v1 C3).

/// Spacing between major ruler ticks on screen, in pixels.
pub const MAJOR_TICK_PX: f64 = 110.0;

/// Round tick spacings in seconds, from 1 ms up to 30 minutes.
pub const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];

/// The smallest round number of seconds in [`STEPS`] that is at least `min_seconds`, or `3600.0`
/// when none is.
pub fn ruler_step(min_seconds: f64) -> f64 {
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// The frame of the major ruler tick nearest `frame` at `frames_per_px` and `sample_rate` Hz.
pub fn ruler_tick_near(frame: u64, frames_per_px: f64, sample_rate: u32) -> u64 {
    let rate = f64::from(sample_rate.max(1));
    let step = ruler_step(frames_per_px / rate * MAJOR_TICK_PX);
    let t = (frame as f64 / rate / step).round() * step;
    (t * rate).round().max(0.0) as u64
}
