//! Dynamics (B029): a gate/expander, a compressor and a limiter in one stateful effect, for
//! cleaning up voice.
//!
//! The static curve ([`static_curve`], in dB) has four regions: below `gate_db` the level is
//! expanded by `expand_ratio`; between the gate and `comp_db` it is left alone; above `comp_db`
//! it is compressed by `comp_ratio` through a soft knee `knee_db` wide; and nothing leaves above
//! `limit_db` (a ratio of infinity). `makeup_db` is added before the limiter, so the limiter is the
//! final ceiling whatever the makeup.
//!
//! The detector is a peak follower in the log domain: the level in dB rises and falls through
//! one-pole smoothers (`attack_ms`, `release_ms`), and the gain applied is the curve's output
//! minus its input at that smoothed level. Its state is kept in `f64` and carried across chunks.
//! "Linked" takes the louder channel as the level for every channel; "per channel" gives each
//! channel its own detector.
//!
//! Lookahead: with `lookahead_ms` above zero the audio is delayed by that many frames inside the
//! fixed length of the range while the detector sees the undelayed signal, so the gain is ready
//! when the loud part arrives. The first lookahead frames of the range are therefore silence
//! (the delay line starts empty) and the last lookahead frames of the audio are lost off the end.
//! With a lookahead, a final per-sample ceiling keeps every output sample at or below
//! `limit_db`, whatever the attack time; with none, the limiter is only as fast
//! as the attack.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

const GATE: usize = 0;
const EXPAND: usize = 1;
const COMP: usize = 2;
const RATIO: usize = 3;
const KNEE: usize = 4;
const LIMIT: usize = 5;
const ATTACK: usize = 6;
const RELEASE: usize = 7;
const LOOKAHEAD: usize = 8;
const MAKEUP: usize = 9;
const LINK: usize = 10;

/// The level floor, in dB, for silence.
const FLOOR_DB: f64 = -140.0;

pub struct Dynamics;

/// The curve before the makeup and the limiter.
fn curve_core(p: &[f64], x: f64) -> f64 {
    if x < p[GATE] {
        p[GATE] + (x - p[GATE]) * p[EXPAND]
    } else if p[RATIO] > 1.0 {
        let (t, r, w) = (p[COMP], p[RATIO], p[KNEE]);
        let d = x - t;
        if 2.0 * d < -w {
            x
        } else if w > 0.0 && 2.0 * d.abs() <= w {
            let k = d + w / 2.0;
            x + (1.0 / r - 1.0) * k * k / (2.0 * w)
        } else if d <= 0.0 {
            x
        } else {
            t + d / r
        }
    } else {
        x
    }
}

/// The output level in dB for an input level in dB under `params` (in the order of
/// [`Effect::parameters`]), makeup included. The limiter comes last, so the output never
/// exceeds `limit_db` whatever the makeup. A later curve editor draws this.
pub fn static_curve(params: &[f64], in_db: f64) -> f64 {
    (curve_core(params, in_db) + params[MAKEUP]).min(params[LIMIT])
}

