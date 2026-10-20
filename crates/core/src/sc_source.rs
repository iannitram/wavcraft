//! A WavCraft document as a SoundCraft clip source (item f102 / C322).
//!
//! `soundcraft-model` reads sample data through its `SampleSource` trait. [`DocSource`] serves
//! that trait from a [`DocState`], so every read goes through the store's block cache
//! ([`DocState::read`]) and no whole-file copy is made. [`session_with_document`] builds a small
//! session whose one clip plays the document; [`read_checksum`] reads that clip's source back
//! in bounded chunks.

use crate::doc::DocState;
use soundcraft_model::{ChannelFormat, Clip, SampleSource, Session, Source, SourceId, TrackKind};
use soundcraft_time::SampleRate;
use std::sync::Arc;

/// Frames read per call when a source is read back (bounds the scratch buffer).
pub const CHUNK_FRAMES: usize = 65_536;

/// A document state as a SoundCraft sample source. Cheap to make: the state shares its blocks.
#[derive(Debug, Clone)]
pub struct DocSource {
    state: DocState,
}

impl DocSource {
    pub fn new(state: DocState) -> DocSource {
        DocSource { state }
    }
}

impl SampleSource for DocSource {
    fn channels(&self) -> usize {
        self.state.channel_count()
    }
    fn sample_rate(&self) -> u32 {
        self.state.sample_rate()
    }
    fn frames(&self) -> u64 {
        self.state.len_frames()
    }
    fn read(&self, channel: usize, start_frame: u64, out: &mut [f32]) -> usize {
        match self.state.read(channel, start_frame, out) {
            Ok(n) => n,
            Err(e) => {
                log::warn!(target: "wavcraft", "sample source read failed (channel {channel}, frame {start_frame}): {e}");
                0
            }
        }
    }
}

/// A session with one audio track and one clip whose source is `state` (streamed, not copied).
/// Returns the session and the source's id.
pub fn session_with_document(state: &DocState, name: &str) -> Result<(Session, SourceId), String> {
    let rate = SampleRate::new(state.sample_rate()).map_err(|e| format!("sample rate: {e}"))?;
    let frames = state.len_frames();
    if frames == 0 {
        return Err("the document is empty".to_string());
    }
    let channels = state.channel_count();
    let format = match channels {
        1 => ChannelFormat::Mono,
        2 => ChannelFormat::Stereo,
        n => return Err(format!("{n} channels: only mono and stereo documents are mapped to a track")),
    };
    let mut session = Session::new(name, rate);
    let track = session.add_track(TrackKind::Audio, format, Some("Document"));
    let source_id = SourceId(session.alloc());
    let source = Source {
        id: source_id,
        name: name.to_string(),
        path: String::new(),
        channels: u16::try_from(channels).map_err(|_| "too many channels".to_string())?,
        frames,
        sample_rate: state.sample_rate(),
        format: soundcraft_audio_io::FileFormat::Wav,
        time_reference: 0,
        unsaved: false,
    };
    session.sources.push(source);
    session.pool.insert_stream(source_id, Arc::new(DocSource::new(state.clone())));
    let clip_id = session.new_clip_id();
    let length = i64::try_from(frames).map_err(|_| "document too long".to_string())?;
    let clip = Clip::audio(clip_id, name, source_id, 0, 0, length);
    session.track_mut(track).and_then(|t| t.playlist_mut()).ok_or("the new track has no playlist")?.clips.push(clip);
    Ok((session, source_id))
}

/// Read the first clip's source through the session's pool, `CHUNK_FRAMES` at a time, channel
/// by channel. Returns the frames read per channel and an FNV-1a hash over every sample's bits
/// (channel 0 first, then channel 1, ...).
pub fn read_checksum(session: &Session) -> Result<(u64, u64), String> {
    let clip = session.all_clips().next().ok_or("the session has no clip")?;
    let soundcraft_model::ClipContent::Audio { source, offset } = &clip.content else {
        return Err("the first clip is not an audio clip".to_string());
    };
    let src = session.pool.get_source(*source).ok_or("the clip's source is not in the pool")?;
    let (start, len) = (u64::try_from(*offset).map_err(|_| "negative offset")?, u64::try_from(clip.length).map_err(|_| "negative length")?);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut buf = vec![0.0f32; CHUNK_FRAMES];
    let mut frames = 0;
    for ch in 0..src.channels() {
        let (mut at, mut done) = (start, 0u64);
        while done < len {
            let want = usize::try_from(len - done).map_or(CHUNK_FRAMES, |w| w.min(CHUNK_FRAMES));
            let n = src.read(ch, at, &mut buf[..want]);
            if n == 0 {
                return Err(format!("channel {ch} ended at frame {at}, expected {}", start + len));
            }
            for v in &buf[..n] {
                for b in v.to_bits().to_le_bytes() {
                    hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            at += n as u64;
            done += n as u64;
        }
        frames = done;
    }
    Ok((frames, hash))
}
