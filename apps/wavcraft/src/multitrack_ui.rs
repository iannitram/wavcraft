//! The Multitrack editor view (f103): track headers (name, mute, solo, volume) and a timeline
//! that draws each track's clips as plain boxes, over `wavcraft_core::multitrack`. Also the
//! control verbs `SESSION`, `TRACK`, `CLIP` and `VIEW`, so a script can drive it.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use soundcraft_playback::Player as DevicePlayer;
use wavcraft_core::Document;
use wavcraft_core::mtplay::{PlaybackSession, SilentPlayer};
use wavcraft_core::mtrecord::{FAKE_CHANNELS, Recorder};
use wavcraft_core::multitrack::soundcraft_model::{BitDepthSetting, TrackId};
use wavcraft_core::multitrack::{MtError, Multitrack, bit_depth_from_bits, bits_of};
use wavcraft_core::session_file;

const HEADER_W: f32 = 230.0;
const ROW_H: f32 = 64.0;
const RULER_H: f32 = 20.0;
const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const MARKER_COLOUR: Color32 = Color32::from_rgb(240, 190, 70);

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
    /// Where take files are written (f120); the app sets it from the session folder.
    pub take_dir: PathBuf,
    recorder: Option<Recorder>,
    arm_input: usize,
    px_per_sec: f32,
    import_at: f64,
    sessions_made: u32,
    /// Marker being renamed (1-based position) and its text.
    marker_rename: Option<(usize, String)>,
    /// The ruler drag in progress: the time it started at.
    ruler_drag: Option<f64>,
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
            take_dir: std::env::temp_dir().join("wavcraft-takes"),
            recorder: None,
            arm_input: 1,
            px_per_sec: 60.0,
            import_at: 0.0,
            sessions_made: 0,
            marker_rename: None,
            ruler_drag: None,
        }
    }
}

impl MtView {
    pub fn open_dialog(&mut self) {
        self.sessions_made += 1;
        self.dialog = Some(NewDialog { name: format!("Session {}", self.sessions_made), rate: 48_000, bits: 24 });
    }

