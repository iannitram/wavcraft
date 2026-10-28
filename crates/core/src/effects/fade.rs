//! Fade In and Fade Out effects (v1 E1, issue #27): apply a linear, logarithmic or cosine
//! (S-curve) envelope across the selection (or the whole file when nothing is selected),
//! streamed in chunks through [`Effect::begin`] and [`Run`].

use std::io;
use std::ops::Range;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::dsp::fade::{FadeCurve, fade_in_gain, fade_out_gain};

/// Curve choices for [`FadeIn`] and [`FadeOut`], in [`FadeCurve`] order.
pub const CURVE_NAMES: &[&str] = &["Linear", "Logarithmic", "Cosine"];

/// Parameter list for [`FadeIn`] and [`FadeOut`]: `curve` (0 = Linear, 1 = Logarithmic, 2 = Cosine; default Linear).
pub const CURVE_PARAMS: &[ParamDesc] = &[ParamDesc::choice("curve", "Curve", CURVE_NAMES, 0)];

/// Decode the `curve` parameter (`params[0]`, default `0.0`) into a [`FadeCurve`].
pub fn curve_from_param(params: &[f64]) -> FadeCurve {
    match params.first().copied().unwrap_or(0.0) as i64 {
        1 => FadeCurve::Logarithmic,
        2 => FadeCurve::Cosine,
        _ => FadeCurve::Linear,
    }
}

/// Apply one chunk of a fade-in (`fade_out == false`) or fade-out (`fade_out == true`) with `curve`
/// over document frame range `range`, starting at `first_frame`.
pub fn process_fade_chunk(fade_out: bool, curve: FadeCurve, range: Range<u64>, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) {
    let span = range.end.saturating_sub(range.start);
    let denominator = span.saturating_sub(1);
    for (i, o) in input.iter().zip(output.iter_mut()) {
        for (k, (a, b)) in i.iter().zip(o.iter_mut()).enumerate() {
            let frame = first_frame.saturating_add(k as u64);
            let t = if denominator == 0 { 1.0 } else { frame.saturating_sub(range.start) as f64 / denominator as f64 };
            let gain = if fade_out { fade_out_gain(t, curve) } else { fade_in_gain(t, curve) };
            *b = *a * gain as f32;
        }
    }
}

/// One fade run: the curve and the document range, so every chunk knows its normalized position.
struct FadeRun {
    fade_out: bool,
    curve: FadeCurve,
    range: Range<u64>,
}

impl Run for FadeRun {
    fn process(&mut self, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        process_fade_chunk(self.fade_out, self.curve, self.range.clone(), first_frame, input, output);
        Ok(())
    }
}

/// Fade In effect (`effect.fade_in`, `"Fade In"`).
pub struct FadeIn;

/// Fade Out effect (`effect.fade_out`, `"Fade Out"`).
pub struct FadeOut;

impl Effect for FadeIn {
    fn id(&self) -> &'static str {
        "fade_in"
    }

    fn name(&self) -> &'static str {
        "Fade In"
    }

    fn parameters(&self) -> &'static [ParamDesc] {
        CURVE_PARAMS
    }

    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        Ok(Some(Box::new(FadeRun { fade_out: false, curve: curve_from_param(params), range: ctx.range.clone() })))
    }

    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Fade In runs through its Run"))
    }
}

impl Effect for FadeOut {
    fn id(&self) -> &'static str {
        "fade_out"
    }

    fn name(&self) -> &'static str {
        "Fade Out"
    }

    fn parameters(&self) -> &'static [ParamDesc] {
        CURVE_PARAMS
    }

    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        Ok(Some(Box::new(FadeRun { fade_out: true, curve: curve_from_param(params), range: ctx.range.clone() })))
    }

    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Fade Out runs through its Run"))
    }
}
