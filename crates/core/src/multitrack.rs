//! Multitrack basics (f103, roadmap C074/C097): a session of audio tracks with clips, built on
//! SoundCraft's session model (`soundcraft-model`). This module owns no data format of its own:
//! the session, its tracks, clips and sources are the model's, and its save format is the model's
//! JSON (`Session::to_json`). What lives here is the editing vocabulary the app and the control
//! channel share: make a session, add / remove / rename tracks, set mute / solo / volume, and put
//! a file or an open document on a track as a clip.
//!
//! Memory: the model keeps a clip's audio in RAM (`SourcePool`), so an import is capped
//! ([`MAX_IMPORT_SAMPLES`]) and longer files are refused with a message. They wait for the
//! disk-backed source trait being added to the fork (issue #302).

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use soundcraft_model::time::SampleRate;
use soundcraft_model::{BitDepthSetting, ChannelFormat, Clip, Source, SourceAudio, SourceId, TrackId, TrackKind, truncate_chars};
use soundcraft_model::{MAX_NAME_CHARS, Session};
use soundcraft_model_audio::AudioBuffer as ModelBuffer;

use crate::Document;

pub use soundcraft_model;

/// The most samples (frames times channels) one imported clip may hold in RAM: 128 MiB of `f32`.
pub const MAX_IMPORT_SAMPLES: u64 = 32 * 1024 * 1024;
/// The largest file read for an import (it is decoded whole): 64 MiB.
pub const MAX_IMPORT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Why a multitrack edit was refused. The text is shown to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct MtError(pub String);

impl std::fmt::Display for MtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MtError {}

fn err<T>(s: impl Into<String>) -> Result<T, MtError> {
    Err(MtError(s.into()))
}

/// The bit depths a new session offers.
pub fn bit_depth_from_bits(bits: u32) -> Option<BitDepthSetting> {
    match bits {
        16 => Some(BitDepthSetting::Int16),
        24 => Some(BitDepthSetting::Int24),
        32 => Some(BitDepthSetting::Float32),
        _ => None,
    }
}

/// The bit depth as a number (32 means 32-bit float).
pub fn bits_of(d: BitDepthSetting) -> u32 {
    match d {
        BitDepthSetting::Int16 => 16,
        BitDepthSetting::Int24 => 24,
        BitDepthSetting::Float32 => 32,
    }
}

/// A multitrack session being edited.
pub struct Multitrack {
    pub session: Session,
}

impl Multitrack {
    /// A new, empty session. The rate must be 8 kHz to 768 kHz; an empty name is refused.
    pub fn new(name: &str, rate_hz: u32, depth: BitDepthSetting) -> Result<Multitrack, MtError> {
        Multitrack::new_with_master(name, rate_hz, depth, ChannelFormat::Stereo)
    }

    /// A new session whose master track has the given format: stereo or 5.1 (`Surround51`).
    /// The format is chosen here and kept in the session model (the master track and the first
    /// output path).
    pub fn new_with_master(name: &str, rate_hz: u32, depth: BitDepthSetting, master: ChannelFormat) -> Result<Multitrack, MtError> {
        if !matches!(master, ChannelFormat::Stereo | ChannelFormat::Surround51) {
            return err("a session's master is stereo or 5.1");
        }
        let name = clean_name(name)?;
        let rate = SampleRate::new(rate_hz).map_err(|_| MtError(format!("a session's sample rate must be 8000 to 768000 Hz, not {rate_hz}")))?;
        let mut session = Session::new(name, rate);
        session.bit_depth = depth;
        session.add_track(TrackKind::Master, master, None);
        if let Some(o) = session.outputs.first_mut() {
            o.format = master;
        }
        Ok(Multitrack { session })
    }

    /// The master's format (stereo or 5.1).
    pub fn master_format(&self) -> ChannelFormat {
        self.session.master().map_or(ChannelFormat::Stereo, |m| m.format)
    }

    fn master_mut(&mut self) -> Result<&mut soundcraft_model::Track, MtError> {
        self.session.tracks.iter_mut().find(|t| t.kind == TrackKind::Master).ok_or_else(|| MtError("this session has no master track".into()))
    }

