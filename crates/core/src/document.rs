//! The open file: the document in the session store, and per-channel waveform peaks.
//!
//! A file is opened by streaming it into the session store ([`crate::io::open`]). Since task 05
//! nothing reads it back whole: the waveform, the spectrogram and the player read frames through
//! [`DocState::read`] in bounded chunks, so memory stays flat however long the file is. What a
//! document keeps in RAM besides the store's block cache is the peaks, about a third of a byte a
//! sample.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};

use crate::doc::{Channel, DocState, Piece, SourceFormat};
pub use crate::io::OpenError;
use crate::io::OpenOptions;
use crate::spectrogram::Samples;
use crate::store::{BLOCK_FRAMES, Store, StoreOptions};

/// Samples per level-0 block of the peaks.
pub const PEAK_BLOCK: usize = 32;
/// Blocks of one level merged into a block of the next.
const FANOUT: usize = 4;
/// Frames read at a time to build the peaks (a multiple of [`PEAK_BLOCK`]).
const PEAK_CHUNK: usize = BLOCK_FRAMES;
/// Columns a span of at least this many frames is drawn from the peaks; closer in, from samples.
const PEAK_MIN_SPAN: f64 = 64.0;

const EMPTY: (f32, f32) = (f32::INFINITY, f32::NEG_INFINITY);

fn merge(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn finish(acc: (f32, f32)) -> (f32, f32) {
    if acc.0 > acc.1 { (0.0, 0.0) } else { acc }
}

/// Multi-resolution min/max overview of one channel, for drawing at any zoom. The same layout and
/// answers as `soundcraft_audio_io::peaks::Peaks` (the tests hold them equal), built from
/// chunked reads of the store instead of one slice of the whole channel. NaN samples are ignored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WavePeaks {
    len: usize,
    levels: Vec<Vec<(f32, f32)>>,
}

impl WavePeaks {
    /// Build from channel `ch` of `state`, a chunk at a time.
    pub fn build(state: &DocState, ch: usize) -> std::io::Result<WavePeaks> {
        let frames = usize::try_from(state.len_frames()).map_err(|_| std::io::Error::other("document too long"))?;
        let mut level0 = Vec::with_capacity(frames.div_ceil(PEAK_BLOCK));
        let mut chunk = vec![0.0f32; PEAK_CHUNK.min(frames.max(1))];
        let mut at = 0usize;
        while at < frames {
            let want = chunk.len().min(frames - at);
            let buf = chunk.get_mut(..want).ok_or_else(|| std::io::Error::other("peak buffer"))?;
            let got = state.read(ch, at as u64, buf)?;
            if got != want {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the document ended early"));
            }
            level0.extend(buf.chunks(PEAK_BLOCK).map(|c| finish(c.iter().fold(EMPTY, |acc, &s| merge(acc, (s, s))))));
            at += want;
        }
        Ok(WavePeaks::from_level0(frames, level0))
    }

    /// Build from samples already in memory (tests).
    pub fn from_samples(samples: &[f32]) -> WavePeaks {
        let level0 = samples.chunks(PEAK_BLOCK).map(|c| finish(c.iter().fold(EMPTY, |acc, &s| merge(acc, (s, s))))).collect();
        WavePeaks::from_level0(samples.len(), level0)
    }

    fn from_level0(len: usize, level0: Vec<(f32, f32)>) -> WavePeaks {
        let mut levels = Vec::new();
        if level0.is_empty() {
            return WavePeaks { len: 0, levels };
        }
        levels.push(level0);
        while let Some(last) = levels.last() {
            if last.len() <= 1 {
                break;
            }
            let next: Vec<(f32, f32)> = last.chunks(FANOUT).map(|c| c.iter().fold(EMPTY, |acc, &b| merge(acc, b))).collect();
            levels.push(next);
        }
        WavePeaks { len, levels }
    }

    /// Samples the overview was built from.
    pub fn len_samples(&self) -> usize {
        self.len
    }

    /// The overview as plain data (samples, then each level's (min, max)), for the peak cache.
    pub fn to_parts(&self) -> (usize, &[Vec<(f32, f32)>]) {
        (self.len, &self.levels)
    }

