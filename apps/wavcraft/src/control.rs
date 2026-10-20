//! A small control channel for scripted checks (the golden task): loopback only, one line per
//! request, one line per answer, no dependencies. Off unless `WAVCRAFT_CONTROL_PORT` is set.
//!
//! The first line on every connection must be `TOKEN <token>`, the contents of the file named
//! by `WAVCRAFT_CONTROL_TOKEN_FILE` (made owner-only with a CSPRNG token when missing; an
//! existing one must be owner-only, not a link, 64+ hex digits; see `token`). Then:
//!
//! - `STATE` → `OK frames=<n> channels=<n> rate=<hz> selection=<a>-<b> steps=<n> cursor=<i> busy=<0|1> unsaved=<0|1> snap=<0|1> snapzero=<0|1> enabled=<one 0|1 per channel> clipboard=<1-5> clip_frames=<n> undo=<label> redo=<label>`
//!   (`path=<the document's own path>` is last: it may hold spaces; Save Selection As and Export do not change it)
//! - `OPEN <path>` → `OK` (the open runs in the background; poll `STATE` until `busy=0`)
//! - `STATE` → `OK frames=<n> channels=<n> rate=<hz> selection=<a>-<b> steps=<n> cursor=<i> busy=<0|1> undo=<label> redo=<label>`
//! - `OPEN <path>` → `OK` (the open runs in the background and adds a document; poll `STATE` until `busy=0`)
//! - `DOCS` → `OK count=<n> current=<i> | <i> unsaved=<0|1> frames=<n> <name> | …` (the open documents, in the order opened)
//! - `FILES` → `OK count=<shown> total=<n> current=<i> sort=<col:asc|desc|none> filter="…" | <row> doc=<i> current=<0|1> unsaved=<0|1> frames=<n> rate=<hz> ch=<n> bits=<b> format=<f> name=<n> path=<p> | …` (the Files panel's table); `FILES sort <col> asc|desc` (or `sort none`), `FILES filter <text>`, `FILES select <row>` (makes that row's document current), `FILES reveal [row]` (answers the command Reveal would run; it is not run)
//! - `SWITCH <i>` → `OK current=<i>` (every other verb acts on the current document)
//! - `CLOSE` → `OK count=<n>` (closes the current document); with unsaved changes `ERR …` unless `CLOSE discard`
//! - `SELECT <start> <end>` (frames) → `OK selection=<a>-<b>`
//! - `GESTURE shift_click <frame> | double_click | triple_click | edge_drag <from> <to> | shift_left|shift_right [frames] | shift_home | shift_end` → `OK selection=<a>-<b>` (the editor's selection gestures)
//! - `ACTION <name>` (an action table name, `edit.delete`; `view.snap.toggle`, `view.snap.zero.toggle`, `channel.toggle.<n>` with n from 0) → `OK selection=<a>-<b> changed=<0|1>`
//! - `ACTION window.toggle.<panel>` (also `window.show.<panel>`, `window.hide.<panel>`; ids in `wavcraft_core::panels::REGISTRY`) → `OK selection=<a>-<b> changed=0 <panel>=<0|1> …`
//! - `PANELS` → `OK waveform=<0|1> spectral=<0|1> history=<0|1>` (which panels are shown)
//! - `PARAM <amplify.db|normalize.db> <number>` → `OK` (the number Effects › Amplify or Normalize uses)
//! - `SNAP <frame> [frames_per_px]` → `OK frame=<snapped>` (the snap step selection gestures call, with the selection's edges as targets)
//! - `OSCLIP` → `OK ours=<0|1> path=<first file on the system clipboard or ->`; `OSCLIP set <path>` puts a file there as another program would
//! - `MARKERS` → `OK count=<n> <id>,<start>,<end>,<name>;…` (the markers of the current state; a point has start = end)
//! - `ACTION <name>` (an action table name, `edit.delete`) → `OK selection=<a>-<b> changed=<0|1>`
//! - `MIXPASTE mode=insert|overlap|overwrite|modulate [copied=<pct>[,<pct>]] [existing=<pct>[,<pct>]] [invert=0|1] [fade=<ms>] [loops=<n>] [file=<path>]` → `OK selection=<a>-<b>` (Edit › Mix Paste at the cursor; the clipboard unless `file=`)
//! - `PANELS` → `OK waveform=<0|1> spectral=<0|1> history=<0|1> selection=<0|1> zone.left=<ids|-> zone.right=… zone.bottom=… zone.center=… front.left=<id|-> front.right=… front.bottom=… maximized=<id|->` (which panels are shown, then the shown tabs of each dock zone in order, the front tab of each, and the maximised panel)
//! - `PANEL move <id> <left|right|bottom>` | `PANEL maximize <id>` (again restores) | `PANEL close <id>` | `PANEL activate <id>` → the same line as `PANELS`; the layout is saved in the settings file
//! - `KEY <chord>` (`Ctrl+Z`, `Cmd+Shift+Z`, `Space`, `F3`; Ctrl and Cmd mean the same) → `OK command=<name> selection=<a>-<b> changed=<0|1>`
//!   (runs what the shortcut table binds the chord to, as the key would; `ERR` when none)
//! - `SHORTCUTS` → `OK <command>=<chord>[|<chord>]…` for every command (`-` when it has none)
//! - `SHORTCUT set <command> <chord> [reassign]` → `OK …`, or `ERR conflict: <chord> is already <command>`
//!   (with `reassign` the chord is taken from that command); `SHORTCUT clear <command>`;
//!   `SHORTCUT reset` (the defaults). Each saves the user's key file.
//! - `WINDOW minimize|restore` → `OK` (requests keep being answered while it is minimised)
//! - `EFFECT` → `OK <id> [<key>=<default> (<range>)...]; ...` (every effect and its settings)
//! - `EFFECT <name> key=value ...` → `OK selection=<a>-<b> changed=<0|1>` (run an effect on the selection, the whole file when none; unset keys keep their defaults)
//! - `NOISE_PRINT` → `OK frames=<n> channels=<n> rate=<hz>` (the session's noise print, captured by `ACTION nr.capture_print`), or `OK none`
//! - `EFFECT_DIALOG <name> [key=value ...]` opens the effect's dialog on those values; `EFFECT_DIALOG` → `OK open=<id|none> values=[..]`; `EFFECT_DIALOG OK|CANCEL` presses the button
//! - `GENERATE` → the generators and their settings; `GENERATE <tone|noise> key=value ... [seed=N]` → `OK selection=<a>-<b> changed=<0|1>` (insert at the cursor or replace the selection)
//! - `GENERATE_DIALOG <name> [key=value ...]` opens a Generate dialog; `GENERATE_DIALOG PREVIEW` → `OK preview=<frames>`; `GENERATE_DIALOG OK|CANCEL` presses the button
//! - `INSERT_SILENCE <seconds>` → `OK selection=<a>-<b>` (Insert › Silence at the cursor)
//! - `INSERT_FILE <path>` → `OK` (Edit › Insert › Audio from File at the cursor, or over the selection; poll `STATE` until `busy=0`; a refused file shows in the app's log, not here)
//! - `STATS [<start> <end>]` (frames; the selection or the whole file without them) → `OK range=<a>-<b> ...` then, per channel, `ch<i> peak=.. peak_db=.. peak_frame=.. min=.. max=.. dc_pct=.. rms_*_db=.. dyn_db=.. clip_runs=.. clip_samples=.. bits=..`.
//!   Runs the scan at once and keeps it for `ACTION stats.goto_peak|stats.next_clip|stats.prev_clip` (the cursor moves; `ERR` when there is nowhere to go)
//! - `WORKSPACES` → `OK current=<name> modified=<0|1> workspaces=<a>|<b>…` (spaces in names shown as `_`; `Default` is built in; `modified` is 1 when the layout differs from the saved state of the workspace in use)
//! - `WORKSPACE save <name>` (keeps the layout under the name and makes it the one in use) | `switch <name>` | `reset [name]` (back to the saved layout) | `delete <name>` (not Default, not the one in use) → the same line as `WORKSPACES`; the one in use is kept in the settings file
//! - `HISTORY <step>` → `OK` (the History panel's click; 0 is Open)
//! - `HISTORY_DELETE <step>` → `OK removed=<n>` (the step and every later one go; 0 can't)
//! - `HISTORY_CLEAR` → `OK` (Clear History, no confirmation; the current state stays)
//! - `HISTORY_KEY up|down` → `OK cursor=<i>` (the panel's arrow keys)
//! - `AUTOSAVE` → `OK offered=<n> written=<n> failed=<n> pending=<0|1> scanned=<0|1> recoverable=<n>` (Auto Save's journal writer for the open file, and the sessions a crashed run left; poll until `scanned=1`)
//! - `RECOVER [index]` → `OK` (recover that crashed session, 0 by default; poll `STATE` until `busy=0`); `DISCARD_RECOVERY [index]` → `OK`
//! - `CONVERT rate=44100 channels=1 bits=16 dither=tpdf shaping=light quality=high mix=50,50` or `CONVERT preset=<name>` → `OK`
//!   (Convert Sample Type, in the background; poll `STATE` until `busy=0`; `ERR` for a bad setting)
//! - `INTERPRET <hz>` → `OK` (Interpret Sample Rate: relabel the rate, no resampling)
//! - `PRESET list` | `PRESET save <name> | <settings>` | `PRESET delete <name>` → `OK …` (conversion presets)
//! - `META get [field]` / `META set <field> <value>` → `OK <value>` / `OK changed=<0|1>` (one undo step "Edit Metadata" per set; `\r` and `\n` in a value are line breaks)
//! - `SAVE [path] [bits=16|24|32] [float=0|1]` → `OK` (Save, or Save As to `path`, the format from
//!   its extension: .wav, .aiff/.aif, .flac; `bits`/`float=1` choose the sample type; poll `STATE` until `busy=0`)
//! - `QUIT` → `OK`, then the window closes; with unsaved changes in any document `ERR …` unless `QUIT discard`
//! - `METERS [PUSH <l> <r> | RESET]` → `OK l=<dBFS> l_hold=<dBFS> l_clip=<0|1> r=… r_hold=… r_clip=…` (the Levels
//!   panel's bars; `PUSH` gives the tap linear peaks as the audio callback would, `RESET` clears the clip lights)
//! - `LOUDNESS [<start> <end>]` → `OK scope=<file|selection|range> frames=<n> integrated=<LUFS> range=<LU>
//!   true_peak=<dBTP> sample_peak=<dBFS> max_momentary=<LUFS>` (`-inf` for nothing to measure; runs on the
//!   window's thread, so a long file waits)
//! - `MATCH [target=<LUFS>] [tp=<dBTP>] [tol=<LU>] [limit=on|off] [lookahead=<ms>] [release=<ms>] [preset=podcast|broadcast]`
//!   → `OK gain=<dB> limited=<0|1> on_target=<0|1> before=<LUFS> after=<LUFS> true_peak=<dBTP> changed=<0|1>` (Match
//!   Loudness on the whole file as one undoable step, default the podcast preset; runs on the window's thread;
//!   poll `STATE` until `busy=0`, then `LOUDNESS` reads the result back)
//! - `SAVE_SELECTION <path>` → `OK` (File › Save Selection As: only the selected range of the enabled
//!   channels, to a new WAV; the open document keeps its own path and unsaved mark)
//! - `EXPORT <path>` → `OK` (File › Export › File: the whole document to another WAV, same promise)
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
            log::warn!(target: "wavcraft", "control channel off: the token file {} was refused or could not be made: {e}", Path::new(&token_file).display());
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
///   an owner-only ACL on Windows (protected, one entry: the SID of this process's user), and the
///   file is removed if that fails;
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