    /// The master fader, in dB (-60 to +12).
    pub fn set_master_volume_db(&mut self, db: f32) -> Result<(), MtError> {
        if !db.is_finite() {
            return err("a volume must be a number of dB");
        }
        self.master_mut()?.mixer.volume_db = db.clamp(-60.0, 12.0);
        Ok(())
    }

    pub fn set_master_mute(&mut self, on: bool) -> Result<(), MtError> {
        self.master_mut()?.mixer.mute = on;
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.session.name
    }

    pub fn rate(&self) -> u32 {
        self.session.sample_rate.hz()
    }

    /// The audio tracks, in order (the model may also hold a master or buses; not made here).
    pub fn track_ids(&self) -> Vec<TrackId> {
        self.session.tracks.iter().filter(|t| t.kind == TrackKind::Audio).map(|t| t.id).collect()
    }

    pub fn track_count(&self) -> usize {
        self.track_ids().len()
    }

    /// Total clips on the audio tracks' active playlists.
    pub fn clip_count(&self) -> usize {
        self.session.tracks.iter().filter(|t| t.kind == TrackKind::Audio).map(|t| t.clips().len()).sum()
    }

    /// The end of the last clip, in seconds.
    pub fn end_seconds(&self) -> f64 {
        self.session.sample_rate.seconds(self.session.content_end())
    }

    /// Add a stereo (`channels` 2) or mono (1) audio track, or a 5.1 one (6, only in a 5.1
    /// session); `None` names it "Audio 1", "Audio 2"…
    /// (f118 keeps this minimal; issue #333 does per-track formats on its own branch, so expect
    /// this match to overlap with it.)
    pub fn add_track(&mut self, name: Option<&str>, channels: usize) -> Result<TrackId, MtError> {
        let format = match channels {
            1 => ChannelFormat::Mono,
            2 => ChannelFormat::Stereo,
            6 if self.master_format() == ChannelFormat::Surround51 => ChannelFormat::Surround51,
            6 => return err("a 5.1 track needs a 5.1 session: make the session with master=5.1"),
            n => return err(format!("an audio track here has 1, 2 or (in a 5.1 session) 6 channels, not {n}")),
        };
        let name = match name {
            Some(n) => {
                let n = clean_name(n)?;
                if self.session.tracks.iter().any(|t| t.name.eq_ignore_ascii_case(&n)) {
                    return err(format!("there is already a track named {n:?}"));
                }
                Some(n)
            }
            None => None,
        };
        Ok(self.session.add_track(TrackKind::Audio, format, name.as_deref()))
    }

    /// A track by name (any case) or by 1-based position among the audio tracks ("2", or "#2").
    pub fn find_track(&self, key: &str) -> Result<TrackId, MtError> {
        let ids = self.track_ids();
        if let Some(t) = self.session.tracks.iter().find(|t| t.kind == TrackKind::Audio && t.name.eq_ignore_ascii_case(key)) {
            return Ok(t.id);
        }
        if let Ok(n) = key.trim_start_matches('#').parse::<usize>()
            && n >= 1
            && let Some(id) = ids.get(n - 1)
        {
            return Ok(*id);
        }
        err(format!("no track {key:?}"))
    }

    /// Remove a track with its clips; sources no clip uses any more are dropped with it.
    pub fn remove_track(&mut self, id: TrackId) -> Result<(), MtError> {
        let Some(i) = self.session.track_index(id) else { return err("no such track") };
        let used: Vec<SourceId> = self.session.tracks[i].playlists.iter().flat_map(|p| p.clips.iter().filter_map(Clip::source)).collect();
        self.session.tracks.remove(i);
        for s in used {
            if self.session.clips_using(s).next().is_none() {
                self.session.sources.retain(|x| x.id != s);
                self.session.pool.remove(s);
            }
        }
        Ok(())
    }

