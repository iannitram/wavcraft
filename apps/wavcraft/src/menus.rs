//! The menu bar, right-click menus, toolbar, Tools panel and Save As filters as plain data.
//!
//! The app turns entries into clicks, control-channel lines or drawn buttons. Fixed menu-bar
//! rows also hold their v1 feature gates; the test inventory uses those same rows and the
//! existing action, panel, effect, tool, shortcut and save-format sources.

use wavcraft_core::actions::{self, ActionId};

use crate::app::Tool;
use crate::v1::{self, Feature};

/// A fixed menu-bar label and the feature that offers it. Translated rows hold i18n keys,
/// so the inventory and the UI resolve the same text in the current language.
#[derive(Clone, Copy)]
pub struct BarEntry {
    text: &'static str,
    translated: bool,
    pub gate: Option<Feature>,
}

impl BarEntry {
    pub fn label(self) -> String {
        if self.translated { wavcraft_core::i18n::tr(self.text) } else { self.text.to_string() }
    }

    pub fn offered_in(self, release: bool) -> bool {
        self.gate.is_none_or(|feature| v1::offered_in(release, feature))
    }
}

// Declare each row once: the UI names the row and the inventory walks the same rows.
macro_rules! bar_entries {
    ($($name:ident: ($text:expr, $translated:expr, $gate:expr)),* $(,)?) => {
        pub mod bar {
            use super::{BarEntry, Feature};
            $(pub const $name: BarEntry = BarEntry { text: $text, translated: $translated, gate: $gate };)*
        }
        #[cfg(test)]
        const BAR_ENTRIES: &[BarEntry] = &[$(bar::$name),*];
    };
}

