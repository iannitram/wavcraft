//! The STFT engine (B011): unmodified resynthesis is the input, and the result does not depend on
//! how a range was cut into chunks.

use wavcraft_core::stft::{Placement, Stft};

/// Deterministic Gaussian noise (xorshift + Box-Muller), standard deviation `sigma`.
fn gaussian(frames: usize, sigma: f64, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        ((x >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    (0..frames)
        .map(|_| {
            let (u, v) = (next(), next());
            (sigma * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
        })
        .collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

fn error_db(a: &[f32], b: &[f32]) -> f64 {
    let err: Vec<f32> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    db(rms(&err) / rms(a))
}

#[test]
fn analysis_then_resynthesis_with_no_change_is_the_input_to_minus_100_db() {
    let x = gaussian(50_000, 0.3, 1);
    for (n, overlap) in [(512, 50), (512, 75), (4096, 75), (2048, 50)] {
        let stft = Stft::new(n, overlap).unwrap();
        let mut out = vec![0.0f32; x.len()];
        stft.process(&x, &mut out, Placement { input_abs: 0, out_abs: 0, grid: 0, warmup: 0 }, |_| {});
        let e = error_db(&x, &out);
        assert!(e < -100.0, "fft {n} overlap {overlap} %: error {e:.1} dB");
    }
}

#[test]
fn the_grid_origin_and_a_chunked_pass_do_not_change_the_result() {
    let x = gaussian(20_000, 0.3, 2);
    let stft = Stft::new(1024, 75).unwrap();
    // One pass over [5000, 15000) with the grid at 5000, against 7 uneven chunks of it.
    let mut whole = vec![0.0f32; 10_000];
    stft.process(&x, &mut whole, Placement { input_abs: 0, out_abs: 5000, grid: 5000, warmup: 0 }, |s| s.iter_mut().for_each(|b| *b *= 0.5));
    let mut chunked = vec![0.0f32; 10_000];
    let mut at = 0;
    for len in [1, 1023, 2000, 1024, 3000, 1, 2951] {
        stft.process(&x, &mut chunked[at..at + len], Placement { input_abs: 0, out_abs: 5000 + at as i64, grid: 5000, warmup: 0 }, |s| {
            s.iter_mut().for_each(|b| *b *= 0.5)
        });
        at += len;
    }
    assert_eq!(at, 10_000);
    let worst = whole.iter().zip(&chunked).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "worst difference {worst}");
    // And a constant gain of 0.5 is exactly half the input.
    let half: Vec<f32> = x[5000..15000].iter().map(|v| v * 0.5).collect();
    assert!(error_db(&half, &whole) < -100.0);
}
