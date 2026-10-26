//! RMS settings (roadmap A315, core part): the sine / square reference offset, the account-for-DC
//! option and the window width, on exact hand-computed signals. Expected numbers:
//! - a full-scale square wave has RMS 1, a full-scale sine 1/sqrt(2), so the sine reference
//!   offset is 20 * log10(sqrt(2)) = 3.010299956639812 dB;
//! - [0.75, 0.25] repeated has mean 0.5 and mean square 0.3125: RMS sqrt(0.3125) =
//!   0.5590169943749475, and 0.25 once the mean is removed (0.3125 - 0.25 = 0.0625).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::stats::{self, ChannelScan, ChannelStats, RmsReference, RmsSettings};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;
const SINE_DB: f64 = 3.010299956639812;

fn run(data: &[f32], window: u64, chunk: usize, dc: bool) -> ChannelStats {
    let mut s = ChannelScan::new(0, window).with_account_for_dc(dc);
    for c in data.chunks(chunk.max(1)) {
        s.add(c);
    }
    s.finish()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn the_default_settings_are_the_old_behaviour() {
    assert_eq!(RmsReference::default(), RmsReference::Square);
    let d = RmsSettings::default();
    assert_eq!(d.window_ms, 50);
    assert!(!d.account_for_dc);
    assert_eq!(d.reference, RmsReference::Square);
}

#[test]
fn the_reference_offsets_come_from_the_wave_shapes() {
    assert_eq!(RmsReference::Square.offset_db(), 0.0);
    assert!((RmsReference::Sine.offset_db() - SINE_DB).abs() < 1e-12);
}

#[test]
fn rms_in_db_is_relative_to_the_reference() {
    // 0.5: -6.020599913279624 dBFS, so -3.010299956639812 against a sine.
    assert!((RmsReference::Square.db(0.5) - -6.020599913279624).abs() < 1e-12);
    assert!((RmsReference::Sine.db(0.5) - -3.010299956639812).abs() < 1e-12);
    // A full-scale sine (RMS 1/sqrt(2)) reads 0 dB against a sine and -3.01 against a square.
    let sine_rms = std::f64::consts::FRAC_1_SQRT_2;
    assert!(RmsReference::Sine.db(sine_rms).abs() < 1e-12);
    assert!((RmsReference::Square.db(sine_rms) - -SINE_DB).abs() < 1e-12);
    // A full-scale square reads 0 against a square and +3.01 against a sine.
    assert_eq!(RmsReference::Square.db(1.0), 0.0);
    assert!((RmsReference::Sine.db(1.0) - SINE_DB).abs() < 1e-12);
    // Silence stays silence.
    assert_eq!(RmsReference::Square.db(0.0), f64::NEG_INFINITY);
    assert_eq!(RmsReference::Sine.db(0.0), f64::NEG_INFINITY);
}

#[test]
fn a_scanned_full_scale_sine_and_square_read_zero_against_their_own_reference() {
    let sine: Vec<f32> = (0..800).map(|i| (f64::from(i) * std::f64::consts::FRAC_PI_4).sin() as f32).collect();
    let c = run(&sine, 8, 800, false);
    assert!(RmsReference::Sine.db(c.rms_total).abs() < 1e-5, "{}", RmsReference::Sine.db(c.rms_total));
    assert!((RmsReference::Square.db(c.rms_total) - -SINE_DB).abs() < 1e-5);
    assert!(RmsReference::Sine.db(c.rms_avg).abs() < 1e-5);
    let square: Vec<f32> = (0..800).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
    let c = run(&square, 8, 800, false);
    assert_eq!(RmsReference::Square.db(c.rms_total), 0.0);
    assert!((RmsReference::Sine.db(c.rms_total) - SINE_DB).abs() < 1e-12);
}

#[test]
fn accounting_for_dc_removes_the_mean_from_every_rms_figure() {
    let data: Vec<f32> = (0..100).flat_map(|_| [0.75f32, 0.25]).collect();
    let off = run(&data, 2, 100, false);
    assert_eq!(off.windows, 100);
    for v in [off.rms_min, off.rms_max, off.rms_avg, off.rms_total] {
        assert!(close(v, 0.5590169943749475), "{v}");
    }
    let on = run(&data, 2, 100, true);
    assert_eq!(on.windows, 100);
    for v in [on.rms_min, on.rms_max, on.rms_avg, on.rms_total] {
        assert!(close(v, 0.25), "{v}");
    }
    assert_eq!(on.dc_offset, 0.5, "the DC figure itself is unchanged");
    assert_eq!(on.peak, 0.75);
}

#[test]
fn window_rms_uses_the_window_mean_and_total_rms_uses_the_whole_mean() {
    // Two windows of 4: +0.5 then -0.5. Each window is pure DC (RMS 0); the whole has mean 0.
    let mut d = vec![0.5f32; 4];
    d.extend([-0.5f32; 4]);
    let on = run(&d, 4, 8, true);
    assert_eq!((on.windows, on.rms_min, on.rms_max, on.rms_avg), (2, 0.0, 0.0, 0.0));
    assert!(close(on.rms_total, 0.5));
    let off = run(&d, 4, 8, false);
    assert_eq!((off.rms_min, off.rms_max, off.rms_avg, off.rms_total), (0.5, 0.5, 0.5, 0.5));
    // Windows of +0.5 then +1.0: each window is pure DC; the whole has mean 0.75 and mean square
    // 0.625, so the total is sqrt(0.625 - 0.5625) = 0.25.
    let mut d = vec![0.5f32; 4];
    d.extend([1.0f32; 4]);
    let on = run(&d, 4, 8, true);
    assert_eq!((on.rms_min, on.rms_max, on.rms_avg), (0.0, 0.0, 0.0));
    assert!(close(on.rms_total, 0.25), "{}", on.rms_total);
}

#[test]
fn a_part_window_that_is_all_there_is_also_removes_its_mean() {
    // [0.75, 0.25, 0.75]: mean 7/12, mean square 1.1875 / 3; RMS sqrt(0.395833.. - 0.340277..).
    let c = run(&[0.75, 0.25, 0.75], 100, 2, true);
    assert_eq!(c.windows, 1);
    assert!(close(c.rms_min, 0.2357022603955158), "{}", c.rms_min);
    assert!(close(c.rms_total, 0.2357022603955158));
}

#[test]
fn rounding_never_makes_the_rms_not_a_number() {
    // 0.7 a hundred times: mean square minus squared mean comes out slightly below zero in f64.
    let c = run(&[0.7f32; 100], 100, 100, true);
    assert!(c.rms_total.is_finite() && c.rms_total < 1e-6, "{}", c.rms_total);
    assert!(c.rms_avg.is_finite() && c.rms_avg < 1e-6, "{}", c.rms_avg);
    assert!(c.rms_min.is_finite() && c.rms_min < 1e-6);
    assert!(c.rms_max.is_finite() && c.rms_max < 1e-6);
}

#[test]
fn the_dc_option_does_not_depend_on_the_chunk_size() {
    let data: Vec<f32> = (0..1000).map(|i| ((i * 37 % 200) as f32 - 60.0) / 128.0).collect();
    let whole = run(&data, 64, 1000, true);
    for chunk in [1, 7, 64, 65, 999] {
        assert_eq!(run(&data, 64, chunk, true), whole, "chunk {chunk}");
    }
    assert_ne!(whole, run(&data, 64, 1000, false), "this signal has DC, so the option matters");
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-rms-settings-test-{}-{name}", std::process::id()));
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

/// Left alternates 0.75 / 0.25 (mean 0.5), right is a constant 0.5, 200_000 frames each.
fn kit() -> Vec<Vec<f32>> {
    let n = 200_000;
    let l: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.75 } else { 0.25 }).collect();
    vec![l, vec![0.5f32; n]]
}

