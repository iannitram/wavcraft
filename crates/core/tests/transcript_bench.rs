//! L4.6 (#1060): Removing 600 fillers from an hour of 48 kHz stereo in a temp store. Ignored by
//! default (it writes about 1.4 GB to the temp folder); run it in release:
//!
//! `cargo test -p wavcraft-core --release --test transcript_bench -- --ignored --nocapture`
//!
//! It prints the milliseconds and asserts under 1,000 and exactly 1 new History step.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::transcript::delete_ranges;

const RATE: u32 = 48_000;
const SECONDS: u64 = 3_600;
const LABEL: &str = "Remove Fillers";
const FILLER_COUNT: usize = 600;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-transcript-bench-{}-{name}", std::process::id()));
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

/// 600 non-overlapping filler ranges spread across the 3,600 s episode (one 250 ms filler every 6 s).
fn fillers_600() -> Vec<Range<u64>> {
    let count = FILLER_COUNT as u64;
    let step = (SECONDS * u64::from(RATE)) / count;
    let len = u64::from(RATE) / 4;
    (0..count)
        .map(|i| {
            let start = i * step + u64::from(RATE);
            start..start + len
        })
        .collect()
}

// Catches: removing 600 fillers from an hour of audio taking 1 s or longer, or creating more than
// one History step.
#[test]
#[ignore = "writes 1.4 GB and edits an hour of audio; run in release with --ignored"]
fn removing_600_fillers_from_an_hour_takes_under_1_s() {
    let dir = TempDir::new("hour-fillers");
    let made = Instant::now();
    let state = hour_doc(&dir);
    eprintln!("wrote {SECONDS} s of 48 kHz stereo in {:.1} s", made.elapsed().as_secs_f64());

    let fillers = fillers_600();
    assert_eq!(fillers.len(), FILLER_COUNT);

    let mut history = History::new(state, u64::MAX);
    let steps_before = history.steps().len();

    let started = Instant::now();
    let edit = delete_ranges(history.current(), &fillers, Crossfades::default(), LABEL).unwrap();
    history.push_edit(edit);
    let elapsed = started.elapsed();
    let ms = elapsed.as_millis();

    println!("remove_600_fillers_ms={ms}");
    println!("{ms} ms");

    assert!(ms < 1_000, "removing 600 fillers took {ms} ms (limit 1,000 ms)");
    assert_eq!(history.steps().len(), steps_before + 1);
    assert_eq!(history.steps()[steps_before].label(), LABEL);

    let total_cut: u64 = fillers.iter().map(|r| r.end - r.start).sum();
    assert_eq!(history.current().len_frames(), (SECONDS * u64::from(RATE)) - total_cut);
}
