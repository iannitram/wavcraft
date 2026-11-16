//! `wavcraft-rack-bench [--seconds N]`: times a rack of Parametric EQ, Dynamics and De-esser over
//! N seconds of generated stereo 48 kHz audio (default 3600, one hour) and prints one line,
//! `rack_3fx_1h_seconds=<n>`: the time that one hour of audio would take.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn fail(why: &str) -> ExitCode {
    eprintln!("wavcraft-rack-bench: {why}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let mut seconds = 3600u64;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--seconds") => {
                seconds = args
                    .next()
                    .and_then(|v| v.to_str().and_then(|v| v.parse::<u64>().ok()))
                    .filter(|&n| n > 0)
                    .ok_or("--seconds needs a positive number")?;
            }
            _ => return Err("usage: wavcraft-rack-bench [--seconds N]".into()),
        }
    }
    println!("{}", wavcraft_testkit::rack_bench_line(seconds)?);
    Ok(())
}
