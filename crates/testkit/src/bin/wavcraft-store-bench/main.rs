//! `wavcraft-store-bench FILE [--cache-mb N] [--sessions DIR] [--huge]`: task 03's measures.
//!
//! Streamed open into a session store (time, and the process's peak memory above what it held
//! before), a sequential read of the whole document 64 K frames at a time (block cache cleared
//! first, then again), and 10,000 random 4,096-frame reads (median, 99th percentile, worst).
//! `--huge` also checks the huge kit file's frame counter at its far end.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[path = "../wavcraft-bench/memory.rs"]
mod memory;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::store::StoreOptions;
use wavcraft_testkit::{HUGE_FRAMES, Rng, huge_counter};

const CHUNK: usize = 65_536;
const RANDOM_READS: usize = 10_000;
const RANDOM_LEN: usize = 4_096;

fn gb(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "n/a".to_string(), |b| format!("{:.3} GB", b as f64 / 1e9))
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn fail(why: &str) -> ExitCode {
    eprintln!("wavcraft-store-bench: {why}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (mut path, mut cache_mb, mut sessions, mut huge) = (None, 512u64, None, false);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--cache-mb") => match args.next().and_then(|v| v.to_str().and_then(|v| v.parse().ok())) {
                Some(n) => cache_mb = n,
                None => return fail("--cache-mb needs a number"),
            },
            Some("--sessions") => sessions = args.next().map(PathBuf::from),
            Some("--huge") => huge = true,
            _ => path = Some(PathBuf::from(a)),
        }
    }
    let Some(path) = path else { return fail("usage: wavcraft-store-bench FILE [--cache-mb N] [--sessions DIR] [--huge]") };
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    println!("file: {} ({size} bytes)", path.display());
    let rest = memory::read();
    println!("working set at rest: {}", gb(rest.now));

    // 1. Streamed open.
    let opts = OpenOptions { sessions_root: sessions, store: StoreOptions { cache_bytes: cache_mb << 20, ..StoreOptions::default() } };
    let t = Instant::now();
    let doc = match open(&path, &opts, &mut |_| {}, &AtomicBool::new(false)) {
        Ok(d) => d,
        Err(e) => return fail(&format!("{}: {e}", path.display())),
    };
    let open_s = t.elapsed().as_secs_f64();
    let after = memory::read();
    let store = doc.store().clone();
    let (frames, channels, rate) = (doc.len_frames(), doc.channel_count(), doc.sample_rate());
    println!(
        "opened: {:?} {:?} rf64={} {rate} Hz, {channels} ch, {frames} frames ({:.1} s), {} pieces per channel, warnings {:?}",
        doc.source().format,
        doc.source().sample_format,
        doc.source().rf64,
        frames as f64 / f64::from(rate),
        doc.piece_count(0),
        doc.source().warnings
    );
    println!("session folder: {}", store.dir().display());
    println!(
        "streamed open: {open_s:.2} s; store written {} ({:.2} GB/s of f32)",
        gb(Some(store.bytes_on_disk())),
        store.bytes_on_disk() as f64 / 1e9 / open_s.max(1e-9)
    );
    let above = after.peak.zip(rest.now).map(|(p, r)| p.saturating_sub(r));
    println!(
        "peak working set during open: {} = {} above at rest; working set after open: {}",
        gb(after.peak),
        above.map_or_else(|| "n/a".into(), mb),
        gb(after.now)
    );
    println!("cache after open: {:?}", store.cache_stats());

    // 2. Sequential read of everything, 64 K frames at a time, block cache cleared first.
    let mut buf = vec![0.0f32; CHUNK];
    for run in ["first (block cache cleared)", "second (block cache cleared again)"] {
        store.clear_cache();
        let t = Instant::now();
        let mut sum = 0.0f64;
        for ch in 0..channels {
            let mut at = 0u64;
            while at < frames {
                let n = match doc.read(ch, at, &mut buf) {
                    Ok(n) => n,
                    Err(e) => return fail(&format!("read: {e}")),
                };
                sum += buf.iter().take(n).map(|v| f64::from(*v)).sum::<f64>();
                at += n as u64;
            }
        }
        let s = t.elapsed().as_secs_f64();
        println!(
            "sequential read, {run}: {:.0} ms ({:.2} GB/s of f32; checksum {:.3}); working set {}",
            s * 1000.0,
            (frames * channels as u64 * 4) as f64 / 1e9 / s.max(1e-9),
            std::hint::black_box(sum),
            gb(memory::read().now)
        );
    }

    // 3. Random reads.
    let mut rng = Rng::new(3, "store-bench");
    let mut out = vec![0.0f32; RANDOM_LEN];
    let mut times = Vec::with_capacity(RANDOM_READS);
    let before = store.cache_stats();
    for _ in 0..RANDOM_READS {
        let ch = rng.below(channels as u64) as usize;
        let start = rng.below(frames.saturating_sub(RANDOM_LEN as u64).max(1));
        let t = Instant::now();
        if let Err(e) = doc.read(ch, start, &mut out) {
            return fail(&format!("random read: {e}"));
        }
        times.push(t.elapsed().as_secs_f64() * 1e6);
        std::hint::black_box(&out);
    }
    times.sort_by(f64::total_cmp);
    let pick = |q: f64| times.get(((times.len() - 1) as f64 * q) as usize).copied().unwrap_or(0.0);
    let st = store.cache_stats();
    println!(
        "{RANDOM_READS} random {RANDOM_LEN}-frame reads, cache {cache_mb} MB: median {:.1} us, p90 {:.1} us, p99 {:.1} us, worst {:.1} us; hits {} misses {}",
        pick(0.5),
        pick(0.9),
        pick(0.99),
        pick(1.0),
        st.hits - before.hits,
        st.misses - before.misses
    );
    println!("cache: {st:?}");

    // 4. The huge file: length and the counter at its far end.
    if huge {
        println!("expected {HUGE_FRAMES} frames: {}", if frames == HUGE_FRAMES { "right" } else { "WRONG" });
        let mut bad = 0;
        for at in [0, 1 << 24, frames / 2, frames - 1_000] {
            let mut v = vec![0.0f32; 1_000];
            let n = doc.read(1, at, &mut v).unwrap_or(0);
            bad += v.iter().take(n).enumerate().filter(|(i, x)| **x * 8_388_608.0 != huge_counter(at + *i as u64) as f32).count();
        }
        println!("counter check at four places incl. the last 1,000 frames: {} wrong samples", bad);
    }
    println!("peak working set, whole run: {}", gb(memory::read().peak));
    let t = Instant::now();
    drop(doc);
    drop(store);
    println!("closing (session folder removed): {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    ExitCode::SUCCESS
}
