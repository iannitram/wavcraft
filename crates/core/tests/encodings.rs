//! Task 04b acceptance tests 3, 4 and 6: 16- and 24-bit integer blocks beside f32 ones in the
//! same packs, their edge values, cold and warm reads, byte counts and pack deletion.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{
    BlockHandle, Encoding, I24_MAX, I24_MIN, Store, StoreOptions, f32_to_i16_exact, f32_to_i24_exact, i16_to_f32, i24_to_f32,
};

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-enc-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path, pack_bytes: u64, cache_bytes: u64) -> Store {
    Store::create_with(dir, StoreOptions { pack_bytes, cache_bytes, keep: false, max_bytes: None }).unwrap()
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

fn read_all(store: &Store, h: &BlockHandle) -> Vec<f32> {
    let mut out = vec![f32::NAN; h.frames() as usize];
    store.read(h.block(), 0, &mut out).unwrap();
    out
}

/// Test 3: the 24-bit edge values (and the 16-bit ones) are written and read back exactly, the
/// negative ones sign-extended; cold (from disk) and warm (from the cache).
#[test]
fn integer_edge_values_read_back_exactly() {
    let dir = TempDir::new("edges");
    let s = store(&dir.0.join("s"), 1 << 20, 64 << 20);
    let i24 = [-8_388_608, -1, 0, 1, 8_388_607, -4_194_304, 65_535, -65_536];
    let i16 = [i16::MIN, -1, 0, 1, i16::MAX];
    let h24 = s.append_i24(&i24).unwrap();
    let h16 = s.append_i16(&i16).unwrap();
    assert_eq!((h24.encoding(), h16.encoding()), (Encoding::I24, Encoding::I16));
    assert_eq!((h24.block().disk_bytes(), h16.block().disk_bytes()), (24, 10));
    let want24: Vec<f32> = i24.iter().map(|&v| v as f32 / 8_388_608.0).collect();
    let want16: Vec<f32> = i16.iter().map(|&v| f32::from(v) / 32_768.0).collect();
    assert_eq!(want24[0], -1.0);
    assert_eq!(want24[1], -1.0 / 8_388_608.0, "sign-extended, not 0x00FF_FFFF");
    for _ in 0..2 {
        s.clear_cache();
        assert_eq!(bits(&read_all(&s, &h24)), bits(&want24), "cold");
        assert_eq!(bits(&read_all(&s, &h16)), bits(&want16), "cold");
        assert_eq!(bits(&read_all(&s, &h24)), bits(&want24), "warm");
        assert_eq!(bits(&read_all(&s, &h16)), bits(&want16), "warm");
        assert!(s.cache_stats().hits >= 2);
    }
    // On disk: 3 bytes little-endian each, then 2 each.
    s.flush().unwrap();
    let pack = std::fs::read(s.pack_path(1)).unwrap();
    assert_eq!(pack.len(), 34);
    assert_eq!(&pack[..6], &[0x00, 0x00, 0x80, 0xFF, 0xFF, 0xFF]);
    assert_eq!(&pack[24..28], &[0x00, 0x80, 0xFF, 0xFF]);
    // The exact conversions back are the inverse, and refuse what has no exact integer.
    for &v in &i24 {
        assert_eq!(f32_to_i24_exact(i24_to_f32(v)), Some(v));
    }
    for &v in &i16 {
        assert_eq!(f32_to_i16_exact(i16_to_f32(v)), Some(v));
    }
    for x in [1.0f32, 1.5, -1.0 - 1.0 / 8_388_608.0, 0.1, f32::NAN, f32::INFINITY, -0.0] {
        assert_eq!(f32_to_i24_exact(x), None, "{x}: no 24-bit integer reads back as it");
    }
    assert_eq!(f32_to_i16_exact(-1.0), Some(i16::MIN));
    assert_eq!(f32_to_i16_exact(1.0 / 65_536.0), None, "between 16-bit steps");
    assert_eq!(f32_to_i24_exact(1.0 / 65_536.0), Some(128));
    assert_eq!((I24_MIN, I24_MAX), (-8_388_608, 8_388_607));
}

/// Block `k`'s samples in `encoding` (as the f32 a read must return), `frames` long.
fn content(k: usize, encoding: Encoding, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let n = (k * 7_919 + i * 31) as i64;
            match encoding {
                Encoding::I16 => i16_to_f32((n % 65_536 - 32_768) as i16),
                Encoding::I24 => i24_to_f32((n * 257 % 16_777_216 - 8_388_608) as i32),
                Encoding::F32 => (n % 1_000_003) as f32 * 1.3e-6 - 0.65,
            }
        })
        .collect()
}

fn append(s: &Store, encoding: Encoding, samples: &[f32]) -> Arc<BlockHandle> {
    match encoding {
        Encoding::F32 => s.append(samples),
        Encoding::I16 => s.append_i16(&samples.iter().map(|&x| f32_to_i16_exact(x).unwrap()).collect::<Vec<_>>()),
        Encoding::I24 => s.append_i24(&samples.iter().map(|&x| f32_to_i24_exact(x).unwrap()).collect::<Vec<_>>()),
    }
    .unwrap()
}

