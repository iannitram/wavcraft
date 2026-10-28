//! Match Loudness (f33, roadmap B126-B128, C247): bring a document to a target integrated
//! loudness as ONE undoable edit, keeping the true peak under a ceiling either by turning the
//! gain down or by a true-peak-aware look-ahead limiter.
//!
//! The measurement is [`crate::loudness::measure`] (BS.1770 gating, 4x true peak). The gain is
//! `target - measured` dB. If that gain would push the measured true peak over the ceiling:
//!
//! - with limiting **on**, the audio gets the gain and then the limiter below. A limiter lowers
//!   the loudness a little, so the result is measured again and the gain raised by the shortfall
//!   (up to [`MAX_PASSES`] times) until the loudness is within [`SETTLE_LU`] of the target;
//! - with limiting **off**, the gain is capped so the true peak just reaches the ceiling, and the
//!   loudness that results is reported (it is under the target).
//!
//! **The limiter** is linked across channels (one gain for all, so the stereo image holds). Its
//! peak detector looks at the samples and at three points between each pair (a cubic through the
//! four neighbours, a rough 4x oversampling), and the ceiling it aims for is
//! [`LIMITER_MARGIN_DB`] under the one asked for, because that interpolation is not the meter's.
//! The required gain at each sample is minimum-filtered over the look-ahead window, then
//! averaged over the same window (so the gain ramps down before the peak and never overshoots),
//! then allowed back up only at the release time constant. Each chunk reads the few hundred
//! frames it needs on either side straight from the document, so nothing is held but one chunk.
//! It is our own code (no dependence on the Hard Limiter effect).

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::AtomicBool;

use crate::doc::DocState;
use crate::doc::edit::{ChannelMask, Crossfades, Edit, EditError, replace_range};
use crate::loudness::{self, Loudness};

/// Passes at most, when the limiter's loss is made up by more gain.
pub const MAX_PASSES: usize = 4;
/// A limited result this close to the target (LU) or closer needs no further pass.
pub const SETTLE_LU: f64 = 0.1;
/// How far under the asked-for ceiling the limiter aims, dB (its detector is approximate).
pub const LIMITER_MARGIN_DB: f64 = 0.3;

/// The settings of Match Loudness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// Integrated loudness to reach, LUFS.
    pub lufs: f64,
    /// A file already within this many LU of `lufs` is left alone.
    pub tolerance_lu: f64,
    /// Highest true peak allowed, dBTP.
    pub max_true_peak_dbtp: f64,
    /// Use the limiter when the gain would take the true peak over the ceiling.
    pub limit: bool,
    pub look_ahead_ms: f64,
    pub release_ms: f64,
}

impl Target {
    /// "Podcast (-16 LUFS, -1 dBTP)", the default.
    pub const PODCAST: Target =
        Target { lufs: -16.0, tolerance_lu: 0.5, max_true_peak_dbtp: -1.0, limit: true, look_ahead_ms: 12.0, release_ms: 200.0 };
    /// "Broadcast (-23 LUFS, -1 dBTP)".
    pub const BROADCAST: Target = Target { lufs: -23.0, ..Target::PODCAST };
}

impl Default for Target {
    fn default() -> Self {
        Target::PODCAST
    }
}

/// The presets, with the names the panel shows.
pub const PRESETS: [(&str, Target); 2] = [("Podcast (-16 LUFS, -1 dBTP)", Target::PODCAST), ("Broadcast (-23 LUFS, -1 dBTP)", Target::BROADCAST)];

/// What [`plan`] decides from a measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Gain to apply, dB (before any limiting).
    pub gain_db: f64,
    /// The limiter is needed after the gain.
    pub limiter: bool,
    /// The target cannot be reached without limiting (limiting off and the peak in the way).
    pub capped: bool,
    /// Loudness expected from the gain alone, LUFS.
    pub expected_lufs: f64,
}

