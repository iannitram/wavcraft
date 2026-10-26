//! Locked tests for v1 E1 (issue #27, card wav-c-27-c): Fade In and Fade Out effects
//! in the registry and action table, 0.01 dB accuracy at 100 points, selection and channel
//! masking, undo/repeat, and seamless streaming across 64 K-frame blocks.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::dsp::fade::FadeCurve;
use wavcraft_core::effects;
use wavcraft_core::effects::fade::{CURVE_NAMES, CURVE_PARAMS, curve_from_param};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-e1-fade-test-{}-{name}", std::process::id()));
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
    std::fs::create_dir_all(dir).unwrap();
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn read_ch(e: &Editor, ch: usize) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn effect_idx(id: &str) -> u16 {
    effects::all().iter().position(|e| e.id() == id).unwrap() as u16
}

#[test]
fn e1_effect_fade_in_and_out_are_registered_with_curve_parameter() {
    assert_eq!(CURVE_NAMES, &["Linear", "Logarithmic", "Cosine"]);
    let fi = effects::find("fade_in").unwrap();
    let fo = effects::find("fade_out").unwrap();
    assert_eq!(fi.name(), "Fade In");
    assert_eq!(fo.name(), "Fade Out");
    assert_eq!(fi.parameters(), CURVE_PARAMS);
    assert_eq!(fo.parameters(), CURVE_PARAMS);
    assert_eq!(by_name("effect.fade_in").unwrap().menu, "Fade In");
    assert_eq!(by_name("effect.fade_out").unwrap().menu, "Fade Out");
    assert_eq!(curve_from_param(&[]), FadeCurve::Linear);
    assert_eq!(curve_from_param(&[0.0]), FadeCurve::Linear);
    assert_eq!(curve_from_param(&[1.0]), FadeCurve::Logarithmic);
    assert_eq!(curve_from_param(&[2.0]), FadeCurve::Cosine);
    assert_eq!(curve_from_param(&[9.0]), FadeCurve::Linear);
}

#[test]
fn e1_effect_fade_in_and_out_match_formula_to_hundredth_db_at_100_points() {
    let tmp = TempDir::new("db100");
    for (param, tag) in [(0.0, "lin"), (1.0, "log"), (2.0, "cos")] {
        let mut e_in = editor(&tmp.0.join(format!("in-{tag}")), &[vec![1.0; 101]]);
        let mut e_out = editor(&tmp.0.join(format!("out-{tag}")), &[vec![1.0; 101]]);
        e_in.run_effect(effect_idx("fade_in"), vec![param], 0..101).unwrap();
        e_out.run_effect(effect_idx("fade_out"), vec![param], 0..101).unwrap();
        let got_in = read_ch(&e_in, 0);
        let got_out = read_ch(&e_out, 0);
        assert_eq!(got_in[0], 0.0);
        assert_eq!(got_out[100], 0.0);
        for k in 1..=100 {
            let t = k as f64 / 100.0;
            let want = match param as i64 {
                1 => (1.0 + 9.0 * t).log10(),
                2 => 0.5 * (1.0 - (PI * t).cos()),
                _ => t,
            };
            let s_in = f64::from(got_in[k]);
            let s_out = f64::from(got_out[100 - k]);
            assert!(s_in > 0.0 && s_out > 0.0, "{tag} point {k}");
            let db_in = (20.0 * (s_in / want).log10()).abs();
            let db_out = (20.0 * (s_out / want).log10()).abs();
            assert!(db_in <= 0.01, "{tag} fade_in point {k}: {db_in} dB");
            assert!(db_out <= 0.01, "{tag} fade_out point {}: {db_out} dB", 100 - k);
        }
    }
}

#[test]
fn e1_effect_fades_selection_only_and_active_channel_only() {
    let tmp = TempDir::new("sel-chan");
    let mut e = editor(&tmp.0, &[vec![1.0; 300], vec![-0.5; 300]]);
    e.run(ActionId::LeftChannel, 0..0).unwrap();
    let out = e.run_effect(effect_idx("fade_in"), vec![0.0], 100..201).unwrap();
    assert!(out.doc_changed);
    assert_eq!(out.selection, 100..201);
    assert_eq!(e.history.undo_label(), Some("Fade In"));
    let left = read_ch(&e, 0);
    let right = read_ch(&e, 1);
    assert!(left[..100].iter().all(|&x| x == 1.0));
    assert!(left[201..].iter().all(|&x| x == 1.0));
    assert_eq!(left[100], 0.0);
    assert!((left[150] - 0.5).abs() <= 1e-6);
    assert!((left[200] - 1.0).abs() <= 1e-6);
    assert!(right.iter().all(|&x| x == -0.5));
}

#[test]
fn e1_effect_whole_file_when_selection_empty_and_undo_restores() {
    let tmp = TempDir::new("whole-undo");
    let mut e = editor(&tmp.0, &[vec![0.8; 101]]);
    let fade_out = by_name("effect.fade_out").unwrap().id;
    let out = e.run(fade_out, 50..50).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("Fade Out"));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Fade Out");
    let after = read_ch(&e, 0);
    assert!((after[0] - 0.8).abs() <= 1e-6);
    assert!((after[50] - 0.4).abs() <= 1e-6);
    assert_eq!(after[100], 0.0);
    e.run(ActionId::Undo, 50..50).unwrap();
    assert!(read_ch(&e, 0).iter().all(|&x| (x - 0.8).abs() <= 1e-6));
}

#[test]
fn e1_effect_streams_continuously_across_multi_block_selection() {
    let tmp = TempDir::new("blocks");
    let len = BLOCK_FRAMES * 2 + 101; // 131_173 frames across 3 blocks; midpoint is frame 65_586
    let mut e_in = editor(&tmp.0.join("in"), &[vec![1.0; len]]);
    let mut e_out = editor(&tmp.0.join("out"), &[vec![1.0; len]]);
    e_in.run_effect(effect_idx("fade_in"), vec![0.0], 0..len as u64).unwrap();
    e_out.run_effect(effect_idx("fade_out"), vec![0.0], 0..len as u64).unwrap();
    let v_in = read_ch(&e_in, 0);
    let v_out = read_ch(&e_out, 0);
    let mid = len / 2;
    let at_boundary = BLOCK_FRAMES as f64 / (len - 1) as f64;
    assert!((f64::from(v_in[mid]) - 0.5).abs() <= 1e-6);
    assert!((f64::from(v_out[mid]) - 0.5).abs() <= 1e-6);
    assert!((f64::from(v_in[BLOCK_FRAMES]) - at_boundary).abs() <= 1e-6);
    assert!((f64::from(v_out[BLOCK_FRAMES]) - (1.0 - at_boundary)).abs() <= 1e-6);
}
