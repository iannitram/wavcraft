//! The panel registry (roadmap A008): every panel of the window, with an id, a title and the
//! side it docks to by default, and which of them are shown.
//!
//! The window lists the registry in its Window menu; the settings file keeps the ids of the
//! panels that were hidden (so a panel added later starts shown); the control channel reports
//! and changes visibility by id.
//!
//! Docking (roadmap A002, A003, A006, A007): [`Layout`] puts every panel in a zone (left, right,
//! bottom; the editor panels, whose default is the centre, stay there), keeps the tab order in
//! each zone, the zone sizes and the panel maximised under the pointer, and turns the part worth
//! keeping (zone, order, active tab, sizes) into two lines of the settings file and back.
//! Closing a panel is hiding it in [`PanelSet`]. Named workspaces keep the layout as text.
//!
//! Floating panels (roadmap A004): a panel can leave its zone and float in a window of its own,
//! with a position and size ([`Float`]); the layout keeps those, writes them as a `float:` part
//! of the tab text (so settings files and workspaces carry them with no new key), and docking the
//! panel back puts it in a zone again.

use crate::monitors::{self, Monitor};

/// Where a panel sits by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSide {
    Left,
    Right,
    Bottom,
    /// In the editor area.
    Center,
}

impl DockSide {
    /// The zone's name in the settings file and in control commands.
    pub fn name(self) -> &'static str {
        match self {
            DockSide::Left => "left",
            DockSide::Right => "right",
            DockSide::Bottom => "bottom",
            DockSide::Center => "center",
        }
    }

    /// The zone with this name (`center` included; a panel cannot be moved there).
    pub fn parse(name: &str) -> Option<DockSide> {
        [DockSide::Left, DockSide::Right, DockSide::Bottom, DockSide::Center].into_iter().find(|z| z.name() == name.trim().to_ascii_lowercase())
    }
}

/// The three zones panels can be moved to, in the order the settings file lists them.
pub const ZONES: [DockSide; 3] = [DockSide::Left, DockSide::Right, DockSide::Bottom];

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
    PanelInfo { id: "selection", title: "Selection/View", default_dock: DockSide::Right },
    PanelInfo { id: "freq", title: "Frequency Analysis", default_dock: DockSide::Bottom },
    PanelInfo { id: "markers", title: "Markers", default_dock: DockSide::Right },
    PanelInfo { id: "playlist", title: "Playlist", default_dock: DockSide::Right },
    PanelInfo { id: "properties", title: "Properties", default_dock: DockSide::Bottom },
    PanelInfo { id: "transport", title: "Transport", default_dock: DockSide::Bottom },
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
/// the registration can be tested; `menu` passes the real tables.)
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

/// The menu of `panel` as the window shows it: `items_for` over the real tables, with Maximize
/// reading Restore while `maximized` says the panel is the maximised one (`Layout::maximized`).
pub fn menu(panel: &str, maximized: bool) -> Vec<MenuItem> {
    items_for(panel, PANEL_ITEMS, FILE_LISTING_PANELS)
        .into_iter()
        .map(|i| if maximized && i.id == "maximize" { MenuItem { id: "maximize", label: "Restore" } } else { i })
        .collect()
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

/// Which panels are shown. All start shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSet {
    shown: Vec<bool>,
}

