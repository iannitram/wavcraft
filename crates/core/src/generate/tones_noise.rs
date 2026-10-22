//! Generators (f21, roadmap A303/A304): Generate › Tones and Generate › Noise. A [`Generator`]
//! describes its settings with the same [`ParamDesc`] list effects use (so the effect dialog
//! draws it) and makes one [`Source`] per channel; [`crate::actions::Editor::generate`] streams
//! the sources through [`crate::doc::edit::generate`] in chunks, so nothing is generated whole
//! in RAM. [`render`] makes a short buffer for the dialog's Preview.
//!
//! Levels: `amp` is the peak ceiling in dBFS. A tone's components are scaled so their sum never
//! passes it; noise is scaled to a fixed share of full scale and then limited to it, so white
//! noise sits at the ceiling and pink and brown a little below it.
//!
//! To add a generator: implement [`Generator`] and add it to [`REGISTRY`].

use std::f64::consts::TAU;

use crate::effects::ParamDesc;

/// A signal made one chunk at a time, in order.
pub trait Source: Send {
    /// Fill `out` with the next `out.len()` samples.
    fn fill(&mut self, out: &mut [f32]);
}

/// A named signal generator with settings.
pub trait Generator: Sync {
    /// Stable lowercase key (`tone`), the name `GENERATE` takes.
    fn id(&self) -> &'static str;
    /// Menu text and the undo step's name.
    fn name(&self) -> &'static str;
    /// The settings, which include a `dur` (seconds).
    fn parameters(&self) -> &'static [ParamDesc];
    /// The source for one channel. `seed` makes noise repeatable; each channel mixes in its
    /// own index so channels differ.
    fn source(&self, params: &[f64], sample_rate: u32, channel: usize, seed: u64) -> Box<dyn Source>;
    /// The length in seconds (the `dur` setting).
    fn duration(&self, params: &[f64]) -> f64 {
        self.parameters().iter().position(|p| p.id == "dur").and_then(|i| params.get(i)).copied().unwrap_or(0.0)
    }
}

/// Every generator, in Generate-menu order.
pub static REGISTRY: &[&dyn Generator] = &[&Tones, &Noise];

pub fn all() -> &'static [&'static dyn Generator] {
    REGISTRY
}

/// The generator with this [`Generator::id`].
pub fn find(id: &str) -> Option<&'static dyn Generator> {
    REGISTRY.iter().copied().find(|g| g.id() == id)
}

/// Default settings for `g`.
pub fn defaults(g: &dyn Generator) -> Vec<f64> {
    g.parameters().iter().map(|p| p.default).collect()
}

/// Check `params` for `g` (count and ranges).
pub fn validate(g: &dyn Generator, params: &[f64]) -> Result<(), String> {
    crate::effects::validate_list(g.name(), g.parameters(), params)
}

/// Settings from `key=value` text over the defaults (see [`crate::effects::parse_params`]).
pub fn parse_params(g: &dyn Generator, args: &str) -> Result<Vec<f64>, String> {
    crate::effects::parse_list(g.name(), g.parameters(), args)
}

/// `n` frames per channel of `g`, in memory: only for short previews.
pub fn render(g: &dyn Generator, params: &[f64], sample_rate: u32, channels: usize, frames: usize, seed: u64) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| {
            let mut out = vec![0.0f32; frames];
            g.source(params, sample_rate, c, seed).fill(&mut out);
            out
        })
        .collect()
}

/// `10^(db/20)`.
fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

// ---------------------------------------------------------------------------------------------
// Tones

/// Waveform names, in the order of the `wave` choice.
pub const WAVES: &[&str] = &["Sine", "Triangle", "Square", "Sawtooth"];

/// Generate › Tones: one waveform at a base frequency, with up to four more components at whole
/// multiples of it (harmonics 2 to 5), each a share of the base's level.
pub struct Tones;

impl Generator for Tones {
    fn id(&self) -> &'static str {
        "tone"
    }
    fn name(&self) -> &'static str {
        "Generate Tones"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::number("freq", "Base frequency", 20.0, 20000.0, 440.0, "Hz"),
            ParamDesc::choice("wave", "Waveform", WAVES, 0),
            ParamDesc::db("amp", "Amplitude", -96.0, 0.0, -6.0),
            ParamDesc::time("dur", "Duration", 0.01, 3600.0, 1.0, "s"),
            ParamDesc::percent("h2", "2nd harmonic", 0.0, 100.0, 0.0),
            ParamDesc::percent("h3", "3rd harmonic", 0.0, 100.0, 0.0),
            ParamDesc::percent("h4", "4th harmonic", 0.0, 100.0, 0.0),
            ParamDesc::percent("h5", "5th harmonic", 0.0, 100.0, 0.0),
        ];
        P
    }
    fn source(&self, params: &[f64], sample_rate: u32, _channel: usize, _seed: u64) -> Box<dyn Source> {
        let rate = f64::from(sample_rate.max(1));
        let wave = params[1] as u8;
        let mut parts = vec![(params[0], 1.0)];
        for (k, share) in params[4..8].iter().enumerate() {
            let f = params[0] * (k + 2) as f64;
            // A component at or past the Nyquist frequency would fold back; leave it out.
            if *share > 0.0 && f < rate / 2.0 {
                parts.push((f, share / 100.0));
            }
        }
        let total: f64 = parts.iter().map(|(_, a)| a).sum();
        let gain = db_to_gain(params[2]) / total;
        Box::new(ToneSource { parts: parts.into_iter().map(|(f, a)| Part { step: f / rate, phase: 0.0, amp: a * gain }).collect(), wave })
    }
}

