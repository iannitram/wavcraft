//! `wavcraft-edit-bench FILE [--cache-mb N] [--sessions DIR] [--edits N]`: task 04's measures.
//!
//! On a long file (the 3-hour kit file): Cut one hour out of the middle, undo it, paste the hour
//! back; `replace_range` (a gain) over 1 minute and over the whole file; then N random edits
//! (default 200) pushed on the history, with the push time; the working set after them; undo all
//! and a check that the samples are bit-identical to the opened file.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[path = "../wavcraft-bench/memory.rs"]
mod memory;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::store::StoreOptions;
use wavcraft_core::{DocState, History};
use wavcraft_testkit::{DEFAULT_SEED, RandomEdit, Rng, content_hash, random_edit};

fn gb(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "n/a".to_string(), |b| format!("{:.3} GB", b as f64 / 1e9))
}

fn mb(bytes: i128) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1e3)
}

fn fail(why: &str) -> ExitCode {
    eprintln!("wavcraft-edit-bench: {why}");
    ExitCode::FAILURE
}

fn now_bytes() -> i128 {
    i128::from(memory::read().now.unwrap_or(0))
}

fn gain(doc: &DocState, range: std::ops::Range<u64>, g: f32) -> Result<edit::Edit, edit::EditError> {
    edit::replace_range(doc, range, ChannelMask::ALL, "Amplify", Crossfades::default(), &AtomicBool::new(false), |_, i, o| {
        for (a, b) in i.iter().zip(o.iter_mut()) {
            for (x, y) in a.iter().zip(b.iter_mut()) {
                *y = x * g;
            }
        }
        Ok(())
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let (mut path, mut cache_mb, mut sessions, mut edits) = (None, 512u64, None, 200usize);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--cache-mb") => cache_mb = args.next().and_then(|v| v.to_str().and_then(|v| v.parse().ok())).ok_or("--cache-mb needs a number")?,
            Some("--edits") => edits = args.next().and_then(|v| v.to_str().and_then(|v| v.parse().ok())).ok_or("--edits needs a number")?,
            Some("--sessions") => sessions = args.next().map(PathBuf::from),
            _ => path = Some(PathBuf::from(a)),
        }
    }
    let path = path.ok_or("usage: wavcraft-edit-bench FILE [--cache-mb N] [--sessions DIR] [--edits N]")?;
    let total = Instant::now();
    let rest = memory::read();
    println!("file: {}; block cache {cache_mb} MB; working set at rest: {}", path.display(), gb(rest.now));
    let opts = OpenOptions {
        sessions_root: sessions,
        store: StoreOptions { cache_bytes: cache_mb << 20, ..StoreOptions::default() },
        ..OpenOptions::default()
    };
    let t = Instant::now();
    let doc = open(&path, &opts, &mut |_| {}, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
    let store = doc.store().clone();
    store.flush().map_err(|e| e.to_string())?;
    let (len, rate) = (doc.len_frames(), u64::from(doc.sample_rate()));
    println!(
        "open: {:.2} s, {len} frames, {} pieces per channel, store {}",
        t.elapsed().as_secs_f64(),
        doc.piece_count(0),
        gb(Some(store.bytes_on_disk()))
    );
    let t = Instant::now();
    let want = content_hash(&doc).map_err(|e| e.to_string())?;
    println!("hash of the opened document: {:.2} s", t.elapsed().as_secs_f64());
    let cap = History::default_cap(store.dir());
    println!("history disk cap: {}", gb(Some(cap)));
    let mut h = History::new(doc, cap);

    // Cut one hour out of the middle, with and without crossfades.
    let hour = 3_600 * rate;
    let mid = (len / 2).saturating_sub(hour / 2)..(len / 2).saturating_sub(hour / 2) + hour;
    for (name, xf) in [("crossfades 0", Crossfades::NONE), ("default crossfades", Crossfades::default())] {
        let disk = store.bytes_on_disk();
        let t = Instant::now();
        let (_ed, _clip) = edit::cut(h.current(), mid.clone(), ChannelMask::ALL, xf).map_err(|e| e.to_string())?;
        let cut = t.elapsed();
        store.flush().map_err(|e| e.to_string())?;
        println!("cut 1 h from the middle ({name}): {}, store grew {} bytes", ms(cut), store.bytes_on_disk().saturating_sub(disk));
    }
    let t = Instant::now();
    let (ed, clip) = edit::cut(h.current(), mid.clone(), ChannelMask::ALL, Crossfades::default()).map_err(|e| e.to_string())?;
    let t_cut = t.elapsed();
    let t = Instant::now();
    h.push_edit(ed);
    let t_push = t.elapsed();
    let t = Instant::now();
    h.undo().ok_or("undo")?;
    let t_undo = t.elapsed();
    h.redo().ok_or("redo")?;
    let t = Instant::now();
    let ed = edit::paste(h.current(), PasteAt::Insert(mid.start), &clip, ChannelMask::ALL, Crossfades::default()).map_err(|e| e.to_string())?;
    let t_paste = t.elapsed();
    h.push_edit(ed);
    println!("cut + push: {} + {}; undo of the cut: {}; paste the hour back: {}", ms(t_cut), ms(t_push), ms(t_undo), ms(t_paste));
    println!("pieces per channel after paste-back: {}", h.current().piece_count(0));
    while h.undo().is_some() {}
    drop(clip);

    // replace_range over 1 minute, both channels.
    for (name, range) in [("1 minute", 3_600 * rate..3_660 * rate), ("the whole file", 0..len)] {
        let disk = store.bytes_on_disk();
        let mem = now_bytes();
        let t = Instant::now();
        let ed = gain(h.current(), range, 0.5).map_err(|e| e.to_string())?;
        let took = t.elapsed();
        store.flush().map_err(|e| e.to_string())?;
        let m = memory::read();
        println!(
            "replace_range over {name} (gain, both channels): {:.3} s, store grew {}, working set {} ({} vs before), peak working set so far {}",
            took.as_secs_f64(),
            gb(Some(store.bytes_on_disk().saturating_sub(disk))),
            gb(m.now),
            mb(i128::from(m.now.unwrap_or(0)) - mem),
            gb(m.peak)
        );
        h.push_edit(ed);
        h.undo().ok_or("undo")?;
    }

    // Random edits, as test 10.
    let mut rng = Rng::new(DEFAULT_SEED, "task-04-long");
    let mut clipboard = None;
    let mut done: Vec<RandomEdit> = Vec::new();
    let t = Instant::now();
    for _ in 0..edits {
        done.push(random_edit(&mut h, &mut rng, Crossfades::default(), 60 * rate, &mut clipboard).map_err(|e| e.to_string())?);
    }
    println!("{edits} random edits: {:.2} s", t.elapsed().as_secs_f64());
    if !done.is_empty() {
        let mut by: std::collections::BTreeMap<&str, Vec<Duration>> = std::collections::BTreeMap::new();
        for e in &done {
            by.entry(e.label.as_str()).or_default().push(e.edit);
        }
        for (label, mut times) in by {
            times.sort();
            println!("  {label:>14}: {:3}, median {}, slowest {}", times.len(), ms(times[times.len() / 2]), ms(*times.last().unwrap_or(&Duration::ZERO)));
        }
        let mut push: Vec<Duration> = done.iter().map(|e| e.push).collect();
        push.sort();
        let last = done.last().map(|e| (e.behind, e.push));
        println!(
            "  push: median {}, slowest {}; last push ({:?} steps behind): {:?}",
            ms(push[push.len() / 2]),
            ms(*push.last().unwrap_or(&Duration::ZERO)),
            last.map(|l| l.0),
            last.map(|l| ms(l.1))
        );
        if let Some((i, e)) = done.iter().enumerate().max_by_key(|(_, e)| e.push) {
            println!("  slowest push: edit {i} ({}, {} steps behind)", e.label, e.behind);
        }
    } else {
        println!("  no edits run");
    }
    // One more push, timed on its own, with every step behind it.
    let behind = h.cursor();
    let now_len = h.current().len_frames();
    println!("  document now {now_len} frames, {} pieces per channel", h.current().piece_count(0));
    let ed = edit::delete(h.current(), now_len / 3..now_len / 3 + rate, ChannelMask::ALL, Crossfades::default()).map_err(|e| e.to_string())?;
    let t = Instant::now();
    let dropped = h.push_edit(ed);
    println!("  push of a 1 s Delete with {behind} steps behind it: {} ({dropped} dropped)", ms(t.elapsed()));
    h.undo().ok_or("undo")?;
    store.flush().map_err(|e| e.to_string())?;
    let m = memory::read();
    println!(
        "after {edits} edits, history kept ({} steps, {} dropped): working set {}, peak {}; store on disk {}, live {}, history holds {} beyond the current state",
        h.steps().len(),
        h.dropped_total(),
        gb(m.now),
        gb(m.peak),
        gb(Some(store.bytes_on_disk())),
        gb(Some(store.bytes_live())),
        gb(Some(h.held_bytes()))
    );
    let t = Instant::now();
    let mut slowest = Duration::ZERO;
    loop {
        let u = Instant::now();
        if h.undo().is_none() {
            break;
        }
        slowest = slowest.max(u.elapsed());
    }
    println!("undo all: {}, slowest {}", ms(t.elapsed()), ms(slowest));
    let got = content_hash(h.current()).map_err(|e| e.to_string())?;
    println!("bit-identical to the opened file after undo all: {}", got == want);
    println!("total: {:.1} s", total.elapsed().as_secs_f64());
    if got != want {
        return Err("undo all is not bit-identical".into());
    }
    Ok(())
}
