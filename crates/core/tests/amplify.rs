//! Acceptance tests for Amplify (E2, issue #28):
//! - Per-channel gain in dB (`left_db`, `right_db`), range -80 to +40 dB, default 0 dB.
//! - `Link Sliders` (`link`) on by default (`1.0`).
//! - Exact gain to 0.001 dB on a test signal across 64 K-frame block boundaries.
//! - Float headroom kept above 0 dBFS in the document until save.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::by_name;
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SampleType, SourceFormat};
use wavcraft_core::effects::{self, Amplify, ParamDesc, Params, parse_params};
use wavcraft_core::save::{Dither, open_wav, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-amplify-test-{}-{name}", std::process::id()));
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

#[test]
fn amplify_is_registered_with_audition_defaults() {
    let e = effects::find("amplify").expect("amplify is in REGISTRY");
    assert_eq!(e.id(), "amplify");
    assert_eq!(e.name(), "Amplify");
    assert!(by_name("effect.amplify").is_some(), "Amplify is exposed in the action table");

    let p = e.parameters();
    assert_eq!(p.len(), 3);
    assert_eq!(p[0], ParamDesc::db("left_db", "Left", -80.0, 40.0, 0.0));
    assert_eq!(p[1], ParamDesc::db("right_db", "Right", -80.0, 40.0, 0.0));
    assert_eq!(p[2], ParamDesc::choice("link", "Link Sliders", &["off", "on"], 1));

    assert_eq!(Params::defaults(e), vec![0.0, 0.0, 1.0]);
    assert_eq!(parse_params(e, "left_db=-6 right_db=3 link=off").unwrap(), vec![-6.0, 3.0, 0.0]);
    assert_eq!(parse_params(e, "left_db=12dB link=on").unwrap(), vec![12.0, 0.0, 1.0]);
    assert!(parse_params(e, "left_db=-80.1").is_err(), "below -80 dB must be rejected");
    assert!(parse_params(e, "right_db=40.1").is_err(), "above +40 dB must be rejected");
}

#[test]
fn amplify_exact_gain_to_thousandth_db_across_block_boundaries() {
    let t = TempDir::new("exact-db");
    let s = store(&t.0);
    let len = 2 * BLOCK_FRAMES + 500;
    let signal: Vec<f32> = (0..len).map(|k| 0.25 * ((std::f64::consts::TAU * 440.0 * k as f64 / f64::from(RATE)).sin() as f32)).collect();
    let doc = doc_from(&s, std::slice::from_ref(&signal));
    let cancel = AtomicBool::new(false);

    for target_db in [-12.0_f64, -6.0, 6.0, 20.0] {
        let params = [target_db, 0.0, 1.0];
        let edit = effects::apply(&Amplify, &params, &doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
        assert_eq!(edit.label, "Amplify");
        let out = &content(&edit.doc)[0];
        assert_eq!(out.len(), len);
        let mut checked = 0usize;
        for (&x_in, &x_out) in signal.iter().zip(out.iter()) {
            if x_in.abs() >= 0.05 {
                let measured_db = 20.0 * (f64::from(x_out.abs()) / f64::from(x_in.abs())).log10();
                assert!((measured_db - target_db).abs() <= 0.001, "target {target_db} dB, measured {measured_db} dB (in={x_in}, out={x_out})");
                checked += 1;
            }
        }
        assert!(checked > 100_000);
    }
}

#[test]
fn amplify_linked_by_default_scales_both_channels_equally() {
    let t = TempDir::new("linked");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.25; 1000], vec![-0.5; 1000]]);
    let cancel = AtomicBool::new(false);
    let params = parse_params(&Amplify, "left_db=20").unwrap();
    let edit = effects::apply(&Amplify, &params, &doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let c = content(&edit.doc);
    assert!(c[0].iter().all(|&v| (v - 2.5).abs() < 1e-5));
    assert!(c[1].iter().all(|&v| (v - (-5.0)).abs() < 1e-5));
}

#[test]
fn amplify_unlinked_scales_left_and_right_independently() {
    let t = TempDir::new("unlinked");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.5; 1000], vec![0.5; 1000]]);
    let cancel = AtomicBool::new(false);
    let params = parse_params(&Amplify, "left_db=-20 right_db=20 link=off").unwrap();
    let edit = effects::apply(&Amplify, &params, &doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let c = content(&edit.doc);
    assert!(c[0].iter().all(|&v| (v - 0.05).abs() < 1e-6));
    assert!(c[1].iter().all(|&v| (v - 5.0).abs() < 1e-5));
}