/// Windows: an existing token file is used only when its DACL is protected (nothing inherited)
/// and every allow entry is this process's user (by SID). Anything else is refused, as on Unix;
/// it is never repaired, since others may already have read it (the Windows dev machine's check of wavcraft#8).
#[cfg(not(unix))]
fn check_private(path: &Path, _meta: &std::fs::Metadata) -> std::io::Result<()> {
    match crate::win_acl::others_can_read(path)? {
        None => Ok(()),
        Some(why) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("others can read the token file ({why}); refused (delete it and a new one is made)"),
        )),
    }
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())
}

/// Windows: exclusive create, an owner-only DACL before the token goes in; on failure the file goes.
#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(e) = crate::win_acl::set_owner_only(path) {
        drop(f);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    f.write_all(text.as_bytes())
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

    /// The Windows dev machine's check of wavcraft#8 (2026-10-19): on Windows an existing token file whose ACL
    /// lets others read it (inherited entries, or the Users group) is refused, not repaired.
    #[cfg(windows)]
    #[test]
    fn cx_w5_windows_a_token_file_others_can_read_is_refused() {
        let dir = scratch("win-acl");
        let open = dir.join("open.token");
        std::fs::write(&open, "a".repeat(64)).unwrap();
        // Inherited entries from the folder, plus read for Users (S-1-5-32-545).
        let ok = std::process::Command::new("icacls").arg(&open).args(["/grant", "*S-1-5-32-545:(RX)"]).status().unwrap().success();
        assert!(ok);
        assert!(token(&open).is_err(), "readable by Users: refused");
        assert_eq!(std::fs::read_to_string(&open).unwrap(), "a".repeat(64), "not rewritten");
        // A token this code made is owner-only and is accepted the next time.
        let made = dir.join("made.token");
        let t = token(&made).unwrap();
        assert_eq!(token(&made).unwrap(), t);
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
