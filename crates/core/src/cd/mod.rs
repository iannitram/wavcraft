//! CD Layout (f121, roadmap C241): an ordered list of tracks for an audio CD, each with a pause
//! before it, a title, a performer and an ISRC; the disc's total time against the 74 or 80
//! minute limit; and the export as ONE WAV (44.1 kHz, 16-bit, stereo) plus a CUE sheet.
//!
//! Timing is in CD frames (75 a second, 588 sample frames each at 44.1 kHz). Every track's audio
//! is padded with silence to a whole number of CD frames, so each `INDEX` in the CUE sheet is
//! exact to the frame. The export streams: sources are read 64 K frames at a time through
//! [`DocState::read`], converted on the way (channels, sample rate by linear interpolation),
//! and written with [`write_atomic`](crate::atomic::write_atomic), so a failed export leaves
//! nothing half written. The ISRC checks are the validators in [`codes`].
//! **Burning a disc is out of scope** (C243); the files are what a burning
//! program takes. CD-Text is not written (C244); titles and performers go in the CUE sheet only.

use std::fmt;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doc::DocState;
use crate::save::{SampleType, quantize, write_header};
use crate::store::BLOCK_FRAMES;

pub mod codes;
pub mod fade;

pub use codes::{is_valid_isrc as isrc_valid, normalize_isrc as isrc_normalize};

pub const CD_RATE: u32 = 44_100;
/// Sample frames in one CD frame (1/75 s).
pub const SAMPLES_PER_FRAME: u64 = 588;
pub const FRAMES_PER_SEC: u64 = 75;
/// The pause before the first track (and the default start of the disc).
pub const FIRST_PAUSE_SECS: f64 = 2.0;
/// The longest pause a track may be given.
pub const MAX_PAUSE_SECS: f64 = 60.0;

/// Why a layout change or an export was refused.
#[derive(Debug)]
pub struct CdError(pub String);

impl fmt::Display for CdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CdError {}

impl From<io::Error> for CdError {
    fn from(e: io::Error) -> Self {
        CdError(e.to_string())
    }
}

fn err<T>(s: impl Into<String>) -> Result<T, CdError> {
    Err(CdError(s.into()))
}

/// The disc's capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscLimit {
    Min74,
    Min80,
}

impl DiscLimit {
    pub fn minutes(self) -> u64 {
        match self {
            DiscLimit::Min74 => 74,
            DiscLimit::Min80 => 80,
        }
    }

    /// The limit in CD frames.
    pub fn frames(self) -> u64 {
        self.minutes() * 60 * FRAMES_PER_SEC
    }
}

/// `mm:ss:ff` for a count of CD frames (minutes may pass 99 on a long layout).
pub fn format_msf(frames: u64) -> String {
    format!("{:02}:{:02}:{:02}", frames / (FRAMES_PER_SEC * 60), frames / FRAMES_PER_SEC % 60, frames % FRAMES_PER_SEC)
}

/// A pause in seconds as whole CD frames.
pub fn pause_frames(secs: f64) -> u64 {
    (secs * FRAMES_PER_SEC as f64).round() as u64
}

/// One track of the layout.
#[derive(Clone)]
pub struct CdTrack {
    pub source: Arc<DocState>,
    /// Where the audio came from (a file name or a document's name), for the list.
    pub name: String,
    pub pause_secs: f64,
    pub title: String,
    pub performer: String,
    /// As typed; checked by [`isrc_valid`] (empty is allowed: no ISRC).
    pub isrc: String,
}

impl CdTrack {
    /// Samples at 44.1 kHz once converted.
    pub fn cd_samples(&self) -> u64 {
        let (len, rate) = (u128::from(self.source.len_frames()), u128::from(self.source.sample_rate()));
        ((len * u128::from(CD_RATE) + rate / 2) / rate) as u64
    }

    /// Whole CD frames of audio (the conversion padded with silence to a frame boundary).
    pub fn cd_frames(&self) -> u64 {
        self.cd_samples().div_ceil(SAMPLES_PER_FRAME)
    }

