//! f79: Extract Channels to Mono Files and Export Audio of Range Markers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavcraft_core::Markers;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::export::{DEFAULT_CHANNEL_PATTERN, DEFAULT_RANGE_PATTERN, ExportError, ExportOptions, export_ranges, extract_channels};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, temp_files};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-export-test-{}-{name}", std::process::id()));
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

/// Distinct samples per channel, longer than two blocks.
fn data(channels: usize) -> Vec<Vec<f32>> {
    let n = 2 * BLOCK_FRAMES + 12_345;
    (0..channels).map(|c| (0..n).map(|i| (((i * (c + 3)) % 2000) as f32 - 1000.0) / 1024.0 + c as f32 / 4096.0).collect()).collect()
}

fn doc_from(dir: &Path, data: &[Vec<f32>], ranges: &[(&str, u64, u64)]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut m = Markers::new();
    for (name, a, b) in ranges {
        m = m.with_added(*a..*b, Some(name)).unwrap();
    }
    doc.with_markers(Arc::new(m))
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

fn channel_of(doc: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![f32::NAN; doc.len_frames() as usize];
    assert_eq!(doc.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn opts(pattern: &str) -> ExportOptions {
    ExportOptions::new(SampleType::Float32, pattern)
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn extracted_channels_are_sample_identical_to_the_source_channels() {
    let t = TempDir::new("extract");
    let src = data(2);
    let doc = doc_from(&t.0.join("s"), &src, &[]);
    let out = t.0.join("out");
    let files = extract_channels(&doc, &out, "song", &opts(DEFAULT_CHANNEL_PATTERN), &mut |_| {}).unwrap();
    assert_eq!(names(&out), ["song_L.wav", "song_R.wav"]);
    assert_eq!(files.len(), 2);
    for (ch, f) in files.iter().enumerate() {
        let d = reopen(f, &t.0.join("r"));
        assert_eq!(d.channel_count(), 1, "mono");
        assert_eq!(d.sample_rate(), RATE);
        assert_eq!(bits(&channel_of(&d, 0)), bits(&src[ch]), "channel {ch}");
    }
    // The original is untouched.
    assert_eq!(bits(&channel_of(&doc, 0)), bits(&src[0]));
    assert_eq!(doc.channel_count(), 2);
}

#[test]
fn six_channels_get_numbered_names_and_a_mono_file_is_refused() {
    let t = TempDir::new("six");
    let doc = doc_from(&t.0.join("s"), &data(6), &[]);
    let out = t.0.join("out");
    extract_channels(&doc, &out, "surround", &opts("{base}-{channel}"), &mut |_| {}).unwrap();
    assert_eq!(names(&out), ["surround-C1.wav", "surround-C2.wav", "surround-C3.wav", "surround-C4.wav", "surround-C5.wav", "surround-C6.wav"]);
    let mono = doc_from(&t.0.join("m"), &data(1), &[]);
    assert!(matches!(extract_channels(&mono, &t.0.join("o2"), "m", &opts("{name}_{channel}"), &mut |_| {}), Err(ExportError::Nothing(_))));
}

#[test]
fn exported_ranges_are_sample_identical_to_the_source_ranges_and_overlap_is_allowed() {
    let t = TempDir::new("ranges");
    let src = data(2);
    let ranges = [("Verse", 1000u64, 70_000u64), ("Chorus", 60_000, 130_000), ("Tail", 130_000, 140_000)];
    let doc = doc_from(&t.0.join("s"), &src, &ranges);
    let out = t.0.join("out");
    let files = export_ranges(&doc, &out, None, "song", &opts(DEFAULT_RANGE_PATTERN), &mut |_| {}).unwrap();
    assert_eq!(names(&out), ["Chorus.wav", "Tail.wav", "Verse.wav"]);
    assert_eq!(files.len(), 3);
    for ((_, a, b), f) in ranges.iter().zip(&files) {
        let d = reopen(f, &t.0.join("r"));
        assert_eq!(d.channel_count(), 2);
        assert_eq!(d.len_frames(), b - a);
        for (ch, source) in src.iter().enumerate() {
            assert_eq!(bits(&channel_of(&d, ch)), bits(&source[*a as usize..*b as usize]), "{f:?} channel {ch}");
        }
    }
    assert!(temp_files(&out.join("Verse.wav")).is_empty());
}

#[test]
fn names_are_sanitised_numbered_and_never_replace_a_file() {
    let t = TempDir::new("names");
    let doc = doc_from(&t.0.join("s"), &data(1), &[("../evil/name", 0, 100), ("a:b", 200, 300), ("a:b", 400, 500), ("CON", 600, 700)]);
    let out = t.0.join("out");
    export_ranges(&doc, &out, None, "song", &opts("{index}_{name}"), &mut |_| {}).unwrap();
    assert_eq!(names(&out), ["001__evil_name.wav", "002_a_b.wav", "003_a_b.wav", "004__CON.wav"]);
    // Same names again: both sets are kept, and a pattern without an index still gives distinct names.
    std::fs::write(out.join("keep.wav"), b"owner's file").unwrap();
    export_ranges(&doc, &out, None, "song", &opts("keep"), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(out.join("keep.wav")).unwrap(), b"owner's file");
    assert!(out.join("keep_2.wav").exists() && out.join("keep_3.wav").exists() && out.join("keep_4.wav").exists() && out.join("keep_5.wav").exists());
    // Nothing escaped the folder.
    assert_eq!(names(&t.0).iter().filter(|n| n.ends_with(".wav")).count(), 0);
}

#[test]
fn only_the_chosen_ranges_are_exported_and_points_are_skipped() {
    let t = TempDir::new("only");
    let mut doc = doc_from(&t.0.join("s"), &data(1), &[("A", 0, 100), ("B", 200, 300)]);
    let m = doc.markers().with_added(500..500, Some("P")).unwrap();
    doc = doc.with_markers(Arc::new(m));
    let ids: Vec<u64> = doc.markers().iter().filter(|m| m.name == "B").map(|m| m.id).collect();
    let out = t.0.join("out");
    export_ranges(&doc, &out, Some(&ids), "s", &opts("{name}"), &mut |_| {}).unwrap();
    assert_eq!(names(&out), ["B.wav"]);
    export_ranges(&doc, &t.0.join("all"), None, "s", &opts("{name}"), &mut |_| {}).unwrap();
    assert_eq!(names(&t.0.join("all")), ["A.wav", "B.wav"], "the point marker has no audio");
    let none = doc_from(&t.0.join("s2"), &data(1), &[]);
    assert!(matches!(export_ranges(&none, &t.0.join("o3"), None, "s", &opts("{name}"), &mut |_| {}), Err(ExportError::Nothing(_))));
}

#[test]
fn a_failed_write_leaves_no_temporary_file_and_nothing_at_the_name() {
    let t = TempDir::new("fail");
    let doc = doc_from(&t.0.join("s"), &data(2), &[("Long", 0, 100_000)]);
    let out = t.0.join("out");
    let mut o = opts("{name}");
    o.fail_after = Some(BLOCK_FRAMES as u64);
    let err = export_ranges(&doc, &out, None, "s", &o, &mut |_| {}).unwrap_err();
    assert!(matches!(err, ExportError::Save { .. }), "{err}");
    assert_eq!(names(&out), Vec::<String>::new(), "no target and no temp file");
    let err = extract_channels(
        &doc,
        &out,
        "s",
        &{
            let mut o = opts("{name}_{channel}");
            o.fail_after = Some(BLOCK_FRAMES as u64);
            o
        },
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, ExportError::Save { .. }));
    assert_eq!(names(&out), Vec::<String>::new());
}
