//! K1.8: Episode Check on an hour of 48 kHz stereo read from a temp store. Ignored by default (it
//! writes about 1.4 GB to the temp folder); run it in release:
//!
//! `cargo test -p wavcraft-core --release --test episode_check_bench -- --ignored --nocapture`
//!
//! It prints the scan time, the peak memory growth over the scan (Linux `VmHWM`, reset just before
//! the scan; elsewhere not measured) and the cancel latency, and checks them against v1 K1: under 60 s, under 200 MB and
//! under 1 s. The store's block cache is a fixed budget the open document already holds, so it is
//! filled before the baseline is taken; what is measured is what the scan adds.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wavcraft_core::check;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const SECONDS: u64 = 3_600;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-check-bench-{}-{name}", std::process::id()));
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

/// Peak resident memory so far, in bytes (Linux only).
fn high_water() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Start the peak over from the current resident size (Linux 4.0 and later), so the next
/// [`high_water`] is the peak since now and not the peak of writing the test file. False when the
/// system does not allow it (the growth may then read low).
fn reset_high_water() -> bool {
    std::fs::write("/proc/self/clear_refs", "5").is_ok()
}

/// An hour of stereo written block by block (never all in memory): a 220 Hz and a 330 Hz tone
/// at -12 dBFS whose level swings slowly, over a little noise.
fn hour_doc(dir: &TempDir) -> DocState {
    let store = Store::create_with(&dir.0, StoreOptions::default()).unwrap();
    let total = SECONDS * u64::from(RATE);
    let mut pieces: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    let mut lcg = 1u32;
    let mut buf = vec![0f32; BLOCK_FRAMES];
    let mut at = 0u64;
    while at < total {
        let n = (total - at).min(BLOCK_FRAMES as u64) as usize;
        for (ch, hz) in [220.0, 330.0].into_iter().enumerate() {
            for (i, x) in buf[..n].iter_mut().enumerate() {
                let t = (at + i as u64) as f64 / f64::from(RATE);
                lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (f64::from(lcg >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * 0.003;
                let level = 0.25 * (0.8 + 0.2 * (2.0 * std::f64::consts::PI * 0.3 * t).sin());
                *x = (level * (2.0 * std::f64::consts::PI * hz * t).sin() + noise) as f32;
            }
            pieces[ch].push(Piece::whole(store.append(&buf[..n]).unwrap()).unwrap());
        }
        at += n as u64;
    }
    let [l, r] = pieces;
    DocState::new(store, vec![Channel::from_pieces(l), Channel::from_pieces(r)], RATE, SourceFormat::synthetic(RATE, 2)).unwrap()
}

/// How long the scan takes to return after cancel is set at the first progress past `at`.
fn cancel_latency(state: &DocState, at: f64) -> Duration {
    let cancel = AtomicBool::new(false);
    let mut set: Option<Instant> = None;
    let out = check::scan_doc(state, &cancel, &mut |p| {
        if set.is_none() && p > at {
            cancel.store(true, Ordering::Relaxed);
            set = Some(Instant::now());
        }
    })
    .unwrap();
    let done = Instant::now();
    assert!(out.is_none(), "a cancelled scan gives no report");
    done - set.unwrap()
}

#[test]
#[ignore = "writes 1.4 GB and scans an hour of audio; run in release with --ignored"]
fn one_hour_stereo_scan_is_fast_and_small() {
    let dir = TempDir::new("hour");
    let made = Instant::now();
    let state = hour_doc(&dir);
    eprintln!("wrote {SECONDS} s of 48 kHz stereo in {:.1} s", made.elapsed().as_secs_f64());

    // Fill the block cache to its budget, as an open document's would be.
    let mut buf = vec![0f32; BLOCK_FRAMES];
    let mut at = 0;
    while at < state.len_frames() {
        for ch in 0..2 {
            state.read(ch, at, &mut buf).unwrap();
        }
        at += BLOCK_FRAMES as u64;
    }

    if !reset_high_water() {
        eprintln!("could not reset VmHWM: the memory growth is measured from the peak so far");
    }
    let before = high_water();
    let started = Instant::now();
    let report = check::scan_doc(&state, &AtomicBool::new(false), &mut |_| {}).unwrap().unwrap();
    let secs = started.elapsed().as_secs_f64();
    let growth_mb = before.zip(high_water()).map(|(b, a)| a.saturating_sub(b) as f64 / 1e6);

    // Cancel once in the main loop and once in the passes; the slower counts.
    let latency = cancel_latency(&state, 0.25).max(cancel_latency(&state, 0.75)).as_secs_f64();

    eprintln!("findings: {}", report.findings.len());
    println!("scan_seconds={secs:.2}");
    match growth_mb {
        Some(mb) => println!("memory_growth_mb={mb:.1}"),
        None => println!("memory_growth_mb=not measured on this system"),
    }
    println!("cancel_latency_seconds={latency:.6}");

    assert!(secs < 60.0, "the scan took {secs:.1} s (limit 60 s)");
    if let Some(mb) = growth_mb {
        assert!(mb < 200.0, "the scan added {mb:.1} MB (limit 200 MB)");
    }
    assert!(latency < 1.0, "cancel took {latency:.3} s (limit 1 s)");
}