    /// What export changes about this source, or `None` when it is 44.1 kHz stereo already.
    pub fn conversion(&self) -> Option<String> {
        let (rate, ch) = (self.source.sample_rate(), self.source.channel_count());
        let mut parts = Vec::new();
        if rate != CD_RATE {
            parts.push(format!("{rate} Hz to 44100 Hz (linear interpolation)"));
        }
        match ch {
            2 => {}
            1 => parts.push("mono to stereo".to_string()),
            n => parts.push(format!("{n} channels to the first two")),
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// Where a track sits on the disc, in CD frames from the start of the WAV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// `INDEX 00`: the start of the pause (`None` when there is none).
    pub index00: Option<u64>,
    /// `INDEX 01`: the start of the audio.
    pub index01: u64,
    pub frames: u64,
}

/// The track list and the disc's capacity.
pub struct CdLayout {
    pub tracks: Vec<CdTrack>,
    pub limit: DiscLimit,
}

impl Default for CdLayout {
    fn default() -> Self {
        CdLayout { tracks: Vec::new(), limit: DiscLimit::Min80 }
    }
}

/// What an export made.
#[derive(Debug)]
pub struct Exported {
    pub wav: PathBuf,
    pub cue: PathBuf,
    pub total_frames: u64,
    pub warnings: Vec<String>,
}

impl CdLayout {
    /// Add a track at the end; its pause is 2 s for the first track, 0 for the others. Returns
    /// its index.
    pub fn add(&mut self, source: Arc<DocState>, name: &str) -> usize {
        let pause_secs = if self.tracks.is_empty() { FIRST_PAUSE_SECS } else { 0.0 };
        self.tracks.push(CdTrack {
            source,
            name: name.to_string(),
            pause_secs,
            title: name.to_string(),
            performer: String::new(),
            isrc: String::new(),
        });
        self.tracks.len() - 1
    }

    pub fn remove(&mut self, i: usize) -> Result<(), CdError> {
        if i >= self.tracks.len() {
            return err(format!("no track {}", i + 1));
        }
        self.tracks.remove(i);
        Ok(())
    }

    /// Move track `from` so it ends up at index `to`.
    pub fn move_track(&mut self, from: usize, to: usize) -> Result<(), CdError> {
        if from >= self.tracks.len() || to >= self.tracks.len() {
            return err("no such track");
        }
        let t = self.tracks.remove(from);
        self.tracks.insert(to, t);
        Ok(())
    }

    pub fn set_pause(&mut self, i: usize, secs: f64) -> Result<(), CdError> {
        if !secs.is_finite() || !(0.0..=MAX_PAUSE_SECS).contains(&secs) {
            return err(format!("a pause is 0 to {MAX_PAUSE_SECS} seconds"));
        }
        self.tracks.get_mut(i).ok_or_else(|| CdError(format!("no track {}", i + 1)))?.pause_secs = secs;
        Ok(())
    }

    /// Set `title`, `performer` or `isrc` (an ISRC is checked and stored without hyphens in
    /// capitals; empty clears it).
    pub fn set_meta(&mut self, i: usize, key: &str, value: &str) -> Result<(), CdError> {
        let t = self.tracks.get_mut(i).ok_or_else(|| CdError(format!("no track {}", i + 1)))?;
        match key {
            "title" => t.title = value.to_string(),
            "performer" => t.performer = value.to_string(),
            "isrc" if value.is_empty() => t.isrc.clear(),
            "isrc" => {
                t.isrc = isrc_normalize(value).ok_or_else(|| {
                    CdError(format!("{value:?} is not an ISRC: 2 letters, 3 letters or digits, 2 digits, 5 digits (US-ABC-24-00001)"))
                })?;
            }
            _ => return err("the fields are title, performer and isrc"),
        }
        Ok(())
    }

    /// Every track's place on the disc.
    pub fn timeline(&self) -> Vec<Place> {
        let mut at = 0;
        self.tracks
            .iter()
            .map(|t| {
                let pause = pause_frames(t.pause_secs);
                let frames = t.cd_frames();
                let place = Place { index00: (pause > 0).then_some(at), index01: at + pause, frames };
                at += pause + frames;
                place
            })
            .collect()
    }

    /// The whole disc in CD frames.
    pub fn total_frames(&self) -> u64 {
        self.timeline().last().map_or(0, |p| p.index01 + p.frames)
    }

    pub fn total_secs(&self) -> f64 {
        self.total_frames() as f64 / FRAMES_PER_SEC as f64
    }

    /// Total time against the limit: frames over (positive) or room left (`None` when over).
    pub fn over_limit(&self) -> bool {
        self.total_frames() > self.limit.frames()
    }

    /// Things the owner should know before exporting, one line each.
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        for (i, t) in self.tracks.iter().enumerate() {
            if let Some(c) = t.conversion() {
                w.push(format!("Track {} ({}) is converted on export: {c}.", i + 1, t.name));
            }
            if !t.isrc.is_empty() && !isrc_valid(&t.isrc) {
                w.push(format!("Track {} has an invalid ISRC.", i + 1));
            }
        }
        if self.over_limit() {
            w.push(format!(
                "The disc is {} long, over the {}-minute limit ({}).",
                format_msf(self.total_frames()),
                self.limit.minutes(),
                format_msf(self.limit.frames())
            ));
        }
        w
    }

    /// The CUE sheet for a WAV called `wav_name`. Refused with an invalid ISRC or no tracks.
    pub fn cue_sheet(&self, wav_name: &str) -> Result<String, CdError> {
        if self.tracks.is_empty() {
            return err("the layout has no tracks");
        }
        let q = |s: &str| s.replace('"', "'").replace(['\r', '\n'], " ");
        let mut out = format!("FILE \"{}\" WAVE\n", q(wav_name));
        for (i, (t, p)) in self.tracks.iter().zip(self.timeline()).enumerate() {
            out.push_str(&format!("  TRACK {:02} AUDIO\n", i + 1));
            if !t.title.is_empty() {
                out.push_str(&format!("    TITLE \"{}\"\n", q(&t.title)));
            }
            if !t.performer.is_empty() {
                out.push_str(&format!("    PERFORMER \"{}\"\n", q(&t.performer)));
            }
            if !t.isrc.is_empty() {
                let isrc = isrc_normalize(&t.isrc).ok_or_else(|| CdError(format!("track {} has an invalid ISRC ({})", i + 1, t.isrc)))?;
                out.push_str(&format!("    ISRC {isrc}\n"));
            }
            if let Some(a) = p.index00 {
                out.push_str(&format!("    INDEX 00 {}\n", format_msf(a)));
            }
            out.push_str(&format!("    INDEX 01 {}\n", format_msf(p.index01)));
        }
        Ok(out)
    }

    /// Export `<dir>/<name>.wav` and `<dir>/<name>.cue`. `progress` gets 0 to 1; setting
    /// `cancel` stops the export with nothing left behind.
    pub fn export(&self, dir: &Path, name: &str, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> Result<Exported, CdError> {
        let wav_name = format!("{name}.wav");
        let cue = self.cue_sheet(&wav_name)?; // validates before anything is written
        std::fs::create_dir_all(dir)?;
        let (wav_path, cue_path) = (dir.join(&wav_name), dir.join(format!("{name}.cue")));
        let total = self.total_frames();
        let timeline = self.timeline();
        crate::atomic::write_atomic(&wav_path, |file| {
            let mut out = BufWriter::with_capacity(1 << 20, &mut *file);
            // write_header writes RIFF and fmt only; the data chunk header is ours to write.
            let (data_len, _) = write_header(&mut out, 2, CD_RATE, SampleType::Int16, total * SAMPLES_PER_FRAME, 0, u64::from(u32::MAX))?;
            out.write_all(b"data")?;
            out.write_all(&u32::try_from(data_len).unwrap_or(u32::MAX).to_le_bytes())?;
            let mut done = 0u64;
            for (t, p) in self.tracks.iter().zip(&timeline) {
                write_silence(&mut out, (p.index01 - p.index00.unwrap_or(p.index01)) * SAMPLES_PER_FRAME)?;
                write_track(&mut out, t, p.frames * SAMPLES_PER_FRAME, cancel)?;
                done += p.index01 - p.index00.unwrap_or(p.index01) + p.frames;
                progress(done as f64 / total.max(1) as f64);
            }
            out.flush()?;
            Ok(())
        })
        .map_err(|e| CdError(e.to_string()))?;
        crate::atomic::write_atomic(&cue_path, |file| file.write_all(cue.as_bytes()))
            .map_err(|e| CdError(e.to_string()))?;
        log::info!(target: "wavcraft_core::cd", "exported a CD layout of {} tracks, {} CD frames, to {}", self.tracks.len(), total, wav_path.display());
        Ok(Exported { wav: wav_path, cue: cue_path, total_frames: total, warnings: self.warnings() })
    }
}

fn write_silence(out: &mut impl Write, samples: u64) -> io::Result<()> {
    let zeros = vec![0u8; 4 * BLOCK_FRAMES];
    let mut left = samples;
    while left > 0 {
        let n = left.min(BLOCK_FRAMES as u64);
        out.write_all(&zeros[..(n * 4) as usize])?;
        left -= n;
    }
    Ok(())
}

/// `total` sample frames of `t` at 44.1 kHz stereo 16-bit: the converted audio, then silence up
/// to `total`.
fn write_track(out: &mut impl Write, t: &CdTrack, total: u64, cancel: &AtomicBool) -> io::Result<()> {
    let (src, n_audio) = (&t.source, t.cd_samples());
    let (len, rate, chans) = (src.len_frames(), u64::from(src.sample_rate()), src.channel_count());
    let right = usize::from(chans > 1);
    let mut buf = [Vec::<f32>::new(), Vec::<f32>::new()];
    let mut bytes = Vec::with_capacity(BLOCK_FRAMES * 4);
    let mut j = 0u64;
    while j < n_audio {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "the export was cancelled"));
        }
        let n = (n_audio - j).min(BLOCK_FRAMES as u64);
        // Source frame for output frame k is k * rate / 44100; read what the chunk needs.
        let pos = |k: u64| (u128::from(k) * u128::from(rate)) as f64 / f64::from(CD_RATE);
        let lo = (pos(j) as u64).min(len.saturating_sub(1));
        let hi = ((pos(j + n - 1) as u64) + 2).min(len);
        let want = (hi - lo) as usize;
        for (c, b) in buf.iter_mut().enumerate() {
            b.clear();
            b.resize(want, 0.0);
            let ch = if c == 0 { 0 } else { right };
            if c == 1 && right == 0 {
                continue; // mono: the right channel is the left one
            }
            let got = src.read(ch, lo, b)?;
            if got != want {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a source gave fewer frames than it has"));
            }
        }
        bytes.clear();
        for k in j..j + n {
            let p = pos(k);
            let i0 = (p as u64).min(len - 1);
            let (frac, i1) = (if rate == u64::from(CD_RATE) { 0.0 } else { (p - i0 as f64) as f32 }, (i0 + 1).min(len - 1));
            let at = |b: &Vec<f32>, i: u64| b.get((i - lo) as usize).copied().unwrap_or(0.0);
            for c in 0..2 {
                let b = if c == 1 && right == 0 { &buf[0] } else { &buf[c] };
                let x = at(b, i0) * (1.0 - frac) + at(b, i1) * frac;
                bytes.extend_from_slice(&(quantize(x, 16) as i16).to_le_bytes());
            }
        }
        out.write_all(&bytes)?;
        j += n;
    }
    write_silence(out, total - n_audio)
}
