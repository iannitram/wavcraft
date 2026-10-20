//! Channel Mixer (z13.8, B027): identity, swap, sum, invert, mono pass-through, parsing, a
//! selection, and one undo.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-chanmix-test-{}-{name}", std::process::id()));
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

fn stereo(frames: usize) -> Vec<Vec<f32>> {
    vec![(0..frames).map(|i| ((i % 97) as f32 - 48.0) / 64.0).collect(), (0..frames).map(|i| ((i % 53) as f32 - 26.0) / 40.0).collect()]
}

fn index() -> u16 {
    effects::all().iter().position(|e| e.id() == "channel_mixer").expect("registered") as u16
}

fn mixer() -> &'static dyn Effect {
    effects::find("channel_mixer").unwrap()
}

fn settings(text: &str) -> Vec<f64> {
    effects::parse_params(mixer(), text).unwrap()
}

fn run(e: &mut Editor, text: &str, sel: std::ops::Range<u64>) {
    e.run_effect(index(), settings(text), sel).unwrap();
}

#[test]
fn defaults_are_identity_bit_exact() {
    let tmp = TempDir::new("identity");
    let data = stereo(1000);
    let mut e = editor(&tmp.0, &data);
    e.run_effect(index(), effects::Params::defaults(mixer()), 0..0).unwrap();
    for (a, b) in now(&e).iter().flatten().zip(data.iter().flatten()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn swap_swaps_the_channels() {
    let tmp = TempDir::new("swap");
    let data = stereo(1000);
    let mut e = editor(&tmp.0, &data);
    run(&mut e, "l_from_l=0 l_from_r=100 r_from_l=100 r_from_r=0", 0..0);
    assert_eq!(now(&e), vec![data[1].clone(), data[0].clone()]);
}

#[test]
fn mono_sum_puts_the_average_in_both() {
    let tmp = TempDir::new("sum");
    let data = stereo(1000);
    let mut e = editor(&tmp.0, &data);
    run(&mut e, "l_from_l=50 l_from_r=50 r_from_l=50 r_from_r=50", 0..0);
    let out = now(&e);
    assert_eq!(out[0], out[1]);
    for n in 0..1000 {
        let want = ((f64::from(data[0][n]) + f64::from(data[1][n])) / 2.0) as f32;
        assert!((out[0][n] - want).abs() < 1e-6, "frame {n}");
    }
}

#[test]
fn invert_left_negates_left_only() {
    let tmp = TempDir::new("inv");
    let data = stereo(1000);
    let mut e = editor(&tmp.0, &data);
    run(&mut e, "invert_l=on", 0..0);
    let out = now(&e);
    assert_eq!(out[0], data[0].iter().map(|v| -v).collect::<Vec<_>>());
    assert_eq!(out[1], data[1]);
}

#[test]
fn a_mono_doc_passes_unchanged() {
    let tmp = TempDir::new("mono");
    let data = vec![stereo(1000).remove(0)];
    let mut e = editor(&tmp.0, &data);
    run(&mut e, "l_from_l=0 l_from_r=100 invert_l=on", 0..0);
    assert_eq!(now(&e), data);
}

#[test]
fn parse_accepts_percent_and_refuses_out_of_range() {
    let p = settings("l_from_r=50%");
    assert_eq!(p[1], 50.0);
    assert!(effects::parse_params(mixer(), "l_from_r=150").is_err());
    assert!(effects::parse_params(mixer(), "l_from_r=-150").is_err());
}

#[test]
fn a_selection_leaves_the_rest_alone_and_one_undo_restores() {
    let tmp = TempDir::new("sel");
    let data = stereo(BLOCK_FRAMES * 2 + 100);
    let mut e = editor(&tmp.0, &data);
    let (a, b) = (100u64, BLOCK_FRAMES as u64 + 500);
    run(&mut e, "l_from_l=0 l_from_r=100 r_from_l=100 r_from_r=0", a..b);
    let out = now(&e);
    for n in 0..data[0].len() {
        let inside = (n as u64) >= a && (n as u64) < b;
        let (wl, wr) = if inside { (data[1][n], data[0][n]) } else { (data[0][n], data[1][n]) };
        assert_eq!((out[0][n], out[1][n]), (wl, wr), "frame {n}");
    }
    assert_eq!(e.history.undo_label(), Some("Channel Mixer"));
    e.history.undo().unwrap();
    assert_eq!(now(&e), data);
}