impl Default for PanelSet {
    fn default() -> Self {
        PanelSet { shown: vec![true; REGISTRY.len()] }
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

/// The least and most a zone may measure (width for left and right, height for bottom), in points.
pub const ZONE_SIZE_RANGE: (f32, f32) = (120.0, 1200.0);
const DEFAULT_ZONE_SIZES: [f32; 3] = [220.0, 240.0, 180.0];

fn zone_index(zone: DockSide) -> Option<usize> {
    ZONES.iter().position(|z| *z == zone)
}

/// Whether a panel can leave its place: the editor panels (default dock: centre) cannot.
pub fn is_movable(id: &str) -> bool {
    by_id(id).is_some_and(|p| p.default_dock != DockSide::Center)
}

/// The smallest and largest a floating window may measure, in points.
pub const FLOAT_SIZE_RANGE: (f32, f32) = (120.0, 4000.0);
const FLOAT_POS_LIMIT: f32 = 20000.0;
const DEFAULT_FLOAT_SIZE: [f32; 2] = [320.0, 260.0];

/// A panel in a window of its own: where the window is (screen points) and how big, and the zone
/// the panel came from (where a plain "Dock" puts it back).
#[derive(Debug, Clone, PartialEq)]
pub struct Float {
    pub id: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub from: DockSide,
    /// The monitor (by name) the window was on when last placed; `None` until one is known.
    pub monitor: Option<String>,
}

fn clean_rect(pos: [f32; 2], size: [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
    if !pos.iter().chain(size.iter()).all(|v| v.is_finite()) {
        return None;
    }
    let c = |v: f32| v.clamp(-FLOAT_POS_LIMIT, FLOAT_POS_LIMIT).round();
    let s = |v: f32| v.clamp(FLOAT_SIZE_RANGE.0, FLOAT_SIZE_RANGE.1).round();
    Some(([c(pos[0]), c(pos[1])], [s(size[0]), s(size[1])]))
}

/// Where every panel sits: the tab order of each zone, which tab is in front, the zone sizes, and
/// the panel maximised (not kept between runs).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Panel ids per zone (`ZONES` order), hidden panels included so they come back in place.
    tabs: [Vec<String>; 3],
    /// The tab in front per zone (the first shown tab when it is `None` or hidden).
    active: [Option<String>; 3],
    sizes: [f32; 3],
    maximized: Option<String>,
    /// Panels in windows of their own (not in `tabs`), in the order they were floated.
    floating: Vec<Float>,
}

impl Default for Layout {
    fn default() -> Self {
        let mut tabs: [Vec<String>; 3] = Default::default();
        for p in REGISTRY {
            if let Some(i) = zone_index(p.default_dock) {
                tabs[i].push(p.id.to_string());
            }
        }
        Layout { tabs, active: Default::default(), sizes: DEFAULT_ZONE_SIZES, maximized: None, floating: Vec::new() }
    }
}

impl Layout {
    /// The zone a panel sits in: `Center` for the editor panels, `None` for an unknown id or a floating panel.
    pub fn zone_of(&self, id: &str) -> Option<DockSide> {
        if self.is_floating(id) {
            return None;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.iter().any(|t| t == id)) {
            return Some(ZONES[i]);
        }
        by_id(id).map(|_| DockSide::Center)
    }

    /// The zone and tab position (among all its tabs, hidden ones included) of a docked panel.
    pub fn position(&self, id: &str) -> Option<(DockSide, usize)> {
        self.tabs.iter().enumerate().find_map(|(z, t)| t.iter().position(|t| t == id).map(|i| (ZONES[z], i)))
    }

    /// The shown tabs of a zone, in order. The centre lists the shown editor panels.
    pub fn visible(&self, zone: DockSide, set: &PanelSet) -> Vec<&str> {
        match zone_index(zone) {
            Some(i) => self.tabs[i].iter().map(String::as_str).filter(|id| set.is_shown(id)).collect(),
            None => REGISTRY.iter().filter(|p| p.default_dock == DockSide::Center && set.is_shown(p.id)).map(|p| p.id).collect(),
        }
    }

    /// The tab in front in a zone: the one last chosen if it is shown, else the first shown.
    pub fn active_in(&self, zone: DockSide, set: &PanelSet) -> Option<&str> {
        let i = zone_index(zone)?;
        let vis = self.visible(zone, set);
        self.active[i].as_deref().and_then(|a| vis.iter().copied().find(|v| *v == a)).or_else(|| vis.first().copied())
    }

    /// Put a panel's tab in front in its zone. False for an unknown or centre panel.
    pub fn activate(&mut self, id: &str) -> bool {
        match self.zone_of(id).and_then(zone_index) {
            Some(i) => {
                self.active[i] = Some(id.to_string());
                true
            }
            None => false,
        }
    }

    /// Step the front tab of a zone by `delta` among the shown tabs, wrapping round.
    pub fn cycle(&mut self, zone: DockSide, set: &PanelSet, delta: i32) {
        let Some(i) = zone_index(zone) else { return };
        let vis: Vec<String> = self.visible(zone, set).into_iter().map(String::from).collect();
        if vis.len() < 2 {
            return;
        }
        let now = self.active_in(zone, set).and_then(|a| vis.iter().position(|v| v == a)).unwrap_or(0) as i32;
        self.active[i] = Some(vis[(now + delta).rem_euclid(vis.len() as i32) as usize].clone());
    }

    /// Move a panel to `zone`, to tab position `index` among that zone's tabs (the end when
    /// `None`), and bring it to the front there. The editor panels and the centre are refused.
    pub fn move_to(&mut self, id: &str, zone: DockSide, index: Option<usize>) -> Result<(), String> {
        let info = by_id(id).ok_or_else(|| format!("no panel with the id {id:?}"))?;
        if !is_movable(id) {
            return Err(format!("{} is part of the editor and stays in the centre", info.title));
        }
        let to = zone_index(zone).ok_or_else(|| "a panel can be moved to left, right or bottom".to_string())?;
        for tabs in &mut self.tabs {
            tabs.retain(|t| t != id);
        }
        self.floating.retain(|f| f.id != id);
        let at = index.unwrap_or(usize::MAX).min(self.tabs[to].len());
        self.tabs[to].insert(at, id.to_string());
        self.active[to] = Some(id.to_string());
        Ok(())
    }

    /// Whether a panel is in a window of its own.
    pub fn is_floating(&self, id: &str) -> bool {
        self.floating.iter().any(|f| f.id == id)
    }

    /// The floating panels that are shown, in the order they were floated.
    pub fn floats<'a>(&'a self, set: &PanelSet) -> Vec<&'a Float> {
        self.floating.iter().filter(|f| set.is_shown(&f.id)).collect()
    }

