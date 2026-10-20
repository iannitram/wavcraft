//! The panel registry (roadmap A008): every panel of the window, with an id, a title and the
//! side it docks to by default, and which of them are shown.
//!
//! The window lists the registry in its Window menu; the settings file keeps the ids of the
//! panels that were hidden (so a panel added later starts shown); the control channel reports
//! and changes visibility by id. Docking and floating are not here yet: `default_dock` only
//! says where a panel sits today.

/// Where a panel sits by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSide {
    Left,
    Right,
    Bottom,
    /// In the editor area.
    Center,
}

/// One registered panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelInfo {
    /// Stable id: used in the settings file and in control commands. Lower case, no spaces.
    pub id: &'static str,
    /// The name in the Window menu and the panel's heading.
    pub title: &'static str,
    pub default_dock: DockSide,
}

/// Every panel, in Window menu order.
pub const REGISTRY: &[PanelInfo] = &[
    PanelInfo { id: "waveform", title: "Waveform", default_dock: DockSide::Center },
    PanelInfo { id: "spectral", title: "Spectral Frequency Display", default_dock: DockSide::Center },
    PanelInfo { id: "history", title: "History", default_dock: DockSide::Right },
];

/// The registered panel with this id.
pub fn by_id(id: &str) -> Option<&'static PanelInfo> {
    REGISTRY.iter().find(|p| p.id == id)
}

/// One entry of a panel's menu (the ≡ button in its header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    /// Stable id: used by `PANELMENU <panel> run <id>`. Lower case, underscores.
    pub id: &'static str,
    pub label: &'static str,
}

/// A menu item one panel registers for itself (A048). Additive: adding a panel's items does not
/// touch its `PanelInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelItem {
    pub panel: &'static str,
    pub item: MenuItem,
}

/// The items every panel's menu starts with.
pub const STANDARD_ITEMS: &[MenuItem] = &[MenuItem { id: "close", label: "Close Panel" }, MenuItem { id: "maximize", label: "Maximize" }];

/// The id of the Metadata Display item, added to the menu of every panel that lists files.
pub const METADATA_ITEM: MenuItem = MenuItem { id: "metadata_display", label: "Metadata Display…" };

/// Items panels register for themselves, after the standard ones.
pub const PANEL_ITEMS: &[PanelItem] = &[PanelItem { panel: "history", item: MenuItem { id: "clear_history", label: "Clear History…" } }];

/// Panels that list files: their menu also gets the Metadata Display item. A panel that lists
/// files adds its id here (none of today's panels do).
pub const FILE_LISTING_PANELS: &[&str] = &[];

/// The menu of `panel`: the standard items, the panel's own, then Metadata Display if it lists
/// files. Empty for an id that is not registered. (`own` and `file_panels` are parameters so
/// the registration can be tested; `PanelSet::menu` passes the real tables.)
pub fn items_for(panel: &str, own: &[PanelItem], file_panels: &[&str]) -> Vec<MenuItem> {
    if by_id(panel).is_none() {
        return Vec::new();
    }
    let mut items: Vec<MenuItem> = STANDARD_ITEMS.to_vec();
    items.extend(own.iter().filter(|p| p.panel == panel).map(|p| p.item));
    if file_panels.contains(&panel) {
        items.push(METADATA_ITEM);
    }
    items
}

/// A metadata field a file-listing panel can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataField {
    Name,
    Duration,
    SampleRate,
    Channels,
    Format,
    Size,
    Modified,
}

impl MetadataField {
    pub const ALL: &'static [MetadataField] = &[
        MetadataField::Name,
        MetadataField::Duration,
        MetadataField::SampleRate,
        MetadataField::Channels,
        MetadataField::Format,
        MetadataField::Size,
        MetadataField::Modified,
    ];
    /// What the settings file and control commands call it.
    pub fn id(self) -> &'static str {
        match self {
            MetadataField::Name => "name",
            MetadataField::Duration => "duration",
            MetadataField::SampleRate => "rate",
            MetadataField::Channels => "channels",
            MetadataField::Format => "format",
            MetadataField::Size => "size",
            MetadataField::Modified => "modified",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            MetadataField::Name => "File name",
            MetadataField::Duration => "Duration",
            MetadataField::SampleRate => "Sample rate",
            MetadataField::Channels => "Channels",
            MetadataField::Format => "Format",
            MetadataField::Size => "File size",
            MetadataField::Modified => "Date modified",
        }
    }
    pub fn from_id(id: &str) -> Option<MetadataField> {
        MetadataField::ALL.iter().copied().find(|f| f.id() == id.trim())
    }
}

