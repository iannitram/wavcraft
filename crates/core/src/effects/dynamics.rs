//! Dynamics (f28, B037 / B034 / B028): the single-band [`Compressor`], the [`HardLimiter`] and the
//! [`DeEsser`]. All three keep state across the 64 K-frame chunks of a run, so they are
//! [`Stream`]s (see [`Effect::run_stream`]); [`Effect::process`] refuses, because one chunk alone
//! has no sample rate and no envelope.
//!
//! The compressor and the de-esser's multiband mode run SoundCraft's `soundcraft-dsp` plugins
//! (MIT OR Apache-2.0). The limiter is our own: SoundCraft's maximizer has a fixed 1.5 ms
//! look-ahead, and this one's look-ahead, input boost and channel link are settings. Its method
//! is the usual one: a sliding minimum of the gain each sample needs over the look-ahead window,
//! a release, then a moving average over the window, so the gain is already down when a peak
//! comes out of the delay. The ceiling is a **sample** peak: a reconstruction (true peak) may
//! be higher.

use std::collections::VecDeque;
use std::io;

use soundcraft_dsp::biquad::{Biquad, FilterType};
use soundcraft_dsp::{Plugin, db_to_gain, gain_to_db};

use super::{Effect, ParamDesc, Stream};

fn needs_stream() -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "this effect keeps state between chunks and runs only as a stream"))
}

/// Per-sample smoothing coefficient for a time constant in milliseconds.
fn coef(ms: f64, rate: f64) -> f32 {
    let n = ms * 0.001 * rate;
    if n < 1.0e-3 { 0.0 } else { (-1.0 / n).exp() as f32 }
}

/// Run a SoundCraft plugin over in-place slices (it wants one `Vec<f32>` per channel).
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

// ---------------------------------------------------------------------------------------------
// Compressor

pub struct Compressor;

const COMP: &[ParamDesc] = &[
    ParamDesc::db("threshold_db", "Threshold", -60.0, 0.0, -20.0),
    ParamDesc::number("ratio", "Ratio", 1.0, 100.0, 4.0, ":1"),
    ParamDesc::time("attack_ms", "Attack", 0.01, 300.0, 10.0, "ms"),
    ParamDesc::time("release_ms", "Release", 5.0, 3000.0, 100.0, "ms"),
    ParamDesc::db("knee_db", "Knee", 0.0, 24.0, 6.0),
    ParamDesc::db("output_gain_db", "Output gain", -24.0, 24.0, 0.0),
    ParamDesc::choice("auto_makeup", "Auto make-up", &["Off", "On"], 0),
];

/// The make-up gain that restores a full-scale signal to full scale: what a signal at 0 dBFS
/// loses, `(0 - threshold) * (1 - 1/ratio)`, capped at 40 dB.
pub fn auto_makeup_db(threshold_db: f64, ratio: f64) -> f64 {
    (-threshold_db * (1.0 - 1.0 / ratio.max(1.0))).clamp(0.0, 40.0)
}

impl Effect for Compressor {
    fn id(&self) -> &'static str {
        "compressor"
    }
    fn name(&self) -> &'static str {
        "Compressor"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        COMP
    }
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        needs_stream()
    }
    fn run_stream(&self, p: &[f64], rate: u32, channels: usize) -> Option<Box<dyn Stream>> {
        let settings = [("threshold", p[0]), ("ratio", p[1]), ("attack", p[2]), ("release", p[3]), ("knee", p[4]), ("makeup", 0.0)];
        let host = Hosted::new("compressor", rate, channels, &settings)?;
        let makeup = if p[6] >= 0.5 { auto_makeup_db(p[0], p[1]) } else { 0.0 };
        Some(Box::new(CompressorRun { host, gain: db_to_gain((p[5] + makeup) as f32) }))
    }
}

struct CompressorRun {
    host: Hosted,
    gain: f32,
}

