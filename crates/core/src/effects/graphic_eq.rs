//! Graphic EQ (B055): fixed-band sliders for quick tonal shaping. One peaking filter per band
//! (RBJ cookbook form), cascaded, with the filter memory of every channel carried from chunk to
//! chunk by the [`Run`].
//!
//! Settings: `bands` (10, 20 or 30), `g01_db`..`g30_db`, `master_db`. Sliders beyond the chosen
//! band count are ignored (they keep their value, so switching to more bands brings them back).
//! Neighbouring filters overlap, so the filter gains are fitted to make the combined response hit
//! the slider values at the band centres. With every slider and the master at 0 no filter is
//! left, so the audio comes out bit-identical. Bands at or above 0.45 times the sample rate are skipped too. There is no
//! range option: every slider travels -24 to +24 dB.

use std::f64::consts::PI;
use std::io;

use super::{Effect, ParamDesc, Run, RunContext};

// TODO: share with effects/eq.rs once #135 lands

/// 10 bands, one octave apart.
pub const CENTRES_10: [f64; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// 20 bands, half an octave apart.
pub const CENTRES_20: [f64; 20] = [
    25.0, 35.0, 50.0, 70.0, 100.0, 140.0, 200.0, 280.0, 400.0, 560.0, 800.0, 1100.0, 1600.0, 2200.0, 3150.0, 4500.0, 6300.0, 9000.0, 12500.0, 18000.0,
];
/// 30 bands, a third of an octave apart.
pub const CENTRES_30: [f64; 30] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0,
    3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];
/// Filter Q per layout. Chosen so neighbouring bands overlap: a little under the Q whose
/// bandwidth equals the band spacing.
pub const Q_10: f64 = 1.2;
pub const Q_20: f64 = 2.4;
pub const Q_30: f64 = 3.6;

/// Highest band centre used, as a share of the sample rate.
const MAX_CENTRE: f64 = 0.45;

macro_rules! gains {
    ($($id:literal $label:literal),*) => {
        &[$(ParamDesc::db($id, $label, -24.0, 24.0, 0.0)),*]
    };
}

static PARAMS: [ParamDesc; 32] = {
    const G: &[ParamDesc] = gains!(
        "g01_db" "Band 1", "g02_db" "Band 2", "g03_db" "Band 3", "g04_db" "Band 4", "g05_db" "Band 5", "g06_db" "Band 6",
        "g07_db" "Band 7", "g08_db" "Band 8", "g09_db" "Band 9", "g10_db" "Band 10", "g11_db" "Band 11", "g12_db" "Band 12",
        "g13_db" "Band 13", "g14_db" "Band 14", "g15_db" "Band 15", "g16_db" "Band 16", "g17_db" "Band 17", "g18_db" "Band 18",
        "g19_db" "Band 19", "g20_db" "Band 20", "g21_db" "Band 21", "g22_db" "Band 22", "g23_db" "Band 23", "g24_db" "Band 24",
        "g25_db" "Band 25", "g26_db" "Band 26", "g27_db" "Band 27", "g28_db" "Band 28", "g29_db" "Band 29", "g30_db" "Band 30"
    );
    let mut p = [ParamDesc::db("master_db", "Master", -24.0, 24.0, 0.0); 32];
    p[0] = ParamDesc::choice("bands", "Bands", &["10", "20", "30"], 0);
    let mut i = 0;
    while i < 30 {
        p[i + 1] = G[i];
        i += 1;
    }
    p
};

pub struct GraphicEq;

