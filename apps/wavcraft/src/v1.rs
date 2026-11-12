//! What a release build offers (M5; docs/v1/v1.md, "Deliberately not in v1").
//!
//! The features v1 rules out keep their code, but a release build lists none of their menus or
//! panels. A debug build lists them all, so the work on them goes on as before. The panels are
//! gated in `wavcraft_core::panels::PanelSet` and the effects in `wavcraft_core::effects::listed`;
//! this module gates the menus, the Tools list, the Keyboard Shortcuts list and the Effects Rack's plugins.

/// Whether this is a release build.
pub const RELEASE: bool = !cfg!(debug_assertions);

/// A feature of the window that v1 rules out. Its menu text is the entry a release build does not list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// The Multitrack menu, and its File entries (New Multitrack Session, the Session saves and open).
    Multitrack,
    /// View › CD Layout (and the Multitrack editor's View entry).
    CdLayout,
    /// The Video menu.
    Video,
    /// File › Batch Process….
    Batch,
    /// Edit › Mix Paste….
    MixPaste,
    /// Effects › Diagnostics…, and the DeClipper, DeClicker and Automatic Click Remover entries.
    Diagnostics,
    /// Third-party plugins in the Effects Rack: the Scan VST3 button and the slot picker's VST3 plugins.
    Plugins,
}

impl Feature {
    #[cfg(test)]
    pub const ALL: [Feature; 7] =
        [Feature::Multitrack, Feature::CdLayout, Feature::Video, Feature::Batch, Feature::MixPaste, Feature::Diagnostics, Feature::Plugins];

    /// The menu text the feature is listed under.
    pub fn menu_text(self) -> &'static str {
        match self {
            Feature::Multitrack => "Multitrack",
            Feature::CdLayout => "CD Layout",
            Feature::Video => "Video",
            Feature::Batch => "Batch Process…",
            Feature::MixPaste => "Mix Paste…",
            Feature::Diagnostics => "Diagnostics…",
            Feature::Plugins => "VST3 plugins",
        }
    }
}

/// Whether a build offers `f`: a debug build offers every feature, a release build none of them.
pub fn offered_in(release: bool, _feature: Feature) -> bool {
    !release
}

/// Whether this build offers `f`.
pub fn offered(feature: Feature) -> bool {
    offered_in(RELEASE, feature)
}

/// Whether a build lists the Tools entry `name`: the planned Spot Healing tool is out of v1.
pub fn tool_offered_in(release: bool, name: &str) -> bool {
    !(release && name == "tool.spotHealing")
}

/// The Diagnostics commands (Feature::Diagnostics) by their action names.
const DIAGNOSTICS_COMMANDS: &[&str] = &["diagnostics.declip", "diagnostics.declick", "diagnostics.autoClickRemover"];

