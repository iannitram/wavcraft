//! The on-disk sample store: where every sample of an open document lives.
//!
//! Samples are written once, as little-endian `f32`, into append-only **pack files**
//! (`pack-0001.f32`, `pack-0002.f32`, …) in a **session folder**. A **block** is a run of up to
//! [`BLOCK_FRAMES`] samples of one channel, at a fixed place in one pack, and it never changes
//! after [`Store::append`] returns. RAM holds only a **block cache** with a byte budget: a read
//! that misses it reads the whole block with one positioned read (no `mmap`; the workspace forbids
//! `unsafe`).
//!
//! Blocks are handed out as [`Arc<BlockHandle>`]. While a handle lives its block is **live**, and
//! each pack counts its live blocks. When a pack that is no longer being written (a *sealed*
//! pack) has no live blocks left, its file is deleted. Dropping the last handle and the last
//! [`Store`] removes the session folder, unless [`Store::set_keep`] asked to keep it.
//!
//! The **zero block** ([`BlockRef::ZERO`], [`Store::zero`]) is never on disk: a read of it fills
//! zeros, and it is as long as any piece needs (up to `u32::MAX` frames per piece).
//!
//! A `Store` is a cheap handle (`Clone` shares it) and is `Send + Sync`: any number of threads
//! may read while one appends.

use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// The most frames one block holds (256 KiB of `f32`).
pub const BLOCK_FRAMES: usize = 65_536;
/// Packs roll over to a new file once they would pass this size.
pub const DEFAULT_PACK_BYTES: u64 = 1 << 30;
/// The smallest and largest default cache budgets.
pub const MIN_CACHE_BYTES: u64 = 256 << 20;
pub const MAX_CACHE_BYTES: u64 = 1 << 30;
/// The lock file inside a session folder that is in use: who made it (`pid=`, `host=`,
/// `machine=`, `app=`), plain text anyone can read.
pub const LOCK_FILE: &str = "session.lock";

/// The file a running store keeps open with an operating-system lock for its whole life (Codex
/// review CX-W3, 2026-10-19). The system drops the lock when the program ends, however it ends,
/// so a session whose `session.held` can be locked has no program left. Kept apart from
/// [`LOCK_FILE`] because on Windows a locked file cannot be read by others.
pub const HELD_FILE: &str = "session.held";

const BYTES_PER_SAMPLE: u64 = 4;

/// A block's identity: unique within one store, never reused. `BlockId(0)` is the zero block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

/// Where a block's samples are: which pack, at which byte, how many frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockRef {
    pub id: BlockId,
    pub pack: u32,
    pub byte_offset: u64,
    pub frames: u32,
}

impl BlockRef {
    /// The virtual block of silence: never on disk, as long as any piece of it needs to be.
    pub const ZERO: BlockRef = BlockRef { id: BlockId(0), pack: 0, byte_offset: 0, frames: u32::MAX };

    pub fn is_zero(&self) -> bool {
        self.id == BlockId(0)
    }

    /// Bytes this block takes on disk (0 for the zero block).
    pub fn disk_bytes(&self) -> u64 {
        if self.is_zero() { 0 } else { u64::from(self.frames) * BYTES_PER_SAMPLE }
    }
}

/// A live block. Its pack counts it as live until the last `Arc` of it drops.
pub struct BlockHandle {
    block: BlockRef,
    /// `None` for the zero block, which belongs to no store.
    store: Option<Store>,
}

impl BlockHandle {
    pub fn block(&self) -> &BlockRef {
        &self.block
    }

    pub fn id(&self) -> BlockId {
        self.block.id
    }

    pub fn frames(&self) -> u32 {
        self.block.frames
    }

    pub fn is_zero(&self) -> bool {
        self.block.is_zero()
    }

    /// Whether this block lives in `store` (the zero block belongs to every store).
    pub fn belongs_to(&self, store: &Store) -> bool {
        self.store.as_ref().is_none_or(|s| s.same_store(store))
    }
}

impl std::fmt::Debug for BlockHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BlockHandle").field(&self.block).finish()
    }
}

