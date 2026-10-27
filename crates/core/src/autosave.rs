//! Auto Save and crash recovery (roadmap A239).
//!
//! The session store already keeps every sample on disk in append-only packs; what a crash loses
//! is the *piece table* and the *undo history* that say which parts of which blocks make the
//! document. This module writes them to a **journal** (`journal.wcj`) in the session folder and
//! reads them back after a crash.
//!
//! **Writing** ([`write_journal`]; [`Autosaver`] does it on a worker thread, newest snapshot
//! only):
//! 1. [`Store::sync`] makes every block the journal will name durable (flush, fsync);
//! 2. the journal is written to `journal.wcj.tmp` (owner-only on Unix), fsynced, renamed over
//!    `journal.wcj`, and the folder fsynced. So the journal on disk is always a complete one
//!    that names only durable blocks; a crash mid-write leaves the previous one.
//! 3. The [`Autosaver`] **pins** the blocks of the journal last made durable (it holds their
//!    handles), so the store cannot delete a pack the journal on disk still needs, even after the
//!    history has dropped those steps and before the next journal is durable.
//!
//! **Reading** ([`recoverable_sessions`], [`recover`]): a stale session folder (its program is
//! gone: [`crate::store::stale_sessions`]'s checks) that holds a journal is *recoverable*. The
//! stale-session sweep never removes such a folder, damaged journal or not; only
//! [`discard`], which the owner asks for, does. [`recover`] takes the folder's lock (a running
//! program's session is refused), adopts the packs, checks every block against its pack, and
//! rebuilds the states and labels. If anything fails, every file is left as found.
//!
//! The journal is plain text (see `encode`), versioned, with an FNV-1a checksum of everything
//! before its last line, so a damaged one is refused rather than half-read.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use soundcraft_audio_io::{BwfInfo, FileFormat, SampleFormat};

use crate::doc::history::History;
use crate::doc::{Channel, ChunkInfo, DocState, Piece, SourceFormat};
use crate::store::{BlockHandle, BlockId, BlockRef, Encoding, HELD_FILE, LOCK_FILE, Store, StoreOptions, machine_id, stale_candidates};

/// The journal in a session folder.
pub const JOURNAL_FILE: &str = "journal.wcj";
/// Where a journal is written before it is renamed over [`JOURNAL_FILE`]. A crash between the
/// two can leave this file behind.
pub const JOURNAL_TMP: &str = "journal.wcj.tmp";

const MAGIC: &str = "wavcraft-journal 1";

// ---------------------------------------------------------------- snapshot and encoding

/// What a journal records: the history's steps (label and state, oldest first) and the cursor.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub steps: Vec<(String, Arc<DocState>)>,
    pub cursor: usize,
}

impl Snapshot {
    /// The history as it is now (cheap: clones `Arc`s).
    pub fn of(history: &History) -> Snapshot {
        Snapshot { steps: history.steps().iter().map(|s| (s.label().to_string(), s.state().clone())).collect(), cursor: history.cursor() }
    }

    /// A token that changes when the history changes (a step added, dropped or replaced, the
    /// cursor moved): the app compares it to know whether to offer a new snapshot.
    pub fn signature(history: &History) -> (usize, usize, usize, usize, usize) {
        let ptr = |s: Option<&crate::doc::history::Step>| s.map_or(0, |s| Arc::as_ptr(s.state()) as usize);
        (history.steps().len(), history.cursor(), history.dropped_total(), ptr(history.steps().first()), ptr(history.steps().last()))
    }
}

/// The 64-bit FNV-1a hash of `bytes`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// `body` with its checksum line appended: `body`, then `sum `, then [`fnv1a`] of `body`'s bytes
/// as 16 lowercase hex digits, then `\n`.
pub fn seal(body: &str) -> String {
    format!("{}sum {:016x}\n", body, fnv1a(body.as_bytes()))
}

