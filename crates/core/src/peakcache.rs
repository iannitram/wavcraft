//! Peak files for a fast reopen (roadmap A075): the waveform overview of a file, kept in a small
//! cache file in the app's cache folder (never beside the user's audio) so reopening a long file
//! does not recompute it.
//!
//! One cache file per source path, named by a hash of the path. It holds the key it was made for
//! (source size, modification time, a hash of the file's first 64 KiB, the number of channels and
//! frames) and a checksum of its payload. A file whose key differs is [`Miss::Stale`]; one that
//! does not parse or fails its checksum is [`Miss::Corrupt`]; both are ignored and replaced by
//! the next [`store`]. [`evict`] keeps the folder under a size cap, oldest first.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::document::WavePeaks;

/// Default cap for the cache folder: 256 MiB.
pub const DEFAULT_CAP_BYTES: u64 = 256 << 20;
/// How much of the source's start is hashed.
pub const HEADER_HASH_BYTES: usize = 64 << 10;

/// Version 04 also stores the source-aligned block runs after a splice.
/// Version 03: the payload is [`WavePeaks`] with level 0 at one entry per `PEAK_BLOCK` (256)
/// samples; 02 had one per 32, so its level 0 is the wrong length. 02 (and 01, which held
/// soundcraft `Peaks`) is a stale miss, rebuilt and replaced by the next store. Other magic is damaged.
const MAGIC: &[u8; 8] = b"WCPEAK04";
/// The start every version of the format shares; a file with it but another version is a miss.
const MAGIC_PREFIX: &[u8] = b"WCPEAK";
const EXT: &str = "peaks";

/// What identifies a source file's peaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub path: PathBuf,
    pub size: u64,
    /// Modification time as nanoseconds since the Unix epoch (0 when unknown).
    pub mtime_ns: u128,
    /// Hash ([`fnv1a`]) of the first [`HEADER_HASH_BYTES`] bytes.
    pub header_hash: u64,
}

impl Key {
    /// The key of the file at `path` as it is now (reads its first 64 KiB).
    pub fn of_file(path: &Path) -> std::io::Result<Key> {
        let meta = fs::metadata(path)?;
        let mtime_ns = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        let mut head = Vec::new();
        File::open(path)?.take(HEADER_HASH_BYTES as u64).read_to_end(&mut head)?;
        Ok(Key { path: path.to_path_buf(), size: meta.len(), mtime_ns, header_hash: fnv1a(&head) })
    }

    /// The cache file name for this source path (stable for the path, not for its contents).
    pub fn file_name(&self) -> String {
        format!("{:016x}.{EXT}", fnv1a(self.path.to_string_lossy().as_bytes()))
    }
}

/// Why a lookup gave no peaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// No cache file for this path.
    Absent,
    /// A cache file made for another version of the source (size, time, header, shape).
    Stale,
    /// A cache file that is damaged or not ours.
    Corrupt,
}

impl Miss {
    pub fn word(self) -> &'static str {
        match self {
            Miss::Absent => "miss",
            Miss::Stale => "stale",
            Miss::Corrupt => "corrupt",
        }
    }
}

/// A fast 64-bit hash (FNV-style, eight bytes at a time): for the header hash, the checksum and
/// the file names. Not cryptographic; it only has to notice a changed or damaged file.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    const P: u64 = 0x0000_0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut rest = bytes;
    while let Some((w, tail)) = rest.split_first_chunk::<8>() {
        h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(P);
        h ^= h >> 29;
        rest = tail;
    }
    for b in rest {
        h = (h ^ u64::from(*b)).wrapping_mul(P);
    }
    (h ^ bytes.len() as u64).wrapping_mul(P)
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn take_u64(b: &[u8], at: &mut usize) -> Option<u64> {
    let v = u64::from_le_bytes(b.get(*at..*at + 8)?.try_into().ok()?);
    *at += 8;
    Some(v)
}

type EncodedParts<'a> = Vec<(usize, &'a [Vec<(f32, f32)>], &'a [(usize, usize, usize)])>;

