//! The edit actions (task 07, first part): one table of ids, menu labels and Audition's keys,
//! and an [`Editor`] that runs them on a document's [`History`] without a window. The app's Edit
//! menu, its keys and the control channel all run actions from here, and so do the tests.
//!
//! Keys follow Adobe Audition's defaults (`plan/audition-parity.md` and task 07's table): only
//! the binding is matched, never Adobe's text. `cmd` is Command on a Mac and Ctrl elsewhere.

use std::ops::Range;
use std::sync::Arc;

use crate::doc::edit::{self, ChannelMask, Clip, Crossfades, EditError, PasteAt};
use crate::doc::history::History;

/// An action's id, as the menu, the keys and the control channel name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    Silence,
    Crop,
    SelectAll,
    DeselectAll,
    ClearTimeSelection,
    LeftChannel,
    RightChannel,
    InsertSilence,
    CursorToStart,
    CursorToEnd,
    SetIn,
    SetOut,
    ZoomIn,
    ZoomOut,
    ZoomToSelection,
    ZoomOutFull,
    ZeroInward,
    ZeroOutward,
    ZeroLeftToLeft,
    ZeroLeftToRight,
    ZeroRightToLeft,
    ZeroRightToRight,
    RepeatLast,
    RepeatLastDialog,
    MarkerAdd,
    MarkerNext,
    MarkerPrev,
    MarkerDelete,
    MarkerToRange,
    MarkerToPoint,
}

/// Which part of the program carries an action out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Changes the document or the selection: [`Editor::run`].
    Edit,
    /// Needs the window (the view, the playhead, a dialog): the app runs it.
    Window,
}

/// A key with its modifiers. `key` is egui's key name (`"Z"`, `"Delete"`, `"ArrowUp"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: &'static str,
}

const fn key(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: false, alt: false, key }
}
const fn cmd(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: false, alt: false, key }
}
const fn cmd_shift(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: true, alt: false, key }
}
const fn cmd_alt(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: false, alt: true, key }
}
const fn shift(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: true, alt: false, key }
}

/// How far a zero-crossing adjust looks for a sign change (v1 D6: 10 ms).
pub const ZERO_CROSSING_REACH_MS: u64 = 10;

impl Shortcut {
    /// How a menu shows it: `⌘⇧Z` on a Mac, `Ctrl+Shift+Z` elsewhere.
    pub fn text(&self, mac: bool) -> String {
        let key = match self.key {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "Delete" if mac => "⌦",
            "Backspace" if mac => "⌫",
            "Equals" => "=",
            "Minus" => "−",
            "Backslash" => "\\",
            k => k,
        };
        if mac {
            format!("{}{}{}{key}", if self.alt { "⌥" } else { "" }, if self.shift { "⇧" } else { "" }, if self.cmd { "⌘" } else { "" })
        } else {
            let mut s = String::new();
            for (on, name) in [(self.cmd, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
                if on {
                    s.push_str(name);
                }
            }
            s + key
        }
    }
}

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub id: ActionId,
    /// The control channel's name for it (`edit.undo`).
    pub name: &'static str,
    /// The menu text when it does not name a step (Undo and Redo add the step's name).
    pub menu: &'static str,
    /// Audition's default keys; empty where Audition has none (menu only, as there).
    pub keys: &'static [Shortcut],
    pub kind: Kind,
}

const fn edit(id: ActionId, name: &'static str, menu: &'static str, keys: &'static [Shortcut]) -> Action {
    Action { id, name, menu, keys, kind: Kind::Edit }
}
const fn window(id: ActionId, name: &'static str, menu: &'static str, keys: &'static [Shortcut]) -> Action {
    Action { id, name, menu, keys, kind: Kind::Window }
}

