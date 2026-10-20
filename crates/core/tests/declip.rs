//! DeClipper (B045): scan finds clipped runs, repair rebuilds them as one undo step.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{Editor, by_name};
use wavcraft_core::diagnostics::declip::{self, Params};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-declip-test-{}-{name}", std::process::id()));
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

/// 1 kHz sine, `amp` amplitude, `frames` long; the right channel is phase-shifted.
fn sine(frames: usize, amp: f64, phase: f64) -> Vec<f32> {
    (0..frames).map(|n| (amp * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / f64::from(RATE) + phase).sin()) as f32).collect()
}

fn clip(v: &[f32]) -> Vec<f32> {
    v.iter().map(|x| x.clamp(-1.0, 1.0)).collect()
}

/// Runs of |x| >= the default -0.1 dB level, computed the dumb way.
fn runs(v: &[f32], min_run: usize) -> Vec<(u64, u64)> {
    let level = 10f32.powf(-0.1 / 20.0);
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i].abs() >= level {
            let s = i;
            while i < v.len() && v[i].abs() >= level && v[i].signum() == v[s].signum() {
                i += 1;
            }
            if i - s >= min_run {
                out.push((s as u64, (i - s) as u64));
            }
        } else {
            i += 1;
        }
    }
    out
}

struct Fixture {
    original: Vec<Vec<f32>>,
    clipped: Vec<Vec<f32>>,
}

fn fixture() -> Fixture {
    let frames = 3 * RATE as usize;
    // 0.9 scaled by 1.5, then hard-clipped at full scale.
    let original = vec![sine(frames, 0.9 * 1.5, 0.0), sine(frames, 0.9 * 1.5, 1.0)];
    let clipped = original.iter().map(|c| clip(c)).collect();
    Fixture { original, clipped }
}

#[test]
fn scan_finds_every_clipped_run_in_both_channels_including_across_a_chunk_join() {
    let f = fixture();
    let tmp = TempDir::new("scan");
    let e = editor(&tmp.0, &f.clipped);
    let doc = e.history.current();
    let got = declip::scan(doc, 0..0, ChannelMask::ALL, &Params::default(), &AtomicBool::new(false)).unwrap();
    assert!(!got.truncated);
    for ch in 0..2 {
        let want = runs(&f.clipped[ch], 3);
        let have: Vec<(u64, u64)> = got.regions.iter().filter(|r| r.channel == ch).map(|r| (r.start, r.len)).collect();
        assert_eq!(have, want, "channel {ch}");
        assert!(!want.is_empty());
    }
    // At least one run really straddles a 64 K join.
    let join = BLOCK_FRAMES as u64;
    assert!(
        got.regions.iter().any(|r| r.start < join && r.end() > join) || got.regions.iter().any(|r| r.start < 2 * join && r.end() > 2 * join),
        "a run must straddle a chunk join"
    );
    assert!(got.regions.iter().all(|r| r.peak >= 0.98));
}