impl Stream for CompressorRun {
    fn process(&mut self, io: &mut [&mut [f32]]) {
        self.host.run(io);
        if self.gain != 1.0 {
            for b in io.iter_mut() {
                b.iter_mut().for_each(|x| *x *= self.gain);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Hard limiter

pub struct HardLimiter;

const LIMIT: &[ParamDesc] = &[
    ParamDesc::db("max_amplitude_db", "Maximum amplitude", -60.0, 0.0, -0.1),
    ParamDesc::db("input_boost_db", "Input boost", 0.0, 30.0, 0.0),
    ParamDesc::time("lookahead_ms", "Look-ahead", 0.1, 20.0, 3.0, "ms"),
    ParamDesc::time("release_ms", "Release", 1.0, 1000.0, 50.0, "ms"),
    ParamDesc::choice("link_channels", "Link channels", &["Off", "On"], 1),
];

impl Effect for HardLimiter {
    fn id(&self) -> &'static str {
        "hard_limiter"
    }
    fn name(&self) -> &'static str {
        "Hard Limiter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        LIMIT
    }
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        needs_stream()
    }
    fn run_stream(&self, p: &[f64], rate: u32, channels: usize) -> Option<Box<dyn Stream>> {
        let window = ((p[2] * 0.001 * f64::from(rate)).round() as usize).max(1);
        let ceiling = db_to_gain(p[0] as f32);
        let boost = db_to_gain(p[1] as f32);
        let release = coef(p[3], f64::from(rate));
        let groups = if p[4] >= 0.5 {
            vec![Group::new(channels, window, ceiling, release)]
        } else {
            (0..channels).map(|_| Group::new(1, window, ceiling, release)).collect()
        };
        Some(Box::new(LimiterRun { groups, boost, window }))
    }
}

/// One detector and its gain, shared by `ch` channels (all of them when linked, else one).
struct Group {
    ch: usize,
    window: usize,
    ceiling: f32,
    release: f32,
    /// The delayed input: `window` frames per channel, as a ring.
    delay: Vec<Vec<f32>>,
    pos: usize,
    /// Sliding minimum of the required gain over the last `window` frames: (frame, gain).
    mins: VecDeque<(u64, f32)>,
    /// The gain the window's smoothing ring holds, and its running sum.
    smooth: Vec<f32>,
    sum: f64,
    held: f32,
    n: u64,
}

impl Group {
    fn new(ch: usize, window: usize, ceiling: f32, release: f32) -> Group {
        Group {
            ch,
            window,
            ceiling,
            release,
            delay: vec![vec![0.0; window]; ch],
            pos: 0,
            mins: VecDeque::new(),
            smooth: vec![1.0; window],
            sum: window as f64,
            held: 1.0,
            n: 0,
        }
    }

