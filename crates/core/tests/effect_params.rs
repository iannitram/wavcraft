//! Effect parameters (f14, B006/A154): the parameter description, parsing `key=value` settings,
//! the Gain example effect, and Repeat Last Command carrying an effect's settings.

use std::io;
use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, Repeat, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect, ParamDesc, ParamKind};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-effparams-test-{}-{name}", std::process::id()));
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

fn mono(e: &Editor) -> Vec<f32> {
    let doc = e.history.current();
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(0, 0, &mut v).unwrap(), v.len());
    v
}

/// A test effect with one parameter of every kind.
struct Everything;

impl Effect for Everything {
    fn id(&self) -> &'static str {
        "everything"
    }
    fn name(&self) -> &'static str {
        "Everything"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::db("level", "Level", -40.0, 10.0, -3.0),
            ParamDesc::percent("mix", "Mix", 0.0, 100.0, 50.0),
            ParamDesc::time("delay", "Delay", 0.0, 500.0, 20.0, "ms"),
            ParamDesc::choice("shape", "Shape", &["Sine", "Square", "Saw"], 1),
        ];
        P
    }
    fn process(&self, _p: &[f64], _f: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        Ok(())
    }
}

#[test]
fn defaults_come_from_the_description() {
    assert_eq!(effects::Params::defaults(&Everything), vec![-3.0, 50.0, 20.0, 1.0]);
    assert_eq!(Everything.parameters()[0].kind, ParamKind::Db);
    assert_eq!(Everything.parameters()[3].kind, ParamKind::Choice);
}

#[test]
fn key_value_settings_parse_over_the_defaults() {
    let p = effects::parse_params(&Everything, "level=-6 delay=100ms shape=Saw").unwrap();
    assert_eq!(p, vec![-6.0, 50.0, 100.0, 2.0], "unset keys keep the default; a choice by name; a unit suffix is allowed");
    let p = effects::parse_params(&Everything, "mix=25% level=2dB shape=0").unwrap();
    assert_eq!(p, vec![2.0, 25.0, 20.0, 0.0], "a choice by index");
    assert_eq!(effects::parse_params(&Everything, "").unwrap(), effects::Params::defaults(&Everything));
}

#[test]
fn bad_settings_say_what_is_wrong() {
    let e = effects::parse_params(&Everything, "volume=3").unwrap_err();
    assert!(e.contains("volume") && e.contains("level"), "{e}");
    let e = effects::parse_params(&Everything, "level=40").unwrap_err();
    assert!(e.contains("level") && e.contains("-40") && e.contains("10"), "{e}");
    assert!(effects::parse_params(&Everything, "level=loud").is_err());
    assert!(effects::parse_params(&Everything, "level").is_err(), "needs key=value");
    assert!(effects::parse_params(&Everything, "shape=Triangle").is_err());
    assert!(effects::parse_params(&Everything, "shape=3").is_err());
    assert!(effects::parse_params(&Everything, "level=NaN").is_err());
    assert!(effects::validate(&Everything, &[0.0]).is_err(), "wrong count");
    assert!(effects::validate(&Everything, &effects::Params::defaults(&Everything)).is_ok());
}

#[test]
fn invert_has_no_parameters() {
    let inv = effects::find("invert").unwrap();
    assert!(inv.parameters().is_empty());
    assert_eq!(effects::parse_params(inv, "").unwrap(), Vec::<f64>::new());
    assert!(effects::parse_params(inv, "x=1").is_err());
}

fn gain_index() -> u16 {
    match by_name("effect.gain").expect("gain is registered").id {
        ActionId::Effect(i) => i,
        other => panic!("{other:?}"),
    }
}

#[test]
fn gain_scales_by_db() {
    let tmp = TempDir::new("gain");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    let g = effects::find("gain").unwrap();
    let p = effects::parse_params(g, "gain_db=-6.0206").unwrap();
    let out = e.run_effect(gain_index(), p, 0..1000).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.undo_label(), Some("Gain"));
    assert!(mono(&e).iter().all(|v| (v - 0.25).abs() < 1e-4), "-6 dB halves the level");
}

#[test]
fn out_of_range_settings_are_refused_and_change_nothing() {
    let tmp = TempDir::new("refuse");
    let mut e = editor(&tmp.0, &[vec![0.5; 100]]);
    let before = e.history.undo_label().map(str::to_string);
    assert!(e.run_effect(gain_index(), vec![500.0], 0..100).is_err());
    assert!(e.run_effect(gain_index(), vec![], 0..100).is_err());
    assert_eq!(e.history.undo_label().map(str::to_string), before);
    assert!(mono(&e).iter().all(|v| *v == 0.5));
}

#[test]
fn the_action_row_runs_with_the_defaults() {
    let tmp = TempDir::new("row");
    let mut e = editor(&tmp.0, &[vec![0.5; 100]]);
    let g = effects::find("gain").unwrap();
    // Whatever Gain's default is, `ACTION effect.gain` applies exactly that.
    let db = effects::Params::defaults(g)[0];
    e.run(by_name("effect.gain").unwrap().id, 0..100).unwrap();
    let want = 0.5 * 10f32.powf(db as f32 / 20.0);
    assert!(mono(&e).iter().all(|v| (v - want).abs() < 1e-5));
}

#[test]
fn repeat_last_command_repeats_the_effect_with_the_same_settings() {
    let tmp = TempDir::new("repeat");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    e.run_effect(gain_index(), vec![-6.0206], 0..500).unwrap();
    assert_eq!(e.last, Some(Repeat::Effect(gain_index(), vec![-6.0206])));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Gain");
    // Repeat on another range: same settings.
    let out = e.run(ActionId::RepeatLast, 500..1000).unwrap();
    assert!(out.doc_changed);
    assert!(mono(&e).iter().all(|v| (v - 0.25).abs() < 1e-4), "both halves at -6 dB");
    // A second repeat over the first half: -12 dB there.
    e.run(ActionId::RepeatLast, 0..500).unwrap();
    let m = mono(&e);
    assert!((m[100] - 0.125).abs() < 1e-4 && (m[700] - 0.25).abs() < 1e-4);
}

#[test]
fn a_later_command_replaces_the_remembered_effect() {
    let tmp = TempDir::new("replace");
    let mut e = editor(&tmp.0, &[vec![0.5; 1000]]);
    e.run_effect(gain_index(), vec![-6.0], 0..500).unwrap();
    e.run(ActionId::Silence, 0..100).unwrap();
    assert_eq!(e.last, Some(Repeat::Action(ActionId::Silence)));
    // A no-parameter effect is remembered too.
    e.run(by_name("effect.invert").unwrap().id, 0..100).unwrap();
    assert!(matches!(e.last, Some(Repeat::Effect(_, ref p)) if p.is_empty()));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Invert");
}
