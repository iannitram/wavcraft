//! #990: the autosave journal carries each undo step's markers, so crash recovery gives every step
//! back with the markers it had (a marker-only step is not an identical copy of the one before).

use std::path::PathBuf;
use std::sync::Arc;

use wavcraft_core::autosave::{JOURNAL_FILE, RecoverError, Snapshot, recover, seal, unseal, write_journal};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{Store, StoreOptions};
use wavcraft_core::{History, Markers};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-990-{}-{name}", std::process::id()));
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
    StoreOptions { pack_bytes: 1 << 20, cache_bytes: 64 << 20, keep: true, ..StoreOptions::default() }
}

/// A stereo document of 4,000 frames with no markers.
fn doc(s: &Store) -> DocState {
    let chans: Vec<Channel> = (0..2)
        .map(|c| {
            let v: Vec<f32> = (0..4_000).map(|i| ((i * 31 + c * 7) % 1_000) as f32 / 1_000.0 - 0.5).collect();
            Channel::from_pieces([Piece::whole(s.append(&v).unwrap()).unwrap()])
        })
        .collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, 2)).unwrap()
}

/// Step 0 has no markers; step 1 "Add Marker" has three (one named with quotes, spaces, a percent
/// sign and a non-ASCII letter); step 2 "Delete Marker" removes the one with the highest id, so its
/// next id (3) is higher than its highest id plus one (2).
fn history(s: &Store) -> History {
    let mut h = History::new(doc(s), u64::MAX);
    let m = Markers::new()
        .with_added(100..100, Some("intro \"A\" %1 caf\u{e9}"))
        .and_then(|m| m.with_added(500..900, Some("verse")))
        .and_then(|m| m.with_added(3_000..3_000, Some("end")))
        .unwrap();
    h.push_markers("Add Marker", Arc::new(m.clone()));
    let ids: Vec<u64> = m.iter().map(|k| k.id).collect();
    assert_eq!(ids, [0, 1, 2]);
    h.push_markers("Delete Marker", Arc::new(m.without(&[2]).unwrap()));
    h
}

type Shown = Vec<(u64, String, u64, u64)>;

fn shown(m: &Markers) -> Shown {
    m.iter().map(|k| (k.id, k.name.clone(), k.start, k.end)).collect()
}

/// The id the next added marker would get: a marker added at a free spot, then read back.
fn next_id(m: &Markers) -> u64 {
    m.with_added(3_500..3_600, Some("probe")).unwrap().iter().find(|k| k.name == "probe").unwrap().id
}

/// Write the journal of a crashed run and return its folder.
fn crash(root: &std::path::Path, build: impl Fn(&Store) -> History) -> (PathBuf, Vec<(String, Shown, u64)>) {
    let s = Store::create_session(Some(root), opts()).unwrap();
    let h = build(&s);
    let want = h.steps().iter().map(|st| (st.label().to_string(), shown(st.state().markers()), next_id(st.state().markers()))).collect();
    write_journal(&s, &Snapshot::of(&h)).unwrap();
    let dir = s.dir().to_path_buf();
    drop(h);
    drop(s);
    (dir, want)
}

