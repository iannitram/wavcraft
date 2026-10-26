//! Automatic Phase Correction (B076): lines up the two channels of a recording made with a time
//! offset (two microphones, a tape head azimuth error). The lag of the right channel against the
//! left is measured by cross-correlation, and the channel that is early is delayed to match.
//!
//! * `global`: one lag for the whole range, measured in [`Effect::begin`] by reading the document
//!   in blocks (never the whole file at once).
//! * `track`: one lag per analysis window; the applied lag changes by a crossfade over one
//!   window, so there is no click.
//!
//! The lag is a whole number of samples (no sub-sample refinement). The effect is
//! length-preserving: the delayed channel starts with silence and its last frames fall off the
//! end. With `polarity=auto` the right channel is inverted when the correlation peak is negative.
//! Fewer than two active channels: copied through. With more than two, the first two are
//! corrected and the rest are copied through.
//!
//! The run keeps its own frame counter from the start of the range (it never uses `first_frame`),
//! and the last `max_shift` input frames of each channel, so a chunk join changes nothing.

use std::io;
use std::ops::Range;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::doc::DocState;

/// Frames per correlation block (the FFT size is the next power of two above block + max lag).
const BLOCK: usize = 16_384;

pub struct PhaseCorrection;

const MODES: &[&str] = &["global", "track"];
const POLARITY: &[&str] = &["off", "auto"];

/// The lag of `ch[1]` against `ch[0]` over `range`, within +-`max_lag` frames, and whether the
/// correlation at that lag is negative. A positive lag means the right channel is late. Silence
/// gives `(0, false)`.
pub fn measure_lag(doc: &DocState, range: Range<u64>, ch: [usize; 2], max_lag: usize) -> io::Result<(i32, bool)> {
    let len = range.end.saturating_sub(range.start) as usize;
    let max_lag = max_lag.min(len / 2);
    let block = BLOCK.min(len.max(1));
    let size = (block + max_lag).next_power_of_two().max(2);
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mut sum = vec![0.0f64; 2 * max_lag + 1];
    let (mut a, mut b) = (vec![0.0f32; block + max_lag], vec![0.0f32; block + max_lag]);
    let mut fa = vec![Complex32::new(0.0, 0.0); size];
    let mut fb = vec![Complex32::new(0.0, 0.0); size];
    let mut at = range.start;
    while at < range.end {
        let n = block.min((range.end - at) as usize);
        // Left: this block. Right: the block plus max_lag frames of lookahead (so a late right
        // channel is found), zeros past the range.
        let extra = max_lag.min((range.end - at) as usize - n);
        a[..n].fill(0.0);
        let got = doc.read(ch[0], at, &mut a[..n])?;
        a[got..n].fill(0.0);
        let got = doc.read(ch[1], at, &mut b[..n + extra])?;
        b[got..n + extra].fill(0.0);
        for k in 0..size {
            fa[k] = Complex32::new(if k < n { a[k] } else { 0.0 }, 0.0);
            fb[k] = Complex32::new(if k < n + extra { b[k] } else { 0.0 }, 0.0);
        }
        fwd.process(&mut fa);
        fwd.process(&mut fb);
        for k in 0..size {
            fa[k] = fa[k].conj() * fb[k];
        }
        inv.process(&mut fa);
        // c[k] = sum_t left[t] * right[t + k]; negative lags wrap to the top of the array.
        for (i, s) in sum.iter_mut().enumerate() {
            let lag = i as i64 - max_lag as i64;
            let idx = lag.rem_euclid(size as i64) as usize;
            *s += f64::from(fa[idx].re) / size as f64;
        }
        at += n as u64;
    }
    let (best, peak) = sum.iter().enumerate().fold((max_lag, 0.0f64), |m, (i, v)| if v.abs() > m.1.abs() { (i, *v) } else { m });
    if peak.abs() < 1e-12 {
        return Ok((0, false));
    }
    Ok((best as i32 - max_lag as i32, peak < 0.0))
}

