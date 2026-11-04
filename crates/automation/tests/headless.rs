//! The headless backend against real files: a 16-bit WAV written here with known samples, the
//! edits run through the control channel's verbs, and the saved file read back and compared
//! sample for sample with what those edits must produce.

use std::path::{Path, PathBuf};

use wavcraft_automation::Backend;
use wavcraft_automation::headless::Headless;
use wavcraft_core::DocState;
use wavcraft_core::doc::edit::Crossfades;

const RATE: u32 = 48_000;
const FRAMES: usize = 20_000;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-automation-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// Two channels of distinct, deterministic 16-bit samples (never -32768, so inverting is exact).
fn source_samples() -> Vec<Vec<i32>> {
    let l = (0..FRAMES).map(|i| ((i * 7919) % 20_001) as i32 - 10_000).collect();
    let r = (0..FRAMES).map(|i| ((i * 104_729) % 20_011) as i32 - 10_005).collect();
    vec![l, r]
}

/// A plain PCM WAV, 16-bit, interleaved.
fn write_wav(path: &Path, channels: &[Vec<i32>]) {
    let ch = channels.len() as u16;
    let frames = channels[0].len();
    let data_len = (frames * usize::from(ch) * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&ch.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * u32::from(ch) * 2).to_le_bytes());
    out.extend_from_slice(&(ch * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for f in 0..frames {
        for c in channels {
            out.extend_from_slice(&(c[f] as i16).to_le_bytes());
        }
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// Every channel of an open document, as 16-bit sample values.
fn read_back(doc: &DocState) -> Vec<Vec<i32>> {
    (0..doc.channel_count())
        .map(|ch| {
            let mut buf = vec![0f32; doc.len_frames() as usize];
            let n = doc.read(ch, 0, &mut buf).expect("read a channel");
            assert_eq!(n, buf.len(), "a whole channel reads back");
            buf.iter().map(|v| (v * 32_768.0).round() as i32).collect()
        })
        .collect()
}

/// The document a `Headless` has open, read back.
fn current(backend: &Headless) -> Vec<Vec<i32>> {
    read_back(backend.document().expect("a document is open"))
}

#[test]
fn the_source_reads_back_exactly() {
    let dir = scratch("source");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(current(&b), source_samples(), "the reader gives back what was written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_then_invert_then_save_matches_the_expected_samples() {
    let dir = scratch("exact");
    let src = dir.join("src.wav");
    let out = dir.join("out.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SELECT 10000 10100"), "OK selection=10000-10100");
    assert!(b.answer_line("ACTION edit.delete").ends_with("changed=1"), "delete changes the document");
    assert_eq!(b.answer_line("SELECT 10000 10050"), "OK selection=10000-10050");
    assert!(b.answer_line("ACTION effect.invert").ends_with("changed=1"), "invert changes the selection");
    assert_eq!(b.answer_line(&format!("SAVE {}", out.display())), "OK");

    // The expected file: frames 10000..10100 removed, then 10000..10050 of what is left negated.
    let src_samples = source_samples();
    let expected: Vec<Vec<i32>> = src_samples
        .iter()
        .map(|c| {
            (0..FRAMES - 100)
                .map(|p| {
                    let v = if p < 10_000 { c[p] } else { c[p + 100] };
                    if (10_000..10_050).contains(&p) { -v } else { v }
                })
                .collect()
        })
        .collect();

    let mut reread = Headless::new(&dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", out.display())), "OK");
    assert_eq!(current(&reread), expected, "saved file equals the expected samples, both channels");
    assert!(reread.answer_line("STATE").contains("frames=19900 channels=2 rate=48000"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_the_window_crossfades_only_the_edit_joins_differ() {
    let dir = scratch("joins");
    let src = dir.join("src.wav");
    let out = dir.join("out.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    b.answer_line("SELECT 10000 10100");
    assert!(b.answer_line("ACTION edit.delete").ends_with("changed=1"));
    b.answer_line("SELECT 10000 10050");
    assert!(b.answer_line("ACTION effect.invert").ends_with("changed=1"));
    assert_eq!(b.answer_line(&format!("SAVE {}", out.display())), "OK");

    // Each edit boundary is smoothed over the window's crossfade (5 ms, 240 frames at 48 kHz, the
    // longest). Outside those windows the samples are exact; inside them they are not checked here.
    let src_samples = source_samples();
    let zone = |p: usize| (9_760..10_290).contains(&p);
    let mut reread = Headless::new(&dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", out.display())), "OK");
    let got = current(&reread);
    assert_eq!(got[0].len(), FRAMES - 100);
    for (c, samples) in src_samples.iter().enumerate() {
        for p in (0..FRAMES - 100).filter(|p| !zone(*p)) {
            let v = if p < 10_000 { samples[p] } else { samples[p + 100] };
            let want = if (10_000..10_050).contains(&p) { -v } else { v };
            assert_eq!(got[c][p], want, "channel {c} frame {p} outside the edit joins");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_requests_answer_err_and_change_nothing() {
    let dir = scratch("errors");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);

    assert_eq!(b.answer_line("STATE"), "ERR no file is open");
    assert_eq!(b.answer_line("SELECT 1"), "ERR SELECT <start> <end>, in frames");
    assert_eq!(b.answer_line("ACTION no.such"), "ERR no action named \"no.such\"");
    assert_eq!(b.answer_line("EFFECT no.such"), "ERR no effect named \"no.such\"");
    assert_eq!(b.answer_line("FROBNICATE"), "ERR no request named \"FROBNICATE\"");
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SAVE out.unknown"), "ERR no save format for out.unknown");
    assert_eq!(current(&b), source_samples(), "refused requests leave the document as it was");
    let _ = std::fs::remove_dir_all(&dir);
}