    pub fn rename_track(&mut self, id: TrackId, name: &str) -> Result<(), MtError> {
        let name = clean_name(name)?;
        if self.session.tracks.iter().any(|t| t.id != id && t.name.eq_ignore_ascii_case(&name)) {
            return err(format!("there is already a track named {name:?}"));
        }
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.name = name;
        Ok(())
    }

    pub fn set_mute(&mut self, id: TrackId, on: bool) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.mute = on;
        Ok(())
    }

    pub fn set_solo(&mut self, id: TrackId, on: bool) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.solo = on;
        Ok(())
    }

    /// The track's fader, in dB (-60 to +12).
    pub fn set_volume_db(&mut self, id: TrackId, db: f32) -> Result<(), MtError> {
        if !db.is_finite() {
            return err("a volume must be a number of dB");
        }
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.volume_db = db.clamp(-60.0, 12.0);
        Ok(())
    }

    /// Put audio on a track as a clip starting `at_seconds` into the session. The audio is
    /// checked first (the session's rate, the size cap); the clip is named `name`.
    pub fn add_clip_buffer(
        &mut self,
        track: TrackId,
        name: &str,
        path: &str,
        audio: ModelBuffer,
        at_seconds: f64,
    ) -> Result<soundcraft_model::ClipId, MtError> {
        if self.session.track(track).is_none() {
            return err("no such track");
        }
        if !at_seconds.is_finite() || at_seconds < 0.0 {
            return err("a clip's position is a number of seconds, 0 or more");
        }
        if audio.sample_rate != self.rate() {
            return err(format!(
                "the audio is {} Hz and the session {} Hz; resampling on import is not built yet, so make the session {} Hz",
                audio.sample_rate,
                self.rate(),
                audio.sample_rate
            ));
        }
        let frames = audio.frames();
        if frames == 0 {
            return err("the audio is empty");
        }
        let samples = frames as u64 * audio.num_channels() as u64;
        if samples > MAX_IMPORT_SAMPLES {
            return err(format!(
                "the audio is too long to hold in memory as a clip ({samples} samples; the limit is {MAX_IMPORT_SAMPLES}); long files wait for the disk-backed sources (issue #302)"
            ));
        }
        let at = self.session.sample_rate.samples(at_seconds);
        let source = SourceId(self.session.alloc());
        let (channels, rate) = (u16::try_from(audio.num_channels()).unwrap_or(u16::MAX), audio.sample_rate);
        self.session.sources.push(Source {
            id: source,
            name: name.to_string(),
            path: path.to_string(),
            channels,
            frames: frames as u64,
            sample_rate: rate,
            format: soundcraft_model_audio::FileFormat::Wav,
            time_reference: 0,
            unsaved: false,
        });
        self.session.pool.insert(source, Arc::new(SourceAudio::new(audio)));
        let id = self.session.new_clip_id();
        let clip = Clip::audio(id, name, source, 0, at, frames as i64);
        if let Some(pl) = self.session.track_mut(track).and_then(|t| t.playlist_mut()) {
            pl.clips.push(clip);
            pl.sort();
        }
        Ok(id)
    }

    /// Import an audio file as a clip. The file is probed first (rate, length) and read whole
    /// only when it is under the caps.
    pub fn add_clip_file(&mut self, track: TrackId, path: &Path, at_seconds: f64) -> Result<soundcraft_model::ClipId, MtError> {
        let meta = std::fs::metadata(path).map_err(|e| MtError(format!("cannot read {}: {e}", path.display())))?;
        if meta.len() > MAX_IMPORT_FILE_BYTES {
            return err(format!(
                "{} is {} bytes; a clip is held in memory, so files over {MAX_IMPORT_FILE_BYTES} bytes wait for the disk-backed sources (issue #302)",
                path.display(),
                meta.len()
            ));
        }
        let bytes = std::fs::read(path).map_err(|e| MtError(format!("cannot read {}: {e}", path.display())))?;
        let ext = path.extension().and_then(|e| e.to_str());
        let info = soundcraft_model_audio::probe(&bytes, ext).map_err(|e| MtError(format!("cannot decode {}: {e}", path.display())))?;
        if info.frames.saturating_mul(u64::from(info.channels)) > MAX_IMPORT_SAMPLES {
            return err(format!(
                "{} is too long to hold in memory as a clip; long files wait for the disk-backed sources (issue #302)",
                path.display()
            ));
        }
        let (_, audio) = soundcraft_model_audio::decode(&bytes, ext).map_err(|e| MtError(format!("cannot decode {}: {e}", path.display())))?;
        drop(bytes);
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Clip").to_string();
        let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("clip").to_string();
        self.add_clip_buffer(track, &name, &file, audio, at_seconds)
    }

    /// Put an open document on a track as a clip (its audio is copied; the document is untouched).
    pub fn add_clip_document(&mut self, track: TrackId, doc: &Document, at_seconds: f64) -> Result<soundcraft_model::ClipId, MtError> {
        let samples = doc.frames() as u64 * doc.channels() as u64;
        if samples > MAX_IMPORT_SAMPLES {
            return err("the document is too long to hold in memory as a clip; long files wait for the disk-backed sources (issue #302)");
        }
        let buf = ModelBuffer { sample_rate: doc.sample_rate(), channels: doc.audio.channels.clone() };
        let name = doc.name();
        self.add_clip_buffer(track, &name, &name, buf, at_seconds)
    }

    /// One line of JSON for the control channel and the tests: the session and every audio track
    /// with its clips (start and length in seconds).
    pub fn state_json(&self) -> String {
        let rate = self.session.sample_rate;
        let tracks: Vec<_> = self
            .session
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| {
                let clips: Vec<_> = t
                    .clips()
                    .iter()
                    .map(|c| json!({"name": c.name, "start": rate.seconds(c.start), "length": rate.seconds(c.length), "channels": c.source().and_then(|s| self.session.source(s)).map_or(0, |s| s.channels)}))
                    .collect();
                json!({"name": t.name, "channels": t.channels(), "mute": t.mixer.mute, "solo": t.mixer.solo, "volume_db": t.mixer.volume_db, "clips": clips})
            })
            .collect();
        let master = self
            .session
            .master()
            .map(|m| json!({"format": m.format.label(), "channels": m.channels(), "mute": m.mixer.mute, "volume_db": m.mixer.volume_db}));
        json!({"name": self.session.name, "rate": rate.hz(), "bits": bits_of(self.session.bit_depth), "end": self.end_seconds(), "master": master, "tracks": tracks})
            .to_string()
    }

    /// Mix `frames` frames from session frame `start` down to the master: audio tracks (mute and
    /// solo honoured) at their faders, then the master's fader and mute. Returns one buffer per
    /// master channel (2, or 6 in L R C LFE Ls Rs order). Clip fades and gain envelopes are not
    /// applied yet; clip gain is. This is a plain reference mix for tests and bounce; it holds
    /// `frames` frames only (callers render in blocks).
    pub fn render(&self, start: i64, frames: usize) -> Vec<Vec<f32>> {
        let out_ch = self.master_format().channels();
        let mut out = vec![vec![0.0_f32; frames]; out_ch];
        let any_solo = self.session.tracks.iter().any(|t| t.kind == TrackKind::Audio && t.mixer.solo);
        let mut frame_in: Vec<f32> = Vec::new();
        for t in self.session.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
            if t.mixer.mute || (any_solo && !t.mixer.solo) {
                continue;
            }
            let tg = db_gain(t.mixer.volume_db);
            for c in t.clips().iter().filter(|c| !c.muted) {
                let Some(src) = c.source() else { continue };
                let Some(audio) = self.session.pool.get(src) else { continue };
                let offset = match c.content {
                    soundcraft_model::ClipContent::Audio { offset, .. } => offset,
                    _ => 0,
                };
                let g = tg * db_gain(c.gain_db);
                let n_in = audio.buffer.num_channels();
                for i in 0..frames {
                    let pos = start + i as i64 - c.start;
                    if pos < 0 || pos >= c.length {
                        continue;
                    }
                    let sp = (pos + offset) as usize;
                    frame_in.clear();
                    frame_in.extend((0..n_in).map(|ch| audio.channel(ch).and_then(|d| d.get(sp)).copied().unwrap_or(0.0)));
                    add_frame(&frame_in, g, &mut out, i);
                }
            }
        }
        let mg = self.session.master().map_or(1.0, |m| if m.mixer.mute { 0.0 } else { db_gain(m.mixer.volume_db) });
        for ch in &mut out {
            ch.iter_mut().for_each(|x| *x *= mg);
        }
        out
    }

    /// The session as the model's own JSON (audio is not in it; the sources' file names are).
    pub fn to_json(&self) -> Result<String, MtError> {
        self.session.to_json().map_err(|e| MtError(e.to_string()))
    }
}

