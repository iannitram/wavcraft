//! Invert phase per track (C110): a polarity flag on a multitrack track, and the one sample
//! operation that applies it. The flag lives in the model's own mixer strip
//! (`Track::mixer.phase_invert`), so it is saved and reloaded with the session's JSON.
//!
//! Not wired yet: the session's mixdown (`Multitrack::render`, PR #345) does not exist on this
//! base, so nothing calls [`apply_invert`] on a track's audio. That hook, and the button in the
//! track header, come with it.

use soundcraft_model::TrackId;

use super::{MtError, Multitrack, err};

impl Multitrack {
    /// Turn a track's phase invert on or off (like `set_mute`). An unknown track is refused
    /// with "no such track".
    pub fn set_invert(&mut self, id: TrackId, on: bool) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.phase_invert = on;
        Ok(())
    }

    /// Whether a track's phase is inverted. An unknown track is refused with "no such track".
    pub fn invert(&self, id: TrackId) -> Result<bool, MtError> {
        let Some(t) = self.session.track(id) else { return err("no such track") };
        Ok(t.mixer.phase_invert)
    }
}

/// Flip the sign of every sample when `invert` is true; leave the buffer alone when false.
pub fn apply_invert(samples: &mut [f32], invert: bool) {
    if invert {
        for s in samples.iter_mut() {
            *s = -*s;
        }
    }
}
