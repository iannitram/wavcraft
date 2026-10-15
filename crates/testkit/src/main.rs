//! `wavcraft-testkit OUT_DIR [--long] [--seed N] [--speech CLEAN.wav --noise NOISE.wav]`
//!
//! Writes WavCraft's synthetic test files into OUT_DIR (keep it outside the repo). Same seed,
//! same bytes. `--long` adds the 3-hour file (about 2 GB). `--speech` and `--noise` together add
//! mixes at 10 dB and 20 dB SNR plus the clean reference; without them nothing is mixed.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use wavcraft_testkit::{KitOptions, write_kit};

const USAGE: &str = "usage: wavcraft-testkit OUT_DIR [--long] [--seed N] [--speech CLEAN.wav --noise NOISE.wav]";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let mut dir: Option<PathBuf> = None;
    let mut opts = KitOptions::default();
    let (mut speech, mut noise) = (None, None);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--long") => opts.long = true,
            Some("--seed") => match args.next().and_then(|v| v.to_str().and_then(|v| v.parse::<u64>().ok())) {
                Some(seed) => opts.seed = seed,
                None => return fail("--seed needs a whole number"),
            },
            Some("--speech") => speech = args.next().map(PathBuf::from),
            Some("--noise") => noise = args.next().map(PathBuf::from),
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Some(other) if other.starts_with("--") => return fail(&format!("unknown option {other}")),
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => return fail("more than one output folder given"),
        }
    }
    let Some(dir) = dir else { return fail("no output folder given") };
    match (speech, noise) {
        (Some(s), Some(n)) => opts.speech = Some((s, n)),
        (None, None) => {}
        _ => return fail("--speech and --noise go together"),
    }
    let started = Instant::now();
    match write_kit(&dir, &opts) {
        Ok(paths) => {
            for path in &paths {
                let size = std::fs::metadata(path).map_or(0, |m| m.len());
                println!("{:>14} bytes  {}", size, path.display());
            }
            println!("{} files in {:.1} s (seed {})", paths.len(), started.elapsed().as_secs_f64(), opts.seed);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("writing the kit into {}: {e}", dir.display())),
    }
}

fn fail(why: &str) -> ExitCode {
    eprintln!("wavcraft-testkit: {why}\n{USAGE}");
    ExitCode::FAILURE
}
