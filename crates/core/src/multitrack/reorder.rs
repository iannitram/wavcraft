//! Reorder tracks (roadmap C104, core only): move one audio track from one position to another.
//! The drag-the-header gesture is the app's; this is the edit it will call.
//!
//! Positions are 0-based and count the audio tracks only, in the order `Multitrack::track_ids`
//! lists them. Any track that is not an audio track (a master, an aux) keeps its own slot.
//! Nothing else about a track changes: its id, name, clips and mixer (mute, solo, record arm,
//! volume) belong to the track and travel with it, and the session's selection is kept by id.

use super::{MtError, Multitrack, err};
use soundcraft_model::TrackKind;

impl Multitrack {
    /// Move the audio track at position `from` to position `to`; the tracks in between shift by one
    /// and every other track keeps its order. `from == to` changes nothing. A position past the last
    /// audio track is refused (the `from` position is checked first) and nothing changes.
    pub fn move_track(&mut self, from: usize, to: usize) -> Result<(), MtError> {
        let slots: Vec<usize> = self.session.tracks.iter().enumerate().filter(|(_, t)| t.kind == TrackKind::Audio).map(|(i, _)| i).collect();
        let n = slots.len();

        // Check if from position is out of range
        if from >= n {
            return err(format!("track position {from} is out of range: the session has {n} audio tracks"));
        }

        // Check if to position is out of range
        if to >= n {
            return err(format!("track position {to} is out of range: the session has {n} audio tracks"));
        }

        if from == to {
            return Ok(());
        }

        // Move by swapping neighbours across the slots (no cloning)
        if from < to {
            for k in from..to {
                self.session.tracks.swap(slots[k], slots[k + 1]);
            }
        } else {
            for k in (to..from).rev() {
                self.session.tracks.swap(slots[k], slots[k + 1]);
            }
        }

        Ok(())
    }
}
