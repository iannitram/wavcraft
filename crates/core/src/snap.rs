//! Snapping (f09, roadmap A123 and A127): move a frame the pointer is on to the nearest snap
//! target within [`SNAP_PX`] pixels. Targets are the ruler's ticks, the frames the caller lists
//! (selection edges, the cursor), the document's markers and, when asked, zero crossings of the enabled channels. The
//! selection gestures (f08) call [`snap_frame`] (or `Editor::snap`) for each frame they place.

use crate::actions::{Toward, ZERO_CROSSING_REACH_MS, zero_crossing};
use crate::doc::DocState;
use crate::doc::edit::ChannelMask;

/// How close, in pixels, a target must be to pull a frame to it.
pub const SNAP_PX: f64 = 8.0;

/// Snapping's switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapSettings {
    /// The master switch (Audition's default key S).
    pub enabled: bool,
    /// Also snap to zero crossings of the enabled channels.
    pub zero_crossings: bool,
    /// Also snap to markers: point markers and the edges of range markers (f59, A124).
    pub markers: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings { enabled: true, zero_crossings: false, markers: true }
    }
}

/// The smallest "round" number of seconds that is at least `min_seconds`: the ruler's tick step.
pub fn ruler_step(min_seconds: f64) -> f64 {
    const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// The ruler's tick nearest `frame` at this zoom (the ticks the ruler draws).
pub fn ruler_tick_near(frame: u64, frames_per_px: f64, sample_rate: u32) -> u64 {
    let rate = f64::from(sample_rate.max(1));
    let step = ruler_step(frames_per_px / rate * 110.0);
    let t = (frame as f64 / rate / step).round() * step;
    (t * rate).round().max(0.0) as u64
}

/// `frame` snapped, or unchanged when snapping is off or nothing is close. The nearest target
/// wins; on a tie a listed target or the ruler beats a zero crossing. The result is inside the
/// file. `channels` are the enabled channels (zero crossings look at their sum).
pub fn snap_frame(doc: &DocState, settings: &SnapSettings, channels: ChannelMask, frames_per_px: f64, targets: &[u64], frame: u64) -> u64 {
    let len = doc.len_frames();
    let frame = frame.min(len);
    if !settings.enabled || !frames_per_px.is_finite() || frames_per_px <= 0.0 {
        return frame;
    }
    let reach = (SNAP_PX * frames_per_px).round() as u64;
    let near = |c: u64| c.abs_diff(frame) <= reach && c <= len;
    // (distance, target), the first found among equals is kept.
    let mut best: Option<(u64, u64)> = None;
    let mut offer = |c: u64| {
        let d = c.abs_diff(frame);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, c));
        }
    };
    if settings.markers {
        for m in doc.markers().iter() {
            for c in [m.start, m.end] {
                if near(c) {
                    offer(c);
                }
            }
        }
    }
    let tick = ruler_tick_near(frame, frames_per_px, doc.sample_rate());
    if near(tick) {
        offer(tick);
    }
    for &t in targets {
        if near(t) {
            offer(t);
        }
    }
    if settings.zero_crossings && channels.within(doc.channel_count()) != ChannelMask::NONE {
        let zc_reach = reach.min(u64::from(doc.sample_rate()) * ZERO_CROSSING_REACH_MS / 1000);
        let find = |toward| zero_crossing(doc, channels, frame, toward, zc_reach).ok().flatten();
        for c in [find(Toward::Left), find(Toward::Right)].into_iter().flatten() {
            offer(c);
        }
    }
    best.map_or(frame, |(_, c)| c)
}
