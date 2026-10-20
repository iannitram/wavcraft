//! Pitch Shifter and Saturator (f125). Both reuse `soundcraft-dsp`'s realtime plugins (MIT OR
//! Apache-2.0) through [`Effect::begin`], so the plugin's state carries across `replace_range`
//! chunks.

use std::io;
use std::ops::Range;

use soundcraft_dsp::Plugin;
use soundcraft_dsp::plugins::harmonic::Saturator as ScSaturator;
use soundcraft_dsp::plugins::pitch::PitchShifter as ScPitchShifter;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::doc::DocState;

/// Pitch Shifter: semitones and cents, length kept. The plugin delays its output by half its
/// grain window, so the run feeds it that many frames ahead of the chunk and the result lines
/// up with the input.
pub struct PitchShift;

struct PitchRun {
    plugin: ScPitchShifter,
    doc: DocState,
    channels: Vec<usize>,
    end: u64,
    latency: u64,
    buf: Vec<Vec<f32>>,
}

impl PitchRun {
    /// Feed `frames` stream frames starting at `at` (zeros past the range's end); the plugin's
    /// output is left in `self.buf`.
    fn feed(&mut self, at: u64, frames: usize) -> io::Result<()> {
        for (k, ch) in self.channels.iter().enumerate() {
            let b = &mut self.buf[k];
            b.clear();
            b.resize(frames, 0.0);
            let n = (self.end.saturating_sub(at)).min(frames as u64) as usize;
            if n > 0 {
                self.doc.read(*ch, at, &mut b[..n])?;
            }
        }
        self.plugin.process(&mut self.buf, frames);
        Ok(())
    }
}

impl Run for PitchRun {
    fn process(&mut self, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let n = input.first().map_or(0, |c| c.len());
        self.feed(first_frame + self.latency, n)?;
        for (o, b) in output.iter_mut().zip(&self.buf) {
            o.copy_from_slice(&b[..n]);
        }
        Ok(())
    }
}

impl Effect for PitchShift {
    fn id(&self) -> &'static str {
        "pitch_shift"
    }
    fn name(&self) -> &'static str {
        "Pitch Shifter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] =
            &[ParamDesc::number("semitones", "Semitones", -24.0, 24.0, 0.0), ParamDesc::number("cents", "Cents", -100.0, 100.0, 0.0)];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let mut plugin = ScPitchShifter::new();
        plugin.prepare(ctx.sample_rate as f32, 65_536, ctx.channels.len());
        plugin.set_param("semitones", params[0] as f32);
        plugin.set_param("cents", params[1] as f32);
        plugin.reset();
        let latency = plugin.latency() as u64;
        let Range { start, end } = ctx.range;
        let mut run =
            PitchRun { plugin, doc: ctx.doc.clone(), channels: ctx.channels.clone(), end, latency, buf: vec![Vec::new(); ctx.channels.len()] };
        // Prime: the first `latency` stream frames come out as the plugin's own delay.
        run.feed(start, latency as usize)?;
        Ok(Some(Box::new(run)))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Pitch Shifter runs through begin"))
    }
}

/// Saturator / soft clip: drive, curve (soft = tanh, from `soundcraft-dsp`; hard = clip), output
/// gain and dry/wet mix. The output never exceeds the output gain, which acts as its ceiling.
pub struct Saturator;

const CURVES: &[&str] = &["Soft", "Hard"];

struct SatRun {
    soft: Option<ScSaturator>,
    drive: f32,
    mix: f32,
    ceiling: f32,
    buf: Vec<Vec<f32>>,
}

impl Run for SatRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let n = input.first().map_or(0, |c| c.len());
        if let Some(plugin) = self.soft.as_mut() {
            for (b, i) in self.buf.iter_mut().zip(input) {
                b.clear();
                b.extend_from_slice(i);
            }
            plugin.process(&mut self.buf, n);
            for (o, b) in output.iter_mut().zip(&self.buf) {
                for (d, s) in o.iter_mut().zip(b) {
                    *d = s.clamp(-self.ceiling, self.ceiling);
                }
            }
        } else {
            let (g, m, c) = (self.drive, self.mix, self.ceiling);
            for (o, i) in output.iter_mut().zip(input) {
                for (d, s) in o.iter_mut().zip(i.iter()) {
                    let wet = (s * g).clamp(-1.0, 1.0);
                    *d = ((s * (1.0 - m) + wet * m) * c).clamp(-c, c);
                }
            }
        }
        Ok(())
    }
}

impl Effect for Saturator {
    fn id(&self) -> &'static str {
        "saturator"
    }
    fn name(&self) -> &'static str {
        "Saturator"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::db("drive", "Drive", 0.0, 36.0, 6.0),
            ParamDesc::choice("curve", "Curve", CURVES, 0),
            ParamDesc::db("output", "Output", -24.0, 12.0, 0.0),
            ParamDesc::percent("mix", "Mix", 0.0, 100.0, 100.0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let db = |d: f64| 10f64.powf(d / 20.0) as f32;
        let soft = if params[1] == 0.0 {
            let mut p = ScSaturator::new();
            p.prepare(ctx.sample_rate as f32, 65_536, ctx.channels.len());
            p.set_param("drive", params[0] as f32);
            p.set_param("output", params[2] as f32);
            p.set_param("mix", params[3] as f32);
            p.reset();
            Some(p)
        } else {
            None
        };
        Ok(Some(Box::new(SatRun {
            soft,
            drive: db(params[0]),
            mix: params[3] as f32 / 100.0,
            ceiling: db(params[2]),
            buf: vec![Vec::new(); ctx.channels.len()],
        })))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Saturator runs through begin"))
    }
}
