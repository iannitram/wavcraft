//! Gain: scale the level by a number of decibels. The parametrised example effect (f14); it
//! exercises the parameter description and the generic dialog.

use std::io;

use super::{Effect, ParamDesc};

pub struct Gain;

impl Effect for Gain {
    fn id(&self) -> &'static str {
        "gain"
    }
    fn name(&self) -> &'static str {
        "Gain"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[ParamDesc::db("gain_db", "Gain", -60.0, 24.0, 0.0)];
        P
    }
    fn process(&self, params: &[f64], _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let k = 10f64.powf(params[0] / 20.0) as f32;
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = *a * k;
            }
        }
        Ok(())
    }
}