#[test]
fn amplify_respects_active_channel_mask() {
    let t = TempDir::new("mask");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.5; 1000], vec![0.5; 1000]]);
    let cancel = AtomicBool::new(false);
    let params = [-20.0, 20.0, 0.0];
    let edit = effects::apply(&Amplify, &params, &doc, 0..0, ChannelMask::only(1), Crossfades::NONE, &cancel).unwrap();
    let c = content(&edit.doc);
    assert!(c[0].iter().all(|&v| (v - 0.5).abs() < 1e-6), "inactive left channel stays untouched");
    assert!(c[1].iter().all(|&v| (v - 5.0).abs() < 1e-5), "active right channel uses right_db");
}

#[test]
fn amplify_selection_only_leaves_outside_frames_untouched() {
    let t = TempDir::new("selection");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.4; 500]]);
    let cancel = AtomicBool::new(false);
    let params = [20.0, 0.0, 1.0];
    let edit = effects::apply(&Amplify, &params, &doc, 100..200, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let c = content(&edit.doc);
    assert!(c[0][..100].iter().all(|&v| (v - 0.4).abs() < 1e-6));
    assert!(c[0][100..200].iter().all(|&v| (v - 4.0).abs() < 1e-5));
    assert!(c[0][200..].iter().all(|&v| (v - 0.4).abs() < 1e-6));
}

#[test]
fn amplify_zero_db_is_identity() {
    let t = TempDir::new("zero-db");
    let s = store(&t.0);
    let orig = vec![vec![0.125; 500], vec![-0.375; 500]];
    let doc = doc_from(&s, &orig);
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(&Amplify, &[0.0, 0.0, 1.0], &doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let got = content(&edit.doc);
    for (a, b) in orig.iter().zip(got.iter()) {
        let a_bits: Vec<u32> = a.iter().map(|x| x.to_bits()).collect();
        let b_bits: Vec<u32> = b.iter().map(|x| x.to_bits()).collect();
        assert_eq!(a_bits, b_bits);
    }
}

#[test]
fn amplify_keeps_float_headroom_above_zero_dbfs_until_save() {
    let t = TempDir::new("headroom");
    let s = store(&t.0);
    let doc = doc_from(&s, &[vec![0.5; 1000]]);
    let cancel = AtomicBool::new(false);

    // +20 dB (10x) pushes 0.5 (-6.02 dBFS) up to 5.0 (+13.98 dBFS), well above 0 dBFS.
    let boosted = effects::apply(&Amplify, &[20.0, 0.0, 1.0], &doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let boosted_samples = &content(&boosted.doc)[0];
    assert!(boosted_samples.iter().all(|&v| (v - 5.0).abs() < 1e-5), "float headroom kept above 0 dBFS in DocState");

    // Attenuating by -20 dB recovers the original 0.5 without clipping distortion.
    let attenuated = effects::apply(&Amplify, &[-20.0, 0.0, 1.0], &boosted.doc, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let recovered = &content(&attenuated.doc)[0];
    assert!(recovered.iter().all(|&v| (v - 0.5).abs() < 1e-5), "round-trip +20 dB then -20 dB recovers 0.5 unclipped");

    // Saving the +13.98 dBFS document to 32-bit float WAV keeps 5.0; saving to 16-bit PCM clamps at save time.
    let float_wav = t.0.join("float.wav");
    save_wav(&boosted.doc, &float_wav, SampleType::Float32, Dither::Off, &cancel).unwrap();
    let reopened_float = open_wav(&s, &float_wav, &cancel).unwrap();
    assert!(content(&reopened_float)[0].iter().all(|&v| (v - 5.0).abs() < 1e-5));

    let pcm16_wav = t.0.join("pcm16.wav");
    save_wav(&boosted.doc, &pcm16_wav, SampleType::Int16, Dither::Off, &cancel).unwrap();
    let reopened_pcm16 = open_wav(&s, &pcm16_wav, &cancel).unwrap();
    let max_int16 = 32767.0_f32 / 32768.0_f32;
    assert!(content(&reopened_pcm16)[0].iter().all(|&v| (v - max_int16).abs() < 1e-6));
}