    /// `bufs` holds this group's `ch` channels, already boosted. Output is the input `window - 1`
    /// frames earlier, limited.
    fn run(&mut self, bufs: &mut [&mut [f32]]) {
        let frames = bufs.first().map_or(0, |b| b.len());
        let w = self.window;
        for i in 0..frames {
            let mut peak = 0.0f32;
            for (b, d) in bufs.iter_mut().zip(self.delay.iter_mut()) {
                let x = if b[i].is_finite() { b[i] } else { 0.0 };
                peak = peak.max(x.abs());
                d[self.pos] = x;
            }
            let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
            // Sliding minimum over frames n-w+1 ..= n.
            while self.mins.back().is_some_and(|&(_, g)| g >= need) {
                self.mins.pop_back();
            }
            self.mins.push_back((self.n, need));
            while self.mins.front().is_some_and(|&(f, _)| f + (w as u64) <= self.n) {
                self.mins.pop_front();
            }
            let hold = self.mins.front().map_or(1.0, |&(_, g)| g);
            // Down at once, back up by the release.
            self.held = if hold < self.held { hold } else { hold + (self.held - hold) * self.release };
            // Moving average of the held gain over the window; the slot at `pos` is the oldest.
            self.sum += f64::from(self.held) - f64::from(self.smooth[self.pos]);
            self.smooth[self.pos] = self.held;
            let g = ((self.sum / w as f64) as f32).clamp(0.0, 1.0);
            self.pos = (self.pos + 1) % w;
            self.n += 1;
            if self.pos == 0 {
                // Cancel the drift of the running sum.
                self.sum = self.smooth.iter().map(|&v| f64::from(v)).sum();
            }
            // `pos` now names the oldest frame in the ring: the one from `w - 1` frames ago
            // (or this one when `w` is 1).
            for (b, d) in bufs.iter_mut().zip(&self.delay) {
                b[i] = (d[self.pos] * g).clamp(-self.ceiling, self.ceiling);
            }
        }
        debug_assert_eq!(bufs.len(), self.ch);
    }
}

struct LimiterRun {
    groups: Vec<Group>,
    boost: f32,
    window: usize,
}

impl Stream for LimiterRun {
    fn latency(&self) -> usize {
        self.window - 1
    }
    fn process(&mut self, io: &mut [&mut [f32]]) {
        if self.boost != 1.0 {
            for b in io.iter_mut() {
                b.iter_mut().for_each(|x| *x *= self.boost);
            }
        }
        if self.groups.len() == 1 {
            self.groups[0].run(io);
        } else {
            for (g, b) in self.groups.iter_mut().zip(io.iter_mut()) {
                g.run(std::slice::from_mut(b));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// De-esser

pub struct DeEsser;

const DEESS: &[ParamDesc] = &[
    ParamDesc::number("frequency_hz", "Frequency", 2000.0, 16000.0, 6000.0, "Hz"),
    ParamDesc::db("threshold_db", "Threshold", -60.0, 0.0, -30.0),
    ParamDesc::db("amount_db", "Amount", 0.0, 40.0, 10.0),
    ParamDesc::choice("mode", "Mode", &["Multiband", "Broadband"], 0),
];

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
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        needs_stream()
    }
    fn run_stream(&self, p: &[f64], rate: u32, channels: usize) -> Option<Box<dyn Stream>> {
        if p[3] < 0.5 {
            // Multiband: only the band above the frequency is turned down (a dynamic shelf).
            let host = Hosted::new("de_esser", rate, channels, &[("freq", p[0]), ("threshold", p[1]), ("range", p[2])])?;
            return Some(Box::new(MultibandRun(host)));
        }
        let r = f64::from(rate);
        let hp =
            (0..channels).map(|_| Biquad::design(FilterType::HighPass, p[0] as f32, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate as f32)).collect();
        Some(Box::new(BroadbandRun { hp, env: 0.0, att: coef(0.5, r), rel: coef(60.0, r), thr: p[1] as f32, amount: p[2] as f32 }))
    }
}

struct MultibandRun(Hosted);

impl Stream for MultibandRun {
    fn process(&mut self, io: &mut [&mut [f32]]) {
        self.0.run(io);
    }
}

/// Sibilance found by a high-pass detector turns the whole signal down.
struct BroadbandRun {
    hp: Vec<Biquad>,
    env: f32,
    att: f32,
    rel: f32,
    thr: f32,
    amount: f32,
}

impl Stream for BroadbandRun {
    fn process(&mut self, io: &mut [&mut [f32]]) {
        let frames = io.first().map_or(0, |b| b.len());
        for i in 0..frames {
            let mut level = 0.0f32;
            for (b, f) in io.iter().zip(self.hp.iter_mut()) {
                level = level.max(f.process_sample(b[i]).abs());
            }
            let c = if level > self.env { self.att } else { self.rel };
            self.env = level + (self.env - level) * c;
            if !self.env.is_finite() {
                self.env = 0.0;
            }
            let gr = (gain_to_db(self.env) - self.thr).clamp(0.0, self.amount);
            let g = db_to_gain(-gr);
            for b in io.iter_mut() {
                b[i] *= g;
            }
        }
    }
}
