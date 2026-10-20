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