/// Which metadata fields file-listing panels show (the Metadata Display dialog), in
/// `MetadataField::ALL` order. Kept in the settings file as `metadata_fields`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDisplay {
    fields: Vec<MetadataField>,
}

impl Default for MetadataDisplay {
    fn default() -> Self {
        MetadataDisplay { fields: vec![MetadataField::Name, MetadataField::Duration, MetadataField::SampleRate, MetadataField::Channels] }
    }
}

impl MetadataDisplay {
    /// From saved ids; `None` (never saved) gives the defaults, unknown ids are ignored.
    pub fn from_ids<S: AsRef<str>>(ids: Option<&[S]>) -> MetadataDisplay {
        match ids {
            None => MetadataDisplay::default(),
            Some(ids) => {
                MetadataDisplay { fields: MetadataField::ALL.iter().copied().filter(|f| ids.iter().any(|i| i.as_ref().trim() == f.id())).collect() }
            }
        }
    }
    pub fn ids(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.id().to_string()).collect()
    }
    pub fn shows(&self, f: MetadataField) -> bool {
        self.fields.contains(&f)
    }
    pub fn set(&mut self, f: MetadataField, on: bool) {
        self.fields.retain(|x| *x != f);
        if on {
            self.fields.push(f);
            self.fields.sort_by_key(|x| MetadataField::ALL.iter().position(|a| a == x));
        }
    }
    /// `name,duration` (what the control channel reports).
    pub fn report(&self) -> String {
        if self.fields.is_empty() { "none".to_string() } else { self.ids().join(",") }
    }
}

/// Which panels are shown. All start shown. One may be maximized: it alone is drawn, the others
/// keep their shown state for when it is restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSet {
    shown: Vec<bool>,
    maximized: Option<usize>,
}

impl Default for PanelSet {
    fn default() -> Self {
        PanelSet { shown: vec![true; REGISTRY.len()], maximized: None }
    }
}

impl PanelSet {
    /// Everything shown except the panels whose ids are in `hidden` (ids not in the registry
    /// are ignored).
    pub fn from_hidden<S: AsRef<str>>(hidden: &[S]) -> PanelSet {
        let mut set = PanelSet::default();
        for h in hidden {
            if let Some(i) = REGISTRY.iter().position(|p| p.id == h.as_ref().trim()) {
                set.shown[i] = false;
            }
        }
        set
    }

    /// Whether the panel is drawn now: shown, and not hidden behind a maximized panel.
    pub fn is_visible(&self, id: &str) -> bool {
        self.is_shown(id) && self.maximized.is_none_or(|m| REGISTRY[m].id == id)
    }