const ENCODINGS: [Encoding; 3] = [Encoding::F32, Encoding::I16, Encoding::I24];

/// Test 4: F32, I16 and I24 blocks interleaved in the same packs across rollovers, read back
/// straight away, cold, warm and in parts: bit-exact.
#[test]
fn mixed_encodings_share_packs_across_rollovers() {
    let dir = TempDir::new("mixed");
    // Packs of 300 KB: a few blocks each, rolling over at different byte sizes.
    let s = store(&dir.0.join("s"), 300_000, 1 << 20);
    let sizes = [1usize, 65_536, 65_535, 1_000, 40_000, 65_536, 7, 30_000, 65_536];
    let mut blocks = Vec::new();
    for (k, &frames) in sizes.iter().enumerate() {
        for (j, &e) in ENCODINGS.iter().enumerate() {
            let want = content(k * 3 + j, e, frames);
            let h = append(&s, e, &want);
            assert_eq!(bits(&read_all(&s, &h)), bits(&want), "straight after append");
            assert_eq!(h.block().disk_bytes(), frames as u64 * e.width());
            blocks.push((h, want));
        }
    }
    let packs: Vec<u32> = blocks.iter().map(|(h, _)| h.block().pack).collect();
    assert!(packs.last().copied().unwrap_or(0) >= 4, "several rollovers: {packs:?}");
    // Each pack is as long as the blocks in it, at their own widths.
    s.flush().unwrap();
    for p in s.packs() {
        let want: u64 = blocks.iter().filter(|(h, _)| h.block().pack == p).map(|(h, _)| h.block().disk_bytes()).sum();
        assert_eq!(std::fs::metadata(s.pack_path(p)).unwrap().len(), want, "pack {p}");
    }
    for pass in ["cold", "warm"] {
        if pass == "cold" {
            s.clear_cache();
        }
        for (h, want) in blocks.iter().rev() {
            assert_eq!(bits(&read_all(&s, h)), bits(want), "{pass} {:?}", h.block());
            let f = h.frames();
            if f > 10 {
                let mut part = vec![0.0; (f / 2) as usize];
                s.read(h.block(), f / 3, &mut part).unwrap();
                assert_eq!(bits(&part), bits(&want[(f / 3) as usize..(f / 3 + f / 2) as usize]), "{pass} part");
            }
        }
    }
}

/// Test 6: `disk_bytes_referenced`, `bytes_live` and `bytes_on_disk` count each block at its own
/// width, and packs are deleted when their last block goes, as before.
#[test]
fn byte_counts_and_pack_deletion_with_mixed_encodings() {
    let dir = TempDir::new("counts");
    let frames = 65_536usize;
    // One f32 block (256 KiB) + one I16 (128 KiB) + one I24 (192 KiB) per pack: 576 KiB.
    let s = store(&dir.0.join("s"), 576 << 10, 1 << 20);
    let mut pieces = Vec::new();
    for k in 0..9 {
        let e = ENCODINGS[k % 3];
        pieces.push(Piece::whole(append(&s, e, &content(k, e, frames))).unwrap());
    }
    assert_eq!(pieces.iter().map(|p| p.block().block().pack).collect::<Vec<_>>(), vec![1, 1, 1, 2, 2, 2, 3, 3, 3]);
    let per_pack = (frames as u64) * (4 + 2 + 3);
    assert_eq!(s.bytes_live(), 3 * per_pack);
    assert_eq!(s.bytes_on_disk(), 3 * per_pack);

    // A document that uses every block, one of them twice, and one that uses only packs 1 and 3.
    let mut all = Channel::from_pieces(pieces.iter().cloned());
    all.push(pieces[5].sub(0, 100).unwrap());
    let some = Channel::from_pieces(pieces.iter().filter(|p| p.block().block().pack != 2).cloned());
    let doc_all = DocState::new(s.clone(), vec![all], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap();
    let doc_some = DocState::new(s.clone(), vec![some], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap();
    assert_eq!(doc_all.disk_bytes_referenced(), 3 * per_pack, "each block once, at its width");
    assert_eq!(doc_some.disk_bytes_referenced(), 2 * per_pack);
    drop(pieces);
    drop(doc_all);
    assert_eq!(s.packs(), vec![1, 3], "pack 2 had no live blocks left and was sealed: deleted");
    assert_eq!(s.bytes_live(), 2 * per_pack);
    assert_eq!(s.bytes_on_disk(), 2 * per_pack);
    // An I16 block's handle alone keeps its pack.
    let keep = doc_some.channel(0).unwrap().pieces()[1].clone();
    assert_eq!(keep.block().encoding(), Encoding::I16);
    drop(doc_some);
    assert_eq!(s.packs(), vec![1, 3], "pack 3 is still being written");
    assert_eq!(s.bytes_live(), frames as u64 * 2);
    let mut out = vec![0.0; 50];
    s.read(keep.block().block(), 10, &mut out).unwrap();
    assert_eq!(bits(&out), bits(&content(1, Encoding::I16, 60)[10..]));
    drop(keep);
    assert_eq!(s.bytes_live(), 0);
    assert_eq!(s.packs(), vec![3]);
}
