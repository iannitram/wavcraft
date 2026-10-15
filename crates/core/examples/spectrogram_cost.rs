//! What the spectrogram costs on a long file, with and without its overview, and whether a short
//! sound survives the zoom-out.
//!
//! Builds a mono 48 kHz file of quiet noise (three hours unless told otherwise) holding one
//! second of full-scale 3 kHz tone at 46 % of its length (1:23:28 in three hours). It builds the
//! file's overview and prints how long that took and how much memory it holds. Then it renders
//! the file fitted to 640, 1280 and 2048 columns twice, from the audio alone and with the
//! overview, and prints the time per render, whether the two pictures are the same, and the
//! level drawn for the tone. Three hours needs about 2 GB of memory.
//!
//! ```sh
//! cargo run --release -p wavcraft-core --example spectrogram_cost [MINUTES]
//! ```

use std::time::Instant;

use rayon::prelude::*;
use wavcraft_core::SpectrogramParams;
use wavcraft_core::spectrogram::{BLOCK_HOPS, Columns, Overview, render_columns};

const RATE: u32 = 48_000;
const ROWS: usize = 384;
const TONE_HZ: f32 = 3_000.0;

/// Run `work` once to warm up and then `runs` times: its result, and the best and the median
/// time in milliseconds.
fn timed<T>(runs: usize, mut work: impl FnMut() -> T) -> (T, f64, f64) {
    let mut times = Vec::new();
    let mut result = work();
    for _ in 0..runs {
        let t = Instant::now();
        result = work();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    (result, times.first().copied().unwrap_or(0.0), times.get(times.len() / 2).copied().unwrap_or(0.0))
}

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
    let (overview, best, median) = timed(5, || Overview::build(&samples, &p));
    println!(
        "overview, blocks of {BLOCK_HOPS} FFTs: built in {best:.0} ms (median {median:.0}), {:.1} MB, {:.2} MB per channel-hour",
        overview.bytes() as f64 / 1e6,
        overview.bytes() as f64 / 1e6 * 60.0 / minutes as f64
    );

    let tone_row = (0..ROWS).find(|r| p.row_edge_hz(r + 1, ROWS, RATE) <= TONE_HZ).unwrap_or(0);
    println!("{:>5}  {:>26}  {:>26}  {:>5}  {:>10}  {:>10}", "", "audio alone, ms", "with the overview, ms", "", "", "");
    println!(
        "{:>5}  {:>12}  {:>12}  {:>12}  {:>12}  {:>5}  {:>10}  {:>10}",
        "cols", "best", "median", "best", "median", "same", "tone level", "row median"
    );
    for cols in [640usize, 1280, 2048] {
        let columns = Columns { start: 0.0, end: frames as f64, cols };
        let (live, live_best, live_median) = timed(7, || render_columns(&samples, RATE, columns, ROWS, &p, None));
        let (image, best, median) = timed(7, || render_columns(&samples, RATE, columns, ROWS, &p, Some(&overview)));
        let row = image.get(tone_row * cols..(tone_row + 1) * cols).unwrap_or(&[]);
        let tone_col = (tone_at + RATE as usize / 2) * cols / frames;
        let mut sorted = row.to_vec();
        sorted.sort_unstable();
        println!(
            "{cols:>5}  {live_best:>12.2}  {live_median:>12.2}  {best:>12.2}  {median:>12.2}  {:>5}  {:>10}  {:>10}",
            if image == live { "yes" } else { "NO" },
            row.get(tone_col).copied().unwrap_or(0),
            sorted.get(sorted.len() / 2).copied().unwrap_or(0),
        );
    }
}
