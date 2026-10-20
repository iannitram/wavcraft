//! DeClicker and Automatic Click Remover (B046, B072): find short impulses (clicks, pops) in
//! spoken-word and vinyl transfers and redraw each one from the audio around it.
//!
//! **Detector.** For every channel the second difference `e[n] = x[n-1] - 2x[n] + x[n+1]` is
//! taken; it is small for music and speech (smooth) and large for an impulse. The noise level
//! of `e` is a robust running scale (the median of `|e|` over each [`WINDOW`] centres, divided
//! by 0.6745, so a click inside the window does not move it). A centre is *high* when `|e|` is
//! over `k_high` times that scale and *low* when over a third of it; `k_high` follows the
//! sensitivity (8 at 1 %, about 5 at the default 50 %, about 2 at 100 %). A click is a run of
//! low centres (gaps of up to [`GAP`] frames allowed) that holds at least one high centre; the
//! reported [`Region`] spans the run. A run longer than `max_width_ms` is not a click (it is a
//! loud passage) and is dropped. The first difference is judged the same way, which catches the
//! two edges of a flat-topped pulse (no second difference inside it); two such edges of opposite
//! sign, with a plateau between them off the surrounding level, are merged into one click. Both
//! kinds of residual must also clear a fixed floor of about -80 dBFS, so near-silence is left alone. The scan reads 64 K frames at a time and carries the state
//! across chunk joins, so a click that straddles one is found whole.
//!
//! **Repair.** Each region is redrawn by least-squares autoregressive interpolation: an order
//! [`ORDER`] predictor is fitted (autocorrelation, Levinson-Durbin) to up to [`CONTEXT`]
//! samples either side of the click, then the missing samples are chosen to minimise the
//! prediction error over the gap. That follows tones and dense audio far better than a
//! polynomial. Where the context is too short for the predictor (near a file edge) a straight
//! line is drawn; at the very edge of the file, or on silence, the region is skipped.
//! Everything outside the regions is copied through bit for bit, in one
//! [`replace_range`](crate::doc::edit::replace_range), so each operation is one undo step.
//!
//! [`declick`] is the DeClicker ("DeClicker" step); [`auto_click_remove`] is the one-command
//! Automatic Click Remover, whose `threshold` is the same percent as the sensitivity and whose
//! `complexity` sets the widest click: `max_width_ms = complexity / 20`.

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::declip::Repair;
use super::{Region, write_fixes};
use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, EditError};
use crate::store::BLOCK_FRAMES;

/// The most regions one scan holds.
pub const MAX_REGIONS: usize = 100_000;
/// Centres per noise-scale window.
pub const WINDOW: usize = 2048;
/// Frames of gap allowed inside one click.
pub const GAP: u64 = 2;
/// Order of the autoregressive predictor.
pub const ORDER: usize = 16;
/// The widest click [`declick`] redraws, in frames, whatever the sample rate (the repair solves
/// a dense system as large as the click). Wider regions are reported but skipped.
pub const MAX_REPAIR_WIDTH: u64 = 256;
/// Smallest noise scale of a residual window (stops a silent window from having a zero scale).
pub const SCALE_FLOOR: f64 = 1e-5;
/// A residual must exceed this to be a seed (about -80 dBFS), whatever the noise scale.
pub const HIGH_FLOOR: f64 = 1e-4;
/// A residual must exceed this to extend a click.
pub const LOW_FLOOR: f64 = 5e-5;
/// Samples of context either side used to fit the predictor.
pub const CONTEXT: usize = 256;
/// A repaired sample never exceeds this size.
pub const PEAK_LIMIT: f64 = 4.0;
/// Undo-step label of the DeClicker.
pub const LABEL: &str = "DeClicker";
/// Undo-step label of the Automatic Click Remover.
pub const AUTO_LABEL: &str = "Automatic Click Remover";

/// DeClicker settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// How readily impulses are called clicks, percent (1 finds only the loudest, 100 the faintest).
    pub sensitivity: f64,
    /// The widest click, in milliseconds.
    pub max_width_ms: f64,
}

impl Params {
    pub const SENSITIVITY: Range<f64> = 1.0..100.0;
    pub const MAX_WIDTH_MS: Range<f64> = 0.1..5.0;