    /// Rebuild from [`to_parts`](Self::to_parts). `None` when the levels cannot be for `len`
    /// samples (a corrupt cache file).
    pub fn from_parts(len: usize, levels: Vec<Vec<(f32, f32)>>) -> Option<WavePeaks> {
        let level0_ok = match levels.first() {
            None => len == 0,
            Some(l0) => l0.len() == len.div_ceil(PEAK_BLOCK),
        };
        level0_ok.then_some(WavePeaks { len, levels })
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(|l| l.len() * std::mem::size_of::<(f32, f32)>()).sum()
    }

    /// (min, max) over `[start, end)`, clamped to the data; blocks at the edges contribute whole,
    /// so the result always contains the true range. `(0, 0)` for an empty range.
    pub fn minmax(&self, start: usize, end: usize) -> (f32, f32) {
        let end = end.min(self.len);
        if start >= end {
            return (0.0, 0.0);
        }
        let mut lo = start / PEAK_BLOCK;
        let mut hi = end.div_ceil(PEAK_BLOCK);
        let mut acc = EMPTY;
        let mut lvl = 0usize;
        while lo < hi {
            let Some(level) = self.levels.get(lvl) else { break };
            let has_parent = lvl + 1 < self.levels.len();
            if !has_parent || hi - lo < 2 * FANOUT {
                for b in level.get(lo..hi.min(level.len())).unwrap_or_default() {
                    acc = merge(acc, *b);
                }
                break;
            }
            while !lo.is_multiple_of(FANOUT) && lo < hi {
                acc = merge(acc, level.get(lo).copied().unwrap_or(EMPTY));
                lo += 1;
            }
            while !hi.is_multiple_of(FANOUT) && hi > lo {
                hi -= 1;
                acc = merge(acc, level.get(hi).copied().unwrap_or(EMPTY));
            }
            lo /= FANOUT;
            hi /= FANOUT;
            lvl += 1;
        }
        finish(acc)
    }

    /// Per-column (min, max) for `cols` columns over `[start, end)`; columns outside the data are
    /// `(0, 0)`. Each column covers at least one sample.
    pub fn columns(&self, start: f64, end: f64, cols: usize) -> Vec<(f32, f32)> {
        let cols = cols.min(1 << 20);
        let mut out = vec![(0.0, 0.0); cols];
        if cols == 0 || !start.is_finite() || !end.is_finite() || end <= start || self.len == 0 {
            return out;
        }
        let span = (end - start) / cols as f64;
        let len = self.len as f64;
        for (c, slot) in out.iter_mut().enumerate() {
            let s = start + span * c as f64;
            let e = start + span * (c + 1) as f64;
            if e <= 0.0 || s >= len {
                continue;
            }
            let si = s.max(0.0).floor();
            let ei = e.min(len).ceil().max(si + 1.0);
            *slot = self.minmax(si as usize, ei as usize);
        }
        out
    }
}

/// One channel of a [`DocState`] as [`Samples`] for the spectrogram: reads go through the store's
/// block cache. A read that fails gives silence (and one log line), never a panic.
pub struct StoreChannel<'a> {
    state: &'a DocState,
    ch: usize,
    complained: AtomicBool,
}

impl<'a> StoreChannel<'a> {
    pub fn new(state: &'a DocState, ch: usize) -> StoreChannel<'a> {
        StoreChannel { state, ch, complained: AtomicBool::new(false) }
    }
}

impl Samples for StoreChannel<'_> {
    fn len(&self) -> usize {
        usize::try_from(self.state.len_frames()).unwrap_or(usize::MAX)
    }

    fn read(&self, start: usize, out: &mut [f32]) {
        let got = match self.state.read(self.ch, start as u64, out) {
            Ok(n) => n,
            Err(e) => {
                if !self.complained.swap(true, Ordering::Relaxed) {
                    log::warn!(target: "wavcraft", "reading channel {} from the store failed: {e}", self.ch);
                }
                0
            }
        };
        if let Some(rest) = out.get_mut(got..) {
            rest.fill(0.0);
        }
    }
}

/// One audio file open in the session store.
#[derive(Debug, Clone)]
pub struct Document {
    /// Where the file came from (empty for a document made in memory).
    pub path: PathBuf,
    /// What the file said about itself: format, bit depth, rate, channels.
    pub info: AudioInfo,
    /// The document in the session store: the one place the samples live.
    pub state: DocState,
    /// One waveform overview per channel, for drawing at any zoom.
    pub peaks: Vec<WavePeaks>,
    /// How the overviews were made at open (see [`PeakReport`]).
    pub peak_report: PeakReport,
}

