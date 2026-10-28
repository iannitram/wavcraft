//! Recording a take (roadmap C020-C023): the audio side, with no UI and no audio device.
//!
//! The path of a sample: the input callback (or the fake input's timer thread) calls
//! [`Ring::push`], which never blocks, locks or allocates and drops whole frames, counting them,
//! when the writer falls behind. A writer thread ([`Recorder`]) drains the ring and appends to the
//! take through [`TakeWriter`], which keeps the WAV header valid on disk at all times: it is
//! written (with zero data) when the file is made, and its two size fields are rewritten and the
//! file synced about once a second. A process killed mid-take leaves a plain WAV that every
//! reader opens, holding everything up to the last header update (at most about a second less
//! than was recorded; the bytes after it are still in the file for a repair tool to find).
//!
//! A take never overwrites a file ([`create_take`] makes it with exclusive create).

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Bytes in the header [`TakeWriter`] writes (RIFF, `fmt ` of 16 bytes, `data`).
pub const HEADER_BYTES: u64 = 44;
/// How often the header's sizes are rewritten and the file synced.
pub const HEADER_INTERVAL: Duration = Duration::from_secs(1);
/// A WAV file's sizes are 32-bit: a take stops here (about 4 GB, 8 hours of 48 kHz 24-bit mono).
pub const MAX_DATA_BYTES: u64 = 4_000_000_000;

/// The environment variable that replaces the input device with a generated one (`sine:440`).
pub const ENV_FAKE_INPUT: &str = "WAVCRAFT_FAKE_INPUT";

/// What a take is recorded as. Samples are stored as 16-bit or 24-bit integers, or 32-bit float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecFormat {
    pub rate: u32,
    /// Channels in the take (the number of inputs recorded).
    pub channels: u16,
    pub bits: u16,
    /// Which inputs of the device are recorded (C034): bit `i` is input `i + 1`. 0 means the
    /// first `channels` inputs. Otherwise exactly `channels` bits are set, and the take's
    /// channels are those inputs in ascending order.
    pub inputs: u32,
}

/// The most inputs of one device a take can choose from (bits in [`RecFormat::inputs`]).
pub const MAX_INPUTS: u32 = 32;

/// Which device channels make up the take, in take order. `Copy` and fixed-size, so the audio
/// callback can hold it without allocating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelPick {
    idx: [u8; 8],
    n: u8,
}

impl ChannelPick {
    /// The device channels (0-based) picked, in take order.
    pub fn indices(&self) -> &[u8] {
        &self.idx[..usize::from(self.n)]
    }

    /// The number of device channels the device must have for this pick.
    pub fn needs_device_channels(&self) -> usize {
        self.indices().iter().map(|i| usize::from(*i) + 1).max().unwrap_or(1)
    }

    /// Copy the picked channels of interleaved device frames (`dev_channels` wide) into `out`
    /// (the take's channels wide). Frames that are cut short read as silence. Returns the
    /// frames written; no allocation, so the audio callback may call it.
    pub fn gather(&self, src: &[f32], dev_channels: usize, out: &mut [f32]) -> usize {
        let n = usize::from(self.n).max(1);
        let dev = dev_channels.max(1);
        let frames = (src.len() / dev).min(out.len() / n);
        for f in 0..frames {
            for (k, i) in self.indices().iter().enumerate() {
                out[f * n + k] = src[f * dev + usize::from(*i)];
            }
        }
        frames
    }
}

impl Default for RecFormat {
    /// 48 kHz, mono, 24-bit: the owner's v1 rule (issue 51).
    fn default() -> Self {
        RecFormat { rate: 48_000, channels: 1, bits: 24, inputs: 0 }
    }
}

impl RecFormat {
    pub const RATES: [u32; 5] = [44_100, 48_000, 88_200, 96_000, 192_000];
    pub const BITS: [u16; 3] = [16, 24, 32];

    pub fn validate(self) -> Result<RecFormat, String> {
        if !(8_000..=192_000).contains(&self.rate) {
            return Err(format!("sample rate {} Hz is outside 8000 to 192000", self.rate));
        }
        if !(1..=8).contains(&self.channels) {
            return Err(format!("{} channels is outside 1 to 8", self.channels));
        }
        if self.inputs != 0 && self.inputs.count_ones() != u32::from(self.channels) {
            return Err(format!("{} inputs chosen for {} channels: they must match", self.inputs.count_ones(), self.channels));
        }
        if !Self::BITS.contains(&self.bits) {
            return Err(format!("{} bits: use 16, 24 or 32 (32 is float)", self.bits));
        }
        Ok(self)
    }

    /// The device channels this take records, in take order.
    pub fn pick(self) -> ChannelPick {
        let mut p = ChannelPick { idx: [0; 8], n: 0 };
        if self.inputs == 0 {
            for i in 0..self.channels.min(8) {
                p.idx[usize::from(i)] = i as u8;
                p.n += 1;
            }
        } else {
            for i in 0..MAX_INPUTS {
                if self.inputs & (1 << i) != 0 && p.n < 8 {
                    p.idx[usize::from(p.n)] = i as u8;
                    p.n += 1;
                }
            }
        }
        p
    }

    /// The inputs (1-based) this take records: `1,2,3,4`.
    pub fn inputs_text(self) -> String {
        self.pick().indices().iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(",")
    }

    /// Choose inputs (1-based numbers). The channels follow the choice; a choice of just the
    /// first N inputs is stored as the default (0).
    pub fn with_inputs(mut self, list: &[u32]) -> Result<RecFormat, String> {
        let mut mask = 0u32;
        for &n in list {
            if !(1..=MAX_INPUTS).contains(&n) {
                return Err(format!("input {n} is outside 1 to {MAX_INPUTS}"));
            }
            mask |= 1 << (n - 1);
        }
        let count = mask.count_ones();
        if count == 0 {
            return Err("choose at least one input".to_string());
        }
        if count > 8 {
            return Err(format!("{count} inputs: a take holds at most 8 channels"));
        }
        self.channels = count as u16;
        self.inputs = if mask == (1u32 << count) - 1 { 0 } else { mask };
        self.validate()
    }

    /// Bytes in one frame (one sample of every channel).
    pub fn block_align(self) -> u64 {
        u64::from(self.channels) * u64::from(self.bits / 8)
    }

    /// Bytes written for each second of the take (the header excluded).
    pub fn bytes_per_second(self) -> u64 {
        self.block_align() * u64::from(self.rate)
    }