/// The body of a sealed journal text, if its checksum line is there and matches.
pub fn unseal(text: &str) -> Result<&str, String> {
    let pos = text.rfind("sum ").ok_or("no checksum line (the journal is cut short)".to_string())?;

    let body = &text[..pos];
    let tail = &text[pos..];

    let expected_checksum = format!("{:016x}", fnv1a(body.as_bytes()));
    let actual_checksum = tail.trim_end().strip_prefix("sum ").unwrap_or("").trim();

    if actual_checksum == expected_checksum { Ok(body) } else { Err("the checksum does not match (the journal is damaged)".to_string()) }
}

/// A token for a free-form string: bytes other than letters, digits and `-_./~` become `%XX`;
/// the empty string is `%%`.
fn esc(s: &str) -> String {
    if s.is_empty() {
        return "%%".to_string();
    }
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_./~".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn unesc(t: &str) -> Result<String, String> {
    if t == "%%" {
        return Ok(String::new());
    }
    let b = t.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = t.get(i + 1..i + 3).ok_or("a damaged escape")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "a damaged escape")?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "text that is not UTF-8".to_string())
}

/// A block encoding's name in the journal.
fn encoding_name(e: Encoding) -> &'static str {
    match e {
        Encoding::F32 => "f32",
        Encoding::I16 => "i16",
        Encoding::I24 => "i24",
    }
}

fn encoding_from_name(name: &str) -> Option<Encoding> {
    Some(match name {
        "f32" => Encoding::F32,
        "i16" => Encoding::I16,
        "i24" => Encoding::I24,
        _ => return None,
    })
}

fn format_name(f: FileFormat) -> &'static str {
    match f {
        FileFormat::Wav => "Wav",
        FileFormat::Aiff => "Aiff",
        FileFormat::Flac => "Flac",
        FileFormat::Mp3 => "Mp3",
        FileFormat::Ogg => "Ogg",
        FileFormat::Aac => "Aac",
        FileFormat::Alac => "Alac",
        FileFormat::Caf => "Caf",
        _ => "Other",
    }
}

fn format_from(n: &str) -> FileFormat {
    match n {
        "Wav" => FileFormat::Wav,
        "Aiff" => FileFormat::Aiff,
        "Flac" => FileFormat::Flac,
        "Mp3" => FileFormat::Mp3,
        "Ogg" => FileFormat::Ogg,
        "Aac" => FileFormat::Aac,
        "Alac" => FileFormat::Alac,
        "Caf" => FileFormat::Caf,
        _ => FileFormat::Other,
    }
}

fn sample_name(f: SampleFormat) -> &'static str {
    match f {
        SampleFormat::Int8 => "Int8",
        SampleFormat::Int16 => "Int16",
        SampleFormat::Int24 => "Int24",
        SampleFormat::Int32 => "Int32",
        SampleFormat::Float32 => "Float32",
        SampleFormat::Float64 => "Float64",
        _ => "Compressed",
    }
}

fn sample_from(n: &str) -> SampleFormat {
    match n {
        "Int8" => SampleFormat::Int8,
        "Int16" => SampleFormat::Int16,
        "Int24" => SampleFormat::Int24,
        "Int32" => SampleFormat::Int32,
        "Float32" => SampleFormat::Float32,
        "Float64" => SampleFormat::Float64,
        _ => SampleFormat::Compressed,
    }
}

