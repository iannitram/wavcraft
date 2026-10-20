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

/// One adjustable setting, described for a dialog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDesc {
    /// Stable key (`gain_db`).
    pub id: &'static str,
    /// Text for the dialog.
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// `dB`, `ms`, `Hz`, or empty.
    pub unit: &'static str,
}

/// The settings for one run, in the order of [`Effect::parameters`].
pub struct Params;

impl Params {
    /// Default settings for `effect`.
    pub fn defaults(effect: &dyn Effect) -> Vec<f64> {
        effect.parameters().iter().map(|p| p.default).collect()
    }
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
pub static REGISTRY: &[&dyn Effect] = &[&Invert];

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
    let range = if range.start < range.end { range } else { 0..doc.len_frames() };
    edit::replace_range(doc, range.clone(), channels, effect.name(), xf, cancel, |at, input, output| {
        effect.process(params, range.start + at, input, output)
    })
}
