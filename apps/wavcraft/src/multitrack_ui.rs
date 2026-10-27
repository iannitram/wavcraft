//! The Multitrack editor view (f103): track headers (name, mute, solo, volume) and a timeline
//! that draws each track's clips as plain boxes, over `wavcraft_core::multitrack`. Also the
//! control verbs `SESSION`, `TRACK`, `CLIP`, `CLIPPROP` and `VIEW`, so a script can drive it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use soundcraft_playback::Player as DevicePlayer;
use wavcraft_core::Document;
use wavcraft_core::mtplay::{PlaybackSession, SilentPlayer};
use wavcraft_core::mtrecord::{FAKE_CHANNELS, Recorder};
use wavcraft_core::multitrack::automation::parse_param;
use wavcraft_core::multitrack::soundcraft_model::{AutoParam, BitDepthSetting, ChannelFormat, ClipId, SourceId, TrackId, TrackKind};
use wavcraft_core::multitrack::{AudioType, ClipProps, MtError, Multitrack, VideoMeta, bit_depth_from_bits, bits_of, parse_track_format};
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
    five_one: bool,
}

/// The Clip Properties panel's fields while it is open (f119); tempo fields are text so a
/// half-typed number is not lost, and empty means "not set".
struct PropsForm {
    track: TrackId,
    clip: ClipId,
    name: String,
    gain: f32,
    bpm: String,
    beats: String,
    key: String,
}

/// The Multitrack view's state: the session (if one was made), which view is up, and the small
/// bits of UI state around it.
pub struct MtView {
    pub mt: Option<Multitrack>,
    /// The Multitrack editor is the one shown (otherwise the Waveform editor).
    pub show: bool,
    dialog: Option<NewDialog>,
    selected: Option<TrackId>,
    /// The clip the Clip Properties panel is about (f119): its track and id.
    sel_clip: Option<(TrackId, ClipId)>,
    /// The clips picked for an audio type (f123): a click picks one; Shift or Ctrl adds or removes it.
    pub(crate) sel_clips: Vec<ClipId>,
    props: Option<PropsForm>,
    rename: Option<(TrackId, String)>,
    pub message: Option<String>,
    pub transport: Transport,
    /// Where take files are written (f120); the app sets it from the session folder.
    pub take_dir: PathBuf,
    recorder: Option<Recorder>,
    arm_input: usize,
    /// A clip the user asked to edit in the Waveform editor (track, 1-based position); the app
    /// takes it each frame (f116).
    pub edit_request: Option<(TrackId, usize)>,
    px_per_sec: f32,
    import_at: f64,
    sessions_made: u32,
    /// Marker being renamed (1-based position) and its text.
    marker_rename: Option<(usize, String)>,
    /// The ruler drag in progress: the time it started at.
    ruler_drag: Option<f64>,
    /// Normalize a mixdown to this peak (menu check box).
    pub mix_normalize: bool,
    /// Tracks whose automation lane is showing, and which parameter (f109).
    lanes: HashMap<TrackId, AutoParam>,
    /// The point being dragged: track, parameter, index.
    dragging: Option<(TrackId, AutoParam, usize)>,
}

impl Default for MtView {
    fn default() -> Self {
        MtView {
            mt: None,
            show: false,
            dialog: None,
            selected: None,
            sel_clip: None,
            sel_clips: Vec::new(),
            props: None,
            rename: None,
            message: None,
            transport: Transport::default(),
            take_dir: std::env::temp_dir().join("wavcraft-takes"),
            recorder: None,
            arm_input: 1,
            edit_request: None,
            px_per_sec: 60.0,
            import_at: 0.0,
            sessions_made: 0,
            marker_rename: None,
            ruler_drag: None,
            mix_normalize: false,
            lanes: HashMap::new(),
            dragging: None,
        }
    }
}

impl MtView {
    /// What Mixdown needs: the session and the options (`selected_only`: just the picked clips).
    pub fn mixdown_input(
        &self,
        selected_only: bool,
        normalize_db: Option<f32>,
    ) -> Result<(wavcraft_core::multitrack::soundcraft_model::Session, wavcraft_core::mixdown::MixdownOptions), MtError> {
        let mt = self.mt.as_ref().ok_or_else(|| MtError("no session: File > New > Multitrack Session first".into()))?;
        let only_clips = selected_only.then(|| self.sel_clips.clone());
        if selected_only && self.sel_clips.is_empty() {
            return Err(MtError("no clips are selected: click clips in the Multitrack editor first".into()));
        }
        Ok((mt.session.clone(), wavcraft_core::mixdown::MixdownOptions { only_clips, normalize_db }))
    }