impl Effect for Dynamics {
    fn id(&self) -> &'static str {
        "dynamics"
    }
    fn name(&self) -> &'static str {
        "Dynamics"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::db("gate_db", "Gate threshold", -80.0, 0.0, -60.0),
            ParamDesc::number("expand_ratio", "Expander ratio", 1.0, 20.0, 2.0, ""),
            ParamDesc::db("comp_db", "Compressor threshold", -60.0, 0.0, -20.0),
            ParamDesc::number("comp_ratio", "Compressor ratio", 1.0, 20.0, 3.0, ""),
            ParamDesc::db("knee_db", "Knee", 0.0, 12.0, 6.0),
            ParamDesc::db("limit_db", "Limiter ceiling", -24.0, 0.0, -1.0),
            ParamDesc::time("attack_ms", "Attack", 0.1, 200.0, 5.0, "ms"),
            ParamDesc::time("release_ms", "Release", 5.0, 2000.0, 100.0, "ms"),
            ParamDesc::time("lookahead_ms", "Lookahead", 0.0, 10.0, 0.0, "ms"),
            ParamDesc::db("makeup_db", "Makeup gain", -12.0, 24.0, 0.0),
            ParamDesc::choice("link", "Channels", &["linked", "per channel"], 0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let rate = ctx.sample_rate as f64;
        let n = ctx.channels.len();
        let delay = (params[LOOKAHEAD] * rate / 1000.0).round() as usize;
        let linked = params[LINK] == 0.0;
        Ok(Some(Box::new(DynamicsRun {
            p: params.to_vec(),
            attack: (-1.0 / (params[ATTACK] * rate / 1000.0)).exp(),
            release: (-1.0 / (params[RELEASE] * rate / 1000.0)).exp(),
            linked,
            det: vec![Detector::default(); if linked { 1 } else { n }],
            ring: vec![vec![0.0; delay]; n],
            pos: 0,
            gains: vec![0.0; n],
            heard: vec![0.0; n],
        })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Dynamics needs its run"))
    }
}

#[derive(Clone, Copy, Default)]
struct Detector {
    started: bool,
    /// The release-smoothed peak, dB.
    held: f64,
    /// That, smoothed again for the attack, dB.
    level: f64,
}

impl Detector {
    fn follow(&mut self, x_db: f64, attack: f64, release: f64) -> f64 {
        if !self.started {
            self.started = true;
            self.held = x_db;
            self.level = x_db;
        }
        self.held = x_db.max(release * self.held + (1.0 - release) * x_db);
        self.level = attack * self.level + (1.0 - attack) * self.held;
        self.level
    }
}

struct DynamicsRun {
    p: Vec<f64>,
    attack: f64,
    release: f64,
    linked: bool,
    det: Vec<Detector>,
    /// Per channel, the last `lookahead` input frames (empty when there is no lookahead).
    ring: Vec<Vec<f32>>,
    pos: usize,
    gains: Vec<f64>,
    heard: Vec<f32>,
}

/// The detector's view of a sample: a NaN or infinite sample counts as full scale, so the
/// detector state always stays finite.
fn level_db(x: f32) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    (20.0 * (x.abs() as f64).max(1e-12).log10()).max(FLOOR_DB)
}

impl DynamicsRun {
    /// The gain in dB for a detector level, with the ceiling for `delayed_peak` when there is a
    /// lookahead.
    fn gain_db(&self, level: f64, delayed_peak: Option<f32>) -> f64 {
        let mut g = static_curve(&self.p, level) - level;
        if let Some(d) = delayed_peak.filter(|d| *d > 0.0) {
            g = g.min(self.p[LIMIT] - level_db(d));
        }
        g
    }
}

impl Run for DynamicsRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let n = input.len();
        if n != self.ring.len() {
            return Err(io::Error::other("the channel count changed during a run"));
        }
        let delay = self.ring.first().map_or(0, Vec::len);
        let frames = input.first().map_or(0, |c| c.len());
        for f in 0..frames {
            // what comes out this frame: the input, or the input from `delay` frames ago
            for (c, ch) in input.iter().enumerate() {
                self.heard[c] = if delay > 0 { self.ring[c][self.pos] } else { ch[f] };
            }
            if self.linked {
                let loudest = input.iter().map(|ch| if ch[f].is_finite() { ch[f].abs() } else { 1.0 }).fold(0.0f32, f32::max);
                let level = self.det[0].follow(level_db(loudest), self.attack, self.release);
                let ceiling = (delay > 0).then(|| self.heard.iter().fold(0.0f32, |m, v| m.max(if v.is_finite() { v.abs() } else { 1.0 })));
                let g = self.gain_db(level, ceiling);
                self.gains.iter_mut().for_each(|x| *x = g);
            } else {
                for (c, ch) in input.iter().enumerate() {
                    let level = self.det[c].follow(level_db(ch[f]), self.attack, self.release);
                    let ceiling = (delay > 0).then(|| if self.heard[c].is_finite() { self.heard[c].abs() } else { 1.0 });
                    self.gains[c] = self.gain_db(level, ceiling);
                }
            }
            for c in 0..n {
                output[c][f] = (self.heard[c] as f64 * 10f64.powf(self.gains[c] / 20.0)) as f32;
                if delay > 0 {
                    self.ring[c][self.pos] = input[c][f];
                }
            }
            if delay > 0 {
                self.pos = (self.pos + 1) % delay;
            }
        }
        Ok(())
    }
}
