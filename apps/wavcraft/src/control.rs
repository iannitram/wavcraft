//! A small control channel for scripted checks (the golden task): loopback only, one line per
//! request, one line per answer, no dependencies. Off unless `WAVCRAFT_CONTROL_PORT` is set.
//!
//! The first line on every connection must be `TOKEN <token>`, the contents of the file named
//! by `WAVCRAFT_CONTROL_TOKEN_FILE` (made owner-only with a CSPRNG token when missing; an
//! existing one must be owner-only, not a link, 64+ hex digits; see `token`). Then:
//!
//! - `STATE` → `OK frames=<n> channels=<n> rate=<hz> selection=<a>-<b> steps=<n> cursor=<i> busy=<0|1> progress=<0-100> unsaved=<0|1> tool=<name> view=<a>-<b> time_display=<format> selection_text=<range> snap_frames=<0|1> undo=<label> redo=<label> path=<the document's own path>`
//!   (`path=` is last: it may hold spaces; Save Selection As and Export do not change it)
//! - `OPEN <path>` → `OK` (the open runs in the background; poll `STATE` until `busy=0`)
//! - `SELECT <start> <end>` (frames) → `OK selection=<a>-<b>`
//! - `GESTURE shift_click <frame> | double_click | triple_click | edge_drag <from> <to> | shift_left|shift_right [frames] | shift_home | shift_end` → `OK selection=<a>-<b>` (the editor's selection gestures)
//! - `ACTION <name>` (an action table name, `edit.delete`, or an effect, `effect.invert`) → `OK selection=<a>-<b> changed=<0|1>`
//! - `PARAM <amplify.db|normalize.db> <number>` → `OK` (the number Effects › Amplify or Normalize uses)
//! - `KEY <chord>` (`Ctrl+Z`, `Cmd+Shift+Z`, `Space`, `F3`; Ctrl and Cmd mean the same) → `OK command=<name> selection=<a>-<b> changed=<0|1>` (runs what the shortcut table binds the chord to, as the key would; `ERR` when none)
//! - `ACTION window.toggle.<panel>` (also `window.show.<panel>`, `window.hide.<panel>`; ids in `wavcraft_core::panels::REGISTRY`) → `OK selection=<a>-<b> changed=0 <panel>=<0|1> …`
//! - `PANELS` → `OK waveform=<0|1> spectral=<0|1> history=<0|1> selection=<0|1> freq=<0|1> zone.left=<ids|-> zone.right=… zone.bottom=… zone.center=… front.left=<id|-> front.right=… front.bottom=… maximized=<id|->` (which panels are shown, then the shown tabs of each dock zone in order, the front tab of each, and the maximised panel)
//! - `PANEL move <id> <left|right|bottom>` | `PANEL maximize <id>` (again restores) | `PANEL close <id>` | `PANEL activate <id>` → the same line as `PANELS`; the layout is saved in the settings file
//! - `PANELMENU <panel> list` → `OK items=close,maximize,… maximized=<id|none>`; `PANELMENU <panel> run <item>` → `OK maximized=<id|none> confirm_clear_history=<0|1> <panel>=<0|1> …` (a panel's ≡ menu)
//! - `METADATA` → `OK fields=name,duration,… dialog=<0|1>`; `METADATA set <field,…|none>`; `METADATA open` (the Metadata Display dialog)
//! - `SELECT_TIME <start> <end>` (each in the chosen time format, no spaces inside) → `OK selection=<a>-<b>`; `STATE` also reports `time_display`, `selection_text` and `snap_frames`
//! - `SNAP_FRAME <frame>` → `OK frame=<snapped>` (what a ruler click or drag calls: SMPTE display and Snap to Frames on)
//! - `FREQ [fft=4096] [window=hann|blackman|rectangular] [start end]` → `OK range=<a>-<b> fft=<n> window=<w> ch1=<hz>Hz/<db>dB …` (the peak of each channel; `start end` in frames, else the selection, else the moment at the cursor, `at=<frame>`)
//! - `WINDOW minimize|restore` → `OK` (requests keep being answered while it is minimised)
//! - `NOISE_PRINT` → `OK frames=<n> channels=<n> rate=<hz>` (the session's noise print, captured by `ACTION nr.capture_print`), or `OK none`
//! - `EFFECT` → `OK <id> [<key>=<default> (<range>)...]; ...` (every effect and its settings)
//! - `EFFECT <name> key=value ...` → `OK selection=<a>-<b> changed=<0|1>` (run an effect on the selection, the whole file when none; unset keys keep their defaults)
//! - `STRETCH ratio=<0.1-10> pitch=<semitones>` → `OK selection=<a>-<b> changed=<0|1>` (Stretch and Pitch on the selection, the whole file when none; the result is selected); `PITCHDETECT` → `OK hz=<f0>|none` (median pitch of the selection)
//! - `EFFECT_DIALOG <name> [key=value ...]` opens the effect's dialog on those values; `EFFECT_DIALOG` → `OK open=<id|none> values=[..]`; `EFFECT_DIALOG OK|CANCEL` presses the button
//! - `GENERATE` → the generators and their settings; `GENERATE <tone|noise> key=value ... [seed=N]` → `OK selection=<a>-<b> changed=<0|1>` (insert at the cursor or replace the selection)
//! - `GENERATE_DIALOG <name> [key=value ...]` opens a Generate dialog; `GENERATE_DIALOG PREVIEW` → `OK preview=<frames>`; `GENERATE_DIALOG OK|CANCEL` presses the button
//! - `DECLIP [threshold_db=-0.1 min_run=3]` → `OK selection=<a>-<b> changed=<0|1>` (DeClipper on the selection, the whole file when none; one undo step "DeClipper"; `ACTION diagnostics.declip` runs it with the defaults)
//! - `DECLICK [sensitivity=50 max_width_ms=1]` → `OK selection=<a>-<b> changed=<0|1>` (DeClicker on the selection, the whole file when none; one undo step "DeClicker"; `ACTION diagnostics.declick` runs it with the defaults)
//! - `AUTOCLICK [threshold=50 complexity=20]` → same reply (Automatic Click Remover, scan and repair in one command; one undo step "Automatic Click Remover"; `ACTION diagnostics.autoClickRemover`)
//! - `DIAG scan <declip|declick> [key=value ...]` → `OK regions=<n> skipped=<m>` (scan the selection, the whole file when none, into the Diagnostics window's list; answered when the scan ends); `DIAG repair <all|i,j,k>` → `OK repaired=<n> skipped=<m>` (one undo step named for the tool; `ERR` when the list is stale)
//! - `INSERT_SILENCE <seconds>` → `OK selection=<a>-<b>` (Insert › Silence at the cursor)
//! - `DELETE_SILENCE [threshold_db=-48] [min_len_ms=500] [keep_ms=150]` → `OK stretches=<n> removed_s=<seconds> selection=<a>-<b>` (Edit › Delete Silence on the selection, the whole file when none; one undo step; `ERR <why>` when refused, e.g. everything is below the threshold)
//! - `STATS [<start> <end>]` (frames; the selection or the whole file without them) → `OK range=<a>-<b> ...` then, per channel, `ch<i> peak=.. peak_db=.. peak_frame=.. min=.. max=.. dc_pct=.. rms_*_db=.. dyn_db=.. clip_runs=.. clip_samples=.. bits=..`.
//!   Runs the scan at once and keeps it for `ACTION stats.goto_peak|stats.next_clip|stats.prev_clip` (the cursor moves; `ERR` when there is nowhere to go)
//! - `JOB testslow <ms>` → `OK` (a test-only background job over the selection, `<ms>` per 64K-frame chunk; `busy=1` and `progress` while it runs; `JOB cancel` stops it)
//! - `WORKSPACES` → `OK current=<name> modified=<0|1> workspaces=<a>|<b>…` (spaces in names shown as `_`; `Default` is built in; `modified` is 1 when the layout differs from the saved state of the workspace in use)
//! - `WORKSPACE save <name>` (keeps the layout under the name and makes it the one in use) | `switch <name>` | `reset [name]` (back to the saved layout) | `delete <name>` (not Default, not the one in use) → the same line as `WORKSPACES`; the one in use is kept in the settings file
//! - `HISTORY <step>` → `OK` (the History panel's click; 0 is Open)
//! - `META get [field]` / `META set <field> <value>` → `OK <value>` / `OK changed=<0|1>` (one undo step "Edit Metadata" per set; `\r` and `\n` in a value are line breaks)
//! - `CHNAME get` / `CHNAME set <ch from 1> <name>` → `OK <names>` / `OK changed=<0|1>` (channel names; one undo step "Edit Metadata"); `META set INFO.INAM <v>` (or any `INFO.XXXX`) sets a RIFF INFO tag
//! - `HISTORY_DELETE <step>` → `OK removed=<n>` (the step and every later one go; 0 can't)
//! - `HISTORY_CLEAR` → `OK` (Clear History, no confirmation; the current state stays)
//! - `HISTORY_KEY up|down` → `OK cursor=<i>` (the panel's arrow keys)
//! - `MIXER state` → `OK tracks=<n> any_solo=<0|1> | master vol=<db> pan= mute= solo= audible= peak=<db> | t1 ...` (the Mixer panel, over the multitrack session; `ERR` before `SESSION new`)
//! - `MIXER show|hide` → `OK` (the panel's window checkbox)
//! - `MIXER set <track|master> vol=<db> pan=<p> mute=0|1 solo=0|1` → the same line, after the change
//! - `AUTOSAVE` → `OK offered=<n> written=<n> failed=<n> pending=<0|1> scanned=<0|1> recoverable=<n>` (Auto Save's journal writer for the open file, and the sessions a crashed run left; poll until `scanned=1`)
//! - `RECOVER [index]` → `OK` (recover that crashed session, 0 by default; poll `STATE` until `busy=0`); `DISCARD_RECOVERY [index]` → `OK`
//! - `SAVE [path]` → `OK` (Save, or Save As to `path`; poll `STATE` until `busy=0`)
//! - `SAVE_SELECTION <path>` → `OK` (File › Save Selection As: only the selected range of the enabled
//!   channels, to a new WAV; the open document keeps its own path and unsaved mark)
//! - `EXPORT <path>` → `OK` (File › Export › File: the whole document to another WAV, same promise)
//! - `SCSOURCE test` → `OK frames=<n> checksum=<hex>` (the open document read back as a SoundCraft clip source)
//! - `VIDEO attach <path>` → `OK` (the Video panel opens the file on a worker; `ERR` with no file open)
//! - `VIDEO frame <seconds>` → `OK index=<i> size=<w>x<h> time=<t>` once the worker has decoded it, or `ERR <why>`
//!   (with a video clip in a Multitrack session, `<seconds>` is the session time, f129; the picture is the clip's movie frame)
//! - `VIDEO status` → `OK <text>` (what the panel says about the file); `VIDEO detach` → `OK`
//! - `SESSION new <name> [rate=48000] [depth=24]`, `SESSION state`, `TRACK add [name] [fmt=mono|stereo|5.1]|format|remove|rename|mute|solo|volume …`,
//!   `CLIP add <track> <path> <at_seconds>`, `CLIP adddoc <track> <at_seconds>`, `VIEW waveform|multitrack`, `CLIP select <track> <n>|none`, `MIXDOWN [selected] [normalize=<dB>]` (f111)
//!   `SESSION save <path>` / `SESSION open <path>` (the session file, f117; `OK offline=<n>`)
//!   `CLIPPROP <track> <i> tempo=<bpm> beats=<n> key=<text> name=<s> gain=<db>` (clip properties, f119)
//!   `CLIPTYPE <track> <i[,j..]|all> dialogue|music|sfx|ambience|none`, `CLIPTYPE filter <type>|all` (audio types, f123; clip numbers are 1-based)
//!   (multitrack basics, f103; see `multitrack_ui.rs`; `SESSION state` answers `OK view=… <json>`)
//!   `SMARKER add <t> [end]|rename <i> <name>|delete <i>|next|prev|playhead <t>|select <a> <b>|addhere|addsel|state`
//!   `SPLAY [seconds]`, `SSTOP` (play the session, f114; `SESSION state` adds `playing`, `position` in seconds and `underruns`)
//!   `TRACK arm <track> [input=<ch>]`, `TRACK disarm <track>`, `SREC start [seconds]`, `SREC stop` (record the armed tracks while the
//!   rest plays, f120; takes are listed in `SESSION state` under `takes`; `WAVCRAFT_FAKE_INPUT=1` is the only input built so far)
//!   `PUNCH set <in_s> <out_s>`, `PUNCH on|off`, `PUNCH preroll <s>` (punch in/out, f126: with punch on, `SREC start` without a time starts
//!   at the pre-roll before the range and the take is cut to the range; `SESSION state` adds `punch`)
//!   (session markers and ranges, f110)
//! - `METERS [PUSH <l> <r> | RESET]` → `OK l=<dBFS> l_hold=<dBFS> l_clip=<0|1> r=… r_hold=… r_clip=…` (the Levels
//!   panel's bars; `PUSH` gives the tap linear peaks as the audio callback would, `RESET` clears the clip lights)
//! - `LOUDNESS [<start> <end>]` → `OK scope=<file|selection|range> frames=<n> integrated=<LUFS> range=<LU>
//!   true_peak=<dBTP> sample_peak=<dBFS> max_momentary=<LUFS>` (`-inf` for nothing to measure; runs on the
//!   window's thread, so a long file waits)
//! - `CLIP edit <track> <i>` (clip number from 1) → `OK`: the clip's source opens as the document in the
//!   Waveform editor (poll `STATE` until `busy=0`); edit it with the usual verbs. `SESSION return` →
//!   `OK frames=<new> was=<old> trimmed=<n> removed=<n>`: the edited audio becomes the source, clips on it
//!   keep their start and are trimmed to its new length, the Multitrack view shows (f116)
//! - `AUTO add|move|delete|thin <track> <param> …`, `AUTO show <track> <param>|off`, `AUTO state` (automation lanes, f109)
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