bar_entries! {
    FILE: ("menu.file", true, None),
    OPEN: ("Open…", false, None),
    OPEN_RECENT: ("Open Recent", false, None),
    OPEN_APPEND: ("Open Append", false, None),
    TO_CURRENT: ("To Current…", false, None),
    TO_NEW: ("To New…", false, None),
    NEW: ("New", false, None),
    AUDIO_FILE: ("Audio File…", false, None),
    SAVE: ("file.save", true, None),
    SAVE_AS: ("file.save_as", true, None),
    CLOSE: ("Close", false, None),
    SAMPLE_TYPE: ("Sample Type on Save", false, None),
    KEEP_SAMPLE_TYPE: ("Keep the file's own", false, None),
    FLAC_NOTE: ("FLAC takes 16 or 24 bit; it uses 24 bit for the others.", false, None),
    INCLUDE_METADATA: ("Include markers and other metadata", false, None),
    SAVE_SELECTION: ("Save Selection As…", false, None),
    IMPORT: ("Import", false, None),
    IMPORT_FILE: ("File…", false, None),
    RAW_DATA: ("Raw Data…", false, None),
    EXPORT: ("Export", false, None),
    NEW_SESSION: ("Multitrack Session…", false, Some(Feature::Multitrack)),
    SAVE_SESSION: ("Save Session", false, Some(Feature::Multitrack)),
    SAVE_SESSION_AS: ("Save Session As…", false, Some(Feature::Multitrack)),
    OPEN_SESSION: ("Open Session…", false, Some(Feature::Multitrack)),
    SAVE_ALL: ("Save All", false, None),
    CLOSE_ALL: ("Close All", false, None),
    RUN_SCRIPT: ("Run Script…", false, None),
    BATCH: (Feature::Batch.menu_text(), false, Some(Feature::Batch)),
    NEW_RECORDING: ("New Recording…", false, None),
    RECORD_NOW: ("Record Now", false, None),
    SESSION_FOLDER: ("file.session_folder", true, None),
    DEFAULT_SESSION_FOLDER: ("file.default_session_folder", true, None),
    EXPORT_SETTINGS: ("Export Application Settings…", false, None),
    IMPORT_SETTINGS: ("Import Application Settings…", false, None),
    RESTORE_DEFAULTS: ("Restore Preference Defaults…", false, None),
    QUIT: ("file.quit", true, None),
    EDIT: ("menu.edit", true, None),
    MULTITRACK: (Feature::Multitrack.menu_text(), false, Some(Feature::Multitrack)),
    NORMALIZE_MIXDOWN: ("Normalize the mixdown to -1 dB", false, Some(Feature::Multitrack)),
    MIXDOWN_SESSION: ("Mixdown Session to New File", false, Some(Feature::Multitrack)),
    MIXDOWN_CLIPS: ("Mixdown Selected Clips to New File", false, Some(Feature::Multitrack)),
    WINDOW: ("Window", false, None),
    RESTORE_EDITOR: ("Restore Editor", false, None),
    MAXIMIZE_EDITOR: ("Maximize Editor", false, None),
    MIXER_PANEL: ("Mixer panel", false, Some(Feature::Multitrack)),
    LEVELS: ("Levels", false, None),
    METER_INPUT: ("Meter Input Signal", false, None),
    MIC_CHECK: ("Mic Check…", false, None),
    AMPLITUDE_STATS: ("Amplitude/Loudness Statistics", false, None),
    SAMPLE_STATS: ("Sample Statistics (RMS, clipping)", false, None),
    WORKSPACE: ("Workspace", false, None),
    RESET_PANELS: ("Reset Panel Layout", false, None),
    EFFECTS: ("Effects", false, None),
    FAVORITES: ("Favorites", false, None),
    GENERATE: ("Generate", false, None),
    VIEW: ("menu.view", true, None),
    WAVEFORM_EDITOR: ("Waveform editor", false, None),
    MULTITRACK_EDITOR: ("Multitrack editor", false, Some(Feature::Multitrack)),
    CD_LAYOUT: (Feature::CdLayout.menu_text(), false, Some(Feature::CdLayout)),
    RETURN_TO_SESSION: ("Return to session", false, None),
    PLAYBACK_RESAMPLING: ("Playback Resampling", false, None),
    HIGH_QUALITY: ("High quality", false, None),
    FAST: ("Fast", false, None),
    ZOOM_TO_TIME: ("Zoom to Time", false, None),
    SNAPPING: ("Snapping", false, None),
    TIME_DISPLAY: ("Time Display", false, None),
    METADATA_PANEL: ("Metadata panel", false, None),
    SYNC_SELECTION: ("Synchronise Selection Across Files", false, None),
    SYNC_ZOOM: ("Synchronise Zoom and Scroll Across Files", false, None),
    SYNC_PLAYHEAD: ("Synchronise Playhead Across Files", false, None),
    LANGUAGE: ("menu.language", true, None),
    VIDEO: (Feature::Video.menu_text(), false, Some(Feature::Video)),
    ATTACH_VIDEO: ("Attach video file…", false, Some(Feature::Video)),
    DETACH_VIDEO: ("Detach video", false, Some(Feature::Video)),
    VIDEO_PANEL: ("Video panel", false, Some(Feature::Video)),
    HELP: ("Help", false, None),
    KEYBOARD_SHORTCUTS: ("Keyboard Shortcuts", false, None),
    ABOUT: ("About WavCraft…", false, None),
    STOP: ("transport.stop", true, None),
    RESUME: ("▶ Resume", false, None),
    PLAY: ("transport.play", true, None),
    METRONOME: ("Metronome", false, None),
    METRONOME_VOLUME: ("vol ", false, None),
    METRONOME_COUNT_IN: (" bar count-in", false, None),
    STOP_RECORDING: ("■ Stop Recording", false, None),
    RESUME_RECORDING: ("● Resume", false, None),
    PAUSE_RECORDING: ("❚❚ Pause", false, None),
    CANCEL_TIMED_RECORDING: ("Cancel Timed Recording", false, None),
    RECORD: ("● Record", false, None),
    SELECT: ("Select", false, None),
    ENABLE_CHANNELS: ("Enable Channels", false, None),
    SELECT_CHANNEL: ("Select Channel by Drag Position", false, None),
    MIX_PASTE: (Feature::MixPaste.menu_text(), false, Some(Feature::MixPaste)),
    EDIT_SHORTCUTS: ("Keyboard Shortcuts…", false, None),
    PREFERENCES: ("Preferences…", false, None),
    EFFECTS_RACK: ("Effects Rack…", false, None),
    SAVE_NOISE_PRINT: ("Save Noise Print…", false, None),
    LOAD_NOISE_PRINT: ("Load Noise Print…", false, None),
    DIAGNOSTICS: (Feature::Diagnostics.menu_text(), false, Some(Feature::Diagnostics)),
    DECLIPPER: ("DeClipper (defaults)", false, Some(Feature::Diagnostics)),
    DECLICKER: ("DeClicker (defaults)", false, Some(Feature::Diagnostics)),
    CLICK_REMOVER: ("Automatic Click Remover (defaults)", false, Some(Feature::Diagnostics)),
    RECENT_NONE: ("(none)", false, None),
    CLEAR_RECENT: ("Clear Recent", false, None),
    WORKSPACE_NAME: ("Name", false, None),
    SAVE_WORKSPACE: ("Save as New Workspace", false, None),
    RESET_WORKSPACE: ("Reset to Saved Layout", false, None),
    DELETE_WORKSPACE: ("Delete Workspace", false, None),
    NO_WORKSPACES: ("No other saved workspaces", false, None),
}