/// The cache file's bytes for `peaks` (one per channel, each of `frames` samples) under `key`.
fn encode(key: &Key, frames: u64, peaks: &[WavePeaks]) -> Option<Vec<u8>> {
    let parts: EncodedParts<'_> = peaks
        .iter()
        .map(|p| {
            let (len, levels) = p.to_parts();
            (len, levels, p.layout())
        })
        .collect();
    let payload = postcard::to_stdvec(&parts).ok()?;
    let mut out = Vec::with_capacity(payload.len() + 64);
    out.extend_from_slice(MAGIC);
    put_u64(&mut out, key.size);
    out.extend_from_slice(&key.mtime_ns.to_le_bytes());
    put_u64(&mut out, key.header_hash);
    put_u64(&mut out, peaks.len() as u64);
    put_u64(&mut out, frames);
    put_u64(&mut out, payload.len() as u64);
    put_u64(&mut out, fnv1a(&payload));
    out.extend_from_slice(&payload);
    Some(out)
}

type DecodedParts = Vec<(usize, Vec<Vec<(f32, f32)>>, Vec<(usize, usize, usize)>)>;

/// Parse a cache file's bytes against `key`, `channels` and `frames`.
fn decode(bytes: &[u8], key: &Key, channels: usize, frames: u64) -> Result<Vec<WavePeaks>, Miss> {
    if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
        // Another version of this format is not damage: it is a miss, rebuilt by the next store.
        return Err(if bytes.starts_with(MAGIC_PREFIX) { Miss::Stale } else { Miss::Corrupt });
    }
    let mut at = MAGIC.len();
    let size = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let mtime = bytes.get(at..at + 16).and_then(|b| b.try_into().ok()).map(u128::from_le_bytes).ok_or(Miss::Corrupt)?;
    at += 16;
    let header = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let ch = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let fr = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let len = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let sum = take_u64(bytes, &mut at).ok_or(Miss::Corrupt)?;
    let payload = usize::try_from(len).ok().and_then(|n| bytes.get(at..at.checked_add(n)?)).ok_or(Miss::Corrupt)?;
    if fnv1a(payload) != sum {
        return Err(Miss::Corrupt);
    }
    if size != key.size || mtime != key.mtime_ns || header != key.header_hash || ch != channels as u64 || fr != frames {
        return Err(Miss::Stale);
    }
    let parts: DecodedParts = postcard::from_bytes(payload).map_err(|_| Miss::Corrupt)?;
    let peaks: Vec<WavePeaks> =
        parts.into_iter().map(|(len, levels, runs)| WavePeaks::from_layout_parts(len, levels, runs)).collect::<Option<_>>().ok_or(Miss::Corrupt)?;
    if peaks.len() != channels || peaks.iter().any(|p| p.len_samples() as u64 != frames) {
        return Err(Miss::Corrupt);
    }
    Ok(peaks)
}

/// The cached peaks for `key` in `dir`, if there are any for this version of the file with this
/// many channels and frames. A hit refreshes the entry's modification time, so eviction drops the
/// least recently used first.
pub fn load(dir: &Path, key: &Key, channels: usize, frames: u64) -> Result<Vec<WavePeaks>, Miss> {
    let path = dir.join(key.file_name());
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::Absent),
        Err(_) => return Err(Miss::Corrupt),
    };
    let peaks = decode(&bytes, key, channels, frames)?;
    if let Ok(f) = File::options().write(true).open(&path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    Ok(peaks)
}

/// Write the peaks of `key`'s file into `dir` (made if missing), replacing any older entry for
/// the path, then evict down to `cap` bytes. Written to a temporary name and renamed, so a
/// reader never sees half a file. A failure is returned for the log; opening never depends on it.
pub fn store(dir: &Path, key: &Key, frames: u64, peaks: &[WavePeaks], cap: u64) -> std::io::Result<()> {
    let bytes = encode(key, frames, peaks).ok_or_else(|| std::io::Error::other("cannot encode the peaks"))?;
    fs::create_dir_all(dir)?;
    let name = key.file_name();
    let tmp = dir.join(format!("{name}.tmp{}", std::process::id()));
    fs::write(&tmp, &bytes)?;
    if let Err(e) = fs::rename(&tmp, dir.join(&name)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    evict(dir, cap);
    Ok(())
}

/// Cache files in `dir` as `(path, bytes, modified)`, oldest first.
fn entries(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (e.path(), m.len(), m.modified().unwrap_or(UNIX_EPOCH)))
        })
        .collect();
    v.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Delete the oldest cache files until the folder holds at most `cap` bytes. Returns how many
/// went.
pub fn evict(dir: &Path, cap: u64) -> usize {
    let list = entries(dir);
    let mut total: u64 = list.iter().map(|e| e.1).sum();
    let mut gone = 0;
    for (path, bytes, _) in list {
        if total <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= bytes;
            gone += 1;
        }
    }
    gone
}