/// Every action, in Edit-menu order. Keys: Audition 26.5's defaults (task 07, marked † there).
pub const TABLE: &[Action] = &[
    edit(ActionId::Undo, "edit.undo", "Undo", &[cmd("Z")]),
    edit(ActionId::Redo, "edit.redo", "Redo", &[cmd_shift("Z"), cmd("Y")]),
    // Repeat the last repeatable command on the current selection: without its dialog (F3,
    // Shift+R), or with its dialog when it has one (F2, Ctrl+R), as in Audition.
    edit(ActionId::RepeatLast, "edit.repeatLast", "Repeat Last Command", &[key("F3"), shift("R")]),
    window(ActionId::RepeatLastDialog, "edit.repeatLastDialog", "Repeat Last Command…", &[key("F2"), cmd("R")]),
    edit(ActionId::Cut, "edit.cut", "Cut", &[cmd("X")]),
    edit(ActionId::Copy, "edit.copy", "Copy", &[cmd("C")]),
    edit(ActionId::Paste, "edit.paste", "Paste", &[cmd("V")]),
    edit(ActionId::Delete, "edit.delete", "Delete", &[key("Delete"), key("Backspace")]),
    edit(ActionId::Silence, "edit.silence", "Silence", &[]),
    // Insert › Silence… asks for a length (the window's dialog), then runs `Editor::insert_silence`.
    window(ActionId::InsertSilence, "edit.insertSilence", "Insert Silence…", &[]),
    edit(ActionId::Crop, "edit.crop", "Crop", &[cmd("T")]),
    edit(ActionId::SelectAll, "edit.selectAll", "Select All", &[cmd("A")]),
    edit(ActionId::DeselectAll, "edit.deselectAll", "Deselect All", &[cmd_shift("A"), cmd("D")]),
    edit(ActionId::ClearTimeSelection, "edit.clearTimeSelection", "Clear Time Selection", &[key("G")]),
    edit(ActionId::ZeroInward, "edit.zeroCrossings.inward", "Zero Crossings: Adjust Selection Inward", &[shift("I")]),
    edit(ActionId::ZeroOutward, "edit.zeroCrossings.outward", "Zero Crossings: Adjust Selection Outward", &[shift("O")]),
    edit(ActionId::ZeroLeftToLeft, "edit.zeroCrossings.leftToLeft", "Zero Crossings: Left Side to Left", &[shift("H")]),
    edit(ActionId::ZeroLeftToRight, "edit.zeroCrossings.leftToRight", "Zero Crossings: Left Side to Right", &[shift("J")]),
    edit(ActionId::ZeroRightToLeft, "edit.zeroCrossings.rightToLeft", "Zero Crossings: Right Side to Left", &[shift("K")]),
    edit(ActionId::ZeroRightToRight, "edit.zeroCrossings.rightToRight", "Zero Crossings: Right Side to Right", &[shift("L")]),
    window(ActionId::SetIn, "edit.setIn", "Set In Point to Playhead", &[key("I")]),
    window(ActionId::SetOut, "edit.setOut", "Set Out Point to Playhead", &[key("O")]),
    edit(ActionId::LeftChannel, "edit.leftChannel", "Left Channel Only", &[key("ArrowUp")]),
    edit(ActionId::RightChannel, "edit.rightChannel", "Right Channel Only", &[key("ArrowDown")]),
    edit(ActionId::CursorToStart, "view.cursorToStart", "Playhead to Start", &[key("Home")]),
    edit(ActionId::CursorToEnd, "view.cursorToEnd", "Playhead to End", &[key("End")]),
    // Markers (A171, A176, A121): Add Marker is M; the playhead jumps with Ctrl+Alt+Left/Right.
    edit(ActionId::MarkerAdd, "marker.add", "Add Marker", &[key("M")]),
    edit(ActionId::MarkerPrev, "marker.prev", "Playhead to Previous Marker", &[cmd_alt("ArrowLeft")]),
    edit(ActionId::MarkerNext, "marker.next", "Playhead to Next Marker", &[cmd_alt("ArrowRight")]),
    // Delete and convert act on the markers the selection (or cursor) touches; no default keys.
    edit(ActionId::MarkerDelete, "marker.delete", "Delete Marker", &[]),
    edit(ActionId::MarkerToRange, "marker.toRange", "Convert Marker to Range", &[]),
    edit(ActionId::MarkerToPoint, "marker.toPoint", "Convert Marker to Point", &[]),
    window(ActionId::ZoomIn, "view.zoomIn", "Zoom In", &[key("Equals")]),
    window(ActionId::ZoomOut, "view.zoomOut", "Zoom Out", &[key("Minus")]),
    window(ActionId::ZoomToSelection, "view.zoomToSelection", "Zoom to Selection", &[shift("S")]),
    window(ActionId::ZoomOutFull, "view.zoomOutFull", "Zoom Out Full", &[cmd("Backslash")]),
];

