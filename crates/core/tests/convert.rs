//! Convert Sample Type, Interpret Sample Rate and the conversion presets (roadmap A136, A160-A164).

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::Editor;
use wavcraft_core::convert::{self, ConvertSettings, PresetFile, Quality, builtin_presets};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-convert-test-{}-{name}", std::process::id()));
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

fn doc_at(dir: &Path, rate: u32, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap()
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

fn sine(rate: u32, hz: f64, frames: usize, amp: f64) -> Vec<f32> {
    (0..frames).map(|n| (amp * (2.0 * PI * hz * n as f64 / f64::from(rate)).sin()) as f32).collect()
}

fn run(doc: &DocState, s: &ConvertSettings) -> DocState {
    let e = convert::convert(doc, s, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(e.label, "Convert Sample Type");
    e.state
}

fn rate(r: u32) -> ConvertSettings {
    ConvertSettings { rate: Some(r), ..ConvertSettings::default() }
}

/// The RMS of `x[from..to]`, in dB relative to full scale.
fn rms_db(x: &[f32], from: usize, to: usize) -> f64 {
    let ms = x[from..to].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / (to - from) as f64;
    10.0 * ms.max(1e-30).log10()
}

#[test]
fn a_sine_resampled_up_matches_the_ideal_sine() {
    let t = TempDir::new("up");
    let doc = doc_at(&t.0, 44_100, &[sine(44_100, 1000.0, 44_100, 0.5)]);
    let out = run(&doc, &rate(48_000));
    assert_eq!((out.sample_rate(), out.len_frames()), (48_000, 48_000));
    let got = &content(&out)[0];
    let want = sine(48_000, 1000.0, 48_000, 0.5);
    let err: Vec<f32> = got.iter().zip(&want).map(|(a, b)| a - b).collect();
    // Away from the two ends (where the file stops) the error is far below the signal.
    let db = rms_db(&err, 2_000, 46_000);
    assert!(db < -80.0, "error {db:.1} dB re full scale");
}

#[test]
fn a_sine_resampled_down_keeps_its_level_and_a_tone_above_the_new_nyquist_is_removed() {
    let t = TempDir::new("down");
    let doc = doc_at(&t.0, 48_000, &[sine(48_000, 3000.0, 96_000, 0.5), sine(48_000, 12_000.0, 96_000, 0.5)]);
    let out = run(&doc, &rate(16_000));
    assert_eq!(out.len_frames(), 32_000);
    let c = content(&out);
    let kept = rms_db(&c[0], 2_000, 30_000);
    assert!((kept - rms_db(&sine(16_000, 3000.0, 32_000, 0.5), 2_000, 30_000)).abs() < 0.05, "3 kHz level {kept:.2} dB");
    let alias = rms_db(&c[1], 2_000, 30_000);
    assert!(alias < -80.0, "12 kHz tone leaves {alias:.1} dB behind");
}

#[test]
fn every_quality_gives_the_right_length_and_a_steady_level() {
    let t = TempDir::new("q");
    let doc = doc_at(&t.0, 44_100, &[vec![0.25; 30_000]]);
    for q in Quality::ALL {
        let out = run(&doc, &ConvertSettings { rate: Some(22_050), quality: q, ..ConvertSettings::default() });
        assert_eq!(out.len_frames(), 15_000, "{q:?}");
        let c = &content(&out)[0];
        assert!(c[1_000..14_000].iter().all(|v| (v - 0.25).abs() < 1e-4), "{q:?}: DC held");
    }
}

#[test]
fn blocks_of_a_long_file_join_without_a_seam() {
    // Three blocks in, so the resampler's history crosses the block joins.
    let n = BLOCK_FRAMES * 3 + 123;
    let t = TempDir::new("long");
    let doc = doc_at(&t.0, 44_100, &[sine(44_100, 440.0, n, 0.5)]);
    let out = run(&doc, &rate(48_000));
    assert_eq!(out.len_frames(), (n as u64 * 48_000 + 22_050) / 44_100);
    let got = &content(&out)[0];
    let want = sine(48_000, 440.0, got.len(), 0.5);
    let err: Vec<f32> = got.iter().zip(&want).map(|(a, b)| a - b).collect();
    assert!(rms_db(&err, 1_000, got.len() - 1_000) < -80.0);
    // The result was written as whole blocks of the store, never one whole-file buffer.
    assert!(out.channel(0).unwrap().piece_count() >= 3);
}

#[test]
fn mono_to_stereo_duplicates_and_shares_the_blocks() {
    let t = TempDir::new("m2s");
    let data = sine(48_000, 220.0, 1000, 0.5);
    let doc = doc_at(&t.0, 48_000, std::slice::from_ref(&data));
    let out = run(&doc, &ConvertSettings { channels: Some(2), ..ConvertSettings::default() });
    assert_eq!((out.channel_count(), out.len_frames()), (2, 1000));
    let c = content(&out);
    assert_eq!(c[0], data);
    assert_eq!(c[1], data);
    assert_eq!(out.source().channels, 2);
    // Both channels point at the same blocks: no second copy on disk.
    assert!(Arc::ptr_eq(out.channel(0).unwrap(), out.channel(1).unwrap()));
}

#[test]
fn stereo_to_mono_mixes_by_percent() {
    let t = TempDir::new("s2m");
    let l: Vec<f32> = (0..500).map(|n| n as f32 / 1000.0).collect();
    let r: Vec<f32> = (0..500).map(|n| -0.5 + n as f32 / 2000.0).collect();
    let doc = doc_at(&t.0, 48_000, &[l.clone(), r.clone()]);
    let mono = |pct: Vec<f32>| content(&run(&doc, &ConvertSettings { channels: Some(1), mix_pct: pct, ..ConvertSettings::default() }))[0].clone();
    let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    let mean: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) / 2.0).collect();
    assert!(near(&mono(vec![]), &mean), "default is half and half");
    assert!(near(&mono(vec![100.0, 0.0]), &l), "left only");
    assert!(near(&mono(vec![0.0, 100.0]), &r), "right only");
    let mix: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.25 * a + 0.75 * b).collect();
    assert!(near(&mono(vec![25.0, 75.0]), &mix));
}

