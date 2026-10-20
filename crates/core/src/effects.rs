//! One-shot effects on a selection (roadmap B002 and the first effects): Reverse, Amplify,
//! Normalize, Fade In and Fade Out. Each is a pure function `&DocState -> Edit` like the edits in
//! [`crate::doc::edit`], and streams through [`replace_range`]: 64 K-frame chunks, never the whole
//! range in RAM. The maths comes from `soundcraft-dsp`'s offline functions (`reverse`, `gain`) and
//! its fade curves ([`FadeShape`]); only the chunk bookkeeping is ours.

use std::ops::Range;
use std::sync::atomic::AtomicBool;

use soundcraft_dsp::offline::{self, FadeShape};
use soundcraft_dsp::{db_to_gain, gain_to_db};

use crate::doc::DocState;
use crate::doc::edit::{ChannelMask, Crossfades, Edit, EditError, replace_range};

/// The curve of a fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeCurve {
    Linear,
    /// Raised cosine (an S-curve): gentle at both ends.
    RaisedCosine,
}

impl FadeCurve {
    fn shape(self) -> FadeShape {
        match self {
            FadeCurve::Linear => FadeShape::Linear,
            FadeCurve::RaisedCosine => FadeShape::SCurve,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear",
            FadeCurve::RaisedCosine => "Raised Cosine",
        }
    }
}

/// Which end of the selection a fade works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeDir {
    In,
    Out,
}

/// The settings the effects with a number keep between runs (set from the control channel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectParams {
    /// Amplify: the gain in dB.
    pub amplify_db: f32,
    /// Normalize: the peak to reach, in dBFS.
    pub normalize_db: f32,
}

impl Default for EffectParams {
    fn default() -> Self {
        EffectParams { amplify_db: 3.0, normalize_db: -1.0 }
    }
}

/// The highest gain Amplify accepts (what `soundcraft-dsp` itself allows).
pub const MAX_GAIN_DB: f32 = 96.0;

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// Reverse: the selection played backwards. A chunk of the output is the mirror chunk of the
/// input, read straight from the document.
pub fn reverse(doc: &DocState, range: Range<u64>, channels: ChannelMask, xf: Crossfades) -> Result<Edit, EditError> {
    let active = channels.active(doc.channel_count());
    let (start, end) = (range.start, range.end);
    replace_range(doc, range, channels, "Reverse", xf, &never(), |at, _ins, outs| {
        let m = outs.first().map_or(0, |o| o.len()) as u64;
        // Output frames [at, at + m) are input frames (end - (at - start) - m)..(end - (at - start)) reversed.
        let from = end - (at - start) - m;
        let mut bufs: Vec<Vec<f32>> = Vec::with_capacity(active.len());
        for &c in &active {
            let mut v = vec![0.0f32; m as usize];
            let got = doc.read(c, from, &mut v)?;
            v.truncate(got);
            bufs.push(v);
        }
        offline::reverse(&mut bufs);
        for (o, b) in outs.iter_mut().zip(&bufs) {
            o[..b.len()].copy_from_slice(b);
        }
        Ok(())
    })
}

/// Amplify: `db` of gain (negative attenuates). Nothing is clipped; samples may pass full scale.
pub fn amplify(doc: &DocState, range: Range<u64>, channels: ChannelMask, db: f32, xf: Crossfades) -> Result<Edit, EditError> {
    let db = if db.is_finite() { db.min(MAX_GAIN_DB) } else { 0.0 };
    replace_range(doc, range, channels, "Amplify", xf, &never(), |_at, ins, outs| {
        let mut bufs: Vec<Vec<f32>> = ins.iter().map(|i| i.to_vec()).collect();
        offline::gain(&mut bufs, db);
        for (o, b) in outs.iter_mut().zip(&bufs) {
            o.copy_from_slice(b);
        }
        Ok(())
    })
}

/// The loudest sample of `range` over the active channels, in a streaming pass.
pub fn peak(doc: &DocState, range: Range<u64>, channels: ChannelMask) -> Result<f32, EditError> {
    let mut buf = vec![0.0f32; crate::store::BLOCK_FRAMES];
    let mut peak = 0.0f32;
    for c in channels.active(doc.channel_count()) {
        let mut at = range.start;
        while at < range.end {
            let m = (range.end - at).min(buf.len() as u64) as usize;
            let got = doc.read(c, at, &mut buf[..m])?;
            peak = buf[..got].iter().filter(|v| v.is_finite()).fold(peak, |p, v| p.max(v.abs()));
            if got == 0 {
                break;
            }
            at += got as u64;
        }
    }
    Ok(peak)
}

/// Normalize: scale the selection so its loudest sample (over the active channels together)
/// reaches `target_db` dBFS. Silence is left alone: the result is no edit.
pub fn normalize(doc: &DocState, range: Range<u64>, channels: ChannelMask, target_db: f32, xf: Crossfades) -> Result<Edit, EditError> {
    let target = if target_db.is_finite() { target_db.clamp(-144.0, 24.0) } else { 0.0 };
    let p = peak(doc, range.clone(), channels)?;
    if p <= 0.0 {
        return Ok(Edit::noop(doc, "Normalize"));
    }
    let db = gain_to_db(db_to_gain(target) / p).min(MAX_GAIN_DB);
    replace_range(doc, range, channels, "Normalize", xf, &never(), |_at, ins, outs| {
        let mut bufs: Vec<Vec<f32>> = ins.iter().map(|i| i.to_vec()).collect();
        offline::gain(&mut bufs, db);
        for (o, b) in outs.iter_mut().zip(&bufs) {
            o.copy_from_slice(b);
        }
        Ok(())
    })
}

/// The label a fade gets in the history: "Fade In (Linear)".
pub fn fade_label(dir: FadeDir, curve: FadeCurve) -> String {
    format!("Fade {} ({})", if dir == FadeDir::In { "In" } else { "Out" }, curve.name())
}

/// Fade the whole selection in (from silence to unity) or out (to silence) along `curve`.
pub fn fade(doc: &DocState, range: Range<u64>, channels: ChannelMask, dir: FadeDir, curve: FadeCurve, xf: Crossfades) -> Result<Edit, EditError> {
    let (start, n) = (range.start, (range.end - range.start).max(1));
    let shape = curve.shape();
    replace_range(doc, range, channels, &fade_label(dir, curve), xf, &never(), |at, ins, outs| {
        for (i, o) in ins.iter().zip(outs.iter_mut()) {
            for (k, (s, d)) in i.iter().zip(o.iter_mut()).enumerate() {
                let pos = at - start + k as u64;
                let t = match dir {
                    FadeDir::In => pos as f32 / n as f32,
                    FadeDir::Out => (n - 1 - pos) as f32 / n as f32,
                };
                *d = *s * shape.gain(t);
            }
        }
        Ok(())
    })
}
