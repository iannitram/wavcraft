//! The Mixer panel (roadmap C152): a channel strip per track and a master strip, built on
//! SoundCraft's crates. The strip state is `soundcraft_model::Mixer` (fader in dB, pan, mute,
//! solo), the fader taper is the model's `fader_pos_to_db` / `fader_db_to_pos`, the pan law is
//! `soundcraft_dsp::pan` and the meters are the mix engine's `StripMeter`s.
//!
//! The multitrack session is another feature's (issue 305), so the panel runs a small session of
//! its own, made in code: a few tracks of synthetic tones. When the multitrack session lands, the
//! panel is pointed at its `Session` instead (`MixerModel::session_mut`).

use std::collections::HashMap;
use std::sync::Arc;

use soundcraft_dsp::pan::PanLaw;
use soundcraft_mix::MixEngine;
use soundcraft_model::time::SampleRate;
use soundcraft_model::{ChannelFormat, Clip, Session, SourceAudio, SourceId, TrackId, TrackKind, fader_db_to_pos, fader_pos_to_db};

use soundcraft_audio_io::AudioBuffer;

const RATE: u32 = 48_000;
/// Frames rendered for one meter reading.
const BLOCK: usize = 1024;
/// Seconds of tone in each demo track (the meter loops over it).
const TONE_SECONDS: usize = 4;
/// Below this a fader reads "-inf" and a meter is empty.
pub const FLOOR_DB: f32 = -60.0;
/// Where the control channel measures (a steady stretch of the tones).
const MEASURE_AT: i64 = 9_600;

/// One strip's settings as the control channel and the tests see them.
#[derive(Debug, Clone, PartialEq)]
pub struct StripState {
    pub name: String,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Not silenced by a mute, or by another track's solo.
    pub audible: bool,
    /// Peak of the strip's loudest channel, dB (floor at `-144`).
    pub peak_db: f32,
}

/// What `MIXER set` changes; `None` leaves a setting alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SetCmd {
    pub vol: Option<f32>,
    pub pan: Option<f32>,
    pub mute: Option<bool>,
    pub solo: Option<bool>,
}

/// Parse the `vol=<db> pan=<p> mute=0|1 solo=0|1` words of `MIXER set <track> ...`.
pub fn parse_set(words: &[&str]) -> Result<SetCmd, String> {
    let mut cmd = SetCmd::default();
    if words.is_empty() {
        return Err("nothing to set: vol=<db> pan=<p> mute=0|1 solo=0|1".into());
    }
    for w in words {
        let (k, v) = w.split_once('=').ok_or_else(|| format!("{w}: expected key=value"))?;
        let flag = |v: &str| match v {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(format!("{k}={v}: expected 0 or 1")),
        };
        let num = |v: &str| v.parse::<f32>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("{k}={v}: expected a number"));
        match k {
            "vol" => cmd.vol = Some(num(v)?),
            "pan" => cmd.pan = Some(num(v)?),
            "mute" => cmd.mute = Some(flag(v)?),
            "solo" => cmd.solo = Some(flag(v)?),
            _ => return Err(format!("unknown setting {k}")),
        }
    }
    Ok(cmd)
}

pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-7 { -144.0 } else { 20.0 * g.log10() }
}

/// dB text for a fader or a meter: `-inf` at the floor.
pub fn db_text(db: f32) -> String {
    if db <= -143.0 { "-inf".to_string() } else { format!("{db:.1}") }
}

/// The panel's session, the mix engine that meters it and the last readings.
pub struct MixerModel {
    session: Session,
    engine: MixEngine,
    tracks: Vec<TrackId>,
    master: TrackId,
    pos: i64,
    peaks: HashMap<TrackId, f32>,
    master_peak: f32,
    pub pan_law: PanLaw,
}

