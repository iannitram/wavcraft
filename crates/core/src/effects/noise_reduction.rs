//! Noise Reduction (process), B067: attenuate what looks like the captured noise print, bin by
//! bin, in the shared STFT engine ([`crate::stft`]). Our own design, a gain mask:
//!
//! 1. Per FFT frame and bin, compare the signal's magnitude with the print's (scaled to this FFT
//!    size, [`NoisePrint::magnitudes`]) times a fixed margin ([`MARGIN`]). At or below it the bin
//!    is noise and gets the floor gain (`reduce_db`, scaled by `reduction`); from one to two
//!    times it, a smooth ramp up to unity; above that it is signal and passes untouched.
//! 2. Frequency smoothing: a bin's gain is never below the local average of its neighbours', so
//!    the edges of a partial or tone keep their level and isolated bins do not flicker.
//! 3. Spectral decay: a gain that was high falls back no faster than a set rate, so the tail of
//!    a note is not chopped.
//! 4. Time smoothing: the gain is blended with the previous frame's, against musical-noise
//!    flutter.
//!
//! Steps 3 and 4 carry state from frame to frame; [`WARMUP`] frames before each chunk prime them.
//! Audition's controls are a behaviour reference only: no code or presets from any other
//! program.

use std::io;

use super::{ChunkCtx, Effect, ParamDesc};
use crate::noise;
use crate::stft::{Placement, Stft};

/// A bin counts as noise up to this multiple of the print's mean magnitude. Noise bins are
/// Rayleigh-distributed around the mean, so a margin of 3.5 leaves about 0.01 % of them above it.
const MARGIN: f32 = 3.5;
/// The few bins that pass, and the decay and smoothing that spread them, would leave the measured
/// reduction a hair under the asked one, so noise bins get this much more than `reduce_db`.
const ALLOWANCE_DB: f32 = 0.25;
/// Frames analysed before each chunk to prime the decay and smoothing state.
const WARMUP: usize = 16;
const SIZES: [usize; 4] = [1024, 2048, 4096, 8192];

pub struct NoiseReduction;

const P: &[ParamDesc] = &[
    ParamDesc::db("reduce_db", "Reduce by", 1.0, 100.0, 16.0),
    ParamDesc::percent("reduction", "Noise reduction", 0.0, 100.0, 100.0),
    ParamDesc::percent("smoothing_freq", "Smoothing over frequency", 0.0, 100.0, 20.0),
    ParamDesc::percent("smoothing_time", "Smoothing over time", 0.0, 100.0, 25.0),
    ParamDesc::percent("decay", "Spectral decay rate", 0.0, 100.0, 65.0),
    ParamDesc::choice("fft_size", "FFT size", &["1024", "2048", "4096", "8192"], 2),
    ParamDesc::choice("noise_only", "Output noise only", &["Off", "On"], 0),
];

fn size(params: &[f64]) -> usize {
    SIZES[(params[5] as usize).min(SIZES.len() - 1)]
}

impl Effect for NoiseReduction {
    fn id(&self) -> &'static str {
        "noise_reduction"
    }
    fn name(&self) -> &'static str {
        "Noise Reduction"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        P
    }
    fn context_frames(&self, params: &[f64], _sample_rate: u32) -> usize {
        let n = size(params);
        n + WARMUP * (n / 4)
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Noise Reduction needs the audio around each chunk"))
    }
    fn process_in_context(&self, params: &[f64], ctx: &ChunkCtx, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let bad = |why: String| io::Error::new(io::ErrorKind::InvalidInput, why);
        let Some(print) = noise::current() else {
            return Err(bad("Capture a noise print first: select some noise, then Capture Noise Print (Shift+P)".to_string()));
        };
        if print.sample_rate != ctx.sample_rate {
            return Err(bad(format!("The noise print is {} Hz but this file is {} Hz", print.sample_rate, ctx.sample_rate)));
        }
        let n = size(params);
        let stft = Stft::new(n, 75).map_err(bad)?;
        let bins = stft.bins();
        let floor = 10f32.powf(-((params[0] * params[1] / 100.0) as f32 + ALLOWANCE_DB * (params[1] / 100.0) as f32) / 20.0);
        let width = (params[2] / 100.0 * 16.0).round() as usize;
        let decay = 0.98 * (params[4] / 100.0) as f32;
        let blend = 0.8 * (params[3] / 100.0) as f32;
        let noise_only = params[6] >= 1.0;
        let in_abs = ctx.first_frame as i64 - ctx.lead as i64;
        let out_abs = ctx.first_frame as i64;
        for ((inp, out), &channel) in input.iter().zip(output.iter_mut()).zip(ctx.channels) {
            let threshold: Vec<f32> = print.magnitudes(channel, n).iter().map(|m| (m * MARGIN).max(1e-12)).collect();
            let mut raw = vec![0.0f32; bins];
            let mut held = vec![0.0f32; bins];
            let mut last = vec![0.0f32; bins];
            let mut sums = vec![0.0f32; bins + 1];
            let mut primed = false;
            stft.process(inp, out, Placement { input_abs: in_abs, out_abs, grid: ctx.range_start as i64, warmup: WARMUP }, |spec| {
                for k in 0..bins {
                    let t = (spec[k].norm() / threshold[k] - 1.0).clamp(0.0, 1.0);
                    raw[k] = floor + (1.0 - floor) * t * t * (3.0 - 2.0 * t);
                }
                if width > 0 {
                    for k in 0..bins {
                        sums[k + 1] = sums[k] + raw[k];
                    }
                    for k in 0..bins {
                        let (a, b) = (k.saturating_sub(width), (k + width + 1).min(bins));
                        let mean = (sums[b] - sums[a]) / (b - a) as f32;
                        held[k] = raw[k].max(mean);
                    }
                } else {
                    held.copy_from_slice(&raw);
                }
                for k in 0..bins {
                    let g = if primed { held[k].max(decay * last[k]) } else { held[k] };
                    let g = if primed { blend * last[k] + (1.0 - blend) * g } else { g };
                    last[k] = g;
                    spec[k] *= g;
                }
                primed = true;
            });
            if noise_only {
                let mid = inp.get(ctx.lead..ctx.lead + out.len()).unwrap_or(&[]);
                for (o, i) in out.iter_mut().zip(mid) {
                    *o = *i - *o;
                }
            }
        }
        Ok(())
    }
}
