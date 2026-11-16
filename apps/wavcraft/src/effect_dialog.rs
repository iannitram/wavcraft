//! The generic effect dialog (f14, B006): draws any effect's parameters from its
//! [`ParamDesc`] list, with OK and Cancel. It only edits the values; the app applies them.

use egui::{Align2, Key, vec2};
use wavcraft_core::effects::{self, Effect, ParamDesc, ParamKind};
use wavcraft_core::generate;
use wavcraft_core::presets::Presets;

/// What the user did with the dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Ok,
    Cancel,
    /// A generator's Preview button (f21): play a short sample of the current settings.
    Preview,
    /// An effect's Preview button: start or stop the live preview (E6.2, E6.3).
    TogglePreview,
}

/// An open dialog: which effect (its index in [`effects::all`]) and the values being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDialog {
    /// Index into [`effects::all`], or into [`generate::all`] when `generator`.
    pub index: u16,
    pub values: Vec<f64>,
    /// A Generate dialog (f21): same controls, plus a Preview button.
    pub generator: bool,
    /// Opened from the Effects Rack for this slot: OK stores the settings there instead of
    /// running the effect (f31).
    pub rack_slot: Option<usize>,
    /// True while a live preview runs; the app sets it each frame. The Preview button then
    /// reads Stop Preview.
    pub preview_on: bool,
    /// The values last reported through [`EffectDialog::take_changed_values`].
    sent: Vec<f64>,
    /// The name typed in the Presets menu for saving.
    preset_name: String,
    /// What the last preset action said.
    note: String,
}

/// A click in the Presets menu, run after the window is drawn.
enum PresetAction {
    Save,
    Load(String),
    Delete(String),
}

const NO_PRESET_FOLDER: &str = "there is no config folder for presets on this machine";

impl EffectDialog {
    /// Open for the effect at `index`, starting from `values` when they suit it, else the
    /// defaults.
    pub fn new(index: u16, values: Option<Vec<f64>>) -> Option<EffectDialog> {
        let effect = effects::all().get(usize::from(index))?;
        let values = values.filter(|v| effects::validate(*effect, v).is_ok()).unwrap_or_else(|| effects::Params::defaults(*effect));
        Some(EffectDialog {
            index,
            sent: values.clone(),
            values,
            generator: false,
            rack_slot: None,
            preview_on: false,
            preset_name: String::new(),
            note: String::new(),
        })
    }

    /// Open for the generator at `index` of [`generate::all`] (f21).
    pub fn new_generator(index: u16, values: Option<Vec<f64>>) -> Option<EffectDialog> {
        let g = generate::all().get(usize::from(index))?;
        let values = values.filter(|v| generate::validate(*g, v).is_ok()).unwrap_or_else(|| generate::defaults(*g));
        Some(EffectDialog {
            index,
            sent: values.clone(),
            values,
            generator: true,
            rack_slot: None,
            preview_on: false,
            preset_name: String::new(),
            note: String::new(),
        })
    }

