//! A look-ahead true-peak limiter (v1 G3, issue #38: Match Loudness's "Use True Peak Limiting").
//! One gain is shared by all channels; the output is the input delayed by
//! [`TruePeakLimiter::latency_frames`], so a caller feeds that many extra frames (zeros at the
//! end) to get the last of its audio out. State carries from one `process` call to the next.
//!
//! STUB: card wav-c-38-b fills this file's bodies.

/// The limiter's state. See card wav-c-38-b.
pub struct TruePeakLimiter {
    _stub: (),
}

impl TruePeakLimiter {
    /// A limiter for `channels` channels at `sample_rate`, holding the true peak at or below
    /// `ceiling_db` dBTP, with `look_ahead_ms` of look-ahead and `release_ms` release time.
    pub fn new(_sample_rate: u32, _channels: usize, _ceiling_db: f64, _look_ahead_ms: f64, _release_ms: f64) -> TruePeakLimiter {
        // STUB: card wav-c-38-b fills this.
        TruePeakLimiter { _stub: () }
    }

    /// How many frames the output lags the input: the look-ahead in frames.
    pub fn latency_frames(&self) -> usize {
        // STUB: card wav-c-38-b fills this.
        0
    }

    /// Limits `input` (one slice per channel, all the same length) into `output` (the same
    /// shape), delayed by [`Self::latency_frames`].
    pub fn process(&mut self, _input: &[&[f32]], _output: &mut [&mut [f32]]) {
        // STUB: card wav-c-38-b fills this.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_dsp::meter::LoudnessMeter;
    use std::f64::consts::PI;

    const RATE: u32 = 48_000;
    /// 12 ms at 48 kHz.
    const L: usize = 576;

    fn sine(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
        (0..frames).map(|k| (amp * (2.0 * PI * freq * k as f64 / f64::from(RATE)).sin()) as f32).collect()
    }

    /// Runs `input` through `lim` in chunks of `chunk` frames.
    fn run(lim: &mut TruePeakLimiter, input: &[Vec<f32>], chunk: usize) -> Vec<Vec<f32>> {
        let frames = input[0].len();
        let mut out = vec![vec![f32::NAN; frames]; input.len()];
        let mut at = 0;
        while at < frames {
            let end = (at + chunk).min(frames);
            let ins: Vec<&[f32]> = input.iter().map(|c| &c[at..end]).collect();
            let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[at..end]).collect();
            lim.process(&ins, &mut outs);
            at = end;
        }
        out
    }

    fn true_peak_db(ch: &[Vec<f32>]) -> f32 {
        let mut m = LoudnessMeter::new(RATE as f32, ch.len());
        m.process(ch, ch[0].len());
        m.true_peak_db()
    }

    /// `ch` with `extra` zero frames appended to every channel (to flush the look-ahead).
    fn padded(ch: Vec<Vec<f32>>, extra: usize) -> Vec<Vec<f32>> {
        ch.into_iter()
            .map(|mut c| {
                c.extend(std::iter::repeat_n(0.0, extra));
                c
            })
            .collect()
    }

    #[test]
    fn latency_is_the_look_ahead_in_frames() {
        assert_eq!(TruePeakLimiter::new(48_000, 2, -1.0, 12.0, 200.0).latency_frames(), 576);
        assert_eq!(TruePeakLimiter::new(44_100, 2, -1.0, 12.0, 200.0).latency_frames(), 529);
        assert_eq!(TruePeakLimiter::new(96_000, 1, -1.0, 12.0, 200.0).latency_frames(), 1152);
        assert_eq!(TruePeakLimiter::new(48_000, 2, -1.0, 0.0, 200.0).latency_frames(), 16);
    }

