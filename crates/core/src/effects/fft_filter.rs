//! FFT Filter (B054): a frequency response given as up to eight (frequency, gain) points,
//! applied to the spectrum of each STFT frame. The basic spectral filter that repair tools share.
//!
//! Curve rules: point `n` is `fN_hz` / `gN_db`. A point whose frequency is 0 is unused (the
//! default, so a fresh filter is flat). Used points are sorted by frequency; between two of them
//! the gain in dB is interpolated linearly in log-frequency (`scale` = log) or in frequency
//! (`scale` = linear); below the first and above the last, the gain holds that point's value.
//! With no used point the response is 0 dB everywhere.
//!
//! A run reads one FFT length of document audio either side of each chunk (clipped to the
//! processed range, beyond which the range reads as silence) and gives the chunk to the STFT
//! engine on a frame grid fixed to the range start, so chunked output equals a single pass.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};
use crate::doc::DocState;
use crate::stft::{Placement, Stft};

pub struct FftFilter;

const SIZES: [usize; 5] = [1024, 2048, 4096, 8192, 16384];
const POINTS: usize = 8;

const fn params() -> [ParamDesc; 2 + 2 * POINTS] {
    const F: [&str; POINTS] = ["f1_hz", "f2_hz", "f3_hz", "f4_hz", "f5_hz", "f6_hz", "f7_hz", "f8_hz"];
    const G: [&str; POINTS] = ["g1_db", "g2_db", "g3_db", "g4_db", "g5_db", "g6_db", "g7_db", "g8_db"];
    const FL: [&str; POINTS] =
        ["Frequency 1", "Frequency 2", "Frequency 3", "Frequency 4", "Frequency 5", "Frequency 6", "Frequency 7", "Frequency 8"];
    const GL: [&str; POINTS] = ["Gain 1", "Gain 2", "Gain 3", "Gain 4", "Gain 5", "Gain 6", "Gain 7", "Gain 8"];
    let blank = ParamDesc::number("", "", 0.0, 0.0, 0.0);
    let mut out = [blank; 2 + 2 * POINTS];
    out[0] = ParamDesc::choice("fft_size", "FFT size", &["1024", "2048", "4096", "8192", "16384"], 2);
    out[1] = ParamDesc::choice("scale", "Scale", &["log", "linear"], 0);
    let mut i = 0;
    while i < POINTS {
        out[2 + 2 * i] = ParamDesc::frequency(F[i], FL[i], 0.0, 96_000.0, 0.0);
        out[3 + 2 * i] = ParamDesc::db(G[i], GL[i], -120.0, 24.0, 0.0);
        i += 1;
    }
    out
}

static PARAMS: [ParamDesc; 2 + 2 * POINTS] = params();

/// The used points as (Hz, dB), sorted by frequency.
fn curve(p: &[f64]) -> Vec<(f64, f64)> {
    let mut pts: Vec<(f64, f64)> = (0..POINTS).map(|i| (p[2 + 2 * i], p[3 + 2 * i])).filter(|(f, _)| *f > 0.0).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts
}

/// The gain in dB at `hz`.
fn gain_db(pts: &[(f64, f64)], log: bool, hz: f64) -> f64 {
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return 0.0 };
    if hz <= first.0 {
        return first.1;
    }
    if hz >= last.0 {
        return last.1;
    }
    let i = pts.iter().position(|pt| pt.0 > hz).unwrap_or(pts.len() - 1);
    let ((f0, g0), (f1, g1)) = (pts[i - 1], pts[i]);
    let t = if log { (hz / f0).ln() / (f1 / f0).ln() } else { (hz - f0) / (f1 - f0) };
    g0 + (g1 - g0) * t
}

struct FilterRun {
    doc: DocState,
    channels: Vec<usize>,
    range: std::ops::Range<u64>,
    stft: Stft,
    /// The document frame of the next chunk's first frame. Kept here, not taken from `first_frame`.
    next: u64,
    /// Linear gain per bin.
    gains: Vec<f32>,
}

impl Effect for FftFilter {
    fn id(&self) -> &'static str {
        "fft_filter"
    }
    fn name(&self) -> &'static str {
        "FFT Filter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        &PARAMS
    }
    fn begin(&self, p: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        debug_assert_eq!(p.len(), PARAMS.len());
        let stft = Stft::new(SIZES[p[0] as usize], 75).map_err(io::Error::other)?;
        let pts = curve(p);
        let log = p[1] == 0.0;
        let bin_hz = f64::from(ctx.sample_rate) / stft.size() as f64;
        let gains = (0..stft.bins()).map(|b| 10f64.powf(gain_db(&pts, log, b as f64 * bin_hz) / 20.0) as f32).collect();
        Ok(Some(Box::new(FilterRun {
            doc: ctx.doc.clone(),
            channels: ctx.channels.clone(),
            next: ctx.range.start,
            range: ctx.range.clone(),
            stft,
            gains,
        })))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("FFT Filter needs the sample rate: it runs through Effect::begin"))
    }
}

impl Run for FilterRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let first_frame = self.next;
        let n = self.stft.size() as u64;
        let len = input.first().map_or(0, |c| c.len()) as u64;
        let from = first_frame.saturating_sub(n).max(self.range.start);
        let to = (first_frame + len + n).min(self.range.end);
        let mut ctx = vec![0.0f32; (to - from) as usize];
        for (ci, out) in output.iter_mut().enumerate() {
            let got = self.doc.read(self.channels[ci], from, &mut ctx)?;
            if got != ctx.len() {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
            }
            let at = Placement { input_abs: from as i64, out_abs: first_frame as i64, grid: self.range.start as i64, warmup: 0 };
            let gains = &self.gains;
            self.stft.process(&ctx, out, at, |spec| {
                for (s, g) in spec.iter_mut().zip(gains) {
                    *s *= *g;
                }
            });
        }
        self.next += len;
        Ok(())
    }
}