struct Part {
    /// Cycles a sample.
    step: f64,
    /// 0 to 1.
    phase: f64,
    amp: f64,
}

struct ToneSource {
    parts: Vec<Part>,
    wave: u8,
}

/// One cycle of `wave` at `phase` (0 to 1), from -1 to 1.
fn shape(wave: u8, phase: f64) -> f64 {
    match wave {
        0 => (phase * TAU).sin(),
        1 => 1.0 - 4.0 * (phase - 0.5).abs(),
        2 => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 2.0 * phase - 1.0,
    }
}

impl Source for ToneSource {
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let wave = self.wave;
        for p in &mut self.parts {
            for o in out.iter_mut() {
                *o += (shape(wave, p.phase) * p.amp) as f32;
                p.phase += p.step;
                p.phase -= p.phase.floor();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Noise

/// Noise colours, in the order of the `color` choice.
pub const COLORS: &[&str] = &["White", "Pink", "Brown"];

/// Generate › Noise: white, pink (about -3 dB an octave) or brown (about -6 dB an octave).
pub struct Noise;

impl Generator for Noise {
    fn id(&self) -> &'static str {
        "noise"
    }
    fn name(&self) -> &'static str {
        "Generate Noise"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::choice("color", "Color", COLORS, 0),
            ParamDesc::db("amp", "Amplitude", -96.0, 0.0, -12.0),
            ParamDesc::time("dur", "Duration", 0.01, 3600.0, 1.0, "s"),
        ];
        P
    }
    fn source(&self, params: &[f64], _sample_rate: u32, channel: usize, seed: u64) -> Box<dyn Source> {
        Box::new(NoiseSource {
            rng: XorShift::new(seed ^ (channel as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            color: params[0] as u8,
            ceiling: db_to_gain(params[1]),
            pink: [0.0; 3],
            brown: 0.0,
        })
    }
}

/// A tiny xorshift64* generator: noise needs speed and repeatability, not secrecy.
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        // The state may not be 0.
        XorShift(if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed })
    }
    /// The next value, uniform from -1 to 1.
    pub fn next_signed(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

struct NoiseSource {
    rng: XorShift,
    color: u8,
    ceiling: f64,
    /// Paul Kellet's three-pole pink filter.
    pink: [f64; 3],
    brown: f64,
}

/// Gains that put the colours at a similar loudness, found by measuring (see the tests).
const PINK_GAIN: f64 = 0.25;
const BROWN_GAIN: f64 = 3.5;

impl Source for NoiseSource {
    fn fill(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            let w = self.rng.next_signed();
            let v = match self.color {
                0 => w,
                1 => {
                    self.pink[0] = 0.99765 * self.pink[0] + w * 0.0990460;
                    self.pink[1] = 0.96300 * self.pink[1] + w * 0.2965164;
                    self.pink[2] = 0.57000 * self.pink[2] + w * 1.0526913;
                    (self.pink[0] + self.pink[1] + self.pink[2] + w * 0.1848) * PINK_GAIN
                }
                _ => {
                    // A leaky integrator: brown noise without drifting away.
                    self.brown = (self.brown + 0.02 * w) / 1.02;
                    self.brown * BROWN_GAIN
                }
            };
            *o = (v.clamp(-1.0, 1.0) * self.ceiling) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(params: &[f64], rate: u32, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        Tones.source(params, rate, 0, 1).fill(&mut out);
        out
    }

    fn peak(x: &[f32]) -> f64 {
        x.iter().fold(0.0f64, |m, v| m.max(f64::from(v.abs())))
    }
    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }
    fn noise(color: f64, amp: f64, seed: u64, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        Noise.source(&[color, amp, 1.0], 48000, 0, seed).fill(&mut out);
        out
    }

    #[test]
    fn defaults_validate_and_parse() {
        for g in all() {
            assert!(validate(*g, &defaults(*g)).is_ok(), "{}", g.id());
        }
        let p = parse_params(&Tones, "freq=1000 wave=square amp=-12 dur=2").unwrap();
        assert_eq!(&p[..4], &[1000.0, 2.0, -12.0, 2.0]);
        assert!(parse_params(&Tones, "freq=5").is_err());
        assert!(parse_params(&Noise, "color=blue").is_err());
    }

    #[test]
    fn a_sine_has_the_asked_peak_and_period() {
        let x = tone(&[480.0, 0.0, -6.0, 1.0, 0.0, 0.0, 0.0, 0.0], 48000, 4800);
        let want = db_to_gain(-6.0);
        assert!((peak(&x) - want).abs() < 1e-3, "peak {}", peak(&x));
        // 480 Hz at 48 kHz: 100 samples a cycle, so the signal repeats exactly.
        for i in 0..4000 {
            assert!((x[i] - x[i + 100]).abs() < 1e-4);
        }
        assert!(x[0].abs() < 1e-6, "starts at zero");
        assert!((rms(&x) - want / 2f64.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn chunked_fill_equals_one_fill() {
        let p = [441.0, 1.0, -3.0, 1.0, 50.0, 0.0, 25.0, 0.0];
        let whole = tone(&p, 44100, 10_000);
        let mut s = Tones.source(&p, 44100, 0, 0);
        let mut parts = vec![0.0f32; 10_000];
        for c in parts.chunks_mut(777) {
            s.fill(c);
        }
        assert_eq!(whole, parts);
    }

    #[test]
    fn harmonics_stay_under_the_ceiling_and_wave_shapes_differ() {
        for wave in 0..4 {
            let x = tone(&[200.0, f64::from(wave), -6.0, 1.0, 100.0, 100.0, 100.0, 100.0], 48000, 48000);
            assert!(peak(&x) <= db_to_gain(-6.0) + 1e-6, "wave {wave} peak {}", peak(&x));
        }
        let sq = tone(&[100.0, 2.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 48000, 480);
        assert!(sq.iter().all(|v| (v.abs() - 1.0).abs() < 1e-6));
        let saw = tone(&[100.0, 3.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 48000, 960);
        assert!(saw[0] < -0.99 && saw[1] > saw[0] && saw[479] > 0.99 && saw[481] < -0.98, "a rising ramp, 480 samples a cycle");
        let tri = tone(&[100.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 48000, 480);
        assert!((peak(&tri) - 1.0).abs() < 1e-3 && tri[0] < -0.99);
    }

    #[test]
    fn a_harmonic_past_nyquist_is_left_out() {
        let a = tone(&[10_000.0, 0.0, 0.0, 1.0, 0.0, 100.0, 0.0, 0.0], 44100, 1000);
        let b = tone(&[10_000.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 44100, 1000);
        assert_eq!(a, b, "the 3rd harmonic (30 kHz) would fold back");
    }

    #[test]
    fn noise_is_repeatable_by_seed_and_differs_by_channel() {
        assert_eq!(noise(0.0, -6.0, 7, 1000), noise(0.0, -6.0, 7, 1000));
        assert_ne!(noise(0.0, -6.0, 7, 1000), noise(0.0, -6.0, 8, 1000));
        let mut a = vec![0.0; 100];
        let mut b = vec![0.0; 100];
        Noise.source(&[0.0, -6.0, 1.0], 48000, 0, 7).fill(&mut a);
        Noise.source(&[0.0, -6.0, 1.0], 48000, 1, 7).fill(&mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn noise_stays_under_its_ceiling_and_is_not_silent() {
        for color in 0..3 {
            let x = noise(f64::from(color), -12.0, 3, 480_000);
            let (p, r) = (peak(&x), rms(&x));
            eprintln!("color {color}: peak {p:.4} rms {r:.4} (ceiling {:.4})", db_to_gain(-12.0));
            assert!(p <= db_to_gain(-12.0) + 1e-6);
            assert!(r > db_to_gain(-12.0) * 0.1, "color {color} too quiet: {r}");
            let mean = x.iter().map(|v| f64::from(*v)).sum::<f64>() / x.len() as f64;
            assert!(mean.abs() < 0.02, "color {color} has a DC offset of {mean}");
        }
    }

    #[test]
    fn colors_tilt_the_spectrum() {
        // The share of the energy in the sample-to-sample difference falls from white to brown.
        let ratio = |x: &[f32]| {
            let d: f64 = x.windows(2).map(|w| f64::from(w[1] - w[0]).powi(2)).sum();
            d / x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>()
        };
        let (w, p, b) = (ratio(&noise(0.0, 0.0, 5, 200_000)), ratio(&noise(1.0, 0.0, 5, 200_000)), ratio(&noise(2.0, 0.0, 5, 200_000)));
        eprintln!("difference ratios: white {w:.3} pink {p:.3} brown {b:.3}");
        assert!(w > 1.5 && p < w * 0.5 && b < p * 0.5, "{w} {p} {b}");
    }
}