    /// Take a panel out of its zone into a window. `rect` is (position, size) in screen points;
    /// without it the window cascades from the top left. A panel that already floats is left
    /// alone. The editor panels are refused.
    pub fn float(&mut self, id: &str, rect: Option<([f32; 2], [f32; 2])>) -> Result<(), String> {
        let info = by_id(id).ok_or_else(|| format!("no panel with the id {id:?}"))?;
        if !is_movable(id) {
            return Err(format!("{} is part of the editor and cannot float", info.title));
        }
        if self.is_floating(id) {
            return Ok(());
        }
        let from = self.zone_of(id).filter(|z| zone_index(*z).is_some()).unwrap_or(DockSide::Right);
        for tabs in &mut self.tabs {
            tabs.retain(|t| t != id);
        }
        if self.maximized.as_deref() == Some(id) {
            self.maximized = None;
        }
        let n = self.floating.len() as f32 * 28.0;
        let (pos, size) = rect.and_then(|(p, s)| clean_rect(p, s)).unwrap_or(([120.0 + n, 120.0 + n], DEFAULT_FLOAT_SIZE));
        self.floating.push(Float { id: id.to_string(), pos, size, from, monitor: None });
        Ok(())
    }

    /// Float every shown tab of a zone ("Undock Frame"). Returns how many floated.
    pub fn float_group(&mut self, zone: DockSide, set: &PanelSet) -> usize {
        let ids: Vec<String> = self.visible(zone, set).into_iter().map(String::from).collect();
        ids.iter().filter(|id| self.float(id, None).is_ok()).count()
    }

    /// Put a floating panel's window at a place and size (as the window reports them). True when
    /// it changed by over a point.
    pub fn set_float_rect(&mut self, id: &str, pos: [f32; 2], size: [f32; 2]) -> bool {
        let Some((pos, size)) = clean_rect(pos, size) else { return false };
        let Some(f) = self.floating.iter_mut().find(|f| f.id == id) else { return false };
        let moved = f.pos.iter().zip(pos).chain(f.size.iter().zip(size)).any(|(a, b)| (a - b).abs() > 1.0);
        if moved {
            (f.pos, f.size) = (pos, size);
        }
        moved
    }

    /// Note which monitor each floating window is on (by its centre), for the workspace to keep.
    /// True when any tag changed.
    pub fn tag_monitors(&mut self, mons: &[Monitor]) -> bool {
        let mut changed = false;
        for f in &mut self.floating {
            let centre = [f.pos[0] + f.size[0] / 2.0, f.pos[1] + f.size[1] / 2.0];
            let name = monitors::monitor_at(mons, centre).map(|m| m.name.clone());
            if name.is_some() && f.monitor != name {
                f.monitor = name;
                changed = true;
            }
        }
        changed
    }

    /// Put floating windows right for the monitors there are now (the first is the primary). A
    /// window whose monitor is gone moves onto the primary monitor; one with no monitor noted is
    /// given the one it is nearest; every window is clamped wholly inside its monitor. Returns how
    /// many windows changed.
    pub fn reconcile_monitors(&mut self, mons: &[Monitor]) -> usize {
        let Some(primary) = mons.first() else { return 0 };
        let mut changed = 0;
        for f in &mut self.floating {
            let centre = [f.pos[0] + f.size[0] / 2.0, f.pos[1] + f.size[1] / 2.0];
            let home = match &f.monitor {
                Some(name) => mons.iter().find(|m| &m.name == name).unwrap_or(primary),
                None => monitors::monitor_at(mons, centre).unwrap_or(primary),
            };
            let (pos, size) = monitors::clamp_into(f.pos, f.size, home);
            if f.monitor.as_deref() != Some(home.name.as_str()) || pos != f.pos || size != f.size {
                changed += 1;
            }
            (f.pos, f.size, f.monitor) = (pos, size, Some(home.name.clone()));
        }
        changed
    }

    /// Dock a floating panel back into `zone` (its old one when `None`) at the end of the tabs.
    pub fn dock(&mut self, id: &str, zone: Option<DockSide>) -> Result<(), String> {
        let from = self.floating.iter().find(|f| f.id == id).map(|f| f.from);
        self.move_to(id, zone.or(from).unwrap_or(DockSide::Right), None)
    }

    /// Maximise a panel, or restore if it already is. Returns whether it is maximised now, or
    /// `None` for an unknown id (or a floating panel: dock it first).
    pub fn toggle_maximize(&mut self, id: &str) -> Option<bool> {
        by_id(id)?;
        if self.is_floating(id) {
            return None;
        }
        if self.maximized.as_deref() == Some(id) {
            self.maximized = None;
            Some(false)
        } else {
            self.activate(id);
            self.maximized = Some(id.to_string());
            Some(true)
        }
    }