    /// `rate=48000 ch=1 bits=24` (any subset, any order; the rest are the defaults).
    pub fn parse_args(text: &str) -> Result<RecFormat, String> {
        let mut f = RecFormat::default();
        let (mut ch_given, mut chosen) = (false, None);
        for word in text.split_whitespace() {
            let (key, value) = word.split_once('=').ok_or_else(|| format!("expected key=value, got {word:?}"))?;
            let bad = |_| format!("{key}: {value:?} is not a number");
            match key {
                "rate" => f.rate = value.parse().map_err(bad)?,
                "ch" | "channels" => {
                    f.channels = value.parse().map_err(bad)?;
                    ch_given = true;
                }
                "inputs" => {
                    let list = value
                        .split(',')
                        .map(|v| v.trim().parse::<u32>().map_err(|_| format!("inputs: {v:?} is not an input number")))
                        .collect::<Result<Vec<_>, _>>()?;
                    chosen = Some(list);
                }
                "bits" => f.bits = value.parse().map_err(bad)?,
                _ => return Err(format!("unknown setting {key:?} (rate, ch, bits, inputs)")),
            }
        }
        if let Some(list) = chosen {
            let want = f.channels;
            f = f.with_inputs(&list)?;
            if ch_given && f.channels != want {
                return Err(format!("ch={want} does not match the {} inputs chosen", f.channels));
            }
        }
        f.validate()
    }

    /// The 44-byte header for a take holding `data_bytes` of audio.
    pub fn header_for(self, data_bytes: u64) -> [u8; HEADER_BYTES as usize] {
        self.header(data_bytes)
    }

