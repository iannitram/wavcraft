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

mod dynamics;
mod eq;
mod gain;
mod noise_reduction;
pub use dynamics::{Compressor, DeEsser, HardLimiter};
pub use eq::{DcOffset, HighPass, LowPass, Notch, ParametricEq};
pub use gain::Gain;
pub use noise_reduction::NoiseReduction;
mod pitch_saturate;
pub use pitch_saturate::{PitchShift, Saturator};

/// What a setting means, so a dialog can pick the right control and units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A level in decibels.
    Db,
    /// A share, 0 to 100 (`min`/`max` may narrow it).
    Percent,
    /// A duration in `unit` (`ms` or `s`).
    Time,
    /// A frequency in Hz (a dialog may draw it on a log scale).
    Frequency,
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
    pub const fn frequency(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamDesc {
        ParamDesc { id, label, kind: ParamKind::Frequency, min, max, default, unit: "Hz", choices: &[] }
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
    /// Start a run over `ctx`. An effect that needs memory between chunks (a filter), the sample
    /// rate or a measurement of the range (a mean) returns its [`Run`] here; then [`apply`]
    /// calls the run instead of [`Effect::process`]. The default is no run.
    fn begin(&self, _params: &[f64], _ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        Ok(None)
    }
    /// Process one chunk on its own (the stateless path, used when [`Effect::begin`] gives no run).
    /// `first_frame` is the chunk's first frame within the document; `input`
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
    /// A stateful block processor for live use and the rack (f31), or `None` (the default) for an
    /// effect that needs no memory between blocks. Must allocate everything it needs here, not
    /// in `process_block`.
    fn stream(&self, _params: &[f64], _channels: usize, _rate: u32) -> Option<Box<dyn EffectStream>> {
        None
    }
    /// For an effect that keeps state from one chunk to the next (a dynamics envelope, a filter,
    /// a look-ahead window; f28): a processor for one run over a range, at the document's
    /// sample rate and with `channels` active channels. When this returns `Some`, [`apply`] uses
    /// it and never calls [`Effect::process`] (which such an effect may refuse).
    fn run_stream(&self, _params: &[f64], _sample_rate: u32, _channels: usize) -> Option<Box<dyn Stream>> {
        None
    }
}

/// What a stateful effect is told when a run starts (see [`Effect::begin`]).
pub struct RunContext<'a> {
    /// The document being processed (its state before the edit).
    pub doc: &'a DocState,
    /// The frames to process.
    pub range: Range<u64>,
    /// The document channels being processed, in the order the chunks present them.
    pub channels: Vec<usize>,
    pub sample_rate: u32,
}

/// One run of a stateful effect: it sees the chunks of a range in order, so a filter can carry
/// its memory from one 64 K-frame chunk into the next (no click at the join).
pub trait Run {
    /// Same contract as [`Effect::process`]; `first_frame` is within the document.
    fn process(&mut self, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()>;
}

/// An effect running block by block over a live or streamed signal (f31): it keeps whatever
/// state it needs per channel (filter memory, delay lines), so successive blocks join up. Built
/// by [`open_stream`] with all its memory allocated, so [`EffectStream::process_block`] can run
/// on an audio thread.
pub trait EffectStream: Send {
    /// Process one block: `input` and `output` hold one slice per channel, all the same length.
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]);
}

/// The stream of an effect that keeps no state between blocks: each block goes through
/// [`Effect::process`].
struct Stateless {
    effect: &'static dyn Effect,
    params: Vec<f64>,
    frame: u64,
}

impl EffectStream for Stateless {
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        if self.effect.process(&self.params, self.frame, input, output).is_err() {
            for (i, o) in input.iter().zip(output.iter_mut()) {
                o.copy_from_slice(i);
            }
        }
        self.frame += input.first().map_or(0, |c| c.len() as u64);
    }
}

/// An offline [`Stream`] (an effect with `run_stream`, f28) driven block by block for the rack:
/// the block is copied to the output and processed there in place. Its [`Stream::latency`] is not
/// compensated live.
struct RunAdapter {
    run: Box<dyn Stream>,
}

impl EffectStream for RunAdapter {
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        self.run.process(output);
    }
}

/// Open `effect` for streaming over `channels` channels at `rate` Hz with these settings
/// (checked first). An effect with its own state supplies it through [`Effect::stream`]; the
/// others run block by block through [`Effect::process`].
pub fn open_stream(effect: &'static dyn Effect, params: &[f64], channels: usize, rate: u32) -> Result<Box<dyn EffectStream>, String> {
    validate(effect, params)?;
    if let Some(run) = effect.run_stream(params, rate, channels) {
        return Ok(Box::new(RunAdapter { run }));
    }
    Ok(effect.stream(params, channels, rate).unwrap_or_else(|| Box::new(Stateless { effect, params: params.to_vec(), frame: 0 })))
}

/// One run of a stateful effect (see [`Effect::run_stream`]). It sees the range's audio in order,
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
pub static REGISTRY: &[&dyn Effect] = &[
    &Invert,
    &Gain,
    &ParametricEq,
    &HighPass,
    &LowPass,
    &Notch,
    &DcOffset,
    &NoiseReduction,
    &Compressor,
    &HardLimiter,
    &DeEsser,
    &PitchShift,
    &Saturator,
];

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
    let active = channels.active(doc.channel_count());
    let ctx = RunContext { doc, range: range.clone(), channels: active.clone(), sample_rate: rate };
    let mut run = effect.begin(params, &ctx).map_err(EditError::Io)?;
    if run.is_none()
        && let Some(stream) = effect.run_stream(params, rate, active.len())
    {
        return apply_stream(stream, doc, range, channels, &active, effect.name(), xf, cancel);
    }
    let lead = effect.context_frames(params, rate);
    // A stateful run (#135) takes the chunks in order; otherwise a stateless effect that wants no
    // context gets the chunk alone; one that wants context (#139) reads it itself.
    if run.is_some() || lead == 0 {
        return edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, input, output| match run.as_mut() {
            Some(run) => run.process(at, input, output),
            None => effect.process(params, at, input, output),
        });
    }
    // An effect that wants context reads it itself: each chunk plus `lead` frames either side
    // (silence beyond the document), so a spectral effect's output is the same however the
    // range was cut into chunks.
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