    /// The effect being edited. Panics for a generator dialog; see [`EffectDialog::id`].
    pub fn effect(&self) -> &'static dyn Effect {
        effects::all()[usize::from(self.index)]
    }

    /// The effect's or generator's id.
    pub fn id(&self) -> &'static str {
        if self.generator { generate::all()[usize::from(self.index)].id() } else { self.effect().id() }
    }

    fn name(&self) -> &'static str {
        if self.generator { generate::all()[usize::from(self.index)].name() } else { self.effect().name() }
    }

    fn descs(&self) -> &'static [ParamDesc] {
        if self.generator { generate::all()[usize::from(self.index)].parameters() } else { self.effect().parameters() }
    }

    /// Back to the defaults.
    pub fn reset(&mut self) {
        self.values = self.descs().iter().map(|p| p.default).collect();
    }

    /// Whether the dialog offers the live Preview button. Every effect does; a generator keeps
    /// its own one-shot Preview button instead.
    pub fn offers_preview(&self) -> bool {
        !self.generator
    }

    /// The values to send to the preview worker, when they differ from those last sent. Returns
    /// `None` on an unchanged frame: one message per frame in which the values changed; an
    /// unchanged frame gives none.
    pub fn take_changed_values(&mut self) -> Option<Vec<f64>> {
        if self.values == self.sent {
            return None;
        }
        self.sent = self.values.clone();
        Some(self.sent.clone())
    }

    /// Set the name the Presets menu saves under (what the user types there); the tests use it.
    #[cfg(test)]
    pub fn set_preset_name(&mut self, name: &str) {
        self.preset_name = name.to_string();
    }

    /// What the last preset action said; empty when there has been none.
    #[cfg(test)]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// Save the current values as the preset named in the menu's text box.
    pub fn save_preset(&mut self, presets: Option<&Presets>) {
        if self.generator {
            self.note = "presets are for effects, not generators".to_string();
            return;
        }
        let Some(presets) = presets else {
            self.note = NO_PRESET_FOLDER.to_string();
            return;
        };
        self.note = match presets.save_effect(self.id(), &self.preset_name, &self.values) {
            Ok(()) => format!("saved {} preset {:?}", self.name(), self.preset_name),
            Err(why) => why,
        };
    }

    /// Load the preset `name` into the dialog. A preset that does not check out leaves the
    /// values as they were and says why in the note.
    pub fn load_preset(&mut self, presets: Option<&Presets>, name: &str) {
        if self.generator {
            self.note = "presets are for effects, not generators".to_string();
            return;
        }
        let Some(presets) = presets else {
            self.note = NO_PRESET_FOLDER.to_string();
            return;
        };
        let loaded = presets.load_effect(self.id(), name).and_then(|values| effects::validate(self.effect(), &values).map(|()| values));
        self.note = match loaded {
            Ok(values) => {
                self.values = values;
                format!("loaded {} preset {name:?}", self.name())
            }
            Err(why) => why,
        };
    }

    /// Delete the preset `name` for this effect.
    pub fn delete_preset(&mut self, presets: Option<&Presets>, name: &str) {
        if self.generator {
            self.note = "presets are for effects, not generators".to_string();
            return;
        }
        let Some(presets) = presets else {
            self.note = NO_PRESET_FOLDER.to_string();
            return;
        };
        self.note = match presets.delete(self.id(), name) {
            Ok(()) => format!("deleted preset {name:?}"),
            Err(why) => why,
        };
    }

    /// Draw the dialog; `Some` when OK / Enter or Cancel / Escape was chosen. `presets` is the
    /// folder the Presets menu reads and writes (None when there is no config folder).
    pub fn show(&mut self, ctx: &egui::Context, presets: Option<&Presets>) -> Option<Answer> {
        let (title, descs) = (self.name(), self.descs());
        let id = self.id();
        let mut answer = None;
        let mut preset_action = None;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("effect_params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                for (desc, value) in descs.iter().zip(self.values.iter_mut()) {
                    ui.label(desc.label);
                    control(ui, desc, value);
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    answer = Some(Answer::Ok);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
                if ui.button("Reset").clicked() {
                    self.reset();
                }
                if self.generator && ui.button("Preview").clicked() {
                    answer = Some(Answer::Preview);
                }
                if self.offers_preview() && ui.button(if self.preview_on { "Stop Preview" } else { "Preview" }).clicked() {
                    answer = Some(Answer::TogglePreview);
                }
                if !self.generator {
                    ui.menu_button("Presets", |ui| {
                        ui.text_edit_singleline(&mut self.preset_name);
                        if ui.button("Save as the name above").clicked() {
                            preset_action = Some(PresetAction::Save);
                            ui.close();
                        }
                        ui.separator();
                        for name in presets.and_then(|p| p.list(id).ok()).unwrap_or_default() {
                            ui.horizontal(|ui| {
                                if ui.button(&name).clicked() {
                                    preset_action = Some(PresetAction::Load(name.clone()));
                                    ui.close();
                                }
                                if ui.small_button("Delete").clicked() {
                                    preset_action = Some(PresetAction::Delete(name.clone()));
                                    ui.close();
                                }
                            });
                        }
                    });
                }
            });
            if !self.note.is_empty() {
                ui.label(&self.note);
            }
        });
        match preset_action {
            Some(PresetAction::Save) => self.save_preset(presets),
            Some(PresetAction::Load(name)) => self.load_preset(presets, &name),
            Some(PresetAction::Delete(name)) => self.delete_preset(presets, &name),
            None => {}
        }
        if answer.is_none() {
            if ctx.input(|i| i.key_pressed(Key::Enter)) {
                answer = Some(Answer::Ok);
            } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
                answer = Some(Answer::Cancel);
            }
        }
        answer
    }
}