    /// Whether the settings are finite and inside their ranges.
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64, r: &Range<f64>| v.is_finite() && v >= r.start && v <= r.end;
        if !ok(self.sensitivity, &Self::SENSITIVITY) {
            return Err(format!("sensitivity = {} is not 1 to 100", self.sensitivity));
        }
        if !ok(self.max_width_ms, &Self::MAX_WIDTH_MS) {
            return Err(format!("max_width_ms = {} is not 0.1 to 5", self.max_width_ms));
        }
        Ok(())
    }

    /// Settings from `sensitivity=50 max_width_ms=1` (space-separated `key=value`; a key left
    /// out keeps its default; a trailing `%` or `ms` is allowed), as `[sensitivity, max_width_ms]`.
    pub fn parse(args: &str) -> Result<Vec<f64>, String> {
        let d = Params::default();
        let mut v = vec![d.sensitivity, d.max_width_ms];
        parse_into(args, "DeClicker", &["sensitivity", "max_width_ms"], &mut v)?;
        Params { sensitivity: v[0], max_width_ms: v[1] }.validate()?;
        Ok(v)
    }

    /// Settings from the `[sensitivity, max_width_ms]` list.
    pub fn from_list(v: &[f64]) -> Params {
        let d = Params::default();
        Params { sensitivity: v.first().copied().unwrap_or(d.sensitivity), max_width_ms: v.get(1).copied().unwrap_or(d.max_width_ms) }
    }

    /// The multiple of the noise scale a centre must exceed to seed a click.
    fn k_high(&self) -> f64 {
        8.0 - 0.06 * (self.sensitivity - 1.0)
    }
}

impl Default for Params {
    fn default() -> Self {
        Params { sensitivity: 50.0, max_width_ms: 1.0 }
    }
}

/// Automatic Click Remover settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoParams {
    /// The same percent as [`Params::sensitivity`].
    pub threshold: f64,
    /// 2 to 100; the widest click is `complexity / 20` ms.
    pub complexity: f64,
}

impl AutoParams {
    pub const COMPLEXITY: Range<f64> = 2.0..100.0;

    /// The DeClicker settings these map onto.
    pub fn to_params(&self) -> Params {
        Params { sensitivity: self.threshold, max_width_ms: self.complexity / 20.0 }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.complexity.is_finite() || self.complexity < Self::COMPLEXITY.start || self.complexity > Self::COMPLEXITY.end {
            return Err(format!("complexity = {} is not 2 to 100", self.complexity));
        }
        if !self.threshold.is_finite() || self.threshold < Params::SENSITIVITY.start || self.threshold > Params::SENSITIVITY.end {
            return Err(format!("threshold = {} is not 1 to 100", self.threshold));
        }
        Ok(())
    }

    /// Settings from `threshold=50 complexity=20`, as `[threshold, complexity]`.
    pub fn parse(args: &str) -> Result<Vec<f64>, String> {
        let d = AutoParams::default();
        let mut v = vec![d.threshold, d.complexity];
        parse_into(args, "Automatic Click Remover", &["threshold", "complexity"], &mut v)?;
        AutoParams { threshold: v[0], complexity: v[1] }.validate()?;
        Ok(v)
    }

    pub fn from_list(v: &[f64]) -> AutoParams {
        let d = AutoParams::default();
        AutoParams { threshold: v.first().copied().unwrap_or(d.threshold), complexity: v.get(1).copied().unwrap_or(d.complexity) }
    }
}

impl Default for AutoParams {
    fn default() -> Self {
        AutoParams { threshold: 50.0, complexity: 20.0 }
    }
}

fn parse_into(args: &str, who: &str, keys: &[&str], v: &mut [f64]) -> Result<(), String> {
    for word in args.split_whitespace() {
        let Some((key, text)) = word.split_once('=') else { return Err(format!("{word:?}: write key=value")) };
        let key = key.to_ascii_lowercase();
        let Some(i) = keys.iter().position(|k| *k == key) else {
            return Err(format!("{who} has no setting {key:?} (it has: {})", keys.join(", ")));
        };
        let number = text.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
        v[i] = number.parse::<f64>().map_err(|_| format!("{key}: {text:?} is not a number"))?;
    }
    Ok(())
}

fn invalid(why: String) -> EditError {
    EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why))
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    /// The clicks, in frame order (channel order within a frame).
    pub regions: Vec<Region>,
    /// True when the scan stopped at [`MAX_REGIONS`] before reaching the end of the range.
    pub truncated: bool,
}

