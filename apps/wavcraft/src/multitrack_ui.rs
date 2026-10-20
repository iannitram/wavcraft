//! The Multitrack editor view (f103): track headers (name, mute, solo, volume) and a timeline
//! that draws each track's clips as plain boxes, over `wavcraft_core::multitrack`. Also the
//! control verbs `SESSION`, `TRACK`, `CLIP` and `VIEW`, so a script can drive it.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use soundcraft_playback::Player as DevicePlayer;
use wavcraft_core::Document;
use wavcraft_core::mtplay::{PlaybackSession, SilentPlayer};
use wavcraft_core::multitrack::soundcraft_model::{BitDepthSetting, TrackId};
use wavcraft_core::multitrack::{MtError, Multitrack, bit_depth_from_bits, bits_of};

const HEADER_W: f32 = 230.0;
const ROW_H: f32 = 64.0;
const RULER_H: f32 = 20.0;
const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);

/// The session's transport (f114): a device player, or the silent clock under `WAVCRAFT_NO_AUDIO=1`.
/// Only output is ever opened; no input device.
#[derive(Default)]
pub struct Transport {
    device: Option<DevicePlayer>,
    silent: Option<SilentPlayer>,
    snapshot: Option<Arc<PlaybackSession>>,
    playing: bool,
}

impl Transport {
    fn no_audio() -> bool {
        std::env::var_os("WAVCRAFT_NO_AUDIO").is_some_and(|v| v == "1")
    }

    /// Snapshot the session and play it from `from` (session frames).
    pub fn play(&mut self, mt: &Multitrack, from: i64) {
        self.stop();
        let ps = Arc::new(PlaybackSession::new(mt));
        if Self::no_audio() {
            match &self.silent {
                Some(p) => p.set_session(Arc::clone(&ps)),
                None => self.silent = Some(SilentPlayer::new(Arc::clone(&ps))),
            }
            if let Some(p) = &self.silent {
                p.play(from);
            }
        } else {
            let p = DevicePlayer::new(Arc::clone(&ps.session));
            p.play(from, None, None);
            self.device = Some(p);
        }
        self.snapshot = Some(ps);
        self.playing = true;
    }

    pub fn stop(&mut self) {
        if let Some(p) = &self.silent {
            p.stop();
        }
        if let Some(p) = self.device.take() {
            p.stop();
        }
        self.playing = false;
    }

