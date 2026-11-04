//! The request spelling both backends share.

use std::path::PathBuf;

use wavcraft_automation::Request;

#[test]
fn every_request_round_trips_through_its_line() {
    let reqs = [
        Request::Open(PathBuf::from("take one.wav")),
        Request::Select { start: 0, end: 48_000 },
        Request::Action("edit.delete".to_string()),
        Request::Effect { name: "invert".to_string(), args: "gain=3".to_string() },
        Request::Effect { name: "invert".to_string(), args: String::new() },
        Request::Save(PathBuf::from("out.wav")),
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
    assert!(Request::parse("SAVE").is_err());
    assert!(Request::parse("STATE now").is_err());
    assert!(Request::parse("state").is_err());
}
