//! Locked tests for wav-c-118-c (#118, v1 item G4): Amplitude Statistics (peak, true peak, RMS,
//! integrated LUFS, DC offset, clipped samples) measured without changing the audio, fed whole or
//! in chunks. Expected values are fixed here; do not edit them.

use std::f64::consts::PI;

use wavcraft_core::meters::amplitude_stats::{AmplitudeScan, AmplitudeStats, amplitude_stats};

const RATE: f32 = 48_000.0;

fn close(got: f32, want: f32, tol: f32) {
    assert!((got - want).abs() <= tol, "got {got}, want {want} (within {tol})");
}

/// `dc + amp * sin(2 pi f n / 48000)` for `frames` frames.
fn sine(dc: f64, amp: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (dc + amp * (2.0 * PI * hz * n as f64 / 48_000.0).sin()) as f32).collect()
}

#[test]
fn g4_stats_minus_20_dbfs_stereo_sine() {
    // 1 kHz at amplitude 0.1 on both channels, 3 s: -20 dBFS peak, -23.01 dBFS RMS, -20 LUFS.
    let tone = sine(0.0, 0.1, 1000.0, 144_000);
    let stats = amplitude_stats(&[tone.clone(), tone], RATE);
    assert_eq!(stats.frames, 144_000);
    assert_eq!(stats.channels.len(), 2);
    for ch in &stats.channels {
        close(ch.peak_db, -20.0, 0.01);
        close(ch.rms_db, -23.0103, 0.01);
        close(ch.dc_offset_percent, 0.0, 0.0001);
        assert_eq!(ch.clipped_samples, 0);
    }
    close(stats.true_peak_db, -20.0, 0.1);
    close(stats.integrated_lufs, -20.0, 0.1);
}

#[test]
fn g4_stats_dc_offset_and_rms_of_an_offset_sine() {
    // 0.25 + 0.5 sin(1 kHz), 1 s, mono: peak 0.75, mean 0.25, RMS sqrt(0.1875).
    let stats = amplitude_stats(&[sine(0.25, 0.5, 1000.0, 48_000)], RATE);
    assert_eq!(stats.frames, 48_000);
    assert_eq!(stats.channels.len(), 1);
    let ch = &stats.channels[0];
    close(ch.peak_db, -2.4987748, 0.001);
    close(ch.rms_db, -7.269987, 0.001);
    close(ch.dc_offset_percent, 25.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);
}

#[test]
fn g4_stats_counts_clipped_samples_per_channel() {
    let mut left = vec![0.0f32; 1000];
    left[10] = 1.0;
    left[11] = 1.0;
    left[12] = 1.0;
    left[500] = -1.0;
    left[600] = 0.9999;
    left[700] = 1.5;
    let mut right = vec![0.0f32; 1000];
    right[3] = -1.0;
    let stats = amplitude_stats(&[left, right], RATE);
    assert_eq!(stats.frames, 1000);
    let (l, r) = (&stats.channels[0], &stats.channels[1]);
    assert_eq!(l.clipped_samples, 5);
    assert_eq!(r.clipped_samples, 1);
    close(l.peak_db, 3.521825, 0.001);
    close(r.peak_db, 0.0, 0.001);
    close(l.rms_db, -21.39674, 0.001);
    close(r.rms_db, -30.0, 0.001);
    close(l.dc_offset_percent, 0.44999, 0.0001);
    close(r.dc_offset_percent, -0.1, 0.0001);
}

