//! One-shot effects on a selection (roadmap B002 and the first effects): Reverse, Amplify,
//! Normalize, Fade In and Fade Out. Each is a pure function `&DocState -> Edit` like the edits in
//! [`crate::doc::edit`], and streams through [`replace_range`]: 64 K-frame chunks, never the whole
//! range in RAM. The maths comes from `soundcraft-dsp`'s offline functions (`reverse`, `gain`) and
//! its fade curves ([`FadeShape`]); only the chunk bookkeeping is ours.
//!
//! The effect seam (f01) lives in the second half of this file: an [`Effect`] is a named,
//! parameterised sample processor. [`apply`] runs one over a range of a [`DocState`] through the
//! streamed [`replace_range`] path, labelled with the effect's name for the undo step. The
//! registry ([`all`], [`find`]) feeds the action table (`effect.<id>`, see
//! [`crate::actions::all_actions`]) and the app's Effects menu.
//!
//! To add a seam effect: write a type implementing [`Effect`] in its own file, and add it to
//! [`REGISTRY`]. Nothing else changes.

use std::io;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

use soundcraft_dsp::offline::{self, FadeShape};
use soundcraft_dsp::{db_to_gain, gain_to_db};

use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError, replace_range};

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

mod gain;
pub use gain::Gain;

/// What a setting means, so a dialog can pick the right control and units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A level in decibels.
    Db,
    /// A share, 0 to 100 (`min`/`max` may narrow it).
    Percent,
    /// A duration in `unit` (`ms` or `s`).
    Time,
    /// One of [`ParamDesc::choices`]; the value is the index.
    Choice,
    /// A plain number in `unit` (`Hz`, or none); a dialog draws a logarithmic slider for `Hz`
    /// (f21).
    Number,
}

/// One adjustable setting, described for a dialog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDesc {
    /// Stable key (`gain_db`), also the `key` in `EFFECT <name> key=value`.
    pub id: &'static str,
    /// Text for the dialog.
    pub label: &'static str,
    pub kind: ParamKind,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// `dB`, `%`, `ms`, `s`, `Hz`, or empty.
    pub unit: &'static str,
    /// The names of a [`ParamKind::Choice`]'s options (empty for the others).
    pub choices: &'static [&'static str],
}

impl ParamDesc {
    pub const fn db(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Db, min, max, default, unit: "dB", choices: &[] }
    }
    pub const fn percent(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Percent, min, max, default, unit: "%", choices: &[] }
    }
    pub const fn time(id: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Time, min, max, default, unit, choices: &[] }
    }
    pub const fn number(id: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Number, min, max, default, unit, choices: &[] }
    }
    pub const fn choice(id: &'static str, label: &'static str, choices: &'static [&'static str], default: usize) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Choice, min: 0.0, max: (choices.len() - 1) as f64, default: default as f64, unit: "", choices }
    }
}

/// The settings for one run, in the order of [`Effect::parameters`].
pub struct Params;

impl Params {
    /// Default settings for `effect`.
    pub fn defaults(effect: &dyn Effect) -> Vec<f64> {
        effect.parameters().iter().map(|p| p.default).collect()
    }
}

fn range_text(p: &ParamDesc) -> String {
    match p.kind {
        ParamKind::Choice => format!("one of {}", p.choices.join(", ")),
        _ => format!("{} to {}{}", p.min, p.max, if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) }),
    }
}

/// Whether `params` suit `effect`: one finite value per parameter, each inside its range (a
/// choice a whole index).
pub fn validate(effect: &dyn Effect, params: &[f64]) -> Result<(), String> {
    validate_list(effect.name(), effect.parameters(), params)
}

/// [`validate`] for any named list of settings (generators use it too).
pub fn validate_list(name: &str, descs: &[ParamDesc], params: &[f64]) -> Result<(), String> {
    if params.len() != descs.len() {
        return Err(format!("{name} takes {} setting(s), got {}", descs.len(), params.len()));
    }
    for (p, v) in descs.iter().zip(params) {
        let whole = p.kind != ParamKind::Choice || v.fract() == 0.0;
        if !v.is_finite() || *v < p.min || *v > p.max || !whole {
            return Err(format!("{name}: {} = {v} is not {}", p.id, range_text(p)));
        }
    }
    Ok(())
}

