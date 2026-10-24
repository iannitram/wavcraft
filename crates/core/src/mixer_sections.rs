//! Show and hide mixer sections (roadmap C166). A mixer strip has four groups of rows: the EQ,
//! the effects, the sends and the I/O (input and output) rows. The user can hide any of them to
//! make the strips shorter. This file is the core of that: which sections are shown, how to flip
//! one, and a short text form to keep the choice in a settings file.
//!
//! The text form lists the shown sections by id, in the fixed order eq, effects, sends, io, joined
//! by commas ("eq,sends"). No shown section is written as "none". Reading the text ignores case,
//! spaces around items and repeated items, and refuses an empty text, an empty item and any id it
//! does not know. Drawing the strips is not here.

/// One group of rows on a mixer strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerSection {
    Eq,
    Effects,
    Sends,
    Io,
}

impl MixerSection {
    /// Every section, in the fixed order used for drawing and for the text form.
    pub const ALL: [MixerSection; 4] = [MixerSection::Eq, MixerSection::Effects, MixerSection::Sends, MixerSection::Io];

    /// The short lower-case id used in the text form.
    pub fn id(self) -> &'static str {
        match self {
            MixerSection::Eq => "eq",
            MixerSection::Effects => "effects",
            MixerSection::Sends => "sends",
            MixerSection::Io => "io",
        }
    }

    /// The name shown on the menu.
    pub fn label(self) -> &'static str {
        match self {
            MixerSection::Eq => "EQ",
            MixerSection::Effects => "Effects",
            MixerSection::Sends => "Sends",
            MixerSection::Io => "I/O",
        }
    }

    /// The section with this id (any case, spaces around it ignored), or `None`.
    pub fn from_id(text: &str) -> Option<MixerSection> {
        let t = text.trim();
        MixerSection::ALL.into_iter().find(|s| s.id().eq_ignore_ascii_case(t))
    }

    fn bit(self) -> u8 {
        match self {
            MixerSection::Eq => 1,
            MixerSection::Effects => 2,
            MixerSection::Sends => 4,
            MixerSection::Io => 8,
        }
    }
}

/// Which mixer sections are shown. The default shows all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixerSections {
    shown: u8,
}

impl Default for MixerSections {
    fn default() -> Self {
        MixerSections::all_shown()
    }
}

impl MixerSections {
    /// All four sections shown.
    pub fn all_shown() -> MixerSections {
        MixerSections { shown: 0b1111 }
    }

    /// No section shown.
    pub fn none_shown() -> MixerSections {
        MixerSections { shown: 0 }
    }

    /// Whether this section is shown.
    pub fn is_shown(self, section: MixerSection) -> bool {
        self.shown & section.bit() != 0
    }

    /// Show or hide one section. Setting a state it already has changes nothing.
    pub fn set_shown(&mut self, section: MixerSection, on: bool) {
        if on {
            self.shown |= section.bit();
        } else {
            self.shown &= !section.bit();
        }
    }

    /// Flip one section and return its new state (true = now shown).
    pub fn toggle(&mut self, section: MixerSection) -> bool {
        let on = !self.is_shown(section);
        self.set_shown(section, on);
        on
    }

    /// How many sections are shown (0 to 4).
    pub fn shown_count(self) -> usize {
        MixerSection::ALL.into_iter().filter(|s| self.is_shown(*s)).count()
    }

    /// The shown sections, in the fixed order of [`MixerSection::ALL`].
    pub fn shown(self) -> Vec<MixerSection> {
        MixerSection::ALL.into_iter().filter(|s| self.is_shown(*s)).collect()
    }

    /// The text form: shown ids joined by commas, or "none".
    pub fn to_text(self) -> String {
        if self.shown == 0 {
            return "none".to_string();
        }
        self.shown().into_iter().map(MixerSection::id).collect::<Vec<_>>().join(",")
    }

