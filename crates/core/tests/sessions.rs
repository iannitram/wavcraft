//! A281 (f70): disk used by sessions, and "Clean up old sessions". The rules of the start-up
//! sweep (#8) hold: never a live session, never one that can be recovered (it may be the only
//! copy of unsaved work), never one this machine cannot vouch for, and nothing young.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use wavcraft_core::History;
use wavcraft_core::autosave::{JOURNAL_FILE, JOURNAL_TMP, Snapshot, recoverable_sessions, write_journal};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::sessions::{cleanup, removable, usage};
use wavcraft_core::store::{HELD_FILE, LOCK_FILE, Store, StoreOptions, machine_id, this_host};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-sessions-test-{}-{name}", std::process::id()));
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

fn opts() -> StoreOptions {
    StoreOptions { cache_bytes: 16 << 20, keep: true, ..StoreOptions::default() }
}

/// Make every file in `dir`, and the folder itself, look `days` days old.
fn age(dir: &Path, days: u64) {
    let then = SystemTime::now() - Duration::from_secs(days * 86_400);
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        // Windows refuses a write-open of a file a running store holds locked; such a file is left
        // as it is (the live session stays live whatever its age).
        if let Ok(file) = OpenOptions::new().write(true).open(e.path()) {
            file.set_modified(then).unwrap();
        }
    }
    // A folder's own time is set through a handle that may write it: Windows needs backup
    // semantics for that, and elsewhere a read-only handle does.
    #[cfg(windows)]
    let folder = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new().write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(dir)
    };
    #[cfg(not(windows))]
    let folder = std::fs::File::open(dir);
    folder.unwrap().set_modified(then).unwrap();
}

/// A session a run that ended left, with no journal.
fn dead(root: &Path) -> PathBuf {
    let s = Store::create_session(Some(root), opts()).unwrap();
    s.append(&[0.25; 1000]).unwrap();
    let dir = s.dir().to_path_buf();
    drop(s);
    dir
}

fn history_of(s: &Store) -> History {
    let ch: Vec<Channel> = (0..2).map(|_| Channel::from_pieces([Piece::whole(s.append(&[0.5; 2000]).unwrap()).unwrap()])).collect();
    let mut src = SourceFormat::synthetic(48_000, 2);
    src.path = PathBuf::from("synthetic.wav");
    History::new(DocState::new(s.clone(), ch, 48_000, src).unwrap(), u64::MAX)
}

/// A session a crashed run left with a durable journal.
fn recoverable(root: &Path) -> PathBuf {
    let s = Store::create_session(Some(root), opts()).unwrap();
    let h = history_of(&s);
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let dir = s.dir().to_path_buf();
    drop(h);
    drop(s);
    dir
}

fn now() -> SystemTime {
    SystemTime::now()
}

#[test]
fn only_old_sessions_of_dead_runs_are_removed() {
    let t = TempDir::new("old");
    let (old, young) = (dead(&t.0), dead(&t.0));
    age(&old, 40);
    age(&young, 5);
    assert_eq!(removable(&t.0, 30, now()).len(), 1);
    let u = usage(&t.0, 30, now());
    assert_eq!((u.sessions, u.stale, u.recoverable, u.removable), (2, 2, 0, 1), "{u:?}");
    assert!(u.removable_bytes > 0 && u.bytes >= u.removable_bytes);
    let done = cleanup(&t.0, 30, now());
    assert_eq!(done.removed, 1);
    assert!(done.freed > 0);
    assert!(!old.exists(), "the old dead session goes");
    assert!(young.exists(), "the young one stays");
    // A shorter age takes the rest; 0 days means anything not live and not recoverable.
    assert_eq!(cleanup(&t.0, 1, now()).removed, 1, "5 days old is older than 1 day");
    assert!(!young.exists());
}

#[test]
fn a_live_session_is_never_removed_however_old_it_looks() {
    let t = TempDir::new("live");
    let s = Store::create_session(Some(&t.0), opts()).unwrap();
    s.append(&[0.1; 500]).unwrap();
    age(s.dir(), 400);
    assert!(removable(&t.0, 1, now()).is_empty());
    assert_eq!(cleanup(&t.0, 1, now()).removed, 0);
    assert!(s.dir().join(LOCK_FILE).exists(), "the live session is intact");
    assert_eq!(usage(&t.0, 1, now()).removable, 0);
}

#[test]
fn a_session_offered_for_recovery_is_never_removed() {
    let t = TempDir::new("recoverable");
    let good = recoverable(&t.0);
    let half = dead(&t.0);
    std::fs::write(half.join(JOURNAL_TMP), b"half").unwrap();
    let damaged = dead(&t.0);
    std::fs::write(damaged.join(JOURNAL_FILE), b"garbage").unwrap();
    for d in [&good, &half, &damaged] {
        age(d, 400);
    }
    assert_eq!(recoverable_sessions(&t.0).len(), 3, "all three are held back from sweeping (two of them unreadable, kept not deleted)");
    let u = usage(&t.0, 30, now());
    assert_eq!((u.sessions, u.recoverable, u.removable), (3, 3, 0), "{u:?}");
    assert_eq!(cleanup(&t.0, 1, now()).removed, 0);
    let before = std::fs::read(good.join(JOURNAL_FILE)).unwrap();
    assert!(good.exists() && half.exists() && damaged.exists());
    assert_eq!(std::fs::read(good.join(JOURNAL_FILE)).unwrap(), before, "the journal is untouched");
}

#[test]
fn what_this_machine_cannot_vouch_for_is_left_alone() {
    let t = TempDir::new("foreign");
    let mk = |name: &str, lock: Option<String>| {
        let dir = t.0.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(l) = lock {
            std::fs::write(dir.join(LOCK_FILE), l).unwrap();
            std::fs::write(dir.join(HELD_FILE), b"").unwrap();
        }
        std::fs::write(dir.join("pack-0001.f32"), vec![0u8; 4096]).unwrap();
        age(&dir, 400);
        dir
    };
    let host = this_host();
    let other = mk("0123456789abcdef", Some(format!("pid=4000000000\nhost={host}\nmachine={}\napp=wavcraft 0.1.0\n", "f".repeat(32))));
    let not_wavcraft = mk("1123456789abcdef", Some(format!("pid=4000000000\nhost={host}\nmachine={}\napp=other\n", machine_id())));
    let no_lock = mk("2123456789abcdef", None);
    let not_a_session = mk("my-own-folder", Some(format!("pid=4000000000\nhost={host}\nmachine={}\napp=wavcraft 0.1.0\n", machine_id())));
    assert_eq!(cleanup(&t.0, 1, now()).removed, 0);
    for d in [other, not_wavcraft, no_lock, not_a_session] {
        assert!(d.exists(), "{} stays", d.display());
    }
}

#[test]
fn an_unreadable_age_or_a_missing_folder_removes_nothing() {
    let t = TempDir::new("odd");
    assert_eq!(cleanup(&t.0.join("not-there"), 1, now()).removed, 0);
    assert_eq!(usage(&t.0.join("not-there"), 1, now()).sessions, 0);
    // A clock set back (the files look newer than now) counts as no age.
    let d = dead(&t.0);
    age(&d, 100);
    let earlier = SystemTime::now() - Duration::from_secs(200 * 86_400);
    assert!(removable(&t.0, 30, earlier).is_empty());
    assert!(d.exists());
}
