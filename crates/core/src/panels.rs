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
//! Closing a panel is hiding it in [`PanelSet`]. Floating windows and named workspaces are later
//! rows.

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
    PanelInfo { id: EDITOR, title: "Editor", default_dock: DockSide::Center },
    PanelInfo { id: "waveform", title: "Waveform", default_dock: DockSide::Center },
    PanelInfo { id: "spectral", title: "Spectral Frequency Display", default_dock: DockSide::Center },
    PanelInfo { id: "history", title: "History", default_dock: DockSide::Right },
    PanelInfo { id: "selection", title: "Selection", default_dock: DockSide::Bottom },
];

/// The Editor panel (roadmap A029): the area in the centre that holds the waveform and spectral
/// displays. It is always shown, cannot be closed, and can be maximised.
pub const EDITOR: &str = "editor";

/// Whether a panel can be closed (hidden): every panel but the Editor.
pub fn is_closable(id: &str) -> bool {
    by_id(id).is_some() && id != EDITOR
}

/// The registered panel with this id.
pub fn by_id(id: &str) -> Option<&'static PanelInfo> {
    REGISTRY.iter().find(|p| p.id == id)
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
            set.set(h.as_ref().trim(), false);
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

    /// Show or hide a panel. Returns false for an id that is not registered, and for hiding the
    /// Editor (it can't be closed).
    pub fn set(&mut self, id: &str, shown: bool) -> bool {
        if !shown && !is_closable(id) {
            return false;
        }
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
        if verb != "show" && !is_closable(id) && by_id(id).is_some() {
            return Some(Err("the Editor is always shown and can't be closed".into()));
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
}

impl Default for Layout {
    fn default() -> Self {
        let mut tabs: [Vec<String>; 3] = Default::default();
        for p in REGISTRY {
            if let Some(i) = zone_index(p.default_dock) {
                tabs[i].push(p.id.to_string());
            }
        }
        Layout { tabs, active: Default::default(), sizes: DEFAULT_ZONE_SIZES, maximized: None }
    }
}

impl Layout {
    /// The zone a panel sits in: `Center` for the editor panels, `None` for an unknown id.
    pub fn zone_of(&self, id: &str) -> Option<DockSide> {
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
        let at = index.unwrap_or(usize::MAX).min(self.tabs[to].len());
        self.tabs[to].insert(at, id.to_string());
        self.active[to] = Some(id.to_string());
        Ok(())
    }

    /// Maximise a panel, or restore if it already is. Returns whether it is maximised now, or
    /// `None` for an unknown id.
    pub fn toggle_maximize(&mut self, id: &str) -> Option<bool> {
        by_id(id)?;
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
    /// front tab) and `left:220,right:240,bottom:180`.
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
        (tabs, sizes)
    }

    /// The layout from the settings values. Whatever does not make sense is ignored: unknown
    /// ids, editor panels, repeats, unknown zones, sizes outside the range. A movable panel the
    /// text leaves out starts in its default zone.
    pub fn decode(tabs: &str, sizes: &str) -> Layout {
        let mut layout = Layout { tabs: Default::default(), ..Layout::default() };
        let mut seen: Vec<String> = Vec::new();
        for part in tabs.split(';') {
            let Some((zone, ids)) = part.split_once(':') else { continue };
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
    /// zone.center=waveform,spectral front.left=history front.right=- front.bottom=- maximized=-`.
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
            "maximize" => {
                known(id)?;
                set.set(id, true);
                self.toggle_maximize(id);
                Ok(())
            }
            "close" => {
                known(id)?;
                if !is_closable(id) {
                    return Err("the Editor can't be closed; maximize it instead".into());
                }
                set.set(id, false);
                if self.maximized.as_deref() == Some(id) {
                    self.maximized = None;
                }
                Ok(())
            }
            "activate" => {
                known(id)?;
                if self.activate(id) { Ok(()) } else { Err("that panel is in the centre".into()) }
            }
            _ => Err("PANEL move <id> <zone> | maximize <id> | close <id> | activate <id>".into()),
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
        assert_eq!(s.report(), "editor=1 waveform=1 spectral=1 history=1 selection=1");
        assert_eq!(s.apply_action("window.toggle.history"), Some(Ok(false)));
        assert_eq!(s.report(), "editor=1 waveform=1 spectral=1 history=0 selection=1");
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
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history"]);
        assert_eq!(l.visible(DockSide::Bottom, &set), vec!["selection"]);
        assert!(l.visible(DockSide::Left, &set).is_empty());
        assert_eq!(l.visible(DockSide::Center, &set), vec!["editor", "waveform", "spectral"]);
        assert_eq!(l.zone_of("history"), Some(DockSide::Right));
        assert_eq!(l.zone_of("waveform"), Some(DockSide::Center));
        assert_eq!(l.zone_of("nope"), None);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"));
    }

    #[test]
    fn the_editor_is_a_panel_that_cannot_be_closed_but_can_be_maximised() {
        let (mut l, mut set) = (Layout::default(), all());
        assert_eq!(by_id(EDITOR).map(|p| (p.title, p.default_dock)), Some(("Editor", DockSide::Center)));
        assert!(!is_closable(EDITOR) && is_closable("history") && !is_closable("nope"));
        assert!(!set.set(EDITOR, false) && set.is_shown(EDITOR));
        assert_eq!(set.toggle(EDITOR), None);
        assert!(matches!(set.apply_action("window.hide.editor"), Some(Err(_))));
        assert!(matches!(set.apply_action("window.toggle.editor"), Some(Err(_))));
        assert_eq!(set.apply_action("window.show.editor"), Some(Ok(true)));
        assert!(PanelSet::from_hidden(&["editor"]).is_shown(EDITOR), "a settings file can't hide it");
        assert!(l.apply_command(&mut set, "close editor").unwrap_err().contains("can't be closed"));
        assert!(l.apply_command(&mut set, "move editor left").is_err());
        l.apply_command(&mut set, "maximize editor").unwrap();
        assert_eq!(l.maximized(&set), Some("editor"));
        l.apply_command(&mut set, "maximize editor").unwrap();
        assert_eq!(l.maximized(&set), None);
    }

    #[test]
    fn a_panel_moves_between_zones_and_into_a_tab_position() {
        let (mut l, set) = (Layout::default(), all());
        l.move_to("history", DockSide::Left, None).unwrap();
        assert_eq!(l.visible(DockSide::Left, &set), vec!["history"]);
        assert_eq!(l.visible(DockSide::Bottom, &set), vec!["selection"]);
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
        l.move_to("selection", DockSide::Right, None).unwrap();
        l.activate("selection");
        set.set("selection", false);
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history"]);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"), "a hidden front tab gives way");
        set.set("selection", true);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("selection"));
        assert_eq!(l.visible(DockSide::Right, &set), vec!["history", "selection"]);
    }

    #[test]
    fn cycling_wraps_over_the_shown_tabs() {
        let (mut l, set) = (Layout::default(), all());
        l.move_to("selection", DockSide::Right, None).unwrap();
        l.activate("history");
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("selection"));
        l.cycle(DockSide::Right, &set, 1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("history"));
        l.cycle(DockSide::Right, &set, -1);
        assert_eq!(l.active_in(DockSide::Right, &set), Some("selection"));
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
        assert_eq!(tabs, "left:selection*;right:;bottom:history*");
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
        assert_eq!(l.visible(DockSide::Bottom, &set), vec!["selection"], "the missing panel returns to its default zone");
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
            "zone.left=- zone.right=history zone.bottom=selection zone.center=editor,waveform,spectral front.left=- front.right=history front.bottom=selection maximized=-"
        );
        l.apply_command(&mut set, "move selection left").unwrap();
        l.apply_command(&mut set, "maximize selection").unwrap();
        assert!(l.report(&set).starts_with("zone.left=selection zone.right=history "));
        assert!(l.report(&set).ends_with("maximized=selection"));
    }
}
