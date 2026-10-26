//! Match Loudness (v1 G3, issue #38; card wav-c-38-c): the effect over a whole document, checked
//! with `soundcraft-dsp`'s meter. Target: integrated loudness within 0.2 LU of the target and the
//! true peak at or below -1.0 dBTP.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use soundcraft_dsp::meter::LoudnessMeter;
use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Params, match_loudness};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-match-loudness-test-{}-{name}", std::process::id()));
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

fn channels(e: &Editor) -> Vec<Vec<f32>> {
    let doc = e.history.current();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// (integrated LUFS, true peak dBTP) by `soundcraft-dsp`'s meter.
fn meter(ch: &[Vec<f32>]) -> (f32, f32) {
    let mut m = LoudnessMeter::new(RATE as f32, ch.len());
    m.process(ch, ch[0].len());
    (m.integrated_lufs(), m.true_peak_db())
}

/// 10 s of a 1 kHz sine at peak `amp` on both channels.
fn quiet_sine(amp: f64) -> Vec<Vec<f32>> {
    let x: Vec<f32> = (0..10 * RATE as usize).map(|k| (amp * (2.0 * PI * 1000.0 * k as f64 / f64::from(RATE)).sin()) as f32).collect();
    vec![x.clone(), x]
}

/// 10 s, both channels: a 440 Hz sine at 0.1 with a 2 ms burst of 1 kHz at 0.9 every 2 s
/// (so the true peak is near -0.9 dBTP while the loudness is near -20 LUFS).
fn peaky() -> Vec<Vec<f32>> {
    let x: Vec<f32> = (0..10 * RATE as usize)
        .map(|k| {
            let t = k as f64 / f64::from(RATE);
            if k % (2 * RATE as usize) < 96 { 0.9 * (2.0 * PI * 1000.0 * t).sin() } else { 0.1 * (2.0 * PI * 440.0 * t).sin() }
        })
        .map(|v| v as f32)
        .collect();
    vec![x.clone(), x]
}

fn index_of(id: &str) -> u16 {
    match by_name(&format!("effect.{id}")).expect("registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    }
}

/// Runs Match Loudness with `args` (`key=value` over the defaults) on the whole of `data` and
/// returns the result's channels.
fn matched(name: &str, data: &[Vec<f32>], args: &str) -> Vec<Vec<f32>> {
    let tmp = TempDir::new(name);
    let mut e = editor(&tmp.0, data);
    let effect = effects::find("match_loudness").expect("match_loudness is registered");
    let p = effects::parse_params(effect, args).unwrap();
    let len = data[0].len() as u64;
    e.run_effect(index_of("match_loudness"), p, 0..len).unwrap();
    channels(&e)
}

#[test]
fn match_loudness_is_registered_with_the_owner_default() {
    let effect = effects::find("match_loudness").expect("match_loudness is registered");
    assert_eq!(effect.name(), "Match Loudness");
    assert!(by_name("effect.match_loudness").is_some());
    assert_eq!(effect.parameters(), match_loudness::PARAMS);
    assert_eq!(Params::defaults(effect), vec![0.0, -16.0, 0.5, -1.0, 1.0, 12.0, 200.0]);
}

#[test]
fn match_loudness_measure_reports_loudness_and_true_peak() {
    let tmp = TempDir::new("measure");
    let data = quiet_sine(0.05);
    let e = editor(&tmp.0, &data);
    let doc = e.history.current();
    let r = match_loudness::measure(doc, 0..0, &[0, 1]).unwrap();
    let (lufs, tp) = meter(&data);
    assert!((r.integrated_lufs - f64::from(lufs)).abs() <= 1e-3, "measure {} vs meter {lufs}", r.integrated_lufs);
    assert!((r.true_peak_db - f64::from(tp)).abs() <= 1e-3, "measure {} vs meter {tp}", r.true_peak_db);
    // a 1 kHz sine peaking at 0.05 on both channels: about -26.02 LUFS and -26.02 dBTP
    assert!((r.integrated_lufs + 26.02).abs() <= 0.1, "integrated {}", r.integrated_lufs);
    assert!((r.true_peak_db + 26.02).abs() <= 0.1, "true peak {}", r.true_peak_db);
}

#[test]
fn match_loudness_raises_a_quiet_file_to_minus_16() {
    let out = matched("quiet", &quiet_sine(0.05), "");
    let (lufs, tp) = meter(&out);
    assert!((lufs + 16.0).abs() <= 0.2, "integrated {lufs} LUFS, want -16 +/- 0.2");
    assert!(tp <= -1.0, "true peak {tp} dBTP");
}

#[test]
fn match_loudness_limits_a_peaky_file_and_still_reaches_minus_16() {
    let input = peaky();
    let (_, tp_in) = meter(&input);
    assert!(tp_in > -1.0, "the test signal should already peak above -1 dBTP ({tp_in})");
    let out = matched("peaky", &input, "");
    let (lufs, tp) = meter(&out);
    assert!((lufs + 16.0).abs() <= 0.2, "integrated {lufs} LUFS, want -16 +/- 0.2");
    assert!(tp <= -1.0, "true peak {tp} dBTP, want at or below -1");
    assert!(tp >= -1.6, "true peak {tp} dBTP: limited far more than needed");
}

#[test]
fn match_loudness_ebu_r128_goes_to_minus_23() {
    let out = matched("ebu", &quiet_sine(0.05), "match_to=1");
    let (lufs, tp) = meter(&out);
    assert!((lufs + 23.0).abs() <= 0.2, "integrated {lufs} LUFS, want -23 +/- 0.2");
    assert!(tp <= -1.0, "true peak {tp} dBTP");
}

#[test]
fn match_loudness_true_peak_mode_sets_the_peak() {
    let out = matched("peak", &quiet_sine(0.05), "match_to=2 target=-3");
    let (_, tp) = meter(&out);
    assert!((tp + 3.0).abs() <= 0.1, "true peak {tp} dBTP, want -3 +/- 0.1");
}

#[test]
fn match_loudness_without_limiting_never_passes_max_true_peak() {
    let out = matched("nolimit", &peaky(), "limiting=0");
    let (lufs, tp) = meter(&out);
    assert!(tp <= -0.99, "true peak {tp} dBTP, want at or below -1");
    assert!(lufs < -16.2, "integrated {lufs} LUFS: without limiting the gain stops at the peak ceiling");
}

#[test]
fn match_loudness_leaves_silence_alone() {
    let data = vec![vec![0.0f32; 2 * RATE as usize]; 2];
    let out = matched("silence", &data, "");
    assert!(out.iter().all(|c| c.iter().all(|&v| v == 0.0)), "silence should stay silent");
}