/// `(files, bytes)` in `dir`.
pub fn info(dir: &Path) -> (usize, u64) {
    let list = entries(dir);
    (list.len(), list.iter().map(|e| e.1).sum())
}

/// Delete every cache file in `dir` (and stray temporary files). Returns how many went.
pub fn clear(dir: &Path) -> usize {
    let mut gone = 0;
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let ours = p.extension().is_some_and(|x| x == EXT) || p.file_name().is_some_and(|n| n.to_string_lossy().contains(&format!(".{EXT}.tmp")));
        if ours && fs::remove_file(&p).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// The app's peak cache folder: `<system cache folder>/wavcraft/peaks`.
pub fn default_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("wavcraft").join("peaks")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-peakcache-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_peaks(frames: usize) -> Vec<WavePeaks> {
        let a: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.01).sin()).collect();
        let b: Vec<f32> = a.iter().map(|v| -v).collect();
        vec![WavePeaks::from_samples(&a), WavePeaks::from_samples(&b)]
    }

    fn key_for(dir: &Path, name: &str, bytes: &[u8]) -> Key {
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        Key::of_file(&p).unwrap()
    }

    #[test]
    fn the_key_follows_size_time_and_header() {
        let dir = scratch("key");
        let k1 = key_for(&dir, "a.wav", b"RIFF-one");
        assert_eq!(Key::of_file(&dir.join("a.wav")).unwrap(), k1, "same file, same key");
        let k2 = key_for(&dir, "a.wav", b"RIFF-two!");
        assert_ne!(k1.size, k2.size);
        assert_ne!(k1.header_hash, k2.header_hash);
        assert_eq!(k1.file_name(), k2.file_name(), "the cache file is named by the path");
        let other = key_for(&dir, "b.wav", b"RIFF-one");
        assert_ne!(k1.file_name(), other.file_name());
        assert!(k1.file_name().ends_with(".peaks"));
        // A changed time alone changes the key.
        let f = File::options().write(true).open(dir.join("a.wav")).unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000)).unwrap();
        drop(f);
        assert_ne!(Key::of_file(&dir.join("a.wav")).unwrap().mtime_ns, k2.mtime_ns);
        assert!(Key::of_file(&dir.join("missing.wav")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn peaks_round_trip_exactly() {
        let dir = scratch("round");
        let cache = dir.join("cache");
        let key = key_for(&dir, "a.wav", b"RIFF-x");
        let peaks = sample_peaks(10_000);
        assert_eq!(load(&cache, &key, 2, 10_000).unwrap_err(), Miss::Absent);
        store(&cache, &key, 10_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
        assert_eq!(load(&cache, &key, 2, 10_000).unwrap(), peaks);
        assert!(cache.read_dir().unwrap().flatten().all(|e| e.path().extension().is_some_and(|x| x == EXT)), "no temporary files left");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_source_is_stale_and_is_replaced() {
        let dir = scratch("stale");
        let cache = dir.join("cache");
        let key = key_for(&dir, "a.wav", b"RIFF-x");
        let peaks = sample_peaks(5_000);
        store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
        // Size, header, time, channel count and length each make the entry stale.
        let mut k = key.clone();
        k.size += 1;
        assert_eq!(load(&cache, &k, 2, 5_000).unwrap_err(), Miss::Stale);
        let mut k = key.clone();
        k.header_hash ^= 1;
        assert_eq!(load(&cache, &k, 2, 5_000).unwrap_err(), Miss::Stale);
        let mut k = key.clone();
        k.mtime_ns += 1;
        assert_eq!(load(&cache, &k, 2, 5_000).unwrap_err(), Miss::Stale);
        assert_eq!(load(&cache, &key, 1, 5_000).unwrap_err(), Miss::Stale);
        assert_eq!(load(&cache, &key, 2, 5_001).unwrap_err(), Miss::Stale);
        // Storing for the new version replaces the old entry: one file, and the old key now misses.
        let mut newer = key.clone();
        newer.mtime_ns += 1;
        let peaks2 = sample_peaks(6_000);
        store(&cache, &newer, 6_000, &peaks2, DEFAULT_CAP_BYTES).unwrap();
        assert_eq!(info(&cache).0, 1);
        assert_eq!(load(&cache, &newer, 2, 6_000).unwrap(), peaks2);
        assert_eq!(load(&cache, &key, 2, 5_000).unwrap_err(), Miss::Stale);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_cache_file_is_ignored_and_replaced() {
        let dir = scratch("corrupt");
        let cache = dir.join("cache");
        let key = key_for(&dir, "a.wav", b"RIFF-x");
        let peaks = sample_peaks(5_000);
        store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
        let file = cache.join(key.file_name());
        let good = fs::read(&file).unwrap();
        // Garbage, a truncation, a flipped payload byte, an empty file, a wrong magic.
        let mut flipped = good.clone();
        let last = flipped.len() - 5;
        flipped[last] ^= 0xff;
        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        for bad in [b"not a cache file at all".to_vec(), good[..good.len() / 2].to_vec(), flipped, Vec::new(), wrong_magic, good[..20].to_vec()] {
            fs::write(&file, &bad).unwrap();
            assert_eq!(load(&cache, &key, 2, 5_000).unwrap_err(), Miss::Corrupt, "{} bytes", bad.len());
        }
        // A directory where the file should be is also not a crash.
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(load(&cache, &key, 2, 5_000).is_err());
        assert!(store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).is_err(), "cannot replace a folder; the error is returned, not a panic");
        fs::remove_dir(&file).unwrap();
        store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
        assert_eq!(load(&cache, &key, 2, 5_000).unwrap(), peaks);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_oldest_entries_go_first_when_over_the_cap() {
        let dir = scratch("evict");
        let cache = dir.join("cache");
        let peaks = sample_peaks(20_000);
        let mut keys = Vec::new();
        for i in 0..4u64 {
            let key = key_for(&dir, &format!("f{i}.wav"), format!("RIFF-{i}").as_bytes());
            store(&cache, &key, 20_000, &peaks, u64::MAX).unwrap();
            let f = File::options().write(true).open(cache.join(key.file_name())).unwrap();
            f.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000 + i * 100)).unwrap();
            keys.push(key);
        }
        let (n, bytes) = info(&cache);
        assert_eq!(n, 4);
        let one = bytes / 4;
        // A hit makes entry 0 the newest, so entries 1 and 2 are the oldest.
        load(&cache, &keys[0], 2, 20_000).unwrap();
        let gone = evict(&cache, one * 2 + one / 2);
        assert_eq!(gone, 2);
        assert!(load(&cache, &keys[0], 2, 20_000).is_ok());
        assert!(load(&cache, &keys[3], 2, 20_000).is_ok());
        assert_eq!(load(&cache, &keys[1], 2, 20_000).unwrap_err(), Miss::Absent);
        assert_eq!(load(&cache, &keys[2], 2, 20_000).unwrap_err(), Miss::Absent);
        // The cap applies on every store, too.
        let key = key_for(&dir, "f9.wav", b"RIFF-9");
        store(&cache, &key, 20_000, &peaks, one).unwrap();
        assert!(info(&cache).1 <= one, "{:?}", info(&cache));
        let n = info(&cache).0;
        assert_eq!(clear(&cache), n);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file in an older version's format (its own magic, otherwise intact) is a stale miss, not
    /// damage, and the next store replaces it.
    #[test]
    fn an_old_version_file_is_a_miss_and_is_rebuilt() {
        let dir = scratch("old-version");
        let cache = dir.join("cache");
        let key = key_for(&dir, "a.wav", b"RIFF-x");
        let peaks = sample_peaks(5_000);
        store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
        let file = cache.join(key.file_name());
        let current = fs::read(&file).unwrap();
        for old in [b"WCPEAK03", b"WCPEAK02", b"WCPEAK01"] {
            let mut bytes = current.clone();
            bytes[..MAGIC.len()].copy_from_slice(old);
            fs::write(&file, &bytes).unwrap();
            assert_eq!(load(&cache, &key, 2, 5_000).unwrap_err(), Miss::Stale, "{}", String::from_utf8_lossy(old));
            store(&cache, &key, 5_000, &peaks, DEFAULT_CAP_BYTES).unwrap();
            assert_eq!(load(&cache, &key, 2, 5_000).unwrap(), peaks);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_removes_only_cache_files() {
        let dir = scratch("clear");
        let cache = dir.join("cache");
        let key = key_for(&dir, "a.wav", b"RIFF-x");
        store(&cache, &key, 100, &[WavePeaks::from_samples(&[0.5; 100])], DEFAULT_CAP_BYTES).unwrap();
        fs::write(cache.join("notes.txt"), "keep").unwrap();
        assert_eq!(clear(&cache), 1);
        assert_eq!(info(&cache), (0, 0));
        assert!(cache.join("notes.txt").exists());
        assert_eq!(clear(&dir.join("nowhere")), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
