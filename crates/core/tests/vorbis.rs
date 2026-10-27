//! f93: Ogg Vorbis save. A synthetic tone is encoded, decoded again with symphonia (through our
//! own streamed open) and checked for length and frequency; the save keeps the temp-file safety.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, SaveFormat, save, save_failing_after, save_vorbis, temp_files};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-vorbis-test-{}-{name}", std::process::id()));
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

fn tone(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames).map(|k| 0.5 * (2.0 * std::f32::consts::PI * freq * k as f32 / RATE as f32).sin()).collect()
}

fn decode(path: &Path, sessions: &Path) -> Vec<Vec<f32>> {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    let d = open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(d.source().format, soundcraft_audio_io::FileFormat::Ogg);
    (0..d.channel_count())
        .map(|c| {
            let mut v = vec![0f32; d.len_frames() as usize];
            assert_eq!(d.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// Frequency from rising zero crossings over the middle of the signal (skips the start-up).
fn frequency(x: &[f32]) -> f32 {
    let (a, b) = (x.len() / 10, x.len() * 9 / 10);
    let mut first = None;
    let (mut last, mut n) = (0usize, 0u32);
    for k in a + 1..b {
        if x[k - 1] < 0.0 && x[k] >= 0.0 {
            first.get_or_insert(k);
            last = k;
            n += 1;
        }
    }
    (n - 1) as f32 * RATE as f32 / (last - first.unwrap()) as f32
}

#[test]
fn f93_a_tone_saved_as_vorbis_decodes_to_the_same_length_and_frequency() {
    let t = TempDir::new("tone");
    // Two blocks and a bit, so the save streams more than one chunk.
    let frames = 2 * BLOCK_FRAMES + 12_345;
    let data = vec![tone(1000.0, frames), tone(2500.0, frames)];
    let doc = doc_from(&t.0.join("store"), &data);
    for q in [-1.0f32, 5.0, 10.0] {
        let path = t.0.join(format!("q{q}.ogg"));
        let mut last = 0.0;
        save_vorbis(&doc, &path, q, &mut |p| last = p).unwrap();
        assert!((last - 1.0).abs() < 1e-9, "progress reached the end");
        let back = decode(&path, &t.0.join("sessions"));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].len(), frames, "q={q}: decoded length equals the document's");
        for (ch, want) in [(0, 1000.0), (1, 2500.0)] {
            let f = frequency(&back[ch]);
            assert!((f - want).abs() < want * 0.01, "q={q} channel {ch}: {f} Hz, wanted {want}");
        }
        assert!(temp_files(&path).is_empty());
    }
}

#[test]
fn f93_higher_quality_makes_a_bigger_file() {
    let t = TempDir::new("size");
    let noisy: Vec<f32> = (0..3 * BLOCK_FRAMES).map(|k| ((k as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32 - 0.5).collect();
    let doc = doc_from(&t.0.join("store"), &[noisy]);
    let size = |q: f32| {
        let p = t.0.join(format!("n{q}.ogg"));
        save_vorbis(&doc, &p, q, &mut |_| {}).unwrap();
        std::fs::metadata(&p).unwrap().len()
    };
    let (lo, mid, hi) = (size(-1.0), size(5.0), size(10.0));
    assert!(lo < mid && mid < hi, "{lo} < {mid} < {hi}");
}

#[test]
fn f93_a_quality_outside_minus_one_to_ten_is_refused_and_leaves_the_target_whole() {
    let t = TempDir::new("range");
    let doc = doc_from(&t.0.join("store"), &[tone(440.0, 1000)]);
    let target = t.0.join("keep.ogg");
    std::fs::write(&target, b"the original bytes").unwrap();
    for q in [-1.5f32, 10.5, f32::NAN] {
        assert!(save_vorbis(&doc, &target, q, &mut |_| {}).is_err(), "{q}");
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(temp_files(&target).is_empty());
}

#[test]
fn f93_a_failed_vorbis_save_leaves_no_temp_file_and_the_original_whole() {
    let t = TempDir::new("fail");
    let doc = doc_from(&t.0.join("store"), &[tone(440.0, 3 * BLOCK_FRAMES)]);
    let target = t.0.join("x.ogg");
    std::fs::write(&target, b"the original bytes").unwrap();
    assert!(save_failing_after(&doc, &target, SaveFormat::Vorbis, SampleType::Float32, BLOCK_FRAMES as u64).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert!(temp_files(&target).is_empty());
    save(&doc, &target, SaveFormat::Vorbis, SampleType::Int16, &mut |_| {}).unwrap();
    assert_ne!(std::fs::read(&target).unwrap(), b"the original bytes");
    assert_eq!(SaveFormat::from_path(Path::new("a.OGG")), Some(SaveFormat::Vorbis));
}
