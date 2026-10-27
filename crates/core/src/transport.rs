//! Transport logic without a window or a sound device (task f23): where playback starts and
//! stops, how it wraps in a loop, which part it skips, and how the view follows the playhead.
//! The player's audio callback and a silent test clock both call [`Plan::normalize`] and
//! [`Plan::advance`], and the app keeps the mode and options in [`Transport`]. Everything here is
//! plain arithmetic on frame positions: it never blocks and never allocates.

use std::ops::Range;

use crate::view::Viewport;

/// Skip Selection plays this long before the skipped part, to hear the cut's lead-in.
pub const SKIP_PREROLL_SECS: f64 = 2.0;
/// ... and this long after it.
pub const SKIP_POSTROLL_SECS: f64 = 2.0;

/// What the transport is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl Mode {
    /// The name `STATE` reports.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Stopped => "stopped",
            Mode::Playing => "playing",
            Mode::Paused => "paused",
        }
    }
}

/// How the view follows the playhead during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Follow {
    /// The view jumps a page when the playhead leaves it (the default).
    #[default]
    Page,
    /// The playhead stays in the middle of the view.
    Center,
}

impl Follow {
    pub fn name(self) -> &'static str {
        match self {
            Follow::Page => "page",
            Follow::Center => "center",
        }
    }
}

/// The part of the file being played: `[start, end)` in file frames, whether it repeats, and a
/// part inside it that is jumped over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    pub start: f64,
    pub end: f64,
    pub looping: bool,
    /// `[skip_start, skip_end)`; empty (`skip_end <= skip_start`) means nothing is skipped.
    pub skip_start: f64,
    pub skip_end: f64,
}

impl Plan {
    pub fn new(start: f64, end: f64) -> Plan {
        Plan { start, end, looping: false, skip_start: 0.0, skip_end: 0.0 }
    }

    fn skip(&self) -> Option<(f64, f64)> {
        (self.skip_end > self.skip_start).then_some((self.skip_start, self.skip_end))
    }

    /// Frames played in one pass: the region less the skipped part inside it.
    pub fn period(&self) -> f64 {
        let region = (self.end - self.start).max(0.0);
        let skipped = self.skip().map_or(0.0, |(a, b)| (b.min(self.end) - a.max(self.start)).max(0.0));
        (region - skipped).max(0.0)
    }

    /// Where playback really is when it has reached `pos`: a position inside the skipped part
    /// moves to its end; a position past the end wraps to the start in a loop (keeping the
    /// overshoot) or is `None` (finished). Positions before the start are played from there.
    pub fn normalize(&self, pos: f64) -> Option<f64> {
        if !pos.is_finite() || self.period() <= 0.0 {
            return None;
        }
        let mut pos = pos.max(self.start);
        // A pass can cross the skip and the end: a few steps always settle it.
        for _ in 0..4 {
            if let Some((a, b)) = self.skip()
                && pos >= a
                && pos < b
            {
                pos = b;
            }
            if pos >= self.end {
                if !self.looping {
                    return None;
                }
                let span = self.end - self.start;
                pos = self.start + (pos - self.end) % span;
                continue;
            }
            return Some(pos);
        }
        None
    }

    /// Move `delta` frames forward from `pos` (a position `normalize` gave), jumping the skipped
    /// part and wrapping in a loop. `None` when playback finishes on the way. `wraps` counts the
    /// times it went round.
    pub fn advance(&self, pos: f64, delta: f64, wraps: &mut u32) -> Option<f64> {
        let mut pos = self.normalize(pos)?;
        let mut left = delta.max(0.0);
        if self.looping {
            // Whole passes bring it back to the same place.
            let period = self.period();
            let passes = (left / period).floor();
            if passes >= 1.0 {
                *wraps = wraps.saturating_add(passes.min(f64::from(u32::MAX)) as u32);
                left -= passes * period;
            }
        }
        for _ in 0..8 {
            let boundary = match self.skip() {
                Some((a, _)) if pos < a => a.min(self.end),
                _ => self.end,
            };
            if pos + left < boundary {
                return Some(pos + left);
            }
            left -= boundary - pos;
            let wrapped = boundary >= self.end;
            pos = self.normalize(boundary)?;
            if wrapped {
                *wraps = wraps.saturating_add(1);
            }
        }
        Some(pos)
    }

    /// Like [`Plan::normalize`] for playing backwards: valid positions are `[start, end)`; one
    /// at or past the end is brought just inside it, one inside the skipped part moves to just
    /// before it, and one before the start wraps to the end in a loop or is `None` (finished).
    pub fn normalize_rev(&self, pos: f64) -> Option<f64> {
        if !pos.is_finite() || self.period() <= 0.0 {
            return None;
        }
        let mut pos = pos.min(self.end - REV_EPS);
        for _ in 0..4 {
            if let Some((a, b)) = self.skip()
                && pos >= a
                && pos < b
            {
                pos = a - REV_EPS;
            }
            if pos < self.start {
                if !self.looping {
                    return None;
                }
                let span = self.end - self.start;
                pos = self.end - (self.start - pos) % span;
                if pos >= self.end {
                    pos = self.end - REV_EPS;
                }
                continue;
            }
            return Some(pos);
        }
        None
    }