    pub fn open_dialog(&mut self) {
        self.sessions_made += 1;
        self.dialog = Some(NewDialog { name: format!("Session {}", self.sessions_made), rate: 48_000, bits: 24, five_one: false });
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
                self.sel_clips.clear();
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

    fn create_with_master(&mut self, name: &str, rate: u32, depth: BitDepthSetting, master: ChannelFormat) -> Result<(), MtError> {
        if self.recorder.is_some() {
            return Err(MtError("recording: stop it first (SREC stop) so the takes are placed, then start a new session".into()));
        }
        self.mt = Some(Multitrack::new_with_master(name, rate, depth, master)?);
        self.transport.stop();
        self.selected = None;
        self.sel_clips.clear();
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
                    ui.label("Master");
                    egui::ComboBox::from_id_salt("new-master").selected_text(if d.five_one { "5.1" } else { "Stereo" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.five_one, false, "Stereo");
                        ui.selectable_value(&mut d.five_one, true, "5.1");
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
            let master = if d.five_one { ChannelFormat::Surround51 } else { ChannelFormat::Stereo };
            let depth = bit_depth_from_bits(bits).unwrap_or_default();
            self.message = match self.create_with_master(&name, rate, depth, master) {
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
            if ui.button("Add 5.1 Track").clicked() {
                note(&mut failure, mt.add_track_format(None, ChannelFormat::Surround51).map(|_| ()));
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
            let sel = self.sel_clip.filter(|(t, c)| mt.session.track(*t).is_some_and(|t| t.clips().iter().any(|x| x.id == *c)));
            if ui.add_enabled(sel.is_some(), egui::Button::new("Clip Properties…")).on_hover_text("click a clip first").clicked()
                && let Some((t, c)) = sel
            {
                let (cl, m) = (mt.session.track(t).and_then(|t| t.clips().iter().find(|x| x.id == c)).cloned(), mt.clip_tempo(c));
                if let Some(cl) = cl {
                    self.props = Some(PropsForm {
                        track: t,
                        clip: c,
                        name: cl.name,
                        gain: cl.gain_db,
                        bpm: m.bpm.map(|b| b.to_string()).unwrap_or_default(),
                        beats: m.beats.map(|b| b.to_string()).unwrap_or_default(),
                        key: m.key.unwrap_or_default(),
                    });
                }
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
        // Audio types (f123): assign to the selected clips, filter the view, and the properties line.
        ui.horizontal(|ui| {
            ui.label("Audio type");
            let sel = self.sel_clips.clone();
            let kinds = sel.iter().map(|c| mt.clip_type(*c)).collect::<Vec<_>>();
            let current = match kinds.first() {
                Some(k) if kinds.iter().all(|x| x == k) => k.map_or("None", AudioType::label),
                Some(_) => "Mixed",
                None => "-",
            };
            ui.add_enabled_ui(!sel.is_empty(), |ui| {
                egui::ComboBox::from_id_salt("clip-type").selected_text(current).show_ui(ui, |ui| {
                    for t in AudioType::ALL {
                        if ui.selectable_label(false, t.label()).clicked() {
                            note(&mut failure, mt.set_clip_type(&sel, Some(t)));
                        }
                    }
                    if ui.selectable_label(false, "None").clicked() {
                        note(&mut failure, mt.set_clip_type(&sel, None));
                    }
                });
            });
            ui.weak(format!("{} clip(s) selected (click a clip; Shift/Ctrl adds)", sel.len()));
            ui.separator();
            ui.label("Show");
            egui::ComboBox::from_id_salt("type-filter").selected_text(mt.type_filter.map_or("All clips", AudioType::label)).show_ui(ui, |ui| {
                ui.selectable_value(&mut mt.type_filter, None, "All clips");
                for t in AudioType::ALL {
                    ui.selectable_value(&mut mt.type_filter, Some(t), t.label());
                }
            });
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
                let fmt_label = t.format.label();
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
                        ui.weak(fmt_label).on_hover_text(format!("{chans} channel(s); a clip of another channel count is refused, mono into stereo is duplicated, stereo into mono averaged"));
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
                    let mut shown = self.lanes.contains_key(&id);
                    if ui.toggle_value(&mut shown, "Auto").on_hover_text("Show the automation lane (volume or pan)").changed() {
                        if shown {
                            self.lanes.insert(id, AutoParam::Volume);
                        } else {
                            self.lanes.remove(&id);
                        }
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
                    let rate = f64::from(mt.rate());
                    self.sel_clip = lane_resp.interact_pointer_pos().and_then(|pp| {
                        let sec = f64::from(pp.x - lane.left()) / f64::from(self.px_per_sec);
                        clips.iter().find(|c| (c.start as f64 / rate..(c.start + c.length) as f64 / rate).contains(&sec)).map(|c| (id, c.id))
                    });
                }
                let rate = f64::from(mt.rate());
                let p = ui.painter_at(lane);
                if self.transport.is_playing() {
                    let x = lane.left() + (self.transport.position() as f64 / rate) as f32 * self.px_per_sec;
                    p.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(1.5, Color32::from_rgb(255, 200, 60)));
                }
                for (ci, c) in clips.iter().enumerate().filter(|(_, c)| mt.clip_visible(c.id)) {
                    let x0 = lane.left() + (c.start as f64 / rate) as f32 * self.px_per_sec;
                    let w = ((c.length as f64 / rate) as f32 * self.px_per_sec).max(2.0);
                    let cr = Rect::from_min_size(pos2(x0, lane.top() + 4.0), vec2(w, ROW_H - 8.0));
                    let fill = if mute { colour.gamma_multiply(0.25) } else { colour.gamma_multiply(0.6) };
                    p.rect_filled(cr, 3.0, fill);
                    let picked = self.sel_clip == Some((id, c.id));
                    p.rect_stroke(
                        cr,
                        3.0,
                        Stroke::new(if picked { 2.5 } else { 1.0 }, if picked { Color32::WHITE } else { colour }),
                        StrokeKind::Inside,
                    );
                    if self.sel_clips.contains(&c.id) {
                        p.rect_stroke(cr, 3.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
                    }
                    let mut label_at = cr.left_top() + vec2(4.0, 3.0);
                    if let Some(ty) = mt.clip_type(c.id) {
                        let [tr, tg, tb] = ty.colour();
                        let tag = Rect::from_min_size(cr.left_top(), vec2(cr.width(), 14.0));
                        p.rect_filled(tag, 3.0, Color32::from_rgb(tr, tg, tb));
                        p.text(tag.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, ty.label(), egui::FontId::proportional(10.0), Color32::BLACK);
                        label_at.y += 14.0;
                    }
                    p.text(label_at, egui::Align2::LEFT_TOP, &c.name, egui::FontId::proportional(12.0), Color32::WHITE);
                    let cresp = ui.interact(cr, ui.id().with(("clip", id.0, c.id.0)), Sense::click()).on_hover_text(format!(
                        "{} · type: {}",
                        c.name,
                        mt.clip_type(c.id).map_or("none", AudioType::label)
                    ));
                    if cresp.clicked() {
                        // A click on a clip selects its track, as a click on the lane does.
                        self.selected = Some(id);
                        // f123: a click picks this clip for its audio type; Shift or Ctrl adds it, or takes it back out.
                        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                        match self.sel_clips.iter().position(|x| *x == c.id) {
                            Some(pos) if add => {
                                self.sel_clips.remove(pos);
                            }
                            None if add => self.sel_clips.push(c.id),
                            _ => self.sel_clips = vec![c.id],
                        }
                    }
                    if cresp.double_clicked() {
                        self.edit_request = Some((id, ci + 1));
                    }
                    cresp.context_menu(|ui| {
                        if ui.button("Edit in Waveform editor").clicked() {
                            self.edit_request = Some((id, ci + 1));
                            ui.close();
                        }
                    });
                }
                if self.lanes.contains_key(&id) {
                    automation_row(ui, mt, id, width, self.px_per_sec, &mut self.lanes, &mut self.dragging, &mut failure);
                }
            }
            // The master track, always at the bottom: its own fader and mute.
            if let Some(m) = mt.session.master() {
                let (label, mut mute, mut vol) = (m.format.label(), m.mixer.mute, m.mixer.volume_db);
                let (row, _) = ui.allocate_exact_size(vec2(HEADER_W + width, ROW_H), Sense::hover());
                let header = Rect::from_min_size(row.min, vec2(HEADER_W - 4.0, ROW_H));
                let p = ui.painter();
                p.rect_filled(header, 3.0, Color32::from_rgb(40, 40, 40));
                p.rect_filled(Rect::from_min_size(pos2(row.left() + HEADER_W, row.top()), vec2(width, ROW_H)), 0.0, BACKGROUND);
                p.rect_filled(Rect::from_min_size(header.min, vec2(4.0, ROW_H)), 0.0, Color32::from_rgb(140, 140, 140));
                let mut h =
                    ui.new_child(egui::UiBuilder::new().max_rect(header.shrink2(vec2(8.0, 3.0))).layout(egui::Layout::top_down(egui::Align::Min)));
                h.horizontal(|ui| {
                    ui.label(egui::RichText::new("Master").strong());
                    ui.weak(label);
                    ui.toggle_value(&mut mute, "M").on_hover_text("Mute the master");
                });
                h.add(egui::Slider::new(&mut vol, -60.0..=12.0).suffix(" dB").fixed_decimals(1));
                let _ = (mt.set_master_mute(mute), mt.set_master_volume_db(vol));
            }
            if mt.track_count() == 0 {
                ui.label("No tracks yet: Add Stereo, Mono or 5.1 Track.");
            }
        });
        if let Some(id) = remove {
            self.sel_clips.clear();
            if self.selected == Some(id) {
                self.selected = None;
            }
            note(&mut failure, mt.remove_track(id));
        }
        if failure.is_some() {
            self.message = failure;
        }
    }

    /// The source of clip `i` (1-based) on the track `key` names, its name and its audio as a
    /// float WAV's bytes: what the app needs to open it in the Waveform editor (f116).
    pub fn clip_source_wav(&self, track: TrackId, i: usize) -> Result<(SourceId, String, Vec<u8>), MtError> {
        let mt = self.mt.as_ref().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
        let (_, src) = mt.clip_source(track, i)?;
        Ok((src, mt.source_name(src), mt.source_wav(src)?))
    }

    /// Put edited audio back as `source` (see `Multitrack::replace_source_audio`).
    pub fn replace_source(
        &mut self,
        source: SourceId,
        audio: wavcraft_core::multitrack::ModelBuffer,
    ) -> Result<wavcraft_core::multitrack::SourceChange, MtError> {
        self.mt.as_mut().ok_or_else(|| MtError("no session".into()))?.replace_source_audio(source, audio)
    }

    /// The track a control word names (name or position).
    pub fn find_track(&self, key: &str) -> Result<TrackId, MtError> {
        self.mt.as_ref().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?.find_track(key)
    }

    /// The Clip Properties panel (f119): name, gain, tempo in BPM, beats and key. "Apply" writes
    /// them to the clip in the model. "Detect" (read the tempo from the audio) is not built yet.
    pub fn props_ui(&mut self, ctx: &egui::Context) {
        let (Some(mt), Some(f)) = (&mut self.mt, &mut self.props) else { return };
        let (mut open, mut apply, mut close) = (true, false, false);
        egui::Window::new("Clip Properties").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("clip-props").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut f.name);
                ui.end_row();
                ui.label("Gain");
                ui.add(egui::Slider::new(&mut f.gain, -60.0..=24.0).suffix(" dB").fixed_decimals(1));
                ui.end_row();
                ui.label("Original tempo");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.bpm).desired_width(70.0).hint_text("BPM"));
                    ui.add_enabled(false, egui::Button::new("Detect")).on_disabled_hover_text("Detecting the tempo from the audio comes later");
                });
                ui.end_row();
                ui.label("Beats");
                ui.add(egui::TextEdit::singleline(&mut f.beats).desired_width(70.0));
                ui.end_row();
                ui.label("Key");
                ui.add(egui::TextEdit::singleline(&mut f.key).desired_width(90.0).hint_text("optional, e.g. A minor"));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                apply = ui.button("Apply").clicked();
                close = ui.button("Close").clicked();
            });
        });
        let mut failure = None;
        if apply {
            let num = |s: &str| -> Result<Option<f64>, MtError> {
                let s = s.trim();
                if s.is_empty() { Ok(None) } else { s.parse().map(Some).map_err(|_| MtError(format!("{s:?} is not a number"))) }
            };
            let r = num(&f.bpm).and_then(|bpm| {
                let beats = num(&f.beats)?;
                mt.set_clip_props(
                    f.track,
                    f.clip,
                    &ClipProps {
                        name: Some(f.name.clone()),
                        gain_db: Some(f.gain),
                        bpm: Some(bpm),
                        beats: Some(beats),
                        key: Some(Some(f.key.clone())),
                    },
                )
            });
            note(&mut failure, r);
        }
        self.message = failure;
        if !open || close {
            self.props = None;
        }
    }

    /// One control line for `AUTO`: add / move / delete / thin / show / state (f109).
    fn auto_control(&mut self, w: &[String]) -> Result<String, MtError> {
        let word = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
        let usage = || {
            Err(MtError(
                "usage: AUTO add <track> <param> <t> <v> | move <track> <param> <i> <t> <v> | delete <track> <param> <i> | thin <track> <param> <tol> | show <track> <param>|off | state"
                    .into(),
            ))
        };
        let num = |i: usize, what: &str| word(i).parse::<f64>().map_err(|_| MtError(format!("{what} is a number")));
        let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
        let verb = word(0);
        if verb == "state" && w.len() == 1 {
            let shown: Vec<(TrackId, AutoParam)> = self.lanes.iter().map(|(i, p)| (*i, p.clone())).collect();
            return Ok(format!("OK {}", mt.automation_state(&shown)));
        }
        if w.len() < 3 {
            return usage();
        }
        let id = mt.find_track(word(1))?;
        if verb == "show" && w.len() == 3 {
            if word(2) == "off" {
                self.lanes.remove(&id);
            } else {
                self.lanes.insert(id, parse_param(word(2))?);
            }
            self.show = true;
            return Ok("OK".into());
        }
        let param = parse_param(word(2))?;
        let index = |i: usize| word(i).parse::<usize>().map_err(|_| MtError("<i> is a point number from 0".into()));
        let count = |mt: &Multitrack| mt.lane(id, &param).map_or(0, |l| l.points.len());
        match (verb, w.len()) {
            ("add", 5) => {
                let i = mt.automation_add(id, &param, num(3, "<t>")?, num(4, "<v>")? as f32)?;
                Ok(format!("OK index={i} points={}", count(mt)))
            }
            ("move", 6) => {
                let i = mt.automation_move(id, &param, index(3)?, num(4, "<t>")?, num(5, "<v>")? as f32)?;
                Ok(format!("OK index={i} points={}", count(mt)))
            }
            ("delete", 4) => {
                mt.automation_delete(id, &param, index(3)?)?;
                Ok(format!("OK points={}", count(mt)))
            }
            ("thin", 4) => {
                let n = mt.automation_thin(id, &param, num(3, "<tol>")? as f32)?;
                Ok(format!("OK removed={n} points={}", count(mt)))
            }
            _ => usage(),
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
            ("AUTO", _) => self.auto_control(w),
            ("VIEW", v @ ("waveform" | "multitrack")) => {
                self.show = v == "multitrack";
                Ok(format!("OK view={v}"))
            }
            ("VIEW", _) => usage("VIEW waveform|multitrack"),
            ("CLIPTYPE", _) => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                const U: &str = "CLIPTYPE <track> <i[,j...]|all> dialogue|music|sfx|ambience|none | CLIPTYPE filter dialogue|music|sfx|ambience|all";
                if word(0) == "filter" && w.len() == 2 {
                    mt.type_filter = if word(1).eq_ignore_ascii_case("all") { None } else { AudioType::parse(word(1))? };
                    return Ok(format!("OK filter={}", mt.type_filter.map_or("all", AudioType::key)));
                }
                if w.len() != 3 {
                    return usage(U);
                }
                let track = mt.find_track(word(0))?;
                let t = AudioType::parse(word(2))?;
                let n = mt.session.track(track).map_or(0, |tr| tr.clips().len());
                let positions: Vec<usize> = if word(1) == "all" {
                    (1..=n).collect()
                } else {
                    word(1)
                        .split(',')
                        .map(|x| x.parse().map_err(|_| MtError(format!("{x:?} is not a clip number (1-based)"))))
                        .collect::<Result<_, _>>()?
                };
                let ids = positions.iter().map(|p| mt.clip_at(track, *p)).collect::<Result<Vec<_>, _>>()?;
                mt.set_clip_type(&ids, t)?;
                Ok(format!("OK clips={}", ids.len()))
            }
            ("SESSION", "new") if w.len() >= 2 => {
                let (mut rate, mut bits, mut master) = (48_000_u32, 24_u32, ChannelFormat::Stereo);
                for kv in &w[2..] {
                    match kv.split_once('=') {
                        Some(("master", "stereo")) => master = ChannelFormat::Stereo,
                        Some(("master", "5.1")) => master = ChannelFormat::Surround51,
                        Some(("master", _)) => return usage("master=stereo|5.1"),
                        Some(("rate", v)) => rate = v.parse().map_err(|_| MtError("rate=<hz>".into()))?,
                        Some(("depth" | "bits", v)) => bits = v.parse().map_err(|_| MtError("depth=16|24|32".into()))?,
                        _ => return usage("SESSION new <name> [rate=48000] [depth=16|24|32] [master=stereo|5.1]"),
                    }
                }
                let depth = bit_depth_from_bits(bits).ok_or_else(|| MtError("depth must be 16, 24 or 32 (float)".into()))?;
                self.create_with_master(word(1), rate, depth, master)?;
                Ok("OK".into())
            }
            ("SESSION", "new") => usage("SESSION new <name> [rate=48000] [depth=16|24|32] [master=stereo|5.1]"),
            ("MASTER", _) => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                if w.is_empty() {
                    return usage("MASTER vol=<dB> mute=0|1");
                }
                for kv in w {
                    match kv.split_once('=') {
                        Some(("vol" | "volume", v)) => mt.set_master_volume_db(v.parse().map_err(|_| MtError("vol is a number of dB".into()))?)?,
                        Some(("mute", v)) => mt.set_master_mute(matches!(v, "1" | "on"))?,
                        _ => return usage("MASTER vol=<dB> mute=0|1"),
                    }
                }
                Ok("OK".into())
            }
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
                    "add" if word(1).eq_ignore_ascii_case("video") => {
                        mt.add_video_track(Some(word(2)).filter(|n| !n.is_empty()))?;
                        Ok(format!("OK tracks={}", mt.session.tracks.iter().filter(|t| t.kind != TrackKind::Master).count()))
                    }
                    "add" => {
                        // `TRACK add [name] [mono|stereo|5.1]` or `TRACK add [name] fmt=mono|stereo|5.1`.
                        let mut format = ChannelFormat::Stereo;
                        let mut name: Option<&str> = None;
                        for (i, a) in w.iter().enumerate().skip(1) {
                            if let Some(f) = a.strip_prefix("fmt=") {
                                format = parse_track_format(f)?;
                            } else if i == 1 {
                                name = Some(a.as_str()).filter(|n| !n.is_empty());
                            } else if i == 2 {
                                format = parse_track_format(a)?;
                            } else {
                                return usage("TRACK add [name] [fmt=mono|stereo|5.1]");
                            }
                        }
                        mt.add_track_format(name, format)?;
                        Ok(format!("OK tracks={}", mt.track_count()))
                    }
                    "format" if w.len() == 3 => {
                        let id = mt.find_track(word(1))?;
                        mt.set_track_format(id, parse_track_format(word(2))?)?;
                        Ok("OK".into())
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
                        "TRACK add [name] [fmt=mono|stereo|5.1] | format <track> mono|stereo|5.1 | remove <track> | rename <track> <new> | mute|solo <track> 0|1 | volume <track> <dB> | arm <track> [input=<ch>] | disarm <track>",
                    ),
                }
            }
            ("CLIP", "add") if w.len() == 4 => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let id = mt.find_track(word(1))?;
                let at: f64 = word(3).parse().map_err(|_| MtError("<at_seconds> is a number".into()))?;
                if mt.session.track(id).is_some_and(|t| t.kind == TrackKind::Video) {
                    let path = PathBuf::from(word(2));
                    let i = crate::video::probe(&path).map_err(MtError)?;
                    let meta = VideoMeta { width: i.width, height: i.height, fps: i.fps, duration: i.duration, codec: i.codec };
                    mt.add_clip_video(id, &path.to_string_lossy(), &meta, at)?;
                } else {
                    mt.add_clip_file(id, &PathBuf::from(word(2)), at)?;
                }
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
            ("CLIPPROP", _) if w.len() >= 2 => {
                let mt = self.mt.as_mut().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let t = mt.find_track(word(0))?;
                let i: usize = word(1).parse().map_err(|_| MtError("<i> is a 0-based clip number".into()))?;
                let clip = mt.clip_at_index(t, i)?;
                let mut p = ClipProps::default();
                let opt = |v: &str, what: &str| -> Result<Option<f64>, MtError> {
                    if v.is_empty() || v == "none" { Ok(None) } else { v.parse().map(Some).map_err(|_| MtError(format!("{what} is a number"))) }
                };
                for kv in &w[2..] {
                    match kv.split_once('=') {
                        Some(("name", v)) => p.name = Some(v.into()),
                        Some(("gain", v)) => p.gain_db = Some(v.parse().map_err(|_| MtError("gain is a number of dB".into()))?),
                        Some(("tempo" | "bpm", v)) => p.bpm = Some(opt(v, "tempo")?),
                        Some(("beats", v)) => p.beats = Some(opt(v, "beats")?),
                        Some(("key", v)) => p.key = Some((!v.is_empty() && v != "none").then(|| v.to_string())),
                        _ => return usage(CLIPPROP_USAGE),
                    }
                }
                mt.set_clip_props(t, clip, &p)?;
                Ok("OK".into())
            }
            ("CLIPPROP", _) => usage(CLIPPROP_USAGE),
            ("CLIP", "select") if w.len() == 2 && word(1) == "none" => {
                self.sel_clips.clear();
                Ok("OK selected=0".into())
            }
            ("CLIP", "select") if w.len() == 3 => {
                let mt = self.mt.as_ref().ok_or_else(|| MtError("no session: SESSION new <name> first".into()))?;
                let id = mt.find_track(word(1))?;
                let n: usize = word(2).parse().map_err(|_| MtError("<n> is the clip's 1-based position on the track".into()))?;
                let clip = mt
                    .session
                    .track(id)
                    .and_then(|t| t.clips().get(n.wrapping_sub(1)))
                    .map(|c| c.id)
                    .ok_or_else(|| MtError(format!("no clip {n} on that track")))?;
                toggle(&mut self.sel_clips, clip);
                Ok(format!("OK selected={}", self.sel_clips.len()))
            }
            _ => usage("CLIP add <track> <path> <at_seconds> | CLIP adddoc <track> <at_seconds> | CLIP select <track> <n> | CLIP select none"),
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