impl Drop for BlockHandle {
    fn drop(&mut self) {
        if let Some(store) = &self.store {
            store.release(&self.block);
        }
    }
}

static ZERO_HANDLE: LazyLock<Arc<BlockHandle>> = LazyLock::new(|| Arc::new(BlockHandle { block: BlockRef::ZERO, store: None }));

/// How a store is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    /// Roll over to a new pack before a pack passes this many bytes (tests set it small).
    pub pack_bytes: u64,
    /// The block cache's budget in bytes.
    pub cache_bytes: u64,
    /// Leave the session folder on disk when the store is dropped.
    pub keep: bool,
    /// Refuse an append that would take the bytes written past this, with the error a full
    /// disk gives (`StorageFull`). `None`: no limit but the disk. Tests use it to act out a disk
    /// filling up.
    pub max_bytes: Option<u64>,
}

impl Default for StoreOptions {
    fn default() -> Self {
        StoreOptions { pack_bytes: DEFAULT_PACK_BYTES, cache_bytes: default_cache_bytes(), keep: false, max_bytes: None }
    }
}

/// This machine's physical memory in bytes, when the system says.
pub fn physical_memory() -> Option<u64> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    Some(sys.total_memory()).filter(|m| *m > 0)
}

/// The default cache budget: 1/16 of physical memory, at least 256 MB and at most 1 GB
/// (512 MB on an 8 GB machine). 256 MB when the memory size is unknown.
pub fn default_cache_bytes() -> u64 {
    physical_memory().map_or(MIN_CACHE_BYTES, |m| (m / 16).clamp(MIN_CACHE_BYTES, MAX_CACHE_BYTES))
}

/// A session folder this sweep may remove: named as [`Store::create_session`] names them (16 hex
/// digits), holding WavCraft's [`LOCK_FILE`] whose program is no longer running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleSession {
    pub dir: PathBuf,
    pub pid: u32,
    pub bytes: u64,
}

/// This machine's name, as session lock files record it (empty when the system won't say).
pub fn this_host() -> String {
    sysinfo::System::host_name().unwrap_or_default()
}

/// This machine's id for session locks: 32 random hex digits made once and kept in the local
/// (not roaming) data folder, `wavcraft/machine-id`. Two machines can share a hostname, never
/// this (Codex review CX-W3). Empty when it can't be read or made: then nothing is swept.
pub fn machine_id() -> String {
    static ID: LazyLock<String> = LazyLock::new(|| {
        let Some(dir) = dirs::data_local_dir().map(|d| d.join("wavcraft")) else { return String::new() };
        let path = dir.join("machine-id");
        let valid = |s: &str| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit());
        if let Ok(s) = std::fs::read_to_string(&path)
            && valid(s.trim())
        {
            return s.trim().to_string();
        }
        let Ok(id) = crate::random::hex(16) else { return String::new() };
        let made = std::fs::create_dir_all(&dir).and_then(|()| {
            let mut f = OpenOptions::new().write(true).create_new(true).open(&path)?;
            f.write_all(id.as_bytes())
        });
        match made {
            Ok(()) => id,
            // Another run made it first: use theirs.
            Err(_) => std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| valid(s)).unwrap_or_default(),
        }
    });
    ID.clone()
}