    #[test]
    fn a_quiet_signal_passes_unchanged_after_the_latency() {
        let input = vec![sine(1000.0, 0.1, 48_000), sine(440.0, 0.1, 48_000)];
        let mut lim = TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0);
        let out = run(&mut lim, &input, 4096);
        for (c, (o, i)) in out.iter().zip(&input).enumerate() {
            assert!(o[..L].iter().all(|&v| v == 0.0), "channel {c}: the first {L} frames are the delay");
            for (n, (got, want)) in o[L..].iter().zip(i).enumerate() {
                assert!((got - want).abs() <= 1e-6, "channel {c} frame {n}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn a_full_scale_sine_comes_out_at_or_below_the_ceiling() {
        let input = padded(vec![sine(997.0, 1.0, 96_000), sine(997.0, 1.0, 96_000)], L);
        let mut lim = TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0);
        let out = run(&mut lim, &input, 4096);
        let tp = true_peak_db(&out);
        assert!(tp <= -1.0, "true peak {tp} dBTP is above the -1 dBTP ceiling");
        assert!(tp >= -1.5, "true peak {tp} dBTP: limited far more than needed");
    }

    #[test]
    fn an_inter_sample_peak_is_caught() {
        // A quarter-rate sine at 45 degrees: every sample is +/-0.7071 (-3 dBFS), the peaks
        // between them are 1.0 (0 dBTP). A sample-peak limiter lets it through.
        let x: Vec<f32> = (0..48_000).map(|n| (PI / 2.0 * n as f64 + PI / 4.0).sin() as f32).collect();
        let input = padded(vec![x.clone(), x], L);
        assert!(true_peak_db(&input) >= -0.5, "the test signal should read about 0 dBTP");
        let mut lim = TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0);
        let out = run(&mut lim, &input, 4096);
        let tp = true_peak_db(&out);
        assert!(tp <= -1.0, "true peak {tp} dBTP is above the -1 dBTP ceiling");
        assert!(tp >= -1.6, "true peak {tp} dBTP: limited far more than needed");
    }

    #[test]
    fn the_gain_comes_back_over_the_release_time() {
        // 0.5 s at full scale, then 2 s at 0.1 (-20 dBFS), one 1 kHz sine, mono.
        let input: Vec<f32> = (0..120_000)
            .map(|k| {
                let amp = if k < 24_000 { 1.0 } else { 0.1 };
                (amp * (2.0 * PI * 1000.0 * k as f64 / f64::from(RATE)).sin()) as f32
            })
            .collect();
        let input = vec![input];
        let mut lim = TruePeakLimiter::new(RATE, 1, -1.0, 12.0, 200.0);
        let out = run(&mut lim, &input, 4096);
        // 100 ms after the loud part: still turned down, but passing
        let peak = (28_800..29_760).map(|n| out[0][n + L].abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.098, "100 ms after the loud part the gain should still be recovering (peak {peak})");
        assert!(peak > 0.05, "100 ms after the loud part the quiet signal should pass (peak {peak})");
        // 1 s after it: back to unity
        for (n, (got, want)) in out[0][72_000 + L..].iter().zip(&input[0][72_000..]).enumerate() {
            assert!((got - want).abs() <= 2e-4, "frame {}: {got} vs {want}", 72_000 + n);
        }
    }

    #[test]
    fn chunk_size_does_not_change_the_output() {
        let loud: Vec<f32> = (0..30_000usize)
            .map(|k| {
                let amp = if (k / 3000).is_multiple_of(2) { 1.2 } else { 0.2 };
                (amp * (2.0 * PI * 700.0 * k as f64 / f64::from(RATE)).sin()) as f32
            })
            .collect();
        let input = vec![loud.clone(), loud.iter().map(|v| v * 0.5).collect()];
        let whole = run(&mut TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0), &input, 30_000);
        let thousands = run(&mut TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0), &input, 1000);
        let odd = run(&mut TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0), &input, 333);
        assert_eq!(whole, thousands);
        assert_eq!(whole, odd);
    }

    #[test]
    fn all_channels_share_one_gain() {
        let left = sine(1000.0, 1.0, 48_000);
        let right: Vec<f32> = left.iter().map(|v| v * 0.1).collect();
        let input = vec![left, right];
        let mut lim = TruePeakLimiter::new(RATE, 2, -1.0, 12.0, 200.0);
        let out = run(&mut lim, &input, 4096);
        // the left channel was limited, so the right one was turned down by the same amount
        let left_peak = out[0][L..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(left_peak < 0.95, "left peak {left_peak}: the left channel should be limited");
        for (n, (r, l)) in out[1].iter().zip(&out[0]).enumerate() {
            assert!((r - l * 0.1).abs() <= 1e-6, "frame {n}: right {r} vs left {l} x 0.1");
        }
    }
}
