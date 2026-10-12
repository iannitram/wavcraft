//! What the spectrogram costs on a long file, and whether a short sound survives the zoom-out.
//!
//! Builds a mono 48 kHz file of quiet noise (three hours unless told otherwise) holding one
//! second of full-scale 3 kHz tone at 46 % of its length (1:23:28 in three hours), renders it
//! fitted to 640, 1280 and 2048 columns, and prints the time per render and the level drawn for
//! the tone. Three hours needs about 2 GB of memory.
//!
//! ```sh
//! cargo run --release -p wavecraft-core --example spectrogram_cost [MINUTES]
//! ```

use std::time::Instant;

use rayon::prelude::*;
use wavecraft_core::SpectrogramParams;
use wavecraft_core::spectrogram::render;

const RATE: u32 = 48_000;
const ROWS: usize = 384;
const TONE_HZ: f32 = 3_000.0;

fn main() {
    let minutes = std::env::args().nth(1).and_then(|a| a.parse::<usize>().ok()).unwrap_or(180).clamp(1, 180);
    let frames = minutes * 60 * RATE as usize;
    // 5008 s of 10800: the 1:23:28 of the README's three-hour case, at the same place in any length.
    let tone_at = (frames as f64 * 5008.0 / 10_800.0) as usize / RATE as usize * RATE as usize;

    let built = Instant::now();
    let mut samples = vec![0.0f32; frames];
    // Noise at about -65 dBFS RMS, from a small generator seeded per block so blocks can be filled side by side.
    samples.par_chunks_mut(1 << 20).enumerate().for_each(|(block, chunk)| {
        let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ (block as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03);
        for v in chunk {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.002;
        }
    });
    for (i, v) in samples.iter_mut().skip(tone_at).take(RATE as usize).enumerate() {
        *v = (std::f32::consts::TAU * TONE_HZ * i as f32 / RATE as f32).sin();
    }
    println!(
        "{minutes} min mono at {RATE} Hz: {frames} frames, built in {:.1} s, {} threads",
        built.elapsed().as_secs_f64(),
        rayon::current_num_threads()
    );

    let p = SpectrogramParams::default();
    let tone_row = (0..ROWS).find(|r| p.row_edge_hz(r + 1, ROWS, RATE) <= TONE_HZ).unwrap_or(0);
    println!("{:>5}  {:>9}  {:>9}  {:>10}  {:>10}", "cols", "best ms", "median ms", "tone level", "row median");
    for cols in [640usize, 1280, 2048] {
        let mut times = Vec::new();
        let mut image = Vec::new();
        for _ in 0..7 {
            let t = Instant::now();
            image = render(&samples, RATE, 0.0, frames as f64, cols, ROWS, &p);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let row = image.get(tone_row * cols..(tone_row + 1) * cols).unwrap_or(&[]);
        let tone_col = (tone_at + RATE as usize / 2) * cols / frames;
        let mut sorted = row.to_vec();
        sorted.sort_unstable();
        println!(
            "{cols:>5}  {:>9.2}  {:>9.2}  {:>10}  {:>10}",
            times.first().copied().unwrap_or(0.0),
            times.get(times.len() / 2).copied().unwrap_or(0.0),
            row.get(tone_col).copied().unwrap_or(0),
            sorted.get(sorted.len() / 2).copied().unwrap_or(0),
        );
    }
}