/// Extra Save As container filters, after the format filters, in dialog order.
pub const SAVE_CONTAINER_FILTERS: &[(&str, &[&str])] = &[("Sony Wave64", &["w64"]), ("Apple CAF", &["caf"]), ("Sun/NeXT AU", &["au"])];

/// Every fixed label the menu bar and its submenus can draw in this build, plus the Window
/// list, Tools panel, rack slot picker, Keyboard Shortcuts list and Save As filters (M5.5).
/// Effect-dialog ellipses are markers, so effects retain their action-table labels.
/// Recent-file names, workspace names, Zoom to Time slot times, language names and Undo/Redo
/// history steps are user data: only their fixed entries and submenu titles are included.
/// Other live values (FFT size, custom frame rate and metronome values) are omitted too.
#[cfg(test)]
pub fn bar_labels(release: bool) -> Vec<String> {
    use wavcraft_core::i18n::tr;
    use wavcraft_core::panels::{self, PanelSet};
    use wavcraft_core::save::{SampleType, SaveFormat};
    use wavcraft_core::view::TimeFormat;

    let mut labels: Vec<String> = BAR_ENTRIES.iter().filter(|row| row.offered_in(release)).map(|row| row.label()).collect();
    let panels = PanelSet::for_build(release);
    labels.extend(panels::REGISTRY.iter().filter(|panel| panels.offers(panel.id)).map(|panel| panel.title.to_string()));
    labels.extend(TOOLS.iter().filter(|tool| v1::tool_offered_in(release, tool.name)).map(|tool| tool.label.to_string()));
    labels.extend(crate::app::WaveApp::effects_menu_rows(release).iter().map(|action| action.menu.to_string()));
    labels.extend(crate::rack_panel::slot_effects(release).iter().map(|effect| effect.name().to_string()));
    labels.extend(crate::rack_panel::slot_plugins(release));
    if v1::offered_in(release, Feature::Plugins) {
        // Unlike scanned plugin names, this rack button exists even before a scan has run.
        labels.push("Scan VST3".to_string());
    }
    labels.extend(crate::shortcut_editor::listed(release).map(|command| command.label().to_string()));
    labels.extend(v1::save_formats_in(release, SaveFormat::Wav).iter().map(|format| format.name().to_string()));
    labels.extend(SAVE_CONTAINER_FILTERS.iter().map(|(label, _)| (*label).to_string()));

    // Edit and View use translations; Markers and the grouped Edit items use action.menu.
    for action in actions::TABLE {
        let grouped = matches!(
            action.id,
            ActionId::SelectAll
                | ActionId::DeselectAll
                | ActionId::ClearTimeSelection
                | ActionId::LeftChannel
                | ActionId::RightChannel
                | ActionId::BothChannels
        );
        if (action.name.starts_with("edit.") && !grouped)
            || (action.name.starts_with("view.")
                && !action.name.starts_with("view.time_display.")
                && !action.name.starts_with("view.snap.")
                && !action.name.starts_with("view.zoom.preset."))
            || action.name.starts_with("tool.")
        {
            labels.push(tr(&format!("action.{}", action.name)));
        }
        if action.name.starts_with("marker.")
            || action.name.starts_with("view.snap.")
            || matches!(
                action.id,
                ActionId::SelectAll
                    | ActionId::SelectCurrentView
                    | ActionId::DeselectAll
                    | ActionId::ClearTimeSelection
                    | ActionId::SelectToStart
                    | ActionId::SelectToEnd
                    | ActionId::LeftChannel
                    | ActionId::RightChannel
                    | ActionId::BothChannels
            )
        {
            labels.push(action.menu.to_string());
        }
    }
    labels.extend(effects_extra().into_iter().filter_map(|entry| match entry {
        Entry::Item(item) => Some(item.label.to_string()),
        Entry::Sep => None,
    }));
    labels.extend(FAVORITES.iter().filter(|favorite| favorite_action(favorite).is_some()).map(|favorite| format!("{}…", favorite.label)));
    labels.extend(wavcraft_core::generate::all().iter().map(|generator| format!("{}…", generator.name().trim_start_matches("Generate "))));
    labels.extend([SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32].iter().map(|ty| ty.describe().to_string()));
    labels.extend(TimeFormat::ALL.into_iter().map(|format| match format {
        TimeFormat::Custom(_) => format.menu().to_string(),
        _ => format.menu_with_rate(),
    }));
    for slot in 1..=wavcraft_core::view::ZOOM_SLOTS {
        labels.push(format!("Set Slot {slot} from View"));
    }
    // The ungated View submenus implemented in app/spectral.rs.
    labels.extend(
        [
            "Frequency scale",
            "Linear",
            "Logarithmic (above 1 kHz)",
            "Logarithmic (full)",
            "Reset frequency range",
            "Spectral display",
            "Resolution up",
            "Resolution down",
        ]
        .into_iter()
        .map(str::to_string),
    );
    labels.extend(wavcraft_core::settings::SpecWindow::ALL.iter().map(|window| window.name().to_string()));
    labels.extend([40, 60, 80, 100, 120, 140, 160].into_iter().map(|db| format!("{db} dB range")));
    labels
}

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