/// A run of low centres being followed in one channel.
struct Open {
    first: u64,
    last: u64,
    high: bool,
    peak: f32,
    /// The first difference at the run's largest step (signed) and the noise scale it was judged by.
    step: f64,
    step_scale: f64,
    /// For a merged pulse, the edge runs it was made from (so a better pairing can undo it).
    parts: Vec<Open>,
}

/// Per-channel scan state carried across chunks.
struct Lane {
    carry: Vec<f32>,
    open: Option<Open>,
    /// Finished runs held back in case one is the first edge of a flat-topped pulse.
    pending: Vec<Open>,
    scale_e: f64,
    scale_d: f64,
}

/// Median-based noise scale of one window of residuals (at least [`SCALE_FLOOR`]).
fn window_scale(win: &[f32], sorted: &mut Vec<f32>) -> f64 {
    sorted.clear();
    sorted.extend(win.iter().map(|e| e.abs()));
    let mid = sorted.len() / 2;
    let (_, med, _) = sorted.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
    (f64::from(*med) / 0.6745).max(SCALE_FLOOR)
}

fn mean(v: &[f32]) -> f64 {
    v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len().max(1) as f64
}

/// Whether runs `a` then `b` (a gap apart) are the two edges of one flat-topped pulse: steps of
/// opposite sign and similar size, no wider than `max_width` in all. Such a pulse has no second
/// difference inside it. When neither edge is strong on its own the plateau must also stand off
/// the few samples either side by about the size of the steps.
fn is_pulse(doc: &DocState, ch: usize, a: &Open, b: &Open, max_width: u64) -> io::Result<bool> {
    if b.last - a.first + 1 > max_width || a.step * b.step >= 0.0 {
        return Ok(false);
    }
    let (sa, sb) = (a.step.abs(), b.step.abs());
    if sa < 3.0 * a.step_scale || sb < 3.0 * b.step_scale || sa > 2.0 * sb || sb > 2.0 * sa {
        return Ok(false);
    }
    if a.high || b.high {
        return Ok(true);
    }
    const SIDE: u64 = 4;
    let (ls, rs) = (a.first.saturating_sub(SIDE), b.last + 1);
    let (ln, rn) = ((a.first - ls) as usize, SIDE.min(doc.len_frames().saturating_sub(rs)) as usize);
    if ln < 2 || rn < 2 {
        return Ok(false);
    }
    let mut left = vec![0.0f32; ln];
    let mut right = vec![0.0f32; rn];
    let mut mid = vec![0.0f32; (b.first - a.last + 1) as usize];
    if doc.read(ch, ls, &mut left)? != ln || doc.read(ch, rs, &mut right)? != rn || doc.read(ch, a.last, &mut mid)? != mid.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
    }
    let lift = mean(&mid) - (mean(&left) + mean(&right)) / 2.0;
    // The plateau's level is averaged over many samples, so it is a better measure than either step.
    Ok(lift * a.step > 0.0 && lift.abs() >= 0.6 * sa.max(sb) && lift.abs() <= 2.0 * (sa + sb))
}