    /// Move `delta` frames from `pos`: forward for a positive `delta` (as [`Plan::advance`]),
    /// backward for a negative one, jumping the skipped part and wrapping in a loop. `None` when
    /// playback finishes on the way.
    pub fn advance_signed(&self, pos: f64, delta: f64, wraps: &mut u32) -> Option<f64> {
        if delta >= 0.0 {
            return self.advance(pos, delta, wraps);
        }
        let mut pos = self.normalize_rev(pos)?;
        let mut left = -delta;
        if self.looping {
            let period = self.period();
            let passes = (left / period).floor();
            if passes >= 1.0 {
                *wraps = wraps.saturating_add(passes.min(f64::from(u32::MAX)) as u32);
                left -= passes * period;
            }
        }
        for _ in 0..8 {
            let skip_below = self.skip().filter(|(_, b)| pos >= *b);
            let boundary = skip_below.map_or(self.start, |(_, b)| b);
            if pos - left >= boundary {
                return self.normalize_rev(pos - left);
            }
            left -= pos - boundary;
            match skip_below {
                // Over the skipped part: on from just before it.
                Some((a, _)) if a > self.start => pos = a,
                // Past the start: finished, or round to the end in a loop.
                _ => {
                    if !self.looping {
                        return None;
                    }
                    pos = self.end;
                    *wraps = wraps.saturating_add(1);
                }
            }
        }
        self.normalize_rev(pos)
    }
}

impl Plan {
    /// Where playback is `delta` (>= 0) frames of play after `from`, going backward when `rev`:
    /// the one rule the player's read-ahead thread and the window both use to map the play
    /// order onto file frames. `None` when playback has finished by then.
    pub fn position_after(&self, from: f64, delta: f64, rev: bool, wraps: &mut u32) -> Option<f64> {
        match (rev, delta > 0.0) {
            (false, _) => self.advance(from, delta, wraps),
            (true, false) => self.normalize_rev(from),
            (true, true) => self.advance_signed(from, -delta, wraps),
        }
    }

    /// How many frames play from `from` (as `normalize` / `normalize_rev` gave it) until
    /// playback finishes, going backward when `rev`; infinite in a loop, 0 when nothing plays.
    pub fn frames_from(&self, from: f64, rev: bool) -> f64 {
        if self.looping {
            return if self.period() > 0.0 { f64::INFINITY } else { 0.0 };
        }
        let Some(p) = (if rev { self.normalize_rev(from) } else { self.normalize(from) }) else { return 0.0 };
        let skipped = |lo: f64, hi: f64| self.skip().map_or(0.0, |(a, b)| (b.min(hi) - a.max(lo)).max(0.0));
        if rev {
            let top = p.floor() + 1.0;
            ((top - self.start) - skipped(self.start, top)).max(0.0)
        } else {
            ((self.end - p) - skipped(p, self.end)).max(0.0)
        }
    }

    /// How many consecutive file frames, starting at the one `pos` falls in and going backward
    /// when `rev`, are played one after the other with no jump (at least 1): the run the
    /// read-ahead thread can copy from the store in one read.
    pub fn run_len(&self, pos: f64, rev: bool) -> u64 {
        if rev {
            let boundary = match self.skip() {
                Some((_, b)) if pos >= b => b,
                _ => self.start,
            };
            ((pos.floor() - boundary.ceil()) + 1.0).max(1.0) as u64
        } else {
            let boundary = match self.skip() {
                Some((a, _)) if pos < a => a.min(self.end),
                _ => self.end,
            };
            (boundary - pos.floor()).ceil().max(1.0) as u64
        }
    }
}

/// A hair, so a backward position just inside a boundary is not read as on the other side.
const REV_EPS: f64 = 1e-6;

/// Shuttle speeds, one per press of L (or J): 1x, 2x, 4x, 8x.
pub const SHUTTLE_SPEEDS: [f64; 4] = [1.0, 2.0, 4.0, 8.0];
/// The slow speed of K+J / K+L (held).
pub const NUDGE_SPEED: f64 = 0.25;
/// Rewind and Fast Forward move the playhead this far at a click.
pub const STEP_SECS: f64 = 1.0;
/// Held longer than this, the Rewind / Fast Forward buttons shuttle instead of stepping.
pub const HOLD_SECS: f64 = 0.35;
/// ... at this level of [`SHUTTLE_SPEEDS`] (4x).
pub const HOLD_LEVEL: i8 = 3;

