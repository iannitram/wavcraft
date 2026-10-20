//! Filters and EQ (f27): Parametric EQ, High-Pass and Low-Pass quick filters, Notch, and DC Offset
//! removal. The biquad designs are `soundcraft-dsp`'s (RBJ cookbook). Each effect starts a
//! [`Run`] that keeps every channel's filter memory from one 64 K-frame chunk to the next, so
//! chunk joins are seamless; the filters start at rest at the first frame of the range.

use std::io;

use soundcraft_dsp::biquad::{Biquad, FilterType};

use super::{Effect, ParamDesc, Run, RunContext};

/// Butterworth Q values of the two sections of a 24 dB/oct (fourth order) filter.
const BUTTERWORTH_4: [f32; 2] = [0.541_196_1, 1.306_563];
/// Butterworth Q of one 12 dB/oct (second order) section.
const BUTTERWORTH_2: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// The Q of the EQ's shelves.
const SHELF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// One filter chain per channel; every channel gets the same design.
struct Chains {
    chains: Vec<Vec<Biquad>>,
}

impl Chains {
    fn new(channels: usize, design: impl Fn() -> Vec<Biquad>) -> Chains {
        Chains { chains: (0..channels).map(|_| design()).collect() }
    }
}

impl Run for Chains {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for ((i, o), chain) in input.iter().zip(output.iter_mut()).zip(self.chains.iter_mut()) {
            o.copy_from_slice(i);
            for section in chain.iter_mut() {
                section.process_block(o);
            }
        }
        Ok(())
    }
}

fn rate(ctx: &RunContext) -> f32 {
    ctx.sample_rate as f32
}

/// Parametric EQ: a low shelf, two peaking bells and a high shelf. A band at 0 dB is left out.
pub struct ParametricEq;

impl Effect for ParametricEq {
    fn id(&self) -> &'static str {
        "parametric_eq"
    }
    fn name(&self) -> &'static str {
        "Parametric EQ"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::frequency("low_freq", "Low shelf frequency", 20.0, 2000.0, 100.0),
            ParamDesc::db("low_gain_db", "Low shelf gain", -24.0, 24.0, 0.0),
            ParamDesc::frequency("mid1_freq", "Bell 1 frequency", 20.0, 20000.0, 500.0),
            ParamDesc::db("mid1_gain_db", "Bell 1 gain", -24.0, 24.0, 0.0),
            ParamDesc::number("mid1_q", "Bell 1 Q", 0.1, 20.0, 1.0, ""),
            ParamDesc::frequency("mid2_freq", "Bell 2 frequency", 20.0, 20000.0, 2500.0),
            ParamDesc::db("mid2_gain_db", "Bell 2 gain", -24.0, 24.0, 0.0),
            ParamDesc::number("mid2_q", "Bell 2 Q", 0.1, 20.0, 1.0, ""),
            ParamDesc::frequency("high_freq", "High shelf frequency", 1000.0, 20000.0, 8000.0),
            ParamDesc::db("high_gain_db", "High shelf gain", -24.0, 24.0, 0.0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let sr = rate(ctx);
        let p = params.to_vec();
        let design = move || {
            let bands = [
                (FilterType::LowShelf, p[0], SHELF_Q, p[1]),
                (FilterType::Peak, p[2], p[4] as f32, p[3]),
                (FilterType::Peak, p[5], p[7] as f32, p[6]),
                (FilterType::HighShelf, p[8], SHELF_Q, p[9]),
            ];
            bands.iter().filter(|b| b.3 != 0.0).map(|&(kind, f, q, g)| Biquad::design(kind, f as f32, q, g as f32, sr)).collect()
        };
        Ok(Some(Box::new(Chains::new(ctx.channels.len(), design))))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Parametric EQ needs the sample rate: run it through effects::apply"))
    }
}

const SLOPES: &[&str] = &["12", "24"];

fn cut_chain(kind: FilterType, cutoff: f64, slope: f64, sr: f32) -> Vec<Biquad> {
    let qs: &[f32] = if slope >= 1.0 { &BUTTERWORTH_4 } else { &[BUTTERWORTH_2] };
    qs.iter().map(|&q| Biquad::design(kind, cutoff as f32, q, 0.0, sr)).collect()
}

