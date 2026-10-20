//! The spectral display's options (roadmap A061, A063-A065): show or hide it, the divider
//! between it and the waveform, resolution (FFT length), window function and decibel range.
//! They live in the settings file, and `SPECTRAL …` on the control channel sets them.

use egui::{Color32, CursorIcon, Rect, Sense, Stroke, pos2, vec2};
use wavcraft_core::settings::SpectralSettings;
use wavcraft_core::spectrogram::{FFT_SIZES, MAX_RANGE_DB, MIN_RANGE_DB, WindowKind, step_fft_size};

use super::WaveApp;

/// Share of the height the waveform keeps when both views show, in percent.
pub const DEFAULT_SPLIT_PCT: u32 = 50;
/// The divider stays this far (in percent) from either edge.
const SPLIT_MIN_PCT: u32 = 10;
const SPLIT_MAX_PCT: u32 = 90;
/// Decibel ranges the View menu offers.
const RANGES_DB: [u32; 7] = [60, 80, 100, 120, 140, 160, 180];
/// How tall the draggable divider is, in points.
const HANDLE: f32 = 8.0;
const DIVIDER: Color32 = Color32::from_gray(90);

impl WaveApp {
    /// Take the saved options (each clamped: the file may have been edited by hand).
    pub(super) fn apply_spectral_settings(&mut self, s: &SpectralSettings) {
        if let Some(show) = s.show {
            self.show_spectrum = show;
        }
        if let Some(fft) = s.fft {
            self.params.fft_size = step_fft_size(fft, 0);
        }
        if let Some(window) = s.window.as_deref().and_then(WindowKind::parse) {
            self.params.window = window;
        }
        if let Some(range) = s.range_db {
            self.params = self.params.with_range_db(range as f32);
        }
        if let Some(pct) = s.split_pct {
            self.spectral_split_pct = pct.clamp(SPLIT_MIN_PCT, SPLIT_MAX_PCT);
        }
    }