/// The stale session folders under `root`: a run that ended without closing its store (killed,
/// crashed, or quit by a signal) leaves its folder, about 1.3 GB per hour of stereo audio, and
/// nothing else ever removes it. A folder counts as stale only when all of these hold, and
/// anything uncertain stays (Codex review CX-W3, 2026-10-19):
/// - its name is a session id (16 hex digits) and it is a folder;
/// - its [`LOCK_FILE`] says `app=wavcraft` and `machine=` **this machine's id** ([`machine_id`]):
///   a folder on a shared disk may be another computer's, even one with the same hostname;
/// - its [`HELD_FILE`] exists and this sweep can take its operating-system lock: no running
///   program holds it, whatever its pid or its program's name. A session from before
///   `session.held` (no machine id either) is never swept.
pub fn stale_sessions(root: &Path) -> Vec<StaleSession> {
    let machine = machine_id();
    if machine.is_empty() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let dir = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()) && dir.is_dir()) {
            continue;
        }
        let Ok(lock) = std::fs::read_to_string(dir.join(LOCK_FILE)) else { continue };
        let ours = lock.lines().any(|l| l.starts_with("app=wavcraft")) && lock.lines().any(|l| l.strip_prefix("machine=") == Some(machine.as_str()));
        if !ours {
            continue;
        }
        let pid = lock.lines().find_map(|l| l.strip_prefix("pid=")).and_then(|p| p.trim().parse::<u32>().ok()).unwrap_or(0);
        let Ok(held) = OpenOptions::new().read(true).write(true).open(dir.join(HELD_FILE)) else { continue };
        if held.try_lock().is_err() {
            continue; // a running program holds it
        }
        drop(held);
        let bytes = std::fs::read_dir(&dir).map_or(0, |d| d.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum());
        out.push(StaleSession { dir, pid, bytes });
    }
    out
}

/// Remove the [`stale_sessions`] under `root` (the default session root when `None`), logging
/// each one once. Returns the bytes freed.
pub fn sweep_stale_sessions(root: Option<&Path>) -> u64 {
    let root = root.map_or_else(default_sessions_root, Path::to_path_buf);
    let mut freed = 0;
    for s in stale_sessions(&root) {
        match std::fs::remove_dir_all(&s.dir) {
            Ok(()) => {
                freed += s.bytes;
                log::warn!(
                    "decision: sessions: removed a session folder left by a run that ended without closing it (pid {}, its lock released): {} MB in {}",
                    s.pid,
                    s.bytes / 1_000_000,
                    s.dir.display()
                );
            }
            Err(e) => log::warn!("decision: sessions: could not remove the stale session folder {}: {e}", s.dir.display()),
        }
    }
    freed
}

/// Where session folders go by default: `<system cache dir>/wavcraft/sessions`.
pub fn default_sessions_root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("wavcraft").join("sessions")
}

/// What the block cache holds and has held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub budget_bytes: u64,
    pub blocks: usize,
    pub bytes: u64,
    /// The most it has held at once since it was made or last cleared.
    pub peak_blocks: usize,
    pub peak_bytes: u64,
    pub hits: u64,
    pub misses: u64,
}

/// The shared store. `Clone` is cheap and shares it.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("dir", &self.inner.dir).finish_non_exhaustive()
    }
}

type Samples = Arc<Vec<f32>>;

struct Inner {
    dir: PathBuf,
    /// [`HELD_FILE`], locked while the store lives.
    held: Mutex<Option<File>>,
    keep: AtomicBool,
    pack_bytes: u64,
    next_id: AtomicU64,
    /// The pack being written, readable without the writer's lock.
    writing_pack: AtomicU32,
    writer: Mutex<Writer>,
    packs: Mutex<HashMap<u32, Pack>>,
    cache: Mutex<Cache>,
    live_bytes: AtomicU64,
    max_bytes: Option<u64>,
    /// Bytes appended since the store was made (never goes down).
    written: AtomicU64,
}

struct Writer {
    /// The pack being written (0 before the first append).
    pack: u32,
    out: Option<BufWriter<File>>,
    /// Bytes in the pack, buffered ones included.
    len: u64,
    /// Bytes of the pack that have reached the OS.
    flushed: u64,
}

struct Pack {
    live: u64,
    sealed: bool,
    len: u64,
    /// Opened on the first read.
    file: Option<Arc<File>>,
}

#[cfg(test)]
static CREATE_HOOK: Mutex<Option<Box<dyn Fn(&Path) + Send>>> = Mutex::new(None);

impl Store {
    /// A store in `dir` (made if missing) with the default options.
    pub fn create(dir: &Path) -> io::Result<Store> {
        Store::create_with(dir, StoreOptions::default())
    }

