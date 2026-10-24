//! A/B compare for effect settings (B169): two sets of settings, which one is active, flip,
//! copy one onto the other, and apply the active one. Gain (`gain_db`, -60 to 24, default 0) is
//! the effect with a setting; Invert has none.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{AbCompare, Gain, Invert, Side};
use wavcraft_core::store::{Store, StoreOptions, BLOCK_FRAMES};
use wavcraft_core::EditError;

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-ab-compare-test-{}-{name}", std::process::id()));
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

fn doc(dir: &Path, data: &[f32]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let ch = Channel::from_pieces(data.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()));
    DocState::new(s, vec![ch], RATE, SourceFormat::synthetic(RATE, 1)).unwrap()
}

fn read(d: &DocState) -> Vec<f32> {
    let mut v = vec![f32::NAN; d.len_frames() as usize];
    assert_eq!(d.read(0, 0, &mut v).unwrap(), v.len());
    v
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-5 * b.abs().max(1.0)
}

fn two_sets() -> AbCompare {
    let mut ab = AbCompare::new(&Gain);
    ab.set_params(&Gain, Side::A, &[-20.0]).unwrap();
    ab.set_params(&Gain, Side::B, &[20.0]).unwrap();
    ab
}

#[test]
fn other_is_the_opposite_side() {
    assert_eq!(Side::A.other(), Side::B);
    assert_eq!(Side::B.other(), Side::A);
}

#[test]
fn a_new_compare_holds_the_defaults_on_both_sides_with_a_active() {
    let ab = AbCompare::new(&Gain);
    assert_eq!(ab.active(), Side::A);
    assert_eq!(ab.params(Side::A), &[0.0]);
    assert_eq!(ab.params(Side::B), &[0.0]);
    assert_eq!(ab.active_params(), &[0.0]);
    assert!(!ab.differs());
}

#[test]
fn from_params_puts_the_same_settings_on_both_sides() {
    let ab = AbCompare::from_params(&[-6.0]);
    assert_eq!(ab.active(), Side::A);
    assert_eq!(ab.params(Side::A), &[-6.0]);
    assert_eq!(ab.params(Side::B), &[-6.0]);
    assert!(!ab.differs());
}

#[test]
fn set_params_stores_one_side_only_and_keeps_the_active_side() {
    let mut ab = AbCompare::new(&Gain);
    ab.set_params(&Gain, Side::B, &[-12.5]).unwrap();
    assert_eq!(ab.params(Side::A), &[0.0]);
    assert_eq!(ab.params(Side::B), &[-12.5]);
    assert_eq!(ab.active(), Side::A);
    ab.set_params(&Gain, Side::A, &[3.0]).unwrap();
    assert_eq!(ab.params(Side::A), &[3.0]);
    assert_eq!(ab.params(Side::B), &[-12.5]);
}

#[test]
fn set_params_refuses_bad_settings_and_changes_nothing() {
    let mut ab = two_sets();
    let before = ab.clone();
    let too_loud = ab.set_params(&Gain, Side::A, &[99.0]).unwrap_err();
    assert!(too_loud.contains("gain_db"), "{too_loud}");
    let too_many = ab.set_params(&Gain, Side::B, &[1.0, 2.0]).unwrap_err();
    assert!(too_many.contains("takes 1 setting"), "{too_many}");
    assert!(ab.set_params(&Gain, Side::B, &[f64::NAN]).is_err());
    assert!(ab.set_params(&Gain, Side::A, &[]).is_err());
    assert_eq!(ab, before);
}

#[test]
fn toggle_flips_and_returns_the_new_side() {
    let mut ab = AbCompare::new(&Gain);
    assert_eq!(ab.toggle(), Side::B);
    assert_eq!(ab.active(), Side::B);
    assert_eq!(ab.toggle(), Side::A);
    assert_eq!(ab.active(), Side::A);
}

#[test]
fn toggling_keeps_both_sets() {
    let mut ab = two_sets();
    ab.toggle();
    ab.toggle();
    ab.toggle();
    assert_eq!(ab.params(Side::A), &[-20.0]);
    assert_eq!(ab.params(Side::B), &[20.0]);
}

#[test]
fn set_active_chooses_a_side_and_choosing_it_again_changes_nothing() {
    let mut ab = AbCompare::new(&Gain);
    ab.set_active(Side::B);
    assert_eq!(ab.active(), Side::B);
    ab.set_active(Side::B);
    assert_eq!(ab.active(), Side::B);
    ab.set_active(Side::A);
    assert_eq!(ab.active(), Side::A);
    ab.set_active(Side::A);
    assert_eq!(ab.active(), Side::A);
}