/// A tool in the Tools panel: `tool` is `None` for the ones not built yet. The tools themselves
/// are `app::Tool` (the tools bar and the editor's gestures use the same value).
pub struct ToolEntry {
    pub name: &'static str,
    pub label: &'static str,
    pub tool: Option<Tool>,
    pub hint: &'static str,
}

pub const TOOLS: &[ToolEntry] = &[
    ToolEntry { name: "tool.timeSelection", label: "Time Selection", tool: Some(Tool::Select), hint: "Drag to select, click to place the cursor" },
    ToolEntry { name: "tool.hand", label: "Hand", tool: Some(Tool::Hand), hint: "Drag to scroll through time" },
    ToolEntry { name: "tool.zoom", label: "Zoom", tool: Some(Tool::Zoom), hint: "Click to zoom in, Alt+click to zoom out" },
    ToolEntry {
        name: "tool.marquee",
        label: "Marquee Selection",
        tool: Some(Tool::Marquee),
        hint: "Drag a time x frequency rectangle on the spectral display",
    },
    ToolEntry { name: "tool.lasso", label: "Lasso Selection", tool: Some(Tool::Lasso), hint: "Drag a free-form outline on the spectral display" },
    ToolEntry { name: "tool.paintbrush", label: "Paintbrush Selection", tool: None, hint: "Not available yet" },
    ToolEntry { name: "tool.spotHealing", label: "Spot Healing", tool: None, hint: "Not available yet" },
];

/// One entry of the Favorites menu: a registered effect, shown under its own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Favorite {
    pub label: &'static str,
    pub effect_id: &'static str,
}

/// The Favorites menu. v1 needs only these two; Audition's longer Favorites list is after 1.0 (#630).
pub const FAVORITES: &[Favorite] = &[Favorite { label: "Fade In", effect_id: "fade_in" }, Favorite { label: "Fade Out", effect_id: "fade_out" }];

