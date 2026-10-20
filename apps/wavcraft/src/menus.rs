//! The right-click menus, the toolbar and the Tools panel (A351, A014, A035) as plain data.
//!
//! Nothing here has logic of its own: an entry is either a row of the action table
//! (`wavcraft_core::actions::TABLE`, so the label, the key and the way it runs are the Edit and
//! View menus') or one of the few app commands the table does not hold (open, save, play/stop,
//! the tools). The app turns an entry into a click, a control-channel line or a drawn button.

use wavcraft_core::actions::{self, ActionId};

/// What an entry does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// A row of the action table (run with `WaveApp::run_action`).
    Action(ActionId),
    /// File › Open… (a file dialog: not available from the control channel).
    Open,
    /// File › Save (writes the file in place when it can; asks where only for a non-WAV source).
    Save,
    /// Play the selection, or stop.
    PlayStop,
    /// Make this the active tool.
    Tool(Tool),
}

/// The editor's tools. Only the first three are built; the rest of the Audition set is listed
/// in the Tools panel as not available yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Drag selects, click places the cursor (the behaviour before the Tools panel).
    #[default]
    TimeSelection,
    /// Drag scrolls through time.
    Hand,
    /// Click zooms in around the pointer, Shift+click zooms out.
    Zoom,
}

/// A tool in the Tools panel: `tool` is `None` for the ones not built yet.
pub struct ToolEntry {
    pub name: &'static str,
    pub label: &'static str,
    pub tool: Option<Tool>,
    pub hint: &'static str,
}

pub const TOOLS: &[ToolEntry] = &[
    ToolEntry {
        name: "tool.timeSelection",
        label: "Time Selection",
        tool: Some(Tool::TimeSelection),
        hint: "Drag to select, click to place the cursor",
    },
    ToolEntry { name: "tool.hand", label: "Hand", tool: Some(Tool::Hand), hint: "Drag to scroll through time" },
    ToolEntry { name: "tool.zoom", label: "Zoom", tool: Some(Tool::Zoom), hint: "Click to zoom in, Shift+click to zoom out" },
    ToolEntry { name: "tool.marquee", label: "Marquee Selection", tool: None, hint: "Not available yet" },
    ToolEntry { name: "tool.lasso", label: "Lasso Selection", tool: None, hint: "Not available yet" },
    ToolEntry { name: "tool.paintbrush", label: "Paintbrush Selection", tool: None, hint: "Not available yet" },
    ToolEntry { name: "tool.spotHealing", label: "Spot Healing", tool: None, hint: "Not available yet" },
];

/// One menu or toolbar entry. `Sep` is a separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Sep,
    Item(Item),
}

/// A named command with its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub name: &'static str,
    pub label: &'static str,
    pub cmd: Cmd,
}

const fn app(name: &'static str, label: &'static str, cmd: Cmd) -> Entry {
    Entry::Item(Item { name, label, cmd })
}

/// The table row for `id` as an entry: its name and menu text are the table's.
pub fn act(id: ActionId) -> Entry {
    let a = actions::action(id);
    Entry::Item(Item { name: a.name, label: a.menu, cmd: Cmd::Action(id) })
}

/// Which context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Editor,
    Ruler,
    History,
}

impl Menu {
    pub fn by_name(name: &str) -> Option<Menu> {
        match name {
            "editor" => Some(Menu::Editor),
            "ruler" => Some(Menu::Ruler),
            "history" => Some(Menu::History),
            _ => None,
        }
    }

    /// The entries, top to bottom. The ruler has no time-format or snapping entries: neither
    /// setting exists in the app yet, so there is nothing to put in them (A351, alpha).
    pub fn entries(self) -> Vec<Entry> {
        match self {
            Menu::Editor => vec![
                act(ActionId::Cut),
                act(ActionId::Copy),
                act(ActionId::Paste),
                act(ActionId::Delete),
                act(ActionId::Silence),
                act(ActionId::Crop),
                Entry::Sep,
                act(ActionId::SelectAll),
                act(ActionId::DeselectAll),
                Entry::Sep,
                act(ActionId::ZoomToSelection),
                act(ActionId::ZoomOutFull),
            ],
            Menu::Ruler => vec![act(ActionId::ZoomIn), act(ActionId::ZoomOut), act(ActionId::ZoomToSelection), act(ActionId::ZoomOutFull)],
            Menu::History => vec![act(ActionId::Undo), act(ActionId::Redo)],
        }
    }
}