const CLIPPROP_USAGE: &str = "CLIPPROP <track> <i> [tempo=<bpm>|none] [beats=<n>|none] [key=<text>] [name=<s>] [gain=<db>]";

fn toggle(v: &mut Vec<wavcraft_core::multitrack::soundcraft_model::ClipId>, id: wavcraft_core::multitrack::soundcraft_model::ClipId) {
    if let Some(i) = v.iter().position(|x| *x == id) {
        v.remove(i);
    } else {
        v.push(id);
    }
}

/// The range a lane is drawn over (volume shows -60 to +12 dB; the model's floor is lower).
fn display_range(p: &AutoParam) -> (f32, f32) {
    match p {
        AutoParam::Volume => (-60.0, 12.0),
        _ => (-1.0, 1.0),
    }
}

/// The automation lane row under a track (f109): a header with the parameter choice and the
/// lane drawn as a polyline. Click empty lane to add a point, drag a point to move it
/// (time and value), right-click a point to delete it, Thin removes redundant points.
#[allow(clippy::too_many_arguments)]
fn automation_row(
    ui: &mut egui::Ui,
    mt: &mut Multitrack,
    id: TrackId,
    width: f32,
    px_per_sec: f32,
    lanes: &mut HashMap<TrackId, AutoParam>,
    dragging: &mut Option<(TrackId, AutoParam, usize)>,
    failure: &mut Option<String>,
) {
    let Some(param) = lanes.get(&id).cloned() else { return };
    let (row, _) = ui.allocate_exact_size(vec2(HEADER_W + width, ROW_H), Sense::hover());
    let header = Rect::from_min_size(row.min, vec2(HEADER_W - 4.0, ROW_H));
    let lane = Rect::from_min_size(pos2(row.left() + HEADER_W, row.top()), vec2(width, ROW_H));
    ui.painter().rect_filled(header, 3.0, Color32::from_rgb(26, 30, 38));
    ui.painter().rect_filled(lane, 0.0, Color32::from_rgb(14, 16, 20));
    let mut h = ui.new_child(egui::UiBuilder::new().max_rect(header.shrink2(vec2(8.0, 3.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut choice = param.clone();
    h.horizontal(|ui| {
        ui.weak("Lane");
        egui::ComboBox::from_id_salt(("auto-param", id.0)).selected_text(param.label()).width(70.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut choice, AutoParam::Volume, "volume");
            ui.selectable_value(&mut choice, AutoParam::Pan(0), "pan");
        });
    });
    let n = mt.lane(id, &param).map_or(0, |l| l.points.len());
    h.horizontal(|ui| {
        ui.weak(format!("{n} points"));
        if ui
            .add_enabled(n > 2, egui::Button::new("Thin").small())
            .on_hover_text("Remove points that change the curve by under a tolerance")
            .clicked()
        {
            let tol = if param == AutoParam::Volume { 0.25 } else { 0.01 };
            note(failure, mt.automation_thin(id, &param, tol).map(|_| ()));
        }
    });
    h.weak("click: add · drag: move · right-click: delete");
    if choice != param {
        lanes.insert(id, choice);
        return;
    }
    let (lo, hi) = display_range(&param);
    let rate = f64::from(mt.rate());
    let to_pos =
        |t: f64, v: f32| pos2(lane.left() + t as f32 * px_per_sec, lane.bottom() - 4.0 - ((v.clamp(lo, hi) - lo) / (hi - lo)) * (ROW_H - 8.0));
    let from_pos = |p: egui::Pos2| {
        let t = f64::from(((p.x - lane.left()) / px_per_sec).max(0.0));
        let v = lo + ((lane.bottom() - 4.0 - p.y) / (ROW_H - 8.0)).clamp(0.0, 1.0) * (hi - lo);
        (t, v)
    };
    let pts: Vec<egui::Pos2> =
        mt.lane(id, &param).map(|l| l.points.iter().map(|p| to_pos(p.at as f64 / rate, p.value)).collect()).unwrap_or_default();
    let resp = ui.interact(lane, ui.id().with(("auto", id.0)), Sense::click_and_drag());
    let near = |at: egui::Pos2| pts.iter().position(|q| q.distance(at) <= 8.0);
    if let Some(at) = resp.interact_pointer_pos() {
        if resp.drag_started() {
            *dragging = near(at).map(|i| (id, param.clone(), i));
        }
        if resp.dragged()
            && let Some((tid, tp, i)) = dragging.clone().filter(|(t, p, _)| *t == id && *p == param)
        {
            let (t, v) = from_pos(at);
            match mt.automation_move(tid, &tp, i, t, v) {
                Ok(ni) => *dragging = Some((tid, tp, ni)),
                Err(e) => *failure = Some(e.to_string()),
            }
        }
        if resp.clicked() && near(at).is_none() {
            let (t, v) = from_pos(at);
            note(failure, mt.automation_add(id, &param, t, v).map(|_| ()));
        }
        if resp.secondary_clicked()
            && let Some(i) = near(at)
        {
            note(failure, mt.automation_delete(id, &param, i));
        }
    }
    if resp.drag_stopped() {
        *dragging = None;
    }
    let p = ui.painter_at(lane);
    let (zero_v, zero_label) = if param == AutoParam::Volume { (0.0, "0 dB") } else { (0.0, "centre") };
    let zy = to_pos(0.0, zero_v).y;
    p.line_segment([pos2(lane.left(), zy), pos2(lane.right(), zy)], Stroke::new(1.0, Color32::from_gray(60)));
    p.text(pos2(lane.left() + 3.0, zy - 1.0), egui::Align2::LEFT_BOTTOM, zero_label, egui::FontId::monospace(9.0), Color32::from_gray(110));
    let line = Color32::from_rgb(240, 190, 70);
    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
        let mut path = vec![pos2(lane.left(), first.y)];
        path.extend(pts.iter().copied());
        path.push(pos2(lane.right(), last.y));
        p.add(egui::Shape::line(path, Stroke::new(1.5, line)));
    }
    for q in &pts {
        p.circle_filled(*q, 4.0, line);
        p.circle_stroke(*q, 4.0, Stroke::new(1.0, Color32::BLACK));
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
pub fn tokens(line: &str) -> Result<Vec<String>, String> {
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
    fn the_control_verbs_set_the_master() {
        let mut v = MtView::default();
        assert!(v.control("MASTER", "vol=-3", None).starts_with("ERR no session"));
        assert!(v.control("SESSION", "new X master=quad", None).starts_with("ERR"));
        assert_eq!(v.control("SESSION", "new X master=5.1", None), "OK");
        assert_eq!(v.control("TRACK", "add Surr 5.1", None), "OK tracks=1");
        assert_eq!(v.control("MASTER", "vol=-6 mute=1", None), "OK");
        let st = v.control("SESSION", "state", None);
        assert!(st.contains(r#""master":{"channels":6"#) && st.contains(r#""mute":true"#) && st.contains(r#""volume_db":-6.0"#), "{st}");
        assert!(v.control("MASTER", "vol=x", None).starts_with("ERR"));
        assert_eq!(v.control("SESSION", "new Y", None), "OK");
        assert_eq!(v.control("TRACK", "add Surr 5.1", None), "OK tracks=1", "a 5.1 track is allowed beside a stereo master too (#333)");
    }

    #[test]
    fn cliptype_assigns_one_or_many_and_sets_the_filter() {
        let mut v = MtView::default();
        assert!(v.control("CLIPTYPE", "1 1 music", None).starts_with("ERR no session"));
        v.control("SESSION", "new T", None);
        v.control("TRACK", "add A", None);
        assert!(v.control("CLIPTYPE", "A 1 music", None).starts_with("ERR no clip 1"));
        assert!(v.control("CLIPTYPE", "A all music", None).starts_with("ERR no clips"));
        assert!(v.control("CLIPTYPE", "A 1", None).starts_with("ERR usage"));
        assert!(v.control("CLIPTYPE", "filter foley", None).starts_with("ERR no audio type"));
        assert_eq!(v.control("CLIPTYPE", "filter sfx", None), "OK filter=sfx");
        assert!(v.control("SESSION", "state", None).contains(r#""filter":"sfx""#));
        assert_eq!(v.control("CLIPTYPE", "filter all", None), "OK filter=all");
    }

    #[test]
    fn the_control_verbs_make_a_session_with_tracks() {
        let mut v = MtView::default();
        assert!(v.control("TRACK", "add", None).starts_with("ERR no session"));
        assert_eq!(v.control("SESSION", "new Demo rate=44100 depth=16", None), "OK");
        assert!(v.show, "making a session shows it");
        assert_eq!(v.control("TRACK", "add Drums", None), "OK tracks=1");
        assert_eq!(v.control("TRACK", "add", None), "OK tracks=2");
        assert_eq!(v.control("TRACK", "add Sur fmt=5.1", None), "OK tracks=3");
        assert!(v.control("TRACK", "add Bad fmt=7.1", None).starts_with("ERR"));
        assert_eq!(v.control("TRACK", "format Sur mono", None), "OK");
        assert_eq!(v.control("TRACK", "remove Sur", None), "OK tracks=2");
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

    #[test]
    fn clipprop_needs_a_session_and_a_clip_and_checks_its_values() {
        let mut v = MtView::default();
        assert!(v.control("CLIPPROP", "1 0 tempo=90", None).starts_with("ERR no session"));
        v.control("SESSION", "new Demo", None);
        v.control("TRACK", "add Drums", None);
        assert!(v.control("CLIPPROP", "1 0 tempo=90", None).starts_with("ERR the track has 0 clip"));
        assert!(v.control("CLIPPROP", "1", None).starts_with("ERR usage"));
        assert!(v.control("CLIPPROP", "1 x", None).starts_with("ERR <i>"));
        assert!(v.control("CLIPPROP", "nope 0", None).starts_with("ERR no track"));
    }

    #[test]
    fn the_auto_verbs_add_move_delete_and_thin_points() {
        let mut v = MtView::default();
        assert!(v.control("AUTO", "state", None).starts_with("ERR no session"));
        v.control("SESSION", "new Demo", None);
        v.control("TRACK", "add Drums", None);
        assert_eq!(v.control("AUTO", "add Drums volume 1 0", None), "OK index=0 points=1");
        assert_eq!(v.control("AUTO", "add 1 vol 2 -3", None), "OK index=1 points=2");
        assert_eq!(v.control("AUTO", "add 1 volume 3 -6", None), "OK index=2 points=3");
        assert_eq!(v.control("AUTO", "move Drums volume 0 4 -9", None), "OK index=2 points=3");
        assert_eq!(v.control("AUTO", "thin Drums volume 0.5", None), "OK removed=1 points=2");
        assert_eq!(v.control("AUTO", "delete Drums volume 0", None), "OK points=1");
        assert!(v.control("AUTO", "delete Drums volume 5", None).starts_with("ERR no point 5"));
        assert!(v.control("AUTO", "add Drums mute 1 1", None).starts_with("ERR automation lanes here"));
        assert!(v.control("AUTO", "add Drums pan x 1", None).starts_with("ERR"));
        assert_eq!(v.control("AUTO", "show Drums pan", None), "OK");
        let st = v.control("AUTO", "state", None);
        assert!(st.contains(r#""shown":{"Drums":"pan"}"#) && st.contains(r#""volume":[[4.0,-9.0]]"#), "{st}");
    }
}
