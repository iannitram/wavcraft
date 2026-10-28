//! Edit › Keyboard Shortcuts…: a searchable list of commands with their chords. Select a command,
//! press "Assign", then the new chord; a chord another command has asks first (reassign or cancel).
//! The dialog only reports what the user chose ([`Change`]); the app applies and saves it.

use egui::Key;
use wavcraft_core::keyboard::{ANSI_ROWS, Mods};
use wavcraft_core::shortcut_sets::DEFAULT_SET;
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};

/// What the user chose in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Give `command` the chord; `reassign` takes it from the command that has it.
    Assign { command: Command, chord: Shortcut, reassign: bool },
    /// Remove all of the command's chords.
    Clear(Command),
    /// Restore Defaults.
    Reset,
    /// Save the keys as a named set, and use it.
    SaveSet(String),
    /// Use a saved set (`default` is the shipped table).
    SwitchSet(String),
    /// Delete a saved set.
    DeleteSet(String),
}

/// A chord that needs the user's answer: it is another command's now.
struct Pending {
    command: Command,
    chord: Shortcut,
    holder: Command,
}

#[derive(Default)]
pub struct ShortcutDialog {
    filter: String,
    selected: Option<Command>,
    /// Waiting for the chord to assign to `selected`.
    capturing: bool,
    pending: Option<Pending>,
    message: Option<String>,
    /// The modifiers the drawn keyboard shows bindings for.
    mods: Mods,
    /// A key clicked on the drawn keyboard: the list shows only the commands on it.
    key_filter: Option<&'static str>,
    /// The name typed for Save As.
    set_name: String,
}

/// Size of one key unit on the drawn keyboard, in points.
const UNIT: f32 = 27.0;