    fn header(self, data_bytes: u64) -> [u8; HEADER_BYTES as usize] {
        let data = data_bytes.min(u64::from(u32::MAX) - 36) as u32;
        let float = self.bits == 32;
        let mut h = [0u8; HEADER_BYTES as usize];
        h[0..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
        h[8..16].copy_from_slice(b"WAVEfmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&(if float { 3u16 } else { 1u16 }).to_le_bytes());
        h[22..24].copy_from_slice(&self.channels.to_le_bytes());
        h[24..28].copy_from_slice(&self.rate.to_le_bytes());
        h[28..32].copy_from_slice(&(self.bytes_per_second() as u32).to_le_bytes());
        h[32..34].copy_from_slice(&(self.block_align() as u16).to_le_bytes());
        h[34..36].copy_from_slice(&self.bits.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&data.to_le_bytes());
        h
    }
}

/// A lock-free single-producer, single-consumer queue of `f32` samples (interleaved frames).
///
/// The producer is the audio callback; the consumer is the writer thread. Samples live in
/// atomics, so there is no `unsafe`. When the queue is full the producer drops whole frames and
/// counts them; it never waits.
pub struct Ring {
    slots: Box<[AtomicU32]>,
    channels: usize,
    /// Next sample the consumer reads (counts samples since the start, never wraps in practice).
    head: AtomicU64,
    /// Next sample the producer writes.
    tail: AtomicU64,
    dropped_frames: AtomicU64,
    /// While set, `push` discards what it is given (a pause: not a drop, not counted).
    paused: AtomicBool,
    /// Largest absolute sample since the last [`Ring::take_peak`], as the bits of a non-negative
    /// `f32` (which order like integers).
    peak: AtomicU32,
    /// Software input monitoring (C025): every frame the input callback pushes is also copied
    /// here, before the pause check, so the performer hears the input while a punch-in waits.
    monitor: Option<Arc<MonitorRing>>,
    /// The input latency the device reported, in microseconds (0: it did not say). Set by the
    /// input callback from the stream timestamps (C010).
    reported_latency_us: AtomicU64,
}

impl Ring {
    /// A ring holding up to `frames` frames of `channels` samples.
    pub fn new(frames: usize, channels: u16) -> Arc<Ring> {
        Ring::with_monitor(frames, channels, None)
    }

    /// [`Ring::new`] that also feeds `monitor` (its channel count must be `channels`).
    pub fn with_monitor(frames: usize, channels: u16, monitor: Option<Arc<MonitorRing>>) -> Arc<Ring> {
        let channels = usize::from(channels.max(1));
        let slots = (0..frames.max(1) * channels).map(|_| AtomicU32::new(0)).collect();
        Arc::new(Ring {
            slots,
            channels,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            peak: AtomicU32::new(0),
            monitor: monitor.filter(|m| m.channels() == channels),
            reported_latency_us: AtomicU64::new(0),
        })
    }

    /// Producer side: the latency the device reports for this input, in microseconds.
    pub fn set_reported_latency_us(&self, us: u64) {
        self.reported_latency_us.store(us, Ordering::Relaxed);
    }

    /// The input latency the device reported, if it did (C010).
    pub fn reported_latency_us(&self) -> Option<u64> {
        Some(self.reported_latency_us.load(Ordering::Relaxed)).filter(|v| *v > 0)
    }

    /// Producer side: queue whole frames from `samples` (a trailing partial frame is ignored).
    /// Returns the frames queued; the rest are counted as dropped.
    pub fn push(&self, samples: &[f32]) -> usize {
        let ch = self.channels;
        let frames = samples.len() / ch;
        if let Some(m) = &self.monitor {
            m.push(samples);
        }
        if self.paused.load(Ordering::Relaxed) {
            return 0;
        }
        let cap = self.slots.len() as u64;
        let (tail, head) = (self.tail.load(Ordering::Relaxed), self.head.load(Ordering::Acquire));
        let free_frames = (cap - (tail - head)) as usize / ch;
        let take = frames.min(free_frames);
        let mut peak = 0u32;
        for (i, s) in samples[..take * ch].iter().enumerate() {
            let bits = s.abs().to_bits();
            peak = peak.max(if s.is_nan() { 0 } else { bits });
            self.slots[((tail + i as u64) % cap) as usize].store(s.to_bits(), Ordering::Relaxed);
        }
        self.tail.store(tail + (take * ch) as u64, Ordering::Release);
        if peak > 0 {
            self.peak.fetch_max(peak, Ordering::Relaxed);
        }
        if take < frames {
            self.dropped_frames.fetch_add((frames - take) as u64, Ordering::Relaxed);
        }
        take
    }

    /// Consumer side: move up to `max_frames` frames into `out` (appended). Returns the frames moved.
    pub fn pop(&self, out: &mut Vec<f32>, max_frames: usize) -> usize {
        let ch = self.channels;
        let cap = self.slots.len() as u64;
        let (head, tail) = (self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Acquire));
        let frames = (((tail - head) as usize) / ch).min(max_frames);
        out.reserve(frames * ch);
        for i in 0..(frames * ch) as u64 {
            out.push(f32::from_bits(self.slots[((head + i) % cap) as usize].load(Ordering::Relaxed)));
        }
        self.head.store(head + (frames * ch) as u64, Ordering::Release);
        frames
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Frames dropped so far because the writer fell behind.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Pause or resume the take (C029): while paused the input is discarded, so the file holds
    /// only what was recorded. Not a drop: `dropped_frames` does not count it.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Frames accepted into the queue since the start (the file's length once the writer has
    /// caught up). At a resume this is where the new stretch begins in the file.
    pub fn accepted_frames(&self) -> u64 {
        self.tail.load(Ordering::Acquire) / self.channels as u64
    }

    /// The largest absolute sample since the last call (0.0 to above 1.0 when it clipped).
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

/// The path from the input callback to the output callback for software input monitoring (C025):
/// a lock-free single-producer, single-consumer queue of interleaved `f32` frames in atomics (no
/// `unsafe`). Neither side blocks, locks or allocates. It is small on purpose, so the delay it
/// can add is bounded: when the producer finds it full it drops the new frames, and when the
/// consumer finds more than three quarters of it waiting it skips the oldest down to a quarter;
/// both count as overflow. When the consumer wants more than is there it gets silence for the
/// rest and the shortfall is counted as an underrun (only once the input has started).
pub struct MonitorRing {
    slots: Box<[AtomicU32]>,
    channels: usize,
    /// Next sample the consumer reads. Written by the consumer only.
    head: AtomicU64,
    /// Next sample the producer writes. Written by the producer only.
    tail: AtomicU64,
    overflow_frames: AtomicU64,
    underrun_frames: AtomicU64,
    /// Frames the consumer has taken for playing (not counting silence).
    played_frames: AtomicU64,
}

impl MonitorRing {
    /// A ring of `frames` frames of `channels` samples (at least 4 frames).
    pub fn new(frames: usize, channels: u16) -> Arc<MonitorRing> {
        let channels = usize::from(channels.max(1));
        let slots = (0..frames.max(4) * channels).map(|_| AtomicU32::new(0)).collect();
        Arc::new(MonitorRing {
            slots,
            channels,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            overflow_frames: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            played_frames: AtomicU64::new(0),
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The capacity in frames.
    pub fn capacity_frames(&self) -> usize {
        self.slots.len() / self.channels
    }

    /// Producer side: queue whole frames; what does not fit is dropped and counted. Returns the
    /// frames queued.
    pub fn push(&self, samples: &[f32]) -> usize {
        let ch = self.channels;
        let frames = samples.len() / ch;
        let cap = self.slots.len() as u64;
        let (tail, head) = (self.tail.load(Ordering::Relaxed), self.head.load(Ordering::Acquire));
        let take = frames.min((cap - (tail - head)) as usize / ch);
        for (i, s) in samples[..take * ch].iter().enumerate() {
            self.slots[((tail + i as u64) % cap) as usize].store(s.to_bits(), Ordering::Relaxed);
        }
        self.tail.store(tail + (take * ch) as u64, Ordering::Release);
        if take < frames {
            self.overflow_frames.fetch_add((frames - take) as u64, Ordering::Relaxed);
        }
        take
    }

    /// Consumer side: fill `out` (whole frames; a trailing partial frame is zeroed) and return
    /// the frames that came from the queue; the rest of `out` is silence.
    pub fn pop_into(&self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        let cap = self.slots.len() as u64;
        let (mut head, tail) = (self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Acquire));
        let cap_frames = self.slots.len() / ch;
        let waiting = ((tail - head) as usize) / ch;
        if waiting > cap_frames * 3 / 4 {
            let skip = waiting - cap_frames / 4;
            head += (skip * ch) as u64;
            self.overflow_frames.fetch_add(skip as u64, Ordering::Relaxed);
        }
        let want = out.len() / ch;
        let got = (((tail - head) as usize) / ch).min(want);
        for (i, o) in out[..got * ch].iter_mut().enumerate() {
            *o = f32::from_bits(self.slots[((head + i as u64) % cap) as usize].load(Ordering::Relaxed));
        }
        out[got * ch..].iter_mut().for_each(|o| *o = 0.0);
        self.head.store(head + (got * ch) as u64, Ordering::Release);
        if got < want && tail > 0 {
            self.underrun_frames.fetch_add((want - got) as u64, Ordering::Relaxed);
        }
        self.played_frames.fetch_add(got as u64, Ordering::Relaxed);
        got
    }

    /// Frames waiting for the output.
    pub fn waiting_frames(&self) -> usize {
        ((self.tail.load(Ordering::Acquire) - self.head.load(Ordering::Acquire)) as usize) / self.channels
    }

    /// Frames dropped or skipped because the output did not keep up.
    pub fn overflow_frames(&self) -> u64 {
        self.overflow_frames.load(Ordering::Relaxed)
    }

    /// Frames the output asked for that had not arrived yet (heard as silence).
    pub fn underrun_frames(&self) -> u64 {
        self.underrun_frames.load(Ordering::Relaxed)
    }

    /// Frames handed to the output so far.
    pub fn played_frames(&self) -> u64 {
        self.played_frames.load(Ordering::Relaxed)
    }
}

/// The take's file: a WAV header that is valid after every call to [`TakeWriter::update_header`].
pub struct TakeWriter {
    file: File,
    path: PathBuf,
    fmt: RecFormat,
    data_bytes: u64,
    /// The data size the header on disk holds now.
    header_bytes: u64,
    scratch: Vec<u8>,
    /// The marker beside the take, held (locked) until the take is finished cleanly.
    marker: Option<Marker>,
}

/// Suffix of the marker file beside a take that has not been finished: `Take ….wav.recording`.
/// It is made with the take and removed only when the header is final, so a crashed take is the
/// one that still has it (see `take_recovery`). It holds the format as text and is locked by the
/// recording process while the take runs, which tells a live take from a dead one.
pub const MARKER_SUFFIX: &str = ".recording";

/// The marker's path for a take.
pub fn marker_path(take: &Path) -> PathBuf {
    let mut name = take.as_os_str().to_os_string();
    name.push(MARKER_SUFFIX);
    PathBuf::from(name)
}

struct Marker {
    file: File,
    path: PathBuf,
}

impl TakeWriter {
    /// Make `path` (never over an existing file) with a valid, empty header, synced.
    pub fn create(path: &Path, fmt: RecFormat) -> io::Result<TakeWriter> {
        let fmt = fmt.validate().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut file = std::fs::OpenOptions::new().write(true).read(true).create_new(true).open(path)?;
        file.write_all(&fmt.header(0))?;
        file.sync_all()?;
        let marker = make_marker(path, fmt);
        Ok(TakeWriter { file, path: path.to_path_buf(), fmt, data_bytes: 0, header_bytes: 0, scratch: Vec::new(), marker })
    }

    /// The take is finished and its header final: remove the marker. Without this call (a crash)
    /// the marker stays and the next start-up offers the take for recovery. Never fails the take.
    pub fn finish_clean(&mut self) {
        if let Some(m) = self.marker.take() {
            drop(m.file);
            if let Err(e) = std::fs::remove_file(&m.path) {
                log::warn!(target: "wavcraft", "recording: cannot remove {}: {e}", m.path.display());
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn format(&self) -> RecFormat {
        self.fmt
    }

    /// Frames written so far (whether or not the header has caught up).
    pub fn frames(&self) -> u64 {
        self.data_bytes / self.fmt.block_align()
    }

    /// Bytes the file holds now (header and data).
    pub fn file_bytes(&self) -> u64 {
        HEADER_BYTES + self.data_bytes
    }

    /// Append interleaved frames. Stops short (and says so) at [`MAX_DATA_BYTES`].
    pub fn write_frames(&mut self, samples: &[f32]) -> io::Result<()> {
        let width = usize::from(self.fmt.bits / 8);
        let room = (MAX_DATA_BYTES.saturating_sub(self.data_bytes) / self.fmt.block_align()) as usize * usize::from(self.fmt.channels);
        let samples = &samples[..samples.len().min(room)];
        self.scratch.clear();
        for &s in samples {
            let s = if s.is_nan() { 0.0 } else { s };
            match self.fmt.bits {
                16 => self.scratch.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()),
                24 => self.scratch.extend_from_slice(&((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
                _ => self.scratch.extend_from_slice(&s.to_le_bytes()),
            }
        }
        debug_assert_eq!(self.scratch.len(), samples.len() * width);
        self.file.write_all(&self.scratch)?;
        self.data_bytes += self.scratch.len() as u64;
        if self.data_bytes >= MAX_DATA_BYTES {
            return Err(io::Error::other("the take reached the 4 GB limit of a WAV file"));
        }
        Ok(())
    }

    /// Rewrite the header's sizes to match the data on disk, then sync. After this returns, a
    /// kill loses nothing written so far.
    pub fn update_header(&mut self) -> io::Result<()> {
        self.file.sync_data()?;
        let whole = self.data_bytes - self.data_bytes % self.fmt.block_align();
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&self.fmt.header(whole))?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.sync_all()?;
        self.header_bytes = whole;
        Ok(())
    }

    /// The data size the header holds on disk.
    pub fn header_data_bytes(&self) -> u64 {
        self.header_bytes
    }
}

/// Make the marker for a take just created and lock it. A take whose marker cannot be made is
/// still recorded (it is logged), it just is not offered after a crash.
fn make_marker(take: &Path, fmt: RecFormat) -> Option<Marker> {
    let path = marker_path(take);
    let text = format!("WavCraft unfinished take\nrate={}\nch={}\nbits={}\n", fmt.rate, fmt.channels, fmt.bits);
    let made = std::fs::OpenOptions::new().write(true).read(true).create(true).truncate(true).open(&path).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        // Held until the process ends; a refused lock only means the take looks live to others.
        let _ = f.try_lock();
        Ok(f)
    });
    match made {
        Ok(file) => Some(Marker { file, path }),
        Err(e) => {
            log::warn!(target: "wavcraft", "recording: cannot make {}: {e}; this take will not be offered for recovery after a crash", path.display());
            None
        }
    }
}

/// Make a new take in `dir` (made if missing) named `Take YYYY-MM-DD HH.MM.SS.wav` (UTC), with
/// ` (2)`, ` (3)`... if that name exists. Never overwrites.
pub fn create_take(dir: &Path, fmt: RecFormat, unix_secs: u64) -> io::Result<TakeWriter> {
    std::fs::create_dir_all(dir)?;
    let stem = take_stem(unix_secs);
    for n in 1..1000 {
        let name = if n == 1 { format!("{stem}.wav") } else { format!("{stem} ({n}).wav") };
        match TakeWriter::create(&dir.join(name), fmt) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            other => return other,
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "a thousand takes with this name exist"))
}

/// `Take 2026-10-20 14.03.22` for a Unix time in seconds (UTC).
pub fn take_stem(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs = unix_secs % 86_400;
    // Civil date from days since 1970-01-01 (Hinnant's algorithm, public domain).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("Take {year:04}-{month:02}-{day:02} {:02}.{:02}.{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// A finished take.
#[derive(Clone, Debug)]
pub struct Take {
    pub path: PathBuf,
    pub fmt: RecFormat,
    pub frames: u64,
    pub dropped_frames: u64,
    /// Set when the writer stopped early (disk full, the size limit); the take up to there is kept.
    pub error: Option<String>,
}

/// What the status readout shows (C023).
#[derive(Clone, Debug, Default)]
pub struct RecStatus {
    pub frames: u64,
    pub file_bytes: u64,
    pub dropped_frames: u64,
    pub error: Option<String>,
}

struct Shared {
    ring: Arc<Ring>,
    stop: AtomicBool,
    frames: AtomicU64,
    file_bytes: AtomicU64,
    error: Mutex<Option<String>>,
}

/// The writer thread and its take. Dropping it stops and finishes the take.
pub struct Recorder {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<TakeWriter>>,
    path: PathBuf,
    fmt: RecFormat,
}

impl Recorder {
    /// Start draining `ring` into `writer` on its own thread.
    pub fn start(writer: TakeWriter, ring: Arc<Ring>) -> io::Result<Recorder> {
        Recorder::start_stalled(writer, ring, Duration::ZERO)
    }

    /// As [`Recorder::start`], but the writer first waits `stall`: a slow disk, for tests of the
    /// dropped-sample warning (C035). The wait ends early when the take is stopped.
    pub fn start_stalled(writer: TakeWriter, ring: Arc<Ring>, stall: Duration) -> io::Result<Recorder> {
        let (path, fmt) = (writer.path.clone(), writer.fmt);
        let shared = Arc::new(Shared {
            ring,
            stop: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            file_bytes: AtomicU64::new(writer.file_bytes()),
            error: Mutex::new(None),
        });
        let worker = shared.clone();
        let handle = std::thread::Builder::new().name("take writer".into()).spawn(move || write_loop(writer, &worker, stall))?;
        Ok(Recorder { shared, handle: Some(handle), path, fmt })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn format(&self) -> RecFormat {
        self.fmt
    }

    pub fn status(&self) -> RecStatus {
        RecStatus {
            frames: self.shared.frames.load(Ordering::Relaxed),
            file_bytes: self.shared.file_bytes.load(Ordering::Relaxed),
            dropped_frames: self.shared.ring.dropped_frames(),
            error: self.shared.error.lock().ok().and_then(|e| e.clone()),
        }
    }

    /// Stop the callback's source first (it must not push after this), then call this: the
    /// ring is drained, the header made final and the file synced.
    pub fn stop(mut self) -> Take {
        self.finish()
    }

    fn finish(&mut self) -> Take {
        self.shared.stop.store(true, Ordering::Release);
        let mut error = None;
        if let Some(h) = self.handle.take()
            && h.join().is_err()
        {
            error = Some("the take writer stopped unexpectedly".to_string());
        }
        let status = self.status();
        Take { path: self.path.clone(), fmt: self.fmt, frames: status.frames, dropped_frames: status.dropped_frames, error: error.or(status.error) }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.finish();
        }
    }
}

fn write_loop(mut w: TakeWriter, shared: &Shared, stall: Duration) -> TakeWriter {
    let mut buf: Vec<f32> = Vec::new();
    let max_frames = 8_192;
    let mut last_header = Instant::now();
    let fail = |why: String| {
        if let Ok(mut e) = shared.error.lock() {
            e.get_or_insert(why);
        }
    };
    let began = Instant::now();
    while began.elapsed() < stall && !shared.stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }
    loop {
        // Read the stop flag first: whatever is queued before it was set is still drained.
        let stopping = shared.stop.load(Ordering::Acquire);
        buf.clear();
        let n = shared.ring.pop(&mut buf, max_frames);
        if n > 0 {
            let result = w.write_frames(&buf);
            shared.frames.store(w.frames(), Ordering::Relaxed);
            shared.file_bytes.store(w.file_bytes(), Ordering::Relaxed);
            if let Err(e) = result {
                fail(format!("writing the take stopped: {e}"));
                break;
            }
        }
        if last_header.elapsed() >= HEADER_INTERVAL {
            if let Err(e) = w.update_header() {
                fail(format!("updating the take's header failed: {e}"));
                break;
            }
            last_header = Instant::now();
        }
        if n == 0 {
            if stopping {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    match w.update_header() {
        Ok(()) => w.finish_clean(),
        Err(e) => fail(format!("finishing the take's header failed: {e}")),
    }
    w
}

/// A generated input: a sine on every channel (one tone per input, see [`FakeSpec`]), produced on a timer thread at the take's rate.
/// Stands in for a microphone in tests and scripted runs (`WAVCRAFT_FAKE_INPUT=sine:440`, or `sine:440+660+880+1100` for four inputs).
pub struct FakeInput {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// What `WAVCRAFT_FAKE_INPUT` asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeSpec {
    /// The tone on each input of the fake device (`sine:440+660+880`): input `k` plays tone `k`.
    /// A single tone plays on every input.
    pub tones: Vec<f64>,
    /// `ring=<ms>`: the queue holds only this much (default: two seconds).
    pub ring_ms: Option<u32>,
    /// `stall=<ms>`: the writer waits this long before it starts, so a small ring overflows.
    pub stall_ms: u32,
}

impl FakeSpec {
    /// `sine:440`, optionally with `,ring=<ms>` and `,stall=<ms>` (an overflow on purpose, C035).
    /// `Ok(None)` when the variable is unset or empty.
    pub fn parse(text: Option<&str>) -> Result<Option<FakeSpec>, String> {
        let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(None) };
        let bad = || format!("{ENV_FAKE_INPUT}={text:?}: expected sine:<hz>[+<hz>...][,ring=<ms>][,stall=<ms>]");
        let mut parts = text.split(',');
        let tones = parts
            .next()
            .and_then(|p| p.strip_prefix("sine:"))
            .and_then(|v| v.split('+').map(|t| t.trim().parse::<f64>().ok().filter(|h| h.is_finite() && *h > 0.0)).collect::<Option<Vec<_>>>())
            .filter(|t| !t.is_empty() && t.len() <= MAX_INPUTS as usize)
            .ok_or_else(bad)?;
        let mut spec = FakeSpec { tones, ring_ms: None, stall_ms: 0 };
        for p in parts {
            match p.trim().split_once('=') {
                Some(("ring", v)) => spec.ring_ms = Some(v.trim().parse::<u32>().ok().filter(|v| *v > 0).ok_or_else(bad)?),
                Some(("stall", v)) => spec.stall_ms = v.trim().parse().map_err(|_| bad())?,
                _ => return Err(bad()),
            }
        }
        Ok(Some(spec))
    }

    /// The tone of each channel of a take recorded as `fmt`: channel `k` carries the tone of the
    /// device input it was picked from. A fake device with several tones has that many inputs.
    pub fn tones_for(&self, fmt: RecFormat) -> Result<Vec<f64>, String> {
        fmt.pick()
            .indices()
            .iter()
            .map(|&i| match (self.tones.len(), self.tones.get(usize::from(i))) {
                (1, _) => Ok(self.tones[0]),
                (_, Some(t)) => Ok(*t),
                (n, None) => Err(format!("the fake input has {n} inputs, and input {} was asked for", i + 1)),
            })
            .collect()
    }

    pub fn from_env() -> Result<Option<FakeSpec>, String> {
        FakeSpec::parse(std::env::var(ENV_FAKE_INPUT).ok().as_deref())
    }
}

impl FakeInput {
    /// Feed `ring` from a timer thread, in step with the clock (a slow wake-up catches up).
    pub fn spawn(spec: FakeSpec, fmt: RecFormat, ring: Arc<Ring>) -> io::Result<FakeInput> {
        let tones = spec.tones_for(fmt).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new().name("fake input".into()).spawn(move || {
            let (rate, ch) = (f64::from(fmt.rate), usize::from(fmt.channels));
            let start = Instant::now();
            let mut made: u64 = 0;
            let mut chunk = vec![0.0f32; 480 * ch];
            while !flag.load(Ordering::Relaxed) {
                let due = (start.elapsed().as_secs_f64() * rate) as u64;
                while made < due && !flag.load(Ordering::Relaxed) {
                    let frames = ((due - made) as usize).min(480);
                    for f in 0..frames {
                        for (k, hz) in tones.iter().enumerate() {
                            chunk[f * ch + k] = (std::f64::consts::TAU * hz * (made + f as u64) as f64 / rate).sin() as f32 * 0.5;
                        }
                    }
                    ring.push(&chunk[..frames * ch]);
                    made += frames as u64;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        })?;
        Ok(FakeInput { stop, handle: Some(handle) })
    }

    /// Stop producing and wait for the thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for FakeInput {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-record-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// (channels, rate, bits, data size from the header) read straight from the bytes.
    fn header_of(path: &Path) -> (u16, u32, u16, u32, u64) {
        let b = std::fs::read(path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(&b[36..40], b"data");
        let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        assert_eq!(u32at(4), 36 + u32at(40), "RIFF size matches the data size");
        (u16at(22), u32at(24), u16at(34), u32at(40), b.len() as u64)
    }

    #[test]
    fn monitor_ring_passes_frames_in_order_and_zero_fills() {
        let m = MonitorRing::new(8, 2);
        assert_eq!(m.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 3);
        let mut out = [9.0f32; 8];
        assert_eq!(m.pop_into(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0, 0.0]);
        assert_eq!(m.played_frames(), 3);
        assert_eq!(m.underrun_frames(), 1, "asked for four frames, three were there");
        assert_eq!(m.overflow_frames(), 0);
    }

    #[test]
    fn monitor_ring_underrun_is_silence_and_not_counted_before_the_input_starts() {
        let m = MonitorRing::new(16, 1);
        let mut out = [7.0f32; 4];
        assert_eq!(m.pop_into(&mut out), 0);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(m.underrun_frames(), 0, "the input has not started");
        m.push(&[0.5]);
        let mut one = [0.0f32; 1];
        m.pop_into(&mut one);
        m.pop_into(&mut out);
        assert_eq!(m.underrun_frames(), 4, "now the output is ahead of the input");
    }

    #[test]
    fn monitor_ring_overflow_drops_new_frames_and_counts_them() {
        let m = MonitorRing::new(4, 1);
        assert_eq!(m.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 4);
        assert_eq!(m.overflow_frames(), 2);
        assert_eq!(m.push(&[7.0]), 0, "full: nothing waits for room");
        assert_eq!(m.overflow_frames(), 3);
    }

    #[test]
    fn monitor_ring_skips_the_oldest_when_the_output_stalls_so_the_delay_stays_bounded() {
        let m = MonitorRing::new(8, 1);
        let ramp: Vec<f32> = (0..8).map(|i| i as f32).collect();
        m.push(&ramp);
        let mut out = [0.0f32; 2];
        // Eight waiting is above three quarters (6): skip down to a quarter (2), then play them.
        assert_eq!(m.pop_into(&mut out), 2);
        assert_eq!(out, [6.0, 7.0], "the newest frames are the ones kept");
        assert_eq!(m.overflow_frames(), 6);
    }

    #[test]
    fn monitor_ring_between_two_threads_keeps_order() {
        let m = MonitorRing::new(64, 1);
        let producer = {
            let m = m.clone();
            std::thread::spawn(move || {
                let mut next = 0u32;
                while next < 20_000 {
                    if m.push(&[next as f32]) == 1 {
                        next += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            })
        };
        let (mut last, mut out) = (-1.0f32, [0.0f32; 16]);
        let started = std::time::Instant::now();
        while last < 19_999.0 && started.elapsed() < Duration::from_secs(20) {
            let got = m.pop_into(&mut out);
            for v in &out[..got] {
                assert!(*v > last, "order kept (skips allowed, repeats and reorders not): {v} after {last}");
                last = *v;
            }
            std::thread::yield_now();
        }
        producer.join().unwrap();
    }

    #[test]
    fn a_ring_with_a_monitor_copies_even_while_paused_and_reports_latency() {
        let m = MonitorRing::new(16, 2);
        let ring = Ring::with_monitor(16, 2, Some(m.clone()));
        ring.set_paused(true);
        assert_eq!(ring.push(&[0.1, 0.2, 0.3, 0.4]), 0, "the take discards while paused");
        let mut out = [0.0f32; 4];
        assert_eq!(m.pop_into(&mut out), 2, "the monitor still hears the input");
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(ring.reported_latency_us(), None);
        ring.set_reported_latency_us(12_500);
        assert_eq!(ring.reported_latency_us(), Some(12_500));
        // A monitor with the wrong channel count is not attached.
        let odd = Ring::with_monitor(16, 1, Some(m.clone()));
        odd.push(&[0.9]);
        assert_eq!(m.pop_into(&mut out), 0);
    }

    #[test]
    fn ring_keeps_order_wraps_and_drops_whole_frames() {
        let ring = Ring::new(4, 2);
        assert_eq!(ring.push(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0]), 3);
        // One frame of room: of two pushed, one is kept and one dropped.
        assert_eq!(ring.push(&[4.0, -4.0, 5.0, -5.0]), 1);
        assert_eq!(ring.dropped_frames(), 1);
        let mut out = Vec::new();
        assert_eq!(ring.pop(&mut out, 3), 3);
        assert_eq!(out, [1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        // Wrap around the end of the storage; an odd trailing sample is ignored, not queued.
        assert_eq!(ring.push(&[6.0, -6.0, 7.0, -7.0, 8.0]), 2);
        out.clear();
        assert_eq!(ring.pop(&mut out, 10), 3);
        assert_eq!(out, [4.0, -4.0, 6.0, -6.0, 7.0, -7.0]);
        assert!(ring.is_empty());
        assert_eq!(ring.take_peak(), 7.0);
        assert_eq!(ring.take_peak(), 0.0);
    }

    #[test]
    fn ring_between_two_threads_loses_and_reorders_nothing() {
        let ring = Ring::new(1024, 1);
        let producer = ring.clone();
        let total = 200_000usize;
        let t = std::thread::spawn(move || {
            let mut i = 0usize;
            while i < total {
                let end = (i + 100).min(total);
                let block: Vec<f32> = (i..end).map(|v| v as f32).collect();
                let n = producer.push(&block);
                i += n;
                if n < block.len() {
                    std::thread::yield_now();
                }
            }
        });
        let mut got = 0usize;
        let mut out = Vec::new();
        while got < total {
            out.clear();
            let n = ring.pop(&mut out, 512);
            for (k, v) in out.iter().enumerate() {
                assert_eq!(*v, (got + k) as f32);
            }
            got += n;
        }
        t.join().unwrap();
    }

    #[test]
    fn rec_format_parses_and_refuses() {
        assert_eq!(RecFormat::parse_args("").unwrap(), RecFormat { rate: 48_000, channels: 1, bits: 24, inputs: 0 });
        assert_eq!(RecFormat::parse_args("ch=2 bits=16 rate=44100").unwrap(), RecFormat { rate: 44_100, channels: 2, bits: 16, inputs: 0 });
        assert!(RecFormat::parse_args("bits=20").is_err());
        assert!(RecFormat::parse_args("ch=0").is_err());
        assert!(RecFormat::parse_args("rate=5").is_err());
        assert!(RecFormat::parse_args("rate=abc").is_err());
        assert!(RecFormat::parse_args("speed=3").is_err());
        assert_eq!(RecFormat::default().bytes_per_second(), 144_000);
    }

    #[test]
    fn take_names_are_utc_dates_and_never_collide() {
        assert_eq!(take_stem(0), "Take 1970-01-01 00.00.00");
        assert_eq!(take_stem(1_792_505_002), "Take 2026-10-20 14.03.22");
        assert_eq!(take_stem(951_782_400 + 86_399), "Take 2000-02-29 23.59.59");
        let dir = scratch("names");
        let a = create_take(&dir, RecFormat::default(), 1_792_505_002).unwrap();
        let b = create_take(&dir, RecFormat::default(), 1_792_505_002).unwrap();
        assert_ne!(a.path(), b.path());
        assert!(b.path().to_string_lossy().ends_with("Take 2026-10-20 14.03.22 (2).wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_is_never_overwritten() {
        let dir = scratch("exclusive");
        let path = dir.join("x.wav");
        std::fs::write(&path, b"precious").unwrap();
        assert!(TakeWriter::create(&path, RecFormat::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"precious");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// C022: the header is valid from the moment the file exists, and after each update it
    /// covers everything written; a copy of the file taken then (what a kill leaves) reads whole.
    #[test]
    fn the_header_is_valid_at_every_moment() {
        let dir = scratch("header");
        let fmt = RecFormat { rate: 48_000, channels: 2, bits: 24, inputs: 0 };
        let mut w = create_take(&dir, fmt, 5).unwrap();
        assert_eq!(header_of(w.path()), (2, 48_000, 24, 0, 44), "valid and empty at once");
        w.write_frames(&vec![0.25; 2 * 1000]).unwrap();
        // Before an update the header still says 0: valid, but short.
        assert_eq!(header_of(w.path()).3, 0);
        w.update_header().unwrap();
        assert_eq!(header_of(w.path()), (2, 48_000, 24, 6_000, 44 + 6_000));
        // More data after the update: a kill now leaves the file longer than its header says,
        // and the header still describes a whole number of frames.
        w.write_frames(&vec![0.25; 2 * 500]).unwrap();
        let killed = dir.join("killed.wav");
        std::fs::copy(w.path(), &killed).unwrap();
        let (_, _, _, data, len) = header_of(&killed);
        assert_eq!(data, 6_000);
        assert_eq!(len, 44 + 9_000);
        assert_eq!(u64::from(data) % fmt.block_align(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn samples_are_converted_at_each_depth() {
        for (bits, width) in [(16u16, 2usize), (24, 3), (32, 4)] {
            let dir = scratch(&format!("depth{bits}"));
            let mut w = create_take(&dir, RecFormat { rate: 48_000, channels: 1, bits, inputs: 0 }, 1).unwrap();
            w.write_frames(&[1.0, -1.0, 0.5, 0.0, 2.0, f32::NAN]).unwrap();
            w.update_header().unwrap();
            let b = std::fs::read(w.path()).unwrap();
            let d = &b[44..];
            assert_eq!(d.len(), 6 * width);
            let at = |i: usize| -> f64 {
                let s = &d[i * width..(i + 1) * width];
                match bits {
                    16 => f64::from(i16::from_le_bytes([s[0], s[1]])) / 32767.0,
                    24 => f64::from(i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) / 8_388_607.0,
                    _ => f64::from(f32::from_le_bytes([s[0], s[1], s[2], s[3]])),
                }
            };
            let tol = if bits == 32 { 0.0 } else { 1e-4 };
            let want = if bits == 32 { [1.0, -1.0, 0.5, 0.0, 2.0, 0.0] } else { [1.0, -1.0, 0.5, 0.0, 1.0, 0.0] };
            for (i, w) in want.iter().enumerate() {
                assert!((at(i) - w).abs() <= tol, "{bits}-bit sample {i}: {} vs {w}", at(i));
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_pause_discards_input_without_counting_a_drop() {
        let ring = Ring::new(100, 2);
        assert_eq!(ring.push(&[0.1; 20]), 10);
        ring.set_paused(true);
        assert!(ring.is_paused());
        assert_eq!(ring.push(&[0.9; 20]), 0);
        assert_eq!((ring.dropped_frames(), ring.accepted_frames()), (0, 10));
        ring.set_paused(false);
        assert_eq!(ring.push(&[0.2; 10]), 5);
        assert_eq!(ring.accepted_frames(), 15);
        let mut out = Vec::new();
        assert_eq!(ring.pop(&mut out, 100), 15);
        assert!(out[..20].iter().all(|v| *v == 0.1) && out[20..].iter().all(|v| *v == 0.2), "the paused stretch is absent");
    }

    #[test]
    fn a_paused_take_holds_only_what_was_recorded() {
        let dir = scratch("paused");
        let fmt = RecFormat { rate: 48_000, channels: 1, bits: 16, inputs: 0 };
        let ring = Ring::new(48_000, 1);
        let rec = Recorder::start(create_take(&dir, fmt, 11).unwrap(), ring.clone()).unwrap();
        ring.push(&[0.5; 4_800]);
        ring.set_paused(true);
        ring.push(&[0.5; 9_600]);
        ring.set_paused(false);
        let resume_at = ring.accepted_frames();
        ring.push(&[0.5; 2_400]);
        let take = rec.stop();
        assert_eq!((take.frames, take.dropped_frames, resume_at), (7_200, 0, 4_800));
        assert_eq!(header_of(&take.path).3, 14_400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// C035: a writer that falls behind a small ring loses frames, counts them, and the take
    /// is still a readable WAV holding exactly the frames that were kept.
    #[test]
    fn an_overflow_is_counted_and_the_take_stays_readable() {
        let dir = scratch("overflow");
        let fmt = RecFormat { rate: 48_000, channels: 1, bits: 16, inputs: 0 };
        let ring = Ring::new(1_000, 1);
        let rec = Recorder::start_stalled(create_take(&dir, fmt, 12).unwrap(), ring.clone(), Duration::from_millis(150)).unwrap();
        for _ in 0..10 {
            ring.push(&[0.5; 480]);
        }
        std::thread::sleep(Duration::from_millis(200));
        ring.push(&[0.5; 480]);
        let take = rec.stop();
        assert_eq!(take.dropped_frames, 4_800 - 1_000);
        assert_eq!(take.frames + take.dropped_frames, 5_280);
        assert_eq!(u64::from(header_of(&take.path).3), take.frames * 2);
        assert_eq!(take.error, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fake_input_spec_takes_overflow_knobs() {
        assert_eq!(FakeSpec::parse(Some("sine:440")).unwrap(), Some(FakeSpec { tones: vec![440.0], ring_ms: None, stall_ms: 0 }));
        assert_eq!(
            FakeSpec::parse(Some("sine:220,ring=50,stall=1500")).unwrap(),
            Some(FakeSpec { tones: vec![220.0], ring_ms: Some(50), stall_ms: 1500 })
        );
        assert!(FakeSpec::parse(Some("sine:220,ring=0")).is_err());
        assert!(FakeSpec::parse(Some("sine:220,speed=1")).is_err());
        assert!(FakeSpec::parse(Some("saw:220")).is_err());
        assert_eq!(FakeSpec::parse(Some("")).unwrap(), None);
    }

    #[test]
    fn the_recorder_drains_the_ring_and_finishes_the_header() {
        let dir = scratch("recorder");
        let fmt = RecFormat { rate: 48_000, channels: 1, bits: 16, inputs: 0 };
        let ring = Ring::new(48_000, 1);
        let rec = Recorder::start(create_take(&dir, fmt, 9).unwrap(), ring.clone()).unwrap();
        for _ in 0..10 {
            ring.push(&[0.5; 480]);
            std::thread::sleep(Duration::from_millis(3));
        }
        let take = rec.stop();
        assert_eq!((take.frames, take.dropped_frames, take.error.clone()), (4_800, 0, None));
        assert_eq!(header_of(&take.path), (1, 48_000, 16, 9_600, 44 + 9_600));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_the_recorder_still_finishes_the_take() {
        let dir = scratch("dropped");
        let ring = Ring::new(4_800, 1);
        let rec = Recorder::start(create_take(&dir, RecFormat::default(), 3).unwrap(), ring.clone()).unwrap();
        let path = rec.path().to_path_buf();
        ring.push(&[0.1; 1_000]);
        drop(rec);
        assert_eq!(header_of(&path).3, 3_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fake_input_spec_parses() {
        assert_eq!(FakeSpec::parse(None), Ok(None));
        assert_eq!(FakeSpec::parse(Some("")), Ok(None));
        assert_eq!(FakeSpec::parse(Some("sine:440")), Ok(Some(FakeSpec { tones: vec![440.0], ring_ms: None, stall_ms: 0 })));
        assert!(FakeSpec::parse(Some("sine:abc")).is_err());
        assert!(FakeSpec::parse(Some("noise")).is_err());
        assert!(FakeSpec::parse(Some("sine:-5")).is_err());
    }

    /// The whole path without a device: fake sine -> ring -> writer thread -> file. The take
    /// has about the wall-clock length and the sine's frequency, found by counting zero crossings.
    #[test]
    fn a_fake_sine_take_has_the_right_length_and_pitch() {
        let dir = scratch("fake");
        let fmt = RecFormat { rate: 48_000, channels: 1, bits: 24, inputs: 0 };
        let ring = Ring::new(48_000, 1);
        let rec = Recorder::start(create_take(&dir, fmt, 7).unwrap(), ring.clone()).unwrap();
        let fake = FakeInput::spawn(FakeSpec { tones: vec![440.0], ring_ms: None, stall_ms: 0 }, fmt, ring).unwrap();
        std::thread::sleep(Duration::from_millis(1_200));
        fake.stop();
        let take = rec.stop();
        assert_eq!(take.error, None);
        assert_eq!(take.dropped_frames, 0);
        let seconds = take.frames as f64 / 48_000.0;
        assert!((1.0..1.6).contains(&seconds), "{seconds} s");
        let (_, _, _, data, _) = header_of(&take.path);
        assert_eq!(u64::from(data), take.frames * 3);
        let bytes = std::fs::read(&take.path).unwrap();
        let samples: Vec<i32> = bytes[44..].as_chunks::<3>().0.iter().map(|s| i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8).collect();
        let crossings = samples.windows(2).filter(|p| p[0] <= 0 && p[1] > 0).count() as f64;
        let hz = crossings / seconds;
        assert!((hz - 440.0).abs() < 5.0, "{hz} Hz");
        let peak = samples.iter().map(|s| s.abs()).max().unwrap();
        assert!((3_900_000..4_300_000).contains(&peak), "peak {peak} is about half of full scale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inputs_are_parsed_and_channels_follow_them() {
        let f = RecFormat::parse_args("inputs=1,2,3,4").unwrap();
        assert_eq!((f.channels, f.inputs), (4, 0), "the first four inputs are the default pick");
        let f = RecFormat::parse_args("ch=2 inputs=3,4").unwrap();
        assert_eq!((f.channels, f.inputs, f.inputs_text().as_str()), (2, 0b1100, "3,4"));
        let f = RecFormat::parse_args("inputs=4,2").unwrap();
        assert_eq!(f.inputs_text(), "2,4", "the take's channels are in ascending input order");
        assert_eq!(RecFormat::parse_args("ch=4").unwrap().inputs_text(), "1,2,3,4");
        assert!(RecFormat::parse_args("ch=3 inputs=1,2").is_err(), "ch must match the choice");
        assert!(RecFormat::parse_args("inputs=0").is_err());
        assert!(RecFormat::parse_args("inputs=1,x").is_err());
        assert!(RecFormat::parse_args("inputs=1,2,3,4,5,6,7,8,9").is_err());
        assert!(RecFormat { rate: 48_000, channels: 2, bits: 16, inputs: 0b1 }.validate().is_err());
    }

    /// C034: a subset of a wider device lands in the take's channels in order.
    #[test]
    fn a_subset_of_the_device_inputs_is_gathered_in_order() {
        let fmt = RecFormat::parse_args("inputs=2,4").unwrap();
        let pick = fmt.pick();
        assert_eq!((pick.indices(), pick.needs_device_channels()), (&[1u8, 3][..], 4));
        // Four device channels, three frames: value = frame * 10 + channel.
        let src: Vec<f32> = (0..3).flat_map(|f| (0..4).map(move |c| (f * 10 + c) as f32)).collect();
        let mut out = [0.0f32; 6];
        assert_eq!(pick.gather(&src, 4, &mut out), 3);
        assert_eq!(out, [1.0, 3.0, 11.0, 13.0, 21.0, 23.0]);
        // Output room for two frames only: two frames are gathered, none overrun.
        let mut small = [0.0f32; 4];
        assert_eq!(pick.gather(&src, 4, &mut small), 2);
        // The default pick is the identity on the first channels.
        let id = RecFormat::parse_args("ch=2").unwrap().pick();
        let mut out2 = [0.0f32; 6];
        assert_eq!(id.gather(&src[..12], 4, &mut out2), 3);
        assert_eq!(out2, [0.0, 1.0, 10.0, 11.0, 20.0, 21.0]);
    }

    #[test]
    fn the_fake_input_gives_each_input_its_own_tone() {
        let spec = FakeSpec::parse(Some("sine:440+660+880+1100")).unwrap().unwrap();
        assert_eq!(spec.tones, [440.0, 660.0, 880.0, 1100.0]);
        let all = RecFormat::parse_args("inputs=1,2,3,4").unwrap();
        assert_eq!(spec.tones_for(all).unwrap(), [440.0, 660.0, 880.0, 1100.0]);
        let some = RecFormat::parse_args("inputs=2,4").unwrap();
        assert_eq!(spec.tones_for(some).unwrap(), [660.0, 1100.0]);
        assert!(spec.tones_for(RecFormat::parse_args("inputs=5").unwrap()).is_err(), "the fake device has four inputs");
        let one = FakeSpec::parse(Some("sine:440")).unwrap().unwrap();
        assert_eq!(one.tones_for(all).unwrap(), [440.0; 4], "one tone plays on every input");
        assert!(FakeSpec::parse(Some("sine:440+")).is_err());
    }

    /// C034 + C022: a 4-channel take through the writer thread holds each channel's own tone,
    /// and a copy of the file taken mid-take (what a kill leaves) is a readable 4-channel WAV.
    #[test]
    fn a_four_channel_take_keeps_every_tone_and_survives_a_kill() {
        let dir = scratch("four");
        let fmt = RecFormat::parse_args("rate=48000 bits=16 inputs=1,2,3,4").unwrap();
        let ring = Ring::new(48_000, 4);
        let rec = Recorder::start(create_take(&dir, fmt, 8).unwrap(), ring.clone()).unwrap();
        let spec = FakeSpec::parse(Some("sine:400+800+1200+2400")).unwrap().unwrap();
        let fake = FakeInput::spawn(spec, fmt, ring).unwrap();
        // Past one header interval, so the on-disk header has been rewritten at least once.
        std::thread::sleep(HEADER_INTERVAL + Duration::from_millis(500));
        let killed = dir.join("killed.wav");
        std::fs::copy(rec.path(), &killed).unwrap();
        let (ch, rate, bits, data, _) = header_of(&killed);
        assert_eq!((ch, rate, bits), (4, 48_000, 16));
        assert!(data >= 4 * 2 * 40_000 && u64::from(data) % fmt.block_align() == 0, "{data} data bytes, whole frames");
        fake.stop();
        let take = rec.stop();
        assert_eq!((take.error.clone(), take.dropped_frames), (None, 0));
        assert_eq!(header_of(&take.path).0, 4);
        let bytes = std::fs::read(&take.path).unwrap();
        let samples: Vec<i16> = bytes[44..].as_chunks::<2>().0.iter().map(|s| i16::from_le_bytes(*s)).collect();
        assert_eq!(samples.len() as u64, take.frames * 4);
        let seconds = take.frames as f64 / 48_000.0;
        for (k, want) in [400.0, 800.0, 1200.0, 2400.0].into_iter().enumerate() {
            let crossings = samples.chunks(4).map(|f| f[k]).collect::<Vec<_>>().windows(2).filter(|p| p[0] <= 0 && p[1] > 0).count() as f64;
            let hz = crossings / seconds;
            assert!((hz - want).abs() < want * 0.02 + 3.0, "channel {} is {hz} Hz, wanted {want}", k + 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_slow_writer_shows_as_dropped_frames_not_a_stall() {
        let ring = Ring::new(100, 1);
        for _ in 0..5 {
            ring.push(&[0.0; 50]);
        }
        assert_eq!(ring.dropped_frames(), 150);
    }
}
