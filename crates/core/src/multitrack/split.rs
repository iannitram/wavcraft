//! Split a stereo track into two mono tracks (C111). A new file beside `multitrack.rs`; the
//! multitrack model has no undo, so this edit has none either.
//!
//! Rules (all of them are tested in `crates/core/tests/split_to_mono.rs`):
//! - Only a stereo audio track can be split. An unknown track, a track that is not an audio
//!   track, and a stereo track whose clip audio is missing or not two channels are refused with an
//!   [`MtError`], and then nothing has changed.
//! - The two new tracks are named "<name> L" and "<name> R" (the base is cut so the whole name
//!   stays within `MAX_NAME_CHARS`). A name another track already has is refused.
//! - They take the stereo track's place: left first, right right after it; every other track
//!   keeps its place.
//! - Everything on the stereo track is copied to both (volume, trim, mute, solo, colour, height,
//!   inserts, sends, automation, comments, folder, playlists and their names), except: the ids and
//!   names; the format (mono); the pan (left is -1.0, right is +1.0, as a hard-panned pair);
//!   record arm and input monitor (both off).
//! - Each audio clip becomes two clips with the same start, length, offset, gain, fades and name.
//!   The left clip plays the left channel of the clip's source, the right clip the right channel.
//!   Each source the stereo track used gets two new one-channel sources, named "<source> L" and
//!   "<source> R" (path "<source name> L.wav"), shared by every clip that used the old source.
//!   An old source no clip uses any more is dropped, with its audio.

use std::collections::BTreeMap;
use std::sync::Arc;

use soundcraft_audio_io::AudioBuffer as ModelBuffer;
use soundcraft_model::{ChannelFormat, ClipContent, MAX_NAME_CHARS, Source, SourceAudio, SourceId, TrackId, TrackKind};

use super::{MtError, Multitrack, err};

/// "<base> <side>", with the base cut so the result is at most `MAX_NAME_CHARS` characters.
/// `side` is 'L' or 'R'.
pub fn split_name(base: &str, side: char) -> String {
    let cut = base.chars().take(MAX_NAME_CHARS - 2).collect::<String>();
    format!("{cut} {side}")
}

impl Multitrack {
    /// Make the one-channel sources "<name> L" and "<name> R" from the two-channel source `src`
    /// (new ids, left channel and right channel). Returns (left id, right id).
    fn mono_sources(&mut self, src: SourceId) -> Result<(SourceId, SourceId), MtError> {
        let Some(info) = self.session.source(src).cloned() else { return err("no such source") };
        let Some(audio) = self.session.pool.get(src).cloned() else { return err("the audio of a clip is not in memory") };
        if audio.buffer.channels.len() != 2 {
            return err("the audio of a clip is not two channels");
        }

        let left_id = SourceId(self.session.alloc());
        let right_id = SourceId(self.session.alloc());

        let name_l = format!("{} L", info.name);
        let name_r = format!("{} R", info.name);

        let source_l = Source {
            id: left_id,
            name: name_l.clone(),
            path: format!("{name_l}.wav"),
            channels: 1,
            frames: info.frames,
            sample_rate: info.sample_rate,
            format: soundcraft_audio_io::FileFormat::Wav,
            time_reference: info.time_reference,
            unsaved: true,
        };

        let source_r = Source {
            id: right_id,
            name: name_r.clone(),
            path: format!("{name_r}.wav"),
            channels: 1,
            frames: info.frames,
            sample_rate: info.sample_rate,
            format: soundcraft_audio_io::FileFormat::Wav,
            time_reference: info.time_reference,
            unsaved: true,
        };

        self.session.sources.push(source_l);
        self.session.sources.push(source_r);

        let buffer_l = ModelBuffer { sample_rate: audio.buffer.sample_rate, channels: vec![audio.buffer.channels[0].clone()] };

        let buffer_r = ModelBuffer { sample_rate: audio.buffer.sample_rate, channels: vec![audio.buffer.channels[1].clone()] };

        self.session.pool.insert(left_id, Arc::new(SourceAudio::new(buffer_l)));
        self.session.pool.insert(right_id, Arc::new(SourceAudio::new(buffer_r)));

        Ok((left_id, right_id))
    }