/// The toolbar under the menu bar: file, history, transport, zoom, then the tool buttons.
pub fn toolbar() -> Vec<Entry> {
    let mut v = vec![
        app("file.open", "Open", Cmd::Open),
        app("file.save", "Save", Cmd::Save),
        Entry::Sep,
        act(ActionId::Undo),
        act(ActionId::Redo),
        Entry::Sep,
        app("transport.playStop", "Play/Stop", Cmd::PlayStop),
        Entry::Sep,
        act(ActionId::ZoomIn),
        act(ActionId::ZoomOut),
        act(ActionId::ZoomToSelection),
        act(ActionId::ZoomOutFull),
        Entry::Sep,
    ];
    v.extend(TOOLS.iter().filter_map(|t| t.tool.map(|tool| app(t.name, t.label, Cmd::Tool(tool)))));
    v
}

/// The short text on a toolbar button (the label is its tooltip).
pub fn glyph(item: &Item) -> &'static str {
    match item.name {
        "file.open" => "Open",
        "file.save" => "Save",
        "edit.undo" => "Undo",
        "edit.redo" => "Redo",
        "transport.playStop" => "Play/Stop",
        "view.zoomIn" => "Zoom +",
        "view.zoomOut" => "Zoom -",
        "view.zoomToSelection" => "To Sel.",
        "view.zoomOutFull" => "Fit",
        "tool.timeSelection" => "Time",
        "tool.hand" => "Hand",
        "tool.zoom" => "Zoom",
        _ => item.label,
    }
}

/// Find a toolbar entry by its control name.
pub fn toolbar_item(name: &str) -> Option<Item> {
    toolbar().into_iter().find_map(|e| match e {
        Entry::Item(i) if i.name == name => Some(i),
        _ => None,
    })
}

/// Find a menu entry by its control name.
pub fn menu_item(menu: Menu, name: &str) -> Option<Item> {
    menu.entries().into_iter().find_map(|e| match e {
        Entry::Item(i) if i.name == name => Some(i),
        _ => None,
    })
}

/// Whether the editor's context menu opens on a right-click. The "right-click extends the
/// selection" preference (#115) is on another branch: when it merges this reads it and answers
/// true only for its "menu" value. Until then the menu is the default.
pub fn editor_menu_on_right_click() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<&'static str> {
        entries.iter().filter_map(|e| if let Entry::Item(i) = e { Some(i.name) } else { None }).collect()
    }

    #[test]
    fn menu_names_come_from_the_action_table() {
        for menu in [Menu::Editor, Menu::Ruler, Menu::History] {
            for e in menu.entries() {
                if let Entry::Item(Item { name, cmd: Cmd::Action(id), label }) = e {
                    let a = actions::by_name(name).expect("every menu name is a table name");
                    assert_eq!((a.id, a.menu), (id, label));
                }
            }
        }
    }

    #[test]
    fn the_editor_menu_has_the_selection_commands() {
        let n = names(&Menu::Editor.entries());
        for want in ["edit.cut", "edit.copy", "edit.paste", "edit.delete", "edit.silence", "view.zoomToSelection"] {
            assert!(n.contains(&want), "{want} in {n:?}");
        }
    }

    #[test]
    fn toolbar_has_the_common_commands_and_unique_names() {
        let n = names(&toolbar());
        for want in [
            "file.open",
            "file.save",
            "edit.undo",
            "edit.redo",
            "transport.playStop",
            "view.zoomIn",
            "view.zoomOut",
            "tool.timeSelection",
            "tool.hand",
        ] {
            assert!(n.contains(&want), "{want} in {n:?}");
        }
        let mut sorted = n.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), n.len());
    }

    #[test]
    fn lookups_find_items_and_refuse_strangers() {
        assert_eq!(menu_item(Menu::Editor, "edit.copy").map(|i| i.cmd), Some(Cmd::Action(ActionId::Copy)));
        assert!(menu_item(Menu::Ruler, "edit.copy").is_none());
        assert_eq!(toolbar_item("tool.hand").map(|i| i.cmd), Some(Cmd::Tool(Tool::Hand)));
        assert!(Menu::by_name("nope").is_none());
    }

    #[test]
    fn every_tool_entry_with_a_tool_is_on_the_toolbar() {
        let n = names(&toolbar());
        for t in TOOLS.iter().filter(|t| t.tool.is_some()) {
            assert!(n.contains(&t.name));
        }
        assert!(TOOLS.iter().any(|t| t.tool.is_none()), "planned tools are listed");
    }
}