#[test]
fn channel_changes_with_no_mapping_and_bad_numbers_are_refused() {
    let t = TempDir::new("bad");
    let doc = doc_at(&t.0, 48_000, &[vec![0.1; 100], vec![0.2; 100]]);
    let go = |s: ConvertSettings| convert::convert(&doc, &s, &AtomicBool::new(false), &mut |_| {}).unwrap_err().to_string();
    assert!(go(ConvertSettings { channels: Some(3), ..ConvertSettings::default() }).contains("not supported"));
    assert!(go(ConvertSettings { channels: Some(1), mix_pct: vec![100.0], ..ConvertSettings::default() }).contains("percentages"));
    assert!(go(rate(0)).contains("not a usable"));
    assert!(go(rate(9_999_999)).contains("not a usable"));
}

#[test]
fn bit_depth_is_a_label_that_changes_what_a_save_writes_and_no_sample() {
    let t = TempDir::new("bits");
    let data = sine(48_000, 100.0, 4800, 0.5);
    let doc = doc_at(&t.0, 48_000, std::slice::from_ref(&data));
    assert_eq!(SampleType::for_source(doc.source()), SampleType::Float32);
    let out = run(&doc, &ConvertSettings::parse("bits=16").unwrap());
    assert_eq!(SampleType::for_source(out.source()), SampleType::Int16);
    assert!(Arc::ptr_eq(out.channel(0).unwrap(), doc.channel(0).unwrap()), "no sample copied");
    let out24 = run(&doc, &ConvertSettings::parse("bits=24").unwrap());
    assert_eq!(SampleType::for_source(out24.source()), SampleType::Int24);
    // Choosing what the document already has changes nothing.
    let e = convert::convert(&doc, &ConvertSettings::parse("bits=32").unwrap(), &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(e.is_noop());
}

#[test]
fn a_conversion_is_one_undoable_step() {
    let t = TempDir::new("undo");
    let doc = doc_at(&t.0, 44_100, &[sine(44_100, 500.0, 44_100, 0.5), sine(44_100, 700.0, 44_100, 0.5)]);
    let before = content(&doc);
    let mut ed = Editor::new(History::new(doc, u64::MAX));
    let s = ConvertSettings::parse("rate=22050 channels=1 bits=16").unwrap();
    let e = convert::convert(ed.history.current(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
    ed.history.push_edit(e);
    let now = ed.history.current().clone();
    assert_eq!((now.sample_rate(), now.channel_count(), now.len_frames()), (22_050, 1, 22_050));
    assert_eq!(ed.history.steps().len(), 2, "Open plus one step");
    assert_eq!(ed.history.undo_label(), Some("Convert Sample Type"));
    ed.history.undo();
    let back = ed.history.current();
    assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (44_100, 2, 44_100));
    assert_eq!(content(back), before);
    ed.history.redo();
    assert_eq!(ed.history.current().sample_rate(), 22_050);
}

#[test]
fn interpret_sample_rate_relabels_without_touching_a_sample() {
    let t = TempDir::new("interp");
    let doc = doc_at(&t.0, 44_100, &[sine(44_100, 500.0, 44_100, 0.5)]);
    let mut ed = Editor::new(History::new(doc.clone(), u64::MAX));
    let e = convert::interpret_rate(&doc, 48_000).unwrap();
    assert_eq!(e.label, "Interpret Sample Rate");
    ed.history.push_edit(e);
    let now = ed.history.current().clone();
    assert_eq!((now.sample_rate(), now.len_frames()), (48_000, 44_100), "same frames, new rate");
    assert_eq!(content(&now), content(&doc));
    assert_eq!(ed.history.steps().len(), 2, "recorded even though every channel is shared");
    ed.history.undo();
    assert_eq!(ed.history.current().sample_rate(), 44_100);
    assert!(convert::interpret_rate(&doc, 44_100).unwrap().is_noop());
    assert!(convert::interpret_rate(&doc, 0).is_err());
}

#[test]
fn cancel_stops_a_conversion_and_changes_nothing() {
    let t = TempDir::new("cancel");
    let doc = doc_at(&t.0, 44_100, &[vec![0.1; BLOCK_FRAMES * 2]]);
    let r = convert::convert(&doc, &rate(48_000), &AtomicBool::new(true), &mut |_| {});
    assert!(matches!(r, Err(wavcraft_core::EditError::Cancelled)));
}

fn wav_header(path: &Path) -> (u16, u16, u32, u16) {
    let b = std::fs::read(path).unwrap();
    let u16at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    (u16at(20), u16at(22), u32::from_le_bytes([b[24], b[25], b[26], b[27]]), u16at(34))
}

#[test]
fn a_converted_document_saves_at_its_new_rate_channels_and_depth() {
    let t = TempDir::new("save");
    let doc = doc_at(&t.0.join("s"), 48_000, &[sine(48_000, 300.0, 48_000, 0.5), sine(48_000, 400.0, 48_000, 0.5)]);
    let out = run(&doc, &ConvertSettings::parse("rate=24000 channels=1 bits=16").unwrap());
    let file = t.0.join("out.wav");
    save_wav(&out, &file, SampleType::for_source(out.source()), &mut |_| {}).unwrap();
    // (format tag, channels, rate, bits)
    assert_eq!(wav_header(&file), (1, 1, 24_000, 16));
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 44 + 24_000 * 2);
}

#[test]
fn settings_text_round_trips_and_rejects_nonsense() {
    let s = ConvertSettings::parse("rate=44100 channels=1 bits=24 quality=best mix=30,70").unwrap();
    assert_eq!(
        s,
        ConvertSettings {
            rate: Some(44_100),
            channels: Some(1),
            mix_pct: vec![30.0, 70.0],
            bits: Some(SampleType::Int24),
            dither: None,
            quality: Quality::Best
        }
    );
    assert_eq!(ConvertSettings::parse(&s.to_text()).unwrap(), s);
    for bad in ["rate=fast", "bits=12", "quality=ultra", "colour=red", "rate"] {
        assert!(ConvertSettings::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn presets_save_reload_and_delete_and_the_built_in_ones_stay() {
    let t = TempDir::new("presets");
    let path = t.0.join("convert-presets.txt");
    let mut f = PresetFile::load(Some(path.clone()));
    assert!(f.user().is_empty());
    let builtin = builtin_presets();
    assert!(builtin.len() >= 3);
    assert!(f.all().len() == builtin.len());
    let s = ConvertSettings::parse("rate=32000 channels=1 bits=16 quality=normal").unwrap();
    f.save("Phone memo", s.clone()).unwrap();
    f.save("phone MEMO", s.clone()).unwrap();
    assert_eq!(f.user().len(), 1, "same name replaces");
    let again = PresetFile::load(Some(path.clone()));
    assert_eq!(again.find("Phone memo").unwrap().settings, s);
    assert!(f.save(&builtin[0].name, s.clone()).is_err(), "built-in name refused");
    assert!(f.save("a|b", s.clone()).is_err());
    assert!(f.delete(&builtin[0].name).is_err());
    f.delete("Phone memo").unwrap();
    assert!(PresetFile::load(Some(path)).user().is_empty());
    assert!(f.delete("Phone memo").is_err());
}