    /// Split the stereo track `id` into two mono tracks (left, right) in its place. Returns the
    /// new track ids (left, right).
    pub fn split_to_mono(&mut self, id: TrackId) -> Result<(TrackId, TrackId), MtError> {
        let Some(i) = self.session.track_index(id) else { return err("no such track") };
        let old = self.session.tracks[i].clone();

        if old.kind != TrackKind::Audio {
            return err(format!("track {:?} is not an audio track", old.name));
        }

        if old.format != ChannelFormat::Stereo {
            return err(format!("track {:?} is {}; only a stereo track can be split", old.name, old.format.label()));
        }

        let (ln, rn) = (split_name(&old.name, 'L'), split_name(&old.name, 'R'));

        // Check if any other track already has these names
        for t in &self.session.tracks {
            if t.id != old.id {
                if t.name.eq_ignore_ascii_case(&ln) {
                    return err(format!("there is already a track named {:?}", ln));
                }
                if t.name.eq_ignore_ascii_case(&rn) {
                    return err(format!("there is already a track named {:?}", rn));
                }
            }
        }

        // Check audio of every audio clip in every playlist
        for playlist in &old.playlists {
            for clip in &playlist.clips {
                if clip.is_audio() {
                    let Some(src) = clip.source() else { continue };
                    if let Some(audio) = self.session.pool.get(src).cloned() {
                        if audio.buffer.channels.len() != 2 {
                            return err(format!("the audio of clip {:?} is not available as two channels", clip.name));
                        }
                    } else {
                        return err(format!("the audio of clip {:?} is not available as two channels", clip.name));
                    }
                }
            }
        }

        let mut made: BTreeMap<SourceId, (SourceId, SourceId)> = BTreeMap::new();

        let mut left = old.clone();
        let mut right = old.clone();

        let lid = TrackId(self.session.alloc());
        let rid = TrackId(self.session.alloc());

        left.id = lid;
        right.id = rid;
        left.name = ln;
        right.name = rn;
        left.format = ChannelFormat::Mono;
        right.format = ChannelFormat::Mono;
        left.mixer.pan = vec![-1.0];
        right.mixer.pan = vec![1.0];
        left.mixer.record_arm = false;
        left.mixer.input_monitor = false;
        right.mixer.record_arm = false;
        right.mixer.input_monitor = false;

        // Process clips in each playlist
        for (p, playlist) in old.playlists.iter().enumerate() {
            for (k, clip) in playlist.clips.iter().enumerate() {
                if clip.is_audio() {
                    let Some(src) = clip.source() else { continue };
                    let ids = made.get(&src).copied();
                    let (ls, rs) = match ids {
                        Some(ids) => ids,
                        None => {
                            let new_ids = self.mono_sources(src)?;
                            made.insert(src, new_ids);
                            new_ids
                        }
                    };

                    // Create left clip
                    let ClipContent::Audio { offset, .. } = clip.content else { continue };
                    let mut lc = clip.clone();
                    lc.id = self.session.new_clip_id();
                    lc.content = ClipContent::Audio { source: ls, offset };

                    // Create right clip
                    let mut rc = clip.clone();
                    rc.id = self.session.new_clip_id();
                    rc.content = ClipContent::Audio { source: rs, offset };

                    left.playlists[p].clips[k] = lc;
                    right.playlists[p].clips[k] = rc;
                } else {
                    // For non-audio clips, just create new ones with fresh IDs
                    let mut lc = clip.clone();
                    lc.id = self.session.new_clip_id();
                    let mut rc = clip.clone();
                    rc.id = self.session.new_clip_id();

                    left.playlists[p].clips[k] = lc;
                    right.playlists[p].clips[k] = rc;
                }
            }
        }

        // Replace the stereo track with the pair
        self.session.tracks[i] = left;
        self.session.tracks.insert(i + 1, right);
        // The takes listed for the old track id go with it, as in `remove_track` (f120).
        self.takes.retain(|k| k.track != id);

        // Drop old sources nobody uses any more
        for (old_src_id, _) in made {
            if self.session.clips_using(old_src_id).next().is_none() {
                self.session.sources.retain(|x| x.id != old_src_id);
                self.session.pool.remove(old_src_id);
            }
        }

        Ok((lid, rid))
    }
}
