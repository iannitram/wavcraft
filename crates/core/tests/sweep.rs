//! Stale session folders (2026-10-19, the Mac test machine): a run ended by a signal or a crash left its
//! session store behind (measured: 3 runs, 3 folders, 66 MB for a 1-minute file), and nothing
//! removed it. `sweep_stale_sessions` removes those, and only those.

use std::path::{Path, PathBuf};

use wavcraft_core::autosave::{JOURNAL_FILE, JOURNAL_TMP};
use wavcraft_core::store::{HELD_FILE, LOCK_FILE, Store, StoreOptions, has_journal, machine_id, stale_sessions, sweep_stale_sessions, this_host};

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-sweep-test-{}-{name}", std::process::id()));
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

/// A pid no process has: above every platform's pid range.
const DEAD_PID: u32 = 4_000_000_000;

/// A session-shaped folder with a lock file and some bytes in a pack.
fn session(root: &Path, name: &str, lock: Option<&str>) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(lock) = lock {
        std::fs::write(dir.join(LOCK_FILE), lock).unwrap();
        std::fs::write(dir.join(HELD_FILE), b"").unwrap();
    }
    std::fs::write(dir.join("pack-0001.f32"), vec![0u8; 4096]).unwrap();
    dir
}

/// A session folder from before `session.held`.
fn session_without_held(root: &Path, name: &str, lock: &str) -> PathBuf {
    let dir = session(root, name, Some(lock));
    std::fs::remove_file(dir.join(HELD_FILE)).unwrap();
    dir
}

#[test]
fn a_folder_left_by_a_dead_run_is_stale_and_is_removed() {
    let t = TempDir::new("dead");
    let dead =
        session(&t.0, "0123456789abcdef", Some(&format!("pid={DEAD_PID}\nhost={}\nmachine={}\napp=wavcraft 0.1.0\n", this_host(), machine_id())));
    let found = stale_sessions(&t.0);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].dir.clone(), found[0].pid), (dead.clone(), DEAD_PID));
    assert!(found[0].bytes >= 4096);
    let freed = sweep_stale_sessions(Some(&t.0));
    assert!(freed >= 4096);
    assert!(!dead.exists());
}

#[test]
fn a_live_store_is_never_swept() {
    let t = TempDir::new("live");
    // A real store made by this process: its lock file names this pid.
    let store = Store::create_session(Some(&t.0), StoreOptions { keep: true, ..StoreOptions::default() }).unwrap();
    assert!(stale_sessions(&t.0).is_empty());
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    assert_eq!(std::fs::read_dir(&t.0).unwrap().count(), 1);
    drop(store);
}

#[test]
fn folders_that_are_not_wavcraft_sessions_are_left_alone() {
    let t = TempDir::new("others");
    let dead_lock = format!("pid={DEAD_PID}\nhost={}\napp=wavcraft 0.1.0\n", this_host());
    let keep = [
        // not a session id
        session(&t.0, "my-recordings", Some(&dead_lock)),
        session(&t.0, "0123456789ABCDEFG", Some(&dead_lock)),
        // a session id, but no lock file (mid-creation, or not ours)
        session(&t.0, "fedcba9876543210", None),
        // another program's lock file
        session(&t.0, "00000000000000aa", Some(&format!("pid={DEAD_PID}\nhost={}\napp=soundcraft 0.9\n", this_host()))),
        // a dead pid, but on ANOTHER machine (a shared session folder): its pid means nothing here
        session(&t.0, "00000000000000cc", Some(&format!("pid={DEAD_PID}\nhost=some-other-mac\napp=wavcraft 0.1.0\n"))),
        // an older lock without a host: whose machine it was can't be told, so it stays
        session(&t.0, "00000000000000dd", Some(&format!("pid={DEAD_PID}\napp=wavcraft 0.1.0\n"))),
        // a lock file without a pid
        session(&t.0, "00000000000000bb", Some("app=wavcraft 0.1.0\n")),
        // this machine's id, but a session from before session.held (no lock to test): stays
        session_without_held(
            &t.0,
            "00000000000000ee",
            &format!("pid={DEAD_PID}\nhost={}\nmachine={}\napp=wavcraft 0.1.0\n", this_host(), machine_id()),
        ),
    ];
    let loose = t.0.join("0123456789abcdee");
    std::fs::write(&loose, b"a file, not a folder").unwrap();
    assert!(stale_sessions(&t.0).is_empty());
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    for k in keep.iter().chain([&loose]) {
        assert!(k.exists(), "{} was removed", k.display());
    }
}

#[test]
fn a_missing_root_is_nothing_to_sweep() {
    let t = TempDir::new("missing");
    assert_eq!(sweep_stale_sessions(Some(&t.0.join("not-there"))), 0);
}