    /// The playhead, in session frames.
    pub fn position(&self) -> i64 {
        match (&self.silent, &self.device) {
            (Some(p), _) if Self::no_audio() => p.position(),
            (_, Some(p)) => p.position(),
            _ => 0,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn underruns(&self) -> u64 {
        self.snapshot.as_ref().map_or(0, |s| s.underruns())
    }

    /// Stops at the end of the content; call every frame.
    fn tick(&mut self, end: i64) {
        if self.playing && self.position() >= end {
            self.stop();
        }
    }
}

/// File › New › Multitrack Session…: the fields being typed.
pub struct NewDialog {
    name: String,
    rate: u32,
    bits: u32,
}

/// The Multitrack view's state: the session (if one was made), which view is up, and the small
/// bits of UI state around it.
pub struct MtView {
    pub mt: Option<Multitrack>,
    /// The Multitrack editor is the one shown (otherwise the Waveform editor).
    pub show: bool,
    dialog: Option<NewDialog>,
    selected: Option<TrackId>,
    rename: Option<(TrackId, String)>,
    pub message: Option<String>,
    pub transport: Transport,
    px_per_sec: f32,
    import_at: f64,
    sessions_made: u32,
}

impl Default for MtView {
    fn default() -> Self {
        MtView {
            mt: None,
            show: false,
            dialog: None,
            selected: None,
            rename: None,
            message: None,
            transport: Transport::default(),
            px_per_sec: 60.0,
            import_at: 0.0,
            sessions_made: 0,
        }
    }
}

impl MtView {
    pub fn open_dialog(&mut self) {
        self.sessions_made += 1;
        self.dialog = Some(NewDialog { name: format!("Session {}", self.sessions_made), rate: 48_000, bits: 24 });
    }

    fn create(&mut self, name: &str, rate: u32, depth: BitDepthSetting) -> Result<(), MtError> {
        self.mt = Some(Multitrack::new(name, rate, depth)?);
        self.transport.stop();
        self.selected = None;
        self.rename = None;
        self.show = true;
        Ok(())
    }

    /// The "new session" dialog, when it is up.
    pub fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.dialog else { return };
        let (mut create, mut cancel) = (false, false);
        egui::Window::new("New Multitrack Session").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ctx,
            |ui| {
                egui::Grid::new("new-session").num_columns(2).show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut d.name);
                    ui.end_row();
                    ui.label("Sample rate");
                    egui::ComboBox::from_id_salt("new-rate").selected_text(format!("{} Hz", d.rate)).show_ui(ui, |ui| {
                        for r in RATES {
                            ui.selectable_value(&mut d.rate, r, format!("{r} Hz"));
                        }
                    });
                    ui.end_row();
                    ui.label("Bit depth");
                    egui::ComboBox::from_id_salt("new-bits")
                        .selected_text(if d.bits == 32 { "32-bit float".to_string() } else { format!("{}-bit", d.bits) })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.bits, 16, "16-bit");
                            ui.selectable_value(&mut d.bits, 24, "24-bit");
                            ui.selectable_value(&mut d.bits, 32, "32-bit float");
                        });
                    ui.end_row();
                });
                if let Some(m) = &self.message {
                    ui.colored_label(Color32::from_rgb(235, 80, 70), m);
                }
                ui.horizontal(|ui| {
                    create = ui.button("Create").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            },
        );
        if create {
            let (name, rate, bits) = (d.name.clone(), d.rate, d.bits);
            let depth = bit_depth_from_bits(bits).unwrap_or_default();
            self.message = match self.create(&name, rate, depth) {
                Ok(()) => {
                    self.dialog = None;
                    None
                }
                Err(e) => Some(e.to_string()),
            };
        } else if cancel {
            self.dialog = None;
        }
    }

    /// The Multitrack editor: toolbar, then a header column and the timeline for each track.
    pub fn ui(&mut self, ui: &mut egui::Ui, doc: Option<&Document>) {
        let Some(mt) = &mut self.mt else {
            ui.centered_and_justified(|ui| {
                ui.label("File › New › Multitrack Session… makes a session; View › Waveform editor goes back to the file.");
            });
            return;
        };
        let mut failure: Option<String> = None;
        ui.horizontal(|ui| {
            ui.strong(format!(
                "{} · {} Hz · {}",
                mt.name(),
                mt.rate(),
                if bits_of(mt.session.bit_depth) == 32 { "32-bit float".into() } else { format!("{}-bit", bits_of(mt.session.bit_depth)) }
            ));
            ui.separator();
            let end_frames = mt.session.content_end();
            self.transport.tick(end_frames);
            if self.transport.is_playing() {
                if ui.button("■ Stop").clicked() {
                    self.transport.stop();
                }
                ui.ctx().request_repaint();
            } else if ui.add_enabled(end_frames > 0, egui::Button::new("▶ Play")).on_hover_text("play the session from the start").clicked() {
                self.transport.play(mt, 0);
            }
            let at = self.transport.position() as f64 / f64::from(mt.rate());
            ui.monospace(format!("{:02}:{:05.2}", (at / 60.0) as u32, at % 60.0));
            ui.separator();
            if ui.button("Add Stereo Track").clicked() {
                note(&mut failure, mt.add_track(None, 2).map(|_| ()));
            }
            if ui.button("Add Mono Track").clicked() {
                note(&mut failure, mt.add_track(None, 1).map(|_| ()));
            }
            ui.separator();
            ui.label("Clip at");
            ui.add(egui::DragValue::new(&mut self.import_at).speed(0.1).range(0.0..=36_000.0).suffix(" s"));
            let target = self.selected.filter(|id| mt.session.track(*id).is_some()).or_else(|| mt.track_ids().first().copied());
            if ui.add_enabled(target.is_some(), egui::Button::new("Import File as Clip…")).on_hover_text("onto the selected track").clicked()
                && let Some(t) = target
                && let Some(path) =
                    rfd::FileDialog::new().add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"]).pick_file()
            {
                note(&mut failure, mt.add_clip_file(t, &path, self.import_at).map(|_| ()));
            }
            if ui.add_enabled(target.is_some() && doc.is_some(), egui::Button::new("Import Open Document as Clip")).clicked()
                && let (Some(t), Some(d)) = (target, doc)
            {
                note(&mut failure, mt.add_clip_document(t, d, self.import_at).map(|_| ()));
            }
            ui.separator();
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut self.px_per_sec, 5.0..=400.0).logarithmic(true).show_value(false));
        });
        if let Some(m) = &self.message {
            ui.colored_label(Color32::from_rgb(235, 80, 70), m);
        }
        ui.separator();
        let ids = mt.track_ids();
        let width = (((mt.end_seconds() + 10.0).max(30.0)) as f32 * self.px_per_sec).max(200.0);
        let mut remove = None;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            // The time ruler, offset by the header column.
            let (rr, _) = ui.allocate_exact_size(vec2(HEADER_W + width, RULER_H), Sense::hover());
            let painter = ui.painter_at(rr);
            let step = ruler_step(60.0 / self.px_per_sec);
            let mut s = 0.0_f64;
            while (s as f32) * self.px_per_sec < width {
                let x = rr.left() + HEADER_W + s as f32 * self.px_per_sec;
                painter.line_segment([pos2(x, rr.bottom() - 6.0), pos2(x, rr.bottom())], Stroke::new(1.0, Color32::GRAY));
                painter.text(pos2(x + 3.0, rr.top()), egui::Align2::LEFT_TOP, format!("{s}s"), egui::FontId::monospace(10.0), Color32::GRAY);
                s += step;
            }
            for id in ids {
                let Some(t) = mt.session.track(id) else { continue };
                let (name, chans, clips, [r, g, b]) = (t.name.clone(), t.channels(), t.clips().to_vec(), t.color);
                let (mut mute, mut solo, mut vol) = (t.mixer.mute, t.mixer.solo, t.mixer.volume_db);
                let (row, _) = ui.allocate_exact_size(vec2(HEADER_W + width, ROW_H), Sense::hover());
                let header = Rect::from_min_size(row.min, vec2(HEADER_W - 4.0, ROW_H));
                let lane = Rect::from_min_size(pos2(row.left() + HEADER_W, row.top()), vec2(width, ROW_H));
                let selected = self.selected == Some(id);
                let p = ui.painter();
                p.rect_filled(header, 3.0, if selected { Color32::from_rgb(44, 52, 66) } else { Color32::from_rgb(32, 36, 44) });
                p.rect_filled(lane, 0.0, BACKGROUND);
                let colour = Color32::from_rgb(r, g, b);
                p.rect_filled(Rect::from_min_size(header.min, vec2(4.0, ROW_H)), 0.0, colour);
                // Header: name (click to select, double-click to rename), M, S, volume, remove.
                let mut h =
                    ui.new_child(egui::UiBuilder::new().max_rect(header.shrink2(vec2(8.0, 3.0))).layout(egui::Layout::top_down(egui::Align::Min)));
                h.horizontal(|ui| {
                    if let Some((rid, text)) = self.rename.as_mut().filter(|(rid, _)| *rid == id) {
                        let resp = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                        resp.request_focus();
                        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                        if enter {
                            let to = text.clone();
                            note(&mut failure, mt.rename_track(*rid, &to));
                            self.rename = None;
                        } else if esc {
                            self.rename = None;
                        }
                    } else {
                        let l = ui.add(egui::Label::new(egui::RichText::new(&name).strong()).sense(Sense::click()));
                        if l.clicked() {
                            self.selected = Some(id);
                        }
                        if l.double_clicked() {
                            self.rename = Some((id, name.clone()));
                        }
                        ui.weak(if chans == 1 { "mono" } else { "stereo" });
                    }
                });
                h.horizontal(|ui| {
                    ui.toggle_value(&mut mute, "M").on_hover_text("Mute");
                    ui.toggle_value(&mut solo, "S").on_hover_text("Solo");
                    if ui.small_button("Rename").clicked() {
                        self.rename = Some((id, name.clone()));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(id);
                    }
                });
                h.add(egui::Slider::new(&mut vol, -60.0..=12.0).suffix(" dB").fixed_decimals(1));
                let _ = (mt.set_mute(id, mute), mt.set_solo(id, solo), mt.set_volume_db(id, vol));
                // Timeline: the clips, drawn as boxes with their names.
                let lane_resp = ui.interact(lane, ui.id().with(("lane", id.0)), Sense::click());
                if lane_resp.clicked() {
                    self.selected = Some(id);
                }
                let rate = f64::from(mt.rate());
                let p = ui.painter_at(lane);
                if self.transport.is_playing() {
                    let x = lane.left() + (self.transport.position() as f64 / rate) as f32 * self.px_per_sec;
                    p.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(1.5, Color32::from_rgb(255, 200, 60)));
                }
                for c in &clips {
                    let x0 = lane.left() + (c.start as f64 / rate) as f32 * self.px_per_sec;
                    let w = ((c.length as f64 / rate) as f32 * self.px_per_sec).max(2.0);
                    let cr = Rect::from_min_size(pos2(x0, lane.top() + 4.0), vec2(w, ROW_H - 8.0));
                    let fill = if mute { colour.gamma_multiply(0.25) } else { colour.gamma_multiply(0.6) };
                    p.rect_filled(cr, 3.0, fill);
                    p.rect_stroke(cr, 3.0, Stroke::new(1.0, colour), StrokeKind::Inside);
                    p.text(cr.left_top() + vec2(4.0, 3.0), egui::Align2::LEFT_TOP, &c.name, egui::FontId::proportional(12.0), Color32::WHITE);
                }
            }
            if mt.track_count() == 0 {
                ui.label("No tracks yet: Add Stereo Track or Add Mono Track.");
            }
        });
        if let Some(id) = remove {
            if self.selected == Some(id) {
                self.selected = None;
            }
            note(&mut failure, mt.remove_track(id));
        }
        if failure.is_some() {
            self.message = failure;
        }
    }

    /// One control line for `SESSION`, `TRACK`, `CLIP` or `VIEW`; the answer starts `OK` or `ERR`.
    pub fn control(&mut self, cmd: &str, rest: &str, doc: Option<&Document>) -> String {
        let words = match tokens(rest) {
            Ok(w) => w,
            Err(e) => return format!("ERR {e}"),
        };
        match self.control_inner(cmd, &words, doc) {
            Ok(s) => s,
            Err(e) => format!("ERR {e}"),
        }
    }

    fn control_inner(&mut self, cmd: &str, w: &[String], doc: Option<&Document>) -> Result<String, MtError> {
        let word = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
        let usage = |u: &str| Err(MtError(format!("usage: {u}")));
        match (cmd, word(0)) {
            ("VIEW", v @ ("waveform" | "multitrack")) => {
                self.show = v == "multitrack";
                Ok(format!("OK view={v}"))
            }
            ("VIEW", _) => usage("VIEW waveform|multitrack"),
            ("SESSION", "new") if w.len() >= 2 => {
                let (mut rate, mut bits) = (48_000_u32, 24_u32);
                for kv in &w[2..] {
                    match kv.split_once('=') {
                        Some(("rate", v)) => rate = v.parse().map_err(|_| MtError("rate=<hz>".into()))?,
                        Some(("depth" | "bits", v)) => bits = v.parse().map_err(|_| MtError("depth=16|24|32".into()))?,
                        _ => return usage("SESSION new <name> [rate=48000] [depth=16|24|32]"),
                    }
                }
                let depth = bit_depth_from_bits(bits).ok_or_else(|| MtError("depth must be 16, 24 or 32 (float)".into()))?;
                self.create(word(1), rate, depth)?;
                Ok("OK".into())
            }
            ("SESSION", "new") => usage("SESSION new <name> [rate=48000] [depth=16|24|32]"),
            ("SPLAY", _) => {
                let mt = self.mt.as_ref().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let at: f64 = if w.is_empty() { 0.0 } else { word(0).parse().map_err(|_| MtError("usage: SPLAY [seconds]".into()))? };
                if !at.is_finite() || at < 0.0 {
                    return usage("SPLAY [seconds], 0 or more");
                }
                if mt.session.content_end() <= 0 {
                    return Err(MtError("the session has no clips to play".into()));
                }
                self.transport.play(mt, mt.session.sample_rate.samples(at));
                Ok("OK".into())
            }
            ("SSTOP", _) => {
                self.transport.stop();
                Ok("OK".into())
            }
            ("SESSION", "state") => match &self.mt {
                Some(mt) => {
                    let mut v = mt.state_value();
                    v["playing"] = self.transport.is_playing().into();
                    v["position"] = (self.transport.position() as f64 / f64::from(mt.rate())).into();
                    v["underruns"] = self.transport.underruns().into();
                    Ok(format!("OK view={} {v}", if self.show { "multitrack" } else { "waveform" }))
                }
                None => Ok(format!("OK view={} none", if self.show { "multitrack" } else { "waveform" })),
            },
            ("SESSION", _) => usage("SESSION new <name> [rate=] [depth=] | SESSION state"),
            ("TRACK", verb) => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                match verb {
                    "add" => {
                        let channels = match word(2) {
                            "" | "stereo" | "2" => 2,
                            "mono" | "1" => 1,
                            _ => return usage("TRACK add [name] [mono|stereo]"),
                        };
                        let name = Some(word(1)).filter(|n| !n.is_empty());
                        mt.add_track(name, channels)?;
                        Ok(format!("OK tracks={}", mt.track_count()))
                    }
                    "remove" if w.len() == 2 => {
                        let id = mt.find_track(word(1))?;
                        mt.remove_track(id)?;
                        Ok(format!("OK tracks={}", mt.track_count()))
                    }
                    "rename" if w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        mt.rename_track(id, word(2))?;
                        Ok("OK".into())
                    }
                    "mute" | "solo" if w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        let on = matches!(word(2), "1" | "on");
                        if verb == "mute" {
                            mt.set_mute(id, on)?
                        } else {
                            mt.set_solo(id, on)?
                        }
                        Ok("OK".into())
                    }
                    "volume" if w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        mt.set_volume_db(id, word(2).parse().map_err(|_| MtError("volume is a number of dB".into()))?)?;
                        Ok("OK".into())
                    }
                    _ => {
                        usage("TRACK add [name] [mono|stereo] | remove <track> | rename <track> <new> | mute|solo <track> 0|1 | volume <track> <dB>")
                    }
                }
            }
            ("CLIP", "add") if w.len() == 4 => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let id = mt.find_track(word(1))?;
                let at: f64 = word(3).parse().map_err(|_| MtError("<at_seconds> is a number".into()))?;
                mt.add_clip_file(id, &PathBuf::from(word(2)), at)?;
                Ok(format!("OK clips={}", mt.clip_count()))
            }
            ("CLIP", "adddoc") if w.len() == 3 => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let id = mt.find_track(word(1))?;
                let at: f64 = word(2).parse().map_err(|_| MtError("<at_seconds> is a number".into()))?;
                let d = doc.ok_or_else(|| MtError("no document is open".into()))?;
                mt.add_clip_document(id, d, at)?;
                Ok(format!("OK clips={}", mt.clip_count()))
            }
            _ => usage("CLIP add <track> <path> <at_seconds> | CLIP adddoc <track> <at_seconds>"),
        }
    }
}