fn db_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// The 5.1 to stereo downmix (the ITU-R BS.775 fold-down, the same one `soundcraft-mix` uses).
/// Channel order is L R C LFE Ls Rs. The LFE is dropped; the centre and the surrounds go to both
/// sides at -3 dB (0.7071):
/// `Lo = L + 0.7071 C + 0.7071 Ls`, `Ro = R + 0.7071 C + 0.7071 Rs`. No normalisation is applied,
/// so a loud full-scale 5.1 mix can exceed full scale in stereo.
pub fn downmix_51_to_stereo(f: [f32; 6]) -> [f32; 2] {
    const K: f32 = std::f32::consts::FRAC_1_SQRT_2;
    [f[0] + K * f[2] + K * f[4], f[1] + K * f[2] + K * f[5]]
}

/// Add one input frame (1, 2 or 6 channels) at gain `g` into the master buffers at index `i`.
/// Mono goes to L and R (to the centre in 5.1); stereo goes to L and R; 5.1 goes straight to a
/// 5.1 master and through [`downmix_51_to_stereo`] to a stereo one. Other widths are skipped.
fn add_frame(inp: &[f32], g: f32, out: &mut [Vec<f32>], i: usize) {
    match (inp.len(), out.len()) {
        (1, 2) => {
            out[0][i] += inp[0] * g;
            out[1][i] += inp[0] * g;
        }
        (1, 6) => out[2][i] += inp[0] * g,
        (2, 2 | 6) => {
            out[0][i] += inp[0] * g;
            out[1][i] += inp[1] * g;
        }
        (6, 6) => inp.iter().enumerate().for_each(|(ch, v)| out[ch][i] += v * g),
        (6, 2) => {
            let d = downmix_51_to_stereo([inp[0], inp[1], inp[2], inp[3], inp[4], inp[5]]);
            out[0][i] += d[0] * g;
            out[1][i] += d[1] * g;
        }
        _ => {}
    }
}

