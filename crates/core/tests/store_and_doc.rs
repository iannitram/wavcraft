//! Task 03 acceptance tests 1-7 and 12: the store, its cache and live counts, and the piece table.
//! (Tests 8-11, streamed open of the kit files, are in `crates/testkit/tests/open.rs`.)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use proptest::prelude::*;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, BlockHandle, LOCK_FILE, Store, StoreOptions};

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-core-test-{}-{name}", std::process::id()));
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

/// Sample `i` of test block `k`: exact in f32, different for every (k, i) that matters here.
fn sample_at(k: usize, i: usize) -> f32 {
    ((k * 7_919 + i * 31) % 1_000_003) as f32 - 500_000.0
}

fn block_samples(k: usize, frames: usize) -> Vec<f32> {
    (0..frames).map(|i| sample_at(k, i)).collect()
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

fn read_all(store: &Store, h: &BlockHandle) -> Vec<f32> {
    let mut out = vec![f32::NAN; h.frames() as usize];
    store.read(h.block(), 0, &mut out).unwrap();
    out
}

/// Blocks appended so far, with the test number each was made from.
type SharedBlocks = Arc<Mutex<Vec<(usize, Arc<BlockHandle>)>>>;
/// Block sizes; picks of (block, offset, len); reads of (start, frames).
type Layout = (Vec<usize>, Vec<(usize, u32, u32)>, Vec<(u64, usize)>);

const BLOCK_BYTES: u64 = BLOCK_FRAMES as u64 * 4;

/// Test 1: blocks of 1, 65,535 and 65,536 frames across pack rollovers, read back cold and warm.
#[test]
fn blocks_read_back_bit_exact_across_pack_rollovers_cold_and_warm() {
    let dir = TempDir::new("t1");
    // Room for one full block plus a little: every third block starts a new pack.
    let s = store(&dir.0.join("s"), BLOCK_BYTES + 1_000, 64 << 20);
    let sizes = [1usize, 65_535, 65_536, 1, 65_536, 65_535, 65_536];
    let blocks: Vec<(Arc<BlockHandle>, Vec<f32>)> = sizes
        .iter()
        .enumerate()
        .map(|(k, &n)| {
            let samples = block_samples(k, n);
            // Readable as soon as append returns, before any flush.
            let h = s.append(&samples).unwrap();
            assert_eq!(bits(&read_all(&s, &h)), bits(&samples), "block {k} straight after append");
            (h, samples)
        })
        .collect();
    let packs: Vec<u32> = blocks.iter().map(|(h, _)| h.block().pack).collect();
    assert!(packs.windows(2).all(|w| w[0] <= w[1]));
    assert!(*packs.last().unwrap() >= 4, "several rollovers: {packs:?}");
    assert_eq!(s.packs().len(), *packs.last().unwrap() as usize);
    for round in ["cold", "warm"] {
        if round == "cold" {
            s.clear_cache();
        }
        for (k, (h, want)) in blocks.iter().enumerate() {
            assert_eq!(bits(&read_all(&s, h)), bits(want), "block {k} {round}");
            // A part of a block, from an offset.
            if want.len() > 10 {
                let mut part = vec![0.0; 7];
                s.read(h.block(), 3, &mut part).unwrap();
                assert_eq!(bits(&part), bits(&want[3..10]));
            }
        }
        if round == "cold" {
            assert!(s.cache_stats().misses >= sizes.len() as u64);
        } else {
            assert!(s.cache_stats().hits >= sizes.len() as u64);
        }
    }
    // Reads past a block's end are errors, not panics; the zero block reads zeros.
    let mut too_long = vec![0.0; 2];
    assert!(s.read(blocks[0].0.block(), 0, &mut too_long).is_err());
    let mut z = vec![1.0; 100];
    s.read(Store::zero().block(), 1_000_000, &mut z).unwrap();
    assert!(z.iter().all(|v| *v == 0.0));
    assert!(s.append(&[]).is_err());
    assert!(s.append(&vec![0.0; BLOCK_FRAMES + 1]).is_err());
    assert_eq!(s.bytes_live(), sizes.iter().map(|n| *n as u64 * 4).sum::<u64>());
}

/// Test 2: a budget of 4 blocks never holds more than 4, and reads stay right.
#[test]
fn the_cache_keeps_to_its_budget() {
    let dir = TempDir::new("t2");
    let s = store(&dir.0.join("s"), 1 << 30, 4 * BLOCK_BYTES);
    let blocks: Vec<Arc<BlockHandle>> = (0..100).map(|k| s.append(&block_samples(k, BLOCK_FRAMES)).unwrap()).collect();
    s.clear_cache();
    for _ in 0..3 {
        for (k, h) in blocks.iter().enumerate() {
            let mut out = vec![0.0; 100];
            s.read(h.block(), 1_000, &mut out).unwrap();
            assert_eq!(bits(&out), bits(&block_samples(k, 1_100)[1_000..]), "block {k}");
            let st = s.cache_stats();
            assert!(st.blocks <= 4 && st.bytes <= 4 * BLOCK_BYTES, "{st:?}");
        }
    }
    let st = s.cache_stats();
    assert!(st.peak_blocks <= 4 && st.peak_bytes <= 4 * BLOCK_BYTES, "{st:?}");
    assert_eq!(st.peak_blocks, 4);
    // The last four read are the ones held: reading them again hits.
    let before = s.cache_stats().hits;
    for h in &blocks[96..] {
        read_all(&s, h);
    }
    assert_eq!(s.cache_stats().hits, before + 4);
}

/// Test 3: 8 reader threads on random blocks while one thread appends; no wrong read, no deadlock.
#[test]
fn eight_readers_and_one_writer_at_once() {
    let dir = TempDir::new("t3");
    // A small cache and small packs, so reads miss, evict, and cross pack rollovers.
    let s = store(&dir.0.join("s"), 8 * BLOCK_BYTES, 16 * BLOCK_BYTES);
    let blocks: SharedBlocks = Arc::new(Mutex::new(Vec::new()));
    for k in 0..8 {
        let h = s.append(&block_samples(k, 4_096 + k)).unwrap();
        blocks.lock().unwrap().push((k, h));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel();
    let mut threads = Vec::new();
    for t in 0..8u64 {
        let (s, blocks, stop, done_tx) = (s.clone(), blocks.clone(), stop.clone(), done_tx.clone());
        threads.push(std::thread::spawn(move || {
            let mut x = 0x2545_F491_4F6C_DD1D ^ (t + 1);
            let mut reads = 0u64;
            while !stop.load(Ordering::Relaxed) || reads < 200 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let (k, h) = {
                    let list = blocks.lock().unwrap();
                    let (k, h) = &list[(x % list.len() as u64) as usize];
                    (*k, h.clone())
                };
                let frames = h.frames() as usize;
                let off = (x >> 20) as usize % frames;
                let n = ((x >> 40) as usize % 2_000).min(frames - off).max(1);
                let mut out = vec![0.0; n];
                s.read(h.block(), off as u32, &mut out).unwrap();
                let want: Vec<f32> = (off..off + n).map(|i| sample_at(k, i)).collect();
                assert_eq!(bits(&out), bits(&want), "block {k} at {off}");
                reads += 1;
            }
            done_tx.send(reads).unwrap();
        }));
    }
    let writer = {
        let (s, blocks) = (s.clone(), blocks.clone());
        std::thread::spawn(move || {
            for k in 8..400 {
                let h = s.append(&block_samples(k, 1 + (k * 977) % BLOCK_FRAMES)).unwrap();
                blocks.lock().unwrap().push((k, h));
            }
        })
    };
    writer.join().unwrap();
    stop.store(true, Ordering::Relaxed);
    let mut total = 0;
    for _ in 0..8 {
        total += done_rx.recv_timeout(Duration::from_secs(120)).expect("a reader hung (deadlock?)");
    }
    for t in threads {
        t.join().unwrap();
    }
    assert!(total >= 1_600);
    assert!(s.packs().len() > 3, "the writer crossed pack rollovers while readers read");
}

/// Test 4: when nothing uses pack 2 any more, its file is deleted; the others stay.
#[test]
fn a_sealed_pack_with_no_live_blocks_is_deleted() {
    let dir = TempDir::new("t4");
    let s = store(&dir.0.join("s"), 2 * BLOCK_BYTES, 64 << 20);
    // Two blocks per pack: packs 1, 2 and 3.
    let pieces: Vec<Piece> = (0..6).map(|k| Piece::whole(s.append(&block_samples(k, BLOCK_FRAMES)).unwrap()).unwrap()).collect();
    assert_eq!(pieces.iter().map(|p| p.block().block().pack).collect::<Vec<_>>(), vec![1, 1, 2, 2, 3, 3]);
    // Pack 2's blocks used twice over (pieces of the same block share its handle).
    let mut all = Channel::from_pieces(pieces.iter().cloned());
    all.push(pieces[2].sub(10, 20).unwrap());
    let keep = Channel::from_pieces(pieces.iter().filter(|p| p.block().block().pack != 2).cloned());
    drop(pieces);
    let doc_all = DocState::new(s.clone(), vec![all], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap();
    let doc_keep = DocState::new(s.clone(), vec![keep], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap();
    for p in 1..=3 {
        assert!(s.pack_path(p).exists(), "pack {p}");
    }
    assert_eq!(s.bytes_live(), 6 * BLOCK_BYTES);
    drop(doc_all);
    assert!(!s.pack_path(2).exists(), "pack 2 has no live blocks and is sealed");
    assert!(s.pack_path(1).exists() && s.pack_path(3).exists());
    assert_eq!(s.packs(), vec![1, 3]);
    assert_eq!(s.bytes_live(), 4 * BLOCK_BYTES);
    // Pack 3 is still being written: emptying it does not delete it until it is sealed.
    let tail = doc_keep.channel(0).unwrap().pieces()[2..].to_vec();
    let head = DocState::new(
        s.clone(),
        vec![Channel::from_pieces(doc_keep.channel(0).unwrap().pieces()[..2].to_vec())],
        48_000,
        SourceFormat::synthetic(48_000, 1),
    )
    .unwrap();
    drop(doc_keep);
    drop(tail);
    assert!(s.pack_path(3).exists(), "the pack being written stays");
    let next = s.append(&block_samples(9, BLOCK_FRAMES)).unwrap();
    assert_eq!(next.block().pack, 4);
    assert!(!s.pack_path(3).exists(), "sealed with no live blocks: deleted");
    assert!(s.pack_path(1).exists());
    let mut out = vec![0.0; 10];
    head.read(0, 5, &mut out).unwrap();
    assert_eq!(bits(&out), bits(&block_samples(0, 15)[5..]));
}

/// Test 6: `locate` at and around every piece boundary, at the end and beyond; and `split_at`.
#[test]
fn locate_and_split_at_piece_boundaries() {
    let dir = TempDir::new("t6");
    let s = store(&dir.0.join("s"), 1 << 30, 64 << 20);
    let lens = [1u32, 5, 65_536, 2, 300];
    let mut ch = Channel::new();
    for (k, n) in lens.iter().enumerate() {
        let b = s.append(&block_samples(k, 65_536)).unwrap();
        ch.push(Piece::new(b, 0, *n).unwrap());
    }
    let mut start = 0u64;
    for (i, n) in lens.iter().enumerate() {
        let end = start + u64::from(*n);
        assert_eq!(ch.locate(start), Some((i, 0)));
        assert_eq!(ch.locate(end - 1), Some((i, n - 1)));
        if start > 0 {
            assert_eq!(ch.locate(start - 1), Some((i - 1, lens[i - 1] - 1)));
        }
        assert_eq!(ch.piece_start(i), Some(start));
        start = end;
    }
    let len = ch.len();
    assert_eq!(len, lens.iter().map(|n| u64::from(*n)).sum::<u64>());
    // Documented: no piece holds the end or anything past it.
    assert_eq!(ch.locate(len), None);
    assert_eq!(ch.locate(len + 1), None);
    assert_eq!(ch.locate(u64::MAX), None);
    // split_at: at a boundary it splits nothing; inside a piece it makes two; at the end it
    // gives the count; past the end, None.
    let before = ch.clone();
    assert_eq!(ch.split_at(6), Some(2));
    assert_eq!(ch.piece_count(), 5);
    assert_eq!(ch.split_at(6 + 1_000), Some(3));
    assert_eq!(ch.piece_count(), 6);
    assert_eq!(ch.locate(6 + 1_000), Some((3, 0)));
    assert_eq!(ch.split_at(len), Some(6));
    assert_eq!(ch.split_at(len + 1), None);
    assert_eq!(ch.len(), len);
    let (mut a, mut b) = (vec![0.0; len as usize], vec![0.0; len as usize]);
    assert_eq!(ch.read(&s, 0, &mut a).unwrap(), len as usize);
    before.read(&s, 0, &mut b).unwrap();
    assert_eq!(bits(&a), bits(&b), "splitting never changes the samples");
    // Reads past the end return what is there.
    let mut out = vec![0.0; 10];
    assert_eq!(ch.read(&s, len - 3, &mut out).unwrap(), 3);
    assert_eq!(ch.read(&s, len, &mut out).unwrap(), 0);
    assert_eq!(ch.read(&s, len + 100, &mut out).unwrap(), 0);
}

/// Test 7: a 3-hour (518,400,000-frame) document of zero-block pieces and one real block near
/// the end reads that block at frame 518,399,000, without allocating the document.
#[test]
fn three_hours_of_frames_read_at_the_far_end() {
    const LEN: u64 = 518_400_000;
    let dir = TempDir::new("t7");
    let s = store(&dir.0.join("s"), 1 << 30, 64 << 20);
    let real = block_samples(7, 1_000);
    let block = s.append(&real).unwrap();
    let at = 518_398_500u64;
    let mut channels = Vec::new();
    for _ in 0..2 {
        let mut ch = Channel::new();
        // In pieces of at most one block's length, as a document of real audio would have.
        let mut left = at;
        while left > 0 {
            let n = left.min(BLOCK_FRAMES as u64) as u32;
            ch.push(Piece::zeros(n).unwrap());
            left -= u64::from(n);
        }
        ch.push(Piece::whole(block.clone()).unwrap());
        ch.push_zeros(LEN - ch.len());
        channels.push(ch);
    }
    let doc = DocState::new(s.clone(), channels, 48_000, SourceFormat::synthetic(48_000, 2)).unwrap();
    assert_eq!(doc.len_frames(), LEN);
    assert!(doc.piece_count(0) > 7_900);
    let mut out = vec![0.0; 500];
    assert_eq!(doc.read(1, 518_399_000, &mut out).unwrap(), 500);
    assert_eq!(bits(&out), bits(&real[500..]));
    // Across the block's start, and the very end of the document.
    let mut out = vec![1.0; 10];
    doc.read(0, at - 5, &mut out).unwrap();
    assert_eq!(bits(&out[5..]), bits(&real[..5]));
    assert!(out[..5].iter().all(|v| *v == 0.0));
    let mut out = vec![1.0; 10];
    assert_eq!(doc.read(0, LEN - 4, &mut out).unwrap(), 4);
    assert!(out[..4].iter().all(|v| *v == 0.0));
    assert_eq!(doc.disk_bytes_referenced(), 4_000, "one real block, counted once; the zero block is free");
    let (mut l, mut r) = (vec![0.0; 100], vec![0.0; 100]);
    assert_eq!(doc.read_frames(518_399_000, &mut [&mut l, &mut r]).unwrap(), 100);
    assert_eq!(bits(&l), bits(&r));
    assert!(doc.read(2, 0, &mut out).is_err(), "no third channel");
}

/// Rule 2: channels of different lengths are refused.
#[test]
fn channels_of_different_lengths_are_refused() {
    let dir = TempDir::new("len");
    let s = store(&dir.0.join("s"), 1 << 30, 64 << 20);
    let mut a = Channel::new();
    a.push_zeros(10);
    let mut b = Channel::new();
    b.push_zeros(11);
    assert!(DocState::new(s.clone(), vec![a, b], 48_000, SourceFormat::synthetic(48_000, 2)).is_err());
    assert!(DocState::new(s, Vec::new(), 48_000, SourceFormat::synthetic(48_000, 0)).is_err());
}

/// Test 12: the session folder goes when the store drops, and stays with `keep`.
#[test]
fn the_session_folder_is_removed_on_drop_and_kept_with_keep() {
    let root = TempDir::new("t12");
    let s = Store::create_session(Some(&root.0), StoreOptions { pack_bytes: 1 << 20, cache_bytes: 1 << 20, keep: false, max_bytes: None }).unwrap();
    let dir = s.dir().to_path_buf();
    assert!(dir.starts_with(&root.0) && dir.join(LOCK_FILE).exists());
    // A second store cannot take a folder in use.
    assert!(Store::create(&dir).is_err());
    let h = s.append(&[1.0, 2.0]).unwrap();
    assert!(s.pack_path(1).exists());
    drop(s);
    assert!(dir.exists(), "a live block keeps the store, and its folder, alive");
    drop(h);
    assert!(!dir.exists(), "gone with the last handle");

    let s = Store::create_session(Some(&root.0), StoreOptions { pack_bytes: 1 << 20, cache_bytes: 1 << 20, keep: false, max_bytes: None }).unwrap();
    s.set_keep(true);
    let dir = s.dir().to_path_buf();
    let h = s.append(&[1.0, 2.0]).unwrap();
    drop((s, h));
    assert!(dir.join("pack-0001.f32").exists() && dir.join(LOCK_FILE).exists(), "kept as a crash would leave it");
    assert_eq!(std::fs::metadata(dir.join("pack-0001.f32")).unwrap().len(), 8, "flushed on drop");
}

/// The default cache budget: 1/16 of physical memory, 256 MB to 1 GB.
#[test]
fn the_default_cache_budget_is_clamped() {
    let b = wavcraft_core::store::default_cache_bytes();
    assert!((256 << 20..=1 << 30).contains(&b), "{b}");
    assert!(wavcraft_core::store::physical_memory().is_some());
}

/// Random pieces of random blocks, for the property test.
fn arb_layout() -> impl Strategy<Value = Layout> {
    (
        prop::collection::vec(1usize..=3_000, 1..6),
        prop::collection::vec((0usize..6, 0u32..3_000, 1u32..3_000), 1..40),
        prop::collection::vec((0u64..200_000, 0usize..9_000), 1..20),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Test 5: a channel built from random pieces of random blocks (and the zero block) reads
    /// any range exactly as a plain `Vec<f32>` built the same way.
    #[test]
    fn a_channel_of_random_pieces_reads_like_the_reference((sizes, picks, reads) in arb_layout()) {
        let dir = TempDir::new("t5");
        let s = store(&dir.0.join("s"), 3 * BLOCK_BYTES, 4 * 3_000 * 4);
        let blocks: Vec<(Arc<BlockHandle>, Vec<f32>)> = sizes.iter().enumerate().map(|(k, &n)| {
            let samples = block_samples(k + 1, n);
            (s.append(&samples).unwrap(), samples)
        }).collect();
        let mut ch = Channel::new();
        let mut model: Vec<f32> = Vec::new();
        for (b, off, len) in picks {
            if b == 5 {
                ch.push(Piece::zeros(len).unwrap());
                model.extend(std::iter::repeat_n(0.0, len as usize));
                continue;
            }
            let (h, samples) = &blocks[b % blocks.len()];
            let off = off % h.frames();
            let len = len.min(h.frames() - off);
            ch.push(Piece::new(h.clone(), off, len).unwrap());
            model.extend_from_slice(&samples[off as usize..(off + len) as usize]);
        }
        prop_assert_eq!(ch.len(), model.len() as u64);
        s.clear_cache();
        for (start, n) in reads {
            let start = start % (model.len() as u64 + 5);
            let mut out = vec![f32::NAN; n];
            let got = ch.read(&s, start, &mut out).unwrap();
            let from = (start as usize).min(model.len());
            let want = &model[from..(from + n).min(model.len())];
            prop_assert_eq!(got, want.len());
            prop_assert_eq!(bits(&out[..got]), bits(want));
        }
    }
}
