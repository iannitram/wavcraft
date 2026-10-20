//! Diagnostics (B045 and later): finding and mending faults in recorded audio. Each tool scans
//! a document read-only, reports [`Region`]s, and may offer a repair that goes through the
//! streamed [`replace_range`](crate::doc::edit::replace_range) path as one undo step.

pub mod declick;
pub mod declip;

use std::sync::atomic::AtomicBool;

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