    /// Write the options to the settings file.
    pub(super) fn save_spectral_settings(&mut self) {
        self.spectral_dirty = false;
        self.settings.spectral = SpectralSettings {
            show: Some(self.show_spectrum),
            fft: Some(self.params.fft_size),
            window: Some(self.params.window.name().to_string()),
            range_db: Some(self.params.range_db().round() as u32),
            split_pct: Some(self.spectral_split_pct),
        };
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the spectral display options to {}: {e}", file.display());
        }
    }

    /// The options as `STATE` reports them.
    pub(super) fn spectral_state(&self) -> String {
        format!(
            "spectral={} fft={} window={} range={} split={}",
            u8::from(self.show_spectrum),
            self.params.fft_size,
            self.params.window.name(),
            self.params.range_db().round(),
            self.spectral_split_pct
        )
    }

    /// Spectral resolution up (`steps` > 0: a longer FFT, finer in pitch) or down.
    pub(super) fn step_resolution(&mut self, steps: i32) {
        self.params.fft_size = step_fft_size(self.params.fft_size, steps);
        self.spectral_changed();
    }

    pub(super) fn set_spectrum_shown(&mut self, show: bool) {
        self.show_spectrum = show;
        self.spectral_changed();
    }

    fn spectral_changed(&mut self) {
        self.save_spectral_settings();
    }

    /// `SPECTRAL …` on the control channel.
    pub(super) fn control_spectral(&mut self, rest: &str) -> String {
        const USAGE: &str = "ERR SPECTRAL [show|hide|toggle | fft <64..16384>|up|down | window <name> | range <20..200> | split <10..90>]";
        let mut words = rest.split_whitespace();
        let (verb, arg) = (words.next(), words.next());
        match (verb, arg, words.next()) {
            (None, None, _) => {}
            (Some("show"), None, None) => self.set_spectrum_shown(true),
            (Some("hide"), None, None) => self.set_spectrum_shown(false),
            (Some("toggle"), None, None) => self.set_spectrum_shown(!self.show_spectrum),
            (Some("fft"), Some("up"), None) => self.step_resolution(1),
            (Some("fft"), Some("down"), None) => self.step_resolution(-1),
            (Some("fft"), Some(n), None) => match n.parse::<usize>() {
                Ok(n) if FFT_SIZES.contains(&n) => {
                    self.params.fft_size = n;
                    self.spectral_changed();
                }
                _ => return format!("ERR fft must be one of {FFT_SIZES:?}"),
            },
            (Some("window"), Some(_), _) => {
                let name = rest.trim_start().strip_prefix("window").unwrap_or("").trim();
                match WindowKind::parse(name) {
                    Some(w) => {
                        self.params.window = w;
                        self.spectral_changed();
                    }
                    None => {
                        let all: Vec<&str> = WindowKind::ALL.iter().map(|w| w.name()).collect();
                        return format!("ERR window must be one of {all:?}");
                    }
                }
            }
            (Some("range"), Some(db), None) => match db.parse::<f32>() {
                Ok(db) if db.is_finite() && (MIN_RANGE_DB..=MAX_RANGE_DB).contains(&db) => {
                    self.params = self.params.with_range_db(db);
                    self.spectral_changed();
                }
                _ => return format!("ERR range must be {MIN_RANGE_DB}..{MAX_RANGE_DB} dB"),
            },
            (Some("split"), Some(pct), None) => match pct.parse::<u32>() {
                Ok(p) if (SPLIT_MIN_PCT..=SPLIT_MAX_PCT).contains(&p) => {
                    self.spectral_split_pct = p;
                    self.spectral_changed();
                }
                _ => return format!("ERR split must be {SPLIT_MIN_PCT}..{SPLIT_MAX_PCT} (percent of the height the waveform keeps)"),
            },
            _ => return USAGE.to_string(),
        }
        format!("OK {}", self.spectral_state())
    }

    /// View › Spectral display: resolution, window and decibel range.
    pub(super) fn spectral_menu(&mut self, ui: &mut egui::Ui) {
        let mac = cfg!(target_os = "macos");
        let keys = if mac { ("⇧⌘↑", "⇧⌘↓") } else { ("Ctrl+Shift+↑", "Ctrl+Shift+↓") };
        ui.menu_button("Spectral display", |ui| {
            if ui.add(egui::Button::new("Resolution up").shortcut_text(keys.0)).clicked() {
                self.step_resolution(1);
            }
            if ui.add(egui::Button::new("Resolution down").shortcut_text(keys.1)).clicked() {
                self.step_resolution(-1);
            }
            ui.label(format!("FFT size {}", self.params.fft_size));
            ui.separator();
            for w in WindowKind::ALL {
                if ui.radio(self.params.window == w, w.label()).clicked() {
                    self.params.window = w;
                    self.spectral_changed();
                }
            }
            ui.separator();
            for db in RANGES_DB {
                if ui.radio(self.params.range_db().round() as u32 == db, format!("{db} dB range")).clicked() {
                    self.params = self.params.with_range_db(db as f32);
                    self.spectral_changed();
                }
            }
        });
    }

    /// The keys for resolution up and down (Shift+Ctrl+Up/Down; Shift+Cmd on a Mac).
    pub(super) fn spectral_keys(&mut self, ctx: &egui::Context) {
        let mods = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        let (up, down) = ctx.input_mut(|i| (i.consume_key(mods, egui::Key::ArrowUp), i.consume_key(mods, egui::Key::ArrowDown)));
        if up {
            self.step_resolution(1);
        }
        if down {
            self.step_resolution(-1);
        }
    }
}

/// Where the divider is and how to draw it (after the lanes, so it shows over their edges).
pub struct Divider {
    pub y: f32,
    lit: bool,
    /// The person moved it or toggled the display: the options want saving.
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
        let pct = ((pos.y - body.top()) / body.height() * 100.0).round().clamp(SPLIT_MIN_PCT as f32, SPLIT_MAX_PCT as f32) as u32;
        out.changed = pct != *split_pct;
        *split_pct = pct;
    }
    out
}
