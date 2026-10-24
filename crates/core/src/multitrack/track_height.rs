//! Track height presets (roadmap C106). A multitrack track is drawn at one of a few heights
//! (micro, mini, small, medium, large, jumbo, extreme). SoundCraft's model already has the
//! `TrackHeight` type, its sizes in points and the `Track::height` field, so none of that is
//! rebuilt here. What is added, in core only, is the way to read and change a track's height by
//! track id: set an exact height, set one by its name, or step one preset taller or shorter.
//!
//! A new track is `Medium` (88 points). Stepping stops at `Micro` and `Extreme`. A custom height
//! is kept as given and drawn clamped to 16 to 440 points. A name is one of the seven preset
//! names (any case, spaces around it ignored); "custom" and "fit to window" are not names here.
//!
//! Every call that fails changes nothing. Setting returns the old height; stepping returns the
//! new height. Drawing the rows is not here.

use soundcraft_model::{TrackHeight, TrackId};

use super::{MtError, Multitrack, err};

impl Multitrack {
    /// A track's height.
    pub fn track_height(&self, id: TrackId) -> Result<TrackHeight, MtError> {
        match self.session.track(id) {
            Some(t) => Ok(t.height),
            None => err("no such track"),
        }
    }

    /// A track's height in points, as the model draws it (custom heights clamped to 16 to 440).
    pub fn track_height_points(&self, id: TrackId) -> Result<f32, MtError> {
        Ok(self.track_height(id)?.points())
    }

    /// Set a track's height; returns the height it had. An unknown track is an error.
    pub fn set_track_height(&mut self, id: TrackId, height: TrackHeight) -> Result<TrackHeight, MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        Ok(std::mem::replace(&mut t.height, height))
    }

    /// Set a track's height from a preset name; returns the height it had. An unknown name or an
    /// unknown track is an error and changes nothing.
    pub fn set_track_height_named(&mut self, id: TrackId, name: &str) -> Result<TrackHeight, MtError> {
        let Some(height) = TrackHeight::from_id(name.trim()) else {
            return err(format!("a track height is one of micro, mini, small, medium, large, jumbo, extreme, not {name:?}"));
        };
        self.set_track_height(id, height)
    }

    /// Move a track's height to the next preset taller (`taller` true) or shorter; returns the
    /// new height. At the largest or smallest preset it stays. An unknown track is an error.
    pub fn step_track_height(&mut self, id: TrackId, taller: bool) -> Result<TrackHeight, MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.height = t.height.step(taller);
        Ok(t.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;

    fn setup() -> Result<(Multitrack, TrackId), MtError> {
        let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24)?;
        let id = m.add_track(Some("Voice"), 1)?;
        Ok((m, id))
    }

    #[test]
    fn new_track_height_is_medium_88() -> Result<(), MtError> {
        let (m, id) = setup()?;
        assert_eq!(m.track_height(id), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height_points(id), Ok(88.0));
        Ok(())
    }

    #[test]
    fn set_track_height_returns_old_and_applies() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.set_track_height(id, TrackHeight::Large), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Large));
        assert_eq!(m.track_height_points(id), Ok(132.0));
        Ok(())
    }

    #[test]
    fn set_track_height_named_ignores_case_and_spaces() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.set_track_height_named(id, "  SMALL "), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height_points(id), Ok(46.0));
        Ok(())
    }

    #[test]
    fn named_unknown_and_custom_are_refused_and_change_nothing() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        let msg = |name: &str| MtError(format!("a track height is one of micro, mini, small, medium, large, jumbo, extreme, not {name:?}"));
        assert_eq!(m.set_track_height_named(id, "huge"), Err(msg("huge")));
        assert_eq!(m.set_track_height_named(id, "custom"), Err(msg("custom")));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Medium));
        Ok(())
    }

    #[test]
    fn custom_height_points_are_clamped() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Custom(5000))?;
        assert_eq!(m.track_height_points(id), Ok(440.0));
        m.set_track_height(id, TrackHeight::Custom(1))?;
        assert_eq!(m.track_height_points(id), Ok(16.0));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Custom(1)));
        Ok(())
    }

    #[test]
    fn step_taller_from_medium_is_large() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Large));
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Jumbo));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Jumbo));
        Ok(())
    }

    #[test]
    fn step_taller_stops_at_extreme() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Jumbo)?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Extreme));
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Extreme));
        Ok(())
    }

    #[test]
    fn step_shorter_stops_at_micro() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Small)?;
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Mini));
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Micro));
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Micro));
        Ok(())
    }

    #[test]
    fn step_from_custom_goes_to_the_next_preset() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Custom(100))?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Large));
        m.set_track_height(id, TrackHeight::Custom(100))?;
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Medium));
        Ok(())
    }

    #[test]
    fn unknown_track_is_refused_everywhere() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.remove_track(id)?;
        let gone = Err(MtError("no such track".to_string()));
        assert_eq!(m.track_height(id), gone);
        assert_eq!(m.track_height_points(id), Err(MtError("no such track".to_string())));
        assert_eq!(m.set_track_height(id, TrackHeight::Large), Err(MtError("no such track".to_string())));
        assert_eq!(m.set_track_height_named(id, "large"), Err(MtError("no such track".to_string())));
        assert_eq!(m.step_track_height(id, true), Err(MtError("no such track".to_string())));
        Ok(())
    }

    #[test]
    fn setting_one_track_leaves_the_others() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        let other = m.add_track(Some("Guitar"), 2)?;
        m.set_track_height(id, TrackHeight::Jumbo)?;
        assert_eq!(m.track_height(other), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Jumbo));
        Ok(())
    }
}