/// Hand a finished run to the lane. Runs stay held while one could still pair with a later run
/// as the two edges of a pulse (so noise-sized runs on the plateau do not break the pairing); a
/// run that does pair absorbs everything held after its first edge. Held runs that are out of
/// reach, or all of them when `run` is `None`, are released. Returns true when the region cap
/// was hit.
fn settle(doc: &DocState, regions: &mut Vec<Region>, lane: &mut Lane, ch: usize, run: Option<Open>, max_width: u64) -> io::Result<bool> {
    let mut release = |lane: &mut Lane, upto: usize| -> bool {
        for o in lane.pending.drain(..upto) {
            let width = o.last - o.first + 1;
            if o.high && width <= max_width {
                if regions.len() >= MAX_REGIONS {
                    return true;
                }
                regions.push(Region { channel: ch, start: o.first, len: width, peak: o.peak });
            }
        }
        false
    };
    let Some(r) = run else {
        let n = lane.pending.len();
        return Ok(release(lane, n));
    };
    // Of the held runs (and the edges inside held pulses) that pair with this one, take the one
    // with the sharpest edge: a noise-sized run also passes the test, but its step is smaller.
    // (index, how many of its parts to keep, the edge it pairs with)
    let mut best: Option<(usize, bool, f64)> = None;
    for (i, p) in lane.pending.iter().enumerate() {
        let score = p.step.abs().min(r.step.abs());
        if r.last - p.first < max_width && best.is_none_or(|(_, _, sc)| score > sc) && is_pulse(doc, ch, p, &r, max_width)? {
            best = Some((i, false, score));
        }
        if let Some(e) = p.parts.last() {
            let score = e.step.abs().min(r.step.abs());
            if r.last - e.first < max_width && best.is_none_or(|(_, _, sc)| score >= sc) && is_pulse(doc, ch, e, &r, max_width)? {
                best = Some((i, true, score));
            }
        }
    }
    let join = |p: Open, r: Open| {
        let step = if p.step.abs() >= r.step.abs() { p.step } else { r.step };
        Open { first: p.first, last: r.last, high: true, peak: p.peak.max(r.peak), step, step_scale: p.step_scale, parts: vec![p, r] }
    };
    match best {
        Some((i, false, _)) => {
            let tail: Vec<Open> = lane.pending.drain(i..).collect();
            if let Some(p) = tail.into_iter().next() {
                lane.pending.push(join(p, r));
            }
        }
        Some((i, true, _)) => {
            let tail: Vec<Open> = lane.pending.drain(i..).collect();
            if let Some(mut m) = tail.into_iter().next()
                && let Some(e) = m.parts.pop()
            {
                // Undo the earlier pairing: its first edge goes back to being a run of its own.
                lane.pending.extend(m.parts);
                lane.pending.push(join(e, r));
            }
        }
        None => lane.pending.push(r),
    }
    let newest = lane.pending.last().map_or(0, |o| o.last);
    let out = lane.pending.iter().take_while(|o| newest - o.first >= max_width).count();
    Ok(release(lane, out))
}

/// Find the clicks of `channels` within `range` (empty: the whole file). A click wider than
/// [`MAX_REPAIR_WIDTH`] frames is still reported when it is within `max_width_ms`; [`declick`]
/// leaves it alone.
pub fn scan(doc: &DocState, range: Range<u64>, channels: ChannelMask, params: &Params, cancel: &AtomicBool) -> Result<Scan, EditError> {
    params.validate().map_err(invalid)?;
    let len = doc.len_frames();
    let range = if range.start < range.end { range } else { 0..len };
    if range.end > len {
        return Err(EditError::OutOfRange { range, len });
    }
    let max_width = ((params.max_width_ms * f64::from(doc.sample_rate()) / 1000.0).round() as u64).max(1);
    let k_high = params.k_high();
    let (k_low_e, k_low_d) = (k_high / 3.0, k_high / 2.5);
    let active = channels.active(doc.channel_count());
    let mut lanes: Vec<Lane> =
        active.iter().map(|_| Lane { carry: Vec::new(), open: None, pending: Vec::new(), scale_e: 0.0, scale_d: 0.0 }).collect();
    let mut regions: Vec<Region> = Vec::new();
    let mut truncated = false;
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut work: Vec<f32> = Vec::with_capacity(BLOCK_FRAMES + 2);
    let mut es: Vec<f32> = Vec::with_capacity(BLOCK_FRAMES);
    let mut ds: Vec<f32> = Vec::with_capacity(BLOCK_FRAMES);
    let mut sorted: Vec<f32> = Vec::with_capacity(WINDOW);

    let mut at = range.start;
    'outer: while at < range.end {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        let m = (range.end - at).min(BLOCK_FRAMES as u64) as usize;
        for (k, &c) in active.iter().enumerate() {
            if doc.read(c, at, &mut buf[..m])? != m {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
            let lane = &mut lanes[k];
            work.clear();
            work.extend_from_slice(&lane.carry);
            work.extend_from_slice(&buf[..m]);
            // work[0] is frame `base`; centre j (1..len-1) is frame base + j.
            let base = at - lane.carry.len() as u64;
            es.clear();
            ds.clear();
            for j in 1..work.len().saturating_sub(1) {
                es.push(work[j - 1] - 2.0 * work[j] + work[j + 1]);
                ds.push(work[j] - work[j - 1]);
            }
            lane.carry.clear();
            let keep = work.len().min(2);
            lane.carry.extend_from_slice(&work[work.len() - keep..]);
            let first_centre = base + 1;
            for (w, win) in es.chunks(WINDOW).enumerate() {
                let dwin = &ds[w * WINDOW..w * WINDOW + win.len()];
                if win.len() >= 64 || lane.scale_e == 0.0 {
                    lane.scale_e = window_scale(win, &mut sorted);
                    lane.scale_d = window_scale(dwin, &mut sorted);
                }
                let hi_e = (k_high * lane.scale_e).max(HIGH_FLOOR);
                let lo_e = (k_low_e * lane.scale_e).max(LOW_FLOOR);
                let hi_d = (k_high * lane.scale_d).max(HIGH_FLOOR);
                let lo_d = (k_low_d * lane.scale_d).max(LOW_FLOOR);
                for (i, &e) in win.iter().enumerate() {
                    let idx = w * WINDOW + i;
                    let frame = first_centre + idx as u64;
                    let (me, d) = (f64::from(e.abs()), f64::from(dwin[i]));
                    let md = d.abs();
                    if me > lo_e || md > lo_d {
                        let x = work[idx + 1].abs();
                        let high = me > hi_e || md > hi_d;
                        match &mut lane.open {
                            Some(o) if frame - o.last <= GAP + 1 => {
                                o.last = frame;
                                o.high |= high;
                                o.peak = o.peak.max(x);
                                if md > o.step.abs() {
                                    o.step = d;
                                    o.step_scale = lane.scale_d;
                                }
                            }
                            slot => {
                                let done = slot.take();
                                *slot = Some(Open { first: frame, last: frame, high, peak: x, step: d, step_scale: lane.scale_d, parts: Vec::new() });
                                if done.is_some() && settle(doc, &mut regions, lane, c, done, max_width)? {
                                    truncated = true;
                                    break 'outer;
                                }
                            }
                        }
                    } else if let Some(o) = &lane.open
                        && frame - o.last > GAP
                    {
                        let done = lane.open.take();
                        if settle(doc, &mut regions, lane, c, done, max_width)? {
                            truncated = true;
                            break 'outer;
                        }
                    }
                }
            }
        }
        at += m as u64;
    }
    if !truncated {
        for (k, lane) in lanes.iter_mut().enumerate() {
            let done = lane.open.take();
            if done.is_some() && settle(doc, &mut regions, lane, active[k], done, max_width)? {
                truncated = true;
                break;
            }
            if settle(doc, &mut regions, lane, active[k], None, max_width)? {
                truncated = true;
                break;
            }
        }
    }
    regions.sort_by_key(|r| (r.start, r.channel));
    Ok(Scan { regions, truncated })
}

