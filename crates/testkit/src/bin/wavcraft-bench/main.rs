//! `wavcraft-bench FILE`: how fast and how big WavCraft is on one file, with no window.
//!
//! One line per measure: open time and the process's peak memory during it, memory held after
//! it, the waveform peaks' build time, the spectrogram overview's build time and the time of a
//! whole-file picture at 640, 1280 and 2048 columns (the spectrogram worker's own calls, channel
//! by channel, as the app makes them), and a full read of every sample in order. Then how open's
//! time splits between reading the bytes and decoding them.
//!
//! Run it in a release build; it needs about three times the file's size in memory for the
//! moment of opening (bytes + decoded samples), as the app does.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod memory;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use wavcraft_core::document::{StoreChannel, WavePeaks};
use wavcraft_core::spectrogram::{Columns, Overview, render_columns_from};
use wavcraft_core::{Document, SpectrogramParams};

/// Rows per channel of a picture: the most the app asks for.
const ROWS: usize = 384;
/// Frames per read, as a block of the planned document model holds.
const READ_CHUNK: usize = 65_536;

fn gb(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "n/a".to_string(), |b| format!("{:.2} GB", b as f64 / 1e9))
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Run `work` once to warm up, then `runs` times: the best and the median time in ms.
fn timed(runs: usize, mut work: impl FnMut()) -> (f64, f64) {
    work();
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            work();
            ms(t)
        })
        .collect();
    times.sort_by(f64::total_cmp);
    (times.first().copied().unwrap_or(0.0), times.get(times.len() / 2).copied().unwrap_or(0.0))
}

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: wavcraft-bench FILE");
        return ExitCode::FAILURE;
    };
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    println!("file: {} ({size} bytes)", path.display());
    println!("threads: {} (rayon)", rayon::current_num_threads());
    println!("working set at start: {}", gb(memory::read().now));

    // 1. Open, as the app does: read, decode, build one waveform overview per channel.
    let t = Instant::now();
    let doc = match Document::open(&path) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("wavcraft-bench: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let open_ms = ms(t);
    let after_open = memory::read();
    let (frames, channels, rate) = (doc.frames(), doc.channels(), doc.sample_rate());
    println!("opened: {:?} {:?}, {rate} Hz, {channels} ch, {frames} frames ({:.1} s)", doc.info.format, doc.info.sample_format, doc.duration_secs());
    println!("open (read + decode + peaks): {open_ms:.0} ms");
    println!("peak working set during open: {}", gb(after_open.peak));
    println!("working set after open: {}", gb(after_open.now));
    println!("samples as f32 (what the old whole-file copy held): {:.2} GB", (frames * channels * 4) as f64 / 1e9);

    // 2. Waveform peaks: built eagerly inside open; timed again alone, channels side by side as open does.
    let t = Instant::now();
    let peaks: Vec<WavePeaks> = std::thread::scope(|s| {
        let state = &doc.state;
        let jobs: Vec<_> = (0..channels).map(|ch| s.spawn(move || WavePeaks::build(state, ch))).collect();
        jobs.into_iter().filter_map(|job| job.join().ok().and_then(Result::ok)).collect()
    });
    println!("peaks build (inside open; timed again alone): {:.0} ms for {} channels", ms(t), peaks.len());
    drop(peaks);

    // 3. Spectrogram: the worker's calls. Overviews channel after channel, then each picture
    // channel after channel, all with the app's default analysis.
    let params = SpectrogramParams::default();
    let t = Instant::now();
    let mut per_channel = Vec::new();
    let sources: Vec<StoreChannel> = (0..channels).map(|ch| StoreChannel::new(&doc.state, ch)).collect();
    let overviews: Vec<Overview> = sources
        .iter()
        .map(|samples| {
            let c = Instant::now();
            let o = Overview::build_from(samples, &params);
            per_channel.push(format!("{:.0}", ms(c)));
            o
        })
        .collect();
    let overview_ms = ms(t);
    println!("working set after the overviews: {}", gb(memory::read().now));
    let overview_bytes: usize = overviews.iter().map(Overview::bytes).sum();
    println!(
        "spectrogram overview build: {overview_ms:.0} ms for {channels} ch ({} ms per channel), {:.1} MB",
        per_channel.join(" / "),
        overview_bytes as f64 / 1e6
    );
    for cols in [640usize, 1280, 2048] {
        let columns = Columns { start: 0.0, end: frames as f64, cols };
        let (best, median) = timed(5, || {
            for (samples, overview) in sources.iter().zip(&overviews) {
                std::hint::black_box(render_columns_from(samples, rate, columns, ROWS, &params, Some(overview)));
            }
        });
        println!(
            "spectrogram picture, whole file, {cols} x {ROWS} per channel: best {best:.1} ms, median {median:.1} ms for {channels} ch ({:.1} ms per channel)",
            median / channels.max(1) as f64
        );
    }
    let after_spectrogram = memory::read();
    drop(overviews);

    // 4. A full read of every sample in order, a block of frames at a time, into a scratch buffer.
    let t = Instant::now();
    let mut scratch = vec![0.0f32; READ_CHUNK];
    let mut sum = 0.0f64;
    for ch in 0..channels {
        let mut at = 0u64;
        while at < frames as u64 {
            let got = doc.state.read(ch, at, &mut scratch).unwrap_or(0);
            if got == 0 {
                break;
            }
            sum += scratch.iter().take(got).map(|v| f64::from(*v)).sum::<f64>();
            at += got as u64;
        }
    }
    let read_ms = ms(t);
    let bytes = (frames * channels * 4) as f64;
    println!(
        "full ordered read, {READ_CHUNK} frames at a time: {read_ms:.0} ms ({:.2} GB/s of f32; checksum {:.6})",
        bytes / 1e9 / (read_ms / 1000.0).max(1e-9),
        std::hint::black_box(sum)
    );
    println!("working set after the spectrogram: {}", gb(after_spectrogram.now));
    drop(doc);

    // 5. Where open's time goes: the same steps again, one at a time.
    let t = Instant::now();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("wavcraft-bench: cannot read {} again", path.display());
        return ExitCode::FAILURE;
    };
    let read_ms = ms(t);
    let t = Instant::now();
    let decoded = soundcraft_audio_io::decode(&bytes, path.extension().and_then(|e| e.to_str()));
    let decode_ms = ms(t);
    drop(bytes);
    drop(decoded);
    println!("open split: read the file {read_ms:.0} ms, decode {decode_ms:.0} ms (the rest of open is the peaks)");
    println!("peak working set, whole run: {}", gb(memory::read().peak));
    ExitCode::SUCCESS
}
