//! Per-track automation (f109, roadmap C174 and C178): volume and pan lanes with editable points.
//! The lane, its points, the value range and the interpolation are SoundCraft's own
//! (`soundcraft_model::AutomationLane`); this module only adds the editing vocabulary the Multitrack
//! view and the control channel share: add, move, delete and thin, and the value at a time.
//! Playback and mixdown will follow the same lanes when they land.

use serde_json::json;
use soundcraft_model::TrackId;
use soundcraft_model::{AutoParam, AutomationLane};

use super::{MtError, Multitrack, err};

/// The parameters this alpha has lanes for.
pub const LANE_PARAMS: [AutoParam; 2] = [AutoParam::Volume, AutoParam::Pan(0)];

/// `volume` or `pan`; anything else is refused with the list.
pub fn parse_param(s: &str) -> Result<AutoParam, MtError> {
    match AutoParam::parse(s) {
        Some(p) if LANE_PARAMS.contains(&p) => Ok(p),
        _ => err(format!("automation lanes here are volume or pan, not {s:?}")),
    }
}

fn check_value(v: f32) -> Result<(), MtError> {
    if v.is_finite() { Ok(()) } else { err("a point's value must be a number") }
}

impl Multitrack {
    fn samples_at(&self, seconds: f64) -> Result<i64, MtError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return err("a time is a number of seconds, 0 or more");
        }
        let s = (seconds * f64::from(self.rate())).round();
        if s > soundcraft_model::time::MAX_POSITION as f64 {
            return err("that time is past the end of what a session can hold");
        }
        Ok(s as i64)
    }

    /// The lane of `param` on a track, if it has one yet.
    pub fn lane(&self, track: TrackId, param: &AutoParam) -> Option<&AutomationLane> {
        self.session.track(track)?.lane(param)
    }

    /// The value of the track's `param` at `seconds`: the lane's (linear between points, held at
    /// the ends), or the parameter's default when there is no lane or it has no points.
    pub fn automation_value_at(&self, track: TrackId, param: &AutoParam, seconds: f64) -> f32 {
        let default = param.range().2;
        let at = (seconds.max(0.0) * f64::from(self.rate())).round() as i64;
        self.lane(track, param).map_or(default, |l| l.value_at(at, default))
    }

    /// Add a point (or replace the one at that time). The value is clamped to the parameter's
    /// range. Returns the point's index.
    pub fn automation_add(&mut self, track: TrackId, param: &AutoParam, seconds: f64, value: f32) -> Result<usize, MtError> {
        check_value(value)?;
        let at = self.samples_at(seconds)?;
        let Some(t) = self.session.track_mut(track) else { return err("no such track") };
        let lane = t.lane_mut(param);
        lane.set_point(at, value);
        Ok(lane.points.iter().position(|p| p.at == at).unwrap_or(0))
    }

    /// Move point `index` to a new time and value. A point already at the new time is replaced.
    /// Returns the point's new index.
    pub fn automation_move(&mut self, track: TrackId, param: &AutoParam, index: usize, seconds: f64, value: f32) -> Result<usize, MtError> {
        check_value(value)?;
        let at = self.samples_at(seconds)?;
        let Some(t) = self.session.track_mut(track) else { return err("no such track") };
        let Some(lane) = t.automation.iter_mut().find(|l| &l.param == param) else { return err(format!("no {} lane yet", param.label())) };
        if index >= lane.points.len() {
            return err(format!("no point {index}: the lane has {}", lane.points.len()));
        }
        lane.points.remove(index);
        lane.set_point(at, value);
        Ok(lane.points.iter().position(|p| p.at == at).unwrap_or(0))
    }

    /// Delete point `index`.
    pub fn automation_delete(&mut self, track: TrackId, param: &AutoParam, index: usize) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(track) else { return err("no such track") };
        let Some(lane) = t.automation.iter_mut().find(|l| &l.param == param) else { return err(format!("no {} lane yet", param.label())) };
        if index >= lane.points.len() {
            return err(format!("no point {index}: the lane has {}", lane.points.len()));
        }
        lane.points.remove(index);
        Ok(())
    }

    /// Thin the lane: remove points whose removal changes the curve by less than `tolerance` (in
    /// the parameter's own unit: dB for volume, pan units for pan). Returns how many were removed.
    pub fn automation_thin(&mut self, track: TrackId, param: &AutoParam, tolerance: f32) -> Result<usize, MtError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return err("a tolerance is a number, 0 or more");
        }
        let Some(t) = self.session.track_mut(track) else { return err("no such track") };
        let Some(lane) = t.automation.iter_mut().find(|l| &l.param == param) else { return err(format!("no {} lane yet", param.label())) };
        Ok(lane.thin(tolerance))
    }

    /// The automation of every audio track as JSON: `{track name: {param: [[seconds, value]…]}}`.
    pub fn automation_json(&self) -> serde_json::Value {
        let rate = self.session.sample_rate;
        let mut out = serde_json::Map::new();
        for t in self.session.tracks.iter().filter(|t| t.kind == soundcraft_model::TrackKind::Audio) {
            let mut lanes = serde_json::Map::new();
            for l in &t.automation {
                let pts: Vec<_> = l.points.iter().map(|p| json!([rate.seconds(p.at), p.value])).collect();
                lanes.insert(l.param.label(), json!(pts));
            }
            out.insert(t.name.clone(), serde_json::Value::Object(lanes));
        }
        serde_json::Value::Object(out)
    }
}

