//! The open file: decoded audio and per-channel waveform peaks.
//!
//! Since task 03 a file is opened by streaming it into the session store ([`crate::io::open`]),
//! and the [`DocState`] it makes is read back here into one whole-channel buffer, because the
//! views and the player still want one. That adapter is temporary: task 05 moves them onto
//! [`DocState::read`] and removes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use soundcraft_audio_io::peaks::Peaks;
use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};

use crate::doc::DocState;
pub use crate::io::OpenError;
use crate::io::OpenOptions;

/// One audio file, decoded whole into memory.
///
/// The audio sits behind an [`Arc`] so the playback thread and background analysis can hold it
/// without copying; an edit will swap in a new buffer.
#[derive(Debug, Clone)]
pub struct Document {
    /// Where the file came from (empty for a document made in memory).
    pub path: PathBuf,
    /// What the file said about itself: format, bit depth, rate, channels.
    pub info: AudioInfo,
    /// The samples, planar `f32`.
    pub audio: Arc<AudioBuffer>,
    /// One waveform overview per channel, for drawing at any zoom.
    pub peaks: Vec<Peaks>,
    /// The document in the session store, when it was opened from a file (`None` for audio
    /// made in memory). `audio` is a copy of it until task 05.
    pub state: Option<DocState>,
}

impl Document {
    /// Stream `path` into a new session store, then read it back whole (see the module notes).
    pub fn open(path: &Path) -> Result<Document, OpenError> {
        Document::open_with(path, &mut |_| {}, &AtomicBool::new(false))
    }

    /// [`open`](Self::open) with progress (0 to 1, for the streaming part) and a cancel flag.
    pub fn open_with(path: &Path, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Document, OpenError> {
        Document::open_in(path, &OpenOptions::default(), progress, cancel)
    }

    /// [`open_with`](Self::open_with) with the session folder and store set by `opts`.
    pub fn open_in(path: &Path, opts: &OpenOptions, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Document, OpenError> {
        let state = crate::io::open(path, opts, progress, cancel)?;
        Document::from_state(path.to_path_buf(), state)
    }

    /// The document for `state` (after an edit, or an undo), read back whole for the views and
    /// the player (see the module notes): the same adapter `open` uses, until task 05.
    pub fn from_state(path: PathBuf, state: DocState) -> Result<Document, OpenError> {
        let audio = state.to_audio_buffer().map_err(|e| OpenError::Io(e.to_string()))?;
        let src = state.source();
        let info = AudioInfo {
            format: src.format,
            sample_format: src.sample_format,
            sample_rate: state.sample_rate(),
            channels: u16::try_from(state.channel_count()).unwrap_or(u16::MAX),
            frames: state.len_frames(),
            bwf: src.bwf.clone(),
        };
        let mut doc = Document::build(path, info, audio)?;
        doc.state = Some(state);
        Ok(doc)
    }

    /// Decode a file already in memory. `path` supplies the extension hint and the display name.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Document, OpenError> {
        let ext = path.extension().and_then(|e| e.to_str());
        let (info, audio) = soundcraft_audio_io::decode(bytes, ext).map_err(|e| OpenError::Decode(e.to_string()))?;
        Document::build(path.to_path_buf(), info, audio)
    }

    /// Wrap audio that is already decoded (new files, tests).
    pub fn from_buffer(audio: AudioBuffer) -> Result<Document, OpenError> {
        let info = AudioInfo {
            format: FileFormat::Wav,
            sample_format: SampleFormat::Float32,
            sample_rate: audio.sample_rate,
            channels: u16::try_from(audio.num_channels()).unwrap_or(u16::MAX),
            frames: audio.frames() as u64,
            bwf: None,
        };
        Document::build(PathBuf::new(), info, audio)
    }

    fn build(path: PathBuf, info: AudioInfo, audio: AudioBuffer) -> Result<Document, OpenError> {
        if audio.frames() == 0 || audio.sample_rate == 0 {
            return Err(OpenError::Empty);
        }
        // One overview per channel, built side by side: an hour of audio per channel takes a while.
        let peaks = std::thread::scope(|s| {
            let jobs: Vec<_> = audio.channels.iter().map(|ch| s.spawn(move || Peaks::build(ch))).collect();
            jobs.into_iter().zip(&audio.channels).map(|(job, ch)| job.join().unwrap_or_else(|_| Peaks::build(ch))).collect()
        });
        Ok(Document { path, info, audio: Arc::new(audio), peaks, state: None })
    }

    /// Length in frames.
    pub fn frames(&self) -> usize {
        self.audio.frames()
    }

    /// Sample rate in Hz (never 0 for an open document).
    pub fn sample_rate(&self) -> u32 {
        self.audio.sample_rate
    }

    /// Number of channels.
    pub fn channels(&self) -> usize {
        self.audio.num_channels()
    }

    /// Length in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.audio.duration_secs()
    }

    /// What was wrong with the file that still opened (a header claiming more data than is
    /// there, an unusual sample size…), for the app to show. Empty for a sound file or a
    /// document made in memory.
    pub fn warnings(&self) -> &[String] {
        self.state.as_ref().map(|s| s.source().warnings.as_slice()).unwrap_or(&[])
    }

    /// File name for a title bar ("Untitled" for a document made in memory).
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// `(min, max)` per pixel column for drawing channel `ch` over the frame range
    /// `[start, end)`. Zoomed in past the overview's resolution it reads the samples themselves,
    /// so single samples stay visible.
    pub fn waveform_columns(&self, ch: usize, start: f64, end: f64, cols: usize) -> Vec<(f32, f32)> {
        let (Some(samples), Some(peaks)) = (self.audio.channels.get(ch), self.peaks.get(ch)) else { return vec![(0.0, 0.0); cols] };
        if cols == 0 || !start.is_finite() || !end.is_finite() || end <= start {
            return vec![(0.0, 0.0); cols];
        }
        let per_col = (end - start) / cols as f64;
        if per_col >= 64.0 {
            // The overview crate's own column walk was replaced by interpolating `spans`; keep the
            // exact bucket min/max this view has always drawn, now over `minmax(samples, ..)`.
            let len = samples.len() as f64;
            return (0..cols)
                .map(|c| {
                    let (s, e) = (start + per_col * c as f64, start + per_col * (c + 1) as f64);
                    if e <= 0.0 || s >= len {
                        return (0.0, 0.0);
                    }
                    let si = s.max(0.0).floor();
                    let ei = e.min(len).ceil().max(si + 1.0);
                    peaks.minmax(samples, si as usize, ei as usize)
                })
                .collect();
        }
        (0..cols)
            .map(|c| {
                let a = start + c as f64 * per_col;
                let b = a + per_col;
                if b <= 0.0 || a >= samples.len() as f64 {
                    return (0.0, 0.0);
                }
                let first = a.max(0.0).floor() as usize;
                let last = (b.ceil() as usize).clamp(first + 1, samples.len());
                let span = samples.get(first..last).unwrap_or(&[]);
                let folded = span.iter().filter(|v| v.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |m, v| (m.0.min(*v), m.1.max(*v)));
                if folded.0 > folded.1 { (0.0, 0.0) } else { folded }
            })
            .collect()
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
        assert_eq!(*doc.audio, buf, "a float WAV round trip is exact");
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
