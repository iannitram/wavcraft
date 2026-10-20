//! Generators through the editor (f21, A303/A304/A308): insert at the cursor, replace the
//! selection, one undo step, streamed in chunks, repeatable noise.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::generate;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-generate-test-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn channel(e: &Editor, c: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut out = vec![0.0f32; doc.len_frames() as usize];
    assert_eq!(doc.read(c, 0, &mut out).unwrap(), out.len());
    out
}

fn index(id: &str) -> u16 {
    generate::all().iter().position(|g| g.id() == id).unwrap() as u16
}

fn settings(id: &str, text: &str) -> Vec<f64> {
    generate::parse_params(generate::find(id).unwrap(), text).unwrap()
}

#[test]
fn a_tone_is_inserted_at_the_cursor_on_every_channel_as_one_step() {
    let tmp = TempDir::new("insert");
    let mut e = editor(&tmp.0, &[vec![0.25; 1000], vec![-0.5; 1000]]);
    let p = settings("tone", "freq=480 dur=0.5 amp=-6 wave=sine");
    let out = e.generate(index("tone"), &p, 400..400, 1).unwrap();
    let n = (0.5 * f64::from(RATE)) as u64;
    assert!(out.doc_changed);
    assert_eq!(out.selection, 400..400 + n, "the new audio is selected");
    assert_eq!(e.history.current().len_frames(), 1000 + n);
    assert_eq!(e.history.undo_label(), Some("Generate Tones"));
    for (c, v) in [(0, 0.25f32), (1, -0.5)] {
        let x = channel(&e, c);
        assert!(x[..400].iter().all(|s| *s == v) && x[400 + n as usize..].iter().all(|s| *s == v), "channel {c} around the tone");
        let peak = x[400..400 + n as usize].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((f64::from(peak) - 10f64.powf(-6.0 / 20.0)).abs() < 1e-3, "peak {peak}");
    }
    e.history.undo();
    assert_eq!(e.history.current().len_frames(), 1000, "one undo removes it");
}

#[test]
fn a_tone_replaces_the_selection() {
    let tmp = TempDir::new("replace");
    let mut e = editor(&tmp.0, &[vec![0.25; 10_000]]);
    let p = settings("tone", "dur=0.05");
    let n = (0.05 * f64::from(RATE)) as u64;
    e.generate(index("tone"), &p, 1000..4000, 1).unwrap();
    assert_eq!(e.history.current().len_frames(), 10_000 - 3000 + n);
    let x = channel(&e, 0);
    assert!(x[..1000].iter().all(|s| *s == 0.25) && x[1000 + n as usize..].iter().all(|s| *s == 0.25));
}

#[test]
fn a_long_tone_is_continuous_across_chunks_and_has_the_asked_frequency() {
    let tmp = TempDir::new("long");
    let mut e = editor(&tmp.0, &[vec![0.0; 10]]);
    // 20 s: several 64 K-frame chunks. 400 Hz at 48 kHz is exactly 120 samples a cycle.
    let p = settings("tone", "freq=400 dur=20 amp=0");
    e.generate(index("tone"), &p, 10..10, 1).unwrap();
    let x = channel(&e, 0);
    let t = &x[10..];
    assert_eq!(t.len(), 20 * RATE as usize);
    for i in (0..t.len()).step_by(997) {
        let want = (std::f64::consts::TAU * 400.0 * i as f64 / f64::from(RATE)).sin();
        assert!((f64::from(t[i]) - want).abs() < 2e-4, "frame {i}: {} vs {want}", t[i]);
    }
    let rises = t.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    assert!((rises as i64 - 8000).abs() <= 1, "{rises} cycles in 20 s");
}

#[test]
fn noise_is_repeatable_with_a_seed() {
    let tmp = TempDir::new("noise");
    let run = |seed| {
        let mut e = editor(&tmp.0.join(format!("s{seed}")), &[vec![0.0; 100]]);
        e.generate(index("noise"), &settings("noise", "color=pink dur=0.3 amp=-12"), 0..0, seed).unwrap();
        channel(&e, 0)
    };
    std::fs::create_dir_all(tmp.0.join("s5")).unwrap();
    std::fs::create_dir_all(tmp.0.join("s6")).unwrap();
    let (a, b) = (run(5), run(5));
    assert_eq!(a, b);
    assert_ne!(a, run(6));
    assert!(a.iter().fold(0.0f32, |m, s| m.max(s.abs())) <= 10f32.powf(-12.0 / 20.0) + 1e-6);
}

#[test]
fn bad_settings_are_refused_and_add_no_step() {
    let tmp = TempDir::new("bad");
    let mut e = editor(&tmp.0, &[vec![0.0; 100]]);
    let steps = e.history.steps().len();
    assert!(e.generate(index("tone"), &[0.0; 8], 0..0, 1).is_err(), "frequency 0");
    assert!(e.generate(index("tone"), &[440.0], 0..0, 1).is_err(), "wrong count");
    assert!(e.generate(99, &[], 0..0, 1).is_err());
    assert_eq!(e.history.steps().len(), steps);
    assert_eq!(e.history.current().len_frames(), 100);
}