    /// A store in `dir` (made if missing). Fails if `dir` already holds a lock file: another
    /// store is using it, or one ended without cleaning up.
    ///
    /// The order inside matters: [`HELD_FILE`] is created and locked before [`LOCK_FILE`] is
    /// written. [`stale_sessions`] skips folders without a readable lock file, so creating and
    /// locking `HELD_FILE` first ensures a concurrent sweep never mistakes a session mid-creation
    /// for a stale one.
    pub fn create_with(dir: &Path, opts: StoreOptions) -> io::Result<Store> {
        std::fs::create_dir_all(dir)?;
        // Held for the store's whole life; the system releases it when the program ends.
        let held = OpenOptions::new().read(true).write(true).create_new(true).open(dir.join(HELD_FILE))?;
        #[cfg(test)]
        if let Some(hook) = lock(&CREATE_HOOK).as_ref() {
            hook(dir);
        }
        held.try_lock().map_err(|e| io::Error::other(format!("could not lock the session: {e}")))?;
        let mut lock_file = OpenOptions::new().write(true).create_new(true).open(dir.join(LOCK_FILE))?;
        writeln!(
            lock_file,
            "pid={}\nhost={}\nmachine={}\napp=wavcraft {}",
            std::process::id(),
            this_host(),
            machine_id(),
            env!("CARGO_PKG_VERSION")
        )?;
        Ok(Store {
            inner: Arc::new(Inner {
                dir: dir.to_path_buf(),
                held: Mutex::new(Some(held)),
                keep: AtomicBool::new(opts.keep),
                pack_bytes: opts.pack_bytes.max(BLOCK_FRAMES as u64 * BYTES_PER_SAMPLE),
                next_id: AtomicU64::new(1),
                writing_pack: AtomicU32::new(0),
                writer: Mutex::new(Writer { pack: 0, out: None, len: 0, flushed: 0 }),
                packs: Mutex::new(HashMap::new()),
                cache: Mutex::new(Cache::new(opts.cache_bytes)),
                live_bytes: AtomicU64::new(0),
                max_bytes: opts.max_bytes,
                written: AtomicU64::new(0),
            }),
        })
    }

    /// A store in a new session folder with a random name under `root` (the system cache
    /// folder's `wavcraft/sessions` when `None`).
    pub fn create_session(root: Option<&Path>, opts: StoreOptions) -> io::Result<Store> {
        let root = root.map_or_else(default_sessions_root, Path::to_path_buf);
        std::fs::create_dir_all(&root)?;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let id = session_id(COUNTER.fetch_add(1, Ordering::Relaxed));
            let dir = root.join(id);
            match std::fs::create_dir(&dir) {
                Ok(()) => return Store::create_with(&dir, opts),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("could not find a free session folder name"))
    }

    /// Whether `other` is a handle to this same store.
    pub fn same_store(&self, other: &Store) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// The session folder.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// Keep the session folder (and its lock file, as a crash would leave it) when the store is
    /// dropped, or not.
    pub fn set_keep(&self, keep: bool) {
        self.inner.keep.store(keep, Ordering::Relaxed);
    }

    /// The zero block's handle (shared; it counts against no pack).
    pub fn zero() -> Arc<BlockHandle> {
        ZERO_HANDLE.clone()
    }

