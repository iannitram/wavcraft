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

/// `source_samples` with `f` applied to every sample whose frame is in `range`: what the document
/// must hold after an edit that touches exactly those frames.
fn source_with(range: std::ops::Range<usize>, f: impl Fn(i32) -> i32) -> Vec<Vec<i32>> {
    source_samples().into_iter().map(|c| c.into_iter().enumerate().map(|(p, v)| if range.contains(&p) { f(v) } else { v }).collect()).collect()
}

/// Save what `b` has open as `name` in `dir`, open that file in a fresh backend and read it back.
fn saved(b: &mut Headless, dir: &Path, name: &str) -> Vec<Vec<i32>> {
    let out = dir.join(name);
    assert_eq!(b.answer_line(&format!("SAVE {}", out.display())), "OK");
    let mut reread = Headless::new(dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", out.display())), "OK");
    current(&reread)
}

#[test]
fn effect_invert_on_a_selection_changes_only_the_selection() {
    let dir = scratch("effect-invert");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SELECT 4000 4500"), "OK selection=4000-4500");
    assert_eq!(b.answer_line("EFFECT invert"), "OK selection=4000-4500 changed=1");
    assert_eq!(current(&b), source_with(4_000..4_500, |v| -v), "only frames 4000..4500 are negated, both channels");
    assert_eq!(saved(&mut b, &dir, "out.wav"), source_with(4_000..4_500, |v| -v), "the saved file holds the same samples");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn effect_takes_the_effect_prefix_and_its_settings() {
    let dir = scratch("effect-gain");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SELECT 2000 3000"), "OK selection=2000-3000");
    assert_eq!(b.answer_line("EFFECT effect.gain gain_db=-6"), "OK selection=2000-3000 changed=1");

    // The gain is computed here in f64, apart from the effect's own f32 maths: the two agree to
    // within one 16-bit step, and the samples outside the selection are exact.
    let k = 10f64.powf(-6.0 / 20.0);
    let want = source_with(2_000..3_000, |v| (f64::from(v) * k).round() as i32);
    let got = saved(&mut b, &dir, "out.wav");
    for (c, (g, w)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g.len(), FRAMES, "channel {c} keeps its length");
        for (p, (g, w)) in g.iter().zip(w).enumerate() {
            let slack = i32::from((2_000..3_000).contains(&p));
            assert!((g - w).abs() <= slack, "channel {c} frame {p}: got {g}, want {w} (within {slack})");
        }
    }
    assert_ne!(got, source_samples(), "the gain changed the selection");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn effect_with_no_selection_runs_on_the_whole_file() {
    let dir = scratch("effect-whole");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    // Nothing selected right after OPEN.
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert!(b.answer_line("STATE").contains("selection=0-0"), "OPEN leaves nothing selected");
    assert!(b.answer_line("EFFECT invert").ends_with("changed=1"));
    assert_eq!(current(&b), source_with(0..FRAMES, |v| -v), "every frame of both channels is negated");
    assert_eq!(saved(&mut b, &dir, "out.wav"), source_with(0..FRAMES, |v| -v), "the saved file holds the same samples");

    // A collapsed selection (start == end) is also "none": it does not mean "zero frames".
    assert_eq!(b.answer_line("SELECT 7000 7000"), "OK selection=7000-7000");
    assert!(b.answer_line("EFFECT invert").ends_with("changed=1"));
    assert_eq!(current(&b), source_samples(), "a second whole-file invert gives the source back");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn action_effect_with_no_selection_runs_on_the_whole_file_but_other_actions_do_not() {
    let dir = scratch("action-whole");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    // Delete is not an effect: with nothing selected it has nothing to delete, and does not widen.
    let reply = b.answer_line("ACTION edit.delete");
    assert!(reply.starts_with("OK ") && reply.ends_with("changed=0"), "unexpected reply {reply:?}");
    assert_eq!(current(&b), source_samples(), "delete with no selection leaves the document as it was");

    // An effect action is: it widens to the whole file.
    assert!(b.answer_line("ACTION effect.invert").ends_with("changed=1"));
    assert_eq!(current(&b), source_with(0..FRAMES, |v| -v), "every frame of both channels is negated");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn effect_reverse_runs_the_one_shot() {
    let dir = scratch("effect-reverse");
    let src = dir.join("src.wav");
    write_wav(&src, &[vec![1, 2, 3, 4]]);
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);

    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SELECT 0 4"), "OK selection=0-4");
    assert_eq!(b.answer_line("EFFECT reverse"), "OK selection=0-4 changed=1");
    assert_eq!(current(&b), vec![vec![4, 3, 2, 1]], "the four frames run backwards");
    assert_eq!(b.answer_line("EFFECT reverse x=1"), "ERR reverse takes no settings here: set them with PARAM");
    assert_eq!(current(&b), vec![vec![4, 3, 2, 1]], "a refused one-shot changes nothing");
    let reply = b.answer_line("EFFECT nope");
    assert!(reply.starts_with("ERR ") && reply.contains("no effect named"), "{reply}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `write_wav`'s file with one more chunk (`note`, 12 bytes) after `data`, the kind a save
/// copies when "Include markers and other metadata" is on.
fn write_wav_with_a_chunk(path: &Path, channels: &[Vec<i32>]) {
    write_wav(path, channels);
    let mut bytes = std::fs::read(path).expect("read the test WAV back");
    bytes.extend_from_slice(b"note");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(b"twelve bytes");
    let riff = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(path, bytes).expect("write the test WAV");
}

/// The sample format a fresh backend reads `path` back as.
fn reopened_format(dir: &Path, path: &Path) -> wavcraft_core::SampleFormat {
    let mut reread = Headless::new(dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", path.display())), "OK");
    reread.document().expect("a document is open").source().sample_format
}

#[test]
fn save_bits_24_writes_a_24_bit_file() {
    let dir = scratch("save-bits");
    let src = dir.join("src.wav");
    let out = dir.join("out.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");

    assert_eq!(b.answer_line(&format!("SAVE {} bits=24", out.display())), "OK");
    assert_eq!(reopened_format(&dir, &out), wavcraft_core::SampleFormat::Int24, "bits=24 gives a 24-bit file");
    assert!(!dir.join("out.wav bits=24").exists(), "the option is not part of the file name");
    assert_eq!(current(&b), source_samples());

    assert_eq!(b.answer_line(&format!("SAVE {}", out.display())), "OK");
    assert_eq!(reopened_format(&dir, &out), wavcraft_core::SampleFormat::Int16, "without the option the source's 16 bits are kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_flac_float_is_refused() {
    let dir = scratch("save-flac-float");
    let src = dir.join("src.wav");
    let out = dir.join("out.flac");
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    let reply = b.answer_line(&format!("SAVE {} float=1", out.display()));
    assert!(reply.starts_with("ERR ") && reply.contains("FLAC cannot hold"), "{reply}");
    assert!(!out.exists(), "nothing is written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_aiff_sowt() {
    let dir = scratch("save-sowt");
    let src = dir.join("src.wav");
    let out = dir.join("o.aiff");
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line(&format!("SAVE {} aiff=sowt", out.display())), "OK");
    let bytes = std::fs::read(&out).expect("the AIFF file");
    assert!(bytes.windows(4).any(|w| w == b"sowt"), "an AIFF-C sowt file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_quality_changes_the_ogg() {
    let dir = scratch("save-quality");
    let src = dir.join("src.wav");
    let (low, high) = (dir.join("q0.ogg"), dir.join("q8.ogg"));
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line(&format!("SAVE {} quality=0", low.display())), "OK");
    assert_eq!(b.answer_line(&format!("SAVE {} quality=8", high.display())), "OK");
    let size = |p: &Path| std::fs::metadata(p).expect("an Ogg file").len();
    assert!(size(&high) > size(&low), "quality=8 ({} bytes) is larger than quality=0 ({} bytes)", size(&high), size(&low));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_meta_0_writes_only_audio() {
    let dir = scratch("save-meta");
    let src = dir.join("src.wav");
    let (with, without) = (dir.join("with.wav"), dir.join("without.wav"));
    write_wav_with_a_chunk(&src, &source_samples());
    let mut b = Headless::new(&dir);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    let data = (FRAMES * 2 * 2) as u64;

    assert_eq!(b.answer_line(&format!("SAVE {}", with.display())), "OK");
    assert!(std::fs::metadata(&with).expect("saved").len() > 44 + data, "metadata is on by default: the chunk is kept");
    assert_eq!(b.answer_line(&format!("SAVE {} meta=0", without.display())), "OK");
    assert_eq!(std::fs::metadata(&without).expect("saved").len(), 44 + data, "only the header and the samples");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_with_no_path_overwrites_the_source() {
    let dir = scratch("save-in-place");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::with_crossfades(&dir, Crossfades::NONE);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line("SELECT 100 200"), "OK selection=100-200");
    assert!(b.answer_line("EFFECT invert").ends_with("changed=1"));
    assert_eq!(b.answer_line("SAVE"), "OK");

    let mut reread = Headless::new(&dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(current(&reread), source_with(100..200, |v| -v), "the opened file now holds the edit");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_picks_the_window_s_format_for_a_name_without_one_or_a_container() {
    let dir = scratch("save-containers");
    let src = dir.join("src.wav");
    write_wav(&src, &source_samples());
    let mut b = Headless::new(&dir);
    assert_eq!(b.answer_line(&format!("OPEN {}", src.display())), "OK");
    assert_eq!(b.answer_line(&format!("SAVE {}", dir.join("plain").display())), "OK");
    assert!(dir.join("plain.wav").exists(), "a name without an extension is saved as a WAV");
    assert_eq!(b.answer_line(&format!("SAVE {}", dir.join("o.w64").display())), "OK");
    let mut reread = Headless::new(&dir);
    assert_eq!(reread.answer_line(&format!("OPEN {}", dir.join("o.w64").display())), "OK");
    assert_eq!(current(&reread), source_samples(), "the Wave64 file holds the samples");
    let _ = std::fs::remove_dir_all(&dir);
}
