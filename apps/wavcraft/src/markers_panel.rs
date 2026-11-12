//! The Markers panel (A172-A175, A181): a table of the markers and ranges. A click picks a row,
//! a double-click goes to it (makes it the time selection), a click on the only picked row
//! renames it; plus the state for dragging a marker's handle in the ruler.
//!
//! Kept out of `app.rs` so that file only gains the few lines that call it.

use crate::theme;
use std::ops::Range;

use egui::{Key, Pos2, Rect};
use wavcraft_core::actions::Editor;
use wavcraft_core::settings::{MarkerColumns, Settings};
use wavcraft_core::{Marker, MarkerPick, Markers, Viewport, format_time};

/// How close (in pixels) to a marker's flag a press grabs it.
const GRAB_PX: f32 = 6.0;

/// Which of the panel's buttons are enabled for the picked rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PickButtons {
    pub delete: bool,
    pub merge: bool,
    pub to_range: bool,
    pub to_point: bool,
}

/// The buttons enabled for the picked markers: Delete for one or more, Merge for two or more,
/// To Range for one point, To Point for one range. Nothing picked enables none.
pub fn buttons_for(markers: &Markers, pick: &MarkerPick) -> PickButtons {
    let picked: Vec<&Marker> = markers.iter().filter(|m| pick.ids.contains(&m.id)).collect();
    let one = match picked[..] {
        [m] => Some(m),
        _ => None,
    };
    PickButtons {
        delete: !picked.is_empty(),
        merge: picked.len() >= 2,
        to_range: one.is_some_and(Marker::is_point),
        to_point: one.is_some_and(|m| !m.is_point()),
    }
}

/// A marker being dragged in the ruler: the move is applied (one history step) on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drag {
    pub id: u64,
    /// Frames from the marker's start to where it was grabbed.
    offset: i64,
    /// Where the marker would start now.
    pub start: u64,
}

/// What the panel changed that the window must act on.
#[derive(Default)]
pub struct PanelOut {
    /// A double-click went to markers: make this the time selection (a point is the cursor).
    pub select: Option<Range<u64>>,
    /// The columns changed: save the settings.
    pub columns_changed: bool,
}

#[derive(Default)]
pub struct MarkersUi {
    pub pick: MarkerPick,
    /// The marker whose name is being typed, and the text so far.
    renaming: Option<(u64, String)>,
    focus_rename: bool,
    /// When a click opened the rename (input time), so the second half of a double-click that
    /// lands in the new editor still goes to the row.
    rename_opened: Option<f64>,
    /// The description being typed for the one picked marker: (id, text so far).
    desc_edit: Option<(u64, String)>,
    pub drag: Option<Drag>,
}

impl MarkersUi {
    pub fn new() -> MarkersUi {
        MarkersUi::default()
    }

    /// Go to row `index` (the MARKER select control verb, and a double-click in the panel): picks,
    /// and returns the selection it makes.
    pub fn click(&mut self, markers: &Markers, index: usize, shift: bool, ctrl: bool) -> Option<Range<u64>> {
        self.pick.click(markers, index, shift, ctrl);
        self.pick.span(markers)
    }

    /// A click on a cell of row `index` in the panel. A double-click goes to the row: picks it and
    /// returns the selection it makes, and starts no rename. A single click only picks, and
    /// returns `None`; a plain single click on the row that is already the only picked one starts
    /// renaming it instead (the Name is the one cell edited in the table).
    pub fn row_click(&mut self, markers: &Markers, index: usize, shift: bool, ctrl: bool, double: bool) -> Option<Range<u64>> {
        let id = markers.id_at(index)?;
        if double {
            // The first click of a double-click on the picked row may have started a rename.
            if self.renaming.as_ref().is_some_and(|(r, _)| *r == id) {
                self.renaming = None;
            }
            return self.click(markers, index, shift, ctrl);
        }
        if !shift && !ctrl && self.pick.ids == [id] {
            let name = markers.iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default();
            self.renaming = Some((id, name));
            self.focus_rename = true;
            return None;
        }
        self.renaming = None;
        self.pick.click(markers, index, shift, ctrl);
        None
    }

    /// The marker whose flag is under `pos` in the ruler, if any.
    pub fn handle_at(&self, markers: &Markers, view: &Viewport, ruler: Rect, pos: Pos2) -> Option<u64> {
        if !ruler.contains(pos) {
            return None;
        }
        markers.iter().rev().find(|m| (ruler.left() + view.x_of(m.start as f64) - pos.x).abs() <= GRAB_PX).map(|m| m.id)
    }

    /// Start dragging marker `id`, grabbed at `frame`.
    pub fn start_drag(&mut self, markers: &Markers, id: u64, frame: u64) {
        if let Some(m) = markers.iter().find(|m| m.id == id) {
            self.drag = Some(Drag { id, offset: frame as i64 - m.start as i64, start: m.start });
        }
    }

