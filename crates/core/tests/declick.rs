//! DeClicker and Automatic Click Remover (B046, B072): scan finds clicks, repair redraws them.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{Editor, by_name};
use wavcraft_core::diagnostics::declick::{self, AutoParams, Params};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const FRAMES: usize = 4 * RATE as usize;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-declick-test-{}-{name}", std::process::id()));
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

fn now(e: &Editor) -> Vec<Vec<f32>> {
    let doc = e.history.current();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn sine(phase: f64) -> Vec<f32> {
    (0..FRAMES).map(|n| (0.5 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / f64::from(RATE) + phase).sin()) as f32).collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Gaussian white noise at -20 dBFS RMS (0.1).
fn noise(seed: u64) -> Vec<f32> {
    let mut r = Rng(seed);
    (0..FRAMES)
        .map(|_| {
            let (u, v) = (r.next().max(1e-12), r.next());
            (0.1 * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()) as f32
        })
        .collect()
}

/// (first frame, width) of the 20 synthetic clicks: widths 1..30, one across the first 64 K join.
fn click_plan() -> Vec<(u64, usize)> {
    let mut v: Vec<(u64, usize)> = (0..20).map(|i| (5_000 + i as u64 * 9_000, 1 + (i * 7) % 30)).collect();
    v[7] = (BLOCK_FRAMES as u64 - 6, 12);
    v
}

/// A click: an alternating-sign burst of size 0.8 for its whole width.
fn add_clicks(clean: &[f32]) -> Vec<f32> {
    let mut x = clean.to_vec();
    for (s, w) in click_plan() {
        for k in 0..w {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            x[s as usize + k] += (0.8 * sign) as f32;
        }
    }
    x
}

fn fixtures() -> Vec<(&'static str, Vec<Vec<f32>>)> {
    vec![("sine", vec![sine(0.0), sine(1.0)]), ("noise", vec![noise(1), noise(2)])]
}

#[test]
fn finds_the_clicks_within_2_ms_and_few_false_regions_on_clean_audio() {
    for (name, clean) in fixtures() {
        let dirty: Vec<Vec<f32>> = clean.iter().map(|c| add_clicks(c)).collect();
        let tmp = TempDir::new(&format!("scan-{name}"));
        let e = editor(&tmp.0, &dirty);
        let cancel = AtomicBool::new(false);
        let got = declick::scan(e.history.current(), 0..0, ChannelMask::ALL, &Params::default(), &cancel).unwrap();
        let tol = 2 * u64::from(RATE) / 1000;
        for ch in 0..2 {
            let found =
                click_plan().iter().filter(|(s, _)| got.regions.iter().any(|r| r.channel == ch && r.start <= s + tol && r.end() + tol >= *s)).count();
            assert!(found >= 18, "{name} ch{ch}: only {found} of 20 clicks found");
        }
        let tmp2 = TempDir::new(&format!("clean-{name}"));
        let e2 = editor(&tmp2.0, &clean);
        let none = declick::scan(e2.history.current(), 0..0, ChannelMask::ALL, &Params::default(), &cancel).unwrap();
        for ch in 0..2 {
            let n = none.regions.iter().filter(|r| r.channel == ch).count();
            assert!(n <= 2, "{name} ch{ch}: {n} false regions");
        }
    }
}

#[test]
fn repair_cuts_the_error_leaves_the_rest_bit_identical_and_is_one_undo() {
    for (name, clean) in fixtures() {
        let dirty: Vec<Vec<f32>> = clean.iter().map(|c| add_clicks(c)).collect();
        let tmp = TempDir::new(&format!("repair-{name}"));
        let mut e = editor(&tmp.0, &dirty);
        let doc = e.history.current().clone();
        let cancel = AtomicBool::new(false);
        let scan = declick::scan(&doc, 0..0, ChannelMask::ALL, &Params::default(), &cancel).unwrap();
        let r = declick::declick(&doc, 0..0, ChannelMask::ALL, &Params::default(), Crossfades::NONE, &cancel).unwrap();
        assert_eq!(r.found, scan.regions.len());
        assert_eq!(r.edit.label, "DeClicker");
        e.history.push_edit(r.edit);
        assert_eq!(e.history.undo_label(), Some("DeClicker"));
        let after = now(&e);
        let (mut before, mut left) = (0.0f64, 0.0f64);
        #[allow(clippy::needless_range_loop)]
        for ch in 0..2 {
            let mut inside = vec![false; FRAMES];
            for reg in scan.regions.iter().filter(|r| r.channel == ch) {
                for i in reg.start..reg.end() {
                    inside[i as usize] = true;
                }
            }
            for (s, w) in click_plan() {
                for i in s as usize..s as usize + w {
                    inside[i] = true;
                }
            }
            for i in 0..FRAMES {
                if inside[i] {
                    let (d0, d1) = (f64::from(dirty[ch][i] - clean[ch][i]), f64::from(after[ch][i] - clean[ch][i]));
                    before += d0 * d0;
                    left += d1 * d1;
                } else {
                    assert_eq!(after[ch][i].to_bits(), dirty[ch][i].to_bits(), "{name} ch{ch} frame {i} must be untouched");
                }
            }
        }
        let (rb, ra) = (before.sqrt(), left.sqrt());
        assert!(ra <= 0.2 * rb, "{name}: error fell from {rb} to {ra}");
        e.history.undo();
        assert_eq!(now(&e), dirty, "{name}: one undo restores the file exactly");
    }
}

#[test]
fn automatic_click_remover_matches_scan_plus_repair_at_the_mapped_settings() {
    let clean = [noise(5), sine(0.3)];
    let dirty: Vec<Vec<f32>> = clean.iter().map(|c| add_clicks(c)).collect();
    let tmp = TempDir::new("auto");
    let mut a = editor(&tmp.0, &dirty);
    let tmp2 = TempDir::new("manual");
    let mut b = editor(&tmp2.0, &dirty);
    let auto = AutoParams { threshold: 60.0, complexity: 40.0 };
    let id = by_name("diagnostics.autoClickRemover").expect("in the action table").id;
    assert!(a.run_auto_click(vec![auto.threshold, auto.complexity], 0..0).unwrap().doc_changed);
    assert_eq!(a.history.undo_label(), Some("Automatic Click Remover"));
    let mapped = auto.to_params();
    assert!(b.run_declick(vec![mapped.sensitivity, mapped.max_width_ms], 0..0).unwrap().doc_changed);
    assert_eq!(b.history.undo_label(), Some("DeClicker"));
    assert_eq!(now(&a), now(&b));
    assert_ne!(now(&a), dirty);
    a.history.undo();
    assert_eq!(now(&a), dirty);
    // The table row runs it with the defaults.
    assert!(a.run(id, 0..0).unwrap().doc_changed);
    let id = by_name("diagnostics.declick").expect("in the action table").id;
    b.history.undo();
    assert!(b.run(id, 0..0).unwrap().doc_changed);
}

#[test]
fn a_selection_starting_past_frame_zero_limits_the_repair() {
    let clean = [noise(9)];
    let dirty: Vec<Vec<f32>> = clean.iter().map(|c| add_clicks(c)).collect();
    let tmp = TempDir::new("sel");
    let mut e = editor(&tmp.0, &dirty);
    let sel = 60_000u64..100_000;
    assert!(e.run_declick(vec![50.0, 1.0], sel.clone()).unwrap().doc_changed);
    let after = now(&e);
    for i in 0..FRAMES {
        if (i as u64) < sel.start || (i as u64) >= sel.end {
            assert_eq!(after[0][i].to_bits(), dirty[0][i].to_bits(), "frame {i} outside the selection");
        }
    }
    assert_ne!(after, dirty);
}

#[test]
fn bad_settings_are_refused() {
    let tmp = TempDir::new("params");
    let e = editor(&tmp.0, &[sine(0.0)]);
    let doc = e.history.current();
    let cancel = AtomicBool::new(false);
    for bad in [
        Params { sensitivity: 0.0, max_width_ms: 1.0 },
        Params { sensitivity: 101.0, max_width_ms: 1.0 },
        Params { sensitivity: f64::NAN, max_width_ms: 1.0 },
        Params { sensitivity: 50.0, max_width_ms: 0.01 },
        Params { sensitivity: 50.0, max_width_ms: 9.0 },
    ] {
        assert!(declick::scan(doc, 0..0, ChannelMask::ALL, &bad, &cancel).is_err(), "{bad:?}");
    }
    assert!(
        declick::auto_click_remove(doc, 0..0, ChannelMask::ALL, &AutoParams { threshold: 50.0, complexity: 1000.0 }, Crossfades::NONE, &cancel)
            .is_err()
    );
    assert!(Params::parse("sensitivity=50% max_width_ms=2ms").is_ok());
    assert!(Params::parse("bogus=1").is_err());
    assert!(AutoParams::parse("threshold=30 complexity=10").is_ok());
}
