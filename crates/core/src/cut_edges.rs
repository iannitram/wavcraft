//! Cut edges for edit by transcript (L3, L4): moves each edge of a cut from the word time the
//! speech model gave to the quietest moment close by, then onto a zero crossing. Word times are
//! only good to tens of milliseconds, so a cut placed exactly on them can keep the tail of a
//! deleted word or clip the start of the next one. Pure arithmetic on a short mono window that
//! the caller reads from the store ([`window`]); never the whole file (I7).
//!
//! Ported from toaster's `src-tauri/src/managers/splice/boundaries.rs` (`snap_to_zero_crossing`,
//! `snap_to_energy_valley`, `snap_segments_energy_biased`) at commit
//! 5973af681d17e8347b504473153a48d333db01de of https://github.com/itsnotaboutthecell/toaster.
//! Changes: frames instead of microseconds, a running sum of squares so an edge costs O(reach),
//! a caller-supplied window instead of the decoded file, and this crate's zero-crossing rule
//! (`actions::zero_crossing`: frame `i` is a crossing when it is 0 or its sign differs from
//! frame `i - 1`). That code is used under the MIT licence:
//!
//! Copyright (c) 2025 CJ Pais
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use std::ops::Range;

/// How far an edge may move to find a quieter moment: long enough to span the move from a
/// voiced sound to silence at a word edge, short enough not to reach into the next syllable.
pub const ENERGY_REACH_MS: u32 = 20;
/// How far from the quiet moment the zero crossing may be: less than one voice period.
pub const ZERO_REACH_MS: u32 = 5;
/// Length of the loudness measure: about one voice period, long enough to ignore a single
/// sample of noise.
pub const ENERGY_FRAME_MS: u32 = 2;

/// The three reaches in frames at one sample rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// Frames either side of the edge searched for the quietest measure.
    pub energy: u64,
    /// Frames either side of the quietest point searched for a zero crossing.
    pub zero: u64,
    /// Half the loudness measure's length (the measure is `2 * half` frames).
    pub half: u64,
}

impl Reach {
    /// The default reaches ([`ENERGY_REACH_MS`], [`ZERO_REACH_MS`], [`ENERGY_FRAME_MS`]) at
    /// `rate`; each at least one frame, the measure at least 4 frames.
    pub fn at_rate(rate: u32) -> Reach {
        let frames = |ms: u32| (u64::from(rate) * u64::from(ms) / 1000).max(1);
        Reach { energy: frames(ENERGY_REACH_MS), zero: frames(ZERO_REACH_MS), half: (frames(ENERGY_FRAME_MS) / 2).max(2) }
    }

    /// Frames either side of an edge that [`quiet_edge`] may look at.
    fn span(&self) -> u64 {
        self.energy + self.half + self.zero + 1
    }
}

/// The frames to read (all enabled channels summed to one) for the edge at `edge` in a file of
/// `len` frames.
pub fn window(edge: u64, reach: &Reach, len: u64) -> Range<u64> {
    let edge = edge.min(len);
    edge.saturating_sub(reach.span())..edge.saturating_add(reach.span()).min(len)
}

/// Moves `edge` to the quietest `2 * reach.half` frames within `reach.energy` of it (on a tie,
/// the one nearest the edge), then to the nearest zero crossing within `reach.zero` of that
/// point. `mono` holds frames `first..first + mono.len()`. When `mono` does not cover the
/// search, or holds no crossing near the quiet point, the edge stays where the step before left
/// it.
pub fn quiet_edge(edge: u64, mono: &[f32], first: u64, reach: &Reach) -> u64 {
    let end = first + mono.len() as u64;
    if edge < first || edge > end {
        return edge;
    }
    // Centres whose measure [c - half, c + half) lies inside the window.
    let lo = edge.saturating_sub(reach.energy).max(first + reach.half);
    let hi = edge.saturating_add(reach.energy).min(end.saturating_sub(reach.half));
    if hi < lo {
        return edge;
    }
    let mut sums = Vec::with_capacity(mono.len() + 1);
    let mut total = 0.0f64;
    sums.push(total);
    for s in mono {
        total += f64::from(*s) * f64::from(*s);
        sums.push(total);
    }
    let at = |frame: u64| (frame - first) as usize;
    let energy = |c: u64| sums[at(c + reach.half)] - sums[at(c - reach.half)];
    let mut best = (f64::INFINITY, u64::MAX, edge);
    for c in lo..=hi {
        let e = energy(c);
        let dist = c.abs_diff(edge);
        if e < best.0 - 1e-12 || ((e - best.0).abs() <= 1e-12 && dist < best.1) {
            best = (e, dist, c);
        }
    }
    let valley = best.2;
    let crosses = |i: u64| i > first && i < end && (mono[at(i)] == 0.0 || (mono[at(i - 1)] < 0.0) != (mono[at(i)] < 0.0));
    for d in 0..=reach.zero {
        if let Some(i) = valley.checked_sub(d).filter(|&i| crosses(i)) {
            return i;
        }
        if crosses(valley + d) {
            return valley + d;
        }
    }
    valley
}

