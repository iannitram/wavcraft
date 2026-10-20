//! Speech Volume Leveler (B038): evens out loud and quiet talkers in one pass.
//!
//! A stateful effect on the [`Run`] seam. The level is the mean power over all active channels
//! (linked, so the stereo image holds) smoothed by a one-pole filter with a 400 ms time constant
//! and corrected for its start-up, which reads as a ~400 ms sliding window. The wanted gain is
//! `amount * (target - level)` in dB, clamped to the cut and boost limits, and the applied gain
//! chases it with an attack (gain falling) and a release (gain rising) time.
//!
//! Below the gate level the gain stops following the level: a cut is held, a boost eases back to
//! 0 dB at the attack rate, so silence and room noise are never raised. There is no lookahead
//! (the seam is length-preserving), so a sudden loud word is caught after the attack time.
//! The sample position is never read from `first_frame`; the run only counts what it has seen.

use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

/// Detector time constant, seconds.
const DETECTOR_TAU: f64 = 0.4;

pub struct SpeechLeveler;

impl Effect for SpeechLeveler {
    fn id(&self) -> &'static str {
        "speech_leveler"
    }
    fn name(&self) -> &'static str {
        "Speech Volume Leveler"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::db("target_db", "Target level (RMS)", -40.0, 0.0, -18.0),
            ParamDesc::percent("amount", "Amount", 0.0, 100.0, 60.0),
            ParamDesc::db("max_gain_db", "Maximum boost", 0.0, 30.0, 12.0),
            ParamDesc::db("max_cut_db", "Maximum cut", 0.0, 30.0, 12.0),
            ParamDesc::db("gate_db", "Gate level", -80.0, -20.0, -45.0),
            ParamDesc::time("attack_ms", "Attack", 10.0, 2000.0, 300.0, "ms"),
            ParamDesc::time("release_ms", "Release", 50.0, 5000.0, 1000.0, "ms"),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let sr = ctx.sample_rate as f64;
        let coef = |ms: f64| 1.0 - (-1.0 / (ms / 1000.0 * sr)).exp();
        Ok(Some(Box::new(LevelerRun {
            target: params[0],
            amount: params[1] / 100.0,
            max_boost: params[2],
            max_cut: params[3],
            gate: params[4],
            attack: coef(params[5]),
            release: coef(params[6]),
            decay: (-1.0 / (DETECTOR_TAU * sr)).exp(),
            power: 0.0,
            weight: 0.0,
            gain_db: 0.0,
        })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("the Speech Volume Leveler runs through its Run"))
    }
}

struct LevelerRun {
    target: f64,
    amount: f64,
    max_boost: f64,
    max_cut: f64,
    gate: f64,
    attack: f64,
    release: f64,
    decay: f64,
    /// Smoothed mean power, and the weight it has accumulated (1 once warmed up).
    power: f64,
    weight: f64,
    gain_db: f64,
}

impl Run for LevelerRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let frames = input.first().map_or(0, |c| c.len());
        let n = input.len().max(1) as f64;
        for k in 0..frames {
            let p = input.iter().map(|c| (c[k] as f64).powi(2)).sum::<f64>() / n;
            self.power = self.decay * self.power + (1.0 - self.decay) * p;
            self.weight = self.decay * self.weight + (1.0 - self.decay);
            let level_db = 10.0 * (self.power / self.weight).max(1e-12).log10();
            if level_db < self.gate {
                if self.gain_db > 0.0 {
                    self.gain_db = (self.gain_db - self.attack * self.gain_db).max(0.0);
                }
            } else {
                let want = (self.amount * (self.target - level_db)).clamp(-self.max_cut, self.max_boost);
                let coef = if want < self.gain_db { self.attack } else { self.release };
                self.gain_db += coef * (want - self.gain_db);
            }
            let g = if self.gain_db == 0.0 { 1.0 } else { 10f64.powf(self.gain_db / 20.0) as f32 };
            for (i, o) in input.iter().zip(output.iter_mut()) {
                o[k] = i[k] * g;
            }
        }
        Ok(())
    }
}
