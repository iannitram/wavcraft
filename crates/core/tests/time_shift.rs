//! Time Shift: delay a range by a set time, length-preserving, streamed across chunks.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, TimeShift};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-tshift-test-{}-{name}", std::process::id()));
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

fn doc(dir: &std::path::Path, rate: u32, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap()
}

fn read(d: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn run(d: &DocState, ms: f64, range: std::ops::Range<u64>, mask: ChannelMask) -> wavcraft_core::doc::edit::Edit {
    effects::apply(&TimeShift, &[ms], d, range, mask, Crossfades::NONE, &AtomicBool::new(false)).unwrap()
}

#[test]
fn an_impulse_shifted_10_ms_at_48k_lands_exactly_480_frames_later() {
    let tmp = TempDir::new("impulse");
    let mut x = vec![0.0f32; 4000];
    x[1000] = 1.0;
    let d = doc(&tmp.0, 48_000, &[x]);
    let e = run(&d, 10.0, 0..0, ChannelMask::ALL);
    assert_eq!(e.label, "Time Shift");
    let got = read(&e.state, 0);
    assert_eq!(got.len(), 4000);
    assert_eq!(got[1480], 1.0);
    assert_eq!(got.iter().filter(|v| **v != 0.0).count(), 1);
}

#[test]
fn a_shift_longer_than_a_chunk_is_exact_across_three_chunks() {
    let tmp = TempDir::new("ramp");
    let frames = BLOCK_FRAMES * 3;
    let ramp: Vec<f32> = (0..frames).map(|i| (i + 1) as f32).collect();
    let d = doc(&tmp.0, 48_000, std::slice::from_ref(&ramp));
    let shift = 70_000usize;
    let e = run(&d, shift as f64 * 1000.0 / 48_000.0, 0..0, ChannelMask::ALL);
    let got = read(&e.state, 0);
    for (f, v) in got.iter().enumerate() {
        let want = if f < shift { 0.0 } else { ramp[f - shift] };
        assert_eq!(*v, want, "frame {f}");
    }
}

#[test]
fn only_the_masked_channel_moves() {
    let tmp = TempDir::new("mask");
    let a: Vec<f32> = (0..3000).map(|i| (i + 1) as f32).collect();
    let b: Vec<f32> = (0..3000).map(|i| -((i + 1) as f32)).collect();
    let d = doc(&tmp.0, 48_000, &[a.clone(), b.clone()]);
    let e = run(&d, 10.0, 0..0, ChannelMask::only(1));
    assert_eq!(read(&e.state, 0), a);
    let got = read(&e.state, 1);
    assert!(got[..480].iter().all(|v| *v == 0.0));
    assert_eq!(got[480..], b[..2520]);
}

#[test]
fn a_range_shifts_inside_itself_and_loses_its_tail() {
    let tmp = TempDir::new("range");
    let a: Vec<f32> = (0..2000).map(|i| (i + 1) as f32).collect();
    let d = doc(&tmp.0, 48_000, std::slice::from_ref(&a));
    let e = run(&d, 1.0, 500..1500, ChannelMask::ALL); // 48 frames
    let got = read(&e.state, 0);
    assert_eq!(got[..500], a[..500]);
    assert!(got[500..548].iter().all(|v| *v == 0.0));
    assert_eq!(got[548..1500], a[500..1452]);
    assert_eq!(got[1500..], a[1500..]);
}

#[test]
fn shift_zero_is_bit_identical() {
    let tmp = TempDir::new("zero");
    let a: Vec<f32> = (0..BLOCK_FRAMES + 99).map(|i| (i as f32 * 0.37).sin()).collect();
    let d = doc(&tmp.0, 44_100, std::slice::from_ref(&a));
    let e = run(&d, 0.0, 0..0, ChannelMask::ALL);
    assert_eq!(read(&e.state, 0), a);
}

#[test]
fn one_undo_restores() {
    let tmp = TempDir::new("undo");
    let a: Vec<f32> = (0..5000).map(|i| (i + 1) as f32).collect();
    let d = doc(&tmp.0, 48_000, std::slice::from_ref(&a));
    let mut h = History::new(d, u64::MAX);
    let e = run(h.current(), 10.0, 0..0, ChannelMask::ALL);
    h.push_edit(e);
    assert_eq!(h.undo_label(), Some("Time Shift"));
    assert_ne!(read(h.current(), 0), a);
    let back = h.undo().unwrap();
    assert_eq!(read(&back, 0), a);
}

#[test]
fn parse_params_refuses_20000_ms_and_accepts_the_limit() {
    assert!(effects::parse_params(&TimeShift, "shift_ms=20000").is_err());
    assert!(effects::parse_params(&TimeShift, "shift_ms=-1").is_err());
    assert_eq!(effects::parse_params(&TimeShift, "shift_ms=10000ms").unwrap(), vec![10_000.0]);
    assert_eq!(effects::parse_params(&TimeShift, "").unwrap(), vec![0.0]);
    assert!(effects::find("time_shift").is_some());
}