#[derive(Clone, Copy)]
struct Coef {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// RBJ peaking EQ, normalised by a0.
fn peaking(centre: f64, q: f64, db: f64, rate: f64) -> Coef {
    let a = 10f64.powf(db / 40.0);
    let w = 2.0 * PI * centre / rate;
    let alpha = w.sin() / (2.0 * q);
    let cw = w.cos();
    let a0 = 1.0 + alpha / a;
    Coef { b0: (1.0 + alpha * a) / a0, b1: -2.0 * cw / a0, b2: (1.0 - alpha * a) / a0, a1: -2.0 * cw / a0, a2: (1.0 - alpha / a) / a0 }
}

/// Magnitude of a biquad at angular frequency `w`, in dB.
fn response_db(c: &Coef, w: f64) -> f64 {
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let num = (c.b0 + c.b1 * c1 + c.b2 * c2).powi(2) + (c.b1 * s1 + c.b2 * s2).powi(2);
    let den = (1.0 + c.a1 * c1 + c.a2 * c2).powi(2) + (c.a1 * s1 + c.a2 * s2).powi(2);
    10.0 * (num / den).log10()
}

/// Filter gains (dB) whose cascade lands on `targets` at the band centres. Neighbouring peaking
/// filters overlap, so setting each filter to its slider value would pile the skirts up; instead
/// the interaction between bands is solved out: a first guess from the inverse of the one-dB
/// response matrix, then a few corrections from the measured response.
fn fit_gains(centres: &[f64], q: f64, targets: &[f64], rate: f64) -> Vec<f64> {
    let n = centres.len();
    if targets.iter().all(|t| *t == 0.0) {
        return vec![0.0; n];
    }
    let w: Vec<f64> = centres.iter().map(|c| 2.0 * PI * c / rate).collect();
    // m[i][j]: response at centre i per dB of filter j.
    let m: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| response_db(&peaking(centres[j], q, 6.0, rate), w[i]) / 6.0).collect()).collect();
    let inv = invert(&m);
    let apply = |inv: &[Vec<f64>], v: &[f64]| -> Vec<f64> { inv.iter().map(|row| row.iter().zip(v).map(|(a, b)| a * b).sum()).collect() };
    let mut g = apply(&inv, targets);
    for _ in 0..3 {
        let got: Vec<f64> =
            (0..n).map(|i| (0..n).map(|j| if g[j] == 0.0 { 0.0 } else { response_db(&peaking(centres[j], q, g[j], rate), w[i]) }).sum()).collect();
        let resid: Vec<f64> = targets.iter().zip(&got).map(|(t, a)| t - a).collect();
        for (x, d) in g.iter_mut().zip(apply(&inv, &resid)) {
            *x = (*x + d).clamp(-48.0, 48.0);
        }
    }
    g
}

/// Inverse by Gauss-Jordan with partial pivoting (the matrix is small and well conditioned).
fn invert(m: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = m.len();
    let mut a: Vec<Vec<f64>> =
        m.iter().enumerate().map(|(i, r)| r.iter().copied().chain((0..n).map(|j| if i == j { 1.0 } else { 0.0 })).collect()).collect();
    for c in 0..n {
        let p = (c..n).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs())).unwrap();
        a.swap(c, p);
        let d = a[c][c];
        for v in a[c].iter_mut() {
            *v /= d;
        }
        for r in 0..n {
            if r != c {
                let f = a[r][c];
                if f != 0.0 {
                    let pivot = a[c].clone();
                    for (x, y) in a[r].iter_mut().zip(&pivot) {
                        *x -= f * y;
                    }
                }
            }
        }
    }
    a.into_iter().map(|r| r[n..].to_vec()).collect()
}

/// Direct form II transposed memory of one stage on one channel.
#[derive(Clone, Copy, Default)]
struct State {
    z1: f64,
    z2: f64,
}

struct EqRun {
    stages: Vec<Coef>,
    master: f64,
    /// `state[channel][stage]`.
    state: Vec<Vec<State>>,
}

impl Effect for GraphicEq {
    fn id(&self) -> &'static str {
        "graphic_eq"
    }
    fn name(&self) -> &'static str {
        "Graphic EQ"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        &PARAMS
    }
    fn begin(&self, params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let (centres, q): (&[f64], f64) = match params[0] as usize {
            0 => (&CENTRES_10, Q_10),
            1 => (&CENTRES_20, Q_20),
            _ => (&CENTRES_30, Q_30),
        };
        let rate = ctx.sample_rate as f64;
        // Only bands below the limit take part; the rest are skipped whatever their slider says.
        let live: Vec<usize> = (0..centres.len()).filter(|i| centres[*i] < MAX_CENTRE * rate).collect();
        let targets: Vec<f64> = live.iter().map(|i| params[1 + i]).collect();
        let fitted = fit_gains(&live.iter().map(|i| centres[*i]).collect::<Vec<_>>(), q, &targets, rate);
        let stages: Vec<Coef> = live.iter().zip(&fitted).filter(|(_, g)| g.abs() > 1e-9).map(|(i, g)| peaking(centres[*i], q, *g, rate)).collect();
        let master = params[31];
        Ok(Some(Box::new(EqRun { state: vec![vec![State::default(); stages.len()]; ctx.channels.len()], stages, master: 10f64.powf(master / 20.0) })))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("Graphic EQ runs through its Run"))
    }
}

impl Run for EqRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        let unity = self.master == 1.0;
        for ((i, o), st) in input.iter().zip(output.iter_mut()).zip(self.state.iter_mut()) {
            if self.stages.is_empty() && unity {
                o.copy_from_slice(i);
                continue;
            }
            for (a, b) in i.iter().zip(o.iter_mut()) {
                let mut x = *a as f64;
                for (c, s) in self.stages.iter().zip(st.iter_mut()) {
                    let y = c.b0 * x + s.z1;
                    s.z1 = c.b1 * x - c.a1 * y + s.z2;
                    s.z2 = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *b = (x * self.master) as f32;
            }
        }
        Ok(())
    }
}