impl MixerModel {
    /// `n` mono tracks of a sine each (different pitches, 0.5 amplitude) and a master.
    pub fn demo(n: usize) -> MixerModel {
        let mut session = Session::new("Mixer", SampleRate::HZ_48000);
        let frames = RATE as usize * TONE_SECONDS;
        let mut tracks = Vec::new();
        for i in 0..n {
            let t = session.add_track(TrackKind::Audio, ChannelFormat::Mono, Some(&format!("Track {}", i + 1)));
            let hz = 220.0 * 2f32.powf(i as f32 / 2.0);
            let tone: Vec<f32> = (0..frames).map(|k| 0.5 * (std::f32::consts::TAU * hz * k as f32 / RATE as f32).sin()).collect();
            let buffer = AudioBuffer { sample_rate: RATE, channels: vec![tone] };
            let src = SourceId(1000 + i as u64);
            session.pool.insert(src, Arc::new(SourceAudio::new(buffer)));
            let cid = session.new_clip_id();
            if let Some(pl) = session.track_mut(t).and_then(|t| t.playlist_mut()) {
                pl.clips.push(Clip::audio(cid, "tone", src, 0, 0, frames as i64));
            }
            tracks.push(t);
        }
        let master = session.add_track(TrackKind::Master, ChannelFormat::Stereo, None);
        MixerModel {
            session,
            engine: MixEngine::new(RATE as f32, BLOCK),
            tracks,
            master,
            pos: 0,
            peaks: HashMap::new(),
            master_peak: 0.0,
            pan_law: PanLaw::Minus3,
        }
    }

    #[allow(dead_code)] // the seam for the multitrack session (issue 305)
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// The track ids, in strip order (the master is separate).
    #[cfg(test)]
    pub fn track_ids(&self) -> &[TrackId] {
        &self.tracks
    }

    fn any_solo(&self) -> bool {
        self.session.tracks.iter().any(|t| t.mixer.solo && !t.inactive)
    }

    /// The mix engine's rule: silent when muted, or when another track is soloed (unless solo-safe).
    pub fn audible(&self, id: TrackId) -> bool {
        let Some(t) = self.session.track(id) else { return false };
        if t.kind == TrackKind::Master {
            return true;
        }
        !(t.mixer.mute || (self.any_solo() && !t.mixer.solo && !t.mixer.solo_safe))
    }

    /// Apply `cmd` to a strip; the model clamps what it is given (fader -144..+12 dB, pan -1..1).
    pub fn set(&mut self, id: TrackId, cmd: SetCmd) -> Result<(), String> {
        let t = self.session.track_mut(id).ok_or("no such track")?;
        if let Some(v) = cmd.vol {
            t.mixer.volume_db = v;
        }
        if let Some(p) = cmd.pan {
            // A mono strip has one pan; a stereo strip's pan is a balance both channels carry.
            let n = t.mixer.pan.len().max(1);
            t.mixer.pan = vec![p; n];
        }
        if let Some(m) = cmd.mute {
            t.mixer.mute = m;
        }
        if let Some(s) = cmd.solo {
            t.mixer.solo = s;
        }
        t.mixer.sanitize();
        Ok(())
    }

    /// Track by 1-based number, or `master`.
    pub fn resolve(&self, word: &str) -> Result<TrackId, String> {
        if word.eq_ignore_ascii_case("master") {
            return Ok(self.master);
        }
        let n: usize = word.parse().map_err(|_| format!("{word}: a track number from 1, or master"))?;
        n.checked_sub(1).and_then(|i| self.tracks.get(i)).copied().ok_or_else(|| format!("no track {n}"))
    }

    /// Fader position (0..1, the model's taper) of a strip.
    pub fn fader_pos(&self, id: TrackId) -> f32 {
        self.session.track(id).map_or(0.0, |t| fader_db_to_pos(t.mixer.volume_db))
    }

    /// Move a fader by position through the model's taper.
    pub fn set_fader_pos(&mut self, id: TrackId, pos: f32) {
        let _ = self.set(id, SetCmd { vol: Some(fader_pos_to_db(pos)), ..SetCmd::default() });
    }

