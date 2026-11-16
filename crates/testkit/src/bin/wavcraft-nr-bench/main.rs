//! `wavcraft-nr-bench CLEAN NOISY [--reduce-db N] [--reduction N] [--fft N] [--sessions DIR]`:
//! runs Noise Reduction on NOISY and measures it against CLEAN (G2.7). Prints three lines:
//! `floor_drop_db=<n>`, `speech_change_db=<n>` and `print_frames=<n>`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use wavcraft_core::DocState;
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_testkit::{NrSettings, nr_measure};

const USAGE: &str = "usage: wavcraft-nr-bench CLEAN NOISY [--reduce-db N] [--reduction N] [--fft N] [--sessions DIR]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("wavcraft-nr-bench: {why}");
            ExitCode::FAILURE
        }
    }
}

fn number<T: std::str::FromStr>(flag: &str, value: Option<std::ffi::OsString>) -> Result<T, String> {
    value.and_then(|v| v.to_str().and_then(|v| v.parse().ok())).ok_or_else(|| format!("{flag} needs a number"))
}

fn open_file(path: &Path, sessions: &Option<PathBuf>) -> Result<DocState, String> {
    let opts = OpenOptions { sessions_root: sessions.clone(), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let (mut files, mut sessions, mut settings) = (Vec::new(), None, NrSettings::default());
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--reduce-db") => settings.reduce_db = number("--reduce-db", args.next())?,
            Some("--reduction") => settings.reduction = number("--reduction", args.next())?,
            Some("--fft") => settings.fft_size = number("--fft", args.next())?,
            Some("--sessions") => sessions = args.next().map(PathBuf::from),
            _ => files.push(PathBuf::from(a)),
        }
    }
    let [clean, noisy] = files.as_slice() else { return Err(USAGE.to_string()) };
    let (clean, noisy) = (open_file(clean, &sessions)?, open_file(noisy, &sessions)?);
    let m = nr_measure(&clean, &noisy, &settings)?;
    println!("floor_drop_db={:.2}", m.floor_drop_db);
    println!("speech_change_db={:.2}", m.speech_change_db);
    println!("print_frames={}", m.print_frames);
    Ok(())
}
