//! The spectral display's controls on the app: the divider between it and the waveform, the
//! View menu's entry, the resolution keys and the `SPECTRAL …` control verb. The options are the
//! Spectral Displays preferences (`spec_resolution`, `spec_window`, `spec_db_range`) plus the
//! divider's share (`spec_split_pct`); whether the display shows is the Window menu's panel.

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use wavcraft_core::actions::ActionId;
use wavcraft_core::freq_axis::{FreqAxis, Scale};
use wavcraft_core::settings::{MAX_SPEC_DB_RANGE, MAX_SPEC_SPLIT_PCT, MIN_SPEC_DB_RANGE, MIN_SPEC_SPLIT_PCT, SPEC_RESOLUTIONS, SpecWindow};
use wavcraft_core::shortcuts::Command;
use wavcraft_core::spectrogram::{SpectrogramParams, step_fft_size};
use wavcraft_core::statusbar::StatusField;

use super::{SpectrumCache, WaveApp};

/// Decibel ranges the View menu offers (inside the preference's limits).
const RANGES_DB: [u32; 7] = [40, 60, 80, 100, 120, 140, 160];
/// How tall the draggable divider is, in points.
const HANDLE: f32 = 8.0;
const DIVIDER: Color32 = Color32::from_gray(90);

impl WaveApp {
    /// The options as `STATE` reports them.
    pub(super) fn spectral_state(&self) -> String {
        format!(
            "spectral={} fft={} window={} range={} split={}",
            u8::from(self.panels.is_shown("spectral")),
            self.settings.spec_resolution,
            self.settings.spec_window.name(),
            self.settings.spec_db_range,
            self.settings.spec_split_pct
        )
    }

    /// Apply a changed resolution, window or range to the display at once, and save it.
    pub(super) fn spectral_changed(&mut self) {
        self.settings.apply_to_spectrogram(&mut self.params);
        self.spectrum = None;
        self.spectral_dirty = true;
    }

    /// Spectral resolution up (`steps` > 0: a longer FFT, finer in pitch) or down.
    pub(super) fn step_resolution(&mut self, steps: i32) {
        self.settings.spec_resolution = step_fft_size(self.settings.spec_resolution as usize, steps) as u32;
        self.spectral_changed();
    }

    /// `SPECTRAL …` on the control channel.
    pub(super) fn control_spectral(&mut self, rest: &str) -> String {
        const USAGE: &str = "ERR SPECTRAL [show|hide|toggle | fft <256..8192>|up|down | window <name> | range <40..160> | split <10..90> | view <lo_hz> <hi_hz> [lin|log_partial|log] | scale lin|log_partial|log_full | scroll <hz> | reset]";
        // The frequency band's verbs answer with the band alone.
        if matches!(rest.split_whitespace().next(), Some("view" | "scale" | "scroll" | "reset")) {
            return self.control_freq_band(rest);
        }
        let mut words = rest.split_whitespace();
        let (verb, arg) = (words.next(), words.next());
        match (verb, arg, words.next()) {
            (None, None, _) => {}
            (Some("show"), None, None) => self.set_panel("spectral", true),
            (Some("hide"), None, None) => self.set_panel("spectral", false),
            (Some("toggle"), None, None) => {
                let shown = self.panels.is_shown("spectral");
                self.set_panel("spectral", !shown);
            }
            (Some("fft"), Some("up"), None) => self.step_resolution(1),
            (Some("fft"), Some("down"), None) => self.step_resolution(-1),
            (Some("fft"), Some(n), None) => match n.parse::<u32>() {
                Ok(n) if SPEC_RESOLUTIONS.contains(&n) => {
                    self.settings.spec_resolution = n;
                    self.spectral_changed();
                }
                _ => return format!("ERR fft must be one of {SPEC_RESOLUTIONS:?}"),
            },
            (Some("window"), Some(name), None) => match SpecWindow::from_name(name) {
                Some(w) => {
                    self.settings.spec_window = w;
                    self.spectral_changed();
                }
                None => return format!("ERR window must be one of {:?}", SpecWindow::NAMES),
            },
            (Some("range"), Some(db), None) => match db.parse::<u32>() {
                Ok(db) if (MIN_SPEC_DB_RANGE..=MAX_SPEC_DB_RANGE).contains(&db) => {
                    self.settings.spec_db_range = db;
                    self.spectral_changed();
                }
                _ => return format!("ERR range must be {MIN_SPEC_DB_RANGE}..{MAX_SPEC_DB_RANGE} dB"),
            },
            (Some("split"), Some(pct), None) => match pct.parse::<u32>() {
                Ok(p) if (MIN_SPEC_SPLIT_PCT..=MAX_SPEC_SPLIT_PCT).contains(&p) => {
                    self.settings.spec_split_pct = p;
                    self.spectral_dirty = true;
                }
                _ => return format!("ERR split must be {MIN_SPEC_SPLIT_PCT}..{MAX_SPEC_SPLIT_PCT} (percent of the height the waveform keeps)"),
            },
            _ => return USAGE.to_string(),
        }
        if self.spectral_dirty {
            self.save_spectral_now();
        }
        format!("OK {}", self.spectral_state())
    }

