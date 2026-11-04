//! Amplitude statistics on a document (A313, A316): the streamed scan over a piece table, with
//! exact expected values on a synthetic signal, a selection, cancel and progress.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::stats::{self, DEFAULT_RMS_WINDOW_MS};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-stats-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn doc(dir: &TempDir, data: &[Vec<f32>], block: usize) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(block).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

/// 16-bit steps: left holds DC 0.25 plus a clipped run at 100_000..100_010 and a peak of 0.75
/// at 30_000; right is a quiet constant 1/64 with a single full-scale sample (not a run).
fn kit() -> Vec<Vec<f32>> {
    let n = 200_000;
    let mut l = vec![0.25f32; n];
    l[30_000] = 0.75;
    l[100_000..100_010].fill(1.0);
    let mut r = vec![1.0f32 / 64.0; n];
    r[150_000] = 1.0;
    vec![l, r]
}

#[test]
fn a_scan_over_the_document_has_the_exact_numbers() {
    let dir = TempDir::new("exact");
    let d = doc(&dir, &kit(), 10_007);
    let mut last = 0.0;
    let st = stats::scan(&d, 0..d.len_frames(), DEFAULT_RMS_WINDOW_MS, &mut |p| last = p, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(last, 1.0);
    assert_eq!(st.channels.len(), 2);
    let (l, r) = (&st.channels[0], &st.channels[1]);
    assert_eq!((l.peak, l.peak_frame), (1.0, 100_000));
    assert_eq!(l.peak_db(), 0.0);
    assert_eq!((l.min, l.max), (0.25, 1.0));
    assert_eq!((l.clipped_runs, l.clipped_samples, l.clip_starts.clone()), (1, 10, vec![100_000]));
    let n = 200_000.0f64;
    let mean = (0.25 * (n - 11.0) + 0.75 + 10.0) / n;
    assert!((l.dc_offset - mean).abs() < 1e-12);
    assert_eq!((r.clipped_runs, r.peak_frame), (0, 150_000), "one full-scale sample is not a run");
    assert_eq!(l.bit_depth, 3, "steps of 0.25");
    assert_eq!(r.bit_depth, 7, "steps of 1/64");
    assert_eq!(st.peak_location(), Some((0, 100_000)));
    assert_eq!(st.next_clip_after(0), Some(100_000));
    assert_eq!(st.prev_clip_before(100_001), Some(100_000));
    assert_eq!(st.prev_clip_before(100_000), None);
}

#[test]
fn a_selection_is_measured_with_document_positions() {
    let dir = TempDir::new("sel");
    let d = doc(&dir, &kit(), 4096);
    let st = stats::scan(&d, 90_000..110_000, 50, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(st.range, 90_000..110_000);
    let l = &st.channels[0];
    assert_eq!(l.frames, 20_000);
    assert_eq!(l.clip_starts, vec![100_000]);
    assert_eq!(st.channels[1].peak, 1.0 / 64.0, "the full-scale right sample is outside");
    // Windows of 2400 frames from 90_000: the run at 100_000 is in window 4 (96_000..98_400 is
    // window 2; 100_800 starts window 4), so the loudest window RMS is above the quiet 0.25.
    assert!(l.rms_max > 0.25 && l.rms_min == 0.25, "{} {}", l.rms_min, l.rms_max);
}

#[test]
fn a_range_past_the_end_is_clamped_and_a_cancel_stops_the_scan() {
    let dir = TempDir::new("clamp");
    let d = doc(&dir, &kit(), 4096);
    let st = stats::scan(&d, 199_000..900_000, 50, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(st.range, 199_000..200_000);
    let stop = AtomicBool::new(false);
    let mut calls = 0;
    let out = stats::scan(
        &d,
        0..200_000,
        50,
        &mut |_| {
            calls += 1;
            stop.store(true, Ordering::Relaxed);
        },
        &stop,
    )
    .unwrap();
    assert!(out.is_none());
    assert_eq!(calls, 1, "stopped after the first chunk");
}

/// `dc + amp * sin(2 pi f n / RATE)` for `frames` frames.
fn sine(dc: f64, amp: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (dc + amp * (2.0 * std::f64::consts::PI * hz * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn scan_all(d: &DocState) -> stats::Stats {
    stats::scan(d, 0..d.len_frames(), DEFAULT_RMS_WINDOW_MS, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap()
}

fn close(got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol, "got {got}, want {want} (within {tol})");
}

#[test]
fn a_stereo_sine_reads_its_peak_and_rms() {
    // 1 kHz at amplitude 0.1 on both channels, 3 s: -20 dBFS peak, -23.01 dBFS RMS.
    let dir = TempDir::new("sine");
    let tone = sine(0.0, 0.1, 1000.0, 144_000);
    let st = scan_all(&doc(&dir, &[tone.clone(), tone], 4800));
    assert_eq!(st.channels.len(), 2);
    for ch in &st.channels {
        assert_eq!(ch.frames, 144_000);
        close(ch.peak_db(), -20.0, 0.01);
        close(ch.rms_total_db(), -23.0103, 0.01);
        close(ch.dc_percent(), 0.0, 0.0001);
        assert_eq!(ch.clipped_samples, 0);
    }
}

#[test]
fn an_offset_sine_reads_its_dc_offset_and_rms() {
    // 0.25 + 0.5 sin(1 kHz), 1 s, mono: peak 0.75, mean 0.25, RMS sqrt(0.1875).
    let dir = TempDir::new("dc");
    let st = scan_all(&doc(&dir, &[sine(0.25, 0.5, 1000.0, 48_000)], 4800));
    let ch = &st.channels[0];
    close(ch.peak_db(), -2.4987748, 0.001);
    close(ch.rms_total_db(), -7.269987, 0.001);
    close(ch.dc_percent(), 25.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);
}

#[test]
fn clipped_samples_count_runs_per_channel() {
    // Live behaviour: only runs of 2 or more samples at 0.9999 or beyond count; a lone one does not.
    let mut left = vec![0.0f32; 1000];
    left[10..13].fill(1.0);
    left[500] = -1.0;
    left[600] = 0.9999;
    left[700] = 1.5;
    let mut right = vec![0.0f32; 1000];
    right[3] = -1.0;
    let dir = TempDir::new("clipcount");
    let st = scan_all(&doc(&dir, &[left, right], 300));
    let (l, r) = (&st.channels[0], &st.channels[1]);
    assert_eq!((l.clipped_runs, l.clipped_samples), (1, 3));
    assert_eq!((r.clipped_runs, r.clipped_samples), (0, 0));
    close(l.peak_db(), 3.521825, 0.001);
    close(r.peak_db(), 0.0, 0.001);
    close(l.rms_total_db(), -21.39674, 0.001);
    close(r.rms_total_db(), -30.0, 0.001);
    close(l.dc_percent(), 0.44999, 0.0001);
    close(r.dc_percent(), -0.1, 0.0001);
}

#[test]
fn block_size_does_not_change_the_numbers() {
    let left = sine(0.0, 0.1, 1000.0, 96_000);
    let right = sine(0.01, 0.3, 440.0, 96_000);
    let (d1, d2) = (TempDir::new("blk-a"), TempDir::new("blk-b"));
    let whole = scan_all(&doc(&d1, &[left.clone(), right.clone()], 60_000));
    let chunked = scan_all(&doc(&d2, &[left, right], 4800));
    close(whole.channels[1].dc_percent(), 1.0, 0.001);
    for (a, b) in chunked.channels.iter().zip(&whole.channels) {
        assert_eq!(a.frames, b.frames);
        close(a.peak_db(), b.peak_db(), 1e-6);
        close(a.rms_total_db(), b.rms_total_db(), 1e-4);
        close(a.dc_percent(), b.dc_percent(), 1e-4);
        assert_eq!(a.clipped_samples, b.clipped_samples);
    }
}

#[test]
fn silence_and_an_empty_document_read_the_floor() {
    let dir = TempDir::new("silent");
    let st = scan_all(&doc(&dir, &[vec![0.0f32; 48_000]], 4800));
    let ch = &st.channels[0];
    assert_eq!(ch.frames, 48_000);
    assert!(ch.peak_db() <= -143.0, "{}", ch.peak_db());
    assert!(ch.rms_total_db() <= -143.0, "{}", ch.rms_total_db());
    close(ch.dc_percent(), 0.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);

    let dir = TempDir::new("empty");
    let st = scan_all(&doc(&dir, &[vec![], vec![]], 4800));
    assert_eq!(st.channels.len(), 2);
    for ch in &st.channels {
        assert_eq!(ch.frames, 0);
        assert_eq!(ch.clipped_samples, 0);
    }
}

#[test]
fn non_finite_samples_count_as_zero() {
    let dir = TempDir::new("nan");
    let st = scan_all(&doc(&dir, &[vec![0.5, f32::NAN, f32::INFINITY, -0.5]], 4));
    let ch = &st.channels[0];
    assert_eq!(ch.frames, 4);
    close(ch.peak_db(), -6.0206, 0.001);
    close(ch.rms_total_db(), -9.0309, 0.001);
    close(ch.dc_percent(), 0.0, 0.0001);
    assert_eq!(ch.clipped_samples, 0);
}
