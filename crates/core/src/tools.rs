//! The tool model (roadmap A049, A057): which tool the pointer uses in the editor. Time
//! Selection and Marquee exist; Move, Lasso and Paintbrush are listed, disabled, so the bar has
//! room for them. Keys are Audition's single-key defaults, for the tools that exist.

/// A tool of the tools bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    /// Drag to select a time range; click to place the cursor.
    #[default]
    TimeSelection,
    /// Not built yet (listed disabled).
    Move,
    /// Drag a rectangle of time and frequency on the spectral display (B135).
    Marquee,
    /// Not built yet (listed disabled).
    Lasso,
    /// Not built yet (listed disabled).
    Paintbrush,
}

/// Every tool, in the bar's order.
pub const ALL: &[Tool] = &[Tool::TimeSelection, Tool::Move, Tool::Marquee, Tool::Lasso, Tool::Paintbrush];

impl Tool {
    /// The name STATE and the control channel use.
    pub fn name(self) -> &'static str {
        match self {
            Tool::TimeSelection => "time_selection",
            Tool::Move => "move",
            Tool::Marquee => "marquee",
            Tool::Lasso => "lasso",
            Tool::Paintbrush => "paintbrush",
        }
    }

    /// The label on the bar.
    pub fn label(self) -> &'static str {
        match self {
            Tool::TimeSelection => "Time Selection",
            Tool::Move => "Move",
            Tool::Marquee => "Marquee",
            Tool::Lasso => "Lasso",
            Tool::Paintbrush => "Paintbrush",
        }
    }

    /// Whether the tool can be picked yet.
    pub fn enabled(self) -> bool {
        matches!(self, Tool::TimeSelection | Tool::Marquee)
    }

    /// The tool's single key (egui key name), where it has one that works.
    pub fn key(self) -> Option<&'static str> {
        match self {
            Tool::TimeSelection => Some("T"),
            Tool::Marquee => Some("E"),
            _ => None,
        }
    }

    /// The tool with this STATE name.
    pub fn by_name(name: &str) -> Option<Tool> {
        ALL.iter().copied().find(|t| t.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_selection_is_the_default_and_with_marquee_the_only_enabled_tool() {
        assert_eq!(Tool::default(), Tool::TimeSelection);
        assert_eq!(ALL.iter().filter(|t| t.enabled()).count(), 2);
        assert!(Tool::Marquee.enabled());
        assert!(Tool::TimeSelection.enabled());
        assert!(!Tool::Move.enabled());
    }

    #[test]
    fn names_round_trip_and_keys_are_unique() {
        for t in ALL {
            assert_eq!(Tool::by_name(t.name()), Some(*t));
        }
        assert_eq!(Tool::by_name("nothing"), None);
        assert_eq!(Tool::TimeSelection.key(), Some("T"));
        let keys: Vec<_> = ALL.iter().filter_map(|t| t.key()).collect();
        let mut dedup = keys.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(keys.len(), dedup.len());
    }
}
