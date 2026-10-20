//! The Convert Sample Type and Interpret Sample Rate dialogs (Edit menu, Shift+T). They only
//! collect settings; the window runs `wavcraft_core::convert` on a worker thread.

use egui::vec2;
use wavcraft_core::convert::{ConvertSettings, PresetFile, Quality};
use wavcraft_core::dither::{DitherSettings, DitherType, Shaping};
use wavcraft_core::save::SampleType;

/// Rates offered in the list; any other whole number can be typed.
const RATES: [u32; 11] = [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Chans {
    Keep,
    Mono,
    Stereo,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bits {
    Keep,
    B16,
    B24,
    F32,
}

/// What the person did this frame.
pub enum Outcome {
    Open,
    Cancel,
    Apply(ConvertSettings),
}

/// The Convert Sample Type dialog's state.
pub struct ConvertDialog {
    rate: String,
    chans: Chans,
    /// Percent of each source channel in a mix to mono.
    mix: Vec<f32>,
    bits: Bits,
    dither: DitherSettings,
    quality: Quality,
    preset_name: String,
    message: Option<String>,
    src_rate: u32,
    src_channels: usize,
}

impl ConvertDialog {
    pub fn new(src_rate: u32, src_channels: usize) -> ConvertDialog {
        ConvertDialog {
            rate: src_rate.to_string(),
            chans: Chans::Keep,
            mix: vec![100.0 / src_channels.max(1) as f32; src_channels],
            bits: Bits::Keep,
            dither: DitherSettings::default(),
            quality: Quality::default(),
            preset_name: String::new(),
            message: None,
            src_rate,
            src_channels,
        }
    }

    /// Fill the controls from `s`.
    fn load(&mut self, s: &ConvertSettings) {
        self.rate = s.rate.unwrap_or(self.src_rate).to_string();
        self.chans = match s.channels {
            None => Chans::Keep,
            Some(1) => Chans::Mono,
            Some(_) => Chans::Stereo,
        };
        if s.mix_pct.len() == self.src_channels {
            self.mix = s.mix_pct.clone();
        }
        self.bits = match s.bits {
            None => Bits::Keep,
            Some(SampleType::Int16) => Bits::B16,
            Some(SampleType::Int24) => Bits::B24,
            Some(SampleType::Int32 | SampleType::Float32) => Bits::F32,
        };
        self.dither = s.dither.unwrap_or_default();
        self.quality = s.quality;
    }

    /// The settings the controls describe, or why they can't be used.
    fn settings(&self) -> Result<ConvertSettings, String> {
        let rate: u32 = self.rate.trim().parse().map_err(|_| "Sample rate: type a whole number of Hz".to_string())?;
        let channels = match self.chans {
            Chans::Keep => None,
            Chans::Mono => Some(1),
            Chans::Stereo => Some(2),
        };
        let bits = match self.bits {
            Bits::Keep => None,
            Bits::B16 => Some(SampleType::Int16),
            Bits::B24 => Some(SampleType::Int24),
            Bits::F32 => Some(SampleType::Float32),
        };
        let mix_pct = if self.chans == Chans::Mono && self.src_channels > 1 { self.mix.clone() } else { Vec::new() };
        Ok(ConvertSettings {
            rate: (rate != self.src_rate).then_some(rate),
            channels,
            mix_pct,
            bits,
            dither: Some(self.dither),
            quality: self.quality,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, presets: &mut PresetFile) -> Outcome {
        let mut out = Outcome::Open;
        egui::Window::new("Convert Sample Type").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    let mut picked = None;
                    egui::ComboBox::from_id_salt("convert-preset").selected_text("Choose…").show_ui(ui, |ui| {
                        for p in presets.all() {
                            if ui.selectable_label(false, &p.name).clicked() {
                                picked = Some(p);
                            }
                        }
                    });
                    if let Some(p) = picked {
                        self.load(&p.settings);
                        self.preset_name = p.name;
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Sample rate (Hz):");
                    ui.add(egui::TextEdit::singleline(&mut self.rate).desired_width(70.0));
                    egui::ComboBox::from_id_salt("convert-rate").selected_text("Common").show_ui(ui, |ui| {
                        for r in RATES {
                            if ui.selectable_label(self.rate == r.to_string(), format!("{r}")).clicked() {
                                self.rate = r.to_string();
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Resample quality:");
                    egui::ComboBox::from_id_salt("convert-quality").selected_text(self.quality.name()).show_ui(ui, |ui| {
                        for q in Quality::ALL {
                            ui.selectable_value(&mut self.quality, q, q.name());
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Channels:");
                    ui.selectable_value(&mut self.chans, Chans::Keep, format!("Keep ({})", self.src_channels));
                    ui.selectable_value(&mut self.chans, Chans::Mono, "Mono");
                    ui.add_enabled_ui(self.src_channels == 1, |ui| ui.selectable_value(&mut self.chans, Chans::Stereo, "Stereo (copy)"));
                });
                if self.chans == Chans::Mono && self.src_channels > 1 {
                    for (i, pct) in self.mix.iter_mut().enumerate() {
                        let name = match (self.src_channels, i) {
                            (2, 0) => "Left".to_string(),
                            (2, _) => "Right".to_string(),
                            _ => format!("Channel {}", i + 1),
                        };
                        ui.horizontal(|ui| {
                            ui.label(format!("  {name} into the mix (%):"));
                            ui.add(egui::DragValue::new(pct).range(0.0..=200.0).speed(1.0));
                        });
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Bit depth:");
                    ui.selectable_value(&mut self.bits, Bits::Keep, "Keep");
                    ui.selectable_value(&mut self.bits, Bits::B16, "16-bit");
                    ui.selectable_value(&mut self.bits, Bits::B24, "24-bit");
                    ui.selectable_value(&mut self.bits, Bits::F32, "32-bit float");
                });
                ui.horizontal(|ui| {
                    ui.label("Dither:");
                    egui::ComboBox::from_id_salt("convert-dither").selected_text(self.dither.dither.name()).show_ui(ui, |ui| {
                        for d in DitherType::ALL {
                            ui.selectable_value(&mut self.dither.dither, d, d.name());
                        }
                    });
                    ui.label("Noise shaping:");
                    egui::ComboBox::from_id_salt("convert-shaping").selected_text(self.dither.shaping.name()).show_ui(ui, |ui| {
                        for sh in Shaping::ALL {
                            ui.selectable_value(&mut self.dither.shaping, sh, sh.name());
                        }
                    });
                });
                ui.label("The depth and dither are used when the file is saved to 16 or 24 bits. Dither is off unless you choose it.");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Preset name:");
                    ui.add(egui::TextEdit::singleline(&mut self.preset_name).desired_width(150.0));
                    if ui.button("Save preset").clicked() {
                        self.message = Some(match self.settings().and_then(|s| presets.save(&self.preset_name, s)) {
                            Ok(()) => format!("Saved preset {:?}", self.preset_name.trim()),
                            Err(e) => e,
                        });
                    }
                    let is_user = presets.user().iter().any(|p| p.name.eq_ignore_ascii_case(self.preset_name.trim()));
                    if ui.add_enabled(is_user, egui::Button::new("Delete preset")).clicked() {
                        self.message = Some(match presets.delete(&self.preset_name) {
                            Ok(()) => format!("Deleted preset {:?}", self.preset_name.trim()),
                            Err(e) => e,
                        });
                    }
                });
                if let Some(m) = &self.message {
                    ui.label(m);
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        match self.settings() {
                            Ok(s) => out = Outcome::Apply(s),
                            Err(e) => self.message = Some(e),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        out = Outcome::Cancel;
                    }
                });
            },
        );
        out
    }
}

/// Edit › Interpret Sample Rate…: the rate to relabel the file as, in Hz.
/// `Some(Some(text))` when OK is pressed, `Some(None)` for Cancel.
pub fn interpret_dialog(ctx: &egui::Context, text: &mut String, current: u32) -> Option<Option<String>> {
    let mut out = None;
    egui::Window::new("Interpret Sample Rate").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
        ctx,
        |ui| {
            ui.label(format!("The file is read as {current} Hz. Samples are not changed: the audio plays faster or slower."));
            ui.horizontal(|ui| {
                ui.label("Interpret as (Hz):");
                ui.add(egui::TextEdit::singleline(text).desired_width(80.0));
                egui::ComboBox::from_id_salt("interpret-rate").selected_text("Common").show_ui(ui, |ui| {
                    for r in RATES {
                        if ui.selectable_label(false, format!("{r}")).clicked() {
                            *text = r.to_string();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    out = Some(Some(text.clone()));
                }
                if ui.button("Cancel").clicked() {
                    out = Some(None);
                }
            });
        },
    );
    out
}
