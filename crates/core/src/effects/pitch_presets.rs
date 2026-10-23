//! Pitch presets (B111): a table of named shifts (octave up and down, fifth, and so on) for the
//! Pitch Shifter ([`PitchShift`]). Each preset is a `semitones` and `cents` pair; converting one
//! gives the effect's parameter values in its own order, checked against the effect's ranges.
//! No formant control: the Pitch Shifter has none.

use std::fmt;

use super::{Effect, PitchShift};

/// One named shift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchPreset {
    /// The name shown to the user and looked up by [`lookup`].
    pub name: &'static str,
    /// Whole or fractional semitones (the effect's `semitones` setting).
    pub semitones: f64,
    /// Cents on top of the semitones (the effect's `cents` setting).
    pub cents: f64,
}

/// Why a preset could not be found or used.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetError {
    /// No preset has this name (the text as typed).
    UnknownName(String),
    /// A value is not finite or lies outside the effect's range for `param`.
    OutOfRange { param: &'static str, value: f64, min: f64, max: f64 },
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PresetError::UnknownName(name) => write!(f, "no pitch preset named {name:?}"),
            PresetError::OutOfRange { param, value, min, max } => {
                write!(f, "pitch {param} {value} is outside {min} to {max}")
            }
        }
    }
}

impl std::error::Error for PresetError {}

const fn preset(name: &'static str, semitones: f64, cents: f64) -> PitchPreset {
    PitchPreset { name, semitones, cents }
}

/// The table, in menu order.
const PRESETS: &[PitchPreset] = &[
    preset("Octave Up", 12.0, 0.0),
    preset("Octave Down", -12.0, 0.0),
    preset("Two Octaves Up", 24.0, 0.0),
    preset("Two Octaves Down", -24.0, 0.0),
    preset("Fifth Up", 7.0, 0.0),
    preset("Fifth Down", -7.0, 0.0),
    preset("Fourth Up", 5.0, 0.0),
    preset("Fourth Down", -5.0, 0.0),
    preset("Major Third Up", 4.0, 0.0),
    preset("Minor Third Down", -3.0, 0.0),
    preset("Semitone Up", 1.0, 0.0),
    preset("Semitone Down", -1.0, 0.0),
    preset("Quarter Tone Up", 0.0, 50.0),
    preset("Quarter Tone Down", 0.0, -50.0),
    preset("A440 to A432", 0.0, -31.77),
];

/// Every preset, in menu order.
pub fn presets() -> &'static [PitchPreset] {
    PRESETS
}

/// The preset names, in menu order.
pub fn names() -> Vec<&'static str> {
    presets().iter().map(|p| p.name).collect()
}

/// The preset called `name`: surrounding spaces ignored, any letter case.
pub fn lookup(name: &str) -> Result<&'static PitchPreset, PresetError> {
    let wanted = name.trim();
    presets().iter().find(|p| p.name.eq_ignore_ascii_case(wanted)).ok_or(PresetError::UnknownName(name.to_string()))
}

/// Whether `semitones` and `cents` are finite and inside the Pitch Shifter's ranges (read from
/// [`PitchShift::parameters`], so they follow the effect).
pub fn check_shift(semitones: f64, cents: f64) -> Result<(), PresetError> {
    for (d, v) in PitchShift.parameters().iter().zip([semitones, cents]) {
        if !v.is_finite() || v < d.min || v > d.max {
            return Err(PresetError::OutOfRange { param: d.id, value: v, min: d.min, max: d.max });
        }
    }
    Ok(())
}

/// The Pitch Shifter's parameter values for the preset called `name`, in the effect's order
/// (`semitones`, `cents`): [`lookup`] then [`PitchPreset::to_params`].
pub fn params_for(name: &str) -> Result<Vec<f64>, PresetError> {
    lookup(name)?.to_params()
}

impl PitchPreset {
    /// The Pitch Shifter's parameter values `[semitones, cents]`, after [`check_shift`].
    pub fn to_params(&self) -> Result<Vec<f64>, PresetError> {
        check_shift(self.semitones, self.cents)?;
        Ok(vec![self.semitones, self.cents])
    }

    /// The frequency ratio of the shift: 2 to the power of (semitones + cents / 100) / 12.
    pub fn ratio(&self) -> f64 {
        2f64.powf((self.semitones + self.cents / 100.0) / 12.0)
    }
}