    /// Write the settings file; a failure is logged, the setting still holds for this run.
    pub(super) fn persist_settings(&mut self) {
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the settings to {}: {e}", file.display());
        }
    }

    /// The open file's sample rate (48 kHz with none open, only to bound the band).
    pub(super) fn rate(&self) -> u32 {
        self.doc.as_ref().map_or(48_000, |d| d.sample_rate())
    }

    /// The spectral display's band as drawn: the parameters' band made usable for the file's rate.
    pub(super) fn freq_axis(&self) -> FreqAxis {
        FreqAxis::from_params(&self.params).clamped(self.rate())
    }

    /// Show `axis` (made usable for the file's rate) in the spectral display.
    fn set_freq_axis(&mut self, axis: FreqAxis) {
        let rate = self.rate();
        set_axis(&mut self.params, &mut self.spectrum, axis, rate);
    }

    /// Keep the band and scale in the settings once nothing is being dragged.
    pub(super) fn keep_freq_axis(&mut self, ctx: &egui::Context) {
        let shown = FreqAxis::from_params(&self.params);
        let kept = self.settings.spectral_view.unwrap_or_else(|| FreqAxis::from_params(&SpectrogramParams::default()));
        if shown != kept && self.freq_drag.is_none() && !ctx.input(|i| i.pointer.any_down()) {
            self.settings.spectral_view = Some(shown);
            self.persist_settings();
        }
    }

    /// `SPECTRAL view <lo_hz> <hi_hz> [lin|log_partial|log]`, `SPECTRAL scale lin|log_partial|log_full`,
    /// `SPECTRAL scroll <hz>` (the band's low edge, as dragging the ruler would move it) and
    /// `SPECTRAL reset` from the control channel.
    fn control_freq_band(&mut self, rest: &str) -> String {
        const USAGE: &str = "ERR SPECTRAL view <lo_hz> <hi_hz> [lin|log_partial|log] | scale lin|log_partial|log_full | scroll <hz> | reset";
        let current = self.freq_axis().scale();
        let mut words = rest.split_whitespace();
        match words.next() {
            Some("view") => {
                let (lo, hi) = (words.next().and_then(|w| w.parse::<f32>().ok()), words.next().and_then(|w| w.parse::<f32>().ok()));
                let scale = match words.next() {
                    None => Some(current),
                    Some(word) => Scale::parse(word),
                };
                match (lo, hi, scale) {
                    (Some(lo), Some(hi), Some(scale)) if lo.is_finite() && hi.is_finite() && lo >= 0.0 && hi > lo => {
                        self.set_freq_axis(FreqAxis { min_hz: lo, max_hz: hi, log: scale != Scale::Linear, partial: scale == Scale::LogPartial });
                        format!("OK {}", self.freq_band_state())
                    }
                    _ => USAGE.to_string(),
                }
            }
            Some("scale") => match words.next().and_then(Scale::parse) {
                Some(scale) => {
                    let axis = self.freq_axis().with_scale_kind(scale, self.rate());
                    self.set_freq_axis(axis);
                    format!("OK {}", self.freq_band_state())
                }
                None => USAGE.to_string(),
            },
            Some("scroll") => match words.next().and_then(|w| w.parse::<f32>().ok()).filter(|hz| hz.is_finite()) {
                Some(hz) => {
                    let axis = self.freq_axis().scroll_to(hz, self.rate());
                    self.set_freq_axis(axis);
                    format!("OK {}", self.freq_band_state())
                }
                None => USAGE.to_string(),
            },
            Some("reset") => {
                self.set_freq_axis(FreqAxis::full_scale(current, self.rate()));
                format!("OK {}", self.freq_band_state())
            }
            _ => USAGE.to_string(),
        }
    }

    /// `freqband=<lo>-<hi>:<lin|log_partial|log>`: the band as drawn.
    pub(super) fn freq_band_state(&self) -> String {
        let a = self.freq_axis();
        format!("freqband={}-{}:{}", a.min_hz, a.max_hz, a.scale().word())
    }

    /// `statusbar=<fields shown>`.
    pub(super) fn status_state(&self) -> String {
        format!("statusbar={}", self.settings.status_fields.to_list())
    }

    /// `STATUSBAR show|hide <field>` and `STATUSBAR list` from the control channel.
    pub(super) fn control_statusbar(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        let (verb, name) = (words.next(), words.next());
        let usage = || {
            let names: Vec<&str> = StatusField::ALL.iter().map(|f| f.key()).collect();
            format!("ERR STATUSBAR show|hide <field> | STATUSBAR list; fields: {}", names.join(" "))
        };
        match (verb, name.map(StatusField::parse)) {
            (Some("list"), _) => format!("OK {}", self.status_state()),
            (Some(v @ ("show" | "hide")), Some(Some(field))) => {
                self.settings.status_fields.set(field, v == "show");
                self.persist_settings();
                format!("OK {}", self.status_state())
            }
            _ => usage(),
        }
    }

    /// View > Frequency scale (linear, logarithmic above 1 kHz, fully logarithmic) and Reset
    /// frequency range.
    pub(super) fn frequency_scale_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Frequency scale", |ui| {
            let current = self.freq_axis().scale();
            for (scale, name) in [(Scale::Linear, "Linear"), (Scale::LogPartial, "Logarithmic (above 1 kHz)"), (Scale::LogFull, "Logarithmic (full)")]
            {
                if ui.radio(current == scale, name).clicked() {
                    let axis = self.freq_axis().with_scale_kind(scale, self.rate());
                    self.set_freq_axis(axis);
                    ui.close();
                }
            }
        });
        if ui.button("Reset frequency range").on_hover_text("Show the whole frequency range (or double-click the frequency ruler)").clicked() {
            self.set_freq_axis(FreqAxis::full_scale(self.freq_axis().scale(), self.rate()));
        }
    }

    /// Write the divider, the display options and the panels' state to the settings file.
    pub(super) fn save_spectral_now(&mut self) {
        self.spectral_dirty = false;
        self.save_panels();
    }

    /// View › Spectral display: resolution, window and decibel range.
    pub(super) fn spectral_menu(&mut self, ui: &mut egui::Ui) {
        let mac = cfg!(target_os = "macos");
        ui.menu_button("Spectral display", |ui| {
            let up = self.keymap.menu_text(Command::Action(ActionId::SpectralResolutionUp), mac);
            let down = self.keymap.menu_text(Command::Action(ActionId::SpectralResolutionDown), mac);
            if ui.add(egui::Button::new("Resolution up").shortcut_text(up)).clicked() {
                self.step_resolution(1);
            }
            if ui.add(egui::Button::new("Resolution down").shortcut_text(down)).clicked() {
                self.step_resolution(-1);
            }
            ui.label(format!("FFT size {}", self.settings.spec_resolution));
            ui.separator();
            for w in SpecWindow::ALL.iter().copied() {
                if ui.radio(self.settings.spec_window == w, w.name()).clicked() {
                    self.settings.spec_window = w;
                    self.spectral_changed();
                }
            }
            ui.separator();
            for db in RANGES_DB {
                if ui.radio(self.settings.spec_db_range == db, format!("{db} dB range")).clicked() {
                    self.settings.spec_db_range = db;
                    self.spectral_changed();
                }
            }
        });
    }
}

