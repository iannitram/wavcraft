//! Auto Save and crash recovery (A239, feature f10): the journal of the piece table and history,
//! written durably and read back after a "crash" (a store dropped with `keep`, which leaves the
//! folder as a killed run does). The rules fixed in #8 hold: the sweep never removes a session
//! that can be recovered, never a live one, and a failed recovery leaves every file as found.

use std::path::{Path, PathBuf};
use std::time::Duration;

use wavcraft_core::History;
use wavcraft_core::autosave::{Autosaver, JOURNAL_FILE, JOURNAL_TMP, RecoverError, Snapshot, discard, recover, recoverable_sessions, write_journal};
use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions, stale_sessions, sweep_stale_sessions};

const RATE: u32 = 48_000;
const NONE: Crossfades = Crossfades::NONE;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-autosave-test-{}-{name}", std::process::id()));
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

fn opts(pack_bytes: u64) -> StoreOptions {
    StoreOptions { pack_bytes, cache_bytes: 64 << 20, keep: true, ..StoreOptions::default() }
}

/// A two-channel document of `frames` distinct values, in blocks of `block` frames.
fn doc(s: &Store, frames: usize, block: usize) -> DocState {
    let chans: Vec<Channel> = (0..2)
        .map(|c| {
            let v: Vec<f32> = (0..frames).map(|i| (((i * 31 + c * 7_919) % 1_000_003) as f32 - 500_000.0) / 524_288.0).collect();
            Channel::from_pieces(v.chunks(block.clamp(1, BLOCK_FRAMES)).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))
        })
        .collect();
    let mut src = SourceFormat::synthetic(RATE, 2);
    src.path = PathBuf::from("/some where/my file %1.wav");
    src.warnings = vec!["cut short: 12 frames missing".to_string()];
    DocState::new(s.clone(), chans, RATE, src).unwrap()
}

fn content(d: &DocState) -> Vec<Vec<u32>> {
    (0..d.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; d.len_frames() as usize];
            assert_eq!(d.read(c, 0, &mut v).unwrap(), v.len());
            v.iter().map(|x| x.to_bits()).collect()
        })
        .collect()
}

/// A history with a few edits of different kinds.
fn edited(s: &Store, frames: usize, block: usize) -> History {
    let mut h = History::new(doc(s, frames, block), u64::MAX);
    let cur = h.current().clone();
    h.push_edit(edit::delete(&cur, 1_000..9_000, ChannelMask::ALL, NONE).unwrap());
    let cur = h.current().clone();
    h.push_edit(edit::silence(&cur, 500..2_500, ChannelMask::only(1), NONE).unwrap());
    let cur = h.current().clone();
    let (cut, clip) = edit::cut(&cur, 3_000..6_000, ChannelMask::ALL, NONE).unwrap();
    h.push_edit(cut);
    let cur = h.current().clone();
    h.push_edit(edit::paste(&cur, PasteAt::Insert(100), &clip, ChannelMask::ALL, NONE).unwrap());
    let cur = h.current().clone();
    h.push_edit(edit::insert_silence(&cur, 50, 700, NONE).unwrap());
    h
}

fn all_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                if e.file_name().to_string_lossy() == "session.lock" { Vec::new() } else { std::fs::read(e.path()).unwrap() },
            )
        })
        .collect();
    v.sort();
    v
}

/// Make a session as a crashed run leaves it: edits, a durable journal, store dropped with `keep`.
type Seen = Vec<(String, Vec<Vec<u32>>)>;

fn crashed(root: &Path, frames: usize, pack_bytes: u64, block: usize) -> (PathBuf, Seen, usize) {
    let s = Store::create_session(Some(root), opts(pack_bytes)).unwrap();
    let mut h = edited(&s, frames, block);
    h.undo();
    let seen: Vec<_> = h.steps().iter().map(|st| (st.label().to_string(), content(st.state()))).collect();
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let (dir, cursor) = (s.dir().to_path_buf(), h.cursor());
    drop(h);
    drop(s);
    (dir, seen, cursor)
}