impl Multitrack {
    /// The state line for `AUTO state`: every track's lanes, and which lanes are showing.
    pub fn automation_state(&self, shown: &[(TrackId, AutoParam)]) -> String {
        let shown: serde_json::Map<String, serde_json::Value> =
            shown.iter().filter_map(|(id, p)| self.session.track(*id).map(|t| (t.name.clone(), json!(p.label())))).collect();
        json!({"lanes": self.automation_json(), "shown": shown}).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;

    fn one_track() -> (Multitrack, TrackId) {
        let mut mt = Multitrack::new("T", 48_000, BitDepthSetting::Int24).unwrap();
        let t = mt.add_track(Some("A"), 2).unwrap();
        (mt, t)
    }

    #[test]
    fn points_are_added_in_time_order_and_replaced_at_the_same_time() {
        let (mut mt, t) = one_track();
        let v = AutoParam::Volume;
        assert_eq!(mt.automation_add(t, &v, 2.0, -6.0).unwrap(), 0);
        assert_eq!(mt.automation_add(t, &v, 1.0, 0.0).unwrap(), 0, "earlier lands first");
        assert_eq!(mt.automation_add(t, &v, 3.0, -12.0).unwrap(), 2);
        assert_eq!(mt.automation_add(t, &v, 2.0, -3.0).unwrap(), 1, "same time replaces");
        let pts = &mt.lane(t, &v).unwrap().points;
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[1].value, -3.0);
        assert!(pts.windows(2).all(|w| w[0].at < w[1].at));
    }

    #[test]
    fn values_are_clamped_and_bad_input_is_refused() {
        let (mut mt, t) = one_track();
        mt.automation_add(t, &AutoParam::Pan(0), 0.0, 5.0).unwrap();
        mt.automation_add(t, &AutoParam::Volume, 0.0, 99.0).unwrap();
        assert_eq!(mt.lane(t, &AutoParam::Pan(0)).unwrap().points[0].value, 1.0);
        assert_eq!(mt.lane(t, &AutoParam::Volume).unwrap().points[0].value, 12.0);
        assert!(mt.automation_add(t, &AutoParam::Volume, -1.0, 0.0).is_err());
        assert!(mt.automation_add(t, &AutoParam::Volume, f64::NAN, 0.0).is_err());
        assert!(mt.automation_add(t, &AutoParam::Volume, 1.0, f32::NAN).is_err());
        assert!(parse_param("mute").is_err());
        assert_eq!(parse_param("vol").unwrap(), AutoParam::Volume);
    }

