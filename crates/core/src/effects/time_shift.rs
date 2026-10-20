//! Time Shift: delay the processed range by a set time. Length-preserving, as the effect seam
//! is: inside the range, output frame f is input frame f - shift, the first `shift` frames are
//! silent and the last `shift` input frames are pushed out and lost (a delay insert in a fixed
//! range). Only the channels the mask selects move.
//!
//! The run keeps one ring buffer per processed channel holding the last `shift` input frames,
//! which carries the delay across 64 K-frame chunks (a shift may be longer than a chunk). The
//! shift is capped at [`MAX_SHIFT_MS`] (10 s), so a run never holds more than
//! 10 s x sample rate x channels floats.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

/// The longest shift, in milliseconds (also the parameter's upper bound).
pub const MAX_SHIFT_MS: f64 = 10_000.0;

pub struct TimeShift;

struct ShiftRun {
    /// One ring per channel, each `shift` frames long (empty for a shift of 0).
    rings: Vec<Vec<f32>>,
    /// Next ring slot to read and overwrite, the same for every channel.
    pos: usize,
}

impl Effect for TimeShift {
    fn id(&self) -> &'static str {
        "time_shift"
    }
    fn name(&self) -> &'static str {
        "Time Shift"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[ParamDesc::time("shift_ms", "Shift", 0.0, MAX_SHIFT_MS, 0.0, "ms")];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let ms = params[0].clamp(0.0, MAX_SHIFT_MS);
        let frames = (ms * f64::from(ctx.sample_rate) / 1000.0).round() as usize;
        Ok(Some(Box::new(ShiftRun { rings: vec![vec![0.0; frames]; ctx.channels.len()], pos: 0 })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Time Shift runs through its Run"))
    }
}

impl Run for ShiftRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let n = input.first().map_or(0, |c| c.len());
        let len = self.rings.first().map_or(0, Vec::len);
        for ((i, o), ring) in input.iter().zip(output.iter_mut()).zip(self.rings.iter_mut()) {
            if len == 0 {
                o.copy_from_slice(i);
                continue;
            }
            let mut pos = self.pos;
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = ring[pos];
                ring[pos] = *a;
                pos += 1;
                if pos == len {
                    pos = 0;
                }
            }
        }
        if len > 0 {
            self.pos = (self.pos + n) % len;
        }
        Ok(())
    }
}