/// New samples for one click, or `None` when it cannot be redrawn.
fn fit(doc: &DocState, r: &Region) -> io::Result<Option<Vec<f32>>> {
    if r.len > MAX_REPAIR_WIDTH {
        return Ok(None);
    }
    let total = doc.len_frames();
    let left = r.start.min(CONTEXT as u64) as usize;
    let right = (total - r.end()).min(CONTEXT as u64) as usize;
    if left < 1 || right < 1 {
        return Ok(None);
    }
    let n = r.len as usize;
    let mut seg = vec![0.0f32; left + n + right];
    let want = seg.len();
    if doc.read(r.channel, r.start - left as u64, &mut seg)? != want {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
    }
    let known = |i: usize| i < left || i >= left + n;
    let line = |seg: &mut [f32]| {
        let (a, b) = (f64::from(seg[left - 1]), f64::from(seg[left + n]));
        for i in 0..n {
            seg[left + i] = (a + (b - a) * (i as f64 + 1.0) / (n as f64 + 1.0)) as f32;
        }
    };
    if left < ORDER || right < ORDER {
        line(&mut seg);
        return Ok(Some(seg[left..left + n].to_vec()));
    }
    // Autocorrelation of the two sides, which together hold only good samples.
    let mut r_ac = [0.0f64; ORDER + 1];
    for side in [&seg[..left], &seg[left + n..]] {
        for (lag, slot) in r_ac.iter_mut().enumerate() {
            *slot += side.iter().zip(&side[lag..]).map(|(a, b)| f64::from(*a) * f64::from(*b)).sum::<f64>();
        }
    }
    if r_ac[0] < 1e-12 {
        return Ok(None);
    }
    r_ac[0] *= 1.0 + 1e-9;
    let Some(coef) = levinson(&r_ac) else {
        line(&mut seg);
        return Ok(Some(seg[left..left + n].to_vec()));
    };
    // Prediction-error kernel a = [1, -c1, .., -cp]; minimise sum_t (sum_j a_j s[t-j])^2 over the
    // unknowns s[left..left+n].
    let mut a = [0.0f64; ORDER + 1];
    a[0] = 1.0;
    for j in 1..=ORDER {
        a[j] = -coef[j - 1];
    }
    let mut bm = vec![0.0f64; n * n];
    let mut rhs = vec![0.0f64; n];
    for t in left..left + n + ORDER {
        if t >= seg.len() {
            break;
        }
        let konst: f64 = (0..=ORDER).filter(|&j| t >= j && known(t - j)).map(|j| a[j] * f64::from(seg[t - j])).sum();
        let unk: Vec<(usize, f64)> = (0..=ORDER).filter(|&j| t >= j && !known(t - j)).map(|j| (t - j - left, a[j])).collect();
        for &(i, ai) in &unk {
            rhs[i] -= ai * konst;
            for &(j, aj) in &unk {
                bm[i * n + j] += ai * aj;
            }
        }
    }
    let trace: f64 = (0..n).map(|i| bm[i * n + i]).sum::<f64>() / n as f64;
    for i in 0..n {
        bm[i * n + i] += 1e-9 * trace.max(1e-12);
    }
    match solve(bm, rhs, n) {
        Some(u) if u.iter().all(|v| v.is_finite()) => Ok(Some(u.iter().map(|v| v.clamp(-PEAK_LIMIT, PEAK_LIMIT) as f32).collect())),
        _ => {
            line(&mut seg);
            Ok(Some(seg[left..left + n].to_vec()))
        }
    }
}

