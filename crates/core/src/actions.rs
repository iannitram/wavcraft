//! The edit actions (task 07, first part): one table of ids, menu labels and Audition's keys,
//! and an [`Editor`] that runs them on a document's [`History`] without a window. The app's Edit
//! menu, its keys and the control channel all run actions from here, and so do the tests.
//!
//! Keys follow Adobe Audition's defaults (`plan/audition-parity.md` and task 07's table): only
//! the binding is matched, never Adobe's text. `cmd` is Command on a Mac and Ctrl elsewhere.

use std::ops::Range;

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

impl Shortcut {
    /// How a menu shows it: `⌘⇧Z` on a Mac, `Ctrl+Shift+Z` elsewhere.
    pub fn text(&self, mac: bool) -> String {
        let key = match self.key {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "Delete" if mac => "⌦",
            "Backspace" if mac => "⌫",
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
}

/// Every action, in Edit-menu order. Keys: Audition 26.5's defaults (task 07, marked † there).
pub const TABLE: &[Action] = &[
    Action { id: ActionId::Undo, name: "edit.undo", menu: "Undo", keys: &[cmd("Z")] },
    Action { id: ActionId::Redo, name: "edit.redo", menu: "Redo", keys: &[cmd_shift("Z"), cmd("Y")] },
    Action { id: ActionId::Cut, name: "edit.cut", menu: "Cut", keys: &[cmd("X")] },
    Action { id: ActionId::Copy, name: "edit.copy", menu: "Copy", keys: &[cmd("C")] },
    Action { id: ActionId::Paste, name: "edit.paste", menu: "Paste", keys: &[cmd("V")] },
    Action { id: ActionId::Delete, name: "edit.delete", menu: "Delete", keys: &[key("Delete"), key("Backspace")] },
    Action { id: ActionId::Silence, name: "edit.silence", menu: "Silence", keys: &[] },
    Action { id: ActionId::Crop, name: "edit.crop", menu: "Crop", keys: &[cmd("T")] },
    Action { id: ActionId::SelectAll, name: "edit.selectAll", menu: "Select All", keys: &[cmd("A")] },
    Action { id: ActionId::DeselectAll, name: "edit.deselectAll", menu: "Deselect All", keys: &[cmd_shift("A"), cmd("D")] },
    Action { id: ActionId::ClearTimeSelection, name: "edit.clearTimeSelection", menu: "Clear Time Selection", keys: &[key("G")] },
    Action { id: ActionId::LeftChannel, name: "edit.leftChannel", menu: "Left Channel Only", keys: &[key("ArrowUp")] },
    Action { id: ActionId::RightChannel, name: "edit.rightChannel", menu: "Right Channel Only", keys: &[key("ArrowDown")] },
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
}

impl Editor {
    pub fn new(history: History) -> Editor {
        Editor { history, channels: ChannelMask::ALL, clipboard: None, crossfades: Crossfades::default() }
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
            _ => a.menu.to_string(),
        }
    }

    /// Run `id` on `selection`. A disabled action does nothing and says why.
    pub fn run(&mut self, id: ActionId, selection: Range<u64>) -> Result<Outcome, EditError> {
        let len = self.len();
        let sel = selection.start.min(len)..selection.end.min(len);
        let unchanged = |selection: Range<u64>| Ok(Outcome { doc_changed: false, selection, message: None });
        if !self.enabled(id, &sel) {
            return Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("{} is not available now", action(id).menu)) });
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
