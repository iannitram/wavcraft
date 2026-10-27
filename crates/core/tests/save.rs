//! Task 06, first part: WAV save. Numbers refer to task 06's acceptance tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{
    ExtraChunk, LEFTOVER_AGE, SampleType, SaveError, SaveFormat, SaveOptions, remove_leftover_temps, save, save_failing_after, save_wav,
    save_wav_failing_after, save_wav_rf64_over, save_wav_with, temp_files, temp_path,
};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-save-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
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

fn bits(v: &[Vec<f32>]) -> Vec<Vec<u32>> {
    v.iter().map(|c| c.iter().map(|x| x.to_bits()).collect()).collect()
}

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

/// Values exactly representable at `bits` (full scale ±1), across the whole range, stereo,
/// longer than two blocks so the writer crosses block edges.
fn exact(bits: u32, frames: usize) -> Vec<Vec<f32>> {
    let scale = (1u64 << (bits - 1)) as f64;
    (0..2)
        .map(|c| (0..frames).map(|k| (((k * 7_919 + c * 104_729) as u64 % (2 * scale as u64)) as f64 - scale) as f32 / scale as f32).collect())
        .collect()
}

#[test]
fn t1_round_trip_is_bit_identical_for_each_sample_type() {
    let t = TempDir::new("round");
    let frames = 2 * BLOCK_FRAMES + 123;
    for (ty, data) in [
        (SampleType::Int16, exact(16, frames)),
        (SampleType::Int24, exact(24, frames)),
        (SampleType::Float32, vec![(0..frames).map(|k| (k as f32 * 0.37).sin() * 1.7).collect(), (0..frames).map(|k| -(k as f32) * 1e-6).collect()]),
    ] {
        let doc = doc_from(&t.0.join(format!("{ty:?}-store")), &data);
        let path = t.0.join(format!("{ty:?}.wav"));
        save_wav(&doc, &path, ty, &mut |_| {}).unwrap();
        let back = reopen(&path, &t.0.join("sessions"));
        assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (RATE, 2, frames as u64), "{ty:?}");
        assert_eq!(bits(&content(&back)), bits(&data), "{ty:?} bit for bit");
        assert_eq!(SampleType::for_source(back.source()), ty, "{ty:?}: a save keeps the type it was opened with");
    }
}