#[test]
fn a_crashed_session_comes_back_with_its_document_and_history() {
    let t = TempDir::new("roundtrip");
    let (dir, seen, cursor) = crashed(&t.0, 100_000, 200_000, 7_000);
    let found = recoverable_sessions(&t.0);
    assert_eq!(found.len(), 1, "{found:?}");
    let info = found[0].info.as_ref().unwrap();
    assert_eq!((info.labels.len(), info.cursor, info.channels, info.sample_rate), (seen.len(), cursor, 2, RATE));
    assert_eq!(info.labels, seen.iter().map(|s| s.0.clone()).collect::<Vec<_>>());

    let rec = recover(&dir, opts(200_000)).unwrap();
    assert_eq!(rec.cursor, cursor);
    assert_eq!(rec.steps.len(), seen.len());
    for ((label, state), (want_label, want)) in rec.steps.iter().zip(&seen) {
        assert_eq!(label, want_label);
        assert_eq!(&content(state), want, "step {label}");
    }
    let src = rec.steps[0].1.source();
    assert_eq!(src.path, PathBuf::from("/some where/my file %1.wav"));
    assert_eq!(src.warnings, vec!["cut short: 12 frames missing".to_string()]);

    // The history works: undo, redo, and a new edit leave every step intact.
    let mut h = rec.into_history(u64::MAX).unwrap();
    assert_eq!(h.undo_label(), Some(seen[cursor].0.as_str()));
    assert!(h.redo().is_some());
    assert!(h.redo().is_none(), "the cursor was on the one before the last step");
    assert_eq!(h.redo_label(), None);
    let cur = h.current().clone();
    let more = edit::delete(&cur, 0..100, ChannelMask::ALL, NONE).unwrap();
    h.push_edit(more);
    let after = content(h.current());
    assert_eq!(after[0].len() as u64, h.current().len_frames());
    h.go_to(0).unwrap();
    assert_eq!(content(h.current()), seen[0].1);
}