fn note(failure: &mut Option<String>, r: Result<(), MtError>) {
    if let Err(e) = r {
        *failure = Some(e.to_string());
    }
}

/// A round step in seconds, at least `min` long.
fn ruler_step(min: f32) -> f64 {
    [1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0].into_iter().find(|s| *s >= f64::from(min)).unwrap_or(3600.0)
}

/// Split a control line into words; double quotes keep spaces inside one word.
fn tokens(line: &str) -> Result<Vec<String>, String> {
    let (mut out, mut cur, mut quoted, mut has) = (Vec::new(), String::new(), false, false);
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if quoted {
        return Err("an opening \" has no closing one".into());
    }
    if has {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_keep_spaces_in_one_word() {
        assert_eq!(tokens(r#"add "Lead Vox" /tmp/a b.wav 1.5"#).unwrap(), ["add", "Lead Vox", "/tmp/a", "b.wav", "1.5"]);
        assert_eq!(tokens(r#"add "Lead Vox" "/tmp/a b.wav" 1.5"#).unwrap().len(), 4);
        assert!(tokens(r#"add "open"#).is_err());
        assert_eq!(tokens("").unwrap().len(), 0);
    }

    #[test]
    fn the_control_verbs_make_a_session_with_tracks() {
        let mut v = MtView::default();
        assert!(v.control("TRACK", "add", None).starts_with("ERR no session"));
        assert_eq!(v.control("SESSION", "new Demo rate=44100 depth=16", None), "OK");
        assert!(v.show, "making a session shows it");
        assert_eq!(v.control("TRACK", "add Drums", None), "OK tracks=1");
        assert_eq!(v.control("TRACK", "add", None), "OK tracks=2");
        assert_eq!(v.control("TRACK", r#"rename 2 "Lead Vox""#, None), "OK");
        assert!(v.control("TRACK", "rename 2 drums", None).starts_with("ERR"));
        assert_eq!(v.control("TRACK", "remove drums", None), "OK tracks=1");
        assert!(v.control("SESSION", "state", None).contains(r#""name":"Lead Vox""#));
        assert!(v.control("SESSION", "new X rate=3", None).starts_with("ERR"));
        assert_eq!(v.control("VIEW", "waveform", None), "OK view=waveform");
        assert!(v.control("CLIP", "add 1 /nonexistent/x.wav 0", None).starts_with("ERR cannot read"));
    }
}