/// [`quiet_edge`] on both edges of each cut in `ranges` (sorted, non-overlapping) in a file of
/// `len` frames. `read(frames)` gives the mono window for one edge, or `None` (that edge then
/// stays). An edge at the start or end of the file stays, since nothing is joined there. A cut
/// keeps its order and never starts before the previous cut's end; a cut whose moved edges would
/// meet or cross keeps its original edges.
pub fn quiet_ranges(ranges: &[Range<u64>], len: u64, reach: &Reach, mut read: impl FnMut(Range<u64>) -> Option<Vec<f32>>) -> Vec<Range<u64>> {
    let mut move_edge = |edge: u64| {
        if edge == 0 || edge >= len {
            return edge.min(len);
        }
        let frames = window(edge, reach, len);
        let first = frames.start;
        match read(frames) {
            Some(mono) => quiet_edge(edge, &mono, first, reach),
            None => edge,
        }
    };
    let mut out: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
    let mut last_end = 0;
    for r in ranges {
        let start = move_edge(r.start).max(last_end);
        let end = move_edge(r.end).min(len);
        let moved = if end > start { start..end } else { r.start.max(last_end)..r.end.min(len) };
        if moved.end > moved.start {
            last_end = moved.end;
            out.push(moved);
        }
    }
    out
}