/// The table row for `id`.
pub fn action(id: ActionId) -> &'static Action {
    TABLE.iter().find(|a| a.id == id).unwrap_or(&TABLE[0])
}

/// The action the control channel calls `name`.
pub fn by_name(name: &str) -> Option<&'static Action> {
    TABLE.iter().find(|a| a.name == name)
}

/// What a run did, for the window to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The document changed: views and the player must read the new state.
    pub doc_changed: bool,
    /// The selection after the action (`start == end` is just the cursor).
    pub selection: Range<u64>,
    /// A message for the status bar.
    pub message: Option<String>,
}

/// One open document's editing state: its history, the active channels and the clipboard.
pub struct Editor {
    pub history: History,
    /// The channels edits act on (Up / Down pick one, as in Audition).
    pub channels: ChannelMask,
    pub clipboard: Option<Clip>,
    pub crossfades: Crossfades,
    /// What Repeat Last Command runs again (None until a repeatable command has run).
    pub last: Option<Repeat>,
}

/// A command Repeat Last Command can run again, with its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// An action from the table, on the selection at the time of the repeat.
    Action(ActionId),
    /// Insert › Silence of this many frames, at the cursor.
    InsertSilence(u64),
}

/// Whether Repeat Last Command may run `id` again. Delete is not repeatable (Audition), nor are
/// Undo/Redo, the clipboard's Cut and Copy, selection moves and the view.
fn repeatable(id: ActionId) -> bool {
    matches!(
        id,
        ActionId::Silence
            | ActionId::Paste
            | ActionId::Crop
            | ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight
    )
}

/// Which way a selection edge moves to find a zero crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
}

/// The nearest zero crossing to `frame` on the side `toward`, at most `reach` frames away, or
/// None. A crossing at `i` means the signal changes sign between frames `i - 1` and `i` (or frame
/// `i` is exactly 0); `frame` itself counts. The signal is the sum of the `channels` asked for,
/// so a stereo edge lands where the active channels' mix crosses (Audition's rule for stereo was
/// not seen; `docs/code-map.md`, Known differences).
pub fn zero_crossing(doc: &crate::doc::DocState, channels: ChannelMask, frame: u64, toward: Toward, reach: u64) -> std::io::Result<Option<u64>> {
    let len = doc.len_frames();
    if len < 2 {
        return Ok(None);
    }
    let frame = frame.min(len);
    // Frames [lo, hi) cover every pair (i - 1, i) the search can look at.
    let (lo, hi) = match toward {
        Toward::Left => (frame.saturating_sub(reach).saturating_sub(1), (frame + 1).min(len)),
        Toward::Right => (frame.saturating_sub(1), (frame + reach + 1).min(len)),
    };
    let mut mix = vec![0.0f64; (hi - lo) as usize];
    let mut buf = vec![0.0f32; mix.len()];
    for ch in (0..doc.channel_count()).filter(|&c| channels.contains(c)) {
        let got = doc.read(ch, lo, &mut buf)?;
        for (m, v) in mix.iter_mut().zip(&buf[..got]) {
            *m += f64::from(*v);
        }
    }
    let at = |i: u64| mix[(i - lo) as usize];
    let crosses = |i: u64| i > lo && i < hi && (at(i) == 0.0 || (at(i - 1) < 0.0) != (at(i) < 0.0));
    let found = match toward {
        Toward::Left => (frame.saturating_sub(reach)..=frame).rev().find(|&i| crosses(i)),
        Toward::Right => (frame..=(frame + reach).min(len - 1)).find(|&i| crosses(i)),
    };
    Ok(found)
}

impl Editor {
    pub fn new(history: History) -> Editor {
        Editor { history, channels: ChannelMask::ALL, clipboard: None, crossfades: Crossfades::default(), last: None }
    }

    fn len(&self) -> u64 {
        self.history.current().len_frames()
    }