    /// Write `samples` (1 to [`BLOCK_FRAMES`] of one channel) as a new block. It can be read as
    /// soon as this returns.
    pub fn append(&self, samples: &[f32]) -> io::Result<Arc<BlockHandle>> {
        if samples.is_empty() || samples.len() > BLOCK_FRAMES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("a block holds 1 to {BLOCK_FRAMES} frames, not {}", samples.len())));
        }
        let frames = samples.len() as u32;
        let bytes = u64::from(frames) * BYTES_PER_SAMPLE;
        let inner = &self.inner;
        let mut w = lock(&inner.writer);
        if inner.max_bytes.is_some_and(|max| inner.written.load(Ordering::Relaxed) + bytes > max) {
            return Err(io::Error::new(io::ErrorKind::StorageFull, "the session store reached its size limit"));
        }
        if w.out.is_none() || w.len + bytes > inner.pack_bytes {
            self.roll_over(&mut w)?;
        }
        let pack = w.pack;
        let byte_offset = w.len;
        let result = match w.out.as_mut() {
            Some(out) => write_samples(out, samples),
            None => Err(io::Error::other("no pack open for writing")),
        };
        if let Err(e) = result {
            // The pack's tail is unknown now: start a fresh pack next time.
            w.out = None;
            return Err(e);
        }
        w.len += bytes;
        inner.written.fetch_add(bytes, Ordering::Relaxed);
        let id = BlockId(inner.next_id.fetch_add(1, Ordering::Relaxed));
        {
            let mut packs = lock(&inner.packs);
            if let Some(p) = packs.get_mut(&pack) {
                p.live += 1;
                p.len = w.len;
            }
        }
        drop(w);
        inner.live_bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(Arc::new(BlockHandle { block: BlockRef { id, pack, byte_offset, frames }, store: Some(self.clone()) }))
    }

    /// Seal the pack being written and start the next one.
    fn roll_over(&self, w: &mut Writer) -> io::Result<()> {
        if let Some(mut out) = w.out.take() {
            // Flush errors are reported, but the pack is sealed either way.
            let flushed = out.flush();
            w.flushed = w.len;
            drop(out);
            self.seal(w.pack);
            flushed?;
        } else if w.pack != 0 {
            self.seal(w.pack);
        }
        let next = w.pack + 1;
        let file = OpenOptions::new().write(true).create_new(true).open(self.pack_path(next))?;
        lock(&self.inner.packs).insert(next, Pack { live: 0, sealed: false, len: 0, file: None });
        w.pack = next;
        w.out = Some(BufWriter::with_capacity(1 << 20, file));
        w.len = 0;
        w.flushed = 0;
        self.inner.writing_pack.store(next, Ordering::SeqCst);
        Ok(())
    }

    /// Mark `pack` as no longer written; delete it now if nothing in it is live.
    fn seal(&self, pack: u32) {
        let dead = {
            let mut packs = lock(&self.inner.packs);
            match packs.get_mut(&pack) {
                Some(p) if p.live == 0 => packs.remove(&pack),
                Some(p) => {
                    p.sealed = true;
                    None
                }
                None => None,
            }
        };
        if let Some(p) = dead {
            drop(p.file);
            self.delete_pack(pack);
        }
    }

    fn delete_pack(&self, pack: u32) {
        if let Err(e) = std::fs::remove_file(self.pack_path(pack)) {
            log::warn!("could not delete {}: {e}", self.pack_path(pack).display());
        }
    }

    /// The file a pack lives in.
    pub fn pack_path(&self, pack: u32) -> PathBuf {
        self.inner.dir.join(format!("pack-{pack:04}.f32"))
    }

    /// A block's handle dropped.
    fn release(&self, block: &BlockRef) {
        let inner = &self.inner;
        inner.live_bytes.fetch_sub(block.disk_bytes(), Ordering::Relaxed);
        lock(&inner.cache).remove(block.id);
        let dead = {
            let mut packs = lock(&inner.packs);
            match packs.get_mut(&block.pack) {
                Some(p) => {
                    p.live = p.live.saturating_sub(1);
                    if p.live == 0 && p.sealed { packs.remove(&block.pack) } else { None }
                }
                None => None,
            }
        };
        if let Some(p) = dead {
            drop(p.file);
            self.delete_pack(block.pack);
        }
    }

    /// Copy `out.len()` samples of `block`, starting `offset` frames in, into `out`.
    pub fn read(&self, block: &BlockRef, offset: u32, out: &mut [f32]) -> io::Result<()> {
        let start = offset as usize;
        let end = start.checked_add(out.len()).filter(|e| *e <= block.frames as usize).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("{} frames at {offset} run past a block of {}", out.len(), block.frames))
        })?;
        if block.is_zero() {
            out.fill(0.0);
            return Ok(());
        }
        let samples = self.samples(block)?;
        let src = samples.get(start..end).ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "block shorter than its handle says"))?;
        out.copy_from_slice(src);
        Ok(())
    }

    /// The whole block, from the cache or from disk.
    fn samples(&self, block: &BlockRef) -> io::Result<Samples> {
        let inner = &self.inner;
        if let Some(s) = lock(&inner.cache).get(block.id) {
            return Ok(s);
        }
        let end = block.byte_offset + block.disk_bytes();
        // A block still in the writer's buffer: push the buffer to the OS first.
        if block.pack == inner.writing_pack.load(Ordering::SeqCst) {
            let mut w = lock(&inner.writer);
            if w.pack == block.pack && w.flushed < end {
                if let Some(out) = w.out.as_mut() {
                    out.flush()?;
                }
                w.flushed = w.len;
            }
        }
        let file = self.pack_file(block.pack)?;
        // A buffer an evicted block left behind, when there is one: no allocation, no zeroing.
        let mut samples = lock(&inner.cache).spare.pop().unwrap_or_default();
        samples.resize(block.frames as usize, 0.0);
        read_exact_at(&file, bytemuck::cast_slice_mut(&mut samples), block.byte_offset)?;
        #[cfg(target_endian = "big")]
        for x in samples.iter_mut() {
            *x = f32::from_bits(u32::from_le(x.to_bits()));
        }
        let samples = Arc::new(samples);
        lock(&inner.cache).insert(block.id, samples.clone());
        Ok(samples)
    }

    fn pack_file(&self, pack: u32) -> io::Result<Arc<File>> {
        let mut packs = lock(&self.inner.packs);
        let p = packs.get_mut(&pack).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("pack {pack} is gone")))?;
        if let Some(f) = &p.file {
            return Ok(f.clone());
        }
        let f = Arc::new(File::open(self.pack_path(pack))?);
        p.file = Some(f.clone());
        Ok(f)
    }

    /// Bytes in pack files on disk now (live and dead blocks, buffered writes included).
    pub fn bytes_on_disk(&self) -> u64 {
        lock(&self.inner.packs).values().map(|p| p.len).sum()
    }

    /// Bytes of the blocks that are live (some handle holds them).
    pub fn bytes_live(&self) -> u64 {
        self.inner.live_bytes.load(Ordering::Relaxed)
    }

    /// The packs that exist now, in order.
    pub fn packs(&self) -> Vec<u32> {
        let mut p: Vec<u32> = lock(&self.inner.packs).keys().copied().collect();
        p.sort_unstable();
        p
    }

    /// Push buffered writes to the OS (not to the disk: no fsync).
    pub fn flush(&self) -> io::Result<()> {
        let mut w = lock(&self.inner.writer);
        if let Some(out) = w.out.as_mut() {
            out.flush()?;
        }
        w.flushed = w.len;
        Ok(())
    }

    pub fn cache_stats(&self) -> CacheStats {
        lock(&self.inner.cache).stats()
    }

    /// Empty the block cache and reset its counters (for cold-read tests and measurements).
    pub fn clear_cache(&self) {
        lock(&self.inner.cache).clear();
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Close every file before the folder goes (Windows will not remove open files).
        if let Ok(w) = self.writer.get_mut() {
            w.out = None;
        }
        if let Ok(p) = self.packs.get_mut() {
            p.clear();
        }
        if let Ok(h) = self.held.get_mut() {
            *h = None;
        }
        if !self.keep.load(Ordering::Relaxed)
            && let Err(e) = std::fs::remove_dir_all(&self.dir)
        {
            log::warn!("could not remove the session folder {}: {e}", self.dir.display());
        }
    }
}