/// Where the divider is and how to draw it (after the lanes, so it shows over their edges).
pub struct Divider {
    pub y: f32,
    lit: bool,
    /// The person moved the divider or toggled the display: the options want saving.
    pub changed: bool,
}

impl Divider {
    pub fn draw(&self, painter: &egui::Painter, body: Rect, show_waveform: bool) {
        if show_waveform {
            let (width, color) = if self.lit { (3.0, Color32::from_gray(170)) } else { (1.0, DIVIDER) };
            painter.line_segment([pos2(body.left(), self.y), pos2(body.right(), self.y)], Stroke::new(width, color));
        }
    }
}

/// The divider between the waveform and the spectral display, over `body`: drag it to move it,
/// double-click it to hide or show the spectral display. With the spectral display hidden there
/// is a thin handle along the bottom edge to bring it back. `y` is the top of `body` when the
/// waveform is hidden (nothing to divide).
pub fn divider(ui: &mut egui::Ui, body: Rect, show_waveform: bool, show_spectrum: &mut bool, split_pct: &mut u32) -> Divider {
    let mut out = Divider { y: body.top(), lit: false, changed: false };
    if !show_waveform {
        return out;
    }
    out.y = if *show_spectrum { body.top() + body.height() * *split_pct as f32 / 100.0 } else { body.bottom() - HANDLE * 0.5 };
    let handle = Rect::from_center_size(pos2(body.center().x, out.y), vec2(body.width(), HANDLE));
    let response = ui.interact(handle, ui.id().with("spectral-divider"), Sense::click_and_drag());
    let response =
        response.on_hover_cursor(CursorIcon::ResizeVertical).on_hover_text("Drag to resize. Double-click to hide or show the spectral display.");
    out.lit = response.hovered() || response.dragged();
    if response.double_clicked() {
        *show_spectrum = !*show_spectrum;
        out.changed = true;
    } else if *show_spectrum
        && response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
        && body.height() > 1.0
    {
        let pct = ((pos.y - body.top()) / body.height() * 100.0).round().clamp(MIN_SPEC_SPLIT_PCT as f32, MAX_SPEC_SPLIT_PCT as f32) as u32;
        out.changed = pct != *split_pct;
        *split_pct = pct;
    }
    out
}

