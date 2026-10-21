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
}
