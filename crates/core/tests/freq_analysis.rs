//! Frequency Analysis (roadmap A309-A312): the spectrum of a moment or a range, in dB with 0 dB
//! a full-scale sine, on synthetic audio.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::ChannelMask;
use wavcraft_core::analysis::freq::{self, FftWindow, MAX_FFT, MIN_FFT};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-freq-test-{}-{name}", std::process::id()));
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

/// A document holding `data` (one `Vec` per channel), written in blocks of 10 000 frames so a
/// scan crosses piece boundaries.
fn doc_from(dir: &TempDir, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(&dir.0, StoreOptions { pack_bytes: 8 << 20, cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans =
        data.iter().map(|c| Channel::from_pieces(c.chunks(10_000.min(BLOCK_FRAMES)).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn sine(hz: f32, amp: f32, frames: usize) -> Vec<f32> {
    (0..frames).map(|i| amp * (TAU * hz * i as f32 / RATE as f32).sin()).collect()
}

/// Deterministic white noise in -1..1.
fn noise(frames: usize, seed: u64) -> Vec<f32> {
    let mut s = seed;
    (0..frames)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        })
        .collect()
}

#[test]
fn a_full_scale_sine_peaks_at_its_frequency_at_0_db() {
    let dir = TempDir::new("sine");
    let doc = doc_from(&dir, &[sine(1000.0, 1.0, 20_000)]);
    let sp = freq::spectrum_at(&doc, 10_000, ChannelMask::ALL, 4096, FftWindow::Hann).unwrap();
    assert_eq!(sp.bins(), 2049);
    let p = sp.peak(0, 0.0, 24_000.0).unwrap();
    let bin_hz = RATE as f32 / 4096.0;
    assert!((p.hz - 1000.0).abs() <= bin_hz, "peak at {} Hz", p.hz);
    assert!((p.db - 0.0).abs() <= 0.5, "peak level {} dB", p.db);
    // The raw bin is within a scalloping loss of full scale.
    let raw = sp.channels[0].iter().cloned().fold(f32::MIN, f32::max);
    assert!(raw > -1.5 && raw <= 0.1, "raw {raw}");
}

#[test]
fn two_tones_are_found_at_their_levels() {
    let dir = TempDir::new("two");
    let a = sine(440.0, db_to_amp(-6.0), 100_000);
    let b = sine(5000.0, db_to_amp(-20.0), 100_000);
    let mix: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
    let doc = doc_from(&dir, &[mix]);
    let sp = freq::scan(&doc, 0..100_000, ChannelMask::ALL, 4096, FftWindow::Hann, &AtomicBool::new(false)).unwrap().unwrap();
    let low = sp.peak(0, 300.0, 600.0).unwrap();
    let high = sp.peak(0, 4000.0, 6000.0).unwrap();
    assert!((low.hz - 440.0).abs() < 12.0 && (low.db + 6.0).abs() <= 1.0, "{low:?}");
    assert!((high.hz - 5000.0).abs() < 12.0 && (high.db + 20.0).abs() <= 1.0, "{high:?}");
}

fn db_to_amp(d: f32) -> f32 {
    10f32.powf(d / 20.0)
}

#[test]
fn white_noise_over_several_chunks_is_flat() {
    let dir = TempDir::new("noise");
    let frames = 3 * RATE as usize;
    assert!(frames > 2 * BLOCK_FRAMES, "the scan must span several chunks");
    let doc = doc_from(&dir, &[noise(frames, 7)]);
    let sp = freq::scan(&doc, 0..frames as u64, ChannelMask::ALL, 1024, FftWindow::Hann, &AtomicBool::new(false)).unwrap().unwrap();
    let hz = |bin: usize| bin as f32 * RATE as f32 / 1024.0;
    let band: Vec<f32> = (0..sp.bins()).filter(|&b| (100.0..=20_000.0).contains(&hz(b))).map(|b| sp.channels[0][b]).collect();
    let mean = band.iter().sum::<f32>() / band.len() as f32;
    for (i, v) in band.iter().enumerate() {
        assert!((v - mean).abs() <= 3.0, "bin {i}: {v} dB against a mean of {mean} dB");
    }
}

#[test]
fn a_scan_ignores_audio_outside_its_range() {
    let dir = TempDir::new("range");
    // 1 kHz in the first second, silence after; scan only the second second.
    let mut data = sine(1000.0, 1.0, RATE as usize);
    data.extend(std::iter::repeat_n(0.0, RATE as usize));
    let doc = doc_from(&dir, &[data]);
    let sp =
        freq::scan(&doc, RATE as u64 + 1000..2 * RATE as u64, ChannelMask::ALL, 4096, FftWindow::Hann, &AtomicBool::new(false)).unwrap().unwrap();
    let p = sp.peak(0, 0.0, 24_000.0).unwrap();
    assert!(p.db < -120.0, "the tone outside the range showed up: {p:?}");
    // And the converse: scanning the first second finds it.
    let sp = freq::scan(&doc, 0..RATE as u64, ChannelMask::ALL, 4096, FftWindow::Hann, &AtomicBool::new(false)).unwrap().unwrap();
    assert!((sp.peak(0, 0.0, 24_000.0).unwrap().hz - 1000.0).abs() < 12.0);
}

#[test]
fn cancel_returns_no_result() {
    let dir = TempDir::new("cancel");
    let doc = doc_from(&dir, &[noise(200_000, 3)]);
    let cancel = AtomicBool::new(true);
    assert!(freq::scan(&doc, 0..200_000, ChannelMask::ALL, 2048, FftWindow::Hann, &cancel).unwrap().is_none());
    cancel.store(false, Ordering::Relaxed);
    assert!(freq::scan(&doc, 0..200_000, ChannelMask::ALL, 2048, FftWindow::Hann, &cancel).unwrap().is_some());
}

#[test]
fn hann_leaks_much_less_than_rectangular_ten_bins_away() {
    let dir = TempDir::new("leak");
    // A tone between bins, so the rectangular window leaks widely.
    let f = (100.5) * RATE as f32 / 4096.0;
    let doc = doc_from(&dir, &[sine(f, 1.0, 20_000)]);
    let at = |w| {
        let sp = freq::spectrum_at(&doc, 10_000, ChannelMask::ALL, 4096, w).unwrap();
        sp.channels[0][100 + 10]
    };
    let (rect, hann) = (at(FftWindow::Rectangular), at(FftWindow::Hann));
    assert!(rect - hann >= 20.0, "rectangular {rect} dB, Hann {hann} dB");
    let blackman = at(FftWindow::Blackman);
    assert!(rect - blackman >= 20.0, "blackman {blackman} dB");
}

#[test]
fn channels_and_their_mean_are_reported() {
    let dir = TempDir::new("channels");
    let doc = doc_from(&dir, &[sine(1000.0, 1.0, 20_000), sine(3000.0, 0.5, 20_000)]);
    let sp = freq::spectrum_at(&doc, 10_000, ChannelMask::ALL, 2048, FftWindow::Hann).unwrap();
    assert_eq!(sp.channels.len(), 2);
    assert!((sp.peak(0, 0.0, 24_000.0).unwrap().hz - 1000.0).abs() < 24.0);
    assert!((sp.peak(1, 0.0, 24_000.0).unwrap().hz - 3000.0).abs() < 24.0);
    // The mean is of power: both tones show up in it, the louder 6 dB above the quieter.
    let m = sp.mean_peak(900.0, 1100.0).unwrap().db - sp.mean_peak(2900.0, 3100.0).unwrap().db;
    assert!((m - 6.0).abs() < 1.5, "{m}");
    // Only channel 1.
    let one = freq::spectrum_at(&doc, 10_000, ChannelMask::only(1), 2048, FftWindow::Hann).unwrap();
    assert_eq!(one.channels.len(), 1);
    assert_eq!(one.channel_ids, vec![1]);
}

#[test]
fn odd_requests_are_refused_or_padded() {
    let dir = TempDir::new("odd");
    let doc = doc_from(&dir, &[sine(1000.0, 1.0, 3000)]);
    for bad in [0, 100, 255, 1000, MAX_FFT * 2] {
        assert!(freq::spectrum_at(&doc, 0, ChannelMask::ALL, bad, FftWindow::Hann).is_err(), "{bad}");
    }
    assert!(freq::spectrum_at(&doc, 0, ChannelMask::ALL, MIN_FFT, FftWindow::Hann).is_ok());
    assert!(freq::spectrum_at(&doc, 0, ChannelMask::ALL, MAX_FFT, FftWindow::Hann).is_ok());
    // A cursor at the very start, past the end, and a range shorter than the FFT all work.
    assert!(freq::spectrum_at(&doc, 99_999, ChannelMask::ALL, 1024, FftWindow::Hann).is_ok());
    assert!(freq::scan(&doc, 0..100, ChannelMask::ALL, 1024, FftWindow::Hann, &AtomicBool::new(false)).unwrap().is_some());
    assert!(freq::scan(&doc, 5..5, ChannelMask::ALL, 1024, FftWindow::Hann, &AtomicBool::new(false)).is_err());
    assert!(freq::scan(&doc, 0..9_000_000, ChannelMask::ALL, 1024, FftWindow::Hann, &AtomicBool::new(false)).is_err());
    assert!(freq::scan(&doc, 0..100, ChannelMask::NONE, 1024, FftWindow::Hann, &AtomicBool::new(false)).is_err());
}

#[test]
fn graph_data_is_tab_separated_hz_and_db_per_channel() {
    let dir = TempDir::new("copy");
    let doc = doc_from(&dir, &[sine(1000.0, 1.0, 5000), sine(2000.0, 1.0, 5000)]);
    let sp = freq::spectrum_at(&doc, 2500, ChannelMask::ALL, 256, FftWindow::Hann).unwrap();
    let text = sp.graph_data();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Channel 1");
    assert_eq!(lines[1], "Hz\tdB");
    assert!(lines[2].starts_with("0.0\t"), "{}", lines[2]);
    assert!(lines.contains(&"Channel 2"));
    assert!(lines.iter().filter(|l| l.contains('\t') && l.split('\t').all(|f| f.parse::<f32>().is_ok())).count() >= 2 * 129);
}
