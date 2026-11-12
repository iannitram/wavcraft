//! The Markers panel (A172-A175, A181): a table of the markers and ranges. A click picks a row,
//! a double-click goes to its span, and a click on the name of the only picked row renames it.
//! Also the state for dragging a marker's handle in the ruler.
//!
//! Kept out of `app.rs` so that file only gains the few lines that call it.

use crate::theme;
use std::ops::Range;

use egui::{Key, Pos2, Rect};
use wavcraft_core::actions::Editor;
use wavcraft_core::settings::{MarkerColumns, Settings};
use wavcraft_core::{MarkerPick, Markers, Viewport, format_time};

/// How close (in pixels) to a marker's flag a press grabs it.
const GRAB_PX: f32 = 6.0;

/// The click a widget got this frame, if any: a double-click, else a single click.
fn click_of(r: &egui::Response, name: bool) -> Option<Click> {
    if r.double_clicked() {
        Some(Click::Double)
    } else if r.clicked() {
        Some(Click::Single { name })
    } else {
        None
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

/// One mouse click on a row of the panel: a single click, or a double-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// `name` is set when the click was on the row's name cell (the cell that renames).
    Single {
        name: bool,
    },
    Double,
}

/// What the panel changed that the window must act on.
#[derive(Default)]
pub struct PanelOut {
    /// A click picked markers: make this the time selection (a point is the cursor).
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
    /// The description being typed for the one picked marker: (id, text so far).
    desc_edit: Option<(u64, String)>,
    pub drag: Option<Drag>,
}

impl MarkersUi {
    pub fn new() -> MarkersUi {
        MarkersUi::default()
    }

    /// The go-to path: picks row `index` and returns the selection it makes. The MARKER select
    /// control verb uses it, and so does a double-click in the panel.
    pub fn click(&mut self, markers: &Markers, index: usize, shift: bool, ctrl: bool) -> Option<Range<u64>> {
        self.pick.click(markers, index, shift, ctrl);
        self.pick.span(markers)
    }

    /// A click on row `index` in the panel. A single click only picks the row, and returns no
    /// span. A single click on the name of the row that was already the only picked row (a plain
    /// click, no shift or ctrl) starts renaming it. A double-click goes: see `click`.
    pub fn panel_click(&mut self, markers: &Markers, index: usize, click: Click, shift: bool, ctrl: bool) -> Option<Range<u64>> {
        match click {
            Click::Double => self.click(markers, index, shift, ctrl),
            Click::Single { name } => {
                let id = markers.id_at(index)?;
                let only_picked = self.pick.ids == [id];
                self.pick.click(markers, index, shift, ctrl);
                if name
                    && only_picked
                    && !shift
                    && !ctrl
                    && let Some(m) = markers.iter().find(|m| m.id == id)
                {
                    self.renaming = Some((id, m.name.clone()));
                    self.focus_rename = true;
                }
                None
            }
        }
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
        let mut clicked: Option<(usize, Click)> = None;
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
                    let cell = |ui: &mut egui::Ui, text: String| ui.selectable_label(picked, text);
                    match &mut self.renaming {
                        Some((id, text)) if *id == m.id => {
                            let edit = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                            if self.focus_rename {
                                edit.request_focus();
                                self.focus_rename = false;
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
                            if let Some(c) = click_of(&ui.selectable_label(picked, &m.name), true) {
                                clicked = Some((index, c));
                            }
                        }
                    }
                    if cols.start
                        && let Some(c) = click_of(&cell(ui, time(m.start)), false)
                    {
                        clicked = Some((index, c));
                    }
                    if cols.end
                        && let Some(c) = click_of(&cell(ui, time(m.end.max(m.start))), false)
                    {
                        clicked = Some((index, c));
                    }
                    if cols.duration
                        && let Some(c) = click_of(&cell(ui, if m.is_point() { String::new() } else { time(m.end - m.start) }), false)
                    {
                        clicked = Some((index, c));
                    }
                    if cols.kind
                        && let Some(c) = click_of(&cell(ui, if m.is_point() { "Point" } else { "Range" }.to_string()), false)
                    {
                        clicked = Some((index, c));
                    }
                    if cols.description
                        && let Some(c) = click_of(&cell(ui, m.description.clone()), false)
                    {
                        clicked = Some((index, c));
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
        ui.horizontal(|ui| {
            merge = ui.add_enabled(picked_ids.len() >= 2, egui::Button::new("Merge")).on_hover_text("Make the picked markers one range").clicked();
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
        if let Some((index, c)) = clicked {
            out.select = self.panel_click(&markers, index, c, shift, ctrl);
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
    fn a_single_click_picks_and_a_double_click_goes() {
        let m = Markers::from_items(vec![("a".into(), 1000, 1000), ("b".into(), 5000, 5400)]);
        let mut ui = MarkersUi::new();
        let b = m.id_at(1).unwrap();
        assert_eq!(ui.panel_click(&m, 1, Click::Single { name: false }, false, false), None, "a single click only picks");
        assert_eq!(ui.pick.ids, vec![b]);
        assert_eq!(ui.panel_click(&m, 1, Click::Double, false, false), Some(5000..5400), "a double-click goes to the span");
        assert_eq!(ui.panel_click(&m, 0, Click::Double, false, false), Some(1000..1000), "a point goes to its frame");
        assert!(ui.renaming.is_none(), "a double-click starts no rename");
    }

    #[test]
    fn a_click_on_the_name_of_the_only_picked_row_starts_renaming_it() {
        let m = Markers::from_items(vec![("a".into(), 1000, 1000), ("b".into(), 5000, 5400)]);
        let mut ui = MarkersUi::new();
        let b = m.id_at(1).unwrap();
        assert_eq!(ui.panel_click(&m, 1, Click::Single { name: true }, false, false), None);
        assert!(ui.renaming.is_none(), "the first click only picks the row");
        assert_eq!(ui.panel_click(&m, 1, Click::Single { name: false }, false, false), None);
        assert!(ui.renaming.is_none(), "a click on another cell does not rename");
        assert_eq!(ui.panel_click(&m, 1, Click::Single { name: true }, false, false), None);
        assert_eq!(ui.renaming, Some((b, "b".to_string())));
        assert!(ui.focus_rename);
    }
}
