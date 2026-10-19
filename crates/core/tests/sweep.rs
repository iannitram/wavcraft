//! Stale session folders (2026-10-19, Shuttlecraft 6): a run ended by a signal or a crash left its
//! session store behind (measured: 3 runs, 3 folders, 66 MB for a 1-minute file), and nothing
//! removed it. `sweep_stale_sessions` removes those, and only those.

use std::path::{Path, PathBuf};

use wavcraft_core::store::{LOCK_FILE, Store, StoreOptions, stale_sessions, sweep_stale_sessions, this_host};

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
    }
    std::fs::write(dir.join("pack-0001.f32"), vec![0u8; 4096]).unwrap();
    dir
}

#[test]
fn a_folder_left_by_a_dead_run_is_stale_and_is_removed() {
    let t = TempDir::new("dead");
    let dead = session(&t.0, "0123456789abcdef", Some(&format!("pid={DEAD_PID}\nhost={}\napp=wavcraft 0.1.0\n", this_host())));
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
    drop(store);
}
