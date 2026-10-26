//! Normalize (E3, #29): the result's sample peak equals the target to 0.01 dB, in % and in dB;
//! Normalize All Channels Equally (on by default) and per channel; DC Bias Adjust (off by
//! default); a selection measured on its own; masked channels neither measured nor changed.
//! Every input has a known peak (one spike over a quieter tone), so every expected value is fixed.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Params};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-normalize-test-{}-{name}", std::process::id()));
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
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
    v
}

/// A 440 Hz sine of amplitude `amp` with one sample set to `spike` at frame `at`.
fn tone_with_spike(amp: f64, frames: usize, at: usize, spike: f32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..frames).map(|k| (amp * (2.0 * PI * 440.0 * k as f64 / f64::from(RATE)).sin()) as f32).collect();
    v[at] = spike;
    v
}

fn peak(v: &[f32]) -> f64 {
    v.iter().map(|x| f64::from(x.abs())).fold(0.0, f64::max)
}

fn db(linear: f64) -> f64 {
    20.0 * linear.log10()
}

fn index_of(id: &str) -> u16 {
    match by_name(&format!("effect.{id}")).expect("registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    }
}

fn run(e: &mut Editor, args: &str, range: std::ops::Range<u64>) {
    let effect = effects::find("normalize").expect("normalize is registered");
    let p = effects::parse_params(effect, args).unwrap();
    e.run_effect(index_of("normalize"), p, range).unwrap();
}

fn scaled(v: &[f32], k: f32) -> Vec<f32> {
    v.iter().map(|x| x * k).collect()
}

#[test]
fn normalize_is_registered_with_audition_defaults() {
    let n = effects::find("normalize").expect("normalize is in the registry");
    assert_eq!(n.name(), "Normalize");
    assert!(by_name("effect.normalize").is_some(), "effect.normalize in the action table");
    let ids: Vec<_> = n.parameters().iter().map(|p| p.id).collect();
    assert_eq!(ids, ["percent", "db", "unit", "equally", "dc_bias"]);
    assert_eq!(Params::defaults(n), vec![100.0, 0.0, 0.0, 1.0, 0.0]);
    assert_eq!(effects::parse_params(n, "percent=50").unwrap(), vec![50.0, 0.0, 0.0, 1.0, 0.0]);
    assert_eq!(effects::parse_params(n, "db=-3dB unit=dB").unwrap(), vec![100.0, -3.0, 1.0, 1.0, 0.0]);
    assert_eq!(effects::parse_params(n, "equally=off dc_bias=on").unwrap(), vec![100.0, 0.0, 0.0, 0.0, 1.0]);
    assert!(effects::parse_params(n, "percent=150").is_err());
    assert!(effects::parse_params(n, "db=1").is_err());
}

#[test]
fn normalize_default_brings_the_whole_file_to_100_percent() {
    let tmp = TempDir::new("default");
    let n = 3 * BLOCK_FRAMES + 500; // crosses chunk joins
    let input = tone_with_spike(0.4, n, BLOCK_FRAMES + 10, -0.5);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, "", 0..0); // the whole file, default settings
    let out = channel(&e, 0);
    assert_eq!(out[BLOCK_FRAMES + 10], -1.0, "the negative spike is the peak and lands on full scale");
    assert!(db(peak(&out)).abs() <= 0.01, "peak {} dBFS, want 0", db(peak(&out)));
    assert_eq!(out, scaled(&input, 2.0), "every sample times 2");
    assert_eq!(e.history.undo_label(), Some("Normalize"));
}

#[test]
fn normalize_to_a_db_target() {
    let tmp = TempDir::new("db");
    let n = 2 * BLOCK_FRAMES + 77;
    let input = tone_with_spike(0.4, n, 1000, 0.5);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, "unit=dB db=-6", 0..0);
    let got = db(peak(&channel(&e, 0)));
    assert!((got + 6.0).abs() <= 0.01, "peak {got} dBFS, want -6");
}

#[test]
fn normalize_to_a_percent_target() {
    let tmp = TempDir::new("percent");
    let n = 2 * BLOCK_FRAMES + 77;
    let input = tone_with_spike(0.4, n, 1000, 0.5);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, "percent=25", 0..0);
    let out = channel(&e, 0);
    assert!((db(peak(&out)) - db(0.25)).abs() <= 0.01, "peak {}, want 0.25", peak(&out));
    assert_eq!(out, scaled(&input, 0.5), "every sample times 0.5");
}

