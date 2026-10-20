//! Reverb (f124): algorithmic room reverb from `soundcraft-dsp` (pre-delay, diffusion, an
//! 8-line feedback network with damping). The tail that would ring on past the end of the
//! selection is cut there (the effect seam keeps the length); select past the sound's end
//! (or insert silence first) to keep the whole tail.

use std::io;

use super::dsp_run::PluginRun;
use super::{Effect, ParamDesc, Run, RunContext};

pub struct Reverb;

impl Effect for Reverb {
    fn id(&self) -> &'static str {
        "reverb"
    }
    fn name(&self) -> &'static str {
        "Reverb"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::percent("room_size", "Room size", 0.0, 100.0, 50.0),
            ParamDesc::time("decay", "Decay time", 0.1, 20.0, 1.2, "s"),
            ParamDesc::frequency("damping", "High damping above", 1000.0, 20000.0, 6000.0),
            ParamDesc::time("pre_delay", "Pre-delay", 0.0, 250.0, 10.0, "ms"),
            ParamDesc::percent("wet", "Wet (dry is the rest)", 0.0, 100.0, 25.0),
        ];
        P
    }
    fn begin(&self, p: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let settings = [("size", p[0] as f32), ("decay", p[1] as f32), ("damping", p[2] as f32), ("pre_delay", p[3] as f32), ("mix", p[4] as f32)];
        Ok(Some(Box::new(PluginRun::new("room_reverb", ctx, &settings)?)))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Reverb runs through Effect::begin"))
    }
}