    /// Render the next block of the session and read the meters (the panel calls this each frame).
    pub fn step(&mut self) {
        let len = (RATE as usize * TONE_SECONDS) as i64;
        if self.pos + BLOCK as i64 >= len {
            self.pos = 0;
        }
        let at = self.pos;
        self.pos += BLOCK as i64;
        self.render_at(at);
    }

    /// A steady reading for the control channel: two blocks from a fixed place.
    pub fn measure(&mut self) {
        self.render_at(MEASURE_AT);
        self.render_at(MEASURE_AT + BLOCK as i64);
    }

    fn render_at(&mut self, at: i64) {
        let mut out = vec![vec![0.0f32; BLOCK]; 2];
        self.engine.pan_law = self.pan_law;
        self.engine.render(&self.session, at, BLOCK, &mut out);
        for (id, m) in &self.engine.meters {
            self.peaks.insert(*id, m.peaks.iter().copied().fold(0.0, f32::max));
        }
        self.master_peak = self.engine.main_meter.peaks.iter().copied().fold(0.0, f32::max);
    }

    /// Peak dB of a strip at the last reading (the master's is the main mix).
    pub fn peak_db(&self, id: TrackId) -> f32 {
        let lin = if id == self.master { self.master_peak } else { self.peaks.get(&id).copied().unwrap_or(0.0) };
        gain_to_db(lin)
    }

    pub fn strip(&self, id: TrackId) -> Option<StripState> {
        let t = self.session.track(id)?;
        let pan = t.mixer.pan.first().copied().unwrap_or(0.0);
        Some(StripState {
            name: t.name.clone(),
            volume_db: t.mixer.volume_db,
            pan,
            mute: t.mixer.mute,
            solo: t.mixer.solo,
            audible: self.audible(id),
            peak_db: self.peak_db(id),
        })
    }

    /// `MIXER state`, one line: `OK tracks=<n> any_solo=<0|1> | master ... | t1 ... | t2 ...`.
    pub fn state_line(&mut self) -> String {
        self.measure();
        let one = |label: &str, s: &StripState| {
            format!(
                "{label} vol={:.2} pan={:.2} mute={} solo={} audible={} peak={:.1}",
                s.volume_db,
                s.pan,
                u8::from(s.mute),
                u8::from(s.solo),
                u8::from(s.audible),
                s.peak_db
            )
        };
        let mut line = format!("OK tracks={} any_solo={}", self.tracks.len(), u8::from(self.any_solo()));
        if let Some(s) = self.strip(self.master) {
            line.push_str(&format!(" | {}", one("master", &s)));
        }
        for (i, id) in self.tracks.clone().iter().enumerate() {
            if let Some(s) = self.strip(*id) {
                line.push_str(&format!(" | {}", one(&format!("t{}", i + 1), &s)));
            }
        }
        line
    }

    /// The control channel's `MIXER <state | set ...>` (what follows the verb).
    pub fn control(&mut self, rest: &str) -> String {
        let words: Vec<&str> = rest.split_whitespace().collect();
        match words.as_slice() {
            ["state"] => self.state_line(),
            ["set", track, settings @ ..] => {
                let done = self.resolve(track).and_then(|id| parse_set(settings).and_then(|c| self.set(id, c)));
                match done {
                    Ok(()) => self.state_line(),
                    Err(e) => format!("ERR MIXER set: {e}"),
                }
            }
            _ => "ERR MIXER state | MIXER set <track|master> vol=<db> pan=<p> mute=0|1 solo=0|1".to_string(),
        }
    }

    /// Render the main mix of the session over `range` offline (tests).
    #[cfg(test)]
    fn render_main(&self, range: soundcraft_model::time::Range) -> Vec<Vec<f32>> {
        soundcraft_mix::render_range(&self.session, range, 512)
    }

