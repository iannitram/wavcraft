//! DeEsser (f28, B028): turns sibilance down. Two modes: **Multiband** turns down only the band
//! above the frequency (SoundCraft's `soundcraft-dsp` de-esser, MIT OR Apache-2.0, a dynamic
//! shelf), **Broadband** turns the whole signal down when a high-pass detector finds sibilance.
//!
//! It keeps state across the 64 K-frame chunks of a run (filter memory and the envelope), so it
//! runs through [`Effect::begin`]; [`Effect::process`] alone refuses, as one chunk has no sample
//! rate and no envelope. [`Effect::stream`] gives the same processor for live use.
//!
//! (f28 also carried a Compressor and a Hard Limiter; the owner's ruling 5 of 2026-10-26 keeps
//! B029's [`Dynamics`](super::dynamics::Dynamics), which already has both, and ports only this.)

use std::io;

use soundcraft_dsp::biquad::{Biquad, FilterType};
use soundcraft_dsp::{Plugin, db_to_gain, gain_to_db};

use super::{Effect, EffectStream, ParamDesc, Run, RunContext};

/// Per-sample smoothing coefficient for a time constant in milliseconds.
fn coef(ms: f64, rate: f64) -> f32 {
    let n = ms * 0.001 * rate;
    if n < 1.0e-3 { 0.0 } else { (-1.0 / n).exp() as f32 }
}

/// A SoundCraft plugin over slices (it wants one `Vec<f32>` per channel).
struct Hosted {
    plugin: Box<dyn Plugin>,
    bufs: Vec<Vec<f32>>,
}

impl Hosted {
    fn new(id: &str, rate: u32, channels: usize, settings: &[(&str, f64)]) -> Option<Hosted> {
        let mut plugin = soundcraft_dsp::create(id)?;
        plugin.prepare(rate as f32, 65_536, channels);
        for (k, v) in settings {
            plugin.set_param(k, *v as f32);
        }
        Some(Hosted { plugin, bufs: vec![Vec::new(); channels] })
    }

    fn run(&mut self, io: &mut [&mut [f32]]) {
        let n = io.first().map_or(0, |b| b.len());
        for (b, src) in self.bufs.iter_mut().zip(io.iter()) {
            b.clear();
            b.extend_from_slice(src);
        }
        self.plugin.process(&mut self.bufs, n);
        for (dst, b) in io.iter_mut().zip(&self.bufs) {
            dst.copy_from_slice(&b[..n]);
        }
    }
}

pub struct DeEsser;

const DEESS: &[ParamDesc] = &[
    ParamDesc::number("frequency_hz", "Frequency", 2000.0, 16000.0, 6000.0, "Hz"),
    ParamDesc::db("threshold_db", "Threshold", -60.0, 0.0, -30.0),
    ParamDesc::db("amount_db", "Amount", 0.0, 40.0, 10.0),
    ParamDesc::choice("mode", "Mode", &["Multiband", "Broadband"], 0),
];

/// One run, either mode: works in place on one slice per active channel.
enum Proc {
    Multiband(Hosted),
    Broadband { hp: Vec<Biquad>, env: f32, att: f32, rel: f32, thr: f32, amount: f32 },
}

impl Proc {
    fn new(p: &[f64], rate: u32, channels: usize) -> Option<Proc> {
        if p[3] < 0.5 {
            let host = Hosted::new("de_esser", rate, channels, &[("freq", p[0]), ("threshold", p[1]), ("range", p[2])])?;
            return Some(Proc::Multiband(host));
        }
        let r = f64::from(rate);
        let hp =
            (0..channels).map(|_| Biquad::design(FilterType::HighPass, p[0] as f32, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate as f32)).collect();
        Some(Proc::Broadband { hp, env: 0.0, att: coef(0.5, r), rel: coef(60.0, r), thr: p[1] as f32, amount: p[2] as f32 })
    }

    fn run(&mut self, io: &mut [&mut [f32]]) {
        match self {
            Proc::Multiband(host) => host.run(io),
            Proc::Broadband { hp, env, att, rel, thr, amount } => {
                let frames = io.first().map_or(0, |b| b.len());
                for i in 0..frames {
                    let mut level = 0.0f32;
                    for (b, f) in io.iter().zip(hp.iter_mut()) {
                        level = level.max(f.process_sample(b[i]).abs());
                    }
                    let c = if level > *env { *att } else { *rel };
                    *env = level + (*env - level) * c;
                    if !env.is_finite() {
                        *env = 0.0;
                    }
                    let gr = (gain_to_db(*env) - *thr).clamp(0.0, *amount);
                    let g = db_to_gain(-gr);
                    for b in io.iter_mut() {
                        b[i] *= g;
                    }
                }
            }
        }
    }

    /// Copy `input` into `output`, then process `output` in place.
    fn run_into(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        for (o, i) in output.iter_mut().zip(input) {
            o.copy_from_slice(i);
        }
        self.run(output);
    }
}

impl Run for Proc {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        self.run_into(input, output);
        Ok(())
    }
}

impl EffectStream for Proc {
    fn process_block(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        self.run_into(input, output);
    }
}

fn unavailable() -> io::Error {
    io::Error::other("the de-esser plugin is not available in this build")
}

impl Effect for DeEsser {
    fn id(&self) -> &'static str {
        "deesser"
    }
    fn name(&self) -> &'static str {
        "DeEsser"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        DEESS
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let run = Proc::new(params, ctx.sample_rate, ctx.channels.len()).ok_or_else(unavailable)?;
        Ok(Some(Box::new(run)))
    }
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "the de-esser keeps state between chunks and runs only through begin"))
    }
    fn stream(&self, params: &[f64], channels: usize, rate: u32) -> Option<Box<dyn EffectStream>> {
        Some(Box::new(Proc::new(params, rate, channels)?))
    }
}