/// The journal's text for `snap`. `next_id` is the store's next block id.
fn encode(snap: &Snapshot, next_id: u64) -> Result<String, String> {
    let Some((_, cur)) = snap.steps.get(snap.cursor) else { return Err("the cursor is past the steps".to_string()) };
    let src = cur.source();
    let mut o = String::new();
    let _ = writeln!(o, "{MAGIC}");
    let _ = writeln!(o, "next-id {next_id}");
    let _ = writeln!(o, "rate {}", cur.sample_rate());
    let _ = writeln!(o, "src-path {}", esc(&src.path.to_string_lossy()));
    let _ = writeln!(o, "src-format {}", format_name(src.format));
    let _ = writeln!(o, "src-sample {}", sample_name(src.sample_format));
    let _ = writeln!(o, "src-rate {}", src.sample_rate);
    let _ = writeln!(o, "src-channels {}", src.channels);
    let _ = writeln!(o, "src-rf64 {}", u8::from(src.rf64));
    if let Some(b) = &src.bwf {
        let _ = writeln!(
            o,
            "src-bwf {} {} {} {} {} {}",
            esc(&b.description),
            esc(&b.originator),
            esc(&b.originator_reference),
            esc(&b.origination_date),
            esc(&b.origination_time),
            b.time_reference
        );
    }
    for c in &src.chunks {
        let _ = writeln!(o, "src-chunk {} {} {}", c.id.iter().map(|b| format!("{b:02x}")).collect::<String>(), c.offset, c.size);
    }
    for w in &src.warnings {
        let _ = writeln!(o, "src-warn {}", esc(w));
    }
    // Distinct blocks and distinct channels (states share most of both).
    let mut blocks: BTreeMap<BlockId, BlockRef> = BTreeMap::new();
    let mut chans: Vec<&Arc<Channel>> = Vec::new();
    let mut chan_index: HashMap<usize, usize> = HashMap::new();
    let mut step_chans: Vec<Vec<usize>> = Vec::new();
    for (_, st) in &snap.steps {
        let mut idx = Vec::new();
        for ch in st.channels() {
            let key = Arc::as_ptr(ch) as usize;
            let i = *chan_index.entry(key).or_insert_with(|| {
                chans.push(ch);
                chans.len() - 1
            });
            idx.push(i);
        }
        step_chans.push(idx);
    }
    for ch in &chans {
        for p in ch.pieces() {
            let b = *p.block().block();
            if !b.is_zero() {
                blocks.insert(b.id, b);
            }
        }
    }
    for b in blocks.values() {
        let _ = writeln!(o, "block {} {} {} {} {}", b.id.0, b.pack, b.byte_offset, b.frames, encoding_name(b.encoding));
    }
    for (i, ch) in chans.iter().enumerate() {
        let _ = writeln!(o, "chan {i} {}", ch.piece_count());
        for p in ch.pieces() {
            let _ = writeln!(o, "p {} {} {}", p.block().block().id.0, p.offset(), p.len());
        }
    }
    for ((label, _), idx) in snap.steps.iter().zip(&step_chans) {
        let list: Vec<String> = idx.iter().map(usize::to_string).collect();
        let _ = writeln!(o, "step {} {}", esc(label), list.join(" "));
    }
    let _ = writeln!(o, "cursor {}", snap.cursor);
    let sum = fnv1a(o.as_bytes());
    let _ = writeln!(o, "sum {sum:016x}");
    Ok(o)
}

/// A journal read back, not yet tied to a store.
struct Parsed {
    next_id: u64,
    rate: u32,
    source: SourceFormat,
    blocks: Vec<BlockRef>,
    chans: Vec<Vec<(u64, u32, u32)>>,
    steps: Vec<(String, Vec<usize>)>,
    cursor: usize,
}