/// The action row a favorite runs: the table row named `effect.<effect_id>`. `None` when no
/// registered effect has that id (the app then leaves the entry out).
pub fn favorite_action(f: &Favorite) -> Option<&'static actions::Action> {
    let name = format!("effect.{}", f.effect_id);
    actions::all_actions().find(|a| a.name == name)
}

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

    /// The entries, top to bottom. The ruler has no time-format entries (the View menu's Time
    /// Display is the place for it); its snapping entries are the View menu's (f49).
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
            Menu::Ruler => vec![
                act(ActionId::ZoomIn),
                act(ActionId::ZoomOut),
                act(ActionId::ZoomToSelection),
                act(ActionId::ZoomOutFull),
                Entry::Sep,
                act(ActionId::SnapToggle),
                act(ActionId::SnapZeroToggle),
                act(ActionId::SnapRulerCoarseToggle),
                act(ActionId::SnapRulerFineToggle),
                act(ActionId::SnapEdgesToggle),
                act(ActionId::SnapMarkersToggle),
            ],
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
        act(ActionId::EpisodeCheck),
        Entry::Sep,
        act(ActionId::SnapToggle),
        act(ActionId::ViewSpectralToggle),
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
        "check.run" => "Check",
        "view.snap.toggle" => "Snap",
        "view.spectral.toggle" => "Spectral",
        "tool.timeSelection" => "Time",
        "tool.hand" => "Hand",
        "tool.zoom" => "Zoom",
        _ => item.label,
    }
}

/// The extra entries the Effects menu draws after its effects and noise-print rows. Plain data,
/// like `toolbar()`: Run Episode Check (`check.run`).
pub fn effects_extra() -> Vec<Entry> {
    vec![act(ActionId::EpisodeCheck)]
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
    fn the_toolbar_has_the_snapping_toggle() {
        let item = toolbar_item("view.snap.toggle").expect("Snapping toggle is on the toolbar");
        assert_eq!(item.cmd, Cmd::Action(ActionId::SnapToggle));
        assert_eq!(glyph(&item), "Snap");
    }

    #[test]
    fn the_toolbar_has_the_spectral_toggle() {
        let item = toolbar_item("view.spectral.toggle").expect("Spectral toggle is on the toolbar");
        assert_eq!(item.cmd, Cmd::Action(ActionId::ViewSpectralToggle));
        assert_eq!(glyph(&item), "Spectral");
        let n = names(&toolbar());
        let snap = n.iter().position(|x| *x == "view.snap.toggle").expect("Snap is on the toolbar");
        assert_eq!(n.get(snap + 1), Some(&"view.spectral.toggle"));
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

    #[test]
    fn the_lasso_is_on_the_toolbar() {
        assert_eq!(toolbar_item("tool.lasso").map(|i| i.cmd), Some(Cmd::Tool(Tool::Lasso)));
    }

    #[test]
    fn e1_favorites_menu_lists_fade_in_and_fade_out() {
        assert_eq!(FAVORITES.len(), 2);
        assert_eq!(FAVORITES.iter().map(|f| f.label).collect::<Vec<_>>(), ["Fade In", "Fade Out"]);
        assert_eq!(FAVORITES.iter().map(|f| f.effect_id).collect::<Vec<_>>(), ["fade_in", "fade_out"]);
        for (f, want) in FAVORITES.iter().zip(["effect.fade_in", "effect.fade_out"]) {
            let a = favorite_action(f).expect("every favorite is a registered effect");
            assert_eq!(a.name, want);
            assert_eq!(a.menu, f.label);
            assert!(matches!(a.id, ActionId::Effect(_)));
        }
    }

    #[test]
    fn toolbar_has_episode_check() {
        let item = toolbar_item("check.run").expect("Run Episode Check is on the toolbar");
        assert_eq!(item.label, "Run Episode Check");
        assert_eq!(item.cmd, Cmd::Action(ActionId::EpisodeCheck));
        assert_eq!(glyph(&item), "Check");
        let n = names(&toolbar());
        let check = n.iter().position(|x| *x == "check.run").expect("Check is on the toolbar");
        let zoom_out_full = n.iter().position(|x| *x == "view.zoomOutFull").expect("Fit is on the toolbar");
        assert!(check > zoom_out_full);
        let mut sorted = n.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), n.len());
    }

    #[test]
    fn effects_menu_has_episode_check() {
        let n = names(&effects_extra());
        assert_eq!(n, ["check.run"]);
        let item = effects_extra()
            .into_iter()
            .find_map(|e| {
                if let Entry::Item(i) = e
                    && i.name == "check.run"
                {
                    Some(i)
                } else {
                    None
                }
            })
            .expect("Run Episode Check is in the extra Effects entries");
        assert_eq!(item.label, "Run Episode Check");
        assert_eq!(item.cmd, Cmd::Action(ActionId::EpisodeCheck));
    }
}