/// A drag that started on the frequency ruler: left button scrolls the band, right button zooms it.
#[derive(Clone, Copy)]
pub(super) struct FreqDrag {
    pub origin: Pos2,
    /// The band when the drag started; the drag is applied to it, so it never drifts.
    pub axis: FreqAxis,
    pub zoom: bool,
    /// Where in the lane (0 bottom, 1 top) the drag began: the frequency kept in place when zooming.
    pub anchor_t: f32,
    /// Height of the lane it began in.
    pub lane_height: f32,
}

/// Show `axis` (made usable for `rate`) in `params`; a picture drawn on the other scale is dropped.
pub(super) fn set_axis(params: &mut SpectrogramParams, spectrum: &mut Option<SpectrumCache>, axis: FreqAxis, rate: u32) {
    let axis = axis.clamped(rate);
    if axis.log != params.log_freq || axis.partial != params.partial_log {
        *spectrum = None;
    }
    axis.apply_to(params);
}

/// The frequency ruler for one lane: a darkened strip with a tick and a Hz or kHz label at each mark.
pub(super) fn draw_freq_ruler(painter: &egui::Painter, lane: Rect, axis: &FreqAxis) {
    let painter = painter.with_clip_rect(lane);
    painter.rect_filled(lane, 0.0, Color32::from_black_alpha(170));
    painter.line_segment([pos2(lane.right(), lane.top()), pos2(lane.right(), lane.bottom())], Stroke::new(1.0, Color32::from_gray(90)));
    for tick in axis.ticks(lane.height(), 26.0) {
        let y = lane.bottom() - tick.t * lane.height();
        painter.line_segment([pos2(lane.right() - 5.0, y), pos2(lane.right(), y)], Stroke::new(1.0, Color32::from_gray(160)));
        // Keep the label inside the lane at the very top and bottom.
        let label_y = y.clamp(lane.top() + 6.0, lane.bottom() - 6.0);
        painter.text(
            pos2(lane.right() - 8.0, label_y),
            egui::Align2::RIGHT_CENTER,
            tick.label,
            egui::FontId::monospace(10.0),
            Color32::from_gray(210),
        );
    }
}
