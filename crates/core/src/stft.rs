//! The shared STFT analysis / overlap-add resynthesis engine (B011). One Hann window at 50 % or
//! 75 % overlap, a configurable FFT size, and a stateless chunk interface: the frame grid is
//! fixed to a caller-chosen origin in document frames, so any chunk of the output depends only on
//! the input within one FFT length of it. A caller that streams a document in chunks (see
//! [`crate::effects::Effect::context_frames`]) therefore gets exactly the output a single pass
//! over the whole range would give, with the overlap correct across every chunk boundary.
//!
//! Resynthesis windows again with the same Hann window and divides by the exact overlap sum of
//! the squared window, so an unmodified spectrum reproduces the input to float rounding.
//! Used by noise reduction; FFT Filter, Sound Remover and the other spectral tools build on it.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Smallest and largest FFT length accepted.
pub const MIN_FFT: usize = 64;
pub const MAX_FFT: usize = 65_536;

/// Where a chunk sits, in document frames, for [`Stft::process`].
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    /// The document frame of `input[0]`.
    pub input_abs: i64,
    /// The document frame of `out[0]`.
    pub out_abs: i64,
    /// The origin of the frame grid (frames start at `grid + k * hop`).
    pub grid: i64,
    /// Extra frames analysed (not synthesised) before the first, to prime a stateful `modify`.
    pub warmup: usize,
}

/// The analysis / synthesis plan for one FFT size and hop.
pub struct Stft {
    n: usize,
    hop: usize,
    window: Vec<f32>,
    /// Per position within a hop: `n` (the unnormalised inverse FFT's gain) times the sum of the
    /// squared window over the frames covering it.
    norm: Vec<f32>,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
}

impl Stft {
    /// A plan with FFT length `n` (a power of two) and `overlap` percent (50 or 75).
    pub fn new(n: usize, overlap_percent: u32) -> Result<Stft, String> {
        if !n.is_power_of_two() || !(MIN_FFT..=MAX_FFT).contains(&n) {
            return Err(format!("FFT size {n} is not a power of two from {MIN_FFT} to {MAX_FFT}"));
        }
        let hop = match overlap_percent {
            50 => n / 2,
            75 => n / 4,
            other => return Err(format!("overlap {other} % is not 50 or 75")),
        };
        let window: Vec<f32> = (0..n).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos()) as f32).collect();
        let norm = (0..hop)
            .map(|j| {
                let sum: f64 = (0..n / hop).map(|m| f64::from(window[j + m * hop]).powi(2)).sum();
                (sum * n as f64) as f32
            })
            .collect();
        let mut planner = FftPlanner::new();
        Ok(Stft { n, hop, window, norm, fwd: planner.plan_fft_forward(n), inv: planner.plan_fft_inverse(n) })
    }

    /// FFT length.
    pub fn size(&self) -> usize {
        self.n
    }
    /// Frames between successive FFT frames.
    pub fn hop(&self) -> usize {
        self.hop
    }
    /// Spectrum length: `size / 2 + 1`.
    pub fn bins(&self) -> usize {
        self.n / 2 + 1
    }
    /// The analysis window (Hann).
    pub fn window(&self) -> &[f32] {
        &self.window
    }
    /// The sum of the squared window: the power gain a noise-like signal sees in one bin.
    pub fn window_energy(&self) -> f32 {
        self.window.iter().map(|w| w * w).sum()
    }

    /// The windowed spectrum of the `size` frames starting at `frame_abs`, taken from `signal`
    /// (which holds the frames from `signal_abs`; anything outside it reads as 0). `out` has
    /// [`bins`](Self::bins) entries; `scratch` is reused between calls.
    pub fn analyse(&self, signal: &[f32], signal_abs: i64, frame_abs: i64, scratch: &mut Vec<Complex32>, out: &mut [Complex32]) {
        scratch.clear();
        scratch.resize(self.n, Complex32::new(0.0, 0.0));
        let off = frame_abs - signal_abs;
        for (i, (s, w)) in scratch.iter_mut().zip(&self.window).enumerate() {
            let at = off + i as i64;
            if at >= 0 && (at as usize) < signal.len() {
                s.re = signal[at as usize] * w;
            }
        }
        self.fwd.process(scratch);
        out.copy_from_slice(&scratch[..self.bins()]);
    }

    /// Resynthesise `out` (which holds the frames from `out_abs`) from `input` (frames from
    /// `input_abs`, zero outside), calling `modify` on every frame's spectrum in time order.
    ///
    /// The frame grid is `grid + k * hop`. Every frame that overlaps `out` is analysed, so `out`
    /// is exact however the caller cut the stream. `warmup` extra frames before the first
    /// are analysed and passed to `modify` but not synthesised, for a `modify` that carries state
    /// from frame to frame. Frames outside `input` are analysed as silence.
    pub fn process<F>(&self, input: &[f32], out: &mut [f32], at: Placement, mut modify: F)
    where
        F: FnMut(&mut [Complex32]),
    {
        let Placement { input_abs, out_abs, grid, warmup } = at;
        out.iter_mut().for_each(|o| *o = 0.0);
        if out.is_empty() {
            return;
        }
        let (n, hop) = (self.n as i64, self.hop as i64);
        let out_end = out_abs + out.len() as i64;
        // Frames with start s in (out_abs - n, out_end): k from ceil((out_abs-n+1-grid)/hop).
        let first = -(-(out_abs - n + 1 - grid)).div_euclid(hop);
        let last = (out_end - 1 - grid).div_euclid(hop);
        let mut scratch = Vec::with_capacity(self.n);
        let mut spec = vec![Complex32::new(0.0, 0.0); self.bins()];
        let mut full = vec![Complex32::new(0.0, 0.0); self.n];
        for k in first - warmup as i64..=last {
            let start = grid + k * hop;
            self.analyse(input, input_abs, start, &mut scratch, &mut spec);
            modify(&mut spec);
            if k < first {
                continue;
            }
            full[..spec.len()].copy_from_slice(&spec);
            full[0].im = 0.0;
            full[self.n / 2].im = 0.0;
            for b in 1..self.n / 2 {
                full[self.n - b] = spec[b].conj();
            }
            self.inv.process(&mut full);
            for (i, (f, w)) in full.iter().zip(&self.window).enumerate() {
                let at = start + i as i64 - out_abs;
                if at >= 0 && (at as usize) < out.len() {
                    out[at as usize] += f.re * w;
                }
            }
        }
        for (i, o) in out.iter_mut().enumerate() {
            *o /= self.norm[(out_abs + i as i64 - grid).rem_euclid(hop) as usize];
        }
    }
}