#[test]
fn normalize_all_channels_equally_keeps_the_balance() {
    let tmp = TempDir::new("equally");
    let n = 2 * BLOCK_FRAMES + 5;
    let l = tone_with_spike(0.4, n, 300, 0.5);
    let r = tone_with_spike(0.2, n, 90_000, 0.25);
    let mut e = editor(&tmp.0, &[l.clone(), r.clone()]);
    run(&mut e, "", 0..0);
    assert_eq!(channel(&e, 0), scaled(&l, 2.0), "left: gain set by the loudest channel");
    assert_eq!(channel(&e, 1), scaled(&r, 2.0), "right: the same gain, so it peaks at 0.5");
}

#[test]
fn normalize_channels_separately_when_equally_is_off() {
    let tmp = TempDir::new("separately");
    let n = 2 * BLOCK_FRAMES + 5;
    let l = tone_with_spike(0.4, n, 300, 0.5);
    let r = tone_with_spike(0.2, n, 90_000, 0.25);
    let mut e = editor(&tmp.0, &[l.clone(), r.clone()]);
    run(&mut e, "equally=off", 0..0);
    assert_eq!(channel(&e, 0), scaled(&l, 2.0), "left: 0.5 to 1.0");
    assert_eq!(channel(&e, 1), scaled(&r, 4.0), "right: 0.25 to 1.0");
}

#[test]
fn normalize_dc_bias_adjust_removes_the_mean_first() {
    // 0.5, 0.0, 0.5, 0.0, ...: mean 0.25, so centred it swings +/-0.25.
    let n = 2 * BLOCK_FRAMES + 2;
    let input: Vec<f32> = (0..n).map(|k| if k % 2 == 0 { 0.5 } else { 0.0 }).collect();

    let tmp = TempDir::new("dc-on");
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, "dc_bias=on", 0..0);
    let want: Vec<f32> = (0..n).map(|k| if k % 2 == 0 { 1.0 } else { -1.0 }).collect();
    assert_eq!(channel(&e, 0), want, "centred, then +/-0.25 brought to +/-1.0");

    let tmp2 = TempDir::new("dc-off");
    let mut e = editor(&tmp2.0, std::slice::from_ref(&input));
    run(&mut e, "", 0..0);
    let want: Vec<f32> = (0..n).map(|k| if k % 2 == 0 { 1.0 } else { 0.0 }).collect();
    assert_eq!(channel(&e, 0), want, "off by default: the offset is scaled with the rest");
}

#[test]
fn normalize_a_selection_by_its_own_peak_and_leave_the_rest() {
    let tmp = TempDir::new("range");
    let n = 3 * BLOCK_FRAMES;
    let mut input = tone_with_spike(0.2, n, 100_000, 0.25);
    input[5] = 0.9; // louder, but outside the selection
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    let sel = 50_000..150_000u64;
    run(&mut e, "", sel.clone());
    let out = channel(&e, 0);
    assert_eq!(&out[..50_000], &input[..50_000], "before the selection: untouched");
    assert_eq!(&out[150_000..], &input[150_000..], "after the selection: untouched");
    assert_eq!(&out[50_000..150_000], &scaled(&input[50_000..150_000], 4.0)[..], "the selection's own peak 0.25 to 1.0");
}

#[test]
fn normalize_measures_only_the_active_channels() {
    let tmp = TempDir::new("mask");
    let n = BLOCK_FRAMES + 40;
    let l = tone_with_spike(0.2, n, 700, 0.25);
    let r = tone_with_spike(0.4, n, 800, 1.0);
    let mut e = editor(&tmp.0, &[l.clone(), r.clone()]);
    e.channels = ChannelMask::only(0);
    run(&mut e, "", 0..0);
    assert_eq!(channel(&e, 0), scaled(&l, 4.0), "the right channel's peak is not counted");
    assert_eq!(channel(&e, 1), r, "the disabled channel is untouched");
}

#[test]
fn normalize_leaves_a_silent_file_silent() {
    let tmp = TempDir::new("silent");
    let n = BLOCK_FRAMES + 3;
    let input = vec![0.0f32; n];
    let mut e = editor(&tmp.0, &[input.clone(), input.clone()]);
    run(&mut e, "equally=off dc_bias=on", 0..0);
    assert_eq!(channel(&e, 0), input);
    assert_eq!(channel(&e, 1), input);
}