/// The control for one setting, by kind.
fn control(ui: &mut egui::Ui, desc: &ParamDesc, value: &mut f64) {
    match desc.kind {
        ParamKind::Choice => {
            let selected = desc.choices.get(*value as usize).copied().unwrap_or("");
            egui::ComboBox::from_id_salt(desc.id).selected_text(selected).show_ui(ui, |ui| {
                for (n, name) in desc.choices.iter().enumerate() {
                    ui.selectable_value(value, n as f64, *name);
                }
            });
        }
        ParamKind::Db | ParamKind::Percent | ParamKind::Time | ParamKind::Frequency | ParamKind::Number => {
            let mut slider = egui::Slider::new(value, desc.min..=desc.max).clamping(egui::SliderClamping::Always);
            if matches!(desc.kind, ParamKind::Number | ParamKind::Frequency) && desc.min > 0.0 {
                slider = slider.logarithmic(true);
            }
            if !desc.unit.is_empty() {
                slider = slider.suffix(format!(" {}", desc.unit));
            }
            ui.add(slider);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_index() -> u16 {
        effects::all().iter().position(|e| e.id() == "gain").unwrap() as u16
    }

    #[test]
    fn opens_with_defaults_or_with_suitable_values() {
        let d = EffectDialog::new(gain_index(), None).unwrap();
        assert_eq!(d.values, vec![0.0]);
        let d = EffectDialog::new(gain_index(), Some(vec![-6.0])).unwrap();
        assert_eq!(d.values, vec![-6.0]);
        let d = EffectDialog::new(gain_index(), Some(vec![999.0])).unwrap();
        assert_eq!(d.values, vec![0.0], "values outside the range fall back to the defaults");
        assert!(EffectDialog::new(999, None).is_none());
    }

    #[test]
    fn a_generator_dialog_opens_on_its_defaults_and_says_so() {
        let d = EffectDialog::new_generator(0, None).unwrap();
        assert!(d.generator);
        assert_eq!(d.id(), "tone");
        assert_eq!(d.values, generate::defaults(generate::all()[0]));
        let mut d = EffectDialog::new_generator(1, Some(vec![1.0, -20.0, 2.0])).unwrap();
        assert_eq!(d.id(), "noise");
        assert_eq!(d.values, vec![1.0, -20.0, 2.0]);
        d.reset();
        assert_eq!(d.values, generate::defaults(generate::all()[1]));
        assert!(EffectDialog::new_generator(99, None).is_none());
    }

    #[test]
    fn reset_restores_the_defaults() {
        let mut d = EffectDialog::new(gain_index(), Some(vec![-6.0])).unwrap();
        d.reset();
        assert_eq!(d.values, vec![0.0]);
    }

    #[test]
    fn e6_every_effect_dialog_offers_preview() {
        for i in 0..effects::all().len() {
            let d = EffectDialog::new(i as u16, None).unwrap();
            assert!(d.offers_preview(), "effect {} (index {i}) should offer Preview", d.id());
        }
        assert!(!EffectDialog::new_generator(0, None).unwrap().offers_preview());
    }

    #[test]
    fn e6_a_changed_setting_sends_exactly_one_message() {
        let mut d = EffectDialog::new(gain_index(), None).unwrap();
        assert_eq!(d.take_changed_values(), None, "an unchanged dialog sends nothing");
        d.values = vec![-6.0];
        assert_eq!(d.take_changed_values(), Some(vec![-6.0]));
        assert_eq!(d.take_changed_values(), None, "the same values are not sent twice");
        d.reset();
        assert_eq!(d.take_changed_values(), Some(vec![0.0]));
        assert_eq!(d.take_changed_values(), None);
    }
}
