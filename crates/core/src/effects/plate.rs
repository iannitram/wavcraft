//! Plate (b086): plate reverb from `soundcraft-dsp` (`plate_reverb`), the bright, dense,
//! pre-delayed sibling of [`super::Reverb`]. Same wrapping: it runs through `Effect::begin`, so
//! the tank carries across chunks, and the tail that would ring on past the end of the selection
//! is cut there (the effect seam keeps the length).

use std::io;

use super::dsp_run::PluginRun;
use super::{Effect, ParamDesc, Run, RunContext};

pub struct Plate;

impl Effect for Plate {
    fn id(&self) -> &'static str {
        "plate"
    }
    fn name(&self) -> &'static str {
        "Plate"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::percent("size", "Plate size", 0.0, 100.0, 50.0),
            ParamDesc::time("decay", "Decay time", 0.1, 20.0, 2.5, "s"),
            ParamDesc::frequency("damping", "High damping above", 1000.0, 20000.0, 10000.0),
            ParamDesc::time("pre_delay", "Pre-delay", 0.0, 250.0, 20.0, "ms"),
            ParamDesc::percent("diffusion", "Diffusion", 0.0, 100.0, 85.0),
            ParamDesc::percent("wet", "Wet (dry is the rest)", 0.0, 100.0, 30.0),
        ];
        P
    }
    fn begin(&self, p: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let settings = [
            ("size", p[0] as f32),
            ("decay", p[1] as f32),
            ("damping", p[2] as f32),
            ("pre_delay", p[3] as f32),
            ("diffusion", p[4] as f32),
            ("mix", p[5] as f32),
        ];
        Ok(Some(Box::new(PluginRun::new("plate_reverb", ctx, &settings)?)))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Plate runs through Effect::begin"))
    }
}