#[test]
fn a_recovered_store_writes_to_a_new_pack_and_cleans_up_when_closed() {
    let t = TempDir::new("append");
    let (dir, seen, _) = crashed(&t.0, 30_000, 1 << 20, 5_000);
    let before = all_files(&dir).into_iter().filter(|(n, _)| n.starts_with("pack-")).collect::<Vec<_>>();
    let rec = recover(&dir, StoreOptions { keep: false, ..opts(1 << 20) }).unwrap();
    let h = rec.into_history(u64::MAX).unwrap();
    let block = h.current().store().append(&[0.5; 100]).unwrap();
    assert!(block.block().pack > before.len() as u32, "new blocks go after the packs found: pack {}", block.block().pack);
    // The old packs are byte for byte what they were.
    for (name, bytes) in &before {
        assert_eq!(&std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(content(h.steps()[0].state()), seen[0].1);
    drop(block);
    drop(h);
    assert!(!dir.exists(), "a clean close removes the recovered session folder");
}

#[test]
fn the_sweep_never_removes_a_session_that_can_be_recovered() {
    let t = TempDir::new("sweep");
    let (dir, _, _) = crashed(&t.0, 30_000, 1 << 20, 5_000);
    // Another crashed run that never made a journal: nothing to recover, swept as before.
    let s = Store::create_session(Some(&t.0), opts(1 << 20)).unwrap();
    let other = s.dir().to_path_buf();
    drop(s);
    assert_eq!(stale_sessions(&t.0).len(), 1);
    sweep_stale_sessions(Some(&t.0));
    assert!(dir.exists(), "the session with a journal stays");
    assert!(!other.exists(), "the one without goes");
    // A damaged journal, a half-written one: still kept.
    std::fs::write(dir.join(JOURNAL_FILE), b"garbage").unwrap();
    sweep_stale_sessions(Some(&t.0));
    assert!(dir.exists());
    std::fs::remove_file(dir.join(JOURNAL_FILE)).unwrap();
    std::fs::write(dir.join(JOURNAL_TMP), b"half").unwrap();
    sweep_stale_sessions(Some(&t.0));
    assert!(dir.exists(), "a half-written journal alone also protects the folder");
}

#[test]
fn a_live_session_is_neither_swept_nor_recoverable() {
    let t = TempDir::new("live");
    let s = Store::create_session(Some(&t.0), opts(1 << 20)).unwrap();
    let h = edited(&s, 30_000, 5_000);
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    assert!(recoverable_sessions(&t.0).is_empty());
    assert!(stale_sessions(&t.0).is_empty());
    assert_eq!(sweep_stale_sessions(Some(&t.0)), 0);
    assert!(matches!(recover(s.dir(), opts(1 << 20)), Err(RecoverError::InUse(_))));
    assert!(matches!(discard(s.dir()), Err(RecoverError::InUse(_))));
    assert!(s.dir().join(JOURNAL_FILE).exists());
}

#[test]
fn a_damaged_journal_is_refused_and_every_file_stays() {
    let t = TempDir::new("damaged");
    let (dir, _, _) = crashed(&t.0, 30_000, 100_000, 5_000);
    let good = std::fs::read(dir.join(JOURNAL_FILE)).unwrap();
    for (what, bad) in [
        ("flipped byte", {
            let mut b = good.clone();
            let i = b.len() / 2;
            b[i] ^= 1;
            b
        }),
        ("cut short", good[..good.len() - 20].to_vec()),
        ("empty", Vec::new()),
    ] {
        std::fs::write(dir.join(JOURNAL_FILE), &bad).unwrap();
        let before = all_files(&dir);
        let e = recover(&dir, opts(100_000)).err().unwrap_or_else(|| panic!("{what}: recovered"));
        assert!(matches!(e, RecoverError::Journal(_)), "{what}: {e}");
        assert_eq!(all_files(&dir), before, "{what}: files changed");
        let found = recoverable_sessions(&t.0);
        assert_eq!(found.len(), 1);
        assert!(found[0].info.is_err(), "{what}");
    }
}

#[test]
fn a_journal_that_does_not_fit_its_packs_deletes_nothing() {
    let t = TempDir::new("missing-pack");
    let (dir, _, _) = crashed(&t.0, 100_000, 70_000, 5_000);
    let mut packs: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pack-")).collect();
    packs.sort_by_key(std::fs::DirEntry::file_name);
    assert!(packs.len() >= 2, "the test wants several packs");
    // Truncate the last pack: some block now runs past its end.
    let last = packs.last().unwrap().path();
    let len = std::fs::metadata(&last).unwrap().len();
    std::fs::OpenOptions::new().write(true).open(&last).unwrap().set_len(len / 2).unwrap();
    let before = all_files(&dir);
    let e = recover(&dir, opts(70_000)).err().expect("a truncated pack is refused");
    assert!(matches!(e, RecoverError::Journal(_)), "{e}");
    assert_eq!(all_files(&dir), before, "a failed recovery changes no file (no pack is deleted as the handles drop)");
}

#[test]
fn the_journal_is_atomic_and_owner_only() {
    let t = TempDir::new("atomic");
    let s = Store::create_session(Some(&t.0), opts(1 << 20)).unwrap();
    let mut h = edited(&s, 30_000, 5_000);
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let first = std::fs::read(s.dir().join(JOURNAL_FILE)).unwrap();
    assert!(!s.dir().join(JOURNAL_TMP).exists(), "the new file is renamed over, none left");
    // A crash while writing the next one leaves its half-written file beside the whole old one.
    std::fs::write(s.dir().join(JOURNAL_TMP), &first[..first.len() / 3]).unwrap();
    let cur = h.current().clone();
    h.push_edit(edit::delete(&cur, 0..10, ChannelMask::ALL, NONE).unwrap());
    drop(cur);
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let second = std::fs::read(s.dir().join(JOURNAL_FILE)).unwrap();
    assert_ne!(first, second);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(s.dir().join(JOURNAL_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "owner-only, mode {mode:o}");
    }
    let n = h.steps().len();
    drop(h);
    drop(s);
    let rec = recover(&recoverable_sessions(&t.0)[0].dir, opts(1 << 20)).unwrap();
    assert_eq!(rec.steps.len(), n);
}

#[test]
fn the_packs_a_durable_journal_names_survive_until_a_newer_journal_is_durable() {
    let t = TempDir::new("pin");
    // Small packs, so dropping the old steps empties and would delete sealed packs.
    let s = Store::create_session(Some(&t.0), StoreOptions { keep: false, ..opts(70_000) }).unwrap();
    let mut h = edited(&s, 150_000, 4_000);
    let saver = Autosaver::start(&s);
    saver.offer(Snapshot::of(&h));
    assert!(saver.wait_idle(Duration::from_secs(20)));
    assert_eq!(saver.stats().written, 1);
    let packs_then = s.packs();
    assert!(packs_then.len() >= 4, "{packs_then:?}");

    // The document shrinks to its first moments, and the history lets every old step go (as the
    // disk cap or Clear History does) before any newer journal exists: the journal on disk
    // still names the old blocks, so their packs must stay.
    let cur = h.current().clone();
    h.push_edit(edit::crop(&cur, 0..4_000, ChannelMask::ALL, NONE).unwrap());
    h.clear();
    drop(cur);
    let now = s.packs();
    assert!(packs_then.iter().all(|p| now.contains(p)), "packs {packs_then:?} -> {now:?}: the autosaver holds the blocks of the journal on disk");
    // The next journal, once durable, lets them go.
    let cur = h.current().clone();
    h.push_edit(edit::delete(&cur, 0..5, ChannelMask::ALL, NONE).unwrap());
    drop(cur);
    saver.offer(Snapshot::of(&h));
    assert!(saver.wait_idle(Duration::from_secs(20)));
    assert_eq!(saver.stats().written, 2);
    let after = s.packs();
    assert!(
        packs_then.iter().any(|p| !after.contains(p)),
        "packs {packs_then:?} -> {after:?}: with the new journal durable the old blocks' packs go"
    );
    let want = content(h.current());
    let dir = s.dir().to_path_buf();
    let n = h.steps().len();
    s.set_keep(true); // the run ends abruptly from here
    drop(saver);
    drop(h);
    drop(s);
    let rec = recover(&dir, opts(70_000)).unwrap();
    assert_eq!(rec.steps.len(), n);
    assert_eq!(content(&rec.steps[rec.cursor].1), want);
}

#[test]
fn offers_pile_up_but_only_the_newest_matters() {
    let t = TempDir::new("coalesce");
    let s = Store::create_session(Some(&t.0), opts(1 << 20)).unwrap();
    let mut h = edited(&s, 30_000, 5_000);
    let saver = Autosaver::start(&s);
    for i in 0..20 {
        let cur = h.current().clone();
        h.push_edit(edit::delete(&cur, 0..(i + 1), ChannelMask::ALL, NONE).unwrap());
        saver.offer(Snapshot::of(&h));
    }
    assert!(saver.wait_idle(Duration::from_secs(20)));
    let st = saver.stats();
    assert_eq!((st.offered, st.handled, st.failed), (20, 20, 0));
    assert!(st.written >= 1 && st.written <= 20);
    let want = content(h.current());
    let (dir, steps) = (s.dir().to_path_buf(), h.steps().len());
    drop(saver);
    drop(h);
    drop(s);
    let rec = recover(&dir, opts(1 << 20)).unwrap();
    assert_eq!(rec.steps.len(), steps, "the journal on disk is the newest snapshot");
    assert_eq!(content(&rec.steps[rec.cursor].1), want);
}

#[test]
fn discard_removes_only_what_the_owner_chose() {
    let t = TempDir::new("discard");
    let (a, _, _) = crashed(&t.0, 30_000, 1 << 20, 5_000);
    let (b, _, _) = crashed(&t.0, 30_000, 1 << 20, 5_000);
    discard(&a).unwrap();
    assert!(!a.exists() && b.exists());
    // Not a session: refused.
    let plain = t.0.join("0123456789abcdef");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(matches!(discard(&plain), Err(RecoverError::NotOurs(_))));
    assert!(plain.exists());
}

#[test]
fn a_zero_block_and_a_one_channel_document_round_trip() {
    let t = TempDir::new("zero");
    let s = Store::create_session(Some(&t.0), opts(1 << 20)).unwrap();
    let mono = Channel::from_pieces([Piece::whole(s.append(&[0.25; 300]).unwrap()).unwrap(), Piece::zeros(1_000).unwrap()]);
    let d = DocState::new(s.clone(), vec![mono], 44_100, SourceFormat::synthetic(44_100, 1)).unwrap();
    let h = History::new(d, u64::MAX);
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let want = content(h.current());
    let dir = s.dir().to_path_buf();
    drop(h);
    drop(s);
    let rec = recover(&dir, opts(1 << 20)).unwrap();
    assert_eq!(content(&rec.steps[0].1), want);
    assert_eq!(rec.steps[0].1.sample_rate(), 44_100);
}