/// Settings from text such as `gain_db=-6` (space-separated `key=value`), over the defaults:
/// a key left out keeps its default. A value may carry its unit (`-6dB`, `25%`, `100ms`); a
/// choice is a name (any case) or an index.
pub fn parse_params(effect: &dyn Effect, args: &str) -> Result<Vec<f64>, String> {
    parse_list(effect.name(), effect.parameters(), args)
}

/// [`parse_params`] for any named list of settings (generators use it too).
pub fn parse_list(name: &str, descs: &[ParamDesc], args: &str) -> Result<Vec<f64>, String> {
    let mut out: Vec<f64> = descs.iter().map(|p| p.default).collect();
    for word in args.split_whitespace() {
        let Some((key, text)) = word.split_once('=') else { return Err(format!("{word:?}: write key=value")) };
        let Some(i) = descs.iter().position(|p| p.id.eq_ignore_ascii_case(key)) else {
            let known: Vec<_> = descs.iter().map(|p| p.id).collect();
            return Err(format!("{name} has no setting {key:?} (it has: {})", if known.is_empty() { "none".to_string() } else { known.join(", ") }));
        };
        let p = &descs[i];
        let by_name = p.choices.iter().position(|c| c.eq_ignore_ascii_case(text));
        out[i] = if let (ParamKind::Choice, Some(n)) = (p.kind, by_name) {
            n as f64
        } else {
            let number = text.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
            number.parse::<f64>().map_err(|_| format!("{}: {text:?} is not a number", p.id))?
        };
    }
    validate_list(name, descs, &out)?;
    Ok(out)
}

/// One line describing the settings, for the control channel: `gain_db=0 [dB, -60 to 24]`.
pub fn describe(effect: &dyn Effect) -> String {
    describe_list(effect.parameters())
}

/// [`describe`] for any list of settings.
pub fn describe_list(descs: &[ParamDesc]) -> String {
    descs.iter().map(|p| format!("{}={} ({})", p.id, p.default, range_text(p))).collect::<Vec<_>>().join(" ")
}

/// A process over audio.
pub trait Effect: Sync {
    /// Stable lowercase key; the control channel calls the effect `effect.<id>`.
    fn id(&self) -> &'static str;
    /// The menu text and the undo step's name (`Invert`).
    fn name(&self) -> &'static str;
    /// The settings a dialog shows. Empty for an effect with none.
    fn parameters(&self) -> &'static [ParamDesc] {
        &[]
    }
    /// Process one chunk. `first_frame` is the chunk's first frame within the document; `input`
    /// and `output` hold one slice per active channel, all the same length. The output has the
    /// same length as the input (the effect seam is length-preserving).
    fn process(&self, params: &[f64], first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()>;
}

/// Polarity inversion: every sample times -1. The reference effect.
pub struct Invert;

impl Effect for Invert {
    fn id(&self) -> &'static str {
        "invert"
    }
    fn name(&self) -> &'static str {
        "Invert"
    }
    fn process(&self, _params: &[f64], _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = -*a;
            }
        }
        Ok(())
    }
}

/// Every effect, in Effects-menu order.
pub static REGISTRY: &[&dyn Effect] = &[&Invert, &Gain];

/// Every registered effect.
pub fn all() -> &'static [&'static dyn Effect] {
    REGISTRY
}

/// The effect with this [`Effect::id`].
pub fn find(id: &str) -> Option<&'static dyn Effect> {
    REGISTRY.iter().copied().find(|e| e.id() == id)
}

/// Run `effect` over `range` of `doc` on `channels`, as one undoable edit named for the effect.
/// An empty `range` means the whole file. Streams through `replace_range`; `cancel` stops it
/// between chunks ([`EditError::Cancelled`], nothing changed).
pub fn apply(
    effect: &dyn Effect,
    params: &[f64],
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Edit, EditError> {
    validate(effect, params).map_err(|why| EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why)))?;
    let range = if range.start < range.end { range } else { 0..doc.len_frames() };
    edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, input, output| effect.process(params, at, input, output))
}
