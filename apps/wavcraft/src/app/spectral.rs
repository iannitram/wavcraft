//! The spectral display's controls on the app: the divider between it and the waveform, the
//! View menu's entry, the resolution keys and the `SPECTRAL …` control verb. The options are the
//! Spectral Displays preferences (`spec_resolution`, `spec_window`, `spec_db_range`) plus the
//! divider's share (`spec_split_pct`); whether the display shows is the Window menu's panel.

use egui::{Color32, CursorIcon, Rect, Sense, Stroke, pos2, vec2};
use wavcraft_core::actions::ActionId;
use wavcraft_core::settings::{MAX_SPEC_DB_RANGE, MAX_SPEC_SPLIT_PCT, MIN_SPEC_DB_RANGE, MIN_SPEC_SPLIT_PCT, SPEC_RESOLUTIONS, SpecWindow};
use wavcraft_core::shortcuts::Command;
use wavcraft_core::spectrogram::step_fft_size;

use super::WaveApp;

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
        const USAGE: &str = "ERR SPECTRAL [show|hide|toggle | fft <256..8192>|up|down | window <name> | range <40..160> | split <10..90>]";
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
