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
use wavcraft_core::{MarkerPick, Markers, Viewport, format_time};

/// How close (in pixels) to a marker's flag a press grabs it.
const GRAB_PX: f32 = 6.0;

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

/// Which end of a marker's extent typed text sets. Not called by the panel yet (the typed-edit step
/// wires it in), so the binary sees it as unused.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// A run of ASCII digits as a number; None for an empty, signed or non-digit text.
#[allow(dead_code)]
fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) { None } else { s.parse().ok() }
}

/// Parses a time in the form the panel shows (`format_time`): `m:ss.mmm`, or `h:mm:ss.mmm` from an
/// hour on. Minutes and seconds are 0..=59; the fraction is one to three digits. Returns the frame
/// at `rate`; None for any other text, a zero rate, or a time too large to count.
#[allow(dead_code)]
pub fn parse_time(text: &str, rate: u32) -> Option<u64> {
    if rate == 0 {
        return None;
    }
    let parts: Vec<&str> = text.trim().split(':').collect();
    let (hours, minutes, secs) = match parts.as_slice() {
        [m, s] => (0, digits(m)?, *s),
        [h, m, s] => (digits(h)?, digits(m)?, *s),
        _ => return None,
    };
    let (whole, fraction) = match secs.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (secs, None),
    };
    let sec = digits(whole)?;
    if minutes >= 60 || sec >= 60 {
        return None;
    }
    let ms = match fraction {
        None => 0,
        Some(f) if (1..=3).contains(&f.len()) => digits(f)?.checked_mul(10u64.pow(3 - f.len() as u32))?,
        Some(_) => return None,
    };
    let total_ms = ((hours.checked_mul(60)?.checked_add(minutes)?).checked_mul(60)?.checked_add(sec)?).checked_mul(1000)?.checked_add(ms)?;
    Some(total_ms.checked_mul(u64::from(rate))? / 1000)
}

/// The extent that typed `text` gives the marker covering `range`, in a file of `len` frames at
/// `rate`. The Start or the End moves to the typed time. None when the text is not a time, the time
/// is past the file, or it would put the end before the start. A start equal to the end is a point.
#[allow(dead_code)]
pub fn typed_extent(range: Range<u64>, edge: Edge, text: &str, rate: u32, len: u64) -> Option<Range<u64>> {
    let frame = parse_time(text, rate)?;
    if frame > len {
        return None;
    }
    match edge {
        Edge::Start if frame <= range.end => Some(frame..range.end),
        Edge::End if frame >= range.start => Some(range.start..frame),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn typed_start_and_end_give_the_new_extent() {
        // 48 kHz; the marker covers 1.000..2.000 s in a 4 s file.
        let (rate, len, range) = (48_000, 192_000, 48_000..96_000);
        assert_eq!(typed_extent(range.clone(), Edge::Start, "0:00.500", rate, len), Some(24_000..96_000));
        assert_eq!(typed_extent(range.clone(), Edge::End, "0:03.000", rate, len), Some(48_000..144_000));
        assert_eq!(typed_extent(range.clone(), Edge::End, "0:00.500", rate, len), None, "an end before the start");
        assert_eq!(typed_extent(range.clone(), Edge::Start, "0:02.500", rate, len), None, "a start after the end");
        assert_eq!(typed_extent(range.clone(), Edge::Start, "abc", rate, len), None);
        assert_eq!(typed_extent(range.clone(), Edge::End, "0:05.000", rate, len), None, "past the 4 s file");
        assert_eq!(typed_extent(range.clone(), Edge::End, "0:04.000", rate, len), Some(48_000..192_000), "the file's last frame is in it");
        assert_eq!(typed_extent(range, Edge::Start, "0:02.000", rate, len), Some(96_000..96_000), "a start at the end is a point");
    }

    #[test]
    fn parse_time_reads_the_panel_form_and_refuses_the_rest() {
        assert_eq!(parse_time("1:02.250", 44_100), Some(62 * 44_100 + 11_025));
        assert_eq!(parse_time("1:00:00.000", 48_000), Some(3600 * 48_000));
        assert_eq!(parse_time("0:00.5", 48_000), Some(24_000), "a short fraction is tenths");
        assert_eq!(parse_time("0:60.000", 48_000), None, "seconds stop at 59");
        assert_eq!(parse_time("60:00.000", 48_000), None, "minutes stop at 59");
        assert_eq!(parse_time("-1:00.000", 48_000), None);
        assert_eq!(parse_time("0:00.1234", 48_000), None, "at most three fraction digits");
        assert_eq!(parse_time("", 48_000), None);
        assert_eq!(parse_time("0:00.500", 0), None, "a zero rate");
    }

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