/// How a document's waveform overviews came about, for the status line and scripted checks.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakReport {
    /// `off` (no cache folder, or not opened from a file), `hit` (loaded from the cache), or
    /// `miss`, `stale`, `corrupt` (computed, and saved for next time).
    pub result: &'static str,
    /// Milliseconds spent loading (on a hit) or computing the overviews.
    pub ms: f64,
    /// Why the cache file could not be written, if it could not.
    pub write_error: Option<String>,
}

impl PeakReport {
    fn off(ms: f64) -> PeakReport {
        PeakReport { result: "off", ms, write_error: None }
    }
}

impl Document {
    /// Stream `path` into a new session store.
    pub fn open(path: &Path) -> Result<Document, OpenError> {
        Document::open_with(path, &mut |_| {}, &AtomicBool::new(false))
    }

    /// [`open`](Self::open) with progress (0 to 1, for the streaming part) and a cancel flag.
    pub fn open_with(path: &Path, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Document, OpenError> {
        Document::open_in(path, &OpenOptions::default(), progress, cancel)
    }

    /// [`open_with`](Self::open_with) with the session folder and store set by `opts`.
    pub fn open_in(path: &Path, opts: &OpenOptions, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Document, OpenError> {
        // The key is taken before the file is read, so a file changed while it opens is stale next time.
        let key = opts.peak_cache.as_ref().and_then(|dir| Some((dir, crate::peakcache::Key::of_file(path).ok()?)));
        let state = crate::io::open(path, opts, progress, cancel)?;
        Document::from_state_cached(path.to_path_buf(), state, key.as_ref().map(|(d, k)| (d.as_path(), k)))
    }

    /// File > Import > Raw Data: [`open_in`](Self::open_in) for a headerless file described by `spec`.
    pub fn open_raw_in(
        path: &Path,
        spec: &crate::io::RawSpec,
        opts: &OpenOptions,
        progress: &mut dyn FnMut(f64),
        cancel: &AtomicBool,
    ) -> Result<Document, OpenError> {
        let state = crate::io::open_raw(path, spec, opts, progress, cancel)?;
        Document::from_state(path.to_path_buf(), state)
    }

    /// The document for `state` (after an edit, or an undo): its peaks are built by reading the
    /// store a chunk at a time, one thread a channel.
    pub fn from_state(path: PathBuf, state: DocState) -> Result<Document, OpenError> {
        Document::from_state_cached(path, state, None)
    }
    /// [`from_state`](Self::from_state) with a peak cache (folder and key): the overviews are
    /// loaded from it when it holds them for this version of the file, otherwise computed and saved.
    pub fn from_state_cached(path: PathBuf, state: DocState, cache: Option<(&Path, &crate::peakcache::Key)>) -> Result<Document, OpenError> {
        if state.len_frames() == 0 || state.sample_rate() == 0 {
            return Err(OpenError::Empty);
        }
        let src = state.source();
        let info = AudioInfo {
            format: src.format,
            sample_format: src.sample_format,
            sample_rate: state.sample_rate(),
            channels: u16::try_from(state.channel_count()).unwrap_or(u16::MAX),
            frames: state.len_frames(),
            bwf: src.bwf.clone(),
        };
        let frames = state.len_frames();
        let channels = state.channel_count();
        let started = std::time::Instant::now();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut report = PeakReport::off(0.0);
        let mut cached = None;
        if let Some((dir, key)) = cache {
            match crate::peakcache::load(dir, key, channels, frames) {
                Ok(peaks) => {
                    report = PeakReport { result: "hit", ms: ms(started), write_error: None };
                    cached = Some(peaks);
                }
                Err(miss) => report.result = miss.word(),
            }
        }
        let peaks = match cached {
            Some(peaks) => peaks,
            None => {
                let t = std::time::Instant::now();
                let built: Vec<std::io::Result<WavePeaks>> = std::thread::scope(|s| {
                    let state = &state;
                    let jobs: Vec<_> = (0..state.channel_count()).map(|ch| (ch, s.spawn(move || WavePeaks::build(state, ch)))).collect();
                    jobs.into_iter()
                        .map(|(ch, job)| {
                            job.join().unwrap_or_else(|_| Err(std::io::Error::other(format!("the peaks thread of channel {ch} panicked"))))
                        })
                        .collect()
                });
                let built = built.into_iter().collect::<Result<Vec<_>, _>>().map_err(|e| OpenError::Io(e.to_string()))?;
                report.ms = ms(t);
                if let Some((dir, key)) = cache {
                    report.write_error =
                        crate::peakcache::store(dir, key, frames, &built, crate::peakcache::DEFAULT_CAP_BYTES).err().map(|e| e.to_string());
                }
                built
            }
        };
        Ok(Document { path, info, state, peaks, peak_report: report })
    }

    /// Decode a file already in memory into a new session store. `path` supplies the extension
    /// hint and the display name.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Document, OpenError> {
        let ext = path.extension().and_then(|e| e.to_str());
        let (info, audio) = soundcraft_audio_io::decode(bytes, ext).map_err(|e| OpenError::Decode(e.to_string()))?;
        Document::from_audio(path.to_path_buf(), info, &audio)
    }

    /// Wrap audio that is already decoded (new files, tests): it is written into a session store
    /// under the system temp folder.
    pub fn from_buffer(audio: AudioBuffer) -> Result<Document, OpenError> {
        let info = AudioInfo {
            format: FileFormat::Wav,
            sample_format: SampleFormat::Float32,
            sample_rate: audio.sample_rate,
            channels: u16::try_from(audio.num_channels()).unwrap_or(u16::MAX),
            frames: audio.frames() as u64,
            bwf: None,
        };
        Document::from_audio(PathBuf::new(), info, &audio)
    }

    fn from_audio(path: PathBuf, info: AudioInfo, audio: &AudioBuffer) -> Result<Document, OpenError> {
        if audio.frames() == 0 || audio.sample_rate == 0 {
            return Err(OpenError::Empty);
        }
        let io = |e: std::io::Error| OpenError::Io(e.to_string());
        let root = std::env::temp_dir().join("wavcraft-memory-sessions");
        let store = Store::create_session(Some(&root), StoreOptions::default()).map_err(io)?;
        let mut channels = Vec::with_capacity(audio.channels.len());
        for samples in &audio.channels {
            let mut channel = Channel::new();
            for chunk in samples.chunks(BLOCK_FRAMES) {
                let block = store.append(chunk).map_err(io)?;
                channel.push(Piece::whole(block).ok_or_else(|| OpenError::Io("empty block".to_string()))?);
            }
            channels.push(channel);
        }
        let channel_count = u16::try_from(channels.len()).unwrap_or(u16::MAX);
        let mut source = SourceFormat::synthetic(audio.sample_rate, channel_count);
        source.format = info.format;
        source.sample_format = info.sample_format;
        let state = DocState::new(store, channels, audio.sample_rate, source).map_err(|e| OpenError::Io(e.to_string()))?;
        Document::from_state(path, state)
    }

    /// The whole document as one in-memory buffer: for tests, the bench, and a multitrack clip
    /// (which holds its audio in memory by design). The waveform, the spectrogram and the player
    /// read the store in chunks and never call this.
    pub fn audio_buffer(&self) -> std::io::Result<AudioBuffer> {
        self.state.to_audio_buffer()
    }

    /// Length in frames.
    pub fn frames(&self) -> usize {
        usize::try_from(self.state.len_frames()).unwrap_or(usize::MAX)
    }

    /// Sample rate in Hz (never 0 for an open document).
    pub fn sample_rate(&self) -> u32 {
        self.state.sample_rate()
    }

    /// Number of channels.
    pub fn channels(&self) -> usize {
        self.state.channel_count()
    }

    /// Length in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.state.len_frames() as f64 / f64::from(self.state.sample_rate().max(1))
    }