#[test]
fn g4_stats_chunked_pushes_match_one_push() {
    // Left: 1 kHz at 0.1. Right: 440 Hz at 0.3 on a DC offset of 0.01. Two seconds.
    let left = sine(0.0, 0.1, 1000.0, 96_000);
    let right = sine(0.01, 0.3, 440.0, 96_000);
    let whole = amplitude_stats(&[left.clone(), right.clone()], RATE);
    let mut scan = AmplitudeScan::new(RATE, 2);
    for start in (0..96_000).step_by(4800) {
        scan.push(&[left[start..start + 4800].to_vec(), right[start..start + 4800].to_vec()]);
    }
    let chunked: AmplitudeStats = scan.finish();
    assert_eq!(chunked.frames, 96_000);
    assert_eq!(whole.frames, 96_000);
    close(whole.channels[0].peak_db, -20.0, 0.01);
    close(whole.channels[1].dc_offset_percent, 1.0, 0.001);
    for (a, b) in chunked.channels.iter().zip(&whole.channels) {
        close(a.peak_db, b.peak_db, 1e-6);
        close(a.rms_db, b.rms_db, 1e-4);
        close(a.dc_offset_percent, b.dc_offset_percent, 1e-4);
        assert_eq!(a.clipped_samples, b.clipped_samples);
    }
    assert_eq!(chunked.channels.len(), 2);
    close(chunked.true_peak_db, whole.true_peak_db, 1e-4);
    close(chunked.integrated_lufs, whole.integrated_lufs, 1e-3);
}

#[test]
fn g4_stats_silence_and_no_audio_read_the_floor() {
    let silent = amplitude_stats(&[vec![0.0f32; 48_000]], RATE);
    assert_eq!(silent.frames, 48_000);
    let ch = &silent.channels[0];
    close(ch.peak_db, -144.0, 0.001);
    close(ch.rms_db, -144.0, 0.001);
    close(ch.dc_offset_percent, 0.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);
    close(silent.true_peak_db, -144.0, 0.001);
    close(silent.integrated_lufs, -144.0, 0.001);

    let empty = amplitude_stats(&[], RATE);
    assert_eq!(empty.frames, 0);
    assert!(empty.channels.is_empty());
    close(empty.true_peak_db, -144.0, 0.001);
    close(empty.integrated_lufs, -144.0, 0.001);

    let unfed = AmplitudeScan::new(RATE, 2).finish();
    assert_eq!(unfed.frames, 0);
    assert_eq!(unfed.channels.len(), 2);
    for ch in &unfed.channels {
        close(ch.peak_db, -144.0, 0.001);
        close(ch.rms_db, -144.0, 0.001);
        close(ch.dc_offset_percent, 0.0, 0.0001);
        assert_eq!(ch.clipped_samples, 0);
    }
}

#[test]
fn g4_stats_non_finite_samples_count_as_zero() {
    let stats = amplitude_stats(&[vec![0.5, f32::NAN, f32::INFINITY, -0.5]], RATE);
    assert_eq!(stats.frames, 4);
    let ch = &stats.channels[0];
    close(ch.peak_db, -6.0206, 0.001);
    close(ch.rms_db, -9.0309, 0.001);
    close(ch.dc_offset_percent, 0.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);
}

#[test]
fn g4_stats_short_and_uneven_pushes() {
    let mut scan = AmplitudeScan::new(RATE, 2);
    // Fewer channels than the scan: ignored whole.
    scan.push(&[vec![0.9f32; 100]]);
    assert_eq!(scan.finish().frames, 0);
    // Uneven lengths: only the 60 frames both channels have are measured; a third channel is ignored.
    scan.push(&[vec![0.5f32; 100], vec![0.25f32; 60], vec![1.0f32; 100]]);
    let stats = scan.finish();
    assert_eq!(stats.frames, 60);
    assert_eq!(stats.channels.len(), 2);
    close(stats.channels[0].peak_db, -6.0206, 0.001);
    close(stats.channels[0].rms_db, -6.0206, 0.001);
    close(stats.channels[0].dc_offset_percent, 50.0, 0.0001);
    close(stats.channels[1].peak_db, -12.041201, 0.001);
    close(stats.channels[1].rms_db, -12.041201, 0.001);
    close(stats.channels[1].dc_offset_percent, 25.0, 0.0001);
    assert_eq!(stats.channels[0].clipped_samples, 0);
    assert_eq!(stats.channels[1].clipped_samples, 0);
}