/// Lock a mutex, carrying on if another thread panicked while holding it (the data stays
/// consistent: every critical section here is a few plain assignments).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_samples(out: &mut BufWriter<File>, samples: &[f32]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        out.write_all(bytemuck::cast_slice(samples))
    }
    #[cfg(target_endian = "big")]
    {
        let bytes: Vec<u8> = samples.iter().flat_map(|x| x.to_le_bytes()).collect();
        out.write_all(&bytes)
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "pack file shorter than the block")),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A folder name nobody else is likely to pick: time, process and a counter, mixed.
fn session_id(counter: u64) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let mut z = nanos ^ (u64::from(std::process::id()) << 32) ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    format!("{:016x}", z ^ (z >> 31))
}

/// An LRU of whole blocks with a byte budget.
struct Cache {
    budget: u64,
    bytes: u64,
    tick: u64,
    map: HashMap<BlockId, (Samples, u64)>,
    order: BTreeMap<u64, BlockId>,
    peak_blocks: usize,
    peak_bytes: u64,
    hits: u64,
    misses: u64,
    /// Buffers of evicted blocks nobody else held, for the next miss to reuse.
    spare: Vec<Vec<f32>>,
}

/// How many evicted buffers are kept for reuse (1 MiB of them).
const SPARE_BUFFERS: usize = 4;

