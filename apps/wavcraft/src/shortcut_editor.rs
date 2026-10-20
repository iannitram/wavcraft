//! Edit › Keyboard Shortcuts…: a searchable list of commands with their chords. Select a command,
//! press "Assign", then the new chord; a chord another command has asks first (reassign or cancel).
//! The dialog only reports what the user chose ([`Change`]); the app applies and saves it.

use egui::Key;
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};

/// What the user chose in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Give `command` the chord; `reassign` takes it from the command that has it.
    Assign { command: Command, chord: Shortcut, reassign: bool },
    /// Remove all of the command's chords.
    Clear(Command),
    /// Restore Defaults.
    Reset,
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
}

impl ShortcutDialog {
    /// Whether the next key press is meant for the dialog (the app must not run it as a command).
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// Draw the dialog. Returns whether it stays open, and what the user chose this frame.
    pub fn show(&mut self, ctx: &egui::Context, keymap: &Keymap, mac: bool) -> (bool, Option<Change>) {
        let mut open = true;
        let mut change = None;
        if self.capturing {
            change = self.capture(ctx, keymap);
        }
        egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut self.filter);
            });
            let needle = self.filter.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("shortcut_list").striped(true).num_columns(2).min_col_width(200.0).show(ui, |ui| {
                    for command in Command::all() {
                        let chords: Vec<String> = keymap.chords(command).map(|c| c.text(mac)).collect();
                        let shown = if chords.is_empty() { "—".to_string() } else { chords.join(", ") };
                        let hay = format!("{} {} {}", command.label(), command.name(), shown).to_lowercase();
                        if !needle.is_empty() && !hay.contains(&needle) {
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