fn decode(text: &str) -> Result<Parsed, String> {
    let sum_at = text.rfind("sum ").ok_or("no checksum line (the journal is cut short)")?;
    let (body, tail) = text.split_at(sum_at);
    if tail.trim_end().strip_prefix("sum ").map(str::trim) != Some(&format!("{:016x}", fnv1a(body.as_bytes()))) {
        return Err("the checksum does not match (the journal is damaged)".to_string());
    }
    let mut lines = body.lines();
    if lines.next() != Some(MAGIC) {
        return Err("not a journal this version reads".to_string());
    }
    let num =
        |t: Option<&str>| -> Result<u64, String> { t.ok_or("a line is too short")?.parse::<u64>().map_err(|_| "a number is wrong".to_string()) };
    let mut p = Parsed {
        next_id: 0,
        rate: 0,
        source: SourceFormat::synthetic(1, 1),
        blocks: Vec::new(),
        chans: Vec::new(),
        steps: Vec::new(),
        cursor: usize::MAX,
    };
    p.source.chunks.clear();
    for line in lines {
        let mut t = line.split_whitespace();
        match t.next() {
            Some("next-id") => p.next_id = num(t.next())?,
            Some("rate") => p.rate = u32::try_from(num(t.next())?).map_err(|_| "a rate is too big")?,
            Some("src-path") => p.source.path = PathBuf::from(unesc(t.next().ok_or("a line is too short")?)?),
            Some("src-format") => p.source.format = format_from(t.next().unwrap_or("")),
            Some("src-sample") => p.source.sample_format = sample_from(t.next().unwrap_or("")),
            Some("src-rate") => p.source.sample_rate = u32::try_from(num(t.next())?).map_err(|_| "a rate is too big")?,
            Some("src-channels") => p.source.channels = u16::try_from(num(t.next())?).map_err(|_| "too many channels")?,
            Some("src-rf64") => p.source.rf64 = num(t.next())? != 0,
            Some("src-bwf") => {
                let mut f = || unesc(t.next().unwrap_or(""));
                let (description, originator, originator_reference, origination_date, origination_time) = (f()?, f()?, f()?, f()?, f()?);
                let time_reference = num(t.next())?;
                p.source.bwf = Some(BwfInfo { description, originator, originator_reference, origination_date, origination_time, time_reference });
            }
            Some("src-chunk") => {
                let hex = t.next().ok_or("a line is too short")?;
                let mut id = [0u8; 4];
                for (i, b) in id.iter_mut().enumerate() {
                    *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2).ok_or("a chunk id is wrong")?, 16).map_err(|_| "a chunk id is wrong")?;
                }
                p.source.chunks.push(ChunkInfo { id, offset: num(t.next())?, size: num(t.next())? });
            }
            Some("src-warn") => p.source.warnings.push(unesc(t.next().ok_or("a line is too short")?)?),
            Some("block") => p.blocks.push(BlockRef {
                id: BlockId(num(t.next())?),
                pack: u32::try_from(num(t.next())?).map_err(|_| "a pack number is too big")?,
                byte_offset: num(t.next())?,
                frames: u32::try_from(num(t.next())?).map_err(|_| "a block is too long")?,
                // A journal from before the encoding was written reads as f32, the only one then.
                encoding: match t.next() {
                    None => Encoding::F32,
                    Some(name) => encoding_from_name(name).ok_or("a block's encoding is unknown")?,
                },
            }),
            Some("chan") => {
                if num(t.next())? != p.chans.len() as u64 {
                    return Err("channels out of order".to_string());
                }
                let n = num(t.next())?;
                p.chans.push(Vec::with_capacity(usize::try_from(n.min(1 << 20)).unwrap_or(0)));
            }
            Some("p") => {
                let piece = (
                    num(t.next())?,
                    u32::try_from(num(t.next())?).map_err(|_| "a piece is too long")?,
                    u32::try_from(num(t.next())?).map_err(|_| "a piece is too long")?,
                );
                p.chans.last_mut().ok_or("a piece before any channel")?.push(piece);
            }
            Some("step") => {
                let label = unesc(t.next().ok_or("a line is too short")?)?;
                let idx = t.map(|x| x.parse::<usize>().map_err(|_| "a channel index is wrong".to_string())).collect::<Result<Vec<_>, _>>()?;
                p.steps.push((label, idx));
            }
            Some("cursor") => p.cursor = usize::try_from(num(t.next())?).map_err(|_| "the cursor is too big")?,
            Some(other) => return Err(format!("a line this version does not know: {other}")),
            None => {}
        }
    }
    if p.steps.is_empty() || p.cursor >= p.steps.len() || p.rate == 0 || p.next_id == 0 {
        return Err("the journal is incomplete".to_string());
    }
    Ok(p)
}

// ---------------------------------------------------------------- writing