    /// The id of the maximized panel, if any.
    pub fn maximized(&self) -> Option<&'static str> {
        self.maximized.map(|m| REGISTRY[m].id)
    }

    /// The panel's menu, with Maximize reading Restore while it is the maximized one.
    pub fn menu(&self, id: &str) -> Vec<MenuItem> {
        let restore = self.maximized() == Some(id);
        items_for(id, PANEL_ITEMS, FILE_LISTING_PANELS)
            .into_iter()
            .map(|i| if restore && i.id == "maximize" { MenuItem { id: "maximize", label: "Restore" } } else { i })
            .collect()
    }

    /// Run a standard menu item (`close`, `maximize`). `None` when `item` is not a standard one
    /// (the app runs panel-specific items); `Some(Err)` for an unregistered panel.
    pub fn run_standard(&mut self, id: &str, item: &str) -> Option<Result<(), String>> {
        if !STANDARD_ITEMS.iter().any(|i| i.id == item) {
            return None;
        }
        let Some(idx) = REGISTRY.iter().position(|p| p.id == id) else {
            return Some(Err(format!("no panel with the id {id:?}")));
        };
        if item == "close" {
            self.shown[idx] = false;
            if self.maximized == Some(idx) {
                self.maximized = None;
            }
        } else {
            self.shown[idx] = true;
            self.maximized = if self.maximized == Some(idx) { None } else { Some(idx) };
        }
        Some(Ok(()))
    }

    /// The ids of the hidden panels, in registry order (what the settings file keeps).
    pub fn hidden_ids(&self) -> Vec<String> {
        REGISTRY.iter().zip(&self.shown).filter(|(_, s)| !**s).map(|(p, _)| p.id.to_string()).collect()
    }

    /// Whether the panel is shown (false for an id that is not registered).
    pub fn is_shown(&self, id: &str) -> bool {
        REGISTRY.iter().position(|p| p.id == id).is_some_and(|i| self.shown[i])
    }

    /// Show or hide a panel. Returns false for an id that is not registered.
    pub fn set(&mut self, id: &str, shown: bool) -> bool {
        match REGISTRY.iter().position(|p| p.id == id) {
            Some(i) => {
                self.shown[i] = shown;
                if !shown && self.maximized == Some(i) {
                    self.maximized = None;
                }
                true
            }
            None => false,
        }
    }

    /// Flip a panel. Returns its new state, or `None` for an id that is not registered.
    pub fn toggle(&mut self, id: &str) -> Option<bool> {
        let now = !self.is_shown(id);
        self.set(id, now).then_some(now)
    }

    /// Carry out a control action name: `window.toggle.<id>`, `window.show.<id>` or
    /// `window.hide.<id>`. `None` when the name is not a window action; `Some(Err)` when it is
    /// but names no panel; `Some(Ok(shown))` with the panel's state afterwards.
    pub fn apply_action(&mut self, name: &str) -> Option<Result<bool, String>> {
        let rest = name.strip_prefix("window.")?;
        let (verb, id) = rest.split_once('.')?;
        if !matches!(verb, "toggle" | "show" | "hide") {
            return None;
        }
        let result = match verb {
            "toggle" => self.toggle(id),
            "show" => self.set(id, true).then_some(true),
            _ => self.set(id, false).then_some(false),
        };
        Some(result.ok_or_else(|| format!("no panel with the id {id:?}")))
    }

    /// `waveform=1 spectral=1 history=0`: the control channel's report.
    pub fn report(&self) -> String {
        REGISTRY.iter().zip(&self.shown).map(|(p, s)| format!("{}={}", p.id, u8::from(*s))).collect::<Vec<_>>().join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_plain() {
        for (i, p) in REGISTRY.iter().enumerate() {
            assert!(!p.id.is_empty() && p.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "{}", p.id);
            assert!(!p.title.is_empty());
            assert!(REGISTRY[i + 1..].iter().all(|q| q.id != p.id), "duplicate id {}", p.id);
        }
        assert_eq!(by_id("history").map(|p| p.default_dock), Some(DockSide::Right));
        assert!(by_id("nope").is_none());
    }

    #[test]
    fn all_start_shown_and_toggle_flips() {
        let mut s = PanelSet::default();
        assert!(REGISTRY.iter().all(|p| s.is_shown(p.id)));
        assert_eq!(s.toggle("history"), Some(false));
        assert!(!s.is_shown("history"));
        assert_eq!(s.toggle("history"), Some(true));
        assert_eq!(s.toggle("nope"), None);
        assert!(!s.is_shown("nope"));
    }

    #[test]
    fn hidden_ids_round_trip_and_unknown_ids_are_ignored() {
        let s = PanelSet::from_hidden(&["history", "gone", " waveform "]);
        assert_eq!(s.hidden_ids(), vec!["waveform", "history"]);
        assert_eq!(PanelSet::from_hidden(&s.hidden_ids()), s);
        assert_eq!(PanelSet::from_hidden::<&str>(&[]), PanelSet::default());
    }

    #[test]
    fn actions_toggle_show_hide_and_report() {
        let mut s = PanelSet::default();
        assert_eq!(s.report(), "waveform=1 spectral=1 history=1");
        assert_eq!(s.apply_action("window.toggle.history"), Some(Ok(false)));
        assert_eq!(s.report(), "waveform=1 spectral=1 history=0");
        assert_eq!(s.apply_action("window.show.history"), Some(Ok(true)));
        assert_eq!(s.apply_action("window.hide.spectral"), Some(Ok(false)));
        assert!(matches!(s.apply_action("window.toggle.nope"), Some(Err(_))));
        assert_eq!(s.apply_action("edit.delete"), None);
        assert_eq!(s.apply_action("window.bogus.history"), None);
    }

    #[test]
    fn registered_items_are_valid() {
        for (i, p) in PANEL_ITEMS.iter().enumerate() {
            assert!(by_id(p.panel).is_some(), "item {} names an unregistered panel", p.item.id);
            assert!(p.item.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{}", p.item.id);
            assert!(!STANDARD_ITEMS.iter().any(|s| s.id == p.item.id) && p.item.id != METADATA_ITEM.id);
            assert!(PANEL_ITEMS[i + 1..].iter().all(|q| !(q.panel == p.panel && q.item.id == p.item.id)));
        }
        assert!(FILE_LISTING_PANELS.iter().all(|id| by_id(id).is_some()));
    }

    #[test]
    fn menu_is_standard_then_own_then_metadata() {
        let own = [
            PanelItem { panel: "history", item: MenuItem { id: "clear_history", label: "Clear History" } },
            PanelItem { panel: "waveform", item: MenuItem { id: "other", label: "Other" } },
        ];
        let ids = |v: Vec<MenuItem>| v.iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(items_for("history", &own, &[])), vec!["close", "maximize", "clear_history"]);
        assert_eq!(ids(items_for("history", &own, &["history"])), vec!["close", "maximize", "clear_history", "metadata_display"]);
        assert_eq!(ids(items_for("spectral", &own, &[])), vec!["close", "maximize"]);
        assert!(items_for("nope", &own, &["nope"]).is_empty());
        assert_eq!(ids(PanelSet::default().menu("history")), vec!["close", "maximize", "clear_history"]);
    }

    #[test]
    fn close_and_maximize_run_and_restore() {
        let mut s = PanelSet::default();
        assert_eq!(s.run_standard("history", "maximize"), Some(Ok(())));
        assert_eq!(s.maximized(), Some("history"));
        assert!(s.is_visible("history") && !s.is_visible("waveform") && s.is_shown("waveform"));
        assert_eq!(s.menu("history")[1].label, "Restore");
        assert_eq!(s.menu("waveform")[1].label, "Maximize");
        assert_eq!(s.run_standard("history", "maximize"), Some(Ok(())));
        assert_eq!(s.maximized(), None);
        assert!(s.is_visible("waveform"));
        s.run_standard("spectral", "maximize");
        assert_eq!(s.run_standard("spectral", "close"), Some(Ok(())));
        assert!(!s.is_shown("spectral") && s.maximized().is_none());
        assert_eq!(s.run_standard("history", "clear_history"), None);
        assert!(matches!(s.run_standard("nope", "close"), Some(Err(_))));
    }

    #[test]
    fn metadata_display_defaults_toggle_and_round_trip() {
        let mut m = MetadataDisplay::default();
        assert_eq!(m.report(), "name,duration,rate,channels");
        m.set(MetadataField::Duration, false);
        m.set(MetadataField::Size, true);
        m.set(MetadataField::Format, true);
        assert_eq!(m.report(), "name,rate,channels,format,size");
        assert_eq!(MetadataDisplay::from_ids(Some(&m.ids())), m);
        assert_eq!(MetadataDisplay::from_ids::<&str>(None), MetadataDisplay::default());
        assert_eq!(MetadataDisplay::from_ids(Some(&["bogus", "size"])).report(), "size");
        assert_eq!(MetadataDisplay::from_ids::<&str>(Some(&[])).report(), "none");
    }
}
