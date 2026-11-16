#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! N3.7: the checker for the ten MCP tasks.
//!
//! `ten_tasks write <dir>` writes `source.wav` and the ten reference files `01-open.wav` to
//! `10-save.wav` into `<dir>`. The ten tasks are the ones `ten_tasks_headless` runs in
//! `crates/automation/tests/mcp_tasks.rs`: each is one `tools/call` line through
//! `wavcraft_automation::mcp::handle` on `Headless` with `Crossfades::NONE`, and each task
//! saves its file after its edit.
//!
//! `ten_tasks check <dir>` writes the same ten files into a scratch folder and compares each
//! of the ten names in `<dir>` with its reference, sample for sample. It prints `same=10` and
//! exits 0 when all ten match. Otherwise it prints `same=N differs=<names>` and exits 1.
//! A wrong argument or an error exits 2 and prints `error: ...` on standard error.
//!
//! The reference files are built here, not checked against the N3.3 assertions: this program
//! only builds files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};
use wavcraft_automation::Backend;
use wavcraft_automation::headless::Headless;
use wavcraft_automation::mcp::handle;
use wavcraft_core::doc::edit::Crossfades;

/// The ten saved files, in task order.
pub const NAMES: [&str; 10] = [
    "01-open.wav",
    "02-select.wav",
    "03-delete.wav",
    "04-silence.wav",
    "05-gain.wav",
    "06-normalize.wav",
    "07-fade-in.wav",
    "08-invert.wav",
    "09-marker.wav",
    "10-save.wav",
];

const RATE: u32 = 48_000;
const FRAMES: u32 = 20_000;
/// Frames per comparison chunk: the checker never holds a whole file in memory.
const CHUNK: usize = 4096;
const USAGE: &str = "usage: ten_tasks write <dir> | ten_tasks check <dir>";

/// A 16-bit, 48000 Hz, stereo WAV, `FRAMES` long. Channel 0 at frame i is `100 * ((i % 100) + 1)`
/// (100 to 10000, never 0); channel 1 is its negative.
fn write_wav(path: &Path) -> Result<(), String> {
    let data_len = FRAMES * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..FRAMES {
        let v = (100 * ((i % 100) + 1)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&(-v).to_le_bytes());
    }
    std::fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
}

/// One headless backend, and the id of the last `tools/call` sent through it.
struct Run {
    head: Headless,
    id: u64,
}

impl Run {
    /// Send one `tools/call`. `label` names the task in an error message.
    fn tool(&mut self, label: &str, name: &str, args: Value) -> Result<(), String> {
        self.id += 1;
        let line = json!({"jsonrpc": "2.0", "id": self.id, "method": "tools/call",
            "params": {"name": name, "arguments": args}})
        .to_string();
        let out = handle(&line, &mut self.head).ok_or_else(|| format!("task {label}: {name} gave no reply"))?;
        let res: Value = serde_json::from_str(&out).map_err(|e| format!("task {label}: the reply to {name} is not JSON: {e}"))?;
        if res["result"]["isError"] != false {
            return Err(format!("task {label}: {name} failed: {res}"));
        }
        Ok(())
    }

    fn select(&mut self, label: &str, start: u64, end: u64) -> Result<(), String> {
        self.tool(label, "select", json!({"start": start, "end": end}))
    }

    /// Save the open document as `dir/name`.
    fn save(&mut self, dir: &Path, name: &str) -> Result<(), String> {
        let path = dir.join(name);
        self.tool(name, "save", json!({"path": path.to_string_lossy()}))
    }
}

/// Run the ten tasks into `dir`, with `sessions` as the scratch sessions folder.
fn run_tasks(dir: &Path, sessions: &Path) -> Result<(), String> {
    let mut run = Run { head: Headless::with_crossfades(sessions, Crossfades::NONE), id: 0 };
    let source = dir.join("source.wav");

    // 1. open
    run.tool(NAMES[0], "open", json!({"path": source.to_string_lossy()}))?;
    run.save(dir, NAMES[0])?;

    // 2. select
    run.select(NAMES[1], 1000, 2000)?;
    run.save(dir, NAMES[1])?;

    // 3. delete
    run.tool(NAMES[2], "action", json!({"name": "edit.delete"}))?;
    run.save(dir, NAMES[2])?;

    // 4. silence
    run.select(NAMES[3], 0, 500)?;
    run.tool(NAMES[3], "action", json!({"name": "edit.silence"}))?;
    run.save(dir, NAMES[3])?;

    // 5. gain
    run.select(NAMES[4], 500, 1000)?;
    run.tool(NAMES[4], "effect", json!({"name": "gain", "args": "gain_db=-6.0206"}))?;
    run.save(dir, NAMES[4])?;

    // 6. normalize: an empty selection runs on the whole file
    run.select(NAMES[5], 0, 0)?;
    run.tool(NAMES[5], "effect", json!({"name": "normalize", "args": "unit=dB db=-2"}))?;
    run.save(dir, NAMES[5])?;

    // 7. fade in
    run.select(NAMES[6], 0, 1000)?;
    run.tool(NAMES[6], "action", json!({"name": "effect.fadeIn.linear"}))?;
    run.save(dir, NAMES[6])?;

    // 8. invert
    run.select(NAMES[7], 2000, 3000)?;
    run.tool(NAMES[7], "effect", json!({"name": "invert"}))?;
    run.save(dir, NAMES[7])?;

    // 9. add marker
    run.select(NAMES[8], 5000, 5000)?;
    run.tool(NAMES[8], "action", json!({"name": "marker.add"}))?;
    run.save(dir, NAMES[8])?;

    // 10. save, with no edit
    run.save(dir, NAMES[9])
}

