//! Dither and noise shaping for bit-depth reduction (roadmap A163).
//!
//! Rounding an `f32` sample to 16 or 24 bits makes an error that follows the signal: a quiet
//! tone comes out as steps (distortion), or vanishes. Adding a little noise before the rounding
//! ([`DitherType`]) makes the error independent of the signal, so the tone survives under a
//! steady hiss. **Dither is off by default** (the owner's rule, as in Audition): with
//! [`DitherSettings::default`] the output is exactly `round(x · 2^(bits−1))`, as [`quantize`]
//! computes it, so a file saved at its own depth stays bit for bit.
//!
//! - **Rectangular** (RPDF): uniform noise of one least-significant bit (LSB) peak to peak.
//!   Removes the signal-dependent *average* error; its power still varies a little with the signal.
//! - **Triangular** (TPDF): the sum of two such values, ±1 LSB. The error's first two moments no
//!   longer depend on the signal at all. The usual choice.
//! - **Noise shaping** ([`Shaping`]) feeds each sample's rounding error back into the next
//!   samples through a short FIR filter, so the noise has the spectrum `|NTF(f)|²` times a flat
//!   one, with `NTF(z) = 1 − Σ c_k z^−k`. Both curves here are high-pass: a zero at 0 Hz, noise
//!   energy moved up to where the ear is least sensitive. The total noise power rises (the price).
//!   - **Light**: `1 − z⁻¹` (first order). Quieter than flat noise below about fs/6, +6 dB at fs/2.
//!   - **Strong**: `1 − 2z⁻¹ + z⁻²` (second order). Quieter below about fs/4 (much quieter in
//!     the bottom octaves), +12 dB at fs/2.
//!
//!   Both are FIR error feedback, so they are unconditionally stable: the stored error is the
//!   rounding error of an unclamped value and stays within ±1.5 LSB however loud the input.
//!
//! The random numbers come from a seeded generator (SplitMix64, one stream per channel from the
//! seed and the channel number), so the same settings give the same file every time. A
//! [`Ditherer`] keeps its generator and filter history between calls, so a long channel can be
//! processed in chunks of any size with the same result as in one go.

use crate::save::quantize;

/// The dither noise added before rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DitherType {
    /// No noise: plain rounding.
    #[default]
    None,
    /// Uniform, 1 LSB peak to peak.
    Rectangular,
    /// Triangular, 2 LSB peak to peak (TPDF).
    Triangular,
}

impl DitherType {
    pub const ALL: [DitherType; 3] = [DitherType::None, DitherType::Triangular, DitherType::Rectangular];

    pub fn name(self) -> &'static str {
        match self {
            DitherType::None => "none",
            DitherType::Rectangular => "rectangular",
            DitherType::Triangular => "triangular",
        }
    }

    pub fn parse(s: &str) -> Option<DitherType> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(DitherType::None),
            "triangular" | "tpdf" => Some(DitherType::Triangular),
            "rectangular" | "rpdf" => Some(DitherType::Rectangular),
            _ => None,
        }
    }
}

/// How the rounding error is fed back to move noise up in frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shaping {
    #[default]
    None,
    Light,
    Strong,
}

impl Shaping {
    pub const ALL: [Shaping; 3] = [Shaping::None, Shaping::Light, Shaping::Strong];

    pub fn name(self) -> &'static str {
        match self {
            Shaping::None => "none",
            Shaping::Light => "light",
            Shaping::Strong => "strong",
        }
    }

    pub fn parse(s: &str) -> Option<Shaping> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Shaping::None),
            "light" => Some(Shaping::Light),
            "strong" => Some(Shaping::Strong),
            _ => None,
        }
    }

    /// The feedback coefficients `c_1..`: the noise transfer function is `1 − Σ c_k z^−k`.
    fn taps(self) -> &'static [f64] {
        match self {
            Shaping::None => &[],
            Shaping::Light => &[1.0],
            Shaping::Strong => &[2.0, -1.0],
        }
    }
}

/// The seed used unless the settings name another.
pub const DEFAULT_SEED: u64 = 0x5EED_D17E_2026_1020;

/// Dither, noise shaping and the seed of the random numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DitherSettings {
    pub dither: DitherType,
    pub shaping: Shaping,
    pub seed: u64,
}

impl Default for DitherSettings {
    fn default() -> Self {
        DitherSettings { dither: DitherType::None, shaping: Shaping::None, seed: DEFAULT_SEED }
    }
}

impl DitherSettings {
    /// Neither dither nor shaping: plain rounding.
    pub fn is_off(&self) -> bool {
        self.dither == DitherType::None && self.shaping == Shaping::None
    }

    /// "no dither", "triangular dither, light noise shaping"…
    pub fn describe(&self) -> String {
        match (self.dither, self.shaping) {
            (DitherType::None, Shaping::None) => "no dither".to_string(),
            (d, Shaping::None) => format!("{} dither", d.name()),
            (DitherType::None, s) => format!("{} noise shaping, no dither", s.name()),
            (d, s) => format!("{} dither, {} noise shaping", d.name(), s.name()),
        }
    }
}

/// SplitMix64: a small, fast generator with good statistics for noise.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [-0.5, 0.5).
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
}

/// Quantises one channel to an integer depth with the chosen dither and shaping. Make one per
/// channel and keep it for the whole channel.
#[derive(Debug, Clone)]
pub struct Ditherer {
    dither: DitherType,
    taps: &'static [f64],
    rng: Rng,
    /// The last rounding errors, newest first, in LSB.
    history: [f64; 2],
}

impl Ditherer {
    /// For channel `channel` of a file (each channel has its own random stream).
    pub fn new(settings: &DitherSettings, channel: usize) -> Ditherer {
        let stream = settings.seed ^ (channel as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        let mut rng = Rng(stream);
        rng.next_u64(); // decorrelate neighbouring seeds
        Ditherer { dither: settings.dither, taps: settings.shaping.taps(), rng, history: [0.0; 2] }
    }

    /// `x` (full scale ±1) as a `bits`-bit integer. NaN is 0; out-of-range values clamp.
    pub fn quantize(&mut self, x: f32, bits: u32) -> i32 {
        if self.dither == DitherType::None && self.taps.is_empty() {
            return quantize(x, bits);
        }
        let scale = (1u64 << (bits - 1)) as f64;
        if x.is_nan() {
            self.history = [0.0; 2];
            return 0;
        }
        let mut u = f64::from(x).clamp(-2.0, 2.0) * scale; // infinities must not reach the feedback
        for (c, e) in self.taps.iter().zip(&self.history) {
            u -= c * e;
        }
        let noise = match self.dither {
            DitherType::None => 0.0,
            DitherType::Rectangular => self.rng.uniform(),
            DitherType::Triangular => self.rng.uniform() + self.rng.uniform(),
        };
        let q = (u + noise).round();
        // The error of the unclamped value, so clipping can't wind the feedback up.
        self.history = [q - u, self.history[0]];
        q.clamp(-scale, scale - 1.0) as i32
    }

    /// Quantise `input` into `out` (same length; extra `out` is left alone).
    pub fn process(&mut self, input: &[f32], bits: u32, out: &mut [i32]) {
        for (o, &x) in out.iter_mut().zip(input) {
            *o = self.quantize(x, bits);
        }
    }
}
