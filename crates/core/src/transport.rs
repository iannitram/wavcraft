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
}

/// The plan for `req` and the frame to start at, or `None` when there is nothing to play.
///
/// - Skip Selection with a selection: from [`SKIP_PREROLL_SECS`] before it to
///   [`SKIP_POSTROLL_SECS`] after it, jumping over the selection;
/// - a selection: just the selection;
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
        (Plan::new(sel_a, sel_b), sel_a)
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
        Request { selection: 0..0, cursor: 0, frames: 1_000_000, view: 0.0..1000.0, rate: 48_000, looping: false, skip_selection: false }
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
}