fn clean_name(name: &str) -> Result<String, MtError> {
    let mut n = name.trim().to_string();
    if n.is_empty() {
        return err("a name cannot be empty");
    }
    truncate_chars(&mut n, MAX_NAME_CHARS);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f32, ch: usize) -> ModelBuffer {
        let n = (rate as f32 * secs) as usize;
        ModelBuffer { sample_rate: rate, channels: (0..ch).map(|_| (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()).collect() }
    }

    fn two_tracks() -> (Multitrack, TrackId, TrackId) {
        let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        let a = m.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
        let b = m.add_track(Some("Voice"), 1).unwrap_or_else(|e| panic!("{e}"));
        (m, a, b)
    }

    #[test]
    fn a_session_is_made_with_its_name_rate_and_depth() {
        let m = Multitrack::new("  Mix  ", 44_100, BitDepthSetting::Float32).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((m.name(), m.rate(), bits_of(m.session.bit_depth)), ("Mix", 44_100, 32));
        assert_eq!((m.track_count(), m.clip_count()), (0, 0));
        assert!(Multitrack::new("", 48_000, BitDepthSetting::Int24).is_err());
        assert!(Multitrack::new("x", 100, BitDepthSetting::Int24).is_err());
        assert_eq!(bit_depth_from_bits(20), None);
    }

    #[test]
    fn tracks_are_added_found_renamed_and_removed() {
        let (mut m, a, b) = two_tracks();
        assert_eq!(m.track_count(), 2);
        assert_eq!(m.session.track(a).map(|t| t.name.as_str()), Some("Audio 1"));
        assert_eq!(m.find_track("voice"), Ok(b));
        assert_eq!(m.find_track("#1"), Ok(a));
        assert_eq!(m.find_track("2"), Ok(b));
        assert!(m.find_track("nope").is_err());
        assert!(m.add_track(Some("VOICE"), 1).is_err(), "names are unique");
        assert!(m.add_track(None, 6).is_err());
        m.rename_track(a, "Music").unwrap_or_else(|e| panic!("{e}"));
        assert!(m.rename_track(a, "voice").is_err());
        assert!(m.rename_track(a, " ").is_err());
        assert_eq!(m.find_track("music"), Ok(a));
        m.remove_track(a).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((m.track_count(), m.find_track("Music").is_err()), (1, true));
        assert!(m.remove_track(a).is_err());
    }

    #[test]
    fn mute_solo_and_volume_are_kept_on_the_track() {
        let (mut m, a, _) = two_tracks();
        m.set_mute(a, true).unwrap_or_else(|e| panic!("{e}"));
        m.set_solo(a, true).unwrap_or_else(|e| panic!("{e}"));
        m.set_volume_db(a, -6.0).unwrap_or_else(|e| panic!("{e}"));
        let t = m.session.track(a).unwrap_or_else(|| panic!("track"));
        assert_eq!((t.mixer.mute, t.mixer.solo, t.mixer.volume_db), (true, true, -6.0));
        m.set_volume_db(a, 99.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.session.track(a).map(|t| t.mixer.volume_db), Some(12.0));
        assert!(m.set_volume_db(a, f32::NAN).is_err());
    }

    #[test]
    fn a_clip_lands_on_the_track_at_its_position() {
        let (mut m, a, b) = two_tracks();
        m.add_clip_buffer(a, "Tone", "tone.wav", tone(48_000, 2.0, 2), 1.5).unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_buffer(b, "Short", "short.wav", tone(48_000, 0.5, 1), 0.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.clip_count(), 2);
        let c = &m.session.track(a).unwrap_or_else(|| panic!("track")).clips()[0];
        assert_eq!((c.start, c.length), (72_000, 96_000));
        assert!((m.end_seconds() - 3.5).abs() < 1e-9);
        assert_eq!(m.session.pool.len(), 2);
    }

    #[test]
    fn clips_are_refused_for_a_wrong_rate_position_or_size() {
        let (mut m, a, _) = two_tracks();
        assert!(m.add_clip_buffer(a, "x", "x", tone(44_100, 0.1, 1), 0.0).unwrap_or_default_err().contains("44100 Hz"));
        assert!(m.add_clip_buffer(a, "x", "x", tone(48_000, 0.1, 1), -1.0).is_err());
        assert!(m.add_clip_buffer(a, "x", "x", ModelBuffer { sample_rate: 48_000, channels: vec![vec![]] }, 0.0).is_err());
        assert!(m.add_clip_buffer(TrackId(999), "x", "x", tone(48_000, 0.1, 1), 0.0).is_err());
        assert_eq!(m.clip_count(), 0);
        assert!(m.session.sources.is_empty(), "a refused clip leaves no source behind");
    }

    #[test]
    fn removing_a_track_drops_its_clips_and_unused_sources() {
        let (mut m, a, b) = two_tracks();
        m.add_clip_buffer(a, "A", "a.wav", tone(48_000, 0.1, 1), 0.0).unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_buffer(b, "B", "b.wav", tone(48_000, 0.1, 1), 0.0).unwrap_or_else(|e| panic!("{e}"));
        m.remove_track(a).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((m.clip_count(), m.session.sources.len(), m.session.pool.len()), (1, 1, 1));
    }

    #[test]
    fn the_session_round_trips_through_the_models_own_json() {
        let (mut m, a, b) = two_tracks();
        m.add_clip_buffer(a, "Tone", "tone.wav", tone(48_000, 1.0, 2), 2.0).unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_buffer(b, "Voice", "voice.wav", tone(48_000, 0.5, 1), 0.25).unwrap_or_else(|e| panic!("{e}"));
        m.set_mute(b, true).unwrap_or_else(|e| panic!("{e}"));
        let text = m.to_json().unwrap_or_else(|e| panic!("{e}"));
        let back = Session::from_json(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((back.name.as_str(), back.sample_rate, back.bit_depth), ("Demo", m.session.sample_rate, BitDepthSetting::Int24));
        assert_eq!(back.tracks, m.session.tracks);
        assert_eq!(back.sources, m.session.sources);
        assert!(back.pool.is_empty(), "the audio is not in the session file");
    }

    #[test]
    fn a_wav_file_and_a_document_import_as_clips() {
        let dir = std::env::temp_dir().join(format!("wavcraft-mt-{}-{}", std::process::id(), crate::random::hex(4).unwrap_or_default()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let path = dir.join("tone.wav");
        let buf = tone(48_000, 0.5, 2);
        let bytes = soundcraft_model_audio::encode(&buf, &soundcraft_model_audio::EncodeOptions::default()).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("{e}"));
        let (mut m, a, b) = two_tracks();
        m.add_clip_file(a, &path, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let c = &m.session.track(a).unwrap_or_else(|| panic!("track")).clips()[0];
        assert_eq!((c.name.as_str(), c.start, c.length), ("tone", 48_000, 24_000));
        assert!(m.add_clip_file(a, &dir.join("missing.wav"), 0.0).is_err());
        let doc = Document::from_buffer(crate::AudioBuffer { sample_rate: 48_000, channels: tone(48_000, 0.25, 1).channels })
            .unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_document(b, &doc, 3.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.clip_count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_state_line_names_tracks_and_clips() {
        let (mut m, a, _) = two_tracks();
        m.add_clip_buffer(a, "Tone", "tone.wav", tone(48_000, 1.0, 2), 2.0).unwrap_or_else(|e| panic!("{e}"));
        let v: serde_json::Value = serde_json::from_str(&m.state_json()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((v["rate"].as_u64(), v["bits"].as_u64(), v["tracks"].as_array().map(Vec::len)), (Some(48_000), Some(24), Some(2)));
        assert_eq!(v["tracks"][0]["clips"][0]["start"].as_f64(), Some(2.0));
        assert_eq!(v["end"].as_f64(), Some(3.0));
    }

    fn dc(rate: u32, frames: usize, ch: usize, v: f32) -> ModelBuffer {
        ModelBuffer { sample_rate: rate, channels: vec![vec![v; frames]; ch] }
    }

    #[test]
    fn a_session_has_a_master_with_the_chosen_format() {
        let m = Multitrack::new("A", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.master_format(), ChannelFormat::Stereo);
        let m = Multitrack::new_with_master("B", 48_000, BitDepthSetting::Int24, ChannelFormat::Surround51).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((m.master_format(), m.session.main_format()), (ChannelFormat::Surround51, ChannelFormat::Surround51));
        assert_eq!(m.track_count(), 0, "the master is not an audio track");
        assert!(Multitrack::new_with_master("C", 48_000, BitDepthSetting::Int24, ChannelFormat::Quad).is_err());
        let v: serde_json::Value = serde_json::from_str(&m.state_json()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((v["master"]["format"].as_str(), v["master"]["channels"].as_u64()), (Some("5.1"), Some(6)));
    }

    #[test]
    fn the_masters_settings_survive_the_models_json() {
        let mut m = Multitrack::new_with_master("B", 48_000, BitDepthSetting::Int24, ChannelFormat::Surround51).unwrap_or_else(|e| panic!("{e}"));
        m.set_master_volume_db(-6.0).unwrap_or_else(|e| panic!("{e}"));
        m.set_master_mute(true).unwrap_or_else(|e| panic!("{e}"));
        let back = Session::from_json(&m.to_json().unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));
        let t = back.master().unwrap_or_else(|| panic!("master"));
        assert_eq!(
            (t.format, t.mixer.volume_db, t.mixer.mute, back.main_format()),
            (ChannelFormat::Surround51, -6.0, true, ChannelFormat::Surround51)
        );
        assert!(m.set_master_volume_db(f32::NAN).is_err());
    }

    #[test]
    fn five_one_tracks_need_a_five_one_session() {
        let mut st = Multitrack::new("S", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        assert!(st.add_track(None, 6).unwrap_or_default_err().contains("master=5.1"));
        let mut m = Multitrack::new_with_master("M", 48_000, BitDepthSetting::Int24, ChannelFormat::Surround51).unwrap_or_else(|e| panic!("{e}"));
        let t = m.add_track(Some("Surround"), 6).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.session.track(t).map(|t| t.channels()), Some(6));
        assert_eq!(m.track_ids(), vec![t]);
    }

    #[test]
    fn the_downmix_follows_the_itu_fold_down() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(downmix_51_to_stereo([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), [1.0, 0.0]);
        assert_eq!(downmix_51_to_stereo([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), [0.0, 0.0], "the LFE is dropped");
        let c = downmix_51_to_stereo([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!((c[0] - k).abs() < 1e-6 && (c[1] - k).abs() < 1e-6, "the centre is -3 dB in both");
        let s = downmix_51_to_stereo([0.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
        assert!((s[0] - k).abs() < 1e-6 && (s[1] - k).abs() < 1e-6, "each surround is -3 dB on its own side");
    }

    #[test]
    fn master_volume_and_mute_apply_in_a_render() {
        let (mut m, a, _) = two_tracks();
        m.add_clip_buffer(a, "dc", "dc.wav", dc(48_000, 1000, 2, 0.5), 0.0).unwrap_or_else(|e| panic!("{e}"));
        let r = m.render(0, 100);
        assert_eq!((r.len(), r[0][10], r[1][10]), (2, 0.5, 0.5));
        m.set_master_volume_db(-6.0).unwrap_or_else(|e| panic!("{e}"));
        let g = db_gain(-6.0) * 0.5;
        assert!((m.render(0, 100)[0][10] - g).abs() < 1e-6, "master -6 dB");
        m.set_master_mute(true).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.render(0, 100)[0][10], 0.0);
        m.set_master_mute(false).unwrap_or_else(|e| panic!("{e}"));
        m.set_volume_db(a, -6.0).unwrap_or_else(|e| panic!("{e}"));
        assert!((m.render(0, 100)[0][10] - g * db_gain(-6.0)).abs() < 1e-6, "track and master faders multiply");
    }

    #[test]
    fn a_five_one_track_downmixes_into_a_stereo_master_and_stays_six_in_a_five_one_session() {
        let mut st = Multitrack::new("S", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        let mut five = Multitrack::new_with_master("F", 48_000, BitDepthSetting::Int24, ChannelFormat::Surround51).unwrap_or_else(|e| panic!("{e}"));
        let t5 = five.add_track(None, 6).unwrap_or_else(|e| panic!("{e}"));
        let mut buf = dc(48_000, 100, 6, 0.0);
        buf.channels[2] = vec![1.0; 100];
        five.add_clip_buffer(t5, "x", "x", buf.clone(), 0.0).unwrap_or_else(|e| panic!("{e}"));
        let r = five.render(0, 10);
        assert_eq!((r.len(), r[2][5], r[0][5]), (6, 1.0, 0.0), "centre stays on the centre channel");
        // The same 6-channel clip on a stereo track of a stereo session: refused to be a 5.1 track,
        // but a clip may still be placed (the track is stereo) and is folded down.
        let t = st.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
        st.add_clip_buffer(t, "x", "x", buf, 0.0).unwrap_or_else(|e| panic!("{e}"));
        let r = st.render(0, 10);
        assert!((r[0][5] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    trait ErrText {
        fn unwrap_or_default_err(self) -> String;
    }
    impl<T> ErrText for Result<T, MtError> {
        fn unwrap_or_default_err(self) -> String {
            self.err().map(|e| e.0).unwrap_or_default()
        }
    }
}
