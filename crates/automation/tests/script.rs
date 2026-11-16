//! script::parse.

use wavcraft_automation::{Request, script::parse};

#[test]
fn comments_and_blank_lines_are_skipped() {
    let text = "SELECT 0 100\n# trim\nACTION edit.delete\nEFFECT invert\n\n# done\nSTATE";
    assert_eq!(
        parse(text).unwrap(),
        vec![
            (1, Request::Select { start: 0, end: 100 }),
            (3, Request::Action("edit.delete".to_string())),
            (4, Request::Effect { name: "invert".to_string(), args: String::new() }),
            (7, Request::State),
        ]
    );
}

#[test]
fn a_bad_line_is_named() {
    let text = "SELECT 0 10\nACTION edit.delete\nSELECT 1 2 3\nSTATE";
    let err = parse(text).unwrap_err();
    assert_eq!(err, "line 3: SELECT <start> <end>, in frames".to_string());
    assert!(err.starts_with("line 3:"));
}

#[test]
fn windows_line_endings_parse() {
    let text = "SELECT 0 10\r\n# note\r\nSTATE\r\n";
    assert_eq!(parse(text).unwrap(), vec![(1, Request::Select { start: 0, end: 10 }), (3, Request::State)]);
}

#[test]
fn save_is_not_a_script_line() {
    let text = "SELECT 0 10\nSAVE out.wav\n";
    assert_eq!(parse(text), Err("line 2: SAVE is not allowed in a script".to_string()));
    assert_eq!(parse("SAVE"), Err("line 1: SAVE is not allowed in a script".to_string()));
}

#[test]
fn open_is_not_a_script_line() {
    let text = "STATE\nOPEN a.wav";
    assert_eq!(parse(text), Err("line 2: OPEN is not allowed in a script".to_string()));
}

#[test]
fn an_empty_script_has_no_requests() {
    assert_eq!(parse(""), Ok(vec![]));
    assert_eq!(parse("# only\n\n"), Ok(vec![]));
}