    /// What was wrong with the file that still opened (a header claiming more data than is
    /// there, an unusual sample size…), for the app to show. Empty for a sound file or a
    /// document made in memory.
    pub fn warnings(&self) -> &[String] {
        self.state.source().warnings.as_slice()
    }

    /// The Properties panel's rows, `(label, value)`: the file's path, format, sample rate,
    /// channels, bit depth, duration and size on disk, then what the file carries (Broadcast Wave
    /// fields when present, and what was wrong with it). Audio made in memory has no path or size.
    pub fn properties(&self) -> Vec<(&'static str, String)> {
        let info = &self.info;
        let on_disk = (!self.path.as_os_str().is_empty()).then(|| std::fs::metadata(&self.path).ok()).flatten();
        let mut rows = vec![
            ("Path", if self.path.as_os_str().is_empty() { "(not saved)".to_string() } else { self.path.display().to_string() }),
            ("Format", format!("{:?}", info.format).to_uppercase()),
            ("Sample rate", format!("{} Hz", self.sample_rate())),
            ("Channels", self.channels().to_string()),
            ("Bit depth", bit_depth_text(info.sample_format).to_string()),
            ("Duration", format!("{} ({} frames)", crate::view::format_time(self.frames(), self.sample_rate()), self.frames())),
            ("Size", on_disk.map_or_else(|| "(no file)".to_string(), |m| format!("{} ({} bytes)", crate::space::format_bytes(m.len()), m.len()))),
        ];
        if let Some(b) = &info.bwf {
            for (label, v) in [
                ("BWF description", &b.description),
                ("BWF originator", &b.originator),
                ("BWF originator reference", &b.originator_reference),
                ("BWF origination date", &b.origination_date),
                ("BWF origination time", &b.origination_time),
            ] {
                if !v.is_empty() {
                    rows.push((label, v.clone()));
                }
            }
            if b.time_reference != 0 {
                rows.push(("BWF time reference", format!("{} samples", b.time_reference)));
            }
        }
        for w in self.warnings() {
            rows.push(("Warning", w.clone()));
        }
        rows
    }