#[test]
fn active_params_follow_the_active_side() {
    let mut ab = two_sets();
    assert_eq!(ab.active_params(), &[-20.0]);
    ab.toggle();
    assert_eq!(ab.active_params(), &[20.0]);
    ab.set_active(Side::A);
    assert_eq!(ab.active_params(), &[-20.0]);
}

#[test]
fn copy_from_a_overwrites_b_and_leaves_a_and_the_active_side() {
    let mut ab = two_sets();
    ab.copy_to_other(Side::A);
    assert_eq!(ab.params(Side::A), &[-20.0]);
    assert_eq!(ab.params(Side::B), &[-20.0]);
    assert_eq!(ab.active(), Side::A);
    assert!(!ab.differs());
}

#[test]
fn copy_from_b_overwrites_a_and_leaves_b_and_the_active_side() {
    let mut ab = two_sets();
    ab.set_active(Side::B);
    ab.copy_to_other(Side::B);
    assert_eq!(ab.params(Side::A), &[20.0]);
    assert_eq!(ab.params(Side::B), &[20.0]);
    assert_eq!(ab.active(), Side::B);
    let mut ab = two_sets();
    ab.copy_to_other(Side::B);
    assert_eq!(ab.active(), Side::A);
    assert_eq!(ab.active_params(), &[20.0]);
}

#[test]
fn differs_tells_whether_the_two_sets_are_unequal() {
    let mut ab = AbCompare::new(&Gain);
    assert!(!ab.differs());
    ab.set_params(&Gain, Side::B, &[-3.0]).unwrap();
    assert!(ab.differs());
    ab.set_params(&Gain, Side::A, &[-3.0]).unwrap();
    assert!(!ab.differs());
}

#[test]
fn apply_active_runs_the_active_side_over_the_whole_file() {
    let t = TempDir::new("whole");
    let d = doc(&t.0, &[1.0; 100]);
    let mut ab = two_sets();
    let cancel = AtomicBool::new(false);
    let quiet = ab.apply_active(&Gain, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    assert_eq!(quiet.label, "Gain");
    assert!(read(&quiet.state).iter().all(|s| close(*s, 0.1)));
    ab.toggle();
    let loud = ab.apply_active(&Gain, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    assert!(read(&loud.state).iter().all(|s| close(*s, 10.0)));
    // The source state is never touched.
    assert!(read(&d).iter().all(|s| *s == 1.0));
}

#[test]
fn apply_active_only_changes_the_range() {
    let t = TempDir::new("range");
    let d = doc(&t.0, &[1.0; 100]);
    let ab = two_sets();
    let cancel = AtomicBool::new(false);
    let edit = ab.apply_active(&Gain, &d, 10..20, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    for (k, s) in read(&edit.state).iter().enumerate() {
        let want = if (10..20).contains(&k) { 0.1 } else { 1.0 };
        assert!(close(*s, want), "frame {k}: {s}");
    }
}

#[test]
fn apply_active_works_for_an_effect_without_settings() {
    let t = TempDir::new("invert");
    let d = doc(&t.0, &[0.25, -0.5, 1.0]);
    let ab = AbCompare::new(&Invert);
    assert!(ab.active_params().is_empty());
    let cancel = AtomicBool::new(false);
    let edit = ab.apply_active(&Invert, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    assert_eq!(edit.label, "Invert");
    assert_eq!(read(&edit.state), vec![-0.25, 0.5, -1.0]);
}

#[test]
fn apply_active_refuses_bad_settings_and_changes_nothing() {
    let t = TempDir::new("bad");
    let d = doc(&t.0, &[1.0; 8]);
    let ab = AbCompare::from_params(&[99.0]);
    let cancel = AtomicBool::new(false);
    let err = ab.apply_active(&Gain, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap_err();
    assert!(matches!(err, EditError::Io(_)), "{err:?}");
    assert!(read(&d).iter().all(|s| *s == 1.0));
}

#[test]
fn apply_active_stops_when_cancelled() {
    let t = TempDir::new("cancel");
    let d = doc(&t.0, &[1.0; 8]);
    let ab = two_sets();
    let cancel = AtomicBool::new(true);
    let err = ab.apply_active(&Gain, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap_err();
    assert!(matches!(err, EditError::Cancelled), "{err:?}");
}
