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
    // STUB: card wav-c-27-c fills this.
    let _ = params;
    FadeCurve::Linear
}

/// Apply one chunk of a fade-in (`fade_out == false`) or fade-out (`fade_out == true`) with `curve`
/// over document frame range `range`, starting at `first_frame`.
pub fn process_fade_chunk(fade_out: bool, curve: FadeCurve, range: Range<u64>, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) {
    // STUB: card wav-c-27-c fills this.
    let _ = (fade_out, range, first_frame, input, output, fade_in_gain(0.0, curve), fade_out_gain(0.0, curve));
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

    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        // STUB: card wav-c-27-c fills this.
        let _ = (params, ctx);
        Ok(None)
    }

    fn process(&self, params: &[f64], first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        // STUB: card wav-c-27-c fills this.
        let _ = (params, first_frame, input, output);
        Ok(())
    }
}

impl Effect for FadeOut {
    fn id(&self) -> &'static str {
        "fade_out"
    }

    fn name(&self) -> &'static str {
        "Fade Out"
    }

    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        // STUB: card wav-c-27-c fills this.
        let _ = (params, ctx);
        Ok(None)
    }

    fn process(&self, params: &[f64], first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        // STUB: card wav-c-27-c fills this.
        let _ = (params, first_frame, input, output);
        Ok(())
    }
}
