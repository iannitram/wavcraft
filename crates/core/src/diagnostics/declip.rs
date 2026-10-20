//! DeClipper (B045): find the stretches where a signal was flattened against full scale, and
//! redraw each one as a smooth curve through the good samples either side.
//!
//! [`scan`] reads a range 64 K frames at a time and reports a [`Region`] for every run of at
//! least [`Params::min_run`] consecutive samples of one sign whose size is at or above the
//! threshold level. [`declip`] scans, fits a cubic (least squares over up to [`FIT_SIDE`]
//! samples each side) to every run, and writes the result through one
//! [`replace_range`](crate::doc::edit::replace_range) from the first run to the last, so the
//! whole repair is one undo step named "DeClipper". Samples outside the runs are copied
//! through untouched.
//!
//! Limits: at most [`MAX_REGIONS`] runs are held; a scan that meets more stops there and sets
//! [`Scan::truncated`] (the first runs in frame order are repaired; run it again for the
//! rest). A run longer than [`MAX_REPAIR_RUN`] frames is a held level, not a clipped peak: it
//! is reported by [`scan`] but left alone by [`declip`] (counted in [`Repair::skipped`], as is
//! a run with fewer than two good samples on a side, at a file edge). There is no gain-reduce
//! option: a repaired peak may exceed full scale (up to [`PEAK_LIMIT`]) and the owner can
//! apply Gain afterwards.

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Region;
use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError};
use crate::store::BLOCK_FRAMES;

/// The most runs one scan holds.
pub const MAX_REGIONS: usize = 100_000;
/// The longest run [`declip`] will redraw, in frames.
pub const MAX_REPAIR_RUN: u64 = 256;
/// Good samples used on each side of a run for the curve fit.
pub const FIT_SIDE: usize = 4;
/// A repaired sample never exceeds this size (full scale is 1.0).
pub const PEAK_LIMIT: f64 = 4.0;
/// The label of the undo step.
pub const LABEL: &str = "DeClipper";

/// Settings for a scan or repair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// The clip level, in dB below full scale (a negative number; 0 is full scale).
    pub threshold_db: f64,
    /// The fewest consecutive samples that count as a clipped run.
    pub min_run: u32,
}

impl Params {
    pub const THRESHOLD_DB: Range<f64> = -30.0..0.0;
    pub const MIN_RUN: Range<u32> = 2..1001;

    /// Whether the settings are finite and inside their ranges.
    pub fn validate(&self) -> Result<(), String> {
        if !self.threshold_db.is_finite() || self.threshold_db < Self::THRESHOLD_DB.start || self.threshold_db > Self::THRESHOLD_DB.end {
            return Err(format!("threshold_db = {} is not -30 to 0 dB", self.threshold_db));
        }
        if !Self::MIN_RUN.contains(&self.min_run) {
            return Err(format!("min_run = {} is not 2 to 1000", self.min_run));
        }
        Ok(())
    }

    /// Settings from `threshold_db=-0.1 min_run=3` (space-separated `key=value`; a key left out
    /// keeps its default; a value may end in `dB`), as `[threshold_db, min_run]`.
    pub fn parse(args: &str) -> Result<Vec<f64>, String> {
        let d = Params::default();
        let mut v = vec![d.threshold_db, f64::from(d.min_run)];
        for word in args.split_whitespace() {
            let Some((key, text)) = word.split_once('=') else { return Err(format!("{word:?}: write key=value")) };
            let i = match key.to_ascii_lowercase().as_str() {
                "threshold_db" => 0,
                "min_run" => 1,
                _ => return Err(format!("DeClipper has no setting {key:?} (it has: threshold_db, min_run)")),
            };
            let number = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            v[i] = number.parse::<f64>().map_err(|_| format!("{key}: {text:?} is not a number"))?;
        }
        if v[1].fract() != 0.0 || v[1] < 0.0 || v[1] > f64::from(u32::MAX) {
            return Err(format!("min_run = {} is not a whole number", v[1]));
        }
        Params { threshold_db: v[0], min_run: v[1] as u32 }.validate()?;
        Ok(v)
    }

    /// The linear clip level.
    fn level(&self) -> f64 {
        10f64.powf(self.threshold_db / 20.0)
    }
}

impl Default for Params {
    fn default() -> Self {
        Params { threshold_db: -0.1, min_run: 3 }
    }
}

fn invalid(why: String) -> EditError {
    EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why))
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    /// The runs, in frame order (channel order within a frame).
    pub regions: Vec<Region>,
    /// True when the scan stopped at [`MAX_REGIONS`] before reaching the end of the range.
    pub truncated: bool,
}

/// What a repair did.
pub struct Repair {
    /// The edit to push on the history (a no-op when nothing was repaired).
    pub edit: Edit,
    /// Runs found.
    pub found: usize,
    /// Runs redrawn.
    pub repaired: usize,
    /// Runs left alone (too long, or too close to a file edge).
    pub skipped: usize,
    /// The scan stopped at [`MAX_REGIONS`].
    pub truncated: bool,
}

/// An open run while scanning one channel.
struct Open {
    start: u64,
    positive: bool,
    len: u64,
    peak: f32,
}