#[test]
fn t2_t3_edits_then_save_equal_the_model() {
    let t = TempDir::new("edits");
    let data = exact(16, 3 * BLOCK_FRAMES);
    let mut e = Editor::new(History::new(doc_from(&t.0.join("store"), &data), u64::MAX));
    e.crossfades = Crossfades::NONE;
    let mut model = data.clone();
    // Delete 1 s, across a block edge.
    let a = BLOCK_FRAMES - 1000;
    e.run(ActionId::Delete, a as u64..(a + RATE as usize) as u64).unwrap();
    model.iter_mut().for_each(|c| {
        c.drain(a..a + RATE as usize);
    });
    // Silence across the next edge, then cut and paste a piece back at the start.
    let s = BLOCK_FRAMES + 500;
    e.run(ActionId::Silence, (s - 700) as u64..(s + 700) as u64).unwrap();
    model.iter_mut().for_each(|c| c[s - 700..s + 700].fill(0.0));
    e.run(ActionId::Cut, 10_000..12_000).unwrap();
    let piece: Vec<Vec<f32>> = model.iter_mut().map(|c| c.drain(10_000..12_000).collect()).collect();
    e.run(ActionId::Paste, 0..0).unwrap();
    model.iter_mut().zip(piece).for_each(|(c, p)| {
        c.splice(0..0, p);
    });
    let path = t.0.join("edited.wav");
    save_wav(e.history.current(), &path, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(bits(&content(&reopen(&path, &t.0.join("sessions")))), bits(&model));
}

#[test]
fn t4_sixteen_to_twentyfour_to_sixteen_is_bit_identical() {
    let t = TempDir::new("depths");
    let data = exact(16, BLOCK_FRAMES + 77);
    let doc = doc_from(&t.0.join("store"), &data);
    let (p24, p16) = (t.0.join("a24.wav"), t.0.join("b16.wav"));
    save_wav(&doc, &p24, SampleType::Int24, &mut |_| {}).unwrap();
    let mid = reopen(&p24, &t.0.join("s1"));
    save_wav(&mid, &p16, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(bits(&content(&reopen(&p16, &t.0.join("s2")))), bits(&data));
}

#[test]
fn t5_a_save_that_fails_halfway_leaves_the_target_whole() {
    let t = TempDir::new("fail");
    let doc = doc_from(&t.0.join("store"), &exact(16, 3 * BLOCK_FRAMES));
    let target = t.0.join("precious.wav");
    std::fs::write(&target, b"the original bytes").unwrap();
    let err = save_wav_failing_after(&doc, &target, SampleType::Int24, BLOCK_FRAMES as u64);
    assert!(err.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes", "target untouched");
    assert!(!temp_path(&target).exists() && temp_files(&target).is_empty(), "no temp file left behind");
    // The same save, not interrupted, replaces it.
    save_wav(&doc, &target, SampleType::Int24, &mut |_| {}).unwrap();
    assert_ne!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(!temp_path(&target).exists() && temp_files(&target).is_empty());
}

#[test]
fn progress_runs_to_one() {
    let t = TempDir::new("progress");
    let doc = doc_from(&t.0.join("store"), &exact(16, 2 * BLOCK_FRAMES + 1));
    let mut seen = Vec::new();
    save_wav(&doc, &t.0.join("p.wav"), SampleType::Int16, &mut |p| seen.push(p)).unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen.windows(2).all(|w| w[1] > w[0]));
    assert_eq!(seen.last().copied(), Some(1.0));
}

#[test]
fn a_save_whose_rename_fails_leaves_no_temp_file_and_the_target_whole() {
    let t = TempDir::new("renamefail");
    let doc = doc_from(&t.0.join("store"), &exact(16, BLOCK_FRAMES));
    // The target is a folder holding a file: the rename over it fails on every system.
    let target = t.0.join("busy.wav");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), b"still here").unwrap();
    assert!(save_wav(&doc, &target, SampleType::Int16, &mut |_| {}).is_err());
    assert!(!temp_path(&target).exists() && temp_files(&target).is_empty(), "no temp file left behind");
    assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"still here", "target untouched");
}

/// Codex review CX-W2 (2026-10-19): a link waiting at the temporary file's name must not make
/// the save write through it (File::create follows links and truncates).
#[cfg(unix)]
#[test]
fn cx_w2_a_link_at_the_temp_name_is_never_followed() {
    let t = TempDir::new("link");
    let doc = doc_from(&t.0.join("store"), &exact(16, BLOCK_FRAMES));
    let target = t.0.join("song.wav");
    let victim = t.0.join("victim.txt");
    std::fs::write(&victim, b"someone else's file").unwrap();
    std::os::unix::fs::symlink(&victim, temp_path(&target)).unwrap();
    save_wav(&doc, &target, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"someone else's file", "the link's target is untouched");
    assert!(!std::fs::symlink_metadata(&target).unwrap().file_type().is_symlink(), "the saved file is a real file");
}

/// Codex review CX-W2: a failing save removes only its own temporary file, never one another
/// save (another window, another run) is writing.
#[test]
fn cx_w2_a_failing_save_never_removes_another_saves_temp_file() {
    let t = TempDir::new("othertemp");
    let doc = doc_from(&t.0.join("store"), &exact(16, 3 * BLOCK_FRAMES));
    let target = t.0.join("shared.wav");
    std::fs::write(temp_path(&target), b"another save's half-written file").unwrap();
    assert!(save_wav_failing_after(&doc, &target, SampleType::Int24, BLOCK_FRAMES as u64).is_err());
    assert_eq!(std::fs::read(temp_path(&target)).unwrap(), b"another save's half-written file");
}

/// A temporary file as a killed save leaves it: named as saves name them, unlocked, last
/// written `age` ago.
fn leftover(target: &Path, tag: &str, age: std::time::Duration) -> PathBuf {
    let name = target.file_name().unwrap().to_string_lossy().into_owned();
    let p = target.with_file_name(format!(".{name}.wavcraft-tmp-{tag}"));
    let f = std::fs::File::create(&p).unwrap();
    f.set_modified(std::time::SystemTime::now() - age).unwrap();
    p
}

