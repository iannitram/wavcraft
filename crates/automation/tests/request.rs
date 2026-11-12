//! The request spelling both backends share.

use std::path::PathBuf;

use wavcraft_automation::{Request, SaveArgs};
use wavcraft_core::save::{AiffKind, SampleType};

#[test]
fn every_request_round_trips_through_its_line() {
    let reqs = [
        Request::Open(PathBuf::from("take one.wav")),
        Request::Select { start: 0, end: 48_000 },
        Request::Action("edit.delete".to_string()),
        Request::Effect { name: "invert".to_string(), args: "gain=3".to_string() },
        Request::Effect { name: "invert".to_string(), args: String::new() },
        Request::Save { path: Some(PathBuf::from("out.wav")), args: SaveArgs::default() },
        Request::Save { path: None, args: SaveArgs::default() },
        Request::Save {
            path: Some(PathBuf::from("my take.aiff")),
            args: SaveArgs { ty: Some(SampleType::Float32), aiff: Some(AiffKind::AifcSowt), quality: Some(2.5), meta: Some(false) },
        },
        Request::State,
    ];
    for req in reqs {
        assert_eq!(Request::parse(&req.line()), Ok(req.clone()), "{:?}", req.line());
    }
}

#[test]
fn an_effect_may_be_named_with_its_action_prefix() {
    assert_eq!(Request::parse("EFFECT effect.invert"), Ok(Request::Effect { name: "invert".to_string(), args: String::new() }));
    assert_eq!(Request::parse("EFFECT invert  gain=3 "), Ok(Request::Effect { name: "invert".to_string(), args: "gain=3".to_string() }));
}

#[test]
fn bad_lines_are_refused_with_the_usage() {
    assert!(Request::parse("SELECT 1").is_err());
    assert!(Request::parse("SELECT 1 2 3").is_err());
    assert!(Request::parse("SELECT a b").is_err());
    assert!(Request::parse("OPEN").is_err());
    assert!(Request::parse("ACTION").is_err());
    assert!(Request::parse("STATE now").is_err());
    assert!(Request::parse("state").is_err());
}

#[test]
fn save_options_are_not_part_of_the_path() {
    let save = |line: &str| Request::parse(line);
    let with = |path: Option<&str>, args: SaveArgs| Ok(Request::Save { path: path.map(PathBuf::from), args });

    let cases = [
        ("SAVE my take.wav bits=16", with(Some("my take.wav"), SaveArgs { ty: Some(SampleType::Int16), ..SaveArgs::default() })),
        ("SAVE a.ogg quality=7", with(Some("a.ogg"), SaveArgs { quality: Some(7.0), ..SaveArgs::default() })),
        ("SAVE", with(None, SaveArgs::default())),
        ("SAVE bits=24", with(None, SaveArgs { ty: Some(SampleType::Int24), ..SaveArgs::default() })),
    ];
    for (line, want) in cases {
        let got = save(line);
        assert_eq!(got, want, "{line:?}");
        let req = got.expect("a save request");
        assert_eq!(Request::parse(&req.line()), Ok(req.clone()), "{line:?} round-trips as {:?}", req.line());
    }

    let err = save("SAVE a.wav float=2").expect_err("float=2 is refused");
    assert!(err.contains("float=<0|1>"), "{err}");
    let err = save("SAVE a.ogg quality=11").expect_err("quality=11 is refused");
    assert!(err.contains("quality=<-1..10>"), "{err}");
}
