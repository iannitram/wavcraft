//! Delay (f124): a feedback delay from `soundcraft-dsp` (its Mod Delay with the modulation off
//! and the feedback low-pass wide open). Echoes that would fall past the end of the selection
//! are cut there (the effect seam keeps the length). Ping-pong between channels is not in yet.

use std::io;

use super::dsp_run::PluginRun;
use super::{Effect, ParamDesc, Run, RunContext};

pub struct Delay;

impl Effect for Delay {
    fn id(&self) -> &'static str {
        "delay"
    }
    fn name(&self) -> &'static str {
        "Delay"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::time("time", "Delay time", 1.0, 2000.0, 250.0, "ms"),
            ParamDesc::percent("feedback", "Feedback", 0.0, 95.0, 30.0),
            ParamDesc::percent("wet", "Wet (dry is the rest)", 0.0, 100.0, 50.0),
        ];
        P
    }
    fn begin(&self, p: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let settings = [("time", p[0] as f32), ("feedback", p[1] as f32), ("mix", p[2] as f32), ("mod_depth", 0.0), ("lpf", 20000.0)];
        Ok(Some(Box::new(PluginRun::new("mod_delay", ctx, &settings)?)))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Delay runs through Effect::begin"))
    }
}