#[cfg(test)]
// A one-cut list is what these tests mean, not a range of ranges.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(frames: usize, hz: f64, amp: impl Fn(usize) -> f64) -> Vec<f32> {
        (0..frames).map(|i| (amp(i) * (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin()) as f32).collect()
    }

    fn is_crossing(mono: &[f32], i: u64) -> bool {
        let i = i as usize;
        mono[i] == 0.0 || (mono[i - 1] < 0.0) != (mono[i] < 0.0)
    }

    #[test]
    fn reach_at_48k() {
        assert_eq!(Reach::at_rate(RATE), Reach { energy: 960, zero: 240, half: 48 });
        assert_eq!(Reach::at_rate(1000).half, 2);
    }

    #[test]
    fn an_edge_in_speech_moves_into_the_nearby_gap() {
        // 200 Hz at 0.5, with 10 ms of silence starting 12 ms after the word edge at 10,000.
        let gap = 10_576..11_056;
        let mono = tone(20_000, 200.0, |i| if gap.contains(&(i as u64)) { 0.0 } else { 0.5 });
        let moved = quiet_edge(10_000, &mono, 0, &Reach::at_rate(RATE));
        assert!(gap.contains(&moved), "moved to {moved}");
        assert!(is_crossing(&mono, moved));
    }

    #[test]
    fn the_quiet_point_lands_on_a_zero_crossing() {
        // 150 Hz whose level dips to 0.05 at frame 10,500, never to silence.
        let mono = tone(20_000, 150.0, |i| 0.05 + (i as f64 - 10_500.0).abs() / 20_000.0);
        let moved = quiet_edge(10_000, &mono, 0, &Reach::at_rate(RATE));
        assert!(moved.abs_diff(10_500) < 320, "moved to {moved}");
        assert!(is_crossing(&mono, moved));
    }

    #[test]
    fn an_edge_in_silence_stays() {
        let mono = vec![0.0f32; 5_000];
        assert_eq!(quiet_edge(2_500, &mono, 0, &Reach::at_rate(RATE)), 2_500);
    }

    #[test]
    fn a_gap_out_of_reach_is_not_reached() {
        let reach = Reach::at_rate(RATE);
        let gap = 12_000..12_480; // 40 ms after the edge
        let mono = tone(20_000, 200.0, |i| if gap.contains(&(i as u64)) { 0.0 } else { 0.5 });
        let moved = quiet_edge(10_000, &mono, 0, &reach);
        assert!(moved.abs_diff(10_000) <= reach.energy + reach.zero, "moved to {moved}");
        assert!(!gap.contains(&moved));
    }

    #[test]
    fn the_window_offset_is_honoured() {
        let reach = Reach::at_rate(RATE);
        let gap = 10_576..11_056;
        let whole = tone(20_000, 200.0, |i| if gap.contains(&(i as u64)) { 0.0 } else { 0.5 });
        let frames = window(10_000, &reach, 20_000);
        let part = &whole[frames.start as usize..frames.end as usize];
        assert_eq!(quiet_edge(10_000, part, frames.start, &reach), quiet_edge(10_000, &whole, 0, &reach));
    }

    #[test]
    fn a_window_too_short_leaves_the_edge() {
        let reach = Reach::at_rate(RATE);
        assert_eq!(quiet_edge(100, &[0.5, -0.5, 0.5], 99, &reach), 100);
        assert_eq!(quiet_edge(100, &[], 100, &reach), 100);
        assert_eq!(quiet_edge(50, &[0.0; 200], 99, &reach), 50);
    }

    #[test]
    fn window_is_clamped_to_the_file() {
        let reach = Reach::at_rate(RATE);
        assert_eq!(window(100, &reach, 50_000), 0..100 + reach.span());
        assert_eq!(window(49_990, &reach, 50_000), 49_990 - reach.span()..50_000);
    }

    #[test]
    fn ranges_keep_order_and_never_overlap() {
        // Silent gaps just after each word edge: 2,000, 6,000, 9,000 and 14,000.
        let gaps = [2_300..2_500, 6_300..6_500, 9_300..9_500, 14_300..14_500];
        let mono = tone(20_000, 200.0, |i| if gaps.iter().any(|g| g.contains(&(i as u64))) { 0.0 } else { 0.5 });
        let reach = Reach::at_rate(RATE);
        let read = |r: Range<u64>| Some(mono[r.start as usize..r.end as usize].to_vec());
        let out = quiet_ranges(&[2_000..6_000, 9_000..14_000], 20_000, &reach, read);
        assert_eq!(out.len(), 2);
        for (r, g) in out.iter().zip([[&gaps[0], &gaps[1]], [&gaps[2], &gaps[3]]]) {
            assert!(g[0].contains(&r.start) && g[1].contains(&r.end), "{r:?}");
        }
        assert!(out[0].end <= out[1].start);
    }

    #[test]
    fn a_cut_that_would_invert_keeps_its_edges() {
        // A 100-frame cut in speech with one short gap after it: both edges would move to it.
        let mono = tone(20_000, 200.0, |i| if (10_300..10_400).contains(&i) { 0.0 } else { 0.5 });
        let reach = Reach::at_rate(RATE);
        let read = |r: Range<u64>| Some(mono[r.start as usize..r.end as usize].to_vec());
        let out = quiet_ranges(&[10_000..10_100], 20_000, &reach, read);
        assert_eq!(out, vec![10_000..10_100]);
    }

    #[test]
    fn edges_at_the_file_bounds_stay() {
        // Speech with a short gap 7 ms in from each end: a cut from the start or to the end
        // must not leave a sliver of the deleted word before the gap.
        let len = 20_000;
        let mono = tone(20_000, 200.0, |i| if (300..492).contains(&i) || (19_500..19_700).contains(&i) { 0.0 } else { 0.5 });
        let reach = Reach::at_rate(RATE);
        let read = |r: Range<u64>| Some(mono[r.start as usize..r.end as usize].to_vec());
        assert_eq!(quiet_ranges(&[0..len], len, &reach, read)[0], 0..len);
        assert_eq!(quiet_ranges(&[0..5_000], len, &reach, read)[0].start, 0);
        assert_eq!(quiet_ranges(&[10_000..len], len, &reach, read)[0].end, len);
    }

    #[test]
    fn an_unreadable_window_keeps_the_edge() {
        let out = quiet_ranges(&[1_000..2_000], 20_000, &Reach::at_rate(RATE), |_| None);
        assert_eq!(out, vec![1_000..2_000]);
    }
}
