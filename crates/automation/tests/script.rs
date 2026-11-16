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

use std::path::{Path, PathBuf};

use wavcraft_automation::Backend;
use wavcraft_automation::headless::Headless;
use wavcraft_automation::script::run;
use wavcraft_core::doc::edit::Crossfades;

const RATE: u32 = 48_000;
const FRAMES: u32 = 20_000;
/// One 16-bit step: the save rounds each sample to the nearest 1/32768.
const LSB: f32 = 1.0 / 32768.0;

/// A 16-bit, 48000 Hz, stereo WAV, `FRAMES` long. Channel 0 at frame i is `100 * ((i % 100) + 1)`
/// (100 to 10000, never 0); channel 1 is its negative.
fn write_wav(path: &Path) {
    let data_len = FRAMES * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..FRAMES {
        let v = (100 * ((i % 100) + 1)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&(-v).to_le_bytes());
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// A headless backend with `tone.wav` open, in a fresh scratch folder named after the test.
fn open_tone(name: &str) -> Headless {
    let dir: PathBuf = std::env::temp_dir().join(format!("wavcraft-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    let wav = dir.join("tone.wav");
    write_wav(&wav);
    let mut head = Headless::with_crossfades(dir.join("sessions"), Crossfades::NONE);
    head.execute(&wavcraft_automation::Request::Open(wav)).expect("open the test WAV");
    head
}

/// Frames `start..start + n` of channel `ch` of the open document.
fn read(head: &Headless, ch: usize, start: u64, n: usize) -> Vec<f32> {
    let doc = head.document().expect("a document is open");
    let mut out = vec![0.0f32; n];
    let got = doc.read(ch, start, &mut out).expect("read the document");
    assert_eq!(got, n, "the document ends before frame {}", start + n as u64);
    out
}

/// The source value of channel `ch` at frame `i`, as the file holds it.
fn source(ch: usize, i: u64) -> f32 {
    let v = (100 * ((i % 100) + 1)) as f32 / 32768.0;
    if ch == 0 { v } else { -v }
}

#[test]
fn five_steps_give_the_expected_samples() {
    let mut head = open_tone("five");
    let text = "SELECT 1000 2000\nACTION edit.delete\nSELECT 0 500\nEFFECT invert\nEFFECT gain gain_db=-6.0206";
    assert_eq!(run(&mut head, text), Ok(()));
    assert_eq!(head.document().unwrap().len_frames(), 19_000);
    for ch in 0..2 {
        let head_part = read(&head, ch, 0, 500);
        for (i, got) in head_part.iter().enumerate() {
            let want = -0.5 * source(ch, i as u64);
            assert!((got - want).abs() <= LSB, "ch {ch} frame {i}: {got} vs {want}");
        }
        let tail = read(&head, ch, 500, 18_500);
        for (j, got) in tail.iter().enumerate() {
            let i = 500 + j as u64;
            let src = if i < 1000 { i } else { i + 1000 };
            let want = source(ch, src);
            assert!((got - want).abs() <= LSB, "ch {ch} frame {i}: {got} vs {want}");
        }
    }
}

#[test]
fn a_failing_line_stops_the_run() {
    let mut head = open_tone("fail");
    let text = "SELECT 0 500\nEFFECT invert\nACTION edit.nope\nEFFECT invert";
    assert_eq!(run(&mut head, text), Err("line 3: no action named \"edit.nope\"".to_string()));
    assert_eq!(head.document().unwrap().len_frames(), 20_000);
    let first = read(&head, 0, 0, 1)[0];
    assert!((first - -(100.0 / 32768.0)).abs() <= LSB, "frame 0 is {first}");
}

#[test]
fn a_parse_error_runs_nothing() {
    let mut head = open_tone("parse");
    let text = "SELECT 0 500\nEFFECT invert\nSELECT 1 2 3";
    assert_eq!(run(&mut head, text), Err("line 3: SELECT <start> <end>, in frames".to_string()));
    let first = read(&head, 0, 0, 1)[0];
    assert!((first - (100.0 / 32768.0)).abs() <= LSB, "frame 0 is {first}");
}