    fn channel_count(&self) -> usize {
        self.history.current().channel_count()
    }

    /// Whether `id` can run with `selection` (the menu greys it out otherwise).
    pub fn enabled(&self, id: ActionId, selection: &Range<u64>) -> bool {
        let has_sel = selection.start < selection.end;
        match id {
            ActionId::Undo => self.history.can_undo(),
            ActionId::Redo => self.history.can_redo(),
            // With no selection, Cut and Copy take the whole file (Audition).
            ActionId::Cut | ActionId::Copy => self.len() > 0,
            ActionId::Paste => self.clipboard.is_some(),
            ActionId::Delete | ActionId::Silence | ActionId::Crop => has_sel,
            ActionId::SelectAll => self.len() > 0,
            ActionId::DeselectAll | ActionId::ClearTimeSelection => has_sel,
            ActionId::LeftChannel | ActionId::RightChannel => self.channel_count() >= 2,
            ActionId::InsertSilence | ActionId::CursorToStart | ActionId::CursorToEnd => true,
            ActionId::MarkerAdd => self.len() > 0,
            ActionId::MarkerNext => self.history.current().markers().next_after(selection.start).is_some(),
            ActionId::MarkerPrev => self.history.current().markers().prev_before(selection.start).is_some(),
            ActionId::MarkerDelete => !self.history.current().markers().touched(selection).is_empty(),
            ActionId::MarkerToRange => has_sel && !self.history.current().markers().touched_kind(selection, true).is_empty(),
            ActionId::MarkerToPoint => !self.history.current().markers().touched_kind(selection, false).is_empty(),
            ActionId::SetIn | ActionId::SetOut | ActionId::ZoomIn | ActionId::ZoomOut | ActionId::ZoomOutFull => self.len() > 0,
            ActionId::ZoomToSelection => has_sel,
            ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => has_sel,
            ActionId::RepeatLast | ActionId::RepeatLastDialog => match self.last {
                Some(Repeat::Action(last)) => self.enabled(last, selection),
                Some(Repeat::InsertSilence(_)) => true,
                None => false,
            },
        }
    }

    /// The menu text for `id`: "Undo Cut", "Redo Paste", or the plain label.
    pub fn label(&self, id: ActionId) -> String {
        let a = action(id);
        match id {
            ActionId::Undo => self.history.undo_label().map_or_else(|| a.menu.to_string(), |l| format!("Undo {l}")),
            ActionId::Redo => self.history.redo_label().map_or_else(|| a.menu.to_string(), |l| format!("Redo {l}")),
            ActionId::LeftChannel if self.channels == ChannelMask::only(0) => "Both Channels".to_string(),
            ActionId::RightChannel if self.channels == ChannelMask::only(1) => "Both Channels".to_string(),
            ActionId::RepeatLast | ActionId::RepeatLastDialog => match self.last {
                Some(Repeat::Action(last)) => format!("Repeat {}", action(last).menu),
                // "Repeat Insert Silence" runs it at once; the dialog version keeps the "…".
                Some(Repeat::InsertSilence(_)) if id == ActionId::RepeatLast => "Repeat Insert Silence".to_string(),
                Some(Repeat::InsertSilence(_)) => format!("Repeat {}", action(ActionId::InsertSilence).menu),
                None => a.menu.to_string(),
            },
            _ => a.menu.to_string(),
        }
    }

    /// Repeat Last Command: the last repeatable command again, with the same settings, on
    /// `selection` (Insert Silence: the same length at the cursor).
    pub fn repeat(&mut self, selection: Range<u64>) -> Result<Outcome, EditError> {
        match self.last {
            Some(Repeat::Action(id)) => self.run(id, selection),
            Some(Repeat::InsertSilence(frames)) => self.insert_silence(selection.start, frames),
            None => Ok(Outcome { doc_changed: false, selection, message: Some("There is no command to repeat yet".to_string()) }),
        }
    }

