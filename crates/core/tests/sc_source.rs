//! Item f102 (C322): a WavCraft document as a SoundCraft clip source.

use soundcraft_model::{ClipContent, SampleSource};
use std::path::{Path, PathBuf};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::sc_source::{DocSource, read_checksum, session_with_document};
use wavcraft_core::store::{Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-scsrc-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>], block: usize) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 1 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(block).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn data(frames: usize) -> Vec<Vec<f32>> {
    (0..2).map(|c| (0..frames).map(|i| ((i * 31 + c * 977) % 2003) as f32 / 1001.0 - 1.0).collect()).collect()
}

#[test]
fn a_document_is_a_clip_source_that_reads_the_same_samples() {
    let dir = TempDir::new("clip");
    let want = data(150_000);
    let doc = doc_from(&dir.0, &want, 1_000);
    let (session, source) = session_with_document(&doc, "doc").unwrap();

    // The model session: one track, one audio clip over the whole document, source streamed.
    assert_eq!(session.sources.len(), 1);
    let clip = session.all_clips().next().unwrap();
    assert_eq!(clip.length as u64, doc.len_frames());
    assert!(matches!(clip.content, ClipContent::Audio { source: s, offset: 0 } if s == source));
    assert!(session.pool.get(source).is_none(), "not held as an in-RAM buffer");
    let src = session.pool.get_source(source).unwrap();
    assert_eq!((src.channels(), src.sample_rate(), src.frames()), (2, RATE, 150_000));

    // Reads match the document, mid-file, across block edges and at the end.
    for (ch, w) in want.iter().enumerate() {
        for (start, len) in [(0usize, 10usize), (995, 2_100), (149_990, 50), (70_000, 65_536)] {
            let mut out = vec![f32::NAN; len];
            let n = src.read(ch, start as u64, &mut out);
            assert_eq!(n, len.min(150_000 - start));
            assert_eq!(out[..n], w[start..start + n]);
        }
    }
    let mut out = [0.0f32; 4];
    assert_eq!(src.read(0, 150_000, &mut out), 0);
    assert_eq!(src.read(9, 0, &mut out), 0);

    // The streamed checksum equals one computed from the data directly.
    let (frames, sum) = read_checksum(&session).unwrap();
    assert_eq!(frames, 150_000);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in &want {
        for v in c {
            for b in v.to_bits().to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    assert_eq!(sum, h);
}

#[test]
fn doc_source_reports_a_failed_read_as_zero_frames() {
    let dir = TempDir::new("fail");
    let doc = doc_from(&dir.0, &data(100), 100);
    let src = DocSource::new(doc);
    let mut out = [0.0f32; 8];
    assert_eq!(src.read(5, 0, &mut out), 0);
}