    /// File › Save Session (`as_new` false: to the file it came from, asking only when it has none)
    /// or Save Session As…; the answer, or the reason it failed, goes in `message`.
    pub fn save_session_dialog(&mut self, as_new: bool) {
        let Some(mt) = &mut self.mt else { return };
        let target = match (&mt.file, as_new) {
            (Some(f), false) => f.clone(),
            _ => {
                let mut dlg = rfd::FileDialog::new().add_filter("WavCraft session", &[session_file::EXTENSION]).set_file_name(format!(
                    "{}.{}",
                    mt.name(),
                    session_file::EXTENSION
                ));
                if let Some(dir) = mt.file.as_ref().and_then(|f| f.parent()) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        self.message = match mt.save_to(&target) {
            Ok(_) => None,
            Err(e) => Some(format!("Could not save the session: {e}")),
        };
    }

    /// File › Open Session…
    pub fn open_session_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("WavCraft session", &[session_file::EXTENSION]).pick_file() {
            self.open_session(&p).ok();
        }
    }

    /// Open a session file; media that cannot be found opens offline, and `message` says which.
    pub fn open_session(&mut self, path: &std::path::Path) -> Result<usize, MtError> {
        match Multitrack::open_from(path) {
            Ok((mt, offline)) => {
                self.mt = Some(mt);
                self.selected = None;
                self.rename = None;
                self.show = true;
                self.message = (!offline.is_empty()).then(|| format!("Opened with {} file(s) offline: {}", offline.len(), offline.join("; ")));
                Ok(offline.len())
            }
            Err(e) => {
                self.message = Some(format!("Could not open the session: {e}"));
                Err(e)
            }
        }
    }

    fn create(&mut self, name: &str, rate: u32, depth: BitDepthSetting) -> Result<(), MtError> {
        if self.recorder.is_some() {
            return Err(MtError("recording: stop it first (SREC stop) so the takes are placed, then start a new session".into()));
        }
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
            if self.recorder.is_some() {
                if ui.button("■ Stop Recording").clicked() {
                    note(&mut failure, stop_recording(&mut self.recorder, mt, &mut self.transport).and_then(|(n, f)| stop_result(n, &f)).map(|_| ()));
                }
                ui.ctx().request_repaint();
            } else if ui.button("● Record").on_hover_text("record the armed tracks from the start while the others play").clicked() {
                note(&mut failure, start_recording(&mut self.recorder, mt, &mut self.transport, &self.take_dir, 0.0));
            }
            let at = self.transport.position() as f64 / f64::from(mt.rate());
            ui.monospace(format!("{:02}:{:05.2}", (at / 60.0) as u32, at % 60.0));
            ui.separator();
            let rate = mt.session.sample_rate;
            let (mut pin, mut pout) = mt.punch.range.map_or((0.0, 0.0), |r| (rate.seconds(r.start), rate.seconds(r.end)));
            ui.label("Punch");
            let moved = ui.add(egui::DragValue::new(&mut pin).speed(0.1).range(0.0..=36_000.0).suffix(" s")).changed()
                | ui.add(egui::DragValue::new(&mut pout).speed(0.1).range(0.0..=36_000.0).suffix(" s")).changed();
            if moved {
                note(&mut failure, mt.punch.set(rate.samples(pin), rate.samples(pout.max(pin + 0.01))));
            }
            let mut on = mt.punch.on;
            if ui.toggle_value(&mut on, "Punch").on_hover_text("record only inside the range, with a pre-roll").changed() {
                if on && mt.punch.range.is_none() {
                    failure = Some("set a punch range first".into());
                } else {
                    mt.punch.on = on;
                }
            }
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
            ui.separator();
            if ui.button("Add Marker").on_hover_text("at the playhead (click the ruler to place it)").clicked() {
                note(&mut failure, mt.add_marker_at_playhead().map(|_| ()));
            }
            if ui.button("Add Range").on_hover_text("from the time selection (drag on the ruler to make one)").clicked() {
                note(&mut failure, mt.add_marker_from_selection().map(|_| ()));
            }
            if ui.button("<").on_hover_text("Previous marker").clicked() {
                note(&mut failure, mt.prev_marker().map(|_| ()));
            }
            if ui.button(">").on_hover_text("Next marker").clicked() {
                note(&mut failure, mt.next_marker().map(|_| ()));
            }
        });
        // The marker list: each marker's name (double-click to rename), jump and delete.
        if mt.marker_count() > 0 {
            let mut jump = None;
            let mut delete = None;
            ui.horizontal_wrapped(|ui| {
                ui.weak("Markers:");
                for m in mt.markers_json() {
                    let i = m["index"].as_u64().unwrap_or(0) as usize;
                    if let Some((ri, text)) = self.marker_rename.as_mut().filter(|(ri, _)| *ri == i) {
                        let resp = ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                        resp.request_focus();
                        let (enter, esc) = ui.input(|k| (k.key_pressed(egui::Key::Enter), k.key_pressed(egui::Key::Escape)));
                        if enter {
                            let to = text.clone();
                            note(&mut failure, mt.rename_marker(*ri, &to));
                            self.marker_rename = None;
                        } else if esc {
                            self.marker_rename = None;
                        }
                    } else {
                        let name = m["name"].as_str().unwrap_or("").to_string();
                        let l = ui.add(egui::Label::new(egui::RichText::new(format!("{i}. {name}")).color(MARKER_COLOUR)).sense(Sense::click()));
                        if l.clicked() {
                            jump = m["start"].as_f64();
                        }
                        if l.double_clicked() {
                            self.marker_rename = Some((i, name));
                        }
                        l.context_menu(|ui| {
                            if ui.button("Rename").clicked() {
                                self.marker_rename = Some((i, m["name"].as_str().unwrap_or("").to_string()));
                                ui.close();
                            }
                            if ui.button("Delete").clicked() {
                                delete = Some(i);
                                ui.close();
                            }
                        });
                        if ui.small_button("x").on_hover_text("Delete this marker").clicked() {
                            delete = Some(i);
                        }
                    }
                }
            });
            if let Some(t) = jump {
                note(&mut failure, mt.set_playhead(t));
            }
            if let Some(i) = delete {
                self.marker_rename = None;
                note(&mut failure, mt.delete_marker(i));
            }
        }
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
            let (rr, ruler_resp) = ui.allocate_exact_size(vec2(HEADER_W + width, RULER_H), Sense::click_and_drag());
            let painter = ui.painter_at(rr);
            // Click sets the playhead; a drag makes the time selection (the "Add Range" source).
            let time_at = |x: f32| f64::from(((x - rr.left() - HEADER_W) / self.px_per_sec).max(0.0));
            if let Some(p) = ruler_resp.interact_pointer_pos().filter(|p| p.x >= rr.left() + HEADER_W) {
                if ruler_resp.drag_started() {
                    self.ruler_drag = Some(time_at(p.x));
                }
                if let (true, Some(a)) = (ruler_resp.dragged(), self.ruler_drag) {
                    let _ = mt.set_selection(a, time_at(p.x));
                }
                if ruler_resp.clicked() {
                    let t = time_at(p.x);
                    let _ = (mt.set_playhead(t), mt.set_selection(t, t));
                }
            }
            if ruler_resp.drag_stopped() {
                self.ruler_drag = None;
            }
            let step = ruler_step(60.0 / self.px_per_sec);
            let mut s = 0.0_f64;
            while (s as f32) * self.px_per_sec < width {
                let x = rr.left() + HEADER_W + s as f32 * self.px_per_sec;
                painter.line_segment([pos2(x, rr.bottom() - 6.0), pos2(x, rr.bottom())], Stroke::new(1.0, Color32::GRAY));
                painter.text(pos2(x + 3.0, rr.top()), egui::Align2::LEFT_TOP, format!("{s}s"), egui::FontId::monospace(10.0), Color32::GRAY);
                s += step;
            }
            // Session markers and ranges on the ruler, then the selection and the playhead.
            let rate_hz = f64::from(mt.rate());
            let xs = |samples: i64| rr.left() + HEADER_W + (samples as f64 / rate_hz) as f32 * self.px_per_sec;
            for m in &mt.session.markers {
                let (x0, x1) = (xs(m.start), xs(m.end));
                if m.end > m.start {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, rr.bottom() - 10.0), pos2(x1.max(x0 + 2.0), rr.bottom())),
                        0.0,
                        MARKER_COLOUR.gamma_multiply(0.45),
                    );
                }
                painter.line_segment([pos2(x0, rr.top()), pos2(x0, rr.bottom())], Stroke::new(1.5, MARKER_COLOUR));
                painter.text(pos2(x0 + 3.0, rr.top() + 10.0), egui::Align2::LEFT_TOP, &m.name, egui::FontId::proportional(11.0), MARKER_COLOUR);
            }
            // Punch range band (f126): red while punch is on, grey while it is set but off.
            if let Some(r) = mt.punch.range {
                let rate = mt.session.sample_rate;
                let band = Rect::from_min_max(
                    pos2(rr.left() + HEADER_W + rate.seconds(r.start) as f32 * self.px_per_sec, rr.top() + 12.0),
                    pos2(rr.left() + HEADER_W + rate.seconds(r.end) as f32 * self.px_per_sec, rr.bottom()),
                );
                let c = if mt.punch.on { Color32::from_rgb(220, 70, 60) } else { Color32::from_gray(110) };
                painter.rect_filled(band, 0.0, c.gamma_multiply(0.45));
            }
            if let Some((a, b)) = mt.selection_seconds() {
                let r = Rect::from_min_max(
                    pos2(rr.left() + HEADER_W + a as f32 * self.px_per_sec, rr.top()),
                    pos2(rr.left() + HEADER_W + b as f32 * self.px_per_sec, rr.bottom()),
                );
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 160, 255, 50));
            }
            let px = xs(mt.session.edit.playhead);
            painter.line_segment([pos2(px, rr.top()), pos2(px, rr.bottom())], Stroke::new(1.0, Color32::from_rgb(255, 255, 255)));
            for id in ids {
                let Some(t) = mt.session.track(id) else { continue };
                let (name, chans, clips, [r, g, b]) = (t.name.clone(), t.channels(), t.clips().to_vec(), t.color);
                let (mut mute, mut solo, mut vol) = (t.mixer.mute, t.mixer.solo, t.mixer.volume_db);
                let mut arm = t.mixer.record_arm;
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
                    ui.toggle_value(&mut arm, "R").on_hover_text("Arm for recording");
                    if arm {
                        ui.add(egui::DragValue::new(&mut self.arm_input).range(1..=FAKE_CHANNELS).prefix("In ")).on_hover_text("input channel");
                    }
                    if ui.small_button("Rename").clicked() {
                        self.rename = Some((id, name.clone()));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(id);
                    }
                });
                h.add(egui::Slider::new(&mut vol, -60.0..=12.0).suffix(" dB").fixed_decimals(1));
                let _ = (mt.set_mute(id, mute), mt.set_solo(id, solo), mt.set_volume_db(id, vol));
                if arm != t_armed(mt, id) {
                    note(&mut failure, if arm { mt.arm_track(id, Some(self.arm_input)) } else { mt.disarm_track(id) });
                }
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

    /// `SMARKER add <t> [end]` | `rename <i> <name>` | `delete <i>` | `next` | `prev` | `playhead <t>`
    /// | `select <a> <b>` | `addhere` | `addsel` | `state` (session markers, f110; `<i>` counts from 1).
    fn smarker(&mut self, w: &[String]) -> Result<String, MtError> {
        let word = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
        let usage = || {
            Err(MtError("usage: SMARKER add <t> [end] | rename <i> <name> | delete <i> | next | prev | playhead <t> | select <a> <b> | addhere | addsel | state".into()))
        };
        let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
        let num = |i: usize| word(i).parse::<f64>().map_err(|_| MtError(format!("{:?} is not a number of seconds", word(i))));
        let pos = |i: usize| word(i).parse::<usize>().map_err(|_| MtError(format!("{:?} is not a marker number", word(i))));
        match (word(0), w.len()) {
            ("add", 2 | 3) => {
                let end = if w.len() == 3 { Some(num(2)?) } else { None };
                let i = mt.add_marker(None, num(1)?, end)?;
                Ok(format!("OK index={i} markers={}", mt.marker_count()))
            }
            ("addhere", 1) => Ok(format!("OK index={} markers={}", mt.add_marker_at_playhead()?, mt.marker_count())),
            ("addsel", 1) => Ok(format!("OK index={} markers={}", mt.add_marker_from_selection()?, mt.marker_count())),
            ("rename", 3) => mt.rename_marker(pos(1)?, word(2)).map(|()| "OK".into()),
            ("delete", 2) => mt.delete_marker(pos(1)?).map(|()| format!("OK markers={}", mt.marker_count())),
            ("next", 1) => Ok(format!("OK index={} playhead={}", mt.next_marker()?, mt.playhead_seconds())),
            ("prev", 1) => Ok(format!("OK index={} playhead={}", mt.prev_marker()?, mt.playhead_seconds())),
            ("playhead", 2) => mt.set_playhead(num(1)?).map(|()| "OK".into()),
            ("select", 3) => mt.set_selection(num(1)?, num(2)?).map(|()| "OK".into()),
            ("state", 1) => Ok(format!("OK {}", mt.markers_state_json())),
            _ => usage(),
        }
    }

    fn control_inner(&mut self, cmd: &str, w: &[String], doc: Option<&Document>) -> Result<String, MtError> {
        if cmd == "SMARKER" {
            return self.smarker(w);
        }
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
            ("PUNCH", verb) => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let rate = mt.session.sample_rate;
                let secs = |i: usize| -> Result<f64, MtError> {
                    let v: f64 = word(i).parse().map_err(|_| MtError("seconds must be numbers".into()))?;
                    if v.is_finite() && v >= 0.0 { Ok(v) } else { Err(MtError("seconds must be 0 or more".into())) }
                };
                match verb {
                    "set" if w.len() == 3 => mt.punch.set(rate.samples(secs(1)?), rate.samples(secs(2)?))?,
                    "preroll" if w.len() == 2 => mt.punch.preroll = rate.samples(secs(1)?),
                    "on" | "off" if w.len() == 1 => {
                        if verb == "on" && mt.punch.range.is_none() {
                            return Err(MtError("no punch range: PUNCH set <in_s> <out_s> first".into()));
                        }
                        mt.punch.on = verb == "on";
                    }
                    _ => return usage("PUNCH set <in_s> <out_s> | PUNCH preroll <s> | PUNCH on|off"),
                }
                Ok("OK".into())
            }
            ("SREC", "start") => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let at: f64 = if w.len() >= 2 {
                    word(1).parse().map_err(|_| MtError("usage: SREC start [seconds]".into()))?
                } else {
                    mt.punch.record_start().map_or(0.0, |s| mt.session.sample_rate.seconds(s))
                };
                if !at.is_finite() || at < 0.0 {
                    return usage("SREC start [seconds], 0 or more");
                }
                start_recording(&mut self.recorder, mt, &mut self.transport, &self.take_dir, at)?;
                Ok("OK".into())
            }
            ("SREC", "stop") => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let (n, f) = stop_recording(&mut self.recorder, mt, &mut self.transport)?;
                let n = stop_result(n, &f)?;
                Ok(format!("OK takes={n}"))
            }
            ("SREC", _) => usage("SREC start [seconds] | SREC stop"),
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
                    v["recording"] = self.recorder.is_some().into();
                    Ok(format!("OK view={} {v}", if self.show { "multitrack" } else { "waveform" }))
                }
                None => Ok(format!("OK view={} none", if self.show { "multitrack" } else { "waveform" })),
            },
            ("SESSION", "save") if w.len() == 2 => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let r = mt.save_to(&PathBuf::from(word(1)))?;
                self.message = None;
                Ok(format!("OK exported={} offline={} path={}", r.exported, r.offline, r.path.display()))
            }
            ("SESSION", "open") if w.len() == 2 => {
                let offline = self.open_session(&PathBuf::from(word(1)))?;
                Ok(format!("OK offline={offline}"))
            }
            ("SESSION", _) => usage("SESSION new <name> [rate=] [depth=] | SESSION state | SESSION save <path> | SESSION open <path>"),
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
                    "arm" if w.len() == 2 || w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        let input = match w.get(2) {
                            None => None,
                            Some(kv) => Some(
                                kv.strip_prefix("input=")
                                    .and_then(|v| v.parse::<usize>().ok())
                                    .ok_or_else(|| MtError("usage: TRACK arm <track> [input=<channel>]".into()))?,
                            ),
                        };
                        mt.arm_track(id, input)?;
                        Ok("OK".into())
                    }
                    "disarm" if w.len() == 2 => {
                        let id = mt.find_track(word(1))?;
                        mt.disarm_track(id)?;
                        Ok("OK".into())
                    }
                    "volume" if w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        mt.set_volume_db(id, word(2).parse().map_err(|_| MtError("volume is a number of dB".into()))?)?;
                        Ok("OK".into())
                    }
                    _ => usage(
                        "TRACK add [name] [mono|stereo] | remove <track> | rename <track> <new> | mute|solo <track> 0|1 | volume <track> <dB> | arm <track> [input=<ch>] | disarm <track>",
                    ),
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

