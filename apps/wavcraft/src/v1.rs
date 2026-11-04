//! What a release build offers (M5; docs/v1/v1.md, "Deliberately not in v1").
//!
//! The features v1 rules out keep their code, but a release build lists none of their menus or
//! panels. A debug build lists them all, so the work on them goes on as before. The panels are
//! gated in `wavcraft_core::panels::PanelSet`; this module gates the menus and the Tools list.

use wavcraft_core::panels::{self, PanelSet};

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
}

impl Feature {
    #[cfg(test)]
    pub const ALL: [Feature; 6] = [Feature::Multitrack, Feature::CdLayout, Feature::Video, Feature::Batch, Feature::MixPaste, Feature::Diagnostics];

    /// The menu text the feature is listed under.
    pub fn menu_text(self) -> &'static str {
        match self {
            Feature::Multitrack => "Multitrack",
            Feature::CdLayout => "CD Layout",
            Feature::Video => "Video",
            Feature::Batch => "Batch Process…",
            Feature::MixPaste => "Mix Paste…",
            Feature::Diagnostics => "Diagnostics…",
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