/// Write `snap` as the session's journal, durably: sync the store, write the new file beside the
/// old one, fsync it, rename it over, fsync the folder. Returns the journal's size in bytes. The
/// old journal stays whole until the rename.
pub fn write_journal(store: &Store, snap: &Snapshot) -> io::Result<u64> {
    let text = encode(snap, store.next_block_id()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    store.sync()?;
    let dir = store.dir();
    let (tmp, dst) = (dir.join(JOURNAL_TMP), dir.join(JOURNAL_FILE));
    let mut opts = OpenOptions::new();
    // A leftover half-written file is removed, not reused: a new file gets the owner-only mode.
    let _ = std::fs::remove_file(&tmp);
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, &dst)?;
    sync_dir(dir);
    Ok(text.len() as u64)
}

/// Make a folder's entries (a new or renamed file) durable. Where the system cannot, nothing.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// How far an [`Autosaver`] has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AutosaveStats {
    /// Snapshots handed to [`Autosaver::offer`].
    pub offered: u64,
    /// Snapshots dealt with: written, failed, or skipped because a newer one had come.
    pub handled: u64,
    /// Journals made durable.
    pub written: u64,
    /// Writes that failed (the previous journal stays).
    pub failed: u64,
}

impl AutosaveStats {
    /// Whether a snapshot is still waiting or being written.
    pub fn pending(&self) -> bool {
        self.handled < self.offered
    }
}

#[derive(Default)]
struct Shared {
    offered: AtomicU64,
    handled: AtomicU64,
    written: AtomicU64,
    failed: AtomicU64,
    last_error: Mutex<Option<String>>,
}