/// The JKL shuttle: `level` runs from -4 (8x backward) to 4 (8x forward), 0 is stopped; `nudge`
/// is -1 or +1 while K+J or K+L is held (a slow crawl that wins over the level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shuttle {
    pub level: i8,
    pub nudge: i8,
}

impl Shuttle {
    /// L: play forward; each further press goes up a speed (the top one stays). Coming from
    /// backward it starts forward at 1x.
    pub fn forward(&mut self) {
        self.forward_in(SHUTTLE_SPEEDS.len());
    }

    /// J: the same, backward.
    pub fn backward(&mut self) {
        self.backward_in(SHUTTLE_SPEEDS.len());
    }

    /// [`Shuttle::forward`] over a ladder of `top` speeds.
    pub fn forward_in(&mut self, top: usize) {
        let top = top.clamp(1, 100) as i8;
        self.nudge = 0;
        self.level = if self.level >= 1 { (self.level + 1).min(top) } else { 1 };
    }

    /// [`Shuttle::backward`] over a ladder of `top` speeds.
    pub fn backward_in(&mut self, top: usize) {
        let top = top.clamp(1, 100) as i8;
        self.nudge = 0;
        self.level = if self.level <= -1 { (self.level - 1).max(-top) } else { -1 };
    }

    /// K: stop.
    pub fn stop(&mut self) {
        *self = Shuttle::default();
    }

    /// K+L (`dir` 1) or K+J (`dir` -1) held.
    pub fn nudge_held(&mut self, dir: i8) {
        self.level = 0;
        self.nudge = dir.signum();
    }

    /// The signed speed, 0 when stopped.
    pub fn speed(&self) -> f64 {
        self.speed_in(&SHUTTLE_SPEEDS)
    }

    /// The signed speed with `ladder` as the speeds the levels stand for (ascending, never empty
    /// in practice; empty falls back to the default ladder), 0 when stopped.
    pub fn speed_in(&self, ladder: &[f64]) -> f64 {
        let ladder = if ladder.is_empty() { &SHUTTLE_SPEEDS[..] } else { ladder };
        if self.nudge != 0 {
            return f64::from(self.nudge.signum()) * NUDGE_SPEED;
        }
        let i = usize::from(self.level.unsigned_abs());
        match i {
            0 => 0.0,
            _ => f64::from(self.level.signum()) * ladder[(i - 1).min(ladder.len() - 1)],
        }
    }

    /// Whether anything is shuttling.
    pub fn active(&self) -> bool {
        self.level != 0 || self.nudge != 0
    }
}

/// The speeds the J and L keys step through, from the preference's list (`jkl_speed_steps`): the
/// ones at normal speed or faster, ascending. The first press plays at the slowest of them (1x
/// in the default list), each further press goes up one. An empty result falls back to
/// [`SHUTTLE_SPEEDS`].
pub fn shuttle_ladder(steps: &[f32]) -> Vec<f64> {
    let mut ladder: Vec<f64> = steps.iter().map(|s| f64::from(*s)).filter(|s| s.is_finite() && *s >= 1.0).collect();
    ladder.sort_by(f64::total_cmp);
    ladder.dedup();
    if ladder.is_empty() { SHUTTLE_SPEEDS.to_vec() } else { ladder }
}

/// The readout in the Transport panel: "1x", "-2x", "0.25x".
pub fn speed_label(speed: f64) -> String {
    let mut s = format!("{speed:.2}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    format!("{s}x")
}

/// What a shuttle starts playing from a stop: the selection if there is one, else the whole file
/// (so it can run both ways), from the cursor going forward, or from the selection's end / the
/// cursor going backward. `None` when there is nothing to play that way.
pub fn shuttle_plan(selection: &Range<usize>, cursor: usize, frames: usize, looping: bool, backward: bool) -> Option<(Plan, f64)> {
    let total = frames as f64;
    let (sel_a, sel_b) = (selection.start.min(frames) as f64, selection.end.min(frames) as f64);
    let (mut plan, from) = if sel_b > sel_a {
        (Plan::new(sel_a, sel_b), if backward { sel_b - 1.0 } else { sel_a })
    } else {
        let c = (cursor as f64).min(total);
        (Plan::new(0.0, total), if backward { c - 1.0 } else { c })
    };
    plan.looping = looping;
    let ok = if backward { plan.normalize_rev(from).is_some() } else { plan.normalize(from).is_some() };
    ok.then_some((plan, from))
}

/// Scrubbing grains: each drag step plays this much of the file (times the speed).
pub const GRAIN_SECS: f64 = 0.08;
/// The speed of a scrub grain never goes beyond this, either way.
pub const SCRUB_MAX_SPEED: f64 = 4.0;
/// ... or below this (a slow drag still sounds).
pub const SCRUB_MIN_SPEED: f64 = 0.25;
/// The fade at each end of a grain, so it does not click.
pub const GRAIN_FADE_SECS: f64 = 0.006;

/// Follows a drag of the playhead: the speed of each step comes from how far and how fast it moved.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scrub {
    last: Option<(f64, f64)>,
}