#[test]
fn scan_with_the_default_settings_equals_scan() {
    let dir = TempDir::new("default");
    let d = doc(&dir, &kit(), 10_007);
    let old = stats::scan(&d, 0..d.len_frames(), 50, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    let new = stats::scan_with(&d, 0..d.len_frames(), &RmsSettings::default(), &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(old, new);
    assert_eq!(new.window_frames, 2400);
    assert!(close(new.channels[0].rms_total, 0.5590169943749475));
    assert!(close(new.channels[1].rms_total, 0.5));
}

#[test]
fn scan_with_threads_the_window_and_the_dc_option_into_the_scan() {
    let dir = TempDir::new("dc");
    let d = doc(&dir, &kit(), 4096);
    let settings = RmsSettings { window_ms: 25, account_for_dc: true, reference: RmsReference::Sine };
    let st = stats::scan_with(&d, 0..d.len_frames(), &settings, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(st.window_frames, 1200, "25 ms at 48 kHz");
    let (l, r) = (&st.channels[0], &st.channels[1]);
    assert_eq!(l.windows, 166, "200_000 / 1200, the part-window is ignored");
    for v in [l.rms_min, l.rms_max, l.rms_avg, l.rms_total] {
        assert!(close(v, 0.25), "{v}");
    }
    assert_eq!(l.dc_offset, 0.5);
    for v in [r.rms_min, r.rms_max, r.rms_avg, r.rms_total] {
        assert_eq!(v, 0.0, "a constant is all DC");
    }
    // The same window without the option keeps the plain RMS.
    let plain = RmsSettings { account_for_dc: false, ..settings };
    let st = stats::scan_with(&d, 0..d.len_frames(), &plain, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(st.window_frames, 1200);
    assert!(close(st.channels[0].rms_total, 0.5590169943749475));
    assert!(close(st.channels[1].rms_max, 0.5));
}

#[test]
fn scan_with_cancels_like_scan() {
    let dir = TempDir::new("cancel");
    let d = doc(&dir, &kit(), 4096);
    let out = stats::scan_with(&d, 0..d.len_frames(), &RmsSettings::default(), &mut |_| {}, &AtomicBool::new(true)).unwrap();
    assert!(out.is_none());
}

#[test]
fn scan_still_takes_its_own_window_and_leaves_the_mean_in() {
    let dir = TempDir::new("scanwin");
    let d = doc(&dir, &kit(), 4096);
    let st = stats::scan(&d, 0..d.len_frames(), 25, &mut |_| {}, &AtomicBool::new(false)).unwrap().unwrap();
    assert_eq!(st.window_frames, 1200);
    assert!(close(st.channels[0].rms_total, 0.5590169943749475), "scan keeps the DC in");
    let same =
        stats::scan_with(&d, 0..d.len_frames(), &RmsSettings { window_ms: 25, ..RmsSettings::default() }, &mut |_| {}, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
    assert_eq!(st, same);
}
