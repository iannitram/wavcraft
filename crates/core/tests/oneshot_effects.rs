//! B002 and the one-shot effects (Reverse, Amplify, Normalize, Fade In, Fade Out) through the
//! action table, on documents longer than one 64 K-frame chunk so the streaming path is used.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-effects-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path) -> Store {
    Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

fn doc_from(s: &Store, data: &[Vec<f32>]) -> DocState {
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<f32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// An editor on `data` with crossfades off, so contents can be checked exactly.
fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = store(dir);
    let mut e = Editor::new(History::new(doc_from(&s, data), u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn now(e: &Editor) -> Vec<Vec<f32>> {
    content(e.history.current())
}

const LEN: usize = 150_000;

/// Two channels, a distinct exact ramp each, longer than two chunks.
fn ramps() -> Vec<Vec<f32>> {
    vec![(0..LEN).map(|k| (k as f32) * 1e-6).collect(), (0..LEN).map(|k| -(k as f32) * 2e-6 - 1e-6).collect()]
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn run(e: &mut Editor, name: &str, sel: std::ops::Range<u64>) {
    let out = e.run(by_name(name).unwrap().id, sel).unwrap();
    assert!(out.doc_changed, "{name} changed nothing");
}

#[test]
fn reverse_mirrors_the_selection_across_chunks_and_undoes() {
    let t = TempDir::new("rev");
    let data = ramps();
    let mut e = editor(&t.0, &data);
    run(&mut e, "effect.reverse", 1000..140_000);
    let c = now(&e);
    for ch in 0..2 {
        assert_eq!(&c[ch][..1000], &data[ch][..1000]);
        assert_eq!(&c[ch][140_000..], &data[ch][140_000..]);
        for k in [0usize, 1, 65_535, 65_536, 100_000, 138_999] {
            assert_eq!(c[ch][1000 + k], data[ch][139_999 - k], "channel {ch} offset {k}");
        }
    }
    assert_eq!(e.history.undo_label(), Some("Reverse"));
    e.run(ActionId::Undo, 0..0).unwrap();
    assert_eq!(now(&e), data);
}

#[test]
fn reverse_twice_is_the_original() {
    let t = TempDir::new("rev2");
    let data = ramps();
    let mut e = editor(&t.0, &data);
    run(&mut e, "effect.reverse", 5..LEN as u64 - 7);
    run(&mut e, "effect.reverse", 5..LEN as u64 - 7);
    assert_eq!(now(&e), data);
}

#[test]
fn amplify_applies_the_db_to_the_selection_only_and_names_its_step() {
    let t = TempDir::new("amp");
    let data = ramps();
    let mut e = editor(&t.0, &data);
    e.set_param("amplify.db", 6.0).unwrap();
    run(&mut e, "effect.amplify", 70_000..120_000);
    let g = 10f32.powf(6.0 / 20.0);
    let c = now(&e);
    assert!(close(c[0][100_000], data[0][100_000] * g));
    assert!(close(c[1][70_000], data[1][70_000] * g));
    assert_eq!(c[0][69_999], data[0][69_999]);
    assert_eq!(c[0][120_000], data[0][120_000]);
    assert_eq!(e.history.undo_label(), Some("Amplify"));
    // Negative gain attenuates; the left channel only leaves the right alone.
    e.set_param("amplify.db", -20.0).unwrap();
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    let before = now(&e);
    run(&mut e, "effect.amplify", 0..1000);
    let after = now(&e);
    assert!(close(after[0][500], before[0][500] * 0.1));
    assert_eq!(after[1], before[1]);
}

#[test]
fn normalize_brings_the_peak_to_the_target() {
    let t = TempDir::new("norm");
    let data = ramps();
    let mut e = editor(&t.0, &data);
    e.set_param("normalize.db", -6.0).unwrap();
    let out = e.run(ActionId::Normalize, 0..0).unwrap(); // no selection: the whole file
    assert!(out.doc_changed, "normalize changed nothing");
    let c = now(&e);
    let peak = c.iter().flatten().fold(0.0f32, |p, v| p.max(v.abs()));
    let want = 10f32.powf(-6.0 / 20.0);
    assert!((peak - want).abs() < 1e-5, "peak {peak} want {want}");
    assert_eq!(e.history.undo_label(), Some("Normalize"));
}

#[test]
fn normalize_of_silence_is_no_edit() {
    let t = TempDir::new("norm0");
    let mut e = editor(&t.0, &[vec![0.0; 1000]]);
    let out = e.run(ActionId::Normalize, 0..1000).unwrap();
    assert!(!out.doc_changed);
}

#[test]
fn fades_follow_their_curves_over_the_selection() {
    let t = TempDir::new("fade");
    let ones = vec![vec![1.0f32; LEN]];
    let (a, b) = (10_000u64, 110_000u64);
    let n = (b - a) as f32;

    let mut e = editor(&t.0.join("li"), &ones);
    run(&mut e, "effect.fadeIn.linear", a..b);
    let c = &now(&e)[0];
    assert_eq!(c[a as usize - 1], 1.0);
    assert_eq!(c[a as usize], 0.0);
    assert!(close(c[(a + 50_000) as usize], 0.5));
    assert!(close(c[(b - 1) as usize], 1.0 - 1.0 / n));
    assert_eq!(c[b as usize], 1.0);
    assert_eq!(e.history.undo_label(), Some("Fade In (Linear)"));

    let mut e = editor(&t.0.join("ci"), &ones);
    run(&mut e, "effect.fadeIn.cosine", a..b);
    let c = &now(&e)[0];
    assert_eq!(c[a as usize], 0.0);
    assert!(close(c[(a + 50_000) as usize], 0.5));
    // Gentler than linear near the start.
    assert!(c[(a + 10_000) as usize] < 0.1, "a raised cosine starts slower than the 0.1 a line gives");
    assert_eq!(e.history.undo_label(), Some("Fade In (Raised Cosine)"));

    let mut e = editor(&t.0.join("lo"), &ones);
    run(&mut e, "effect.fadeOut.linear", a..b);
    let c = &now(&e)[0];
    assert_eq!(c[a as usize], (n - 1.0) / n);
    assert_eq!(c[(b - 1) as usize], 0.0);
    assert_eq!(c[b as usize], 1.0);
    assert_eq!(e.history.undo_label(), Some("Fade Out (Linear)"));

    let mut e = editor(&t.0.join("co"), &ones);
    run(&mut e, "effect.fadeOut.cosine", a..b);
    let c = &now(&e)[0];
    assert_eq!(c[(b - 1) as usize], 0.0);
    assert!((c[(a + 50_000) as usize] - 0.5).abs() < 1e-4);
    assert_eq!(e.history.undo_label(), Some("Fade Out (Raised Cosine)"));
}

#[test]
fn params_are_checked() {
    let t = TempDir::new("params");
    let mut e = editor(&t.0, &ramps());
    assert!(e.set_param("amplify.db", f32::NAN).is_err());
    assert!(e.set_param("amplify.db", 500.0).is_err());
    assert!(e.set_param("normalize.db", 30.0).is_err());
    assert!(e.set_param("volume", 1.0).is_err());
    assert!(e.set_param("normalize.db", -3.0).is_ok());
}