/// The journal's text with `edit` applied to its body lines, sealed again so its checksum holds.
fn rewrite_journal(dir: &std::path::Path, edit: impl Fn(Vec<String>) -> Vec<String>) -> (String, String) {
    let path = dir.join(JOURNAL_FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    let body = unseal(&text).unwrap();
    let lines = edit(body.lines().map(str::to_string).collect());
    let new = seal(&format!("{}\n", lines.join("\n")));
    std::fs::write(&path, &new).unwrap();
    (body.to_string(), new)
}

#[test]
fn each_step_recovers_with_the_same_markers_id_name_start_and_end() {
    let t = TempDir::new("markers");
    let (dir, want) = crash(&t.0, history);
    let rec = recover(&dir, opts()).unwrap();
    assert_eq!(rec.steps.len(), 3);
    let got: Vec<_> = rec.steps.iter().map(|(label, st)| (label.clone(), shown(st.markers()))).collect();
    let want_shown: Vec<_> = want.iter().map(|(label, m, _)| (label.clone(), m.clone())).collect();
    assert_eq!(got, want_shown, "every step comes back with its own markers");
    assert_eq!(got[0].1.len(), 0, "the step before any marker has none");
    assert_eq!(got[1].1.len(), 3);
    assert_eq!(got[2].1.len(), 2, "the step after Delete Marker has two");
}

#[test]
fn a_marker_only_step_comes_back_different_from_the_step_before_it() {
    let t = TempDir::new("markonly");
    let (dir, _) = crash(&t.0, history);
    let rec = recover(&dir, opts()).unwrap();
    assert_ne!(rec.steps[1].1.markers(), rec.steps[2].1.markers(), "Delete Marker changes the markers, not the audio");
    assert!(rec.steps[1].1.markers().iter().any(|k| k.id == 2), "the step before the delete still holds marker 2");
    assert!(rec.steps[2].1.markers().iter().all(|k| k.id != 2));
}

#[test]
fn the_next_marker_id_after_recovery_is_the_one_it_would_have_had() {
    let t = TempDir::new("nextid");
    let (dir, want) = crash(&t.0, history);
    assert_eq!(want[2].2, 3, "the original never reuses the deleted id 2");
    let rec = recover(&dir, opts()).unwrap();
    let got: Vec<u64> = rec.steps.iter().map(|(_, st)| next_id(st.markers())).collect();
    let want_ids: Vec<u64> = want.iter().map(|(_, _, n)| *n).collect();
    assert_eq!(got, want_ids, "ids stay unique across undo and redo");
}

#[test]
fn recovered_markers_stay_sorted_by_start_then_id() {
    let t = TempDir::new("sorted");
    let (dir, _) = crash(&t.0, history);
    let rec = recover(&dir, opts()).unwrap();
    let starts: Vec<(u64, u64)> = rec.steps[1].1.markers().iter().map(|k| (k.start, k.id)).collect();
    assert_eq!(starts, [(100, 0), (500, 1), (3_000, 2)]);
}

#[test]
fn a_journal_with_no_marker_lines_still_reads_with_no_markers() {
    let t = TempDir::new("old");
    let (dir, _) = crash(&t.0, history);
    let (before, after) = rewrite_journal(&dir, |lines| lines.into_iter().filter(|l| !l.starts_with("mark")).collect());
    assert!(before.lines().any(|l| l.starts_with("mark")), "the journal of a history with markers has marker lines");
    assert!(!after.lines().any(|l| l.starts_with("mark")));
    let rec = recover(&dir, opts()).unwrap();
    assert_eq!(rec.steps.len(), 3, "the steps are all there");
    assert!(rec.steps.iter().all(|(_, st)| st.markers().is_empty()), "an old journal recovers with no markers");
}

#[test]
fn a_history_without_markers_writes_no_marker_lines_and_recovers_with_none() {
    let t = TempDir::new("plain");
    let (dir, _) = crash(&t.0, |s| History::new(doc(s), u64::MAX));
    let text = std::fs::read_to_string(dir.join(JOURNAL_FILE)).unwrap();
    assert!(!text.lines().any(|l| l.starts_with("mark")), "nothing to record");
    let rec = recover(&dir, opts()).unwrap();
    assert!(rec.steps.iter().all(|(_, st)| st.markers().is_empty()));
}

#[test]
fn a_marker_line_naming_a_step_that_is_not_there_is_refused() {
    let t = TempDir::new("badstep");
    let (dir, _) = crash(&t.0, history);
    rewrite_journal(&dir, |mut lines| {
        let at = lines.iter().position(|l| l.starts_with("cursor ")).unwrap();
        lines.insert(at, "marks 99 1".to_string());
        lines.insert(at + 1, "mark 99 0 10 20 ghost".to_string());
        lines
    });
    let err = match recover(&dir, opts()) {
        Err(e) => e,
        Ok(_) => panic!("a journal whose marker names step 99 of 3 must not be recovered"),
    };
    assert!(matches!(err, RecoverError::Journal(_)), "{err}");
    assert!(err.to_string().contains("a marker names a step that is not there"), "{err}");
}
