//! Diagnostics (B045 and later): finding and mending faults in recorded audio. Each tool scans
//! a document read-only, reports [`Region`]s, and may offer a repair that goes through the
//! streamed [`replace_range`](crate::doc::edit::replace_range) path as one undo step.

pub mod declick;
pub mod declip;

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError};

/// A stretch of one channel a diagnostic flagged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    /// Channel index in the document.
    pub channel: usize,
    /// First frame of the stretch, in the document.
    pub start: u64,
    /// Number of frames.
    pub len: u64,
    /// The largest absolute sample value inside the stretch.
    pub peak: f32,
}

impl Region {
    /// One past the last frame.
    pub fn end(&self) -> u64 {
        self.start + self.len
    }
}

/// Write replacement samples into a document as one edit labelled `label`. `fixes` are
/// `(channel, first frame, new samples)`, disjoint within a channel; nothing outside them
/// changes. One streamed [`replace_range`](edit::replace_range) from the first fix to the last.
pub(crate) fn write_fixes(
    doc: &DocState,
    mut fixes: Vec<(usize, u64, Vec<f32>)>,
    label: &'static str,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Edit, EditError> {
    fixes.sort_by_key(|f| (f.0, f.1));
    let touched = fixes.iter().fold(ChannelMask::NONE, |m, f| m.with(f.0));
    let active = touched.active(doc.channel_count());
    // Pad the span by the edge crossfade so unchanged audio next to it passes through unchanged.
    let pad = Crossfades::frames(xf.edge, doc.sample_rate()) + 1;
    let first = fixes.iter().map(|f| f.1).min().unwrap_or(0);
    let last = fixes.iter().map(|f| f.1 + f.2.len() as u64).max().unwrap_or(0);
    let span = first.saturating_sub(pad)..(last + pad).min(doc.len_frames());
    let mut next: Vec<usize> = vec![0; active.len()];
    let per_channel: Vec<Vec<&(usize, u64, Vec<f32>)>> = active.iter().map(|&c| fixes.iter().filter(|f| f.0 == c).collect()).collect();
    edit::replace_range(doc, span, touched, label, xf, cancel, |at, input, output| {
        for (k, (i, o)) in input.iter().zip(output.iter_mut()).enumerate() {
            o.copy_from_slice(i);
            let end = at + o.len() as u64;
            let list = &per_channel[k];
            while next[k] < list.len() && list[next[k]].1 + list[next[k]].2.len() as u64 <= at {
                next[k] += 1;
            }
            let mut j = next[k];
            while j < list.len() && list[j].1 < end {
                let (_, s, v) = list[j];
                for (n, &y) in v.iter().enumerate() {
                    let f = s + n as u64;
                    if f >= at && f < end {
                        o[(f - at) as usize] = y;
                    }
                }
                j += 1;
            }
        }
        Ok(())
    })
}

/// What a scan found, whichever tool ran it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The regions, in frame order.
    pub regions: Vec<Region>,
    /// The scan stopped at the tool's region cap before the end of the range.
    pub truncated: bool,
}

/// What a subset repair did.
pub struct Subset {
    /// The edit to push on the history (a no-op when nothing was repaired).
    pub edit: Edit,
    /// Regions redrawn.
    pub repaired: usize,
    /// Regions left alone (too long, or too close to a file edge).
    pub skipped: usize,
}

/// A diagnostic tool with its settings: scan, then repair any chosen subset of what it found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    DeClip(declip::Params),
    DeClick(declick::Params),
}

impl Tool {
    /// The tool's name, also the label of its undo step.
    pub fn label(&self) -> &'static str {
        match self {
            Tool::DeClip(_) => declip::LABEL,
            Tool::DeClick(_) => declick::LABEL,
        }
    }

    /// Find the regions of `channels` within `range` (empty: the whole file).
    pub fn scan(&self, doc: &DocState, range: Range<u64>, channels: ChannelMask, cancel: &AtomicBool) -> Result<Found, EditError> {
        Ok(match self {
            Tool::DeClip(p) => {
                let s = declip::scan(doc, range, channels, p, cancel)?;
                Found { regions: s.regions, truncated: s.truncated }
            }
            Tool::DeClick(p) => {
                let s = declick::scan(doc, range, channels, p, cancel)?;
                Found { regions: s.regions, truncated: s.truncated }
            }
        })
    }

    /// For each region, whether a repair would leave it alone.
    pub fn skipped(&self, doc: &DocState, regions: &[Region], cancel: &AtomicBool) -> Result<Vec<bool>, EditError> {
        let mut out = Vec::with_capacity(regions.len());
        for r in regions {
            if cancel.load(Ordering::Relaxed) {
                return Err(EditError::Cancelled);
            }
            out.push(self.fit(doc, r)?.is_none());
        }
        Ok(out)
    }

    /// Redraw only `regions` (as found by a scan of this document), as one edit labelled with
    /// the tool's name. Nothing outside them changes.
    pub fn repair(&self, doc: &DocState, regions: &[Region], xf: Crossfades, cancel: &AtomicBool) -> Result<Subset, EditError> {
        repair_regions(doc, regions, self.label(), xf, cancel, |d, r| self.fit(d, r))
    }

    fn fit(&self, doc: &DocState, r: &Region) -> io::Result<Option<Vec<f32>>> {
        match self {
            Tool::DeClip(_) => declip::fit(doc, r),
            Tool::DeClick(_) => declick::fit(doc, r),
        }
    }
}

/// Fit and write every region `fit` can redraw as one edit; count the ones it cannot.
pub(crate) fn repair_regions(
    doc: &DocState,
    regions: &[Region],
    label: &'static str,
    xf: Crossfades,
    cancel: &AtomicBool,
    fit: impl Fn(&DocState, &Region) -> io::Result<Option<Vec<f32>>>,
) -> Result<Subset, EditError> {
    let mut fixes: Vec<(usize, u64, Vec<f32>)> = Vec::new();
    let mut skipped = 0;
    for r in regions {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        match fit(doc, r)? {
            Some(v) => fixes.push((r.channel, r.start, v)),
            None => skipped += 1,
        }
    }
    let repaired = fixes.len();
    let edit = if fixes.is_empty() {
        // An empty range is a no-op edit that keeps the history untouched.
        edit::replace_range(doc, 0..0, ChannelMask::NONE, label, xf, cancel, |_, _, _| Ok(()))?
    } else {
        write_fixes(doc, fixes, label, xf, cancel)?
    };
    Ok(Subset { edit, repaired, skipped })
}