    /// The maximised panel, if any, unless it has been hidden since.
    pub fn maximized<'a>(&'a self, set: &PanelSet) -> Option<&'a str> {
        self.maximized.as_deref().filter(|id| set.is_shown(id))
    }

    /// Back to the normal layout.
    pub fn restore(&mut self) {
        self.maximized = None;
    }

    /// A zone's width (left, right) or height (bottom); the centre has none.
    pub fn size(&self, zone: DockSide) -> Option<f32> {
        zone_index(zone).map(|i| self.sizes[i])
    }

    /// Set a zone's size, kept within [`ZONE_SIZE_RANGE`]. True when it changed by over a point.
    pub fn set_size(&mut self, zone: DockSide, size: f32) -> bool {
        let Some(i) = zone_index(zone) else { return false };
        let size = if size.is_finite() { size.clamp(ZONE_SIZE_RANGE.0, ZONE_SIZE_RANGE.1) } else { return false };
        let changed = (self.sizes[i] - size).abs() > 1.0;
        if changed {
            self.sizes[i] = size;
        }
        changed
    }

    /// The two settings lines' values: `left:history*,selection;right:;bottom:` (a `*` marks the
    /// front tab) and `left:220,right:240,bottom:180`. Floating panels add a part to the first:
    /// `;float:history@120_120_320_260_left_main` (x, y, width, height, the zone it came from, and the
    /// name of the monitor it was on, left off when none was known).
    pub fn encode(&self) -> (String, String) {
        let tabs = ZONES
            .iter()
            .enumerate()
            .map(|(i, z)| {
                let ids: Vec<String> =
                    self.tabs[i].iter().map(|t| if self.active[i].as_deref() == Some(t) { format!("{t}*") } else { t.clone() }).collect();
                format!("{}:{}", z.name(), ids.join(","))
            })
            .collect::<Vec<_>>()
            .join(";");
        let sizes = ZONES.iter().enumerate().map(|(i, z)| format!("{}:{}", z.name(), self.sizes[i].round())).collect::<Vec<_>>().join(",");
        if self.floating.is_empty() {
            return (tabs, sizes);
        }
        let floats: Vec<String> = self
            .floating
            .iter()
            .map(|f| {
                let mon = f.monitor.as_deref().map_or(String::new(), |m| format!("_{}", crate::monitors::clean_name(m)));
                format!("{}@{}_{}_{}_{}_{}{mon}", f.id, f.pos[0], f.pos[1], f.size[0], f.size[1], f.from.name())
            })
            .collect();
        (format!("{tabs};float:{}", floats.join(",")), sizes)
    }

    /// The layout from the settings values. Whatever does not make sense is ignored: unknown
    /// ids, editor panels, repeats, unknown zones, sizes outside the range. A movable panel the
    /// text leaves out starts in its default zone.
    pub fn decode(tabs: &str, sizes: &str) -> Layout {
        let mut layout = Layout { tabs: Default::default(), ..Layout::default() };
        let mut seen: Vec<String> = Vec::new();
        for part in tabs.split(';') {
            let Some((zone, ids)) = part.split_once(':') else { continue };
            if zone.trim() == "float" {
                for raw in ids.split(',') {
                    let Some((id, rest)) = raw.trim().split_once('@') else { continue };
                    if !is_movable(id) || seen.iter().any(|s| s == id) {
                        continue;
                    }
                    let mut it = rest.split('_');
                    let nums: Vec<f32> = it.by_ref().take(4).filter_map(|n| n.parse().ok()).collect();
                    let from = it.next().and_then(DockSide::parse).filter(|z| zone_index(*z).is_some()).unwrap_or(DockSide::Right);
                    if let [x, y, w, h] = nums[..]
                        && let Some((pos, size)) = clean_rect([x, y], [w, h])
                    {
                        seen.push(id.to_string());
                        let monitor = it.next().filter(|m| !m.is_empty()).map(String::from);
                        layout.floating.push(Float { id: id.to_string(), pos, size, from, monitor });
                    }
                }
                continue;
            }
            let Some(i) = DockSide::parse(zone).and_then(zone_index) else { continue };
            for raw in ids.split(',') {
                let raw = raw.trim();
                let (id, front) = raw.strip_suffix('*').map_or((raw, false), |r| (r, true));
                if !is_movable(id) || seen.iter().any(|s| s == id) {
                    continue;
                }
                seen.push(id.to_string());
                layout.tabs[i].push(id.to_string());
                if front {
                    layout.active[i] = Some(id.to_string());
                }
            }
        }
        for p in REGISTRY.iter().filter(|p| p.default_dock != DockSide::Center && !seen.iter().any(|s| s == p.id)) {
            if let Some(i) = zone_index(p.default_dock) {
                layout.tabs[i].push(p.id.to_string());
            }
        }
        for part in sizes.split(',') {
            if let Some((zone, n)) = part.split_once(':')
                && let (Some(z), Ok(n)) = (DockSide::parse(zone), n.trim().parse::<f32>())
                && (ZONE_SIZE_RANGE.0..=ZONE_SIZE_RANGE.1).contains(&n)
            {
                layout.set_size(z, n);
            }
        }
        layout
    }

    /// For the control channel: `zone.left=history,selection zone.right=- zone.bottom=-
    /// zone.center=waveform,spectral front.left=history front.right=- front.bottom=- maximized=- floating=history float.history=x,y,w,h`.
    /// Only shown panels are listed, in tab order.
    pub fn report(&self, set: &PanelSet) -> String {
        let list = |v: Vec<&str>| if v.is_empty() { "-".to_string() } else { v.join(",") };
        let mut out: Vec<String> = [DockSide::Left, DockSide::Right, DockSide::Bottom, DockSide::Center]
            .iter()
            .map(|z| format!("zone.{}={}", z.name(), list(self.visible(*z, set))))
            .collect();
        for z in ZONES {
            out.push(format!("front.{}={}", z.name(), self.active_in(z, set).unwrap_or("-")));
        }
        out.push(format!("maximized={}", self.maximized(set).unwrap_or("-")));
        let floats = self.floats(set);
        out.push(format!("floating={}", list(floats.iter().map(|f| f.id.as_str()).collect())));
        for f in floats {
            out.push(format!("float.{}={},{},{},{}", f.id, f.pos[0], f.pos[1], f.size[0], f.size[1]));
            if let Some(m) = &f.monitor {
                out.push(format!("floatmon.{}={m}", f.id));
            }
        }
        out.join(" ")
    }

    /// Carry out `move <id> <zone>`, `maximize <id>`, `close <id>` or `activate <id>` (the words
    /// after the control verb `PANEL`) on the layout and the shown set. Returns the control
    /// channel's answer text, or the reason it was refused.
    pub fn apply_command(&mut self, set: &mut PanelSet, text: &str) -> Result<(), String> {
        let mut words = text.split_whitespace();
        let (verb, id) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        let known = |id: &str| by_id(id).map(|_| ()).ok_or_else(|| format!("no panel with the id {id:?}"));
        match verb {
            "move" => {
                let zone = words.next().ok_or("PANEL move <id> <left|right|bottom>")?;
                let zone = DockSide::parse(zone).ok_or_else(|| format!("no zone named {zone:?}; use left, right or bottom"))?;
                self.move_to(id, zone, None)?;
                set.set(id, true);
                Ok(())
            }
            "float" => {
                known(id)?;
                let rest: Vec<&str> = words.collect();
                let rect = match rest[..] {
                    [] => None,
                    [x, y, w, h] => {
                        let n: Vec<f32> = [x, y, w, h].iter().filter_map(|v| v.parse().ok()).collect();
                        let [x, y, w, h] = n[..] else { return Err("PANEL float <id> [<x> <y> <w> <h>], in screen points".into()) };
                        Some(([x, y], [w, h]))
                    }
                    _ => return Err("PANEL float <id> [<x> <y> <w> <h>], in screen points".into()),
                };
                self.float(id, rect)?;
                set.set(id, true);
                Ok(())
            }
            "dock" => {
                known(id)?;
                let zone = match words.next() {
                    Some(z) => Some(
                        DockSide::parse(z)
                            .filter(|z| zone_index(*z).is_some())
                            .ok_or_else(|| format!("no zone named {z:?}; use left, right or bottom"))?,
                    ),
                    None => None,
                };
                self.dock(id, zone)?;
                set.set(id, true);
                Ok(())
            }
            "maximize" => {
                known(id)?;
                if self.is_floating(id) {
                    return Err("dock the panel first; a floating panel is not maximised".into());
                }
                set.set(id, true);
                self.toggle_maximize(id);
                Ok(())
            }
            "close" => {
                known(id)?;
                set.set(id, false);
                if self.maximized.as_deref() == Some(id) {
                    self.maximized = None;
                }
                Ok(())
            }
            "activate" => {
                known(id)?;
                if self.is_floating(id) {
                    return Ok(());
                }
                if self.activate(id) { Ok(()) } else { Err("that panel is in the centre".into()) }
            }
            _ => Err("PANEL move <id> <zone> | float <id> [x y w h] | dock <id> [zone] | maximize <id> | close <id> | activate <id>".into()),
        }
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
        assert_eq!(s.report(), "waveform=1 spectral=1 history=1 selection=1 freq=1 markers=1 playlist=1 properties=1 transport=1");
        assert_eq!(s.apply_action("window.toggle.history"), Some(Ok(false)));
        assert_eq!(s.report(), "waveform=1 spectral=1 history=0 selection=1 freq=1 markers=1 playlist=1 properties=1 transport=1");
        assert_eq!(s.apply_action("window.show.history"), Some(Ok(true)));
        assert_eq!(s.apply_action("window.hide.spectral"), Some(Ok(false)));
        assert!(matches!(s.apply_action("window.toggle.nope"), Some(Err(_))));
        assert_eq!(s.apply_action("edit.delete"), None);
        assert_eq!(s.apply_action("window.bogus.history"), None);
    }

    fn all() -> PanelSet {
        PanelSet::default()
    }

    #[test]
    fn the_default_layout_follows_the_registry() {
        let (l, set) = (Layout::default(), all());
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history", "selection", "markers", "playlist"]);
        assert!(l.visible(DockSide::Left, &set).is_empty());
        assert_eq!(l.visible(DockSide::Center, &set), vec!["waveform", "spectral"]);
        assert_eq!(l.zone_of("history"), Some(DockSide::Right));
        assert_eq!(l.zone_of("waveform"), Some(DockSide::Center));
        assert_eq!(l.zone_of("nope"), None);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"));
    }

    #[test]
    fn a_panel_moves_between_zones_and_into_a_tab_position() {
        let (mut l, set) = (Layout::default(), all());
        l.move_to("history", DockSide::Left, None).unwrap();
        assert_eq!(l.visible(DockSide::Left, &set), vec!["history"]);
        assert_eq!(l.visible(DockSide::Right, &set), vec!["selection", "markers", "playlist"]);
        l.move_to("selection", DockSide::Left, Some(0)).unwrap();
        assert_eq!(l.visible(DockSide::Left, &set), vec!["selection", "history"]);
        assert_eq!(l.active_in(DockSide::Left, &set), Some("selection"));
        l.move_to("history", DockSide::Left, Some(0)).unwrap();
        assert_eq!(l.visible(DockSide::Left, &set), vec!["history", "selection"]);
        assert!(l.move_to("waveform", DockSide::Left, None).is_err(), "the editor stays");
        assert!(l.move_to("history", DockSide::Center, None).is_err());
        assert!(l.move_to("nope", DockSide::Left, None).is_err());
    }

    #[test]
    fn hidden_tabs_are_skipped_but_keep_their_place() {
        let (mut l, mut set) = (Layout::default(), all());
        l.activate("selection");
        set.set("selection", false);
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history", "markers", "playlist"]);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"), "a hidden front tab gives way");
        set.set("selection", true);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("selection"));
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history", "selection", "markers", "playlist"]);
    }

    #[test]
    fn cycling_wraps_over_the_shown_tabs() {
        let (mut l, set) = (Layout::default(), all());
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("selection"));
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("markers"));
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("playlist"));
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"));
        l.cycle(DockSide::Right, &set, -1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("playlist"));
        l.cycle(DockSide::Left, &set, 1);
    }

    #[test]
    fn maximise_toggles_and_a_closed_panel_is_no_longer_maximised() {
        let (mut l, mut set) = (Layout::default(), all());
        assert_eq!(l.toggle_maximize("spectral"), Some(true));
        assert_eq!(l.maximized(&set), Some("spectral"));
        assert_eq!(l.toggle_maximize("spectral"), Some(false));
        assert_eq!(l.maximized(&set), None);
        l.toggle_maximize("history");
        set.set("history", false);
        assert_eq!(l.maximized(&set), None);
        assert_eq!(l.toggle_maximize("nope"), None);
    }

    #[test]
    fn the_layout_round_trips_through_the_settings_text() {
        let mut l = Layout::default();
        l.move_to("history", DockSide::Bottom, None).unwrap();
        l.move_to("selection", DockSide::Left, None).unwrap();
        assert!(l.set_size(DockSide::Bottom, 300.0));
        assert!(!l.set_size(DockSide::Bottom, 300.4));
        l.toggle_maximize("history");
        let (tabs, sizes) = l.encode();
        assert_eq!(tabs, "left:selection*;right:markers,playlist;bottom:freq,properties,transport,history*");
        assert_eq!(sizes, "left:220,right:240,bottom:300");
        let back = Layout::decode(&tabs, &sizes);
        l.restore();
        assert_eq!(back, l, "the maximised panel is not kept");
    }

    #[test]
    fn decoding_ignores_nonsense_and_places_missing_panels() {
        let l = Layout::decode("left:waveform,history,history,gone;moon:selection;right:", "left:5,bottom:abc,right:nan,x:1");
        let set = all();
        assert_eq!(l.visible(DockSide::Left, &set), vec!["history"]);
        assert_eq!(l.visible(DockSide::Right, &set), vec!["selection", "markers", "playlist"], "the missing panel returns to its default zone");
        assert_eq!(l.size(DockSide::Left), Some(220.0), "out of range: default");
        assert_eq!(Layout::decode("", ""), Layout::default());
        assert_eq!(Layout::decode("garbage", ",,"), Layout::default());
    }

    #[test]
    fn commands_move_maximise_and_close() {
        let (mut l, mut set) = (Layout::default(), all());
        l.apply_command(&mut set, "move history bottom").unwrap();
        assert_eq!(l.zone_of("history"), Some(DockSide::Bottom));
        assert!(l.apply_command(&mut set, "move history moon").is_err());
        assert!(l.apply_command(&mut set, "move waveform left").is_err());
        assert!(l.apply_command(&mut set, "move history").is_err());
        l.apply_command(&mut set, "maximize history").unwrap();
        assert_eq!(l.maximized(&set), Some("history"));
        l.apply_command(&mut set, "close history").unwrap();
        assert!(!set.is_shown("history"));
        assert_eq!(l.maximized(&set), None);
        l.apply_command(&mut set, "move history left").unwrap();
        assert!(set.is_shown("history"), "moving a closed panel opens it there");
        assert!(l.apply_command(&mut set, "close nope").is_err());
        assert!(l.apply_command(&mut set, "dance").is_err());
    }

    #[test]
    fn the_report_names_zones_tabs_and_the_maximised_panel() {
        let (mut l, mut set) = (Layout::default(), all());
        assert_eq!(
            l.report(&set),
            "zone.left=- zone.right=history,selection,markers,playlist zone.bottom=freq,properties,transport zone.center=waveform,spectral front.left=- front.right=history front.bottom=freq maximized=- floating=-"
        );
        l.apply_command(&mut set, "move selection left").unwrap();
        l.apply_command(&mut set, "maximize selection").unwrap();
        assert!(l.report(&set).starts_with("zone.left=selection zone.right=history,markers,playlist "));
        assert!(l.report(&set).contains("maximized=selection"));
    }

    #[test]
    fn a_panel_floats_with_a_rect_and_docks_back() {
        let set = all();
        let mut l = Layout::default();
        assert!(l.float("history", None).is_ok());
        assert!(l.is_floating("history") && l.zone_of("history").is_none());
        assert!(!l.visible(DockSide::Left, &set).contains(&"history"));
        assert_eq!(l.floats(&set).len(), 1);
        assert!(l.set_float_rect("history", [300.4, 80.0], [400.0, 250.0]));
        assert!(!l.set_float_rect("history", [300.0, 80.0], [400.0, 250.0]), "a sub-point change is no change");
        assert_eq!(l.floats(&set)[0].pos, [300.0, 80.0]);
        l.dock("history", None).unwrap();
        assert_eq!(l.zone_of("history"), Some(DockSide::Right));
        assert!(!l.is_floating("history"));
        l.float("history", None).unwrap();
        l.dock("history", Some(DockSide::Bottom)).unwrap();
        assert_eq!(l.zone_of("history"), Some(DockSide::Bottom));
    }

    #[test]
    fn editor_panels_and_unknown_ids_cannot_float() {
        let mut l = Layout::default();
        assert!(l.float("waveform", None).is_err());
        assert!(l.float("nope", None).is_err());
        assert!(l.float("history", Some(([f32::NAN, 0.0], [10.0, 10.0]))).is_ok(), "a bad rect falls back to the default");
        assert_eq!(l.floating[0].size, DEFAULT_FLOAT_SIZE);
    }

    #[test]
    fn floating_state_round_trips_and_moving_a_floating_panel_docks_it() {
        let mut l = Layout::default();
        l.float("selection", Some(([-40.0, 90.0], [333.0, 222.0]))).unwrap();
        l.move_to("history", DockSide::Bottom, None).unwrap();
        let (tabs, sizes) = l.encode();
        assert!(tabs.contains("float:selection@-40_90_333_222_"), "{tabs}");
        let back = Layout::decode(&tabs, &sizes);
        assert_eq!(back, l);
        assert_eq!(back.encode(), (tabs, sizes));
        l.move_to("selection", DockSide::Left, None).unwrap();
        assert!(!l.is_floating("selection") && !l.encode().0.contains("float:"));
    }

    #[test]
    fn decoding_ignores_bad_float_parts() {
        let l = Layout::decode(
            "bottom:history;float:waveform@1_2_300_300_left,ghost@1_2_3_4_left,selection@x_2_300_300_left,history@1_2_300_300_left",
            "",
        );
        assert!(l.floating.is_empty(), "{:?}", l.floating);
        assert_eq!(l.zone_of("selection"), Some(DockSide::Right));
        assert_eq!(l.zone_of("history"), Some(DockSide::Bottom));
        let l = Layout::decode("float:selection@5_6_7_8_moon", "");
        assert_eq!(l.floating[0].size, [FLOAT_SIZE_RANGE.0, FLOAT_SIZE_RANGE.0]);
        assert_eq!(l.floating[0].from, DockSide::Right);
        assert_eq!(l.zone_of("history"), Some(DockSide::Right));
    }

    #[test]
    fn float_and_dock_commands_and_the_report() {
        let (mut set, mut l) = (all(), Layout::default());
        l.apply_command(&mut set, "float history").unwrap();
        let r = l.report(&set);
        assert!(r.contains("floating=history") && r.contains("float.history=120,120,320,260"), "{r}");
        assert!(r.contains("zone.right=selection"), "{r}");
        assert!(l.apply_command(&mut set, "maximize history").is_err());
        assert!(l.apply_command(&mut set, "float waveform").is_err());
        assert!(l.apply_command(&mut set, "dock history moon").is_err());
        l.apply_command(&mut set, "dock history bottom").unwrap();
        let r = l.report(&set);
        assert!(r.contains("floating=- ") || r.ends_with("floating=-"), "{r}");
        assert!(r.contains("zone.bottom=freq,properties,transport,history"), "{r}");
        l.apply_command(&mut set, "float selection").unwrap();
        l.apply_command(&mut set, "dock selection").unwrap();
        assert_eq!(l.zone_of("selection"), Some(DockSide::Right));
        assert_eq!(l.float_group(DockSide::Bottom, &set), 4, "freq, properties, transport and history are all in the bottom zone");
    }

    fn mons(text: &str) -> Vec<Monitor> {
        monitors::parse(text).unwrap()
    }

    #[test]
    fn a_floating_window_remembers_its_monitor_in_the_layout_text() {
        let both = mons("main:0,0,1920,1080;side:1920,0,1280,1024");
        let mut l = Layout::default();
        l.float("history", Some(([2000.0, 100.0], [320.0, 260.0]))).unwrap();
        l.float("selection", Some(([100.0, 100.0], [320.0, 260.0]))).unwrap();
        assert!(l.tag_monitors(&both));
        assert!(!l.tag_monitors(&both));
        let (tabs, sizes) = l.encode();
        assert!(tabs.contains("history@2000_100_320_260_right_side"), "{tabs}");
        assert!(tabs.contains("selection@100_100_320_260_right_main"), "{tabs}");
        let back = Layout::decode(&tabs, &sizes);
        assert_eq!(back, l);
        // Old text without a monitor still decodes (no monitor noted).
        let old = Layout::decode("left:;right:;bottom:;float:history@5_5_300_200_left", "");
        assert_eq!(old.floats(&PanelSet::default())[0].monitor, None);
    }

    #[test]
    fn a_window_whose_monitor_is_gone_moves_onto_the_primary_and_stays_inside() {
        let both = mons("main:0,0,1920,1080;side:1920,0,1280,1024");
        let mut l = Layout::default();
        l.float("history", Some(([2800.0, 700.0], [320.0, 260.0]))).unwrap();
        l.float("selection", Some(([100.0, 100.0], [320.0, 260.0]))).unwrap();
        l.tag_monitors(&both);
        assert_eq!(l.reconcile_monitors(&both), 0, "nothing moves while both are there");
        let only_main = mons("main:0,0,1920,1080");
        assert_eq!(l.reconcile_monitors(&only_main), 1);
        let set = PanelSet::default();
        let f = l.floats(&set);
        assert_eq!((f[0].pos, f[0].monitor.as_deref()), ([1600.0, 700.0], Some("main")));
        assert_eq!((f[1].pos, f[1].monitor.as_deref()), ([100.0, 100.0], Some("main")));
        // The primary need not be the first one the window knew, nor the one at the origin.
        let mut l2 = Layout::default();
        l2.float("history", Some(([10.0, 10.0], [320.0, 260.0]))).unwrap();
        l2.tag_monitors(&both);
        let other = mons("tv:-1000,200,800,600");
        assert_eq!(l2.reconcile_monitors(&other), 1);
        assert_eq!(l2.floats(&set)[0].pos, [-520.0, 200.0]);
    }

    #[test]
    fn an_untagged_window_is_tagged_by_where_it_is_and_float_takes_a_rect_in_the_command() {
        let mut l = Layout::default();
        let mut set = PanelSet::default();
        l.apply_command(&mut set, "float history 2000 100 300 200").unwrap();
        assert!(l.apply_command(&mut set, "float selection 1 2 3").is_err());
        assert!(l.apply_command(&mut set, "float selection 1 2 x 4").is_err());
        assert_eq!(l.floats(&set)[0].pos, [2000.0, 100.0]);
        let both = mons("main:0,0,1920,1080;side:1920,0,1280,1024");
        l.reconcile_monitors(&both);
        let f = l.floats(&set)[0];
        assert_eq!((f.pos, f.monitor.as_deref()), ([2000.0, 100.0], Some("side")));
        assert!(l.report(&set).contains("floatmon.history=side"));
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
        assert_eq!(ids(menu("history", false)), vec!["close", "maximize", "clear_history"]);
        assert_eq!(menu("history", true)[1].label, "Restore");
        assert_eq!(menu("waveform", false)[1].label, "Maximize");
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
