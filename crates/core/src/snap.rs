//! Snapping (f09, roadmap A123 and A127; f49, A125, A126 and A129): move a frame the pointer is
//! on to the nearest snap target within [`SNAP_PX`] pixels. Targets, each with its own switch:
//! the ruler's major ticks (coarse), its minor ticks (fine), the frames the caller lists
//! (selection edges, the cursor), the document's markers (f59, A124) and zero crossings of the
//! enabled channels. The selection gestures (f08) call [`snap_frame`] (or `Editor::snap`) for each frame they place.

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
    /// Snap to the ruler's major ticks, the ones with a time label (f49).
    pub ruler_coarse: bool,
    /// Snap to the ruler's minor ticks, the subdivisions between the labelled ones (f49).
    pub ruler_fine: bool,
    /// Snap to the frames the caller lists: selection edges and the cursor (f49).
    pub edges: bool,
    /// Also snap to markers: point markers and the edges of range markers (f59, A124).
    pub markers: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings { enabled: true, zero_crossings: false, ruler_coarse: true, ruler_fine: false, edges: true, markers: true }
    }
}

/// Pixels the ruler leaves for one labelled (major) tick.
pub const MAJOR_TICK_PX: f64 = 110.0;

/// The major tick step in seconds at this zoom: what the ruler labels.
pub fn major_step(frames_per_px: f64, sample_rate: u32) -> f64 {
    ruler_step(frames_per_px / f64::from(sample_rate.max(1)) * MAJOR_TICK_PX)
}

/// How many minor ticks one `major` step is divided into (10 for 1, 4 for 2, 5 for 5, ...).
pub fn minor_divisions(major: f64) -> u32 {
    const DIVISIONS: [(f64, u32); 19] = [
        (0.001, 10),
        (0.002, 4),
        (0.005, 5),
        (0.01, 10),
        (0.02, 4),
        (0.05, 5),
        (0.1, 10),
        (0.2, 4),
        (0.5, 5),
        (1.0, 10),
        (2.0, 4),
        (5.0, 5),
        (10.0, 10),
        (30.0, 6),
        (60.0, 6),
        (120.0, 4),
        (300.0, 5),
        (600.0, 5),
        (1800.0, 6),
    ];
    DIVISIONS.iter().find(|(m, _)| *m == major).map_or(6, |&(_, n)| n)
}

/// The minor tick step in seconds for a `major` step.
pub fn minor_step(major: f64) -> f64 {
    major / f64::from(minor_divisions(major))
}

/// The smallest "round" number of seconds that is at least `min_seconds`: the ruler's tick step.
pub fn ruler_step(min_seconds: f64) -> f64 {
    const STEPS: [f64; 19] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= min_seconds).unwrap_or(3600.0)
}

/// The multiple of `step` seconds nearest `frame`, as a frame.
fn tick_near(frame: u64, step: f64, sample_rate: u32) -> u64 {
    let rate = f64::from(sample_rate.max(1));
    let t = (frame as f64 / rate / step).round() * step;
    (t * rate).round().max(0.0) as u64
}

/// The ruler's major (labelled) tick nearest `frame` at this zoom.
pub fn ruler_tick_near(frame: u64, frames_per_px: f64, sample_rate: u32) -> u64 {
    tick_near(frame, major_step(frames_per_px, sample_rate), sample_rate)
}

/// The ruler's minor tick nearest `frame` at this zoom. Every major tick is also a minor tick.
pub fn ruler_minor_tick_near(frame: u64, frames_per_px: f64, sample_rate: u32) -> u64 {
    tick_near(frame, minor_step(major_step(frames_per_px, sample_rate)), sample_rate)
}

/// `frame` snapped, or unchanged when snapping is off or nothing is close. The nearest target
/// wins; on a tie a listed target or the ruler beats a zero crossing. The result is inside the
/// file. `channels` are the enabled channels (zero crossings look at their sum).
pub fn snap_frame(doc: &DocState, settings: &SnapSettings, channels: ChannelMask, frames_per_px: f64, targets: &[u64], frame: u64) -> u64 {
    snap_frame_steps(doc, settings, channels, frames_per_px, targets, frame, None)
}

/// [`snap_frame`] with the ruler's tick steps given: `Some((major, minor))` in seconds, as the
/// window's time display draws them (SMPTE and bars-and-beats rulers do not tick in decimal
/// seconds); `None` is the decimal ruler.
pub fn snap_frame_steps(
    doc: &DocState,
    settings: &SnapSettings,
    channels: ChannelMask,
    frames_per_px: f64,
    targets: &[u64],
    frame: u64,
    steps: Option<(f64, f64)>,
) -> u64 {
    let steps = steps.filter(|(major, minor)| major.is_finite() && minor.is_finite() && *major > 0.0 && *minor > 0.0);
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
    if settings.ruler_coarse {
        let tick = match steps {
            Some((major, _)) => tick_near(frame, major, doc.sample_rate()),
            None => ruler_tick_near(frame, frames_per_px, doc.sample_rate()),
        };
        if near(tick) {
            offer(tick);
        }
    }
    if settings.ruler_fine {
        let tick = match steps {
            Some((_, minor)) => tick_near(frame, minor, doc.sample_rate()),
            None => ruler_minor_tick_near(frame, frames_per_px, doc.sample_rate()),
        };
        if near(tick) {
            offer(tick);
        }
    }
    if settings.edges {
        for &t in targets {
            if near(t) {
                offer(t);
            }
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