/// The gain for `m` to reach `t`. `None` when `m` has no loudness (silent, or under 400 ms).
pub fn plan(m: &Loudness, t: &Target) -> Option<Plan> {
    if !m.integrated_lufs.is_finite() {
        return None;
    }
    let wanted = t.lufs - m.integrated_lufs;
    let room = if m.true_peak_dbtp.is_finite() { t.max_true_peak_dbtp - m.true_peak_dbtp } else { f64::INFINITY };
    let over = wanted > room;
    Some(match (over, t.limit) {
        (false, _) => Plan { gain_db: wanted, limiter: false, capped: false, expected_lufs: t.lufs },
        (true, true) => Plan { gain_db: wanted, limiter: true, capped: false, expected_lufs: t.lufs },
        (true, false) => Plan { gain_db: room, limiter: false, capped: true, expected_lufs: m.integrated_lufs + room },
    })
}

#[derive(Debug)]
pub enum MatchError {
    /// Nothing to measure: silence, or less than 400 ms.
    NoLoudness,
    Edit(EditError),
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchError::NoLoudness => write!(f, "nothing to match: the audio is silent or shorter than 400 ms"),
            MatchError::Edit(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MatchError {}

impl From<EditError> for MatchError {
    fn from(e: EditError) -> Self {
        MatchError::Edit(e)
    }
}

impl From<io::Error> for MatchError {
    fn from(e: io::Error) -> Self {
        MatchError::Edit(e.into())
    }
}

/// What a run produced.
#[derive(Debug)]
pub struct MatchResult {
    /// The edit to push on the history (a no-op when the file was already within tolerance).
    pub edit: Edit,
    pub before: Loudness,
    /// Measured on the edited audio.
    pub after: Loudness,
    /// Gain applied, dB (0 when nothing was done).
    pub gain_db: f64,
    pub limited: bool,
    /// The loudness is within the tolerance of the target.
    pub on_target: bool,
    /// One sentence for the status line and the control channel.
    pub message: String,
}

/// Match the whole of `doc` to `target`. `before` is a measurement already taken (a Scan); `None`
/// measures here. `progress` gets the fraction of the current pass; `cancel` stops it.
pub fn run(
    doc: &DocState,
    target: &Target,
    before: Option<Loudness>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<MatchResult, MatchError> {
    let len = doc.len_frames();
    let before = match before {
        Some(m) => m,
        None => loudness::measure(doc, 0..len, cancel, progress)?,
    };
    let Some(p) = plan(&before, target) else { return Err(MatchError::NoLoudness) };
    let within = (before.integrated_lufs - target.lufs).abs() <= target.tolerance_lu;
    let tp_ok = !before.true_peak_dbtp.is_finite() || before.true_peak_dbtp <= target.max_true_peak_dbtp;
    if within && tp_ok {
        return Ok(MatchResult {
            edit: Edit { state: doc.clone(), label: LABEL.to_string(), change: crate::doc::edit::Change::None },
            before,
            after: before,
            gain_db: 0.0,
            limited: false,
            on_target: true,
            message: format!("Already within {:.1} LU of {:.1} LUFS: nothing changed", target.tolerance_lu, target.lufs),
        });
    }
    let mut gain_db = p.gain_db;
    let mut done: Option<(Edit, Loudness, f64)> = None;
    for pass in 0..MAX_PASSES {
        let edit = apply(doc, gain_db, p.limiter.then_some(target), cancel, progress)?;
        let after = loudness::measure(&edit.state, 0..len, cancel, &mut |_| {})?;
        let short = target.lufs - after.integrated_lufs;
        let settled = !p.limiter || short.abs() <= SETTLE_LU || !after.integrated_lufs.is_finite();
        done = Some((edit, after, gain_db));
        if settled || pass + 1 == MAX_PASSES {
            break;
        }
        gain_db += short;
    }
    let Some((edit, after, gain_db)) = done else { return Err(MatchError::NoLoudness) };
    let on_target = (after.integrated_lufs - target.lufs).abs() <= target.tolerance_lu;
    let message = format!(
        "Match Loudness: {:+.2} dB{}; {:.1} LUFS -> {:.1} LUFS, true peak {:.1} dBTP{}",
        gain_db,
        if p.limiter { " with limiting" } else { "" },
        before.integrated_lufs,
        after.integrated_lufs,
        after.true_peak_dbtp,
        if p.capped { format!(" (limited by the {:.1} dBTP ceiling; limiting is off)", target.max_true_peak_dbtp) } else { String::new() }
    );
    Ok(MatchResult { edit, before, after, gain_db, limited: p.limiter, on_target, message })
}

const LABEL: &str = "Match Loudness";

/// The audio times `gain_db`, through the limiter when `limit` is given. One edit.
pub fn apply(doc: &DocState, gain_db: f64, limit: Option<&Target>, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Edit, EditError> {
    let len = doc.len_frames().max(1);
    let pre = 10f64.powf(gain_db / 20.0) as f32;
    let mut limiter = limit.map(|t| Limiter::new(doc, t, pre));
    replace_range(doc, 0..doc.len_frames(), ChannelMask::ALL, LABEL, Crossfades::NONE, cancel, |at, ins, outs| {
        progress(at as f64 / len as f64);
        match &mut limiter {
            None => {
                for (o, i) in outs.iter_mut().zip(ins) {
                    for (o, i) in o.iter_mut().zip(*i) {
                        *o = *i * pre;
                    }
                }
            }
            Some(l) => {
                let gains = l.gains(doc, at, ins.first().map_or(0, |c| c.len()))?;
                for (o, i) in outs.iter_mut().zip(ins) {
                    for ((o, i), g) in o.iter_mut().zip(*i).zip(&gains) {
                        *o = *i * pre * *g;
                    }
                }
            }
        }
        Ok(())
    })
}

/// The streamed look-ahead limiter: [`Limiter::gains`] gives the gain for a chunk.
struct Limiter {
    look: usize,
    /// Frames read before a chunk: the minimum-filter and averaging history plus the interpolator's.
    pad_before: usize,
    ceiling: f32,
    release_coef: f32,
    pre: f32,
    channels: usize,
    len: u64,
    /// Gain at the end of the previous chunk (for the release).
    prev: f32,
}

impl Limiter {
    fn new(doc: &DocState, t: &Target, pre: f32) -> Limiter {
        let rate = f64::from(doc.sample_rate().max(1));
        let look = ((t.look_ahead_ms.max(0.0) / 1000.0 * rate).round() as usize).max(1);
        let release = (t.release_ms.max(0.1) / 1000.0 * rate).max(1.0);
        Limiter {
            look,
            pad_before: 2 * look + 2,
            ceiling: 10f64.powf((t.max_true_peak_dbtp - LIMITER_MARGIN_DB) / 20.0) as f32,
            release_coef: (1.0 - (-1.0 / release).exp()) as f32,
            pre,
            channels: doc.channel_count(),
            len: doc.len_frames(),
            prev: 1.0,
        }
    }

    /// The gain (after `pre`) for frames `at..at + count`, in `(0, 1]`.
    fn gains(&mut self, doc: &DocState, at: u64, count: usize) -> io::Result<Vec<f32>> {
        let lo = at.saturating_sub(self.pad_before as u64);
        let hi = (at + count as u64 + self.look as u64 + 3).min(self.len);
        let n = (hi - lo) as usize;
        let mut planar: Vec<Vec<f32>> = vec![vec![0.0; n]; self.channels];
        {
            let mut bufs: Vec<&mut [f32]> = planar.iter_mut().map(Vec::as_mut_slice).collect();
            doc.read_frames(lo, &mut bufs)?;
        }
        // Required gain at each sample: the ceiling over the larger of the sample and the three
        // interpolated points after it, over every channel.
        let at_ = |c: &[f32], i: isize| -> f32 {
            let j = i.clamp(0, c.len() as isize - 1) as usize;
            c[j] * self.pre
        };
        let mut req = vec![1.0f32; n];
        for (i, r) in req.iter_mut().enumerate() {
            let mut peak = 0.0f32;
            for c in &planar {
                let (p0, p1, p2, p3) = (at_(c, i as isize - 1), at_(c, i as isize), at_(c, i as isize + 1), at_(c, i as isize + 2));
                peak = peak.max(p1.abs());
                for t in [0.25f32, 0.5, 0.75] {
                    // Catmull-Rom between p1 and p2.
                    let v = 0.5
                        * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t);
                    peak = peak.max(v.abs());
                }
            }
            if peak > self.ceiling {
                *r = self.ceiling / peak;
            }
        }
        // Minimum over [j, j + look], by a monotonic queue.
        let mut mn = vec![1.0f32; n];
        let mut q: VecDeque<usize> = VecDeque::new();
        let mut next = 0usize;
        for (j, m) in mn.iter_mut().enumerate() {
            let end = (j + self.look).min(n - 1);
            while next <= end {
                while q.back().is_some_and(|&b| req[b] >= req[next]) {
                    q.pop_back();
                }
                q.push_back(next);
                next += 1;
            }
            while q.front().is_some_and(|&f| f < j) {
                q.pop_front();
            }
            *m = q.front().map_or(1.0, |&f| req[f]);
        }
        // Mean of mn over [i - look, i], then the release.
        let mut prefix = vec![0.0f64; n + 1];
        for (i, m) in mn.iter().enumerate() {
            prefix[i + 1] = prefix[i] + f64::from(*m);
        }
        let mut out = Vec::with_capacity(count);
        for k in 0..count {
            let i = (at - lo) as usize + k;
            if i >= n {
                out.push(self.prev);
                continue;
            }
            let from = i.saturating_sub(self.look);
            let avg = ((prefix[i + 1] - prefix[from]) / (i + 1 - from) as f64) as f32;
            let recovered = self.prev + (1.0 - self.prev) * self.release_coef;
            self.prev = avg.min(recovered);
            out.push(self.prev);
        }
        Ok(out)
    }
}

/// The arithmetic of card wav-c-38-a, ported onto [`plan`] (the one Match Loudness
/// implementation). Card-38's `match_gain_db`/`within_tolerance`/`PRESETS` were dropped with its
/// duplicate module; the cases that map onto this file's semantics are kept here.
#[cfg(test)]
mod card38_arithmetic_tests {
    use super::*;

    fn lo(lufs: f64, true_peak: f64) -> Loudness {
        Loudness { frames: 480_000, integrated_lufs: lufs, range_lu: 0.0, true_peak_dbtp: true_peak, sample_peak_dbfs: true_peak, max_momentary_lufs: lufs }
    }

    fn near(got: f64, want: f64) {
        assert!((got - want).abs() <= 1e-9, "got {got}, want {want}");
    }

    const OWNER: Target = Target::PODCAST;
    const LIMIT_OFF: Target = Target { limit: false, ..Target::PODCAST };

    #[test]
    fn presets_are_the_owner_default_and_the_broadcast_default() {
        assert_eq!(PRESETS[0].1, Target::PODCAST);
        assert_eq!(PRESETS[1].1, Target::BROADCAST);
        assert_eq!(Target::PODCAST.lufs, -16.0);
        assert_eq!(Target::BROADCAST.lufs, -23.0);
        assert_eq!(Target::PODCAST.max_true_peak_dbtp, -1.0);
    }

    #[test]
    fn gain_raises_a_quiet_file_to_the_target() {
        let p = plan(&lo(-20.0, -6.0), &OWNER).unwrap();
        near(p.gain_db, 4.0);
        assert!(!p.limiter && !p.capped);
        near(plan(&lo(-30.0, -12.0), &Target::BROADCAST).unwrap().gain_db, 7.0);
    }

    #[test]
    fn gain_lowers_a_loud_file_to_the_target() {
        near(plan(&lo(-12.5, -0.5), &OWNER).unwrap().gain_db, -3.5);
    }

    #[test]
    fn broadcast_matches_to_minus_23() {
        near(plan(&lo(-20.0, -9.0), &Target::BROADCAST).unwrap().gain_db, -3.0);
    }

    #[test]
    fn limiting_off_caps_the_gain_at_max_true_peak() {
        let p = plan(&lo(-20.0, -3.0), &LIMIT_OFF).unwrap();
        near(p.gain_db, 2.0);
        assert!(p.capped && !p.limiter);
        near(plan(&lo(-20.0, -8.0), &LIMIT_OFF).unwrap().gain_db, 4.0);
        near(plan(&lo(-12.0, -3.0), &LIMIT_OFF).unwrap().gain_db, -4.0);
    }

    #[test]
    fn limiting_on_does_not_cap_the_gain() {
        let p = plan(&lo(-20.0, -3.0), &OWNER).unwrap();
        near(p.gain_db, 4.0);
        assert!(p.limiter && !p.capped);
    }

    #[test]
    fn no_loudness_means_no_plan() {
        assert!(plan(&lo(f64::NAN, -6.0), &OWNER).is_none());
        assert!(plan(&lo(f64::NEG_INFINITY, -6.0), &OWNER).is_none());
    }
}