/// Find the clipped runs of `channels` within `range` (empty: the whole file).
pub fn scan(doc: &DocState, range: Range<u64>, channels: ChannelMask, params: &Params, cancel: &AtomicBool) -> Result<Scan, EditError> {
    params.validate().map_err(invalid)?;
    let len = doc.len_frames();
    let range = if range.start < range.end { range } else { 0..len };
    if range.end > len {
        return Err(EditError::OutOfRange { range, len });
    }
    let level = params.level();
    let min_run = u64::from(params.min_run);
    let active = channels.active(doc.channel_count());
    let mut regions: Vec<Region> = Vec::new();
    let mut open: Vec<Option<Open>> = active.iter().map(|_| None).collect();
    let mut buf = vec![0.0f32; BLOCK_FRAMES];
    let mut at = range.start;
    let mut truncated = false;
    // A finished run is kept when long enough; reports whether the cap stopped the scan.
    let close = |regions: &mut Vec<Region>, ch: usize, o: Open| -> bool {
        if o.len >= min_run {
            if regions.len() >= MAX_REGIONS {
                return true;
            }
            regions.push(Region { channel: ch, start: o.start, len: o.len, peak: o.peak });
        }
        false
    };
    'outer: while at < range.end {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        let m = (range.end - at).min(BLOCK_FRAMES as u64) as usize;
        for (k, &c) in active.iter().enumerate() {
            if doc.read(c, at, &mut buf[..m])? != m {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
            for (i, &x) in buf[..m].iter().enumerate() {
                let hit = f64::from(x.abs()) >= level;
                let pos = x > 0.0;
                let slot = &mut open[k];
                match slot {
                    Some(o) if hit && o.positive == pos => {
                        o.len += 1;
                        o.peak = o.peak.max(x.abs());
                    }
                    _ => {
                        if let Some(o) = slot.take()
                            && close(&mut regions, c, o)
                        {
                            truncated = true;
                            break 'outer;
                        }
                        if hit {
                            *slot = Some(Open { start: at + i as u64, positive: pos, len: 1, peak: x.abs() });
                        }
                    }
                }
            }
        }
        at += m as u64;
    }
    if !truncated {
        for (k, slot) in open.iter_mut().enumerate() {
            if let Some(o) = slot.take()
                && close(&mut regions, active[k], o)
            {
                truncated = true;
                break;
            }
        }
    }
    regions.sort_by_key(|r| (r.start, r.channel));
    Ok(Scan { regions, truncated })
}

/// New samples for one run, or `None` when it cannot be redrawn.
fn fit(doc: &DocState, r: &Region) -> io::Result<Option<Vec<f32>>> {
    if r.len > MAX_REPAIR_RUN {
        return Ok(None);
    }
    let total = doc.len_frames();
    let before = r.start.min(FIT_SIDE as u64) as usize;
    let after = (total - r.end()).min(FIT_SIDE as u64) as usize;
    if before < 2 || after < 2 {
        return Ok(None);
    }
    let n = r.len as usize;
    let mut left = vec![0.0f32; before];
    let mut right = vec![0.0f32; after];
    let mut run = vec![0.0f32; n];
    if doc.read(r.channel, r.start - before as u64, &mut left)? != before
        || doc.read(r.channel, r.end(), &mut right)? != after
        || doc.read(r.channel, r.start, &mut run)? != n
    {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
    }
    // Positions are offsets from the run's first frame, scaled to about -1..1 for a stable fit.
    let centre = (n as f64 - 1.0) / 2.0;
    let scale = n as f64 / 2.0 + FIT_SIDE as f64;
    let points = left
        .iter()
        .enumerate()
        .map(|(j, &y)| ((j as f64 - before as f64), f64::from(y)))
        .chain(right.iter().enumerate().map(|(j, &y)| ((n + j) as f64, f64::from(y))));
    // Normal equations of a cubic least squares fit.
    let mut a = [[0.0f64; 5]; 4];
    for (p, y) in points {
        let x = (p - centre) / scale;
        let pw = [1.0, x, x * x, x * x * x];
        for i in 0..4 {
            for j in 0..4 {
                a[i][j] += pw[i] * pw[j];
            }
            a[i][4] += pw[i] * y;
        }
    }
    let Some(c) = solve4(a) else { return Ok(None) };
    let out = run
        .iter()
        .enumerate()
        .map(|(i, &orig)| {
            let x = (i as f64 - centre) / scale;
            let v = c[0] + x * (c[1] + x * (c[2] + x * c[3]));
            // Keep the sign of the clipped run, never lower than the clipped sample, never wild.
            let sign = if orig < 0.0 { -1.0 } else { 1.0 };
            let mag = (sign * v).max(f64::from(orig.abs())).min(PEAK_LIMIT);
            (sign * mag) as f32
        })
        .collect();
    Ok(Some(out))
}

/// Gaussian elimination with partial pivoting on a 4x4 system (augmented column 4).
fn solve4(mut a: [[f64; 5]; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let piv = (col..4).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        for row in col + 1..4 {
            let f = a[row][col] / a[col][col];
            let pivot_row = a[col];
            for (dst, src) in a[row].iter_mut().zip(pivot_row).skip(col) {
                *dst -= f * src;
            }
        }
    }
    let mut x = [0.0f64; 4];
    for i in (0..4).rev() {
        let s: f64 = (i + 1..4).map(|j| a[i][j] * x[j]).sum();
        x[i] = (a[i][4] - s) / a[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Scan `range` of `channels` (empty range: the whole file) and redraw every clipped run, as
/// one edit labelled "DeClipper". Only the channels that had a run are rewritten.
pub fn declip(
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    params: &Params,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Repair, EditError> {
    let found = scan(doc, range, channels, params, cancel)?;
    // (channel, first frame, new samples), sorted by channel then frame.
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
    let repaired = fixes.len();
    let nothing = |edit| Repair { edit, found: found.regions.len(), repaired, skipped, truncated: found.truncated };
    if fixes.is_empty() {
        // An empty range is a no-op edit that keeps the history untouched.
        return Ok(nothing(edit::replace_range(doc, 0..0, ChannelMask::NONE, LABEL, xf, cancel, |_, _, _| Ok(()))?));
    }
    let edit = super::write_fixes(doc, fixes, LABEL, xf, cancel)?;
    Ok(nothing(edit))
}