/// Whether a build lists the command `name` (an action or shortcut name) in the Keyboard
/// Shortcuts list: a release build leaves out the Diagnostics commands and the out-of-v1 effects.
pub fn command_offered_in(release: bool, name: &str) -> bool {
    let effect_out = name.strip_prefix("effect.").is_some_and(|id| wavcraft_core::effects::OUT_OF_V1.contains(&id));
    let diag_out = DIAGNOSTICS_COMMANDS.contains(&name) && !offered_in(release, Feature::Diagnostics);
    !(release && effect_out) && !diag_out && tool_offered_in(release, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavcraft_core::panels::{self, PanelSet};

    #[test]
    fn a_release_build_offers_no_out_of_v1_feature() {
        for f in Feature::ALL {
            assert!(!offered_in(true, f), "{}", f.menu_text());
            assert!(offered_in(false, f), "a debug build offers {}", f.menu_text());
        }
    }

    #[test]
    fn a_release_build_lists_no_out_of_v1_panel_or_tool() {
        let set = PanelSet::for_build(true);
        // The Window menu lists every panel the build offers; none of them is out of v1.
        let window: Vec<&str> = panels::REGISTRY.iter().filter(|p| set.offers(p.id)).map(|p| p.title).collect();
        let out: Vec<&str> =
            Feature::ALL.iter().map(|f| f.menu_text()).chain(panels::OUT_OF_V1.iter().filter_map(|id| panels::by_id(id)).map(|p| p.title)).collect();
        assert!(!window.is_empty());
        for title in &window {
            assert!(!out.contains(title), "{title} is out of v1 but listed");
        }
        let freq = panels::by_id("freq").expect("the frequency panel is registered").title;
        assert!(window.iter().all(|t| *t != freq), "{freq} is out of v1 but listed");
        assert!(PanelSet::for_build(false).offers("freq"), "a debug build offers the frequency panel");

        // The Tools panel lists the tools the build offers.
        assert!(super::super::menus::TOOLS.iter().any(|t| t.label == "Spot Healing"), "the planned tool is in the list");
        let tools: Vec<&str> = super::super::menus::TOOLS.iter().filter(|t| tool_offered_in(true, t.name)).map(|t| t.label).collect();
        assert!(!tools.contains(&"Spot Healing") && tools.contains(&"Time Selection"), "{tools:?}");
        assert!(super::super::menus::TOOLS.iter().any(|t| tool_offered_in(false, t.name) && t.label == "Spot Healing"));
    }

    #[test]
    fn plugins_are_a_feature_a_release_build_does_not_offer() {
        assert_eq!(Feature::ALL.len(), 7);
        assert!(Feature::ALL.contains(&Feature::Plugins));
        assert!(!offered_in(true, Feature::Plugins));
        assert!(offered_in(false, Feature::Plugins));
    }

    #[test]
    fn a_release_shortcut_list_has_no_out_of_v1_command() {
        let out = ["diagnostics.declip", "diagnostics.declick", "diagnostics.autoClickRemover", "effect.dehummer", "effect.dereverb"];
        for n in out {
            assert!(!command_offered_in(true, n), "{n}");
            assert!(command_offered_in(false, n), "a debug build lists {n}");
        }
        for n in ["edit.delete", "effect.reverse", "effect.noise_reduction", "file.open"] {
            assert!(command_offered_in(true, n) && command_offered_in(false, n), "{n}");
        }
    }

    /// The words of the "Deliberately not in v1" table, as the window would show them.
    const OUT: &[&str] = &[
        "Multitrack",
        "CD Layout",
        "Video",
        "Batch Process",
        "Mix Paste",
        "Diagnostics",
        "DeClipper",
        "DeClicker",
        "Click Remover",
        "DeHummer",
        "DeReverb",
        "Frequency Analysis",
        "Spot Healing",
        "VST3",
    ];

    /// What a build lists outside the menu bar's own text: the gated menu entries, the Window
    /// list, the Tools panel, the Effects Rack's slot picker and the Keyboard Shortcuts list.
    fn listed(release: bool) -> Vec<String> {
        let mut v: Vec<String> = Feature::ALL.iter().filter(|f| offered_in(release, **f)).map(|f| f.menu_text().to_string()).collect();
        let set = PanelSet::for_build(release);
        v.extend(panels::REGISTRY.iter().filter(|p| set.offers(p.id)).map(|p| p.title.to_string()));
        v.extend(super::super::menus::TOOLS.iter().filter(|t| tool_offered_in(release, t.name)).map(|t| t.label.to_string()));
        v.extend(crate::rack_panel::slot_effects(release).iter().map(|e| e.name().to_string()));
        v.extend(crate::rack_panel::slot_plugins(release));
        v.extend(crate::shortcut_editor::listed(release).map(|c| c.label().to_string()));
        v
    }

    #[test]
    fn release_lists_nothing_deliberately_not_in_v1() {
        let (release, debug) = (listed(true), listed(false));
        assert!(release.iter().any(|l| l == "Noise Reduction") && release.iter().any(|l| l == "Markers"), "{release:?}");
        for word in OUT {
            assert!(!release.iter().any(|l| l.contains(word)), "a release build lists {word}: {release:?}");
            assert!(debug.iter().any(|l| l.contains(word)), "a debug build lists {word} (its code stays)");
        }
    }
}