/// Codex review CX-W6 (2026-10-19): a temporary file left by a save that was killed is removed by
/// the next save of the same file; one a running save holds (locked), or one written in the last
/// minute, stays.
#[test]
fn cx_w6_the_next_save_removes_only_temp_files_left_by_a_killed_save() {
    let t = TempDir::new("leftover");
    let doc = doc_from(&t.0.join("store"), &exact(16, BLOCK_FRAMES));
    let target = t.0.join("episode.wav");
    let hour = std::time::Duration::from_secs(3600);
    let dead = leftover(&target, "00000000000000aa", hour);
    let young = leftover(&target, "00000000000000bb", std::time::Duration::ZERO);
    let running = leftover(&target, "00000000000000cc", hour);
    let held = std::fs::OpenOptions::new().write(true).open(&running).unwrap();
    held.try_lock().unwrap();
    // Another file's leftovers are never this save's business.
    let other = leftover(&t.0.join("other.wav"), "00000000000000dd", hour);
    save_wav(&doc, &target, SampleType::Int16, &mut |_| {}).unwrap();
    assert!(!dead.exists(), "left by a killed save: removed");
    assert!(young.exists(), "written in the last {LEFTOVER_AGE:?}: kept");
    assert!(running.exists(), "locked by a running save: kept");
    assert!(other.exists(), "another file's: kept");
    drop(held);
    // Once its save has ended (lock gone) and it is old, it goes too.
    assert_eq!(remove_leftover_temps(&target), 1);
    assert!(!running.exists());
}

// ---- AIFF, FLAC, every sample type, RF64 (feature f35) ----

/// `data` as 16-bit samples read back (what a 16-bit save holds).
fn data_quantized(data: &[Vec<f32>]) -> Vec<Vec<f32>> {
    data.iter().map(|c| c.iter().map(|&x| wavcraft_core::save::quantize(x, 16) as f32 / 32768.0).collect()).collect()
}

fn float_data(frames: usize) -> Vec<Vec<f32>> {
    vec![(0..frames).map(|k| (k as f32 * 0.37).sin() * 1.7).collect(), (0..frames).map(|k| -(k as f32) * 1e-6).collect()]
}

#[test]
fn f35_aiff_round_trips_each_sample_type() {
    let t = TempDir::new("aiff");
    let frames = 2 * BLOCK_FRAMES + 123;
    for (ty, data) in [(SampleType::Int16, exact(16, frames)), (SampleType::Int24, exact(24, frames)), (SampleType::Float32, float_data(frames))] {
        let doc = doc_from(&t.0.join(format!("{ty:?}-store")), &data);
        let path = t.0.join(format!("{ty:?}.aiff"));
        save(&doc, &path, SaveFormat::Aiff, ty, &mut |_| {}).unwrap();
        let back = reopen(&path, &t.0.join("sessions"));
        assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (RATE, 2, frames as u64), "{ty:?}");
        assert_eq!(back.source().format, soundcraft_audio_io::FileFormat::Aiff);
        assert_eq!(bits(&content(&back)), bits(&data), "{ty:?} bit for bit");
        assert_eq!(SampleType::for_source(back.source()), ty, "{ty:?}: the type is kept");
    }
}

#[test]
fn f35_thirty_two_bit_integer_round_trips_within_the_step_in_wav_and_aiff() {
    let t = TempDir::new("int32");
    let data = float_data(BLOCK_FRAMES + 5).into_iter().map(|c| c.into_iter().map(|x| x.clamp(-1.0, 0.999)).collect()).collect::<Vec<Vec<f32>>>();
    let doc = doc_from(&t.0.join("store"), &data);
    for (format, name) in [(SaveFormat::Wav, "a.wav"), (SaveFormat::Aiff, "a.aif")] {
        let path = t.0.join(name);
        save(&doc, &path, format, SampleType::Int32, &mut |_| {}).unwrap();
        let back = content(&reopen(&path, &t.0.join("s")));
        for (a, b) in data.iter().flatten().zip(back.iter().flatten()) {
            assert!((a - b).abs() <= 1e-6, "{format:?}: {a} vs {b}");
        }
    }
}

