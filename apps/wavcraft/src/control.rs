//! A small control channel for scripted checks (the golden task): loopback only, one line per
//! request, one line per answer, no dependencies. Off unless `WAVCRAFT_CONTROL_PORT` is set.
//!
//! The first line on every connection must be `TOKEN <token>`, the contents of the file named
//! by `WAVCRAFT_CONTROL_TOKEN_FILE` (made owner-only with a CSPRNG token when missing; an
//! existing one must be owner-only, not a link, 64+ hex digits; see `token`). Then:
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

/// The token in `path`, or a new one written there (Codex review CX-W4/W5, 2026-10-19):
/// - a new token is 32 bytes from the operating system's CSPRNG as 64 hex digits; no random
///   bytes, no control channel (fail closed);
/// - it is written with exclusive create (never through a link), owner-only: mode 0600 on Unix,
///   an owner-only ACL on Windows (`icacls`), and the file is removed if that fails;
/// - an existing file is used only when it is a plain file (not a link), only its owner can read
///   it, and it holds at least 64 hex digits. Anything else is refused.
fn token(path: &Path) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind};
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.file_type().is_file() {
                return Err(Error::new(ErrorKind::InvalidInput, "the token file is a link or not a file; refused"));
            }
            check_private(path, &meta)?;
            let t = std::fs::read_to_string(path)?.trim().to_string();
            if t.len() < 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::new(ErrorKind::InvalidData, "the token file does not hold a token of 64 hex digits; refused"));
            }
            Ok(t)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let t = wavcraft_core::random::hex(32)?;
            write_private(path, &t)?;
            Ok(t)
        }
        Err(e) => Err(e),
    }
}

/// Unix: only the owner may read or write it (no group or other bits).
#[cfg(unix)]
fn check_private(_path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "others can read the token file; refused (make it 0600)"));
    }
    Ok(())
}

/// Windows: the ACL is made owner-only again (an existing file may come from elsewhere).
#[cfg(not(unix))]
fn check_private(path: &Path, _meta: &std::fs::Metadata) -> std::io::Result<()> {
    owner_only_acl(path)
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())
}

/// Windows: exclusive create, owner-only ACL before the token goes in; on failure the file goes.
#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(e) = owner_only_acl(path) {
        drop(f);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    f.write_all(text.as_bytes())
}

/// Windows: remove inherited entries and grant only the current user (`icacls`, part of
/// Windows; no new dependency).
#[cfg(not(unix))]
fn owner_only_acl(path: &Path) -> std::io::Result<()> {
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) if !d.is_empty() && !u.is_empty() => format!("{d}\\{u}"),
        (_, Ok(u)) if !u.is_empty() => u,
        _ => return Err(std::io::Error::other("no user name for the token file's ACL")),
    };
    let out = std::process::Command::new("icacls").arg(path).args(["/inheritance:r", "/grant:r"]).arg(format!("{user}:F")).output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "icacls could not make the token file owner-only: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::token;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-token-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_token_is_64_hex_digits_in_an_owner_only_file() {
        let dir = scratch("new");
        let path = dir.join("control.token");
        let t = token(&path).unwrap();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(token(&path).unwrap(), t, "the same file gives the same token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex review CX-W5 (2026-10-19): an existing token file that others can read is refused
    /// (anyone who read it could drive the app), and so is a link.
    #[cfg(unix)]
    #[test]
    fn cx_w5_a_token_file_others_can_read_or_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("refuse");
        let open = dir.join("open.token");
        std::fs::write(&open, "a".repeat(64)).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(token(&open).is_err(), "world-readable");
        let real = dir.join("real.token");
        std::fs::write(&real, "b".repeat(64)).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("link.token");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(token(&link).is_err(), "a link");
        // A link to nowhere must not be created through either.
        let dangling = dir.join("dangling.token");
        std::os::unix::fs::symlink(dir.join("made-through-link"), &dangling).unwrap();
        assert!(token(&dangling).is_err());
        assert!(!dir.join("made-through-link").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex review CX-W4: a token too short to be ours (or empty) is refused, not used.
    #[cfg(unix)]
    #[test]
    fn cx_w4_a_short_token_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("short");
        let path = dir.join("short.token");
        std::fs::write(&path, "1234").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(token(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
