//! DeHummer (B073): remove mains hum (50 or 60 Hz) and its harmonics. One narrow peaking cut per
//! harmonic per channel, each with `f64` state carried from chunk to chunk by a [`Run`].
//!
//! Each cut is an RBJ peaking biquad whose gain is `depth_db` at the notch centre, so the depth
//! is exact there and the filter returns to unity away from the hum. A higher `q` is a narrower
//! notch. Harmonics at or above 0.45 times the sample rate are left out. The filters do not
//! depend on the position in the document, so a selection starting anywhere gives the same
//! result as one starting at frame 0.
//!
//! `harmonics` and `skip_low` are counts: the seam's Number kind allows fractions, so a fractional
//! value is rounded to the nearest whole number.

use std::f64::consts::PI;
use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

pub struct DeHummer;

// TODO: share with effects/eq.rs once #135 lands
/// One biquad in transposed direct form II, `f64` throughout.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// RBJ peaking filter: `gain_db` at `freq`, unity far away, width set by `q`.
    fn peaking(freq: f64, q: f64, gain_db: f64, rate: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

struct HumRun {
    /// Per channel, one cut per kept harmonic.
    chains: Vec<Vec<Biquad>>,
    gain: f64,
}

impl Effect for DeHummer {
    fn id(&self) -> &'static str {
        "dehummer"
    }
    fn name(&self) -> &'static str {
        "DeHummer"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::frequency("frequency", "Hum frequency", 40.0, 70.0, 60.0),
            ParamDesc::number("harmonics", "Harmonics", 1.0, 20.0, 5.0),
            ParamDesc::number("q", "Notch Q", 1.0, 200.0, 30.0),
            ParamDesc::db("depth_db", "Depth", -80.0, 0.0, -40.0),
            ParamDesc::number("skip_low", "Leave low harmonics", 0.0, 19.0, 0.0),
            ParamDesc::db("gain_db", "Output gain", -12.0, 12.0, 0.0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let [freq, harmonics, q, depth, skip_low, gain_db] = params else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "DeHummer takes 6 settings"));
        };
        let rate = f64::from(ctx.sample_rate);
        let (harmonics, skip_low) = (harmonics.round() as usize, skip_low.round() as usize);
        let cuts: Vec<Biquad> =
            (skip_low..harmonics).map(|k| freq * (k + 1) as f64).filter(|f| *f < 0.45 * rate).map(|f| Biquad::peaking(f, *q, *depth, rate)).collect();
        Ok(Some(Box::new(HumRun { chains: vec![cuts; ctx.channels.len()], gain: 10f64.powf(gain_db / 20.0) })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("DeHummer keeps filter state and runs only through begin"))
    }
}

impl Run for HumRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for ((i, o), chain) in input.iter().zip(output.iter_mut()).zip(self.chains.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                let mut x = f64::from(*a);
                for cut in chain.iter_mut() {
                    x = cut.tick(x);
                }
                *b = (x * self.gain) as f32;
            }
        }
        Ok(())
    }
}