    /// The zero-crossing adjusts: each moved edge goes to the nearest sign change within
    /// [`ZERO_CROSSING_REACH_MS`] on its side; an edge with none there stays.
    fn zero_crossings(&self, id: ActionId, sel: Range<u64>) -> Result<Outcome, EditError> {
        let doc = self.history.current();
        let reach = u64::from(doc.sample_rate()) * ZERO_CROSSING_REACH_MS / 1000;
        let channels = self.channels.within(doc.channel_count());
        let find = |frame: u64, toward: Toward| -> Result<u64, EditError> {
            Ok(zero_crossing(doc, channels, frame, toward, reach).map_err(EditError::from)?.unwrap_or(frame))
        };
        let (start, end) = match id {
            ActionId::ZeroInward => (find(sel.start, Toward::Right)?, find(sel.end, Toward::Left)?),
            ActionId::ZeroOutward => (find(sel.start, Toward::Left)?, find(sel.end, Toward::Right)?),
            ActionId::ZeroLeftToLeft => (find(sel.start, Toward::Left)?, sel.end),
            ActionId::ZeroLeftToRight => (find(sel.start, Toward::Right)?, sel.end),
            ActionId::ZeroRightToLeft => (sel.start, find(sel.end, Toward::Left)?),
            _ => (sel.start, find(sel.end, Toward::Right)?),
        };
        // An inward adjust never turns the selection inside out.
        let (selection, message) = if start < end {
            let moved = start != sel.start || end != sel.end;
            (
                start..end,
                Some(
                    if moved {
                        "Selection edges moved to zero crossings"
                    } else {
                        "The edges already sit on zero crossings, or none is within 10 ms"
                    }
                    .to_string(),
                ),
            )
        } else {
            (sel, Some("No zero crossings inside the selection within 10 ms".to_string()))
        };
        Ok(Outcome { doc_changed: false, selection, message })
    }

    /// Insert › Silence: `frames` of silence at `at` on every channel (Audition inserts across
    /// all channels); the inserted range ends up selected.
    pub fn insert_silence(&mut self, at: u64, frames: u64) -> Result<Outcome, EditError> {
        let doc = self.history.current().clone();
        let at = at.min(doc.len_frames());
        let edit = edit::insert_silence(&doc, at, frames, self.crossfades)?;
        self.last = Some(Repeat::InsertSilence(frames));
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        Ok(Outcome { doc_changed: changed, selection: at..at + frames, message: changed.then_some(label) })
    }