#[test]
fn f35_flac_round_trips_16_and_24_bit_bit_for_bit() {
    let t = TempDir::new("flac");
    for frames in [1usize, 10, 4096, 4097, 2 * BLOCK_FRAMES + 123] {
        for (ty, bits_n) in [(SampleType::Int16, 16), (SampleType::Int24, 24)] {
            let data = exact(bits_n, frames);
            let doc = doc_from(&t.0.join(format!("{frames}-{bits_n}-store")), &data);
            let path = t.0.join(format!("{frames}-{bits_n}.flac"));
            save(&doc, &path, SaveFormat::Flac, ty, &mut |_| {}).unwrap();
            let back = reopen(&path, &t.0.join("sessions"));
            assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (RATE, 2, frames as u64), "{frames} frames, {ty:?}");
            assert_eq!(back.source().format, soundcraft_audio_io::FileFormat::Flac);
            assert_eq!(bits(&content(&back)), bits(&data), "{frames} frames, {ty:?} bit for bit");
        }
    }
}

#[test]
fn f35_flac_compresses_quiet_and_silent_audio() {
    let t = TempDir::new("flacsize");
    let frames = 4 * BLOCK_FRAMES;
    let silent = vec![vec![0.0f32; frames]; 2];
    let tone: Vec<Vec<f32>> = (0..2).map(|_| (0..frames).map(|k| (k as f32 * 0.05).sin() * 0.5).collect()).collect();
    for (name, data, max_ratio) in [("silent", silent, 50), ("tone", tone, 1)] {
        let doc = doc_from(&t.0.join(format!("{name}-store")), &data);
        let path = t.0.join(format!("{name}.flac"));
        save(&doc, &path, SaveFormat::Flac, SampleType::Int16, &mut |_| {}).unwrap();
        let raw = frames * 2 * 2;
        let size = std::fs::metadata(&path).unwrap().len() as usize;
        assert!(size * max_ratio < raw, "{name}: {size} bytes of {raw}, wanted under 1/{max_ratio}");
        assert_eq!(bits(&content(&reopen(&path, &t.0.join("s")))), bits(&data_quantized(&data)), "{name}");
    }
}

