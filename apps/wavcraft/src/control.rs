//! A small control channel for scripted checks (the golden task): loopback only, one line per
//! request, one line per answer, no dependencies. Off unless `WAVCRAFT_CONTROL_PORT` is set.
//!
//! The first line on every connection must be `TOKEN <token>`, the contents of the file named
//! by `WAVCRAFT_CONTROL_TOKEN_FILE` (made with a fresh random token when missing). Then:
//!
//! - `STATE` → `OK frames=<n> channels=<n> rate=<hz> selection=<a>-<b> steps=<n> cursor=<i> busy=<0|1> undo=<label> redo=<label>`
//! - `OPEN <path>` → `OK` (the open runs in the background; poll `STATE` until `busy=0`)
//! - `SELECT <start> <end>` (frames) → `OK selection=<a>-<b>`
//! - `ACTION <name>` (an action table name, `edit.delete`) → `OK selection=<a>-<b> changed=<0|1>`
//! - `WINDOW minimize|restore` → `OK` (requests keep being answered while it is minimised)
//! - `INSERT_SILENCE <seconds>` → `OK selection=<a>-<b>` (Insert › Silence at the cursor)
//! - `HISTORY <step>` → `OK` (the History panel's click; 0 is Open)
//! - `SAVE [path]` → `OK` (Save, or Save As to `path`; poll `STATE` until `busy=0`)
//! - `QUIT` → `OK`, then the window closes; with unsaved changes `ERR …` unless `QUIT discard`
//!
//! Anything else, or an action that fails, answers `ERR <why>`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

/// One request for the window's thread, with where its answer goes.
pub struct Request {
    pub line: String,
    pub reply: Sender<String>,
}

/// Start listening on `127.0.0.1:<port>` when the environment asks for it. Each request is
/// handed to the window's thread through the returned receiver, and `wake` is called so an
/// idle window looks at it.
pub fn start_from_env(wake: impl Fn() + Send + Sync + 'static) -> Option<Receiver<Request>> {
    let port: u16 = std::env::var("WAVCRAFT_CONTROL_PORT").ok()?.trim().parse().ok()?;
    let token_file = std::env::var_os("WAVCRAFT_CONTROL_TOKEN_FILE")?;
    let token = match token(Path::new(&token_file)) {
        Ok(t) => t,
        Err(e) => {
            log::warn!(target: "wavcraft", "control channel: no token file at {}: {e}", Path::new(&token_file).display());
            return None;
        }
    };
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(l) => l,
        Err(e) => {
            log::warn!(target: "wavcraft", "control channel: cannot listen on 127.0.0.1:{port}: {e}");
            return None;
        }
    };
    log::info!(target: "wavcraft", "control channel on 127.0.0.1:{port}");
    let (tx, rx) = channel();
    let wake = std::sync::Arc::new(wake);
    let _ = std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, token, wake) = (tx.clone(), token.clone(), wake.clone());
            let _ = std::thread::Builder::new().name("control conn".into()).spawn(move || serve(stream, &token, &tx, &*wake));
        }
    });
    Some(rx)
}

/// The token in `path`, or a new random one written there (owner-only on Unix).
fn token(path: &Path) -> std::io::Result<String> {
    if let Ok(t) = std::fs::read_to_string(path)
        && !t.trim().is_empty()
    {
        return Ok(t.trim().to_string());
    }
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos()) as u64 ^ u64::from(std::process::id()) << 32;
    let mut t = String::new();
    for _ in 0..4 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        t.push_str(&format!("{:016x}", z ^ (z >> 31)));
    }
    write_private(path, &t)?;
    Ok(t)
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())
}

/// Windows: no owner-only mode without a new dependency. The file is written where the caller
/// put it, so callers keep it inside the user's own profile (AppData or a scratch folder there),
/// whose folder permissions are already the user's only; the golden task does.
#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

fn serve(stream: TcpStream, token: &str, tx: &Sender<Request>, wake: &(dyn Fn() + Send + Sync)) {
    let Ok(mut out) = stream.try_clone() else { return };
    let mut lines = BufReader::new(stream).lines();
    match lines.next() {
        Some(Ok(first)) if first.trim() == format!("TOKEN {token}") => {
            let _ = writeln!(out, "OK");
        }
        _ => {
            log::warn!(target: "wavcraft", "control channel: a connection from {} gave no valid token; closed", out.peer_addr().map_or_else(|_| "?".to_string(), |a| a.to_string()));
            let _ = writeln!(out, "ERR the first line must be TOKEN <token>");
            return;
        }
    }
    for line in lines.map_while(Result::ok) {
        let (reply, answer) = channel();
        if tx.send(Request { line: line.trim().to_string(), reply }).is_err() {
            return;
        }
        wake();
        let Ok(text) = answer.recv() else { return };
        if writeln!(out, "{text}").is_err() {
            return;
        }
    }
}
