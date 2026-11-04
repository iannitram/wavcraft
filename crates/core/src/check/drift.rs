//! Loudness drift: stretches where the short-term (3 s) loudness stays more than
//! [`Config::drift_lu`] from the integrated loudness of the whole file for at least
//! [`Config::drift_min_s`] seconds.

use super::{Config, Finding, Kind, Severity, Sink};
use crate::loudness::BlockMeter;
use crate::spectrogram::Samples;

/// Sub-blocks per 100 ms block, for placing the edges of a stretch more finely than a block.
const SUBS: usize = 10;
/// Sub-blocks either side of a candidate edge that are averaged to score it (0.5 s).
const EDGE_SUBS: usize = 50;

pub(super) fn drift_pass(channels: &[&dyn Samples], len: u64, cfg: &Config, sink: &mut Sink) {
    let mut meter = BlockMeter::new(cfg.sample_rate, channels.len());
    let block = meter.block();
    let sub = (block / SUBS).max(1);
    let mut planar: Vec<Vec<f32>> = vec![vec![0.0; block]; channels.len().max(1)];
    // Mean square over the channels of each sub-block (for the edges), and each short-term reading
    // with the frame its block ends at.
    let mut power: Vec<f64> = Vec::new();
    let mut readings: Vec<(u64, f64)> = Vec::new();
    let mut at = 0u64;
    while at < len {
        let want = (len - at).min(block as u64) as usize;
        for (ch, src) in planar.iter_mut().zip(channels) {
            ch.resize(want, 0.0);
            src.read(at as usize, ch);
        }
        for s in 0..want.div_ceil(sub) {
            let (lo, hi) = (s * sub, ((s + 1) * sub).min(want));
            let sum: f64 = planar.iter().map(|ch| ch[lo..hi].iter().map(|&x| f64::from(x).powi(2)).sum::<f64>()).sum();
            power.push(sum / (hi - lo) as f64);
        }
        at += want as u64;
        if let Some(st) = meter.process(&planar, want) {
            readings.push((at, f64::from(st)));
        }
    }
    let integrated = f64::from(meter.meter.integrated_lufs());
    if !integrated.is_finite() || integrated <= -143.0 {
        return;
    }
    let mut prefix = vec![0f64; power.len() + 1];
    for (i, p) in power.iter().enumerate() {
        prefix[i + 1] = prefix[i] + p;
    }
    let ctx = Edges { prefix: &prefix, sub: sub as u64, window: 3 * u64::from(cfg.sample_rate) };
    let min_frames = u64::from(cfg.drift_min_s) * u64::from(cfg.sample_rate);

    // A run: (first reading's end frame, last reading's end frame, direction, largest deviation).
    let mut run: Option<(u64, u64, f64, f64)> = None;
    for i in 0..=readings.len() {
        let dev = readings.get(i).filter(|r| r.1 > -143.0).map(|r| r.1 - integrated).filter(|d| d.abs() > cfg.drift_lu);
        let end = readings.get(i).map_or(0, |r| r.0);
        match (run, dev) {
            (Some((a, _, dir, top)), Some(d)) if d.signum() == dir => run = Some((a, end, dir, top.max(d.abs()))),
            (open, d) => {
                if let Some((a, b, dir, top)) = open.filter(|&(a, b, ..)| b - a + block as u64 >= min_frames) {
                    // A stretch that touches either end of the file has no edge there to find.
                    let start = if Some(a) == readings.first().map(|r| r.0) { 0 } else { ctx.edge(a, dir, len) };
                    let stop = if i == readings.len() { len } else { ctx.edge(b, -dir, len) };
                    if stop > start {
                        sink.push(Finding { kind: Kind::LoudnessDrift, severity: Severity::Warning, channel: None, start, end: stop, value: top });
                    }
                }
                run = d.map(|d| (end, end, d.signum(), d.abs()));
            }
        }
    }
}

struct Edges<'a> {
    prefix: &'a [f64],
    sub: u64,
    /// Frames of one short-term window.
    window: u64,
}

impl Edges<'_> {
    /// Mean power of sub-blocks `lo..hi` (clamped to what exists), in dB.
    fn level(&self, lo: i64, hi: i64) -> f64 {
        let n = self.prefix.len() as i64 - 1;
        let (lo, hi) = (lo.clamp(0, n) as usize, hi.clamp(0, n) as usize);
        if hi <= lo {
            return -120.0;
        }
        10.0 * ((self.prefix[hi] - self.prefix[lo]) / (hi - lo) as f64).max(1e-12).log10()
    }

    /// The frame, within one short-term window before `reading_end`, where the power steps most
    /// sharply in direction `dir` (+1 up, -1 down): the edge of the stretch the reading caught.
    fn edge(&self, reading_end: u64, dir: f64, len: u64) -> u64 {
        let hi = (reading_end / self.sub) as i64;
        let lo = hi - (self.window / self.sub) as i64;
        let w = EDGE_SUBS as i64;
        let mut best = (f64::NEG_INFINITY, hi);
        for j in lo.max(1)..=hi {
            let score = dir * (self.level(j, j + w) - self.level(j - w, j));
            if score > best.0 {
                best = (score, j);
            }
        }
        (best.1 as u64 * self.sub).min(len)
    }
}