fn t_armed(mt: &Multitrack, id: TrackId) -> bool {
    mt.session.track(id).is_some_and(|t| t.mixer.record_arm)
}

/// Start recording the armed tracks at `at` seconds, with the session playing from there when
/// it has anything to play (overdub, C038).
fn start_recording(rec: &mut Option<Recorder>, mt: &Multitrack, transport: &mut Transport, dir: &std::path::Path, at: f64) -> Result<(), MtError> {
    if rec.is_some() {
        return Err(MtError("already recording".into()));
    }
    let start = mt.session.sample_rate.samples(at);
    let plans = mt.plan_takes(dir, FAKE_CHANNELS);
    let mut r = Recorder::start(plans, mt.rate(), start)?;
    r.punch = mt.punch.active();
    if mt.session.content_end() > start {
        transport.play(mt, start);
    }
    *rec = Some(r);
    Ok(())
}

/// Stop recording and place every take whose file holds audio. A track that failed does not
/// stop the others: returns how many were placed and one message per problem.
fn stop_recording(rec: &mut Option<Recorder>, mt: &mut Multitrack, transport: &mut Transport) -> Result<(usize, Vec<String>), MtError> {
    let r = rec.take().ok_or_else(|| MtError("not recording".into()))?;
    let (start, punch) = (r.start, r.punch);
    transport.stop();
    let report = r.stop()?;
    let mut failures = report.failures;
    let mut placed = 0;
    for w in report.written.into_iter().filter(|w| w.frames > 0) {
        match mt.place_take_in(w.track, start, &w.path, punch) {
            Ok(_) => placed += 1,
            Err(e) => failures.push(format!("{e} (the file is kept)")),
        }
    }
    Ok((placed, failures))
}

