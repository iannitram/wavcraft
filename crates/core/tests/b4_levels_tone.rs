//! B4.1: the Levels meter reads a 1 kHz tone at -20 dBFS as -20.0 dB, within 0.1.
//!
//! `Levels::update` takes the peak of each block (linear), so the tone is fed as one peak per
//! 480-frame block (0.01 s at 48 kHz). A 1 kHz sine at 48 kHz has 48 samples a period, so every
//! block holds 10 whole periods and its peak is the amplitude, 0.1 (-20 dBFS).

use wavcraft_core::levels::Levels;

const RATE: f32 = 48_000.0;
const FREQ: f32 = 1_000.0;
const AMPLITUDE: f32 = 0.1;
const BLOCK_FRAMES: usize = 480;
const BLOCKS: usize = 100;

#[test]
fn a_minus_20_dbfs_tone_reads_minus_20_db() {
    let mut l = Levels::new();
    let mut n: usize = 0;
    for _ in 0..BLOCKS {
        let mut p = 0.0f32;
        for _ in 0..BLOCK_FRAMES {
            let s = AMPLITUDE * (2.0 * std::f32::consts::PI * FREQ * n as f32 / RATE).sin();
            p = p.max(s.abs());
            n += 1;
        }
        l.update([p, p], 0.01);
    }
    for ch in 0..2 {
        let reading = l.reading(ch);
        assert!((reading.level_db - -20.0).abs() < 0.1, "channel {ch}: expected -20.0 dB within 0.1, got {reading:?}");
    }
}