impl Cache {
    fn new(budget: u64) -> Cache {
        Cache {
            budget,
            bytes: 0,
            tick: 0,
            map: HashMap::new(),
            order: BTreeMap::new(),
            peak_blocks: 0,
            peak_bytes: 0,
            hits: 0,
            misses: 0,
            spare: Vec::new(),
        }
    }

    fn get(&mut self, id: BlockId) -> Option<Samples> {
        self.tick += 1;
        let tick = self.tick;
        match self.map.get_mut(&id) {
            Some((s, stamp)) => {
                self.order.remove(stamp);
                *stamp = tick;
                self.order.insert(tick, id);
                self.hits += 1;
                Some(s.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    fn insert(&mut self, id: BlockId, samples: Samples) {
        let size = samples.len() as u64 * BYTES_PER_SAMPLE;
        if size > self.budget || self.map.contains_key(&id) {
            return;
        }
        while self.bytes + size > self.budget {
            let Some((_, oldest)) = self.order.pop_first() else { break };
            self.remove_entry(oldest);
        }
        self.tick += 1;
        self.order.insert(self.tick, id);
        self.map.insert(id, (samples, self.tick));
        self.bytes += size;
        self.peak_blocks = self.peak_blocks.max(self.map.len());
        self.peak_bytes = self.peak_bytes.max(self.bytes);
    }

    fn remove(&mut self, id: BlockId) {
        if let Some((_, stamp)) = self.map.get(&id) {
            let stamp = *stamp;
            self.order.remove(&stamp);
            self.remove_entry(id);
        }
    }

    fn remove_entry(&mut self, id: BlockId) {
        if let Some((s, _)) = self.map.remove(&id) {
            self.bytes = self.bytes.saturating_sub(s.len() as u64 * BYTES_PER_SAMPLE);
            if self.spare.len() < SPARE_BUFFERS
                && let Ok(v) = Arc::try_unwrap(s)
            {
                self.spare.push(v);
            }
        }
    }

    fn clear(&mut self) {
        *self = Cache::new(self.budget);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            budget_bytes: self.budget,
            blocks: self.map.len(),
            bytes: self.bytes,
            peak_blocks: self.peak_blocks,
            peak_bytes: self.peak_bytes,
            hits: self.hits,
            misses: self.misses,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("wavcraft-store-test-{}-{name}-{id}", std::process::id()));
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

    #[test]
    fn sweep_between_held_creation_and_lock_keeps_new_session() {
        let _guard = lock(&TEST_LOCK);
        let temp = TempDir::new("sweep-held");
        let root = temp.0.clone();

        *lock(&CREATE_HOOK) = Some(Box::new(|session_dir| {
            if let Some(root) = session_dir.parent() {
                sweep_stale_sessions(Some(root));
            }
        }));

        let res = Store::create_session(Some(&root), StoreOptions::default());
        assert!(res.is_ok());
        let store = res.unwrap();
        let dir = store.dir();
        assert!(dir.exists());
        assert!(dir.join(HELD_FILE).exists());
        assert!(dir.join(LOCK_FILE).exists());
        *lock(&CREATE_HOOK) = None;
    }
}
