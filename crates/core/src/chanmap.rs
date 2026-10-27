//! Audio channel mapping and the output buffer size: the pure logic behind the Audio Hardware and
//! Audio Channel Mapping preferences.
//!
//! A [`ChannelMap`] is a set of `from → to` pairs. For **output** the pair is
//! `document channel → device output channel`; for **input** it is
//! `device input channel → record channel`. In the settings file and on the control channel
//! channels count from 1 (as on an interface's panel); in memory they count from 0.
//!
//! An empty map is "no choice made": the player keeps its automatic layout (a mono file on both
//! speakers, a stereo file on the first two outputs, the mean on a one-channel device). Once a
//! pair is set the map is explicit: only mapped document channels are heard, each on its device
//! channel, and two document channels mapped to one device channel are summed.

use std::fmt;

/// The most channels a map accepts (a number above this is a typing mistake).
pub const MAX_CHANNELS: usize = 256;

/// A set of `from → to` channel pairs (0-based inside), at most one `to` for each `from`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelMap {
    pairs: Vec<(u16, u16)>,
}

impl ChannelMap {
    /// Whether no pair is set (the automatic layout).
    pub fn is_default(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The pairs, sorted by `from`, as 0-based `(from, to)`.
    pub fn pairs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.pairs.iter().map(|&(f, t)| (usize::from(f), usize::from(t)))
    }

    /// Map `from` to `to` (both 0-based), replacing any earlier pair for `from`.
    pub fn set(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= MAX_CHANNELS || to >= MAX_CHANNELS {
            return Err(format!("channels are numbered 1 to {MAX_CHANNELS}"));
        }
        self.remove(from);
        self.pairs.push((from as u16, to as u16));
        self.pairs.sort_unstable();
        Ok(())
    }

    /// Forget the pair for `from`.
    pub fn remove(&mut self, from: usize) {
        self.pairs.retain(|&(f, _)| usize::from(f) != from);
    }

    /// Where `from` goes, if it is mapped.
    pub fn get(&self, from: usize) -> Option<usize> {
        self.pairs.iter().find(|&&(f, _)| usize::from(f) == from).map(|&(_, t)| usize::from(t))
    }

    /// The `from` channels that feed `to`: no allocation, so the audio callback can call it.
    pub fn sources(&self, to: usize) -> impl Iterator<Item = usize> + '_ {
        self.pairs.iter().filter(move |&&(_, t)| usize::from(t) == to).map(|&(f, _)| usize::from(f))
    }

    /// Parse `1:3,2:4` (1-based `from:to`, commas or spaces between pairs). Empty text is the
    /// empty map. A repeated `from` is an error.
    pub fn parse(text: &str) -> Result<ChannelMap, String> {
        let mut map = ChannelMap::default();
        for item in text.split([',', ' ', ';']).map(str::trim).filter(|s| !s.is_empty()) {
            let bad = || format!("{item:?} is not a channel pair like 1:3 (channels count from 1)");
            let (a, b) = item.split_once(':').ok_or_else(bad)?;
            let number = |s: &str| s.trim().parse::<usize>().ok().filter(|n| (1..=MAX_CHANNELS).contains(n));
            let (from, to) = (number(a).ok_or_else(bad)?, number(b).ok_or_else(bad)?);
            if map.get(from - 1).is_some() {
                return Err(format!("channel {from} is mapped twice"));
            }
            map.set(from - 1, to - 1)?;
        }
        Ok(map)
    }
}

impl fmt::Display for ChannelMap {
    /// The text [`ChannelMap::parse`] reads back (1-based; empty for the default).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (from, to)) in self.pairs().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{}:{}", from + 1, to + 1)?;
        }
        Ok(())
    }
}

/// The buffer size to ask the device for: `requested` (frames) clamped into the device's
/// supported `range`; `None` (the device default) when nothing was requested, or the device does
/// not say what it supports (asking for a size it may refuse would lose the output).
pub fn choose_buffer(requested: Option<u32>, range: Option<(u32, u32)>) -> Option<u32> {
    let wanted = requested.filter(|&n| n > 0)?;
    let (min, max) = range?;
    Some(wanted.clamp(min.min(max), max.max(min)))
}

/// Buffer sizes worth offering for a device that supports `range`: powers of two inside it, and
/// both ends.
pub fn buffer_choices(range: (u32, u32)) -> Vec<u32> {
    let (min, max) = (range.0.min(range.1), range.0.max(range.1));
    let mut out = vec![min];
    let mut n = 16u32;
    while n <= max && n != 0 {
        if n > min && n < max {
            out.push(n);
        }
        n = n.saturating_mul(2);
        if n == u32::MAX {
            break;
        }
    }
    if max != min {
        out.push(max);
    }
    out
}
