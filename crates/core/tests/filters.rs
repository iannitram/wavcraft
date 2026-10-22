//! Filters and EQ (f27, B056/B057/B060/B117): measured frequency response (a sine in, RMS out),
//! DC removal, filter memory across the 64 K-frame chunks of `replace_range`, and per channel.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use soundcraft_dsp::biquad::{Biquad, FilterType};
use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-filters-test-{}-{name}", std::process::id()));
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

fn sine(freq: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|k| (0.5 * (2.0 * PI * freq * k as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

/// Run `id` with `args` over a whole mono sine of `freq` (2 s, so it crosses a chunk join) and
/// return the gain in dB, measured on the last second (after the filter has settled).
fn gain_at(id: &str, args: &str, freq: f64) -> f64 {
    let tmp = TempDir::new(&format!("{id}-{freq}"));
    let input = sine(freq, 2 * RATE as usize);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, id, args, 0..input.len() as u64);
    let out = channel(&e, 0);
    let tail = RATE as usize..out.len();
    20.0 * (rms(&out[tail.clone()]) / rms(&input[tail])).log10()
}

fn index_of(id: &str) -> u16 {
    match by_name(&format!("effect.{id}")).expect("registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    }
}

fn run(e: &mut Editor, id: &str, args: &str, range: std::ops::Range<u64>) {
    let effect = effects::find(id).unwrap();
    let p = effects::parse_params(effect, args).unwrap();
    e.run_effect(index_of(id), p, range).unwrap();
}

fn near(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: measured {got:.3} dB, expected {want:.3} dB (+/- {tol})");
}

#[test]
fn the_five_are_registered_with_settings() {
    for id in ["parametric_eq", "high_pass", "low_pass", "notch", "dc_offset"] {
        assert!(effects::find(id).is_some(), "{id}");
        assert!(by_name(&format!("effect.{id}")).is_some(), "effect.{id} in the action table");
    }
    assert_eq!(effects::find("parametric_eq").unwrap().parameters().len(), 10);
    assert_eq!(effects::find("high_pass").unwrap().parameters().len(), 2);
    assert_eq!(effects::find("notch").unwrap().parameters().len(), 3);
    assert!(effects::find("dc_offset").unwrap().parameters().is_empty());
    // the slope is a choice written as the number
    let p = effects::parse_params(effects::find("high_pass").unwrap(), "cutoff=120Hz slope=24").unwrap();
    assert_eq!(p, vec![120.0, 1.0]);
}

#[test]
fn parametric_eq_bells_and_shelves_hit_their_gain() {
    near(gain_at("parametric_eq", "mid1_freq=1000 mid1_gain_db=6 mid1_q=1", 1000.0), 6.0, 0.15, "bell +6 at its centre");
    near(gain_at("parametric_eq", "mid1_freq=1000 mid1_gain_db=6 mid1_q=1", 60.0), 0.0, 0.3, "bell far below");
    near(gain_at("parametric_eq", "mid2_freq=2000 mid2_gain_db=-9 mid2_q=2", 2000.0), -9.0, 0.15, "bell -9 at its centre");
    near(gain_at("parametric_eq", "low_freq=200 low_gain_db=6", 40.0), 6.0, 0.3, "low shelf +6 well below");
    near(gain_at("parametric_eq", "low_freq=200 low_gain_db=6", 5000.0), 0.0, 0.3, "low shelf well above");
    near(gain_at("parametric_eq", "high_freq=4000 high_gain_db=-6", 16000.0), -6.0, 0.4, "high shelf -6 well above");
    near(gain_at("parametric_eq", "high_freq=4000 high_gain_db=-6", 200.0), 0.0, 0.3, "high shelf well below");
    near(gain_at("parametric_eq", "", 1000.0), 0.0, 1e-3, "all bands flat");
}

#[test]
fn high_pass_and_low_pass_cut_at_the_slope() {
    near(gain_at("high_pass", "cutoff=1000 slope=12", 1000.0), -3.01, 0.15, "high-pass 12 at cutoff");
    near(gain_at("high_pass", "cutoff=1000 slope=12", 250.0), -24.1, 1.0, "high-pass 12: two octaves down");
    near(gain_at("high_pass", "cutoff=1000 slope=24", 250.0), -48.2, 2.0, "high-pass 24: two octaves down");
    near(gain_at("high_pass", "cutoff=1000 slope=24", 1000.0), -3.01, 0.15, "high-pass 24 at cutoff");
    near(gain_at("high_pass", "cutoff=100 slope=24", 8000.0), 0.0, 0.1, "high-pass passes the highs");
    near(gain_at("low_pass", "cutoff=1000 slope=12", 1000.0), -3.01, 0.15, "low-pass 12 at cutoff");
    near(gain_at("low_pass", "cutoff=1000 slope=12", 4000.0), -24.1, 1.5, "low-pass 12: two octaves up");
    near(gain_at("low_pass", "cutoff=1000 slope=24", 4000.0), -48.2, 3.0, "low-pass 24: two octaves up");
    near(gain_at("low_pass", "cutoff=8000 slope=24", 200.0), 0.0, 0.1, "low-pass passes the lows");
}

#[test]
fn notch_cuts_its_centre_by_the_depth_and_leaves_the_rest() {
    near(gain_at("notch", "freq=1000 q=10 depth_db=40", 1000.0), -40.0, 0.5, "notch depth at its centre");
    near(gain_at("notch", "freq=1000 q=10 depth_db=20", 1000.0), -20.0, 0.3, "notch depth 20");
    near(gain_at("notch", "freq=1000 q=10 depth_db=40", 100.0), 0.0, 0.1, "notch far below");
    near(gain_at("notch", "freq=1000 q=10 depth_db=40", 10000.0), 0.0, 0.1, "notch far above");
    // a lower Q is a wider notch: more cut half an octave away
    let narrow = gain_at("notch", "freq=1000 q=20 depth_db=40", 1400.0);
    let wide = gain_at("notch", "freq=1000 q=2 depth_db=40", 1400.0);
    assert!(wide < narrow - 3.0, "wide {wide:.2} dB should cut more than narrow {narrow:.2} dB at 1.4 kHz");
}

#[test]
fn dc_offset_is_removed_and_the_signal_kept() {
    let tmp = TempDir::new("dc");
    let n = 3 * BLOCK_FRAMES + 1234; // crosses several chunks
    let tone = sine(440.0, n);
    let input: Vec<f32> = tone.iter().map(|v| v + 0.3).collect();
    let right: Vec<f32> = tone.iter().map(|v| v - 0.1).collect();
    let mut e = editor(&tmp.0, &[input.clone(), right]);
    run(&mut e, "dc_offset", "", 0..0); // the whole file
    for c in 0..2 {
        let out = channel(&e, c);
        let mean = out.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
        assert!(mean.abs() < 1e-4, "channel {c} mean after: {mean}");
        let want = tone.clone();
        let worst = out.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 2e-3, "channel {c}: the tone is kept (worst difference {worst})");
    }
    assert_eq!(e.history.undo_label(), Some("Remove DC Offset"));
}

#[test]
fn filter_memory_carries_across_chunks_with_no_click() {
    let tmp = TempDir::new("chunks");
    let n = 3 * BLOCK_FRAMES + 777;
    let input = sine(300.0, n);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    run(&mut e, "high_pass", "cutoff=100 slope=24", 0..0);
    let out = channel(&e, 0);
    // The same filter run over the whole signal in one piece is the reference: equal sample for sample.
    let mut chain: Vec<Biquad> =
        [0.541_196_1f32, 1.306_563].iter().map(|&q| Biquad::design(FilterType::HighPass, 100.0, q, 0.0, RATE as f32)).collect();
    let mut want = input.clone();
    for s in &mut chain {
        s.process_block(&mut want);
    }
    let worst = out.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "chunked and one-piece output differ by {worst}");
    // And the step across each chunk join is no bigger than the signal's own steps.
    let own = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    for join in [BLOCK_FRAMES, 2 * BLOCK_FRAMES, 3 * BLOCK_FRAMES] {
        let step = (out[join] - out[join - 1]).abs();
        assert!(step <= own, "step {step} at chunk join {join} exceeds the largest step elsewhere {own}");
    }
    assert!(own < 0.1, "300 Hz at half scale steps at most about 0.04 per frame, got {own}");
}

#[test]
fn channels_are_filtered_separately_and_a_mask_leaves_the_others() {
    let tmp = TempDir::new("channels");
    let n = 2 * BLOCK_FRAMES + 99;
    let (l, r) = (sine(150.0, n), sine(5000.0, n));
    let mut e = editor(&tmp.0, &[l.clone(), r.clone()]);
    run(&mut e, "low_pass", "cutoff=1000 slope=12", 0..0);
    let (lo, ro) = (channel(&e, 0), channel(&e, 1));
    let mut alone = l.clone();
    let mut chain = Biquad::design(FilterType::LowPass, 1000.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, RATE as f32);
    chain.process_block(&mut alone);
    assert!(lo.iter().zip(&alone).all(|(a, b)| (a - b).abs() < 1e-6), "left is filtered as if alone: the right does not leak in");
    assert!(rms(&ro[RATE as usize / 2..]) < 0.1 * rms(&r), "the 5 kHz right channel is cut");
    // only the left enabled
    let tmp2 = TempDir::new("mask");
    let mut e = editor(&tmp2.0, &[l.clone(), r.clone()]);
    e.channels = wavcraft_core::doc::edit::ChannelMask::only(0);
    run(&mut e, "high_pass", "cutoff=1000 slope=12", 0..0);
    assert_eq!(channel(&e, 1), r, "the disabled channel is untouched");
    assert!(rms(&channel(&e, 0)[RATE as usize / 2..]) < 0.1 * rms(&l), "the enabled channel is filtered");
}

#[test]
fn a_selection_is_filtered_from_rest_and_the_rest_of_the_file_is_untouched() {
    let tmp = TempDir::new("range");
    let input = sine(100.0, 3 * BLOCK_FRAMES);
    let mut e = editor(&tmp.0, std::slice::from_ref(&input));
    let sel = 10_000..150_000u64;
    run(&mut e, "notch", "freq=100 q=5 depth_db=40", sel.clone());
    let out = channel(&e, 0);
    assert_eq!(&out[..10_000], &input[..10_000]);
    assert_eq!(&out[150_000..], &input[150_000..]);
    assert!(rms(&out[100_000..150_000]) < 0.03 * rms(&input[100_000..150_000]), "the notch has taken the 100 Hz tone out of the selection");
}

#[test]
fn out_of_range_settings_are_refused() {
    let hp = effects::find("high_pass").unwrap();
    assert!(effects::parse_params(hp, "cutoff=5").is_err());
    assert!(effects::parse_params(hp, "slope=36").is_err());
    assert!(effects::parse_params(effects::find("notch").unwrap(), "depth_db=90").is_err());
    assert!(effects::parse_params(effects::find("parametric_eq").unwrap(), "mid1_q=0").is_err());
}