    /// File name for a title bar ("Untitled" for a document made in memory).
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// `(min, max)` per pixel column for drawing channel `ch` over the frame range
    /// `[start, end)`. Zoomed in past the overview's resolution it reads just the frames on
    /// screen from the store, so single samples stay visible.
    pub fn waveform_columns(&self, ch: usize, start: f64, end: f64, cols: usize) -> Vec<(f32, f32)> {
        let silent = || vec![(0.0, 0.0); cols];
        let Some(peaks) = self.peaks.get(ch) else { return silent() };
        if cols == 0 || !start.is_finite() || !end.is_finite() || end <= start {
            return silent();
        }
        let per_col = (end - start) / cols as f64;
        if per_col >= PEAK_MIN_SPAN {
            return peaks.columns(start, end, cols);
        }
        let len = self.frames();
        // Only the frames this view shows are read: at most `cols * 64 + 1` of them.
        let first_frame = start.max(0.0).floor() as usize;
        let last_frame = (end.ceil().max(0.0) as usize).min(len);
        if first_frame >= last_frame {
            return silent();
        }
        let mut samples = vec![0.0f32; last_frame - first_frame];
        match self.state.read(ch, first_frame as u64, &mut samples) {
            Ok(n) => samples.truncate(n),
            Err(e) => {
                log::warn!(target: "wavcraft", "reading the waveform from the store failed: {e}");
                return silent();
            }
        }
        (0..cols)
            .map(|c| {
                let a = start + c as f64 * per_col;
                let b = a + per_col;
                if b <= 0.0 || a >= len as f64 {
                    return (0.0, 0.0);
                }
                let first = a.max(0.0).floor() as usize;
                let last = (b.ceil() as usize).clamp(first + 1, len);
                let span = samples.get(first.saturating_sub(first_frame)..last.saturating_sub(first_frame)).unwrap_or(&[]);
                let folded = span.iter().filter(|v| v.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |m, v| (m.0.min(*v), m.1.max(*v)));
                if folded.0 > folded.1 { (0.0, 0.0) } else { folded }
            })
            .collect()
    }
}

