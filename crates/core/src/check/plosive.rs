//! Plosives: short bursts of low-frequency energy (below [`Config::plosive_band_hz`]) that stand
//! well above both the rest of the spectrum and the low band's own level nearby.

use std::f64::consts::{PI, SQRT_2};

use super::{Config, Finding, Kind, Severity, Sink, db};
use crate::spectrogram::Samples;

/// Frames either side of a frame whose median low-band level it is compared with (0.5 s).
const NEIGHBOURS: usize = 50;
/// Low-band RMS under this (linear, about -45 dBFS) is not a plosive however it compares.
const FLOOR: f64 = 0.005_6;

/// A 2nd-order Butterworth low-pass or high-pass (direct form I), run twice for 24 dB per octave.
struct Filter {
    b: [f64; 3],
    a: [f64; 2],
    /// Per stage: the last two inputs, then the last two outputs.
    state: [[f64; 4]; 2],
}

impl Filter {
    fn new(rate: f64, hz: f64, high: bool) -> Filter {
        let w = (PI * hz / rate).tan();
        let k = 1.0 + SQRT_2 * w + w * w;
        let b = if high { [1.0 / k, -2.0 / k, 1.0 / k] } else { [w * w / k, 2.0 * w * w / k, w * w / k] };
        Filter { b, a: [2.0 * (w * w - 1.0) / k, (1.0 - SQRT_2 * w + w * w) / k], state: [[0.0; 4]; 2] }
    }

    fn run(&mut self, x: f64) -> f64 {
        let (b, a) = (self.b, self.a);
        self.state.iter_mut().fold(x, |x, s| {
            let y = b[0] * x + b[1] * s[0] + b[2] * s[1] - a[0] * s[2] - a[1] * s[3];
            *s = [x, s[0], y, s[2]];
            y
        })
    }
}

pub(super) fn plosive_pass(src: &dyn Samples, channel: usize, cfg: &Config, sink: &mut Sink) {
    let len = src.len();
    let frame = (cfg.ms(cfg.plosive_frame_ms) as usize).max(1);
    // The rest of the spectrum starts an octave above the band, so a voice's fundamental, which sits
    // at its edge, does not count as plosive energy.
    let rate = f64::from(cfg.sample_rate);
    let (mut low, mut high) = (Filter::new(rate, cfg.plosive_band_hz, false), Filter::new(rate, cfg.plosive_band_hz * 2.0, true));
    // Per whole frame: low-band RMS, RMS above the band, and whether a sample clipped.
    let mut frames: Vec<(f64, f64, bool)> = Vec::with_capacity(len / frame);
    let mut buf = vec![0f32; frame * 100];
    let mut pos = 0usize;
    while len - pos >= frame {
        let n = (len - pos).min(buf.len());
        src.read(pos, &mut buf[..n]);
        for f in buf[..n].chunks_exact(frame) {
            let (mut lo, mut hi, mut clipped) = (0.0, 0.0, false);
            for &x in f {
                lo += low.run(f64::from(x)).powi(2);
                hi += high.run(f64::from(x)).powi(2);
                clipped |= x.abs() >= cfg.clip_level;
            }
            let n = frame as f64;
            frames.push(((lo / n).sqrt(), (hi / n).sqrt(), clipped));
        }
        pos += n - n % frame;
    }

    let ratio = f64::from(cfg.plosive_ratio);
    let burst: Vec<bool> = (0..frames.len())
        .map(|i| {
            let (lo, rest, clipped) = frames[i];
            if clipped || lo < FLOOR || lo < ratio * rest {
                return false;
            }
            let mut near: Vec<f64> = frames[i.saturating_sub(NEIGHBOURS)..(i + NEIGHBOURS + 1).min(frames.len())].iter().map(|f| f.0).collect();
            let mid = near.len() / 2;
            near.select_nth_unstable_by(mid, f64::total_cmp);
            lo >= ratio * near[mid]
        })
        .collect();

    let min_frames = (cfg.ms(cfg.plosive_min_ms) as usize).div_ceil(frame).max(1);
    let mut i = 0;
    while i < burst.len() {
        if !burst[i] {
            i += 1;
            continue;
        }
        let j = (i..burst.len()).find(|&j| !burst[j]).unwrap_or(burst.len());
        if j - i >= min_frames {
            let top = frames[i..j].iter().map(|f| f.0).fold(0.0, f64::max);
            sink.push(Finding {
                kind: Kind::Plosive,
                severity: Severity::Warning,
                channel: Some(channel),
                start: (i * frame) as u64,
                end: (j * frame) as u64,
                value: db(top),
            });
        }
        i = j;
    }
}