    /// The panel: one strip per track, then the master.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.step();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        let mut ids: Vec<TrackId> = self.tracks.clone();
        ids.push(self.master);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for id in ids {
                    self.strip_ui(ui, id);
                    ui.separator();
                }
            });
        });
    }

    fn strip_ui(&mut self, ui: &mut egui::Ui, id: TrackId) {
        let Some(s) = self.strip(id) else { return };
        let is_master = id == self.master;
        ui.vertical(|ui| {
            ui.set_width(74.0);
            ui.label(egui::RichText::new(&s.name).strong());
            if !is_master {
                ui.horizontal(|ui| {
                    if ui.selectable_label(s.mute, "M").on_hover_text("Mute").clicked() {
                        let _ = self.set(id, SetCmd { mute: Some(!s.mute), ..SetCmd::default() });
                    }
                    if ui.selectable_label(s.solo, "S").on_hover_text("Solo").clicked() {
                        let _ = self.set(id, SetCmd { solo: Some(!s.solo), ..SetCmd::default() });
                    }
                });
                let mut pan = s.pan;
                if ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false)).on_hover_text(format!("Pan {pan:+.2}")).changed() {
                    let _ = self.set(id, SetCmd { pan: Some(pan), ..SetCmd::default() });
                }
            }
            ui.horizontal(|ui| {
                let mut pos = self.fader_pos(id);
                let r = ui.add(egui::Slider::new(&mut pos, 0.0..=1.0).vertical().show_value(false));
                if r.changed() {
                    self.set_fader_pos(id, pos);
                }
                if r.double_clicked() {
                    let _ = self.set(id, SetCmd { vol: Some(0.0), ..SetCmd::default() });
                }
                meter_ui(ui, s.peak_db, s.audible);
            });
            ui.label(format!("{} dB", db_text(s.volume_db)));
            ui.weak(format!("pk {}", db_text(s.peak_db)));
        });
    }
}