/// The bit depth as the Properties panel says it.
fn bit_depth_text(f: SampleFormat) -> &'static str {
    match f {
        SampleFormat::Int8 => "8-bit integer",
        SampleFormat::Int16 => "16-bit integer",
        SampleFormat::Int24 => "24-bit integer",
        SampleFormat::Int32 => "32-bit integer",
        SampleFormat::Float32 => "32-bit float",
        SampleFormat::Float64 => "64-bit float",
        SampleFormat::Compressed => "compressed (no fixed depth)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_audio_io::{BitDepth, EncodeOptions};

    fn ramp(frames: usize, channels: usize) -> AudioBuffer {
        AudioBuffer {
            sample_rate: 48_000,
            channels: (0..channels).map(|c| (0..frames).map(|i| (i as f32 / frames as f32) * if c == 0 { 1.0 } else { -1.0 }).collect()).collect(),
        }
    }

    #[test]
    fn a_wav_opens_with_its_format_and_one_overview_per_channel() {
        let buf = ramp(48_000, 2);
        let bytes =
            soundcraft_audio_io::encode(&buf, &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Float32, ..EncodeOptions::default() })
                .unwrap();
        let doc = Document::from_bytes(Path::new("take 1.wav"), &bytes).unwrap();
        assert_eq!((doc.frames(), doc.sample_rate(), doc.channels()), (48_000, 48_000, 2));
        assert_eq!(doc.info.sample_format, SampleFormat::Float32);
        assert_eq!(doc.peaks.len(), 2);
        assert_eq!(doc.name(), "take 1.wav");
        assert!((doc.duration_secs() - 1.0).abs() < 1e-9);
        assert_eq!(doc.audio_buffer().unwrap(), buf, "a float WAV round trip is exact");
    }

    /// The chunked peaks answer exactly as audio-io's whole-slice ones, over a length that is not a
    /// multiple of a block or a chunk, so the last chunk is short.
    #[test]
    fn chunked_peaks_equal_the_in_memory_peaks() {
        let frames = BLOCK_FRAMES * 2 + 12_345;
        let doc = Document::from_buffer(ramp(frames, 2)).unwrap();
        let audio = doc.audio_buffer().unwrap();
        for ch in 0..2 {
            let whole = WavePeaks::from_samples(&audio.channels[ch]);
            let mine = &doc.peaks[ch];
            assert_eq!(mine.len_samples(), whole.len_samples());
            for &(a, b) in &[(0, frames), (0, 32), (31, 33), (1_000, 99_999), (BLOCK_FRAMES - 5, BLOCK_FRAMES + 5), (frames - 7, frames)] {
                assert_eq!(mine.minmax(a, b), whole.minmax(a, b), "channel {ch} range {a}..{b}");
            }
            assert_eq!(mine.columns(0.0, frames as f64, 777), whole.columns(0.0, frames as f64, 777));
        }
    }

    /// The document holds no whole-channel copy: its peaks are a small fraction of the samples.
    #[test]
    fn the_document_keeps_only_peaks_not_the_samples() {
        let doc = Document::from_buffer(ramp(1_000_000, 1)).unwrap();
        assert!(doc.peaks[0].bytes() < 1_000_000 * 4 / 8, "{} bytes of peaks", doc.peaks[0].bytes());
    }

    #[test]
    fn a_second_open_loads_the_peaks_from_the_cache_and_a_changed_file_does_not() {
        let dir = std::env::temp_dir().join(format!("wavcraft-doc-peaks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |frames: usize| {
            soundcraft_audio_io::encode(
                &ramp(frames, 2),
                &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int16, ..EncodeOptions::default() },
            )
            .unwrap()
        };
        let file = dir.join("take.wav");
        std::fs::write(&file, wav(96_000)).unwrap();
        let opts = OpenOptions { sessions_root: Some(dir.join("sessions")), peak_cache: Some(dir.join("peaks")), ..OpenOptions::default() };
        let open = |opts: &OpenOptions| Document::open_in(&file, opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let first = open(&opts);
        assert_eq!(first.peak_report.result, "miss");
        let second = open(&opts);
        assert_eq!(second.peak_report.result, "hit");
        assert_eq!(second.peaks, first.peaks, "the cached overview is the computed one");
        assert_eq!(second.waveform_columns(0, 0.0, 96_000.0, 200), first.waveform_columns(0, 0.0, 96_000.0, 200));
        // The audio itself still comes through the store.
        assert_eq!(second.audio_buffer().unwrap(), first.audio_buffer().unwrap());
        // A different file at the same path: stale, computed again, then a hit again.
        std::fs::write(&file, wav(120_000)).unwrap();
        assert_eq!(open(&opts).peak_report.result, "stale");
        assert_eq!(open(&opts).peak_report.result, "hit");
        // With no cache folder, nothing is read or written.
        let off = OpenOptions { peak_cache: None, ..opts.clone() };
        assert_eq!(open(&off).peak_report.result, "off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn properties_list_the_file_and_what_it_carries() {
        let dir = std::env::temp_dir().join(format!("wavcraft-doc-props-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bwf = soundcraft_audio_io::BwfInfo { description: "scene 4".into(), originator: "kit".into(), time_reference: 480, ..Default::default() };
        let bytes = soundcraft_audio_io::encode(
            &ramp(96_000, 2),
            &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int24, bwf: Some(bwf), ..EncodeOptions::default() },
        )
        .unwrap();
        let file = dir.join("props.wav");
        std::fs::write(&file, &bytes).unwrap();
        let doc = Document::open_in(
            &file,
            &OpenOptions { sessions_root: Some(dir.join("s")), ..OpenOptions::default() },
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        let rows = doc.properties();
        let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.clone()).unwrap_or_default();
        assert_eq!(get("Path"), file.display().to_string());
        assert_eq!(get("Format"), "WAV");
        assert_eq!(get("Sample rate"), "48000 Hz");
        assert_eq!(get("Channels"), "2");
        assert_eq!(get("Bit depth"), "24-bit integer");
        assert!(get("Duration").starts_with("0:02.000") && get("Duration").contains("96000 frames"), "{}", get("Duration"));
        assert!(get("Size").ends_with(&format!("({} bytes)", bytes.len())), "{}", get("Size"));
        // Broadcast Wave fields show only when the reader found them.
        if doc.info.bwf.is_some() {
            assert_eq!(get("BWF description"), "scene 4");
        }
        // In-memory audio has no path or size.
        let mem = Document::from_buffer(ramp(480, 1)).unwrap().properties();
        assert_eq!(mem[0], ("Path", "(not saved)".to_string()));
        assert_eq!(mem[6].1, "(no file)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_and_empty_audio_are_errors_not_panics() {
        assert!(matches!(Document::from_bytes(Path::new("x.wav"), b"not audio at all"), Err(OpenError::Decode(_))));
        assert_eq!(Document::from_buffer(AudioBuffer::new(48_000, 2, 0)).unwrap_err(), OpenError::Empty);
        assert_eq!(Document::from_buffer(AudioBuffer::new(0, 1, 100)).unwrap_err(), OpenError::Empty);
        assert!(matches!(Document::open(Path::new("definitely/not/here.wav")), Err(OpenError::Io(_))));
    }

    #[test]
    fn waveform_columns_follow_the_audio_at_every_zoom() {
        let doc = Document::from_buffer(ramp(480_000, 2)).unwrap();
        // Zoomed out: 1000 columns over the whole file; the ramp rises on channel 0, falls on 1.
        let out = doc.waveform_columns(0, 0.0, 480_000.0, 1000);
        assert_eq!(out.len(), 1000);
        assert!(out[0].1 < 0.01 && out[999].1 > 0.99, "{:?} {:?}", out[0], out[999]);
        assert!(doc.waveform_columns(1, 0.0, 480_000.0, 1000)[999].0 < -0.99);
        // Zoomed in to one sample per column: each column is exactly its sample.
        let near = doc.waveform_columns(0, 240_000.0, 240_010.0, 10);
        for (i, (lo, hi)) in near.iter().enumerate() {
            let want = (240_000 + i) as f32 / 480_000.0;
            assert_eq!((*lo, *hi), (want, want), "column {i}");
        }
        // Outside the file, a missing channel and broken ranges are silence, never a panic.
        assert_eq!(doc.waveform_columns(0, 480_000.0, 480_100.0, 4), vec![(0.0, 0.0); 4]);
        assert_eq!(doc.waveform_columns(0, -100.0, -1.0, 4), vec![(0.0, 0.0); 4]);
        assert_eq!(doc.waveform_columns(9, 0.0, 100.0, 4), vec![(0.0, 0.0); 4]);
        assert_eq!(doc.waveform_columns(0, f64::NAN, 100.0, 4), vec![(0.0, 0.0); 4]);
        assert_eq!(doc.waveform_columns(0, 100.0, 50.0, 4), vec![(0.0, 0.0); 4]);
        assert!(doc.waveform_columns(0, 0.0, 100.0, 0).is_empty());
    }
}
