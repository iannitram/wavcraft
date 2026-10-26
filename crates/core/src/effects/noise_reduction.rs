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
//! The run reads that context from its own snapshot of the document (see
//! `docs/design/f29-stft-seam.md`).
//! Audition's controls are a behaviour reference only: no code or presets from any other
//! program.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::doc::DocState;
use crate::noise::{self, NoisePrint};
use crate::stft::{Placement, Stft, read_padded};
use std::sync::Arc;

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

/// One run: a snapshot of the document to read each chunk's context from, and the settings.
struct NrRun {
    doc: DocState,
    print: Arc<NoisePrint>,
    channels: Vec<usize>,
    range_start: u64,
    params: Vec<f64>,
    stft: Stft,
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
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let bad = |why: String| io::Error::new(io::ErrorKind::InvalidInput, why);
        let Some(print) = noise::current() else {
            return Err(bad("Capture a noise print first: select some noise, then Capture Noise Print (Shift+P)".to_string()));
        };
        if print.sample_rate != ctx.sample_rate {
            return Err(bad(format!("The noise print is {} Hz but this file is {} Hz", print.sample_rate, ctx.sample_rate)));
        }
        let stft = Stft::new(size(params), 75).map_err(bad)?;
        let run = NrRun { doc: ctx.doc.clone(), print, channels: ctx.channels.clone(), range_start: ctx.range.start, params: params.to_vec(), stft };
        Ok(Some(Box::new(run)))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Noise Reduction needs a run (Effect::begin)"))
    }
}

impl Run for NrRun {
    fn process(&mut self, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let params = &self.params;
        let stft = &self.stft;
        let n = stft.size();
        let bins = stft.bins();
        let floor = 10f32.powf(-((params[0] * params[1] / 100.0) as f32 + ALLOWANCE_DB * (params[1] / 100.0) as f32) / 20.0);
        let width = (params[2] / 100.0 * 16.0).round() as usize;
        let decay = 0.98 * (params[4] / 100.0) as f32;
        let blend = 0.8 * (params[3] / 100.0) as f32;
        let noise_only = params[6] >= 1.0;
        // `first_frame` is already a document frame: `range_start` is only the grid origin.
        let context = n + WARMUP * stft.hop();
        let in_abs = first_frame as i64 - context as i64;
        let out_abs = first_frame as i64;
        for ((out, inp), &channel) in output.iter_mut().zip(input).zip(&self.channels) {
            let audio = read_padded(&self.doc, channel, in_abs, context + out.len() + context)?;
            let threshold: Vec<f32> = self.print.magnitudes(channel, n).iter().map(|m| (m * MARGIN).max(1e-12)).collect();
            let mut raw = vec![0.0f32; bins];
            let mut held = vec![0.0f32; bins];
            let mut last = vec![0.0f32; bins];
            let mut sums = vec![0.0f32; bins + 1];
            let mut primed = false;
            let place = Placement { input_abs: in_abs, out_abs, grid: self.range_start as i64, warmup: WARMUP };
            stft.process(&audio, out, place, |spec| {
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
                for (o, i) in out.iter_mut().zip(inp.iter()) {
                    *o = *i - *o;
                }
            }
        }
        Ok(())
    }
}
