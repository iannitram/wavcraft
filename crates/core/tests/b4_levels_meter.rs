//! Locked tests for wav-c-118-b (#118, v1 item B4): per-channel peak, peak hold, clip light and
//! RMS readings in dBFS. The audio is fed in 480-frame blocks (10 ms at 48 kHz), as the audio
//! callback would. Expected values are fixed here; do not edit them.

use std::f64::consts::PI;

use wavcraft_core::meters::levels_meter::LevelsMeter;

const RATE: f32 = 48_000.0;
const BLOCK: usize = 480;

fn close(got: f32, want: f32, tol: f32) {
    assert!((got - want).abs() <= tol, "got {got}, want {want} (within {tol})");
}

/// `amp * sin(2 pi f n / 48000)` for `frames` frames.
fn sine(amp: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (2.0 * PI * hz * n as f64 / 48_000.0).sin()) as f32).collect()
}

fn feed_blocks(meter: &mut LevelsMeter, left: &[f32], right: &[f32]) {
    for (l, r) in left.chunks(BLOCK).zip(right.chunks(BLOCK)) {
        meter.process(&[l, r]);
    }
}

#[test]
fn b4_meter_reads_peak_and_rms_of_a_test_tone_per_channel() {
    // Left: 1 kHz at amplitude 0.1 (-20 dBFS peak, -23.01 dBFS RMS).
    // Right: 1 kHz at amplitude 0.5 (-6.02 dBFS peak, -9.03 dBFS RMS). Two seconds.
    let mut meter = LevelsMeter::new(RATE, 2);
    feed_blocks(&mut meter, &sine(0.1, 1000.0, 96_000), &sine(0.5, 1000.0, 96_000));
    let levels = meter.levels();
    assert_eq!(levels.len(), 2);
    close(levels[0].peak_db, -20.0, 0.01);
    close(levels[0].hold_db, -20.0, 0.01);
    close(levels[0].rms_db, -23.0103, 0.05);
    assert!(!levels[0].clipped);
    close(levels[1].peak_db, -6.0206, 0.01);
    close(levels[1].hold_db, -6.0206, 0.01);
    close(levels[1].rms_db, -9.0309, 0.05);
    assert!(!levels[1].clipped);
}

#[test]
fn b4_meter_holds_the_peak_for_two_seconds_then_lets_it_fall() {
    let mut meter = LevelsMeter::new(RATE, 1);
    let mut burst = vec![0.0f32; BLOCK];
    burst[100] = 0.5;
    meter.process(&[&burst[..]]);
    let silence = vec![0.0f32; BLOCK];
    for _ in 0..100 {
        meter.process(&[&silence[..]]);
    }
    // One second later: the level fell 20 dB, the hold has not moved.
    let one_second = meter.levels()[0];
    close(one_second.peak_db, -26.0206, 0.02);
    close(one_second.hold_db, -6.0206, 0.01);
    for _ in 0..300 {
        meter.process(&[&silence[..]]);
    }
    // Four seconds later: the level fell 80 dB; the hold, released after 2 s, fell about 40 dB.
    let four_seconds = meter.levels()[0];
    close(four_seconds.peak_db, -86.0206, 0.05);
    close(four_seconds.hold_db, -45.82, 0.3);
}

#[test]
fn b4_meter_clip_light_latches_per_channel_until_reset_clip() {
    let mut meter = LevelsMeter::new(RATE, 2);
    let mut hot = vec![0.0f32; BLOCK];
    hot[7] = 1.0;
    let quiet = vec![0.25f32; BLOCK];
    meter.process(&[&hot[..], &quiet[..]]);
    let levels = meter.levels();
    assert!(levels[0].clipped);
    assert!(!levels[1].clipped);
    close(levels[0].peak_db, 0.0, 0.001);
    let silence = vec![0.0f32; BLOCK];
    for _ in 0..300 {
        meter.process(&[&silence[..], &silence[..]]);
    }
    assert!(meter.levels()[0].clipped, "the clip light stays on through 3 s of silence");
    meter.reset_clip();
    let levels = meter.levels();
    assert!(!levels[0].clipped);
    assert!(!levels[1].clipped);
}

#[test]
fn b4_meter_reset_returns_every_reading_to_the_floor() {
    let mut meter = LevelsMeter::new(RATE, 2);
    let hot = vec![1.0f32; BLOCK];
    meter.process(&[&hot[..], &hot[..]]);
    meter.reset();
    for ch in meter.levels() {
        close(ch.peak_db, -144.0, 0.001);
        close(ch.hold_db, -144.0, 0.001);
        close(ch.rms_db, -144.0, 0.001);
        assert!(!ch.clipped);
    }
}

#[test]
fn b4_meter_ignores_extra_channels_and_leaves_missing_ones() {
    let mut meter = LevelsMeter::new(RATE, 2);
    let half = vec![0.5f32; BLOCK];
    let full = vec![1.0f32; BLOCK];
    // Only one slice: channel 1 is left as it is.
    meter.process(&[&half[..]]);
    let levels = meter.levels();
    assert_eq!(levels.len(), 2);
    close(levels[0].peak_db, -6.0206, 0.01);
    close(levels[1].peak_db, -144.0, 0.001);
    // A third slice is ignored, so its full-scale samples light no clip.
    let mut mono = LevelsMeter::new(RATE, 1);
    mono.process(&[&half[..], &full[..], &full[..]]);
    let levels = mono.levels();
    assert_eq!(levels.len(), 1);
    assert!(!levels[0].clipped);
    // No channels: nothing to read, and feeding it does not panic.
    let mut none = LevelsMeter::new(RATE, 0);
    none.process(&[&half[..]]);
    assert!(none.levels().is_empty());
}