impl Scrub {
    /// The playhead is at `frame` at time `now` (seconds). Returns the signed speed of the grain
    /// to play there: the drag's speed in file-seconds per second, clamped; 1x for the first.
    pub fn touch(&mut self, frame: f64, now: f64, rate: u32) -> f64 {
        let speed = match self.last {
            Some((f0, t0)) if now > t0 && frame != f0 => {
                let v = (frame - f0) / ((now - t0) * f64::from(rate.max(1)));
                v.signum() * v.abs().clamp(SCRUB_MIN_SPEED, SCRUB_MAX_SPEED)
            }
            // Not moving: keep the direction it last had, forward at the start.
            Some((f0, _)) if frame == f0 => SCRUB_MIN_SPEED,
            _ => 1.0,
        };
        self.last = Some((frame, now));
        speed
    }

    pub fn end(&mut self) {
        self.last = None;
    }
}

/// The grain to play for the playhead at `frame` going at `speed`: a plan and the frame to
/// start at (backward grains run from `frame` down).
pub fn grain_plan(frame: f64, speed: f64, rate: u32, frames: usize) -> Option<(Plan, f64)> {
    let total = frames as f64;
    let len = (GRAIN_SECS * f64::from(rate.max(1)) * speed.abs()).max(2.0);
    let frame = frame.clamp(0.0, total);
    if speed >= 0.0 {
        let plan = Plan::new(frame, (frame + len).min(total));
        plan.normalize(frame).map(|from| (plan, from))
    } else {
        let plan = Plan::new((frame - len).max(0.0), frame);
        let from = frame - 1.0;
        plan.normalize_rev(from).map(|from| (plan, from))
    }
}

/// The gain of a grain at `pos`: a short fade in at the start and out at the end of `[start, end)`.
pub fn grain_gain(pos: f64, start: f64, end: f64, fade_frames: f64) -> f32 {
    let fade = fade_frames.max(1.0).min((end - start) * 0.5);
    if fade <= 0.0 {
        return 1.0;
    }
    let edge = (pos - start).min(end - pos);
    (edge / fade).clamp(0.0, 1.0) as f32
}

/// What Play is asked to play.
#[derive(Debug, Clone)]
pub struct Request {
    pub selection: Range<usize>,
    pub cursor: usize,
    pub frames: usize,
    /// The part of the file on screen (for Loop with nothing selected).
    pub view: Range<f64>,
    pub rate: u32,
    pub looping: bool,
    pub skip_selection: bool,
    /// Frames to start before a selection and to carry on after it (0: off).
    pub preroll: usize,
    pub postroll: usize,
}

/// The plan for `req` and the frame to start at, or `None` when there is nothing to play.
///
/// - Skip Selection with a selection: from [`SKIP_PREROLL_SECS`] before it to
///   [`SKIP_POSTROLL_SECS`] after it, jumping over the selection;
/// - a selection: just the selection, or with pre/post-roll from `preroll` before it to
///   `postroll` after it;
/// - Loop with no selection: the view, from the cursor if it is in view;
/// - otherwise: from the cursor to the end of the file.
pub fn plan_for(req: &Request) -> Option<(Plan, f64)> {
    let frames = req.frames as f64;
    let rate = f64::from(req.rate.max(1));
    let (sel_a, sel_b) = (req.selection.start.min(req.frames) as f64, req.selection.end.min(req.frames) as f64);
    let has_sel = sel_b > sel_a;
    let (mut plan, from) = if has_sel && req.skip_selection {
        let start = (sel_a - SKIP_PREROLL_SECS * rate).max(0.0);
        let end = (sel_b + SKIP_POSTROLL_SECS * rate).min(frames);
        let mut p = Plan::new(start, end);
        p.skip_start = sel_a;
        p.skip_end = sel_b;
        (p, start)
    } else if has_sel {
        let start = (sel_a - req.preroll as f64).max(0.0);
        (Plan::new(start, (sel_b + req.postroll as f64).min(frames)), start)
    } else if req.looping {
        let (a, b) = (req.view.start.max(0.0), req.view.end.min(frames));
        let cursor = req.cursor as f64;
        (Plan::new(a, b), if cursor >= a && cursor < b { cursor } else { a })
    } else {
        let c = (req.cursor as f64).min(frames);
        (Plan::new(c, frames), c)
    };
    plan.looping = req.looping;
    (plan.period() > 0.0).then_some((plan, from))
}