/// High-pass quick filter: cuts below the cutoff at 12 or 24 dB per octave.
pub struct HighPass;

impl Effect for HighPass {
    fn id(&self) -> &'static str {
        "high_pass"
    }
    fn name(&self) -> &'static str {
        "High-Pass Filter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] =
            &[ParamDesc::frequency("cutoff", "Cutoff", 20.0, 20000.0, 80.0), ParamDesc::choice("slope", "Slope (dB/oct)", SLOPES, 0)];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let (sr, cutoff, slope) = (rate(ctx), params[0], params[1]);
        Ok(Some(Box::new(Chains::new(ctx.channels.len(), move || cut_chain(FilterType::HighPass, cutoff, slope, sr)))))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "High-Pass Filter needs the sample rate: run it through effects::apply"))
    }
}

/// Low-pass quick filter: cuts above the cutoff at 12 or 24 dB per octave.
pub struct LowPass;

impl Effect for LowPass {
    fn id(&self) -> &'static str {
        "low_pass"
    }
    fn name(&self) -> &'static str {
        "Low-Pass Filter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] =
            &[ParamDesc::frequency("cutoff", "Cutoff", 20.0, 20000.0, 12000.0), ParamDesc::choice("slope", "Slope (dB/oct)", SLOPES, 0)];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let (sr, cutoff, slope) = (rate(ctx), params[0], params[1]);
        Ok(Some(Box::new(Chains::new(ctx.channels.len(), move || cut_chain(FilterType::LowPass, cutoff, slope, sr)))))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Low-Pass Filter needs the sample rate: run it through effects::apply"))
    }
}

/// Notch filter: a narrow band-reject at one frequency; `depth` is the cut at the centre.
pub struct Notch;

impl Effect for Notch {
    fn id(&self) -> &'static str {
        "notch"
    }
    fn name(&self) -> &'static str {
        "Notch Filter"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::frequency("freq", "Frequency", 20.0, 20000.0, 1000.0),
            ParamDesc::number("q", "Q (width)", 0.5, 50.0, 10.0, ""),
            ParamDesc::db("depth_db", "Depth", 0.0, 48.0, 48.0),
        ];
        P
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let (sr, f, q, depth) = (rate(ctx), params[0] as f32, params[1] as f32, params[2] as f32);
        if depth == 0.0 {
            return Ok(Some(Box::new(Chains::new(ctx.channels.len(), Vec::new))));
        }
        // A peaking filter cut by `depth` is a notch of finite depth; its Q sets the width.
        Ok(Some(Box::new(Chains::new(ctx.channels.len(), move || vec![Biquad::design(FilterType::Peak, f, q, -depth, sr)]))))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Notch Filter needs the sample rate: run it through effects::apply"))
    }
}

/// DC Offset removal: subtracts each channel's mean over the processed range.
pub struct DcOffset;

struct Subtract(Vec<f32>);

impl Run for Subtract {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for ((i, o), m) in input.iter().zip(output.iter_mut()).zip(&self.0) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = *a - m;
            }
        }
        Ok(())
    }
}

impl Effect for DcOffset {
    fn id(&self) -> &'static str {
        "dc_offset"
    }
    fn name(&self) -> &'static str {
        "Remove DC Offset"
    }
    fn begin(&self, _params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        // One streaming pass to measure the means (64 K frames at a time, never the whole file).
        let mut buf = vec![0.0f32; crate::store::BLOCK_FRAMES];
        let mut means = Vec::with_capacity(ctx.channels.len());
        for &c in &ctx.channels {
            let (mut sum, mut at) = (0.0f64, ctx.range.start);
            while at < ctx.range.end {
                let m = (ctx.range.end - at).min(buf.len() as u64) as usize;
                let got = ctx.doc.read(c, at, &mut buf[..m])?;
                if got != m {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
                }
                sum += buf[..m].iter().filter(|v| v.is_finite()).map(|&v| f64::from(v)).sum::<f64>();
                at += m as u64;
            }
            means.push((sum / ctx.range.end.saturating_sub(ctx.range.start).max(1) as f64) as f32);
        }
        Ok(Some(Box::new(Subtract(means))))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Remove DC Offset measures the range first: run it through effects::apply"))
    }
}
