//! Task 06, first part: WAV save. Numbers refer to task 06's acceptance tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav, save_wav_failing_after, temp_path};
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
    assert!(!temp_path(&target).exists(), "no temp file left behind");
    // The same save, not interrupted, replaces it.
    save_wav(&doc, &target, SampleType::Int24, &mut |_| {}).unwrap();
    assert_ne!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(!temp_path(&target).exists());
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