    /// The pointer is at `frame`.
    pub fn drag_to(&mut self, frame: u64) {
        if let Some(d) = self.drag.as_mut() {
            d.start = (frame as i64 - d.offset).max(0) as u64;
        }
    }

    /// The markers as drawn: with the dragged one where it would land.
    pub fn shown(&self, markers: &Markers, len: u64) -> Markers {
        match self.drag {
            Some(d) => markers.with_moved(d.id, d.start, len).unwrap_or_else(|| markers.clone()),
            None => markers.clone(),
        }
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, editor: &mut Editor, settings: &mut Settings) -> PanelOut {
        let mut out = PanelOut::default();
        ui.heading("Markers");
        let doc = editor.history.current();
        let (markers, rate) = (doc.markers().clone(), doc.sample_rate());
        ui.horizontal(|ui| {
            ui.label(format!("{} markers", markers.len()));
            ui.menu_button("Columns", |ui| {
                for (i, name) in MarkerColumns::NAMES.iter().enumerate() {
                    let mut on = settings.marker_columns.get(i);
                    if ui.checkbox(&mut on, *name).changed() {
                        settings.marker_columns.set(i, on);
                        out.columns_changed = true;
                    }
                }
            });
        });
        ui.separator();
        let cols = settings.marker_columns;
        let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let time = |frame: u64| format_time(frame as usize, rate);
        let (now, delay) = (ui.input(|i| i.time), ui.ctx().options(|o| o.input_options.max_double_click_delay));
        let mut clicked = None;
        let mut commit: Option<(u64, String)> = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("markers-table").striped(true).num_columns(6).show(ui, |ui| {
                ui.strong("Name");
                for (i, name) in ["Start", "End", "Duration", "Type", "Description"].iter().enumerate() {
                    if cols.get(i) {
                        ui.strong(*name);
                    }
                }
                ui.end_row();
                for (index, m) in markers.iter().enumerate() {
                    let picked = self.pick.ids.contains(&m.id);
                    // A cell's click: None, or Some(double). A triple-click's third click is nothing.
                    let hit = |r: egui::Response| {
                        if r.triple_clicked() {
                            None
                        } else if r.double_clicked() {
                            Some(true)
                        } else if r.clicked() {
                            Some(false)
                        } else {
                            None
                        }
                    };
                    let cell = |ui: &mut egui::Ui, text: String| hit(ui.selectable_label(picked, text));
                    match &mut self.renaming {
                        Some((id, text)) if *id == m.id => {
                            let edit = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                            if self.focus_rename {
                                edit.request_focus();
                                self.focus_rename = false;
                            }
                            // The click that opened this editor, then a second one in it: a double-click.
                            if edit.double_clicked() && self.rename_opened.is_some_and(|t| now - t <= delay) {
                                clicked = Some((index, true));
                            }
                            let (esc, enter) = ui.input(|i| (i.key_pressed(Key::Escape), i.key_pressed(Key::Enter)));
                            if esc {
                                self.renaming = None;
                            } else if enter || edit.lost_focus() {
                                commit = Some((m.id, std::mem::take(text)));
                                self.renaming = None;
                            }
                        }
                        _ => {
                            if let Some(double) = cell(ui, m.name.clone()) {
                                clicked = Some((index, double));
                            }
                        }
                    }
                    let mut take = |c: Option<bool>| {
                        if let Some(double) = c {
                            clicked = Some((index, double));
                        }
                    };
                    if cols.start {
                        take(cell(ui, time(m.start)));
                    }
                    if cols.end {
                        take(cell(ui, time(m.end.max(m.start))));
                    }
                    if cols.duration {
                        take(cell(ui, if m.is_point() { String::new() } else { time(m.end - m.start) }));
                    }
                    if cols.kind {
                        take(cell(ui, if m.is_point() { "Point" } else { "Range" }.to_string()));
                    }
                    if cols.description {
                        take(cell(ui, m.description.clone()));
                    }
                    ui.end_row();
                }
            });
        });
        // The picked markers: Merge (two or more), and one marker's colour and description.
        let picked_ids: Vec<u64> = self.pick.indices(&markers).iter().filter_map(|&i| markers.id_at(i)).collect();
        ui.separator();
        let mut merge = false;
        let mut set_color = None;
        let mut set_desc: Option<(u64, String)> = None;
        let enabled = buttons_for(&markers, &self.pick);
        ui.horizontal(|ui| {
            merge = ui.add_enabled(enabled.merge, egui::Button::new("Merge")).on_hover_text("Make the picked markers one range").clicked();
            if let [id] = picked_ids[..] {
                let current = markers.iter().find(|m| m.id == id).map_or(0, |m| m.color);
                if ui.selectable_label(current == 0, "No colour").clicked() {
                    set_color = Some((id, 0u8));
                }
                for (n, (name, c)) in wavcraft_core::PALETTE.iter().enumerate() {
                    let n = (n + 1) as u8;
                    let swatch = egui::RichText::new(if current == n { "\u{25cf}" } else { "\u{25cb}" }).color(theme::rgb(c[0], c[1], c[2]));
                    if ui.button(swatch).on_hover_text(*name).clicked() {
                        set_color = Some((id, n));
                    }
                }
            }
        });
        if let [id] = picked_ids[..] {
            let stored = markers.iter().find(|m| m.id == id).map(|m| m.description.clone()).unwrap_or_default();
            if self.desc_edit.as_ref().is_none_or(|(i, _)| *i != id) {
                self.desc_edit = Some((id, stored.clone()));
            }
            if let Some((_, text)) = self.desc_edit.as_mut() {
                ui.horizontal(|ui| {
                    ui.label("Description");
                    let edit = ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
                    if edit.lost_focus() && *text != stored {
                        set_desc = Some((id, text.clone()));
                    }
                });
            }
        } else {
            self.desc_edit = None;
        }
        if merge {
            editor.merge_markers(&picked_ids);
            self.pick.ids.retain(|i| editor.history.current().markers().index_of(*i).is_some());
        }
        if let Some((id, n)) = set_color {
            editor.set_marker_color(id, n);
        }
        if let Some((id, text)) = set_desc {
            editor.set_marker_description(id, &text);
            self.desc_edit = None;
        }
        if let Some((index, double)) = clicked {
            out.select = self.row_click(&markers, index, shift, ctrl, double);
            self.rename_opened = (self.renaming.is_some() && self.focus_rename).then_some(now);
        }
        if let Some((id, name)) = commit {
            editor.rename_marker(id, &name);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn a_press_near_a_flag_grabs_it_and_dragging_keeps_the_grab_offset() {
        let m = Markers::from_items(vec![("a".into(), 1000, 1000), ("b".into(), 5000, 5400)]);
        let view = Viewport { start: 0.0, frames_per_px: 10.0 };
        let ruler = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 20.0));
        let mut ui = MarkersUi::new();
        let b = m.id_at(1).unwrap();
        assert_eq!(ui.handle_at(&m, &view, ruler, pos2(503.0, 5.0)), Some(b), "3 px from 5000/10");
        assert_eq!(ui.handle_at(&m, &view, ruler, pos2(520.0, 5.0)), None);
        assert_eq!(ui.handle_at(&m, &view, ruler, pos2(503.0, 40.0)), None, "below the ruler");
        ui.start_drag(&m, b, 5030);
        ui.drag_to(6030);
        assert_eq!(ui.drag.unwrap().start, 6000);
        assert_eq!(ui.shown(&m, 10_000).as_slice()[1].range(), 6000..6400);
        ui.drag_to(0);
        assert_eq!(ui.drag.unwrap().start, 0, "held at the start of the file");
    }

    #[test]
    fn buttons_follow_the_picked_rows() {
        let m = Markers::from_items(vec![("p".into(), 1000, 1000), ("r".into(), 5000, 6000)]);
        let none = PickButtons::default();
        assert_eq!(buttons_for(&m, &MarkerPick::default()), none, "nothing picked: all false");

        let mut pick = MarkerPick::default();
        pick.click(&m, 0, false, false);
        assert_eq!(
            buttons_for(&m, &pick),
            PickButtons { delete: true, merge: false, to_range: true, to_point: false },
            "one point: delete and to_range"
        );

        pick.click(&m, 1, false, false);
        assert_eq!(
            buttons_for(&m, &pick),
            PickButtons { delete: true, merge: false, to_range: false, to_point: true },
            "one range: delete and to_point"
        );

        pick.click(&m, 0, false, true);
        assert_eq!(
            buttons_for(&m, &pick),
            PickButtons { delete: true, merge: true, to_range: false, to_point: false },
            "a point and a range: delete and merge only"
        );
    }

    #[test]
    fn a_single_click_picks_and_a_double_click_goes() {
        let m = Markers::from_items(vec![("a".into(), 1000, 1000), ("b".into(), 5000, 5400)]);
        let mut ui = MarkersUi::new();
        let (a, b) = (m.id_at(0).unwrap(), m.id_at(1).unwrap());
        assert_eq!(ui.row_click(&m, 1, false, false, false), None, "a single click only picks");
        assert_eq!(ui.pick.ids, vec![b]);
        assert_eq!(ui.renaming, None);
        assert_eq!(ui.row_click(&m, 1, false, false, true), Some(5000..5400), "a double click goes");
        assert_eq!(ui.pick.ids, vec![b]);
        assert_eq!(ui.row_click(&m, 0, false, false, true), Some(1000..1000));
        assert_eq!(ui.pick.ids, vec![a]);
        assert_eq!(ui.renaming, None, "a double click starts no rename");
        assert_eq!(ui.row_click(&m, 0, false, false, false), None, "a click on the only picked row edits it");
        assert_eq!(ui.renaming, Some((a, "a".to_string())));
        assert_eq!(ui.row_click(&m, 1, false, false, false), None);
        assert_eq!(ui.pick.ids, vec![b]);
        assert_eq!(ui.renaming, None, "picking another row ends the edit");
    }
}