impl ShortcutDialog {
    /// Whether the next key press is meant for the dialog (the app must not run it as a command).
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// Draw the dialog. Returns whether it stays open, and what the user chose this frame.
    pub fn show(&mut self, ctx: &egui::Context, keymap: &Keymap, mac: bool, sets: &[String], active: Option<&str>) -> (bool, Option<Change>) {
        let mut open = true;
        let mut change = None;
        if self.capturing {
            change = self.capture(ctx, keymap);
        }
        egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
            if change.is_none() {
                change = self.set_row(ui, sets, active);
            }
            ui.separator();
            self.keyboard(ui, keymap, mac);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut self.filter);
            });
            let needle = self.filter.trim().to_lowercase();
            let on_key = self.key_filter.map(|k| keymap.commands_on_key(k));
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("shortcut_list").striped(true).num_columns(2).min_col_width(200.0).show(ui, |ui| {
                    for command in Command::all() {
                        let chords: Vec<String> = keymap.chords(command).map(|c| c.text(mac)).collect();
                        let shown = if chords.is_empty() { "—".to_string() } else { chords.join(", ") };
                        let hay = format!("{} {} {}", command.label(), command.name(), shown).to_lowercase();
                        if !needle.is_empty() && !hay.contains(&needle) {
                            continue;
                        }
                        if on_key.as_ref().is_some_and(|list| !list.contains(&command)) {
                            continue;
                        }
                        if ui.selectable_label(self.selected == Some(command), command.label()).clicked() {
                            self.selected = Some(command);
                            self.capturing = false;
                            self.pending = None;
                            self.message = None;
                        }
                        ui.label(shown);
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            if let Some((p_command, p_chord, p_holder)) = self.pending.as_ref().map(|p| (p.command, p.chord, p.holder)) {
                ui.colored_label(
                    egui::Color32::from_rgb(235, 180, 70),
                    format!("{} is already assigned to {}.", p_chord.text(mac), p_holder.label()),
                );
                ui.horizontal(|ui| {
                    if ui.button(format!("Reassign to {}", p_command.label())).clicked() {
                        change = Some(Change::Assign { command: p_command, chord: p_chord, reassign: true });
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending = None;
                    }
                });
            } else if self.capturing {
                ui.label("Press the new key combination (Esc cancels)…");
            } else {
                ui.horizontal(|ui| {
                    let selected = self.selected;
                    if ui.add_enabled(selected.is_some(), egui::Button::new("Assign…")).on_hover_text("Then press the new key combination").clicked()
                    {
                        self.capturing = true;
                        self.message = None;
                    }
                    if ui.add_enabled(selected.is_some(), egui::Button::new("Clear")).clicked()
                        && let Some(c) = selected
                    {
                        change = Some(Change::Clear(c));
                    }
                    if ui.button("Restore Defaults").clicked() {
                        change = Some(Change::Reset);
                    }
                });
            }
            if let Some(m) = &self.message {
                ui.label(m);
            }
        });
        match change {
            Some(Change::Assign { .. }) => {
                self.pending = None;
                self.capturing = false;
            }
            Some(_) => {
                self.pending = None;
                self.message = None;
            }
            None => {}
        }
        (open, change)
    }

    /// The shortcut sets: pick one, save the current keys under a name, delete the one in use.
    fn set_row(&mut self, ui: &mut egui::Ui, sets: &[String], active: Option<&str>) -> Option<Change> {
        let mut change = None;
        ui.horizontal(|ui| {
            ui.label("Set:");
            egui::ComboBox::from_id_salt("shortcut_set").selected_text(active.unwrap_or(DEFAULT_SET)).show_ui(ui, |ui| {
                if ui.selectable_label(active.is_none(), DEFAULT_SET).clicked() {
                    change = Some(Change::SwitchSet(DEFAULT_SET.to_string()));
                }
                for name in sets {
                    if ui.selectable_label(active == Some(name.as_str()), name).clicked() {
                        change = Some(Change::SwitchSet(name.clone()));
                    }
                }
            });
            if ui.add_enabled(active.is_some(), egui::Button::new("Delete")).on_hover_text("Delete this set (the default can't be deleted)").clicked()
                && let Some(name) = active
            {
                change = Some(Change::DeleteSet(name.to_string()));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Save current keys as:");
            ui.add(egui::TextEdit::singleline(&mut self.set_name).desired_width(140.0));
            if ui.add_enabled(!self.set_name.trim().is_empty(), egui::Button::new("Save As")).clicked() {
                change = Some(Change::SaveSet(self.set_name.trim().to_string()));
            }
        });
        change
    }

    /// The drawn keyboard: keys with a binding for the chosen modifiers are coloured, hovering
    /// shows the command, clicking filters the list below to the commands on that key.
    fn keyboard(&mut self, ui: &mut egui::Ui, keymap: &Keymap, mac: bool) {
        ui.horizontal(|ui| {
            ui.label("Modifiers:");
            ui.checkbox(&mut self.mods.cmd, if mac { "Cmd" } else { "Ctrl" });
            ui.checkbox(&mut self.mods.shift, "Shift");
            ui.checkbox(&mut self.mods.alt, if mac { "Option" } else { "Alt" });
            if let Some(k) = self.key_filter
                && ui.button(format!("Showing key {k}  ✕")).clicked()
            {
                self.key_filter = None;
            }
        });
        let bound = keymap.keys_with(self.mods);
        let (fill, other, outline) = (egui::Color32::from_rgb(52, 120, 190), egui::Color32::from_gray(70), egui::Color32::from_rgb(235, 180, 70));
        let width = ANSI_ROWS.iter().map(|r| r.iter().map(|k| k.width).sum::<f32>()).fold(0.0, f32::max) * UNIT;
        let (area, _) = ui.allocate_exact_size(egui::vec2(width, ANSI_ROWS.len() as f32 * UNIT), egui::Sense::hover());
        for (row, caps) in ANSI_ROWS.iter().enumerate() {
            let mut x = area.left();
            for cap in caps.iter() {
                let rect = egui::Rect::from_min_size(egui::pos2(x, area.top() + row as f32 * UNIT), egui::vec2(cap.width * UNIT, UNIT)).shrink(1.5);
                x += cap.width * UNIT;
                let Some(name) = cap.name else {
                    if !cap.label.is_empty() {
                        ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(1.0, egui::Color32::from_gray(60)), egui::StrokeKind::Inside);
                    }
                    continue;
                };
                let commands: Vec<Command> = bound.iter().filter(|(k, _)| *k == name).map(|(_, c)| *c).collect();
                let anywhere = !keymap.commands_on_key(name).is_empty();
                let id = ui.id().with(("key", name));
                let response = ui.interact(rect, id, egui::Sense::click());
                let colour = if !commands.is_empty() {
                    fill
                } else if anywhere {
                    other
                } else {
                    egui::Color32::from_gray(40)
                };
                let stroke = if self.key_filter == Some(name) {
                    egui::Stroke::new(2.0, outline)
                } else {
                    egui::Stroke::new(1.0, egui::Color32::from_gray(90))
                };
                ui.painter().rect(rect, 3.0, colour, stroke, egui::StrokeKind::Inside);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, cap.label, egui::FontId::proportional(11.0), egui::Color32::WHITE);
                if response.clicked() {
                    self.key_filter = if self.key_filter == Some(name) { None } else { Some(name) };
                }
                let tip = if commands.is_empty() {
                    format!("{name}: nothing is bound with these modifiers")
                } else {
                    format!("{name}: {}", commands.iter().map(|c| c.label()).collect::<Vec<_>>().join(", "))
                };
                response.on_hover_text(tip);
            }
        }
    }

    /// Read the chord pressed this frame (while waiting for one).
    fn capture(&mut self, ctx: &egui::Context, keymap: &Keymap) -> Option<Change> {
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let (key, mods) = pressed?;
        if key == Key::Escape {
            self.capturing = false;
            return None;
        }
        let command = self.selected?;
        let Some(chord) = Shortcut::from_parts(mods.command, mods.shift, mods.alt, key.name()) else {
            self.message = Some(format!("{} can't be used in a shortcut.", key.name()));
            self.capturing = false;
            return None;
        };
        self.capturing = false;
        match keymap.resolve(chord).filter(|h| *h != command) {
            Some(holder) => {
                self.pending = Some(Pending { command, chord, holder });
                None
            }
            None => Some(Change::Assign { command, chord, reassign: false }),
        }
    }
}