/// Levinson-Durbin: predictor coefficients `c` (x[t] ~ sum c[j-1] x[t-j]) from autocorrelation.
fn levinson(r: &[f64; ORDER + 1]) -> Option<[f64; ORDER]> {
    let mut c = [0.0f64; ORDER];
    let mut err = r[0];
    for i in 0..ORDER {
        let acc: f64 = r[i + 1] - (0..i).map(|j| c[j] * r[i - j]).sum::<f64>();
        let k = acc / err;
        if !k.is_finite() || k.abs() >= 1.0 {
            return None;
        }
        let prev = c;
        c[i] = k;
        for j in 0..i {
            c[j] = prev[j] - k * prev[i - 1 - j];
        }
        err *= 1.0 - k * k;
        if err <= 0.0 {
            return None;
        }
    }
    Some(c)
}

/// Gaussian elimination with partial pivoting on an `n` x `n` system.
fn solve(mut m: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| m[i * n + col].abs().total_cmp(&m[j * n + col].abs()))?;
        if m[piv * n + col].abs() < 1e-14 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                m.swap(piv * n + k, col * n + k);
            }
            b.swap(piv, col);
        }
        for row in col + 1..n {
            let f = m[row * n + col] / m[col * n + col];
            if f != 0.0 {
                for k in col..n {
                    m[row * n + k] -= f * m[col * n + k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    let mut x = vec![0.0f64; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|j| m[i * n + j] * x[j]).sum();
        x[i] = (b[i] - s) / m[i * n + i];
    }
    Some(x)
}

fn repair(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    params: &Params,
    label: &'static str,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Repair, EditError> {
    let found = scan(doc, range, channels, params, cancel)?;
    let mut fixes: Vec<(usize, u64, Vec<f32>)> = Vec::new();
    let mut skipped = 0;
    for r in &found.regions {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        match fit(doc, r)? {
            Some(v) => fixes.push((r.channel, r.start, v)),
            None => skipped += 1,
        }
    }
    let (n_found, repaired) = (found.regions.len(), fixes.len());
    let edit = if fixes.is_empty() {
        edit::replace_range(doc, 0..0, ChannelMask::NONE, label, xf, cancel, |_, _, _| Ok(()))?
    } else {
        write_fixes(doc, fixes, label, xf, cancel)?
    };
    Ok(Repair { edit, found: n_found, repaired, skipped, truncated: found.truncated })
}

/// DeClicker: scan `range` of `channels` (empty range: the whole file) and redraw every click,
/// as one edit labelled "DeClicker".
pub fn declick(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    params: &Params,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Repair, EditError> {
    repair(doc, range, channels, params, LABEL, xf, cancel)
}

/// Automatic Click Remover: detect and repair in one command, as one edit labelled
/// "Automatic Click Remover". Gives the same samples as [`declick`] at the mapped settings.
pub fn auto_click_remove(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    params: &AutoParams,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Repair, EditError> {
    params.validate().map_err(invalid)?;
    repair(doc, range, channels, &params.to_params(), AUTO_LABEL, xf, cancel)
}