#[test]
fn f35_flac_refuses_32_bit_and_leaves_the_target_whole() {
    let t = TempDir::new("flacrefuse");
    let doc = doc_from(&t.0.join("store"), &exact(16, 1000));
    let target = t.0.join("keep.flac");
    std::fs::write(&target, b"the original bytes").unwrap();
    for ty in [SampleType::Int32, SampleType::Float32] {
        assert!(save(&doc, &target, SaveFormat::Flac, ty, &mut |_| {}).is_err(), "{ty:?}");
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(temp_files(&target).is_empty());
    assert_eq!(SaveFormat::Flac.fit(SampleType::Float32), SampleType::Int24);
}

#[test]
fn f35_a_failed_aiff_or_flac_save_leaves_no_temp_file_and_the_original_whole() {
    let t = TempDir::new("failfmt");
    let doc = doc_from(&t.0.join("store"), &exact(16, 3 * BLOCK_FRAMES));
    for (format, name, ty) in [
        (SaveFormat::Aiff, "x.aiff", SampleType::Int24),
        (SaveFormat::Flac, "x.flac", SampleType::Int16),
        (SaveFormat::Aiff, "y.aiff", SampleType::Float32),
    ] {
        let target = t.0.join(name);
        std::fs::write(&target, b"the original bytes").unwrap();
        assert!(save_failing_after(&doc, &target, format, ty, BLOCK_FRAMES as u64).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes", "{name}: target untouched");
        assert!(temp_files(&target).is_empty(), "{name}: no temp file left behind");
        save(&doc, &target, format, ty, &mut |_| {}).unwrap();
        assert_ne!(std::fs::read(&target).unwrap(), b"the original bytes");
        assert!(temp_files(&target).is_empty());
    }
}

#[test]
fn f35_the_extension_picks_the_format() {
    for (name, want) in [
        ("a.wav", Some(SaveFormat::Wav)),
        ("a.WAV", Some(SaveFormat::Wav)),
        ("a.aif", Some(SaveFormat::Aiff)),
        ("a.AIFF", Some(SaveFormat::Aiff)),
        ("a.flac", Some(SaveFormat::Flac)),
        ("a.mp3", None),
        ("a", None),
    ] {
        assert_eq!(SaveFormat::from_path(Path::new(name)), want, "{name}");
    }
}

#[test]
fn f35_a_wav_above_the_threshold_is_rf64_and_reads_back() {
    let t = TempDir::new("rf64");
    for (ty, data) in
        [(SampleType::Int24, exact(24, BLOCK_FRAMES + 11)), (SampleType::Float32, float_data(BLOCK_FRAMES + 11)), (SampleType::Int16, exact(16, 7))]
    {
        let doc = doc_from(&t.0.join(format!("{ty:?}-store")), &data);
        let big = t.0.join(format!("{ty:?}-big.wav"));
        save_wav_rf64_over(&doc, &big, ty, 1000).unwrap();
        let bytes = std::fs::read(&big).unwrap();
        let rf64 = bytes.len() > 1000;
        assert_eq!(&bytes[..4], if rf64 { b"RF64" } else { b"RIFF" }, "{ty:?}");
        if rf64 {
            assert_eq!(&bytes[4..8], &u32::MAX.to_le_bytes());
            assert_eq!(&bytes[12..16], b"ds64");
            let (riff, data_len, frames) = (
                u64::from_le_bytes(bytes[20..28].try_into().unwrap()),
                u64::from_le_bytes(bytes[28..36].try_into().unwrap()),
                u64::from_le_bytes(bytes[36..44].try_into().unwrap()),
            );
            assert_eq!(riff, bytes.len() as u64 - 8, "ds64 RIFF size");
            assert_eq!(frames, data[0].len() as u64);
            assert_eq!(
                data_len,
                frames
                    * 2
                    * if ty == SampleType::Int24 {
                        3
                    } else if ty == SampleType::Int16 {
                        2
                    } else {
                        4
                    }
            );
        }
        let back = reopen(&big, &t.0.join("s"));
        assert_eq!(bits(&content(&back)), bits(&data), "{ty:?}");
        // The same save under the real threshold is a plain RIFF.
        let small = t.0.join(format!("{ty:?}-small.wav"));
        save_wav(&doc, &small, ty, &mut |_| {}).unwrap();
        assert_eq!(&std::fs::read(&small).unwrap()[..4], b"RIFF");
    }
}

/// The chunk ids of a RIFF/WAVE file, in order.
fn chunk_ids(bytes: &[u8]) -> Vec<String> {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8, "RIFF size is the file");
    let (mut at, mut ids) = (12, Vec::new());
    while at + 8 <= bytes.len() {
        ids.push(String::from_utf8_lossy(&bytes[at..at + 4]).into_owned());
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8 + len + (len & 1);
    }
    assert_eq!(at, bytes.len(), "chunks tile the file");
    ids
}

fn test_chunks() -> Vec<ExtraChunk> {
    vec![ExtraChunk { id: *b"TEST", data: b"odd".to_vec() }]
}

#[test]
fn a262_metadata_off_writes_only_format_and_data_chunks() {
    let t = TempDir::new("meta");
    let doc = doc_from(&t.0.join("store"), &exact(16, 1000));
    for (include, want) in [(true, vec!["fmt ", "data", "TEST"]), (false, vec!["fmt ", "data"])] {
        let target = t.0.join(format!("m{include}.wav"));
        let opts = SaveOptions { include_metadata: include, extra_chunks: test_chunks(), ..SaveOptions::default() };
        save_wav_with(&doc, &target, SampleType::Int16, &opts, &mut |_| {}).unwrap();
        assert_eq!(chunk_ids(&std::fs::read(&target).unwrap()), want, "include_metadata={include}");
        // The audio is the same either way.
        assert_eq!(bits(&content(&reopen(&target, &t.0.join("s")))), bits(&content(&doc)));
    }
}

#[test]
fn a263_cancel_keeps_the_original_and_leaves_no_temp_file() {
    let t = TempDir::new("cancel");
    let doc = doc_from(&t.0.join("store"), &exact(16, 4 * BLOCK_FRAMES));
    let target = t.0.join("keep.wav");
    std::fs::write(&target, b"the original bytes").unwrap();
    let cancel = AtomicBool::new(false);
    let opts = SaveOptions { include_metadata: true, cancel: Some(&cancel), ..SaveOptions::default() };
    let mut blocks = 0;
    let err = save_wav_with(&doc, &target, SampleType::Int16, &opts, &mut |_| {
        blocks += 1;
        if blocks == 2 {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    assert!(matches!(err, Err(SaveError::Cancelled)), "{err:?}");
    assert_eq!(blocks, 2, "stopped at the next block");
    assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(temp_files(&target).is_empty());
}
