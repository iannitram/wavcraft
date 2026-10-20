//! DeReverb (B074): make a voice recorded in a live room sound closer by pulling down the part of
//! each spectrum that is probably the room's tail.
//!
//! Method, on the shared STFT engine ([`crate::stft`], Hann window, 75 % overlap). A reverberant
//! tail dies away at a steady rate, so the power in a bin now is mostly a decayed copy of the
//! power a short time ago. For each frame and bin the late-reverb power is estimated as the
//! (three-frame average of the) power one early-reflection delay back, scaled by the decay
//! the assumed reverb time gives over that delay. The share of the current power that this
//! explains, times the amount, is removed: the power gain is `max(floor, 1 - amount * late /
//! total)`. Gains are smoothed lightly across neighbouring bins and over a few frames (a
//! gain may rise at once but falls slowly, so attacks keep their edge).
//!
//! Streaming. The estimator needs only a bounded number of earlier frames, so a chunk is
//! processed by re-analysing that many frames of the document before it (the warm-up) and the
//! audio one FFT length on each side of it, read from the document. Each output sample is then
//! a function of the document alone and not of how the range was cut into chunks. Before the
//! first frame of the range the warm-up reads the audio ahead of the selection when there is
//! some, and silence at the start of the file.

use std::io;

use rustfft::num_complex::Complex32;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::doc::DocState;
use crate::stft::{Placement, Stft};

pub struct DeReverb;

/// How far back the early reflections end and the tail is assumed to begin, in milliseconds.
const EARLY_MS: f64 = 50.0;
/// Most frames over which gains are smoothed in time (at 100 % smoothing).
const MAX_SMOOTH_FRAMES: f64 = 6.0;

const FFT_SIZES: &[&str] = &["1024", "2048", "4096"];

impl Effect for DeReverb {
    fn id(&self) -> &'static str {
        "dereverb"
    }
    fn name(&self) -> &'static str {
        "DeReverb"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::percent("amount", "Amount", 0.0, 100.0, 50.0),
            ParamDesc::time("decay_ms", "Reverb time", 100.0, 5000.0, 600.0, "ms"),
            ParamDesc::db("floor_db", "Maximum reduction", -40.0, 0.0, -15.0),
            ParamDesc::percent("smoothing", "Smoothing", 0.0, 100.0, 50.0),
            ParamDesc::choice("fft_size", "FFT size", FFT_SIZES, 1),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let n: usize = FFT_SIZES[params[4] as usize].parse().map_err(|_| io::Error::other("bad FFT size"))?;
        let stft = Stft::new(n, 75).map_err(io::Error::other)?;
        let hop = stft.hop();
        let rate = f64::from(ctx.sample_rate);
        let delay = ((EARLY_MS / 1000.0 * rate / hop as f64).round() as usize).max(2);
        // Power falls 60 dB over the reverb time; this is the fall over the delay.
        let decay = 10f64.powf(-6.0 * (delay * hop) as f64 / rate / (params[1] / 1000.0)) as f32;
        let floor_amp = 10f64.powf(params[2] / 20.0) as f32;
        let smoothing = (params[3] / 100.0) as f32;
        let smooth_frames = (params[3] / 100.0 * MAX_SMOOTH_FRAMES).round() as usize;
        Ok(Some(Box::new(DeReverbRun {
            doc: ctx.doc.clone(),
            channels: ctx.channels.clone(),
            grid: ctx.range.start,
            pos: ctx.range.start,
            stft,
            amount: (params[0] / 100.0) as f32,
            delay,
            decay,
            floor_pow: floor_amp * floor_amp,
            smoothing,
            smooth_frames,
        })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("DeReverb runs only through a run"))
    }
}

struct DeReverbRun {
    doc: DocState,
    channels: Vec<usize>,
    /// The frame grid origin: the start of the range.
    grid: u64,
    /// The document frame of the next chunk (counted here: the first-frame argument is not used).
    pos: u64,
    stft: Stft,
    amount: f32,
    /// Frames between the current frame and the one the tail estimate is taken from.
    delay: usize,
    /// Power decay over `delay` frames.
    decay: f32,
    floor_pow: f32,
    smoothing: f32,
    smooth_frames: usize,
}