#[test]
fn repair_cuts_the_error_by_70_percent_leaves_the_rest_bit_identical_and_is_one_undo() {
    let f = fixture();
    let tmp = TempDir::new("repair");
    let mut e = editor(&tmp.0, &f.clipped);
    let doc = e.history.current().clone();
    let cancel = AtomicBool::new(false);
    let scan = declip::scan(&doc, 0..0, ChannelMask::ALL, &Params::default(), &cancel).unwrap();
    let r = declip::declip(&doc, 0..0, ChannelMask::ALL, &Params::default(), Crossfades::NONE, &cancel).unwrap();
    assert_eq!(r.found, scan.regions.len());
    // The right channel starts inside a run at frame 0: no good samples before it to fit.
    assert_eq!((r.repaired, r.skipped), (scan.regions.len() - 1, 1));
    assert_eq!(r.edit.label, "DeClipper");
    e.history.push_edit(r.edit);
    assert_eq!(e.history.undo_label(), Some("DeClipper"));
    let after = now(&e);

    let (mut err_before, mut err_after) = (0.0f64, 0.0f64);
    #[allow(clippy::needless_range_loop)]
    for ch in 0..2 {
        let mut inside = vec![false; after[ch].len()];
        for r in scan.regions.iter().filter(|r| r.channel == ch) {
            for i in r.start..r.end() {
                inside[i as usize] = true;
                let d0 = f64::from(f.clipped[ch][i as usize]) - f64::from(f.original[ch][i as usize]);
                let d1 = f64::from(after[ch][i as usize]) - f64::from(f.original[ch][i as usize]);
                err_before += d0 * d0;
                err_after += d1 * d1;
            }
        }
        for (i, inn) in inside.iter().enumerate() {
            if !inn {
                assert_eq!(after[ch][i].to_bits(), f.clipped[ch][i].to_bits(), "ch {ch} frame {i} must be untouched");
            }
        }
    }
    let (rms_before, rms_after) = (err_before.sqrt(), err_after.sqrt());
    assert!(rms_after <= 0.3 * rms_before, "error fell from {rms_before} to {rms_after}");

    // Repaired peaks may exceed the old clip level.
    assert!(after.iter().flatten().any(|x| x.abs() > 1.0));

    e.history.undo();
    assert_eq!(now(&e), f.clipped, "one undo restores the clipped file exactly");
}

#[test]
fn clean_audio_has_no_regions_and_repair_changes_nothing() {
    let tmp = TempDir::new("clean");
    let e = editor(&tmp.0, &[sine(100_000, 0.5, 0.0), sine(100_000, 0.5, 0.3)]);
    let doc = e.history.current();
    let cancel = AtomicBool::new(false);
    assert!(declip::scan(doc, 0..0, ChannelMask::ALL, &Params::default(), &cancel).unwrap().regions.is_empty());
    let r = declip::declip(doc, 0..0, ChannelMask::ALL, &Params::default(), Crossfades::NONE, &cancel).unwrap();
    assert!(r.edit.is_noop());
}

#[test]
fn a_selection_and_a_channel_mask_limit_the_scan() {
    let f = fixture();
    let tmp = TempDir::new("limit");
    let e = editor(&tmp.0, &f.clipped);
    let doc = e.history.current();
    let cancel = AtomicBool::new(false);
    let left = declip::scan(doc, 0..0, ChannelMask::only(0), &Params::default(), &cancel).unwrap();
    assert!(left.regions.iter().all(|r| r.channel == 0) && !left.regions.is_empty());
    let part = declip::scan(doc, 1000..20_000, ChannelMask::ALL, &Params::default(), &cancel).unwrap();
    assert!(!part.regions.is_empty() && part.regions.iter().all(|r| r.start >= 1000 && r.end() <= 20_000));
}

#[test]
fn out_of_range_params_are_refused() {
    let tmp = TempDir::new("params");
    let e = editor(&tmp.0, &[sine(1000, 1.5, 0.0)]);
    let doc = e.history.current();
    let cancel = AtomicBool::new(false);
    for bad in [
        Params { threshold_db: 0.5, min_run: 3 },
        Params { threshold_db: -100.0, min_run: 3 },
        Params { threshold_db: f64::NAN, min_run: 3 },
        Params { threshold_db: -0.1, min_run: 0 },
        Params { threshold_db: -0.1, min_run: 1 },
        Params { threshold_db: -0.1, min_run: 100_000 },
    ] {
        assert!(declip::scan(doc, 0..0, ChannelMask::ALL, &bad, &cancel).is_err(), "{bad:?}");
        assert!(declip::declip(doc, 0..0, ChannelMask::ALL, &bad, Crossfades::NONE, &cancel).is_err(), "{bad:?}");
    }
}

#[test]
fn the_action_runs_scan_and_repair_in_one_undo_step() {
    let f = fixture();
    let tmp = TempDir::new("action");
    let mut e = editor(&tmp.0, &f.clipped);
    let id = by_name("diagnostics.declip").expect("in the action table").id;
    let out = e.run(id, 0..0).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("DeClipper"));
    assert!(now(&e).iter().flatten().any(|x| x.abs() > 1.0));
    e.history.undo();
    assert_eq!(now(&e), f.clipped);
}