    /// Run `id` on `selection`. A disabled action does nothing and says why.
    pub fn run(&mut self, id: ActionId, selection: Range<u64>) -> Result<Outcome, EditError> {
        let len = self.len();
        let sel = selection.start.min(len)..selection.end.min(len);
        let unchanged = |selection: Range<u64>| Ok(Outcome { doc_changed: false, selection, message: None });
        if !self.enabled(id, &sel) {
            return Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("{} is not available now", action(id).menu)) });
        }
        if repeatable(id) {
            self.last = Some(Repeat::Action(id));
        }
        match id {
            ActionId::RepeatLast => return self.repeat(sel),
            ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => return self.zero_crossings(id, sel),
            _ => {}
        }
        let doc = self.history.current().clone();
        let (xf, channels) = (self.crossfades, self.channels.within(doc.channel_count()));
        let whole_if_empty = |r: &Range<u64>| if r.start < r.end { r.clone() } else { 0..len };
        let edit = match id {
            ActionId::Undo | ActionId::Redo => {
                let moved = if id == ActionId::Undo { self.history.undo() } else { self.history.redo() };
                let new_len = self.len();
                let selection = sel.start.min(new_len)..sel.end.min(new_len);
                return Ok(Outcome { doc_changed: moved.is_some(), selection, message: None });
            }
            ActionId::Copy => {
                self.clipboard = Some(edit::copy(&doc, whole_if_empty(&sel), channels)?);
                return unchanged(sel);
            }
            ActionId::Cut => {
                let range = whole_if_empty(&sel);
                let (e, clip) = edit::cut(&doc, range.clone(), channels, xf)?;
                self.clipboard = Some(clip);
                let at = if e.state.len_frames() < len { range.start..range.start } else { range };
                (e, at)
            }
            ActionId::Paste => {
                let Some(clip) = self.clipboard.clone() else { return unchanged(sel) };
                let at = if sel.start < sel.end { PasteAt::Replace(sel.clone()) } else { PasteAt::Insert(sel.start) };
                let e = edit::paste(&doc, at, &clip, channels, xf)?;
                // The pasted audio ends up selected.
                let pasted = clip.len_frames();
                (e, sel.start..sel.start + pasted)
            }
            ActionId::Delete => {
                let e = edit::delete(&doc, sel.clone(), channels, xf)?;
                let at = if e.state.len_frames() < len { sel.start..sel.start } else { sel.clone() };
                (e, at)
            }
            ActionId::Silence => (edit::silence(&doc, sel.clone(), channels, xf)?, sel.clone()),
            ActionId::Crop => {
                let e = edit::crop(&doc, sel.clone(), channels, xf)?;
                let new_len = e.state.len_frames();
                let at = if new_len < len { 0..new_len } else { sel.clone() };
                (e, at)
            }
            ActionId::SelectAll => return unchanged(0..len),
            ActionId::DeselectAll | ActionId::ClearTimeSelection => return unchanged(sel.start..sel.start),
            ActionId::CursorToStart => return unchanged(0..0),
            ActionId::CursorToEnd => return unchanged(len..len),
            ActionId::MarkerAdd => {
                // A selection makes a range marker; otherwise a point at the cursor.
                let Some(markers) = doc.markers().with_added(sel.clone(), None) else {
                    return Ok(Outcome { doc_changed: false, selection: sel, message: Some("There is a marker here already".to_string()) });
                };
                self.history.push_markers("Add Marker", Arc::new(markers));
                // Markers draw over the same audio: the views need no rebuild.
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some("Add Marker".to_string()) });
            }
            ActionId::MarkerDelete => {
                let ids = doc.markers().touched(&sel);
                let Some(markers) = doc.markers().without(&ids) else { return Ok(Outcome { doc_changed: false, selection: sel, message: Some("No marker here".to_string()) }) };
                self.history.push_markers("Delete Marker", Arc::new(markers));
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some("Delete Marker".to_string()) });
            }
            ActionId::MarkerToRange | ActionId::MarkerToPoint => {
                let to_range = id == ActionId::MarkerToRange;
                let found = doc.markers().touched_kind(&sel, to_range).first().copied();
                let extent = found.and_then(|i| doc.markers().iter().find(|m| m.id == i)).map(|m| if to_range { sel.clone() } else { m.start..m.start });
                let changed = found.zip(extent).and_then(|(i, r)| doc.markers().with_extent(i, r));
                let label = if to_range { "Convert Marker to Range" } else { "Convert Marker to Point" };
                let Some(markers) = changed else { return Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("{label} is not available now")) }) };
                self.history.push_markers(label, Arc::new(markers));
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(label.to_string()) });
            }
            ActionId::MarkerNext | ActionId::MarkerPrev => {
                let m = doc.markers();
                let to = if id == ActionId::MarkerNext { m.next_after(sel.start) } else { m.prev_before(sel.start) };
                return unchanged(to.map_or(sel.clone(), |f| f..f));
            }
            ActionId::InsertSilence
            | ActionId::SetIn
            | ActionId::SetOut
            | ActionId::ZoomIn
            | ActionId::ZoomOut
            | ActionId::ZoomToSelection
            | ActionId::ZoomOutFull
            // Repeat and the zero-crossing adjusts returned above.
            | ActionId::RepeatLastDialog
            | ActionId::RepeatLast
            | ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => return unchanged(sel),
            ActionId::LeftChannel | ActionId::RightChannel => {
                let ch = usize::from(id == ActionId::RightChannel);
                self.channels =
                    if self.channels.within(doc.channel_count()) == ChannelMask::only(ch) { ChannelMask::ALL } else { ChannelMask::only(ch) };
                let msg = if self.channels == ChannelMask::ALL {
                    "Editing both channels".to_string()
                } else {
                    format!("Editing the {} channel only", ["left", "right"][ch])
                };
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(msg) });
            }
        };
        let (edit, selection) = edit;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        let new_len = self.len();
        Ok(Outcome { doc_changed: changed, selection: selection.start.min(new_len)..selection.end.min(new_len), message: changed.then_some(label) })
    }
}