/// What the estimator remembers from earlier frames, for one channel.
struct FrameState {
    /// Power spectra of the latest `delay + 2` frames, oldest first.
    powers: std::collections::VecDeque<Vec<f32>>,
    /// Gains of the latest `smooth_frames + 1` frames, oldest first.
    gains: std::collections::VecDeque<Vec<f32>>,
}

impl DeReverbRun {
    /// Frames re-analysed before a chunk so the estimator state is exact.
    fn warmup(&self) -> usize {
        self.delay + 1 + self.smooth_frames
    }

    fn modify(&self, st: &mut FrameState, spec: &mut [Complex32]) {
        let bins = spec.len();
        let power: Vec<f32> = spec.iter().map(|c| c.norm_sqr()).collect();
        st.powers.push_back(power);
        if st.powers.len() > self.delay + 2 {
            st.powers.pop_front();
        }
        let mut gain = vec![1.0f32; bins];
        if st.powers.len() == self.delay + 2 {
            let cur = &st.powers[self.delay + 1];
            let (a, b, c) = (&st.powers[0], &st.powers[1], &st.powers[2]);
            // Both powers are averaged over three neighbouring bins, so one noisy bin does not
            // decide its own gain.
            let blur = |v: &dyn Fn(usize) -> f32, i: usize| {
                let (lo, hi) = (i.saturating_sub(1), (i + 1).min(bins - 1));
                (v(lo) + v(i) + v(hi)) / 3.0
            };
            for (i, g) in gain.iter_mut().enumerate() {
                let late = self.decay * blur(&|j| (a[j] + b[j] + c[j]) / 3.0, i);
                let total = blur(&|j| cur[j], i);
                let ratio = (late / (total + 1e-20)).min(1.0);
                *g = (1.0 - self.amount * ratio).max(self.floor_pow).sqrt();
            }
        }
        // Light smoothing across neighbouring bins.
        if self.smoothing > 0.0 && bins > 2 {
            let raw = gain.clone();
            for i in 1..bins - 1 {
                let blur = 0.25 * raw[i - 1] + 0.5 * raw[i] + 0.25 * raw[i + 1];
                gain[i] = (1.0 - self.smoothing) * raw[i] + self.smoothing * blur;
            }
        }
        // Over time: rise at once, fall slowly.
        st.gains.push_back(gain.clone());
        if st.gains.len() > self.smooth_frames + 1 {
            st.gains.pop_front();
        }
        let count = st.gains.len() as f32;
        for (i, (g, s)) in gain.iter_mut().zip(spec.iter_mut()).enumerate() {
            let mean = st.gains.iter().map(|f| f[i]).sum::<f32>() / count;
            *g = g.max(mean);
            *s *= *g;
        }
    }
}

impl Run for DeReverbRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let len = input.first().map_or(0, |c| c.len());
        let n = self.stft.size() as i64;
        let hop = self.stft.hop() as i64;
        let warmup = self.warmup();
        // Audio the frames of this chunk and its warm-up touch.
        let from = (self.pos as i64 - n - (warmup as i64 + 1) * hop).max(0);
        let to = (self.pos as i64 + len as i64 + n).min(self.doc.len_frames() as i64).max(from);
        let mut ctx_audio = vec![0.0f32; (to - from) as usize];
        for (slot, o) in self.channels.iter().zip(output.iter_mut()) {
            ctx_audio.iter_mut().for_each(|v| *v = 0.0);
            let got = self.doc.read(*slot, from as u64, &mut ctx_audio)?;
            ctx_audio.truncate(got);
            let place = Placement { input_abs: from, out_abs: self.pos as i64, grid: self.grid as i64, warmup };
            let mut st = FrameState { powers: Default::default(), gains: Default::default() };
            self.stft.process(&ctx_audio, &mut o[..len], place, |spec| self.modify(&mut st, spec));
            ctx_audio.resize((to - from) as usize, 0.0);
        }
        self.pos += len as u64;
        Ok(())
    }
}
