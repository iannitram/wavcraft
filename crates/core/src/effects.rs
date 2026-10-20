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

mod dynamics;
mod gain;
pub use dynamics::{Compressor, DeEsser, HardLimiter};
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
    /// A plain number in `unit` (`Hz`, `:1`).
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
    /// For an effect that keeps state from one chunk to the next (a dynamics envelope, a filter,
    /// a look-ahead window; f28): a processor for one run over a range, at the document's
    /// sample rate and with `channels` active channels. When this returns `Some`, [`apply`] uses
    /// it and never calls [`Effect::process`] (which such an effect may refuse).
    fn stream(&self, _params: &[f64], _sample_rate: u32, _channels: usize) -> Option<Box<dyn Stream>> {
        None
    }
}

/// One run of a stateful effect (see [`Effect::stream`]). It sees the range's audio in order,
/// in blocks of any size, and works in place. With a [`latency`](Stream::latency) of `L` frames
/// the audio it hands back in a call is the input of `L` frames earlier; [`apply`] feeds it
/// `L` frames ahead (read from the document, zeros past the end) so the result lines up.
pub trait Stream: Send {
    fn latency(&self) -> usize {
        0
    }
    /// Process one block in place; one slice per active channel, all the same length.
    fn process(&mut self, io: &mut [&mut [f32]]);
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
pub static REGISTRY: &[&dyn Effect] = &[&Invert, &Gain, &Compressor, &HardLimiter, &DeEsser];

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
    let active = channels.active(doc.channel_count());
    if let Some(stream) = effect.stream(params, doc.sample_rate(), active.len()) {
        return apply_stream(stream, doc, range, channels, &active, effect.name(), xf, cancel);
    }
    edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, input, output| {
        effect.process(params, range.start + at, input, output)
    })
}

/// [`apply`] for a [`Stream`]: one pass over the range, each chunk read `latency` frames ahead.
#[allow(clippy::too_many_arguments)]
fn apply_stream(
    mut stream: Box<dyn Stream>,
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    active: &[usize],
    label: &str,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Edit, EditError> {
    let len = doc.len_frames();
    let lat = stream.latency() as u64;
    // Read `buf.len()` frames from `from`, zeros past the end of the document.
    let fill = |c: usize, from: u64, buf: &mut [f32]| -> io::Result<()> {
        let got = if from < len { doc.read(c, from, buf)? } else { 0 };
        buf[got..].fill(0.0);
        Ok(())
    };
    if lat > 0 && range.start < range.end && !active.is_empty() {
        // Prime: the first `lat` frames go in and their (empty-window) output is dropped.
        let mut warm: Vec<Vec<f32>> = vec![vec![0.0; lat as usize]; active.len()];
        for (buf, &c) in warm.iter_mut().zip(active) {
            fill(c, range.start, buf)?;
        }
        let mut views: Vec<&mut [f32]> = warm.iter_mut().map(|b| b.as_mut_slice()).collect();
        stream.process(&mut views);
    }
    let active = active.to_vec();
    edit::replace_range(doc, range, channels, label, xf, cancel, move |at, _input, output| {
        for (buf, &c) in output.iter_mut().zip(&active) {
            fill(c, at + lat, buf)?;
        }
        stream.process(output);
        Ok(())
    })
}