/// Move `view` so the playhead is followed (the caller skips this when auto-scroll is off).
pub fn follow_view(mode: Follow, view: &mut Viewport, playhead: f64, width_px: f32, total_frames: usize) {
    let span = view.frames_per_px * f64::from(width_px);
    match mode {
        Follow::Page => {
            if playhead < view.start || playhead >= view.end(width_px) {
                // The playhead lands a twentieth in from the left of the new page.
                view.start = playhead - span * 0.05;
            }
        }
        Follow::Center => view.start = playhead - span * 0.5,
    }
    view.clamp(width_px, total_frames);
}

/// The transport's mode and options (what the Transport panel shows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    pub mode: Mode,
    pub looping: bool,
    pub skip_selection: bool,
    /// On stop the cursor goes back to where play started (Audition's default; Shift+X).
    pub return_on_stop: bool,
    pub follow: Follow,
    /// Auto-scroll during playback; a manual scroll turns it off.
    pub auto_scroll: bool,
    /// Where the current or last play started.
    pub play_start: usize,
    /// Play a selection with its pre-roll and post-roll.
    pub roll: bool,
    /// The JKL shuttle.
    pub shuttle: Shuttle,
}

impl Default for Transport {
    fn default() -> Transport {
        Transport {
            mode: Mode::Stopped,
            looping: false,
            skip_selection: false,
            return_on_stop: true,
            follow: Follow::Page,
            auto_scroll: true,
            play_start: 0,
            roll: false,
            shuttle: Shuttle::default(),
        }
    }
}