/// A vertical peak bar from `FLOOR_DB` to +6 dB; green, amber over -12, red over -3.
fn meter_ui(ui: &mut egui::Ui, peak_db: f32, audible: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 120.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, egui::Color32::from_gray(30));
    let top = 6.0;
    let frac = ((peak_db.clamp(FLOOR_DB, top) - FLOOR_DB) / (top - FLOOR_DB)).clamp(0.0, 1.0);
    let h = rect.height() * frac;
    let colour = if peak_db > -3.0 {
        egui::Color32::from_rgb(220, 60, 50)
    } else if peak_db > -12.0 {
        egui::Color32::from_rgb(220, 180, 50)
    } else if audible {
        egui::Color32::from_rgb(70, 190, 90)
    } else {
        egui::Color32::from_gray(90)
    };
    let fill = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - h), rect.right_bottom());
    p.rect_filled(fill, 2.0, colour);
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_dsp::pan::gains;
    use soundcraft_model::time::Range;

    fn model() -> MixerModel {
        MixerModel::demo(3)
    }

    fn id(m: &MixerModel, n: usize) -> TrackId {
        m.track_ids()[n]
    }

    #[test]
    fn the_fader_taper_round_trips_through_the_model() {
        let mut m = model();
        let t = id(&m, 0);
        for db in [-96.0, -60.0, -24.0, -12.0, -6.0, 0.0, 3.0, 12.0] {
            m.set(t, SetCmd { vol: Some(db), ..SetCmd::default() }).unwrap();
            let pos = m.fader_pos(t);
            assert!((0.0..=1.0).contains(&pos), "{db} dB gave position {pos}");
            m.set_fader_pos(t, pos);
            let back = m.strip(t).unwrap().volume_db;
            assert!((back - db).abs() < 0.05, "{db} dB -> position {pos} -> {back} dB");
        }
        // Positions go through the model's volume and come back, and rise with the fader.
        let mut last = f32::MIN;
        for i in 0..=100 {
            let pos = i as f32 / 100.0;
            m.set_fader_pos(t, pos);
            let db = m.strip(t).unwrap().volume_db;
            assert!(db >= last, "the taper must not fall: {last} then {db} at {pos}");
            last = db;
            if db > -100.0 {
                assert!((m.fader_pos(t) - pos).abs() < 0.001, "position {pos} -> {db} dB -> {}", m.fader_pos(t));
            }
        }
        // Unity sits at a fixed place, the top at +12 dB, the bottom at -inf.
        m.set_fader_pos(t, 1.0);
        assert_eq!(m.strip(t).unwrap().volume_db, 12.0);
        m.set_fader_pos(t, 0.0);
        assert_eq!(db_text(m.strip(t).unwrap().volume_db), "-inf");
    }

    #[test]
    fn the_model_clamps_a_fader_and_a_pan_out_of_range() {
        let mut m = model();
        let t = id(&m, 1);
        m.set(t, SetCmd { vol: Some(40.0), pan: Some(3.0), ..SetCmd::default() }).unwrap();
        let s = m.strip(t).unwrap();
        assert_eq!((s.volume_db, s.pan), (12.0, 1.0));
    }

    #[test]
    fn the_pan_law_is_the_dsp_laws_and_the_engine_follows_it() {
        // The law itself: centre -3 dB, hard sides full, constant power across the sweep.
        let (l, r) = gains(0.0, PanLaw::Minus3);
        assert!((gain_to_db(l) + 3.01).abs() < 0.05 && (l - r).abs() < 1e-6);
        assert_eq!(gains(-1.0, PanLaw::Minus3), (1.0, 0.0));
        for i in -10..=10 {
            let (l, r) = gains(i as f32 / 10.0, PanLaw::Minus3);
            assert!((l * l + r * r - 1.0).abs() < 0.01, "pan {i}: {l} {r}");
        }
        // Through the model: pan a track hard left and the left channel carries the tone alone.
        let mut m = model();
        let t = id(&m, 0);
        for (pan, want_l, want_r) in [(-1.0, true, false), (1.0, false, true), (0.0, true, true)] {
            m.set(t, SetCmd { pan: Some(pan), ..SetCmd::default() }).unwrap();
            for other in [id(&m, 1), id(&m, 2)] {
                m.set(other, SetCmd { mute: Some(true), ..SetCmd::default() }).unwrap();
            }
            let out = m.render_main(Range::new(9_600, 12_000));
            let peak = |c: &Vec<f32>| c.iter().fold(0.0f32, |a, b| a.max(b.abs()));
            assert_eq!(peak(&out[0]) > 0.1, want_l, "pan {pan}: left {}", peak(&out[0]));
            assert_eq!(peak(&out[1]) > 0.1, want_r, "pan {pan}: right {}", peak(&out[1]));
        }
        // Centre: the tone (0.5) is 3 dB down on each side.
        m.set(t, SetCmd { pan: Some(0.0), ..SetCmd::default() }).unwrap();
        let out = m.render_main(Range::new(9_600, 12_000));
        let peak = out[0].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!((gain_to_db(peak) - (gain_to_db(0.5) - 3.01)).abs() < 0.3, "centre peak {} dB", gain_to_db(peak));
    }

    #[test]
    fn the_meter_follows_the_fader_and_the_pan() {
        let mut m = model();
        let t = id(&m, 0);
        m.set(t, SetCmd { vol: Some(0.0), pan: Some(-1.0), ..SetCmd::default() }).unwrap();
        m.measure();
        let full = m.peak_db(t);
        assert!((full - gain_to_db(0.5)).abs() < 0.5, "unity, hard left: {full} dB");
        m.set(t, SetCmd { vol: Some(-12.0), ..SetCmd::default() }).unwrap();
        m.measure();
        let down = m.peak_db(t);
        assert!((full - down - 12.0).abs() < 0.5, "12 dB down: {full} -> {down}");
    }

    #[test]
    fn mute_silences_a_strip_and_the_meter() {
        let mut m = model();
        let (a, b) = (id(&m, 0), id(&m, 1));
        m.measure();
        assert!(m.audible(a) && m.peak_db(a) > -20.0);
        m.set(a, SetCmd { mute: Some(true), ..SetCmd::default() }).unwrap();
        m.measure();
        assert!(!m.audible(a));
        assert!(m.peak_db(a) < -100.0, "muted strip still reads {} dB", m.peak_db(a));
        assert!(m.audible(b) && m.peak_db(b) > -20.0, "the others play on");
        m.set(a, SetCmd { mute: Some(false), ..SetCmd::default() }).unwrap();
        assert!(m.audible(a));
    }

    #[test]
    fn solo_silences_the_others_and_several_solos_stack() {
        let mut m = model();
        let (a, b, c) = (id(&m, 0), id(&m, 1), id(&m, 2));
        m.set(b, SetCmd { solo: Some(true), ..SetCmd::default() }).unwrap();
        assert_eq!((m.audible(a), m.audible(b), m.audible(c)), (false, true, false));
        m.measure();
        assert!(m.peak_db(a) < -100.0 && m.peak_db(c) < -100.0 && m.peak_db(b) > -20.0);
        m.set(c, SetCmd { solo: Some(true), ..SetCmd::default() }).unwrap();
        assert_eq!((m.audible(a), m.audible(b), m.audible(c)), (false, true, true));
        // Mute beats solo: a soloed, muted strip is silent.
        m.set(b, SetCmd { mute: Some(true), ..SetCmd::default() }).unwrap();
        assert!(!m.audible(b));
        // Clearing every solo brings the rest back.
        for t in [b, c] {
            m.set(t, SetCmd { solo: Some(false), mute: Some(false), ..SetCmd::default() }).unwrap();
        }
        assert!(m.audible(a) && m.audible(b) && m.audible(c));
        // The rule agrees with what the engine renders: only the soloed track is in the mix.
        m.set(a, SetCmd { solo: Some(true), ..SetCmd::default() }).unwrap();
        m.set(a, SetCmd { pan: Some(-1.0), ..SetCmd::default() }).unwrap();
        let out = m.render_main(Range::new(9_600, 12_000));
        let rms = |c: &Vec<f32>| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt();
        assert!((rms(&out[0]) - 0.5 / 2f32.sqrt()).abs() < 0.02, "only track 1 should sound: {}", rms(&out[0]));
    }

    #[test]
    fn control_lines_parse_and_apply() {
        assert_eq!(
            parse_set(&["vol=-6", "pan=0.5", "mute=1", "solo=0"]).unwrap(),
            SetCmd { vol: Some(-6.0), pan: Some(0.5), mute: Some(true), solo: Some(false) }
        );
        assert!(parse_set(&[]).is_err());
        assert!(parse_set(&["mute=2"]).is_err());
        assert!(parse_set(&["vol=loud"]).is_err());
        assert!(parse_set(&["vol=NaN"]).is_err());
        assert!(parse_set(&["colour=red"]).is_err());
        let mut m = model();
        let ok = m.control("set 2 vol=-6 pan=-0.5 mute=1");
        assert!(ok.starts_with("OK tracks=3"), "{ok}");
        let t2 = ok.split(" | ").find(|p| p.starts_with("t2 ")).unwrap();
        assert!(t2.contains("vol=-6.00") && t2.contains("pan=-0.50") && t2.contains("mute=1") && t2.contains("audible=0"), "{t2}");
        assert!(m.control("set 9 vol=0").starts_with("ERR"));
        assert!(m.control("set master vol=-3").starts_with("OK"));
        assert!(m.control("nonsense").starts_with("ERR"));
        assert!(m.control("state").contains("| master vol=-3.00"));
    }
}