#[test]
fn a_store_records_this_machine_in_its_lock() {
    let t = TempDir::new("host");
    let store = Store::create_session(Some(&t.0), StoreOptions { keep: true, ..StoreOptions::default() }).unwrap();
    let dir = std::fs::read_dir(&t.0).unwrap().next().unwrap().unwrap().path();
    let lock = std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
    assert!(!this_host().is_empty(), "this machine has a name");
    assert!(lock.lines().any(|l| l == format!("host={}", this_host())), "{lock}");
    assert!(machine_id().len() == 32 && lock.lines().any(|l| l == format!("machine={}", machine_id())), "{lock}");
    // Its session.held is locked while the store lives, and free once it is gone.
    let held = std::fs::OpenOptions::new().read(true).write(true).open(dir.join(HELD_FILE)).unwrap();
    assert!(held.try_lock().is_err(), "held by the live store");
    drop(store);
    assert!(held.try_lock().is_ok(), "released when the store ends");
}

/// Codex review CX-W3 (2026-10-19): a session whose lock file is held open and locked by a
/// running program is never swept, whatever its pid looks like (a renamed program, a reused pid).
#[test]
fn cx_w3_a_session_whose_lock_is_held_is_never_swept() {
    let t = TempDir::new("held");
    let dir =
        session(&t.0, "1111111111111111", Some(&format!("pid={DEAD_PID}\nhost={}\nmachine={}\napp=wavcraft 0.1.0\n", this_host(), machine_id())));
    let held = std::fs::OpenOptions::new().read(true).write(true).open(dir.join(HELD_FILE)).unwrap();
    held.try_lock().unwrap();
    assert!(stale_sessions(&t.0).is_empty(), "a locked session is live");
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    assert!(dir.exists());
    drop(held);
}

/// Codex review CX-W3: two machines can share a hostname (and a disk); a session from another
/// machine (another machine id) is never swept, even with the same hostname and a dead pid.
#[test]
fn cx_w3_another_machine_with_the_same_hostname_is_never_swept() {
    let t = TempDir::new("twin");
    let other = "machine=ffffffffffffffffffffffffffffffff";
    let dir = session(&t.0, "2222222222222222", Some(&format!("pid={DEAD_PID}\nhost={}\n{other}\napp=wavcraft 0.1.0\n", this_host())));
    assert!(stale_sessions(&t.0).is_empty());
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    assert!(dir.exists());
}

/// The lock file of a session left by a dead run on this machine.
fn dead_lock() -> String {
    format!("pid={DEAD_PID}\nhost={}\nmachine={}\napp=wavcraft 0.1.0\n", this_host(), machine_id())
}

/// Card wav-c-86-b (#86, v1 item I4): `has_journal` sees either journal file name.
#[test]
fn c86b_has_journal_sees_the_journal_or_the_half_written_one() {
    let t = TempDir::new("has-journal");
    let dir = session(&t.0, "3333333333333333", Some(&dead_lock()));
    assert!(!has_journal(&dir), "a pack and a lock are not a journal");
    std::fs::write(dir.join(JOURNAL_TMP), b"half").unwrap();
    assert!(has_journal(&dir));
    std::fs::remove_file(dir.join(JOURNAL_TMP)).unwrap();
    std::fs::write(dir.join(JOURNAL_FILE), b"whole").unwrap();
    assert!(has_journal(&dir));
    assert!(!has_journal(&t.0.join("not-there")), "a missing folder has none");
}

/// Card wav-c-86-b: a dead run's session with a journal may hold work to recover: it is not
/// stale, and the sweep keeps it, whether the journal is whole or half-written.
#[test]
fn c86b_a_dead_session_with_a_journal_is_never_swept() {
    let t = TempDir::new("journaled");
    let whole = session(&t.0, "4444444444444444", Some(&dead_lock()));
    std::fs::write(whole.join(JOURNAL_FILE), b"whole").unwrap();
    let half = session(&t.0, "5555555555555555", Some(&dead_lock()));
    std::fs::write(half.join(JOURNAL_TMP), b"half").unwrap();
    assert_eq!(stale_sessions(&t.0), Vec::new());
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    assert!(whole.join(JOURNAL_FILE).exists());
    assert!(half.join(JOURNAL_TMP).exists());
    assert!(whole.join("pack-0001.f32").exists() && half.join("pack-0001.f32").exists());
}

/// Card wav-c-86-b: beside a journaled session, a dead session without one is still swept.
#[test]
fn c86b_only_the_session_without_a_journal_is_swept() {
    let t = TempDir::new("mixed");
    let kept = session(&t.0, "6666666666666666", Some(&dead_lock()));
    std::fs::write(kept.join(JOURNAL_FILE), b"whole").unwrap();
    let gone = session(&t.0, "7777777777777777", Some(&dead_lock()));
    let found = stale_sessions(&t.0);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].dir.clone(), found[0].pid), (gone.clone(), DEAD_PID));
    assert!(sweep_stale_sessions(Some(&t.0)) >= 4096);
    assert!(!gone.exists());
    assert!(kept.join(JOURNAL_FILE).exists());
}
