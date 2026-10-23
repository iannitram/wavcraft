//! A/B compare (B169): two sets of settings for one effect, one of them active. The user flips
//! between A and B to hear which setting is better, copies one set onto the other, and applies
//! the active one. Only the state and the apply live here; the dialog buttons come later.

use std::ops::Range;
use std::sync::atomic::AtomicBool;

use crate::doc::DocState;
use crate::doc::edit::{ChannelMask, Crossfades, Edit, EditError};

use super::{Effect, Params, apply, validate};

/// Which of the two sets of settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

impl Side {
    /// The other side: A for B, B for A.
    pub fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }
}

/// Two sets of settings for one effect (each in the order of [`Effect::parameters`]) and which
/// one is active. A new one is on A.
#[derive(Debug, Clone, PartialEq)]
pub struct AbCompare {
    a: Vec<f64>,
    b: Vec<f64>,
    active: Side,
}

impl AbCompare {
    /// Both sets at the effect's defaults ([`Params::defaults`]), A active.
    pub fn new(effect: &dyn Effect) -> AbCompare {
        let d = Params::defaults(effect);
        AbCompare { a: d.clone(), b: d, active: Side::A }
    }

    /// Both sets equal to `params` (not checked here), A active.
    pub fn from_params(params: &[f64]) -> AbCompare {
        AbCompare { a: params.to_vec(), b: params.to_vec(), active: Side::A }
    }

    /// The active side.
    pub fn active(&self) -> Side {
        self.active
    }

    /// Make `side` the active one (not a toggle: choosing the active side again changes nothing).
    pub fn set_active(&mut self, side: Side) {
        self.active = side;
    }

    /// Flip to the other side and return the side that is now active.
    pub fn toggle(&mut self) -> Side {
        self.active = self.active.other();
        self.active
    }

    /// The settings of `side`.
    pub fn params(&self, side: Side) -> &[f64] {
        match side {
            Side::A => &self.a,
            Side::B => &self.b,
        }
    }

    /// The settings of the active side.
    pub fn active_params(&self) -> &[f64] {
        self.params(self.active)
    }

    /// Store `values` as the settings of `side` if `effect` accepts them ([`validate`]); the
    /// error is validate's text and nothing changes. The active side does not change.
    pub fn set_params(&mut self, effect: &dyn Effect, side: Side, values: &[f64]) -> Result<(), String> {
        validate(effect, values)?;
        if side == Side::A {
            self.a = values.to_vec();
        } else {
            self.b = values.to_vec();
        }
        Ok(())
    }

    /// Copy the settings of `from` onto the other side (A to B for `from` = A). The active side
    /// does not change.
    pub fn copy_to_other(&mut self, from: Side) {
        if from == Side::A {
            self.b = self.a.clone();
        } else {
            self.a = self.b.clone();
        }
    }

    /// Whether A and B hold different settings.
    pub fn differs(&self) -> bool {
        self.a != self.b
    }

    /// Apply `effect` with the active settings over `range` of `doc` (an empty range is the whole
    /// file), through [`apply`]: bad settings come back as its error and change nothing.
    pub fn apply_active(
        &self,
        effect: &dyn Effect,
        doc: &DocState,
        range: Range<u64>,
        channels: ChannelMask,
        xf: Crossfades,
        cancel: &AtomicBool,
    ) -> Result<Edit, EditError> {
        apply(effect, self.active_params(), doc, range, channels, xf, cancel)
    }
}