    #[test]
    fn a_moved_point_changes_time_and_value_and_keeps_the_order() {
        let (mut mt, t) = one_track();
        let v = AutoParam::Volume;
        for (s, x) in [(1.0, 0.0), (2.0, -6.0), (3.0, -12.0)] {
            mt.automation_add(t, &v, s, x).unwrap();
        }
        assert_eq!(mt.automation_move(t, &v, 0, 2.5, -1.0).unwrap(), 1, "now between the others");
        let pts = &mt.lane(t, &v).unwrap().points;
        assert_eq!((pts[0].at, pts[1].at, pts[1].value), (96_000, 120_000, -1.0));
        // Dropped on another point's time: that point is replaced.
        assert_eq!(mt.automation_move(t, &v, 1, 3.0, -9.0).unwrap(), 1);
        assert_eq!(mt.lane(t, &v).unwrap().points.len(), 2);
        assert!(mt.automation_move(t, &v, 7, 1.0, 0.0).is_err());
        assert!(mt.automation_move(t, &AutoParam::Pan(0), 0, 1.0, 0.0).is_err(), "no pan lane");
    }

    #[test]
    fn a_point_is_deleted_by_index() {
        let (mut mt, t) = one_track();
        let v = AutoParam::Volume;
        for s in [1.0, 2.0, 3.0] {
            mt.automation_add(t, &v, s, -s as f32).unwrap();
        }
        mt.automation_delete(t, &v, 1).unwrap();
        let vals: Vec<f32> = mt.lane(t, &v).unwrap().points.iter().map(|p| p.value).collect();
        assert_eq!(vals, [-1.0, -3.0]);
        assert!(mt.automation_delete(t, &v, 2).is_err());
    }

    #[test]
    fn thinning_drops_points_on_a_straight_line_and_keeps_corners() {
        let (mut mt, t) = one_track();
        let v = AutoParam::Volume;
        // A ramp with a tiny wobble, then a corner back up.
        for (s, x) in [(0.0, 0.0), (1.0, -3.01), (2.0, -6.0), (3.0, -9.02), (4.0, -12.0), (5.0, 0.0)] {
            mt.automation_add(t, &v, s, x).unwrap();
        }
        let before: Vec<f32> = (0..=50).map(|i| mt.automation_value_at(t, &v, f64::from(i) / 10.0)).collect();
        let removed = mt.automation_thin(t, &v, 0.1).unwrap();
        assert_eq!(removed, 3, "the three points on the ramp go");
        let pts = &mt.lane(t, &v).unwrap().points;
        assert_eq!(pts.iter().map(|p| p.value).collect::<Vec<_>>(), [0.0, -12.0, 0.0]);
        for (i, b) in before.iter().enumerate() {
            let a = mt.automation_value_at(t, &v, i as f64 / 10.0);
            assert!((a - b).abs() <= 0.1 + 1e-4, "curve moved by {} at {}", (a - b).abs(), i);
        }
        assert_eq!(mt.automation_thin(t, &v, 0.0).unwrap(), 0, "nothing more at zero tolerance");
        assert!(mt.automation_thin(t, &v, -1.0).is_err());
    }

    #[test]
    fn the_value_at_a_time_interpolates_holds_and_defaults() {
        let (mut mt, t) = one_track();
        let v = AutoParam::Volume;
        assert_eq!(mt.automation_value_at(t, &v, 1.0), 0.0, "no lane: the default");
        mt.automation_add(t, &v, 1.0, -10.0).unwrap();
        mt.automation_add(t, &v, 3.0, 0.0).unwrap();
        assert_eq!(mt.automation_value_at(t, &v, 0.0), -10.0, "held before the first");
        assert_eq!(mt.automation_value_at(t, &v, 2.0), -5.0, "halfway");
        assert_eq!(mt.automation_value_at(t, &v, 2.5), -2.5);
        assert_eq!(mt.automation_value_at(t, &v, 9.0), 0.0, "held after the last");
        let p = AutoParam::Pan(0);
        mt.automation_add(t, &p, 0.0, -1.0).unwrap();
        mt.automation_add(t, &p, 4.0, 1.0).unwrap();
        assert_eq!(mt.automation_value_at(t, &p, 2.0), 0.0);
    }

    #[test]
    fn the_json_lists_each_tracks_lanes_in_seconds() {
        let (mut mt, t) = one_track();
        mt.automation_add(t, &AutoParam::Volume, 0.5, -6.0).unwrap();
        let j = mt.automation_json();
        assert_eq!(j["A"]["volume"], serde_json::json!([[0.5, -6.0]]));
    }
}