    /// Read the text form (see the module comment). Any refusal is a message for the user.
    pub fn from_text(text: &str) -> Result<MixerSections, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("a section list is empty: write none to hide every section".to_string());
        }
        let mut out = MixerSections::none_shown();
        if t.eq_ignore_ascii_case("none") {
            return Ok(out);
        }
        for raw in t.split(',') {
            let tok = raw.trim();
            if tok.is_empty() {
                return Err("a section list has an empty item".to_string());
            }
            match MixerSection::from_id(tok) {
                Some(s) => out.set_shown(s, true),
                None => return Err(format!("unknown mixer section {tok:?}: use eq, effects, sends or io")),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shows_all_four() {
        let s = MixerSections::default();
        assert_eq!(s, MixerSections::all_shown());
        assert_eq!(s.shown_count(), 4);
        assert_eq!(s.shown(), MixerSection::ALL.to_vec());
    }

    #[test]
    fn toggle_returns_new_state_and_flips_only_that_section() {
        let mut s = MixerSections::default();
        assert!(!s.toggle(MixerSection::Sends));
        assert!(!s.is_shown(MixerSection::Sends));
        assert!(s.is_shown(MixerSection::Eq));
        assert!(s.is_shown(MixerSection::Effects));
        assert!(s.is_shown(MixerSection::Io));
        assert!(s.toggle(MixerSection::Sends));
        assert_eq!(s, MixerSections::all_shown());
    }

    #[test]
    fn set_shown_is_idempotent() {
        let mut s = MixerSections::default();
        s.set_shown(MixerSection::Eq, false);
        s.set_shown(MixerSection::Eq, false);
        assert_eq!(s.shown(), vec![MixerSection::Effects, MixerSection::Sends, MixerSection::Io]);
        s.set_shown(MixerSection::Eq, true);
        s.set_shown(MixerSection::Eq, true);
        assert_eq!(s, MixerSections::all_shown());
    }

    #[test]
    fn shown_keeps_the_fixed_order() {
        let mut s = MixerSections::none_shown();
        s.set_shown(MixerSection::Io, true);
        s.set_shown(MixerSection::Eq, true);
        s.set_shown(MixerSection::Sends, true);
        assert_eq!(s.shown(), vec![MixerSection::Eq, MixerSection::Sends, MixerSection::Io]);
        assert_eq!(s.shown_count(), 3);
    }

    #[test]
    fn from_id_ignores_case_and_spaces() {
        assert_eq!(MixerSection::from_id(" IO "), Some(MixerSection::Io));
        assert_eq!(MixerSection::from_id("Sends"), Some(MixerSection::Sends));
        assert_eq!(MixerSection::from_id("mixer"), None);
        assert_eq!(MixerSection::from_id("i/o"), None);
        assert_eq!(MixerSection::from_id(""), None);
    }

    #[test]
    fn ids_and_labels_are_fixed() {
        let ids: Vec<&str> = MixerSection::ALL.into_iter().map(MixerSection::id).collect();
        assert_eq!(ids, vec!["eq", "effects", "sends", "io"]);
        let labels: Vec<&str> = MixerSection::ALL.into_iter().map(MixerSection::label).collect();
        assert_eq!(labels, vec!["EQ", "Effects", "Sends", "I/O"]);
    }

    #[test]
    fn to_text_lists_shown_in_order() {
        let mut s = MixerSections::all_shown();
        assert_eq!(s.to_text(), "eq,effects,sends,io");
        s.set_shown(MixerSection::Eq, false);
        assert_eq!(s.to_text(), "effects,sends,io");
        assert_eq!(MixerSections::none_shown().to_text(), "none");
    }

    #[test]
    fn from_text_round_trips_every_choice() {
        for mask in 0u8..16 {
            let mut s = MixerSections::none_shown();
            for (i, section) in MixerSection::ALL.into_iter().enumerate() {
                s.set_shown(section, (mask >> i) & 1 == 1);
            }
            assert_eq!(MixerSections::from_text(&s.to_text()), Ok(s));
        }
    }

    #[test]
    fn from_text_ignores_case_spaces_and_repeats() {
        let s = MixerSections::from_text(" EQ , sends,eq ");
        assert_eq!(s.map(MixerSections::shown), Ok(vec![MixerSection::Eq, MixerSection::Sends]));
        assert_eq!(MixerSections::from_text(" NONE "), Ok(MixerSections::none_shown()));
    }

    #[test]
    fn from_text_refuses_bad_text() {
        assert_eq!(MixerSections::from_text(""), Err("a section list is empty: write none to hide every section".to_string()));
        assert_eq!(MixerSections::from_text("   "), Err("a section list is empty: write none to hide every section".to_string()));
        assert_eq!(MixerSections::from_text("eq,,io"), Err("a section list has an empty item".to_string()));
        assert_eq!(MixerSections::from_text("eq, ,io"), Err("a section list has an empty item".to_string()));
        assert_eq!(MixerSections::from_text("eq,mixer"), Err("unknown mixer section \"mixer\": use eq, effects, sends or io".to_string()));
        assert_eq!(MixerSections::from_text("none,eq"), Err("unknown mixer section \"none\": use eq, effects, sends or io".to_string()));
    }
}