/// The message for a stop: `Ok` when nothing went wrong, else every problem and the count placed.
fn stop_result(placed: usize, failures: &[String]) -> Result<usize, MtError> {
    if failures.is_empty() { Ok(placed) } else { Err(MtError(format!("placed {placed} take(s); problems: {}", failures.join("; ")))) }
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

    #[test]
    fn smarker_verbs_add_rename_navigate_and_delete() {
        let mut v = MtView::default();
        assert!(v.control("SMARKER", "add 1", None).starts_with("ERR no session"));
        v.control("SESSION", "new Demo", None);
        assert_eq!(v.control("SMARKER", "add 4", None), "OK index=1 markers=1");
        assert_eq!(v.control("SMARKER", "add 1 2.5", None), "OK index=1 markers=2");
        assert_eq!(v.control("SMARKER", "rename 2 \"Last one\"", None), "OK");
        assert!(v.control("SMARKER", "state", None).contains(r#""name":"Last one""#));
        assert_eq!(v.control("SMARKER", "next", None), "OK index=1 playhead=1");
        assert_eq!(v.control("SMARKER", "next", None), "OK index=2 playhead=4");
        assert!(v.control("SMARKER", "next", None).starts_with("ERR"));
        assert_eq!(v.control("SMARKER", "prev", None), "OK index=1 playhead=1");
        assert_eq!(v.control("SMARKER", "delete 1", None), "OK markers=1");
        assert!(v.control("SMARKER", "delete 5", None).starts_with("ERR"));
        assert!(v.control("SMARKER", "add x", None).starts_with("ERR"));
        assert!(v.control("SMARKER", "bogus", None).starts_with("ERR usage"));
    }

    #[test]
    fn save_and_open_by_control_keep_the_state() {
        let dir = std::env::temp_dir().join(format!("wavcraft-ui-sf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.wcsession");
        let mut v = MtView::default();
        assert!(v.control("SESSION", &format!("save {}", file.display()), None).starts_with("ERR no session"));
        v.control("SESSION", "new Demo", None);
        v.control("TRACK", "add Drums", None);
        let before = v.control("SESSION", "state", None);
        assert!(v.control("SESSION", &format!("save {}", file.display()), None).starts_with("OK exported=0"));
        let mut w = MtView::default();
        assert_eq!(w.control("SESSION", &format!("open {}", file.display()), None), "OK offline=0");
        assert_eq!(w.control("SESSION", "state", None), before);
        assert!(w.control("SESSION", &format!("open {}", dir.join("nope").display()), None).starts_with("ERR"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_session_is_refused_while_recording() {
        let tmp = std::env::temp_dir().join(format!("wavcraft-mtui-{}", std::process::id()));
        let mut v = MtView::default();
        assert_eq!(v.control("SESSION", "new A", None), "OK");
        assert_eq!(v.control("TRACK", "add Vox", None), "OK tracks=1");
        let plans = {
            let mt = v.mt.as_mut().unwrap();
            let id = mt.session.tracks[0].id;
            mt.arm_track(id, Some(1)).unwrap();
            mt.plan_takes(&tmp, 8)
        };
        v.recorder = Some(Recorder::start_fake(plans, 48_000, 0).unwrap());
        let answer = v.control("SESSION", "new B", None);
        assert!(answer.starts_with("ERR") && answer.contains("stop it first"), "{answer}");
        assert!(v.control("SESSION", "state", None).contains(r#""name":"A""#), "the session is still the first one");
        assert!(v.recorder.is_some(), "and the recording goes on");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