#[derive(Clone, Copy)]
struct Setting {
    lag: i32,
    flip: bool,
}

struct PhaseRun {
    settings: Vec<Setting>,
    /// Frames per window (the whole range for global mode).
    window: u64,
    /// Frames of the range processed so far.
    pos: u64,
    /// Last `hist_len` input frames of the first two channels.
    hist: [Vec<f32>; 2],
    hist_len: usize,
}

impl Effect for PhaseCorrection {
    fn id(&self) -> &'static str {
        "phase_correction"
    }
    fn name(&self) -> &'static str {
        "Automatic Phase Correction"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::choice("mode", "Mode", MODES, 0),
            ParamDesc::time("max_shift_ms", "Largest shift", 0.1, 20.0, 5.0, "ms"),
            ParamDesc::time("window_ms", "Analysis window (track)", 10.0, 1000.0, 100.0, "ms"),
            ParamDesc::choice("polarity", "Polarity", POLARITY, 1),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        if ctx.channels.len() < 2 {
            return Ok(None);
        }
        let rate = f64::from(ctx.sample_rate);
        let max_lag = ((params[1] * rate / 1000.0).round() as usize).max(1);
        let auto = params[3] >= 1.0;
        let ch = [ctx.channels[0], ctx.channels[1]];
        let len = ctx.range.end - ctx.range.start;
        let (window, count) = if params[0] >= 1.0 {
            let w = ((params[2] * rate / 1000.0).round() as u64).max(1);
            (w, (len / w).max(1))
        } else {
            (len.max(1), 1)
        };
        let mut settings: Vec<Setting> = Vec::with_capacity(count as usize);
        for w in 0..count {
            let start = ctx.range.start + w * window;
            let end = if w + 1 == count { ctx.range.end } else { start + window };
            let (lag, neg) = measure_lag(ctx.doc, start..end, ch, max_lag)?;
            settings.push(Setting { lag, flip: auto && neg });
        }
        let hist_len = max_lag.min(len as usize);
        Ok(Some(Box::new(PhaseRun { settings, window, pos: 0, hist: [vec![0.0; hist_len], vec![0.0; hist_len]], hist_len })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        Ok(())
    }
}

impl PhaseRun {
    /// Output for `ch` (0 left, 1 right) at chunk index `i` under `s`, from the history+input.
    fn tap(&self, ext: &[f32], i: usize, ch: usize, s: Setting) -> f32 {
        let delay = (if ch == 0 { s.lag.max(0) } else { (-s.lag).max(0) }) as usize;
        let delay = delay.min(self.hist_len);
        let v = ext[self.hist_len + i - delay];
        if ch == 1 && s.flip { -v } else { v }
    }
}

impl Run for PhaseRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let n = input.first().map_or(0, |c| c.len());
        for (i, o) in input.iter().zip(output.iter_mut()).skip(2) {
            o.copy_from_slice(i);
        }
        let ext: Vec<Vec<f32>> = (0..2).map(|c| self.hist[c].iter().chain(input[c].iter()).copied().collect()).collect();
        let last = self.settings.len() - 1;
        for i in 0..n {
            let rel = self.pos + i as u64;
            let w = ((rel / self.window) as usize).min(last);
            let cur = self.settings[w];
            for (c, out) in output.iter_mut().take(2).enumerate() {
                out[i] = if w == 0 {
                    self.tap(&ext[c], i, c, cur)
                } else {
                    let a = ((rel - w as u64 * self.window) as f32 / self.window as f32).min(1.0);
                    (1.0 - a) * self.tap(&ext[c], i, c, self.settings[w - 1]) + a * self.tap(&ext[c], i, c, cur)
                };
            }
        }
        self.pos += n as u64;
        let h = self.hist_len;
        for c in 0..2 {
            self.hist[c] = ext[c][ext[c].len() - h..].to_vec();
        }
        Ok(())
    }
}