impl Transport {
    /// Where the cursor goes when playback stops at `stopped_at` after starting at `play_start`.
    pub fn cursor_after_stop(&self, stopped_at: usize) -> usize {
        if self.return_on_stop { self.play_start } else { stopped_at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looped(start: f64, end: f64) -> Plan {
        Plan { looping: true, ..Plan::new(start, end) }
    }

    fn skipping(start: f64, end: f64, a: f64, b: f64) -> Plan {
        Plan { skip_start: a, skip_end: b, ..Plan::new(start, end) }
    }

    #[test]
    fn a_plain_range_plays_to_its_end_and_finishes() {
        let p = Plan::new(10.0, 20.0);
        let mut w = 0;
        assert_eq!(p.advance(10.0, 4.0, &mut w), Some(14.0));
        assert_eq!(p.advance(14.0, 5.9, &mut w), Some(19.9));
        assert_eq!(p.advance(14.0, 6.0, &mut w), None, "reaching the end finishes");
        assert_eq!(p.advance(14.0, 600.0, &mut w), None);
        assert_eq!(w, 0);
        assert_eq!(p.normalize(5.0), Some(10.0), "before the start plays from the start");
    }

    #[test]
    fn a_loop_wraps_and_keeps_the_overshoot() {
        let p = looped(100.0, 200.0);
        let mut w = 0;
        assert_eq!(p.normalize(230.0), Some(130.0));
        assert_eq!(p.advance(190.0, 25.0, &mut w), Some(115.0));
        assert_eq!(w, 1);
        // Many passes at once (a long gap between ticks) land in the same place.
        let mut w = 0;
        assert_eq!(p.advance(150.0, 1000.0 + 20.0, &mut w), Some(170.0));
        assert_eq!(w, 10);
        // Exactly the end is the start of the next pass.
        let mut w = 0;
        assert_eq!(p.advance(150.0, 50.0, &mut w), Some(100.0));
        assert_eq!(w, 1);
    }

    #[test]
    fn the_skipped_part_is_jumped_not_played() {
        let p = skipping(0.0, 100.0, 40.0, 60.0);
        let mut w = 0;
        assert_eq!(p.period(), 80.0);
        assert_eq!(p.advance(30.0, 5.0, &mut w), Some(35.0));
        assert_eq!(p.advance(30.0, 10.0, &mut w), Some(60.0), "arrives at the skip's start and jumps");
        assert_eq!(p.advance(30.0, 15.0, &mut w), Some(65.0), "5 frames of the 15 after the jump");
        assert_eq!(p.advance(30.0, 70.0, &mut w), None, "10 + 60 reaches the end");
        assert_eq!(p.normalize(50.0), Some(60.0), "starting inside the skip");
        for pos in 0..100 {
            let n = p.normalize(f64::from(pos));
            assert!(n.is_some_and(|n| !(40.0..60.0).contains(&n)), "{pos} -> {n:?}");
        }
    }

    #[test]
    fn a_loop_with_a_skip_goes_round_without_the_skip() {
        let mut p = skipping(0.0, 100.0, 40.0, 60.0);
        p.looping = true;
        let mut w = 0;
        assert_eq!(p.advance(90.0, 20.0, &mut w), Some(10.0));
        assert_eq!(w, 1);
        assert_eq!(p.advance(90.0, 20.0 + 80.0, &mut w), Some(10.0), "a whole extra pass");
        assert_eq!(w, 3);
    }

    #[test]
    fn nothing_to_play_never_loops_forever() {
        let whole = Plan { looping: true, ..skipping(0.0, 50.0, 0.0, 80.0) };
        let mut w = 0;
        assert_eq!(whole.period(), 0.0);
        assert_eq!(whole.normalize(10.0), None);
        assert_eq!(whole.advance(10.0, 5.0, &mut w), None);
        assert_eq!(looped(5.0, 5.0).normalize(5.0), None);
        assert_eq!(Plan::new(0.0, 10.0).normalize(f64::NAN), None);
    }

    fn req() -> Request {
        Request {
            selection: 0..0,
            cursor: 0,
            frames: 1_000_000,
            view: 0.0..1000.0,
            rate: 48_000,
            looping: false,
            skip_selection: false,
            preroll: 0,
            postroll: 0,
        }
    }

    #[test]
    fn play_starts_at_the_cursor_and_runs_to_the_end() {
        let r = Request { cursor: 5000, ..req() };
        assert_eq!(plan_for(&r), Some((Plan::new(5000.0, 1_000_000.0), 5000.0)));
        assert_eq!(plan_for(&Request { cursor: 1_000_000, ..req() }), None, "at the end: nothing to play");
    }

    #[test]
    fn a_selection_is_played_and_looped_on_request() {
        let r = Request { selection: 100..900, cursor: 100, ..req() };
        assert_eq!(plan_for(&r), Some((Plan::new(100.0, 900.0), 100.0)));
        let l = Request { looping: true, ..r };
        assert_eq!(plan_for(&l), Some((looped(100.0, 900.0), 100.0)));
    }

    #[test]
    fn loop_without_a_selection_loops_the_view_from_the_cursor_if_it_is_in_view() {
        let r = Request { looping: true, view: 2000.0..6000.0, cursor: 3000, ..req() };
        assert_eq!(plan_for(&r), Some((looped(2000.0, 6000.0), 3000.0)));
        let outside = Request { cursor: 10, ..r.clone() };
        assert_eq!(plan_for(&outside), Some((looped(2000.0, 6000.0), 2000.0)));
        // A view hanging over the end of the file is cut to the file.
        let over = Request { view: 900_000.0..2_000_000.0, cursor: 0, ..r };
        assert_eq!(plan_for(&over), Some((looped(900_000.0, 1_000_000.0), 900_000.0)));
    }

    #[test]
    fn skip_selection_plays_from_before_to_after_the_selection() {
        let rate = 48_000;
        let r = Request { selection: 200_000..300_000, cursor: 200_000, skip_selection: true, rate, ..req() };
        let (plan, from) = plan_for(&r).unwrap_or((Plan::new(0.0, 0.0), -1.0));
        assert_eq!((plan.start, plan.end), (200_000.0 - 96_000.0, 300_000.0 + 96_000.0));
        assert_eq!((plan.skip_start, plan.skip_end), (200_000.0, 300_000.0));
        assert_eq!(from, plan.start);
        // Near the file's edges the lead-in and tail are cut to the file.
        let edge = Request { selection: 1000..2000, ..r.clone() };
        assert_eq!(plan_for(&edge).map(|(p, _)| p.start), Some(0.0));
        let tail = Request { selection: 999_000..1_000_000, ..r };
        assert_eq!(plan_for(&tail).map(|(p, _)| p.end), Some(1_000_000.0));
        // With no selection the option changes nothing.
        let none = Request { skip_selection: true, cursor: 7, ..req() };
        assert_eq!(plan_for(&none), Some((Plan::new(7.0, 1_000_000.0), 7.0)));
    }

    #[test]
    fn stop_returns_the_cursor_to_the_start_unless_turned_off() {
        let mut t = Transport { play_start: 1234, ..Transport::default() };
        assert_eq!(t.cursor_after_stop(99_999), 1234, "Audition's default");
        t.return_on_stop = false;
        assert_eq!(t.cursor_after_stop(99_999), 99_999);
    }

    #[test]
    fn the_view_follows_pagewise_or_centred() {
        let (width, total) = (1000.0f32, 1_000_000usize);
        let mut v = Viewport { start: 0.0, frames_per_px: 10.0 };
        follow_view(Follow::Page, &mut v, 5000.0, width, total);
        assert_eq!(v.start, 0.0, "in view: no move");
        follow_view(Follow::Page, &mut v, 10_000.0, width, total);
        assert_eq!(v.start, 10_000.0 - 500.0, "off the right edge: a new page with the playhead near its left");
        follow_view(Follow::Page, &mut v, 3000.0, width, total);
        assert_eq!(v.start, 3000.0 - 500.0, "back behind the view: a page back");
        follow_view(Follow::Center, &mut v, 400_000.0, width, total);
        assert_eq!(v.start, 400_000.0 - 5000.0);
        follow_view(Follow::Center, &mut v, 100.0, width, total);
        assert_eq!(v.start, 0.0, "centred at the start: clamped to the file");
    }

    #[test]
    fn backward_play_walks_down_wraps_and_finishes() {
        let p = Plan::new(100.0, 200.0);
        let mut w = 0;
        assert_eq!(p.advance_signed(150.0, -20.0, &mut w), Some(130.0));
        assert_eq!(p.advance_signed(150.0, -50.0, &mut w), Some(100.0), "the start itself is still played");
        assert_eq!(p.advance_signed(150.0, -51.0, &mut w), None, "going past it finishes");
        assert_eq!(p.normalize_rev(200.0), Some(200.0 - 1e-6), "the end itself is just inside");
        let l = looped(100.0, 200.0);
        assert!(l.advance_signed(110.0, -25.0, &mut w).is_some_and(|v| (v - 185.0).abs() < 1e-3));
        assert_eq!(w, 1);
        let mut w = 0;
        assert_eq!(l.advance_signed(150.0, -1020.0, &mut w).map(|v| v.round()), Some(130.0));
        assert_eq!(w, 10);
        assert_eq!(p.advance_signed(120.0, 5.0, &mut w), Some(125.0), "a positive delta is forward");
    }

    #[test]
    fn backward_play_jumps_the_skipped_part() {
        let p = skipping(0.0, 100.0, 40.0, 60.0);
        let mut w = 0;
        assert_eq!(p.advance_signed(70.0, -5.0, &mut w), Some(65.0));
        let at = p.advance_signed(70.0, -15.0, &mut w).unwrap_or(-1.0);
        assert!((at - 35.0).abs() < 1e-3, "10 down to the skip's end, 5 more below its start: {at}");
        assert!(p.normalize_rev(50.0).is_some_and(|v| v < 40.0), "starting inside the skip");
        for pos in 0..100 {
            let n = p.normalize_rev(f64::from(pos));
            assert!(n.is_some_and(|n| !(40.0..60.0).contains(&n)), "{pos} -> {n:?}");
        }
    }

    #[test]
    fn l_steps_the_speed_up_j_the_same_backwards_k_stops() {
        let mut s = Shuttle::default();
        assert_eq!(s.speed(), 0.0);
        let mut seen = Vec::new();
        for _ in 0..6 {
            s.forward();
            seen.push(s.speed());
        }
        assert_eq!(seen, [1.0, 2.0, 4.0, 8.0, 8.0, 8.0], "the top speed stays");
        s.backward();
        assert_eq!(s.speed(), -1.0, "from forward, J starts backward at 1x");
        s.backward();
        s.backward();
        assert_eq!(s.speed(), -4.0);
        s.stop();
        assert!(!s.active() && s.speed() == 0.0);
        s.nudge_held(1);
        assert_eq!(s.speed(), NUDGE_SPEED);
        s.nudge_held(-1);
        assert_eq!(s.speed(), -NUDGE_SPEED);
        s.forward();
        assert_eq!((s.speed(), s.nudge), (1.0, 0), "a plain L ends the nudge");
    }

    #[test]
    fn the_speed_readout() {
        assert_eq!(speed_label(1.0), "1x");
        assert_eq!(speed_label(-2.0), "-2x");
        assert_eq!(speed_label(0.25), "0.25x");
        assert_eq!(speed_label(0.0), "0x");
        assert_eq!(speed_label(8.0), "8x");
    }

    #[test]
    fn a_shuttle_starts_from_the_selection_or_the_whole_file() {
        let none = 500..500;
        let (p, from) = shuttle_plan(&none, 500, 1000, false, false).expect("plan");
        assert_eq!((p.start, p.end, from), (0.0, 1000.0, 500.0));
        let (p, from) = shuttle_plan(&none, 500, 1000, false, true).expect("plan");
        assert_eq!((p.start, p.end, from), (0.0, 1000.0, 499.0), "backward runs down to the file's start");
        let (p, from) = shuttle_plan(&(200..300), 0, 1000, true, true).expect("plan");
        assert_eq!((p.start, p.end, from, p.looping), (200.0, 300.0, 299.0, true));
        assert!(shuttle_plan(&(0..0), 0, 1000, false, true).is_none(), "nothing before the start");
        assert!(shuttle_plan(&(1000..1000), 1000, 1000, false, false).is_none(), "nothing after the end");
    }

    #[test]
    fn scrub_speed_follows_the_drag() {
        let mut s = Scrub::default();
        assert_eq!(s.touch(1000.0, 0.0, 1000), 1.0, "the first touch plays at 1x");
        assert!((s.touch(1100.0, 0.1, 1000) - 1.0).abs() < 1e-9, "100 frames in 0.1 s at 1 kHz");
        assert_eq!(s.touch(1500.0, 0.2, 1000), SCRUB_MAX_SPEED.min(4.0), "4x, clamped");
        assert!((s.touch(1400.0, 0.3, 1000) + 1.0).abs() < 1e-9, "backward");
        assert_eq!(s.touch(1399.0, 0.4, 1000), -SCRUB_MIN_SPEED, "a crawl is still heard");
        assert_eq!(s.touch(1399.0, 0.5, 1000), SCRUB_MIN_SPEED, "held still");
        s.end();
        assert_eq!(s.touch(0.0, 9.0, 1000), 1.0);
    }

    #[test]
    fn grains_run_forward_or_down_from_the_playhead() {
        let (p, from) = grain_plan(1000.0, 1.0, 48_000, 100_000).expect("grain");
        assert_eq!((p.start, from), (1000.0, 1000.0));
        assert!((p.end - p.start - 3840.0).abs() < 1.0, "80 ms at 48 kHz");
        let (p, from) = grain_plan(10_000.0, -2.0, 48_000, 100_000).expect("grain");
        assert_eq!(p.end, 10_000.0);
        assert_eq!(from, 9999.0);
        assert!((p.end - p.start - 7680.0).abs() < 1.0, "twice as much file");
        assert!(grain_plan(0.0, -1.0, 48_000, 100_000).is_none(), "nothing behind the start");
        assert!(grain_plan(100_000.0, 1.0, 48_000, 100_000).is_none(), "nothing after the end");
        assert_eq!(grain_gain(0.0, 0.0, 1000.0, 100.0), 0.0);
        assert_eq!(grain_gain(50.0, 0.0, 1000.0, 100.0), 0.5);
        assert_eq!(grain_gain(500.0, 0.0, 1000.0, 100.0), 1.0);
        assert_eq!(grain_gain(975.0, 0.0, 1000.0, 100.0), 0.25);
    }

    #[test]
    fn pre_and_post_roll_widen_a_selection_but_not_past_the_file() {
        let r = Request { selection: 5000..9000, cursor: 5000, preroll: 2000, postroll: 1000, ..req() };
        assert_eq!(plan_for(&r), Some((Plan::new(3000.0, 10_000.0), 3000.0)));
        let edge = Request { selection: 500..999_500, cursor: 500, preroll: 2000, postroll: 1000, ..req() };
        assert_eq!(plan_for(&edge), Some((Plan::new(0.0, 1_000_000.0), 0.0)));
        let off = Request { selection: 5000..9000, cursor: 5000, ..req() };
        assert_eq!(plan_for(&off), Some((Plan::new(5000.0, 9000.0), 5000.0)), "off: just the selection");
        // No selection: roll changes nothing.
        let cur = Request { cursor: 7, preroll: 2000, postroll: 1000, ..req() };
        assert_eq!(plan_for(&cur), Some((Plan::new(7.0, 1_000_000.0), 7.0)));
    }
}

#[cfg(test)]
mod play_order_tests {
    use super::*;

    /// The file frames played, in order, by walking `position_after` one frame at a time.
    fn walk(plan: &Plan, from: f64, rev: bool, limit: usize) -> Vec<u64> {
        let mut out = Vec::new();
        let mut w = 0;
        while out.len() < limit {
            let Some(p) = plan.position_after(from, out.len() as f64, rev, &mut w) else { break };
            out.push(p.floor() as u64);
        }
        out
    }

    fn plans() -> Vec<Plan> {
        let mut skip = Plan::new(0.0, 100.0);
        skip.skip_start = 40.0;
        skip.skip_end = 60.0;
        vec![Plan::new(10.0, 50.0), skip, Plan { looping: true, ..Plan::new(10.0, 14.0) }]
    }

    #[test]
    fn frames_from_counts_exactly_what_the_walk_plays() {
        for plan in plans() {
            for rev in [false, true] {
                let from = if rev { plan.end - 1.0 } else { plan.start };
                let n = plan.frames_from(from, rev);
                let walked = walk(&plan, from, rev, 1000).len();
                if plan.looping {
                    assert!(n.is_infinite());
                    assert_eq!(walked, 1000);
                } else {
                    assert_eq!(n as usize, walked, "{plan:?} rev={rev}");
                }
            }
        }
        assert_eq!(Plan::new(5.0, 5.0).frames_from(5.0, false), 0.0);
    }

    #[test]
    fn run_len_never_crosses_a_jump() {
        for plan in plans() {
            for rev in [false, true] {
                let from = if rev { plan.end - 1.0 } else { plan.start };
                let order = walk(&plan, from, rev, 300);
                let mut q = 0;
                while q < order.len() {
                    let run = plan.run_len(order[q] as f64, rev) as usize;
                    for k in 1..run.min(order.len() - q) {
                        let want = if rev { order[q] - k as u64 } else { order[q] + k as u64 };
                        assert_eq!(order[q + k], want, "{plan:?} rev={rev} q={q} k={k}");
                    }
                    q += run;
                }
            }
        }
    }
}
