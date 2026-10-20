//! The effect seam (f01): an [`Effect`] is a named, parameterised sample processor. [`apply`]
//! runs one over a range of a [`DocState`] through the streamed
//! [`replace_range`](crate::doc::edit::replace_range) path (64 K-frame chunks, so a file is never
//! held in RAM), labelled with the effect's name for the undo step. The registry ([`all`],
//! [`find`]) feeds the action table (`effect.<id>`, see [`crate::actions::all_actions`]) and the
//! app's Effects menu.
//!
//! To add an effect: write a type implementing [`Effect`] in its own file, and add it to
//! [`REGISTRY`]. Nothing else changes.

use std::io;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError};

mod gain;
mod noise_reduction;
pub use gain::Gain;
pub use noise_reduction::NoiseReduction;

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
    let descs = effect.parameters();
    if params.len() != descs.len() {
        return Err(format!("{} takes {} setting(s), got {}", effect.name(), descs.len(), params.len()));
    }
    for (p, v) in descs.iter().zip(params) {
        let whole = p.kind != ParamKind::Choice || v.fract() == 0.0;
        if !v.is_finite() || *v < p.min || *v > p.max || !whole {
            return Err(format!("{}: {} = {v} is not {}", effect.name(), p.id, range_text(p)));
        }
    }
    Ok(())
}

/// Settings from text such as `gain_db=-6` (space-separated `key=value`), over the defaults:
/// a key left out keeps its default. A value may carry its unit (`-6dB`, `25%`, `100ms`); a
/// choice is a name (any case) or an index.
pub fn parse_params(effect: &dyn Effect, args: &str) -> Result<Vec<f64>, String> {
    let descs = effect.parameters();
    let mut out = Params::defaults(effect);
    for word in args.split_whitespace() {
        let Some((key, text)) = word.split_once('=') else { return Err(format!("{word:?}: write key=value")) };
        let Some(i) = descs.iter().position(|p| p.id.eq_ignore_ascii_case(key)) else {
            let known: Vec<_> = descs.iter().map(|p| p.id).collect();
            return Err(format!(
                "{} has no setting {key:?} (it has: {})",
                effect.name(),
                if known.is_empty() { "none".to_string() } else { known.join(", ") }
            ));
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
    validate(effect, &out)?;
    Ok(out)
}

/// One line describing the settings, for the control channel: `gain_db=0 [dB, -60 to 24]`.
pub fn describe(effect: &dyn Effect) -> String {
    effect.parameters().iter().map(|p| format!("{}={} ({})", p.id, p.default, range_text(p))).collect::<Vec<_>>().join(" ")
}

/// Where a chunk handed to [`Effect::process_in_context`] sits in the document.
pub struct ChunkCtx<'a> {
    /// The document frame of the chunk's first frame (after the lead).
    pub first_frame: u64,
    /// The first frame of the range being processed: the origin of a spectral effect's frame grid.
    pub range_start: u64,
    /// Frames of context before the chunk in each input slice (and the same number after it).
    pub lead: usize,
    /// The document channel of each input slice.
    pub channels: &'a [usize],
    pub sample_rate: u32,
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
    /// How many frames of surrounding audio [`process_in_context`](Self::process_in_context)
    /// wants on each side of a chunk (0: none, the chunk alone is enough). A spectral effect
    /// asks for its FFT length plus any warm-up, so the overlap is right across chunk edges.
    fn context_frames(&self, _params: &[f64], _sample_rate: u32) -> usize {
        0
    }
    /// Process one chunk with context. Every `input` slice holds `ctx.lead` frames of context,
    /// the chunk (as long as each `output`), then `ctx.lead` frames of context; the document's
    /// real audio, or silence beyond its ends. The default hands the chunk to
    /// [`process`](Self::process).
    fn process_in_context(&self, params: &[f64], ctx: &ChunkCtx, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let m = output.first().map_or(0, |o| o.len());
        let chunk: Vec<&[f32]> = input.iter().map(|i| i.get(ctx.lead..ctx.lead + m).unwrap_or(&[])).collect();
        self.process(params, ctx.first_frame, &chunk, output)
    }
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
pub static REGISTRY: &[&dyn Effect] = &[&Invert, &Gain, &NoiseReduction];

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
    let rate = doc.sample_rate();
    let lead = effect.context_frames(params, rate);
    if lead == 0 {
        return edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, input, output| {
            effect.process(params, at, input, output)
        });
    }
    // An effect that wants context reads it itself: each chunk plus `lead` frames either side
    // (silence beyond the document), so a spectral effect's output is the same however the
    // range was cut into chunks.
    let active = channels.active(doc.channel_count());
    let mut bufs: Vec<Vec<f32>> = vec![Vec::new(); active.len()];
    edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, _input, output| {
        let m = output.first().map_or(0, |o| o.len());
        let first = at;
        let total = lead + m + lead;
        let from = first as i64 - lead as i64;
        for (buf, &c) in bufs.iter_mut().zip(&active) {
            buf.clear();
            buf.resize(total, 0.0);
            let skip = if from < 0 { from.unsigned_abs() as usize } else { 0 };
            if skip < total {
                doc.read(c, from.max(0) as u64, &mut buf[skip..])?;
            }
        }
        let ins: Vec<&[f32]> = bufs.iter().map(|b| &b[..]).collect();
        let ctx = ChunkCtx { first_frame: first, range_start: range.start, lead, channels: &active, sample_rate: rate };
        effect.process_in_context(params, &ctx, &ins, output)
    })
}