/// Writes journals on a worker thread: [`offer`](Autosaver::offer) is cheap and never blocks on
/// the disk; of snapshots that pile up only the newest is written. Dropping it waits for the
/// write in progress and stops the thread (so the store can then close).
pub struct Autosaver {
    tx: Option<Sender<Snapshot>>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Autosaver {
    /// A worker for `store`'s session folder.
    pub fn start(store: &Store) -> Autosaver {
        let (tx, rx) = channel::<Snapshot>();
        let shared = Arc::new(Shared::default());
        let (store, sh) = (store.clone(), shared.clone());
        let thread = std::thread::Builder::new().name("autosave".into()).spawn(move || worker(&store, &rx, &sh)).ok();
        Autosaver { tx: thread.as_ref().and(Some(tx)), thread, shared }
    }

    /// Hand over the history as it is now.
    pub fn offer(&self, snap: Snapshot) {
        if let Some(tx) = &self.tx {
            self.shared.offered.fetch_add(1, Ordering::SeqCst);
            if tx.send(snap).is_err() {
                self.shared.handled.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    pub fn stats(&self) -> AutosaveStats {
        let s = &self.shared;
        AutosaveStats {
            offered: s.offered.load(Ordering::SeqCst),
            handled: s.handled.load(Ordering::SeqCst),
            written: s.written.load(Ordering::SeqCst),
            failed: s.failed.load(Ordering::SeqCst),
        }
    }

    /// The last write error, if the last attempt failed.
    pub fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().ok().and_then(|e| e.clone())
    }

    /// Wait until nothing is pending, at most `timeout`. Whether it got there.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let t0 = Instant::now();
        while self.stats().pending() {
            if t0.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }
}

impl Drop for Autosaver {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn worker(store: &Store, rx: &Receiver<Snapshot>, sh: &Shared) {
    // The blocks of the journal last made durable: held so the store keeps their packs.
    let mut pin: Vec<Arc<BlockHandle>> = Vec::new();
    while let Ok(mut snap) = rx.recv() {
        let mut taken = 1;
        while let Ok(newer) = rx.try_recv() {
            snap = newer;
            taken += 1;
        }
        match write_journal(store, &snap) {
            Ok(_) => {
                pin = pinned(&snap);
                sh.written.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut e) = sh.last_error.lock() {
                    *e = None;
                }
            }
            Err(e) => {
                sh.failed.fetch_add(1, Ordering::SeqCst);
                log::warn!(target: "wavcraft", "autosave: could not write the journal in {}: {e}", store.dir().display());
                if let Ok(mut le) = sh.last_error.lock() {
                    *le = Some(e.to_string());
                }
            }
        }
        drop(snap);
        sh.handled.fetch_add(taken, Ordering::SeqCst);
    }
    drop(pin);
}

fn pinned(snap: &Snapshot) -> Vec<Arc<BlockHandle>> {
    let mut seen: HashMap<BlockId, Arc<BlockHandle>> = HashMap::new();
    for (_, st) in &snap.steps {
        for ch in st.channels() {
            for p in ch.pieces() {
                if !p.block().is_zero() {
                    seen.entry(p.block().id()).or_insert_with(|| p.block().clone());
                }
            }
        }
    }
    seen.into_values().collect()
}

// ---------------------------------------------------------------- finding and recovering

/// What a journal says about its document.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalInfo {
    pub labels: Vec<String>,
    pub cursor: usize,
    pub frames: u64,
    pub channels: usize,
    pub sample_rate: u32,
    pub source_path: PathBuf,
}

/// A stale session folder that holds a journal.
#[derive(Debug, Clone)]
pub struct Recoverable {
    pub dir: PathBuf,
    pub bytes: u64,
    /// When the journal was last written.
    pub saved_at: Option<SystemTime>,
    /// What the journal says, or why it cannot be read (then the folder is kept, not recovered).
    pub info: Result<JournalInfo, String>,
}

fn info_of(p: &Parsed) -> JournalInfo {
    let frames = p
        .steps
        .get(p.cursor)
        .and_then(|(_, idx)| idx.first())
        .and_then(|c| p.chans.get(*c))
        .map_or(0, |c| c.iter().map(|x| u64::from(x.2)).sum::<u64>());
    let channels = p.steps.get(p.cursor).map_or(0, |(_, idx)| idx.len());
    JournalInfo {
        labels: p.steps.iter().map(|(l, _)| l.clone()).collect(),
        cursor: p.cursor,
        frames,
        channels,
        sample_rate: p.rate,
        source_path: p.source.path.clone(),
    }
}

/// The stale sessions under `root` that hold a journal: recovery candidates, newest first. Never
/// a running program's session.
pub fn recoverable_sessions(root: &Path) -> Vec<Recoverable> {
    let mut out: Vec<Recoverable> = stale_candidates(root)
        .into_iter()
        .filter(|(_, journaled)| *journaled)
        .map(|(s, _)| {
            let saved_at = std::fs::metadata(s.dir.join(JOURNAL_FILE)).and_then(|m| m.modified()).ok();
            let info = std::fs::read_to_string(s.dir.join(JOURNAL_FILE))
                .map_err(|e| format!("the journal cannot be read: {e}"))
                .and_then(|t| decode(&t))
                .map(|p| info_of(&p));
            Recoverable { dir: s.dir, bytes: s.bytes, saved_at, info }
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.saved_at));
    out
}

/// Why a session could not be recovered. Every file is as it was.
#[derive(Debug)]
pub enum RecoverError {
    /// A program is running this session (or the lock could not be taken).
    InUse(String),
    /// Not a session this machine made, or not a session folder.
    NotOurs(String),
    /// No journal, or one that cannot be read or does not fit the packs.
    Journal(String),
    Io(io::Error),
}

impl std::fmt::Display for RecoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoverError::InUse(s) | RecoverError::NotOurs(s) | RecoverError::Journal(s) => f.write_str(s),
            RecoverError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RecoverError {}

impl From<io::Error> for RecoverError {
    fn from(e: io::Error) -> Self {
        RecoverError::Io(e)
    }
}

/// A session brought back: its store (now owned by this run), and the history's steps.
pub struct Recovered {
    pub store: Store,
    /// Label and state, oldest first.
    pub steps: Vec<(String, DocState)>,
    pub cursor: usize,
}

impl Recovered {
    /// The undo history, with the cursor where it was. `None` only for an empty recovery (never
    /// made by [`recover`]).
    pub fn into_history(self, cap_disk_bytes: u64) -> Option<History> {
        History::restore(self.steps, self.cursor, cap_disk_bytes)
    }
}

/// Take over the stale session folder `dir` and rebuild its document and undo history from its
/// journal. See the module notes for the rules; on any error the folder is untouched.
pub fn recover(dir: &Path, opts: StoreOptions) -> Result<Recovered, RecoverError> {
    let machine = machine_id();
    let lock = std::fs::read_to_string(dir.join(LOCK_FILE)).map_err(|e| RecoverError::NotOurs(format!("{} has no lock file: {e}", dir.display())))?;
    let ours = lock.lines().any(|l| l.starts_with("app=wavcraft"))
        && !machine.is_empty()
        && lock.lines().any(|l| l.strip_prefix("machine=") == Some(machine.as_str()));
    if !ours {
        return Err(RecoverError::NotOurs(format!("{} was not made by WavCraft on this machine", dir.display())));
    }
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(HELD_FILE))
        .map_err(|e| RecoverError::NotOurs(format!("{} has no session.held: {e}", dir.display())))?;
    if let Err(e) = held.try_lock() {
        return Err(RecoverError::InUse(format!("{} is in use by a running program ({e})", dir.display())));
    }
    let text = std::fs::read_to_string(dir.join(JOURNAL_FILE))
        .map_err(|e| RecoverError::Journal(format!("no readable journal in {}: {e}", dir.display())))?;
    let parsed = decode(&text).map_err(|e| RecoverError::Journal(format!("the journal in {}: {e}", dir.display())))?;
    let store = Store::adopt(dir, held, opts, parsed.next_id)?;
    let bad = |e: String| RecoverError::Journal(format!("the journal in {}: {e}", dir.display()));
    let mut handles: HashMap<u64, Arc<BlockHandle>> = HashMap::new();
    for b in &parsed.blocks {
        let h = store.adopt_block(*b).map_err(|e| bad(e.to_string()))?;
        if handles.insert(b.id.0, h).is_some() {
            return Err(bad(format!("block {} listed twice", b.id.0)));
        }
    }
    let mut channels: Vec<Arc<Channel>> = Vec::with_capacity(parsed.chans.len());
    for pieces in &parsed.chans {
        let mut ch = Channel::new();
        for &(id, offset, len) in pieces {
            let block = if id == 0 {
                Store::zero()
            } else {
                handles.get(&id).cloned().ok_or_else(|| bad(format!("a piece names block {id}, which is not listed")))?
            };
            ch.push(Piece::new(block, offset, len).ok_or_else(|| bad(format!("a piece of block {id} runs past the block")))?);
        }
        channels.push(Arc::new(ch));
    }
    let source = Arc::new(parsed.source);
    let mut steps = Vec::with_capacity(parsed.steps.len());
    for (label, idx) in parsed.steps {
        let chs = idx
            .iter()
            .map(|i| channels.get(*i).cloned().ok_or_else(|| bad("a step names a channel that is not there".to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let state = DocState::from_shared(store.clone(), chs, parsed.rate, source.clone()).map_err(|e| bad(e.to_string()))?;
        steps.push((label, state));
    }
    store.thaw();
    Ok(Recovered { store, steps, cursor: parsed.cursor })
}

/// Remove a stale session folder the owner chose to discard (its recovery declined). Refuses a
/// folder a running program holds, or one this machine did not make. This is the only thing
/// that removes a session with a journal.
pub fn discard(dir: &Path) -> Result<(), RecoverError> {
    let machine = machine_id();
    let lock = std::fs::read_to_string(dir.join(LOCK_FILE)).map_err(|e| RecoverError::NotOurs(format!("{} has no lock file: {e}", dir.display())))?;
    if machine.is_empty()
        || !lock.lines().any(|l| l.strip_prefix("machine=") == Some(machine.as_str()))
        || !lock.lines().any(|l| l.starts_with("app=wavcraft"))
    {
        return Err(RecoverError::NotOurs(format!("{} was not made by WavCraft on this machine", dir.display())));
    }
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(HELD_FILE))
        .map_err(|e| RecoverError::NotOurs(format!("{} has no session.held: {e}", dir.display())))?;
    held.try_lock().map_err(|e| RecoverError::InUse(format!("{} is in use by a running program ({e})", dir.display())))?;
    drop(held);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