/// Write `source.wav` and the ten reference files into `dir`, which must be an existing folder.
pub fn write_all(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    write_wav(&dir.join("source.wav"))?;
    let sessions = std::env::temp_dir().join(format!("wavcraft-ten-tasks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sessions);
    std::fs::create_dir_all(&sessions).map_err(|e| format!("make {}: {e}", sessions.display()))?;
    let result = run_tasks(dir, &sessions);
    let _ = std::fs::remove_dir_all(&sessions);
    result
}

/// Open `file` in a fresh headless backend that keeps its sessions under `sessions`. `None` when
/// the file cannot be opened.
fn open_reader(file: &Path, sessions: &Path) -> Option<Headless> {
    let mut head = Headless::with_crossfades(sessions, Crossfades::NONE);
    let reply = head.answer_line(&format!("OPEN {}", file.display()));
    (reply == "OK").then_some(head)
}

/// Whether two open documents have the same channels, frames and 16-bit samples.
fn same_samples(got: &Headless, want: &Headless) -> Result<bool, String> {
    let (Some(g), Some(w)) = (got.document(), want.document()) else {
        return Ok(false);
    };
    let frames = g.len_frames();
    if g.channel_count() != w.channel_count() || frames != w.len_frames() {
        return Ok(false);
    }
    let mut a = vec![0.0f32; CHUNK];
    let mut b = vec![0.0f32; CHUNK];
    let mut start = 0u64;
    while start < frames {
        let n = CHUNK.min((frames - start) as usize);
        for ch in 0..g.channel_count() {
            let na = g.read(ch, start, &mut a[..n]).map_err(|e| format!("{e:?}"))?;
            let nb = w.read(ch, start, &mut b[..n]).map_err(|e| format!("{e:?}"))?;
            if na != n || nb != n {
                return Ok(false);
            }
            for (x, y) in a[..n].iter().zip(&b[..n]) {
                if (x * 32_768.0).round() as i32 != (y * 32_768.0).round() as i32 {
                    return Ok(false);
                }
            }
        }
        start += n as u64;
    }
    Ok(true)
}

/// Compare the ten files in `dir` with fresh references. Returns how many match and the names
/// that differ (a missing or unreadable file counts as differing).
fn check_in(dir: &Path, scratch: &Path) -> Result<(usize, Vec<String>), String> {
    let reference = scratch.join("reference");
    let sessions = scratch.join("sessions");
    std::fs::create_dir_all(&reference).map_err(|e| format!("make {}: {e}", reference.display()))?;
    std::fs::create_dir_all(&sessions).map_err(|e| format!("make {}: {e}", sessions.display()))?;
    write_all(&reference)?;
    let mut same = 0;
    let mut differs = Vec::new();
    for name in NAMES {
        let want = open_reader(&reference.join(name), &sessions).ok_or_else(|| format!("the reference {name} did not open"))?;
        let matches = match open_reader(&dir.join(name), &sessions) {
            Some(got) => same_samples(&got, &want)?,
            None => false,
        };
        if matches {
            same += 1;
        } else {
            differs.push(name.to_string());
        }
    }
    Ok((same, differs))
}

/// Check the ten files in `dir`. Err only when `dir` is not a folder.
pub fn check(dir: &Path) -> Result<(usize, Vec<String>), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let scratch = std::env::temp_dir().join(format!("wavcraft-ten-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let result = check_in(dir, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// The line `check` prints, and whether every file matched.
pub fn report(dir: &Path) -> Result<(bool, String), String> {
    let (same, differs) = check(dir)?;
    if differs.is_empty() {
        return Ok((true, format!("same={same}")));
    }
    Ok((false, format!("same={same} differs={}", differs.join(","))))
}

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let (command, dir) = match args.as_slice() {
        [command, dir] => (command.to_str(), PathBuf::from(dir)),
        _ => {
            eprintln!("error: wrong arguments");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Some("write") => match write_all(&dir) {
            Ok(()) => {
                println!("wrote={}", NAMES.len());
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("error: {why}");
                ExitCode::from(2)
            }
        },
        Some("check") => match report(&dir) {
            Ok((true, line)) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Ok((false, line)) => {
                println!("{line}");
                ExitCode::from(1)
            }
            Err(why) => {
                eprintln!("error: {why}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("error: unknown command");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
