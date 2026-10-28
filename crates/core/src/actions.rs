//! The edit actions (task 07, first part): one table of ids and menu labels (the keys are in
//! [`crate::shortcuts`]), and an [`Editor`] that runs them on a document's [`History`] without a
//! window. The app's Edit menu, its keys and the control channel all run actions from here, and
//! so do the tests.
//!
//! Keys follow Adobe Audition's defaults and live in [`crate::shortcuts`].

use std::io;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::diagnostics::silence::{self, SilenceError, SilenceParams};
use crate::diagnostics::{declick, declip};
use crate::doc::edit::{self, ChannelMask, Clip, Crossfades, EditError, MixOptions, PasteAt};
use crate::doc::history::History;
use crate::snap::{self, SnapSettings};
use crate::effects::{self, EffectParams, FadeCurve, FadeDir};
use crate::generate;
use std::sync::OnceLock;

/// An action's id, as the menu, the keys and the control channel name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// Set Current Clipboard 1 to 5: Cut, Copy and Paste use the current one.
    Clipboard1,
    Clipboard2,
    Clipboard3,
    Clipboard4,
    Clipboard5,
    CopyToNew,
    PasteToNew,
    CloseAll,
    SaveAll,
    Delete,
    Silence,
    Crop,
    /// Edit > Delete Silence: shorten every long silent stretch to its default keep length.
    DeleteSilence,
    SelectAll,
    DeselectAll,
    ClearTimeSelection,
    LeftChannel,
    RightChannel,
    BothChannels,
    SelectCurrentView,
    SelectToStart,
    SelectToEnd,
    InsertSilence,
    InsertFile,
    CursorToStart,
    CursorToEnd,
    SetIn,
    SetOut,
    ZoomIn,
    ZoomOut,
    ZoomToSelection,
    ZoomOutFull,
    ViewSyncSelection,
    ViewSyncZoom,
    ViewSyncPlayhead,
    ToolSelect,
    ToolHand,
    ToolZoom,
    /// The Marquee tool (B135): drags a time x frequency rectangle on the spectral display.
    ToolMarquee,
    ZoomAmpIn,
    ZoomAmpOut,
    ZoomAmpReset,
    /// Zoom In keeping the selection start (or end) where it is on screen (f69).
    ZoomInAtIn,
    ZoomInAtOut,
    ZoomPreset1,
    ZoomPreset2,
    ZoomPreset3,
    ZoomPreset4,
    ZoomPreset5,
    /// Spectral display resolution (FFT length) up or down, a step along the Preferences page's list.
    SpectralResolutionUp,
    SpectralResolutionDown,
    ChannelsLayered,
    ChannelsStacked,
    ZeroInward,
    ZeroOutward,
    ZeroLeftToLeft,
    ZeroLeftToRight,
    ZeroRightToLeft,
    ZeroRightToRight,
    RepeatLast,
    RepeatLastDialog,
    /// Edit › Convert Sample Type… (A161): opens the window's dialog, then `convert::convert`.
    ConvertSampleType,
    /// Edit › Interpret Sample Rate… (A162): opens the window's dialog, then `convert::interpret_rate`.
    InterpretSampleRate,
    /// View › Snapping on or off (Audition's default key S).
    SnapToggle,
    /// Snap to zero crossings on or off (f09).
    SnapZeroToggle,
    /// Snap to the ruler's major ticks on or off (f49).
    SnapRulerCoarseToggle,
    /// Snap to the ruler's minor ticks on or off (f49).
    SnapRulerFineToggle,
    /// Snap to selection edges and the cursor on or off (f49).
    SnapEdgesToggle,
    /// Snap to markers on or off (f59).
    SnapMarkersToggle,
    /// Enable or disable channel `n` (0-based): edits and the selection act on enabled ones.
    ToggleChannel(u8),
    /// Window > Meter Input Signal (f56): the Levels panel shows the input device while idle.
    MeterInputToggle,
    /// Arm the input monitor meter (on, never off): input peak and clip, nothing recorded.
    MeterInputArm,
    /// Window > Mic Check...
    MicCheck,
    /// Capture Noise Print: learn the selection's noise spectrum (kept for the session).
    CaptureNoisePrint,
    /// Diagnostics › DeClipper (B045): scan and repair clipped peaks.
    DeClip,
    /// Diagnostics › DeClicker (B046): scan and repair clicks and pops.
    DeClick,
    /// Diagnostics › Automatic Click Remover (B072): detect and repair in one command.
    AutoClick,
    /// A registered effect: its index in [`effects::all`].
    Effect(u16),
    // The one-shot effects (B002), on the selection or the whole file.
    Reverse,
    Amplify,
    Normalize,
    FadeInLinear,
    FadeInCosine,
    FadeOutLinear,
    FadeOutCosine,
    /// Window › Sample Statistics: show the panel.
    StatsShow,
    /// The panel's navigation: cursor to the peak, to the next / previous possibly clipped run.
    StatsGotoPeak,
    StatsNextClip,
    StatsPrevClip,
    TimeDecimal,
    TimeCompact,
    TimeSamples,
    TimeSmpte24,
    TimeSmpte25,
    TimeSmpte2997Df,
    TimeSmpte2997Ndf,
    TimeSmpte30,
    TimeSmpte5994,
    TimeCustom,
    TimeCd75,
    TimeBarsBeats,
    EditTempo,
    SnapFrames,
    MarkerAdd,
    MarkerNext,
    MarkerPrev,
    MarkerDelete,
    MarkerToRange,
    MarkerToPoint,
    ExtractChannels,
    ExportRanges,
    GotoTime,
    /// Transport (task f23): all run in the window.
    Play,
    Pause,
    Stop,
    ToggleLoop,
    ToggleSkipSelection,
    ToggleReturnOnStop,
    ToggleAutoScroll,
    FollowPage,
    FollowCenter,
    /// Transport (task f36).
    Rewind,
    FastForward,
    ToggleRoll,
}

/// Which part of the program carries an action out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Changes the document or the selection: [`Editor::run`].
    Edit,
    /// Needs the window (the view, the playhead, a dialog): the app runs it.
    Window,
}

/// How far a zero-crossing adjust looks for a sign change (v1 D6: 10 ms).
pub const ZERO_CROSSING_REACH_MS: u64 = 10;

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub id: ActionId,
    /// The control channel's name for it (`edit.undo`).
    pub name: &'static str,
    /// The menu text when it does not name a step (Undo and Redo add the step's name).
    pub menu: &'static str,
    pub kind: Kind,
}

const fn edit(id: ActionId, name: &'static str, menu: &'static str) -> Action {
    Action { id, name, menu, kind: Kind::Edit }
}
const fn window(id: ActionId, name: &'static str, menu: &'static str) -> Action {
    Action { id, name, menu, kind: Kind::Window }
}

/// Every action, in Edit-menu order. Their keys are in `shortcuts::BINDINGS`.
pub const TABLE: &[Action] = &[
    edit(ActionId::Undo, "edit.undo", "Undo"),
    edit(ActionId::Redo, "edit.redo", "Redo"),
    // Repeat the last repeatable command on the current selection: without its dialog (F3,
    // Shift+R), or with its dialog when it has one (F2, Ctrl+R), as in Audition.
    edit(ActionId::RepeatLast, "edit.repeatLast", "Repeat Last Command"),
    window(ActionId::RepeatLastDialog, "edit.repeatLastDialog", "Repeat Last Command…"),
    edit(ActionId::Cut, "edit.cut", "Cut"),
    edit(ActionId::Copy, "edit.copy", "Copy"),
    edit(ActionId::Paste, "edit.paste", "Paste"),
    // New documents and the all-documents commands: the window carries them out (they add or
    // close documents, ask questions, or start saves).
    window(ActionId::CopyToNew, "edit.copy_to_new", "Copy to New"),
    window(ActionId::PasteToNew, "edit.paste_to_new", "Paste to New"),
    window(ActionId::CloseAll, "file.close_all", "Close All"),
    window(ActionId::SaveAll, "file.save_all", "Save All"),
    edit(ActionId::Clipboard1, "edit.clipboard.1", "Set Current Clipboard: 1"),
    edit(ActionId::Clipboard2, "edit.clipboard.2", "Set Current Clipboard: 2"),
    edit(ActionId::Clipboard3, "edit.clipboard.3", "Set Current Clipboard: 3"),
    edit(ActionId::Clipboard4, "edit.clipboard.4", "Set Current Clipboard: 4"),
    edit(ActionId::Clipboard5, "edit.clipboard.5", "Set Current Clipboard: 5"),
    edit(ActionId::CaptureNoisePrint, "nr.capture_print", "Capture Noise Print"),
    edit(ActionId::Delete, "edit.delete", "Delete"),
    edit(ActionId::Silence, "edit.silence", "Silence"),
    // Insert › Silence… asks for a length (the window's dialog), then runs `Editor::insert_silence`.
    window(ActionId::InsertSilence, "edit.insertSilence", "Insert Silence…"),
    // Insert > Audio from File… picks a file (the window's dialog), then `Editor::insert_file`.
    window(ActionId::InsertFile, "edit.insertFile", "Insert Audio from File…"),
    edit(ActionId::Crop, "edit.crop", "Crop"),
    edit(ActionId::DeleteSilence, "edit.deleteSilence", "Delete Silence"),
    edit(ActionId::SelectAll, "edit.selectAll", "Select All"),
    edit(ActionId::DeselectAll, "edit.deselectAll", "Deselect All"),
    edit(ActionId::ClearTimeSelection, "edit.clearTimeSelection", "Clear Time Selection"),
    edit(ActionId::ZeroInward, "edit.zeroCrossings.inward", "Zero Crossings: Adjust Selection Inward"),
    edit(ActionId::ZeroOutward, "edit.zeroCrossings.outward", "Zero Crossings: Adjust Selection Outward"),
    edit(ActionId::ZeroLeftToLeft, "edit.zeroCrossings.leftToLeft", "Zero Crossings: Left Side to Left"),
    edit(ActionId::ZeroLeftToRight, "edit.zeroCrossings.leftToRight", "Zero Crossings: Left Side to Right"),
    edit(ActionId::ZeroRightToLeft, "edit.zeroCrossings.rightToLeft", "Zero Crossings: Right Side to Left"),
    edit(ActionId::ZeroRightToRight, "edit.zeroCrossings.rightToRight", "Zero Crossings: Right Side to Right"),
    // Diagnostics › DeClipper: scan the selection (or the whole file) and redraw clipped peaks.
    edit(ActionId::DeClip, "diagnostics.declip", "DeClipper"),
    // Diagnostics › DeClicker / Automatic Click Remover: find clicks and pops and redraw them.
    edit(ActionId::DeClick, "diagnostics.declick", "DeClicker"),
    edit(ActionId::AutoClick, "diagnostics.autoClickRemover", "Automatic Click Remover"),
    // Both open a dialog (the window's), then `convert::convert` / `convert::interpret_rate` (A161).
    window(ActionId::ConvertSampleType, "edit.convertSampleType", "Convert Sample Type…"),
    window(ActionId::InterpretSampleRate, "edit.interpretSampleRate", "Interpret Sample Rate…"),
    window(ActionId::SetIn, "edit.setIn", "Set In Point to Playhead"),
    window(ActionId::SetOut, "edit.setOut", "Set Out Point to Playhead"),
    edit(ActionId::LeftChannel, "edit.leftChannel", "Left Channel Only"),
    edit(ActionId::RightChannel, "edit.rightChannel", "Right Channel Only"),
    edit(ActionId::BothChannels, "edit.bothChannels", "Both Channels"),
    // The Select submenu's own entries (Select All and Deselect All are the rows above).
    window(ActionId::SelectCurrentView, "select.currentView", "Select Current View"),
    edit(ActionId::SelectToStart, "select.toStart", "Select from Cursor to Start of File"),
    edit(ActionId::SelectToEnd, "select.toEnd", "Select from Cursor to End of File"),
    edit(ActionId::CursorToStart, "view.cursorToStart", "Playhead to Start"),
    edit(ActionId::CursorToEnd, "view.cursorToEnd", "Playhead to End"),
    // Markers (A171, A176, A121): Add Marker is M; the playhead jumps with Ctrl+Alt+Left/Right.
    edit(ActionId::MarkerAdd, "marker.add", "Add Marker"),
    edit(ActionId::MarkerPrev, "marker.prev", "Playhead to Previous Marker"),
    edit(ActionId::MarkerNext, "marker.next", "Playhead to Next Marker"),
    // Delete and convert act on the markers the selection (or cursor) touches; no default keys.
    edit(ActionId::MarkerDelete, "marker.delete", "Delete Marker"),
    edit(ActionId::MarkerToRange, "marker.toRange", "Convert Marker to Range"),
    edit(ActionId::MarkerToPoint, "marker.toPoint", "Convert Marker to Point"),
    // f79: batch writes of parts of the file; the window picks the folder and writes them.
    window(ActionId::ExtractChannels, "edit.extract_channels", "Extract Channels to Mono Files…"),
    window(ActionId::ExportRanges, "marker.exportRanges", "Export Audio of Range Markers…"),
    window(ActionId::ZoomIn, "view.zoomIn", "Zoom In"),
    window(ActionId::ZoomOut, "view.zoomOut", "Zoom Out"),
    window(ActionId::ZoomToSelection, "view.zoomToSelection", "Zoom to Selection"),
    window(ActionId::ZoomOutFull, "view.zoomOutFull", "Zoom Out Full"),
    // Toggles (View menu checkboxes): what follows the owner when switching between documents.
    window(ActionId::ViewSyncSelection, "view.sync.selection", "Synchronise Selection Across Files"),
    window(ActionId::ViewSyncZoom, "view.sync.zoom", "Synchronise Zoom and Scroll Across Files"),
    window(ActionId::ViewSyncPlayhead, "view.sync.playhead", "Synchronise Playhead Across Files"),
    window(ActionId::ZoomAmpIn, "view.zoom.ampIn", "Zoom In Vertically"),
    window(ActionId::ZoomAmpOut, "view.zoom.ampOut", "Zoom Out Vertically"),
    window(ActionId::ZoomAmpReset, "view.zoom.ampReset", "Reset Vertical Zoom"),
    window(ActionId::ZoomInAtIn, "view.zoom.in_at_in", "Zoom In at In Point"),
    window(ActionId::ZoomInAtOut, "view.zoom.in_at_out", "Zoom In at Out Point"),
    window(ActionId::ZoomPreset1, "view.zoom.preset.1", "Zoom to Time: Slot 1"),
    window(ActionId::ZoomPreset2, "view.zoom.preset.2", "Zoom to Time: Slot 2"),
    window(ActionId::ZoomPreset3, "view.zoom.preset.3", "Zoom to Time: Slot 3"),
    window(ActionId::ZoomPreset4, "view.zoom.preset.4", "Zoom to Time: Slot 4"),
    window(ActionId::ZoomPreset5, "view.zoom.preset.5", "Zoom to Time: Slot 5"),
    window(ActionId::SpectralResolutionUp, "view.spectral.resolutionUp", "Spectral Resolution Up"),
    window(ActionId::SpectralResolutionDown, "view.spectral.resolutionDown", "Spectral Resolution Down"),
    window(ActionId::ChannelsLayered, "view.channels.layered", "Layer Channels"),
    window(ActionId::ChannelsStacked, "view.channels.stacked", "Stack Channels"),
    window(ActionId::MeterInputToggle, "meter.input.toggle", "Meter Input Signal"),
    window(ActionId::MeterInputArm, "meter.input.arm", "Test Input Levels (Monitor)"),
    window(ActionId::MicCheck, "meter.input.micCheck", "Mic Check..."),
    // The mouse tools (f64): a minimal switch; a fuller tool model is on another branch.
    window(ActionId::ToolSelect, "tool.select", "Time Selection Tool"),
    window(ActionId::ToolHand, "tool.hand", "Hand Tool"),
    window(ActionId::ToolZoom, "tool.zoom", "Zoom Tool"),
    window(ActionId::ToolMarquee, "tool.marquee", "Marquee Selection Tool"),
    window(ActionId::StatsShow, "stats.show", "Sample Statistics"),
    window(ActionId::StatsGotoPeak, "stats.goto_peak", "Go to Peak"),
    window(ActionId::StatsNextClip, "stats.next_clip", "Next Possibly Clipped Run"),
    window(ActionId::StatsPrevClip, "stats.prev_clip", "Previous Possibly Clipped Run"),
    window(ActionId::TimeDecimal, "view.time_display.decimal", "Decimal (mm:ss.ddd)"),
    window(ActionId::TimeCompact, "view.time_display.compact", "Compact"),
    window(ActionId::TimeSamples, "view.time_display.samples", "Samples"),
    window(ActionId::GotoTime, "view.gotoTime", "Go to Time…"),
    window(ActionId::TimeSmpte24, "view.time_display.smpte_24", "SMPTE 24 fps"),
    window(ActionId::TimeSmpte25, "view.time_display.smpte_25", "SMPTE 25 fps"),
    window(ActionId::TimeSmpte2997Df, "view.time_display.smpte_2997df", "SMPTE 29.97 fps drop"),
    window(ActionId::TimeSmpte2997Ndf, "view.time_display.smpte_2997ndf", "SMPTE 29.97 fps non-drop"),
    window(ActionId::TimeSmpte30, "view.time_display.smpte_30", "SMPTE 30 fps"),
    window(ActionId::TimeSmpte5994, "view.time_display.smpte_5994", "SMPTE 59.94 fps"),
    window(ActionId::TimeCustom, "view.time_display.custom", "Custom frame rate…"),
    window(ActionId::TimeCd75, "view.time_display.cd75", "Compact Disc (mm:ss:ff, 75 fps)"),
    window(ActionId::TimeBarsBeats, "view.time_display.bars_beats", "Bars and Beats"),
    window(ActionId::EditTempo, "view.editTempo", "Edit Tempo…"),
    window(ActionId::SnapFrames, "view.snap_frames.toggle", "Snap to Frames"),
    // The general snapping switches (f09, f49). Plain S is Snapping; the others have no key.
    edit(ActionId::SnapToggle, "view.snap.toggle", "Snapping"),
    edit(ActionId::SnapZeroToggle, "view.snap.zero.toggle", "Snap to Zero Crossings"),
    edit(ActionId::SnapRulerCoarseToggle, "view.snap.ruler_coarse.toggle", "Snap to Ruler (Coarse)"),
    edit(ActionId::SnapRulerFineToggle, "view.snap.ruler_fine.toggle", "Snap to Ruler (Fine)"),
    edit(ActionId::SnapEdgesToggle, "view.snap.edges.toggle", "Snap to Selection Edges and Cursor"),
    edit(ActionId::SnapMarkersToggle, "view.snap.markers.toggle", "Snap to Markers"),
    // Transport (f23, f36). Space (Play / Stop, and resume after Pause) is handled by the window itself.
    window(ActionId::Play, "transport.play", "Play"),
    window(ActionId::Pause, "transport.pause", "Pause"),
    window(ActionId::Stop, "transport.stop", "Stop"),
    window(ActionId::ToggleLoop, "transport.loop", "Loop Playback"),
    window(ActionId::ToggleSkipSelection, "transport.skipSelection", "Skip Selection"),
    window(ActionId::ToggleReturnOnStop, "transport.returnOnStop", "Return to Start Position on Stop"),
    window(ActionId::ToggleAutoScroll, "transport.autoScroll", "Auto-Scroll"),
    window(ActionId::FollowPage, "transport.followPage", "Auto-Scroll Pagewise"),
    window(ActionId::FollowCenter, "transport.followCenter", "Auto-Scroll Centered"),
    window(ActionId::Rewind, "transport.rewind", "Rewind"),
    window(ActionId::FastForward, "transport.fastForward", "Fast Forward"),
    window(ActionId::ToggleRoll, "transport.roll", "Pre/Post-Roll"),
];

/// The Zoom to Time actions, slot 1 first (f69).
pub const ZOOM_PRESET_ACTIONS: [ActionId; 5] = [ActionId::ZoomPreset1, ActionId::ZoomPreset2, ActionId::ZoomPreset3, ActionId::ZoomPreset4, ActionId::ZoomPreset5];

/// The 0-based Zoom to Time slot an action recalls, if it is one of them.
pub fn zoom_preset_slot(id: ActionId) -> Option<usize> {
    ZOOM_PRESET_ACTIONS.iter().position(|a| *a == id)
}

/// The one-shot effects (B002). Kept apart from [`TABLE`] (which holds no `effect.` rows; the
/// registry's effects come through [`all_actions`]) and listed by [`all_actions`] before the registry's.
pub const ONE_SHOT: &[Action] = &[
    // One-shot effects on the selection (the whole file with none). Amplify uses the editor's
    // `params` (control verb `PARAM`). Normalize has no row: the registry's Normalize (E3, #29)
    // owns `effect.normalize`, and `ActionId::Normalize` still runs through `Editor::run`.
    edit(ActionId::Reverse, "effect.reverse", "Reverse"),
    edit(ActionId::Amplify, "effect.amplify", "Amplify"),
    edit(ActionId::FadeInLinear, "effect.fadeIn.linear", "Fade In (Linear)"),
    edit(ActionId::FadeInCosine, "effect.fadeIn.cosine", "Fade In (Raised Cosine)"),
    edit(ActionId::FadeOutLinear, "effect.fadeOut.linear", "Fade Out (Linear)"),
    edit(ActionId::FadeOutCosine, "effect.fadeOut.cosine", "Fade Out (Raised Cosine)"),
];

/// One row per registered effect (`effect.<id>`, menu text its name), built once. The names are
/// leaked once so rows can be `'static` like the table's.
fn effect_rows() -> &'static [Action] {
    static ROWS: OnceLock<Vec<Action>> = OnceLock::new();
    ROWS.get_or_init(|| {
        effects::all()
            .iter()
            .enumerate()
            .map(|(i, e)| Action {
                id: ActionId::Effect(i as u16),
                name: Box::leak(format!("effect.{}", e.id()).into_boxed_str()),
                menu: e.name(),
                kind: Kind::Edit,
            })
            .collect()
    })
}

/// The table's rows followed by one row per registered effect: the whole action table.
pub fn all_actions() -> impl Iterator<Item = &'static Action> {
    TABLE.iter().chain(ONE_SHOT).chain(effect_rows())
}

/// The snapping switch `id` flips, if it is one of the snapping actions.
pub fn snap_flag(snap: &mut SnapSettings, id: ActionId) -> Option<&mut bool> {
    match id {
        ActionId::SnapToggle => Some(&mut snap.enabled),
        ActionId::SnapZeroToggle => Some(&mut snap.zero_crossings),
        ActionId::SnapRulerCoarseToggle => Some(&mut snap.ruler_coarse),
        ActionId::SnapRulerFineToggle => Some(&mut snap.ruler_fine),
        ActionId::SnapEdgesToggle => Some(&mut snap.edges),
        ActionId::SnapMarkersToggle => Some(&mut snap.markers),
        _ => None,
    }
}

/// Whether the snapping switch `id` stands for is on (`None` for any other action).
pub fn snap_flag_value(snap: &SnapSettings, id: ActionId) -> Option<bool> {
    snap_flag(&mut { *snap }, id).map(|f| *f)
}

/// Flip a snapping switch; the status-bar message says which and how it is now.
pub fn toggle_snap(snap: &mut SnapSettings, id: ActionId) -> Option<String> {
    let flag = snap_flag(snap, id)?;
    *flag = !*flag;
    let on = *flag;
    let what = match id {
        ActionId::SnapToggle => "Snapping",
        ActionId::SnapZeroToggle => "Snap to zero crossings",
        ActionId::SnapRulerCoarseToggle => "Snap to ruler (coarse)",
        ActionId::SnapRulerFineToggle => "Snap to ruler (fine)",
        ActionId::SnapMarkersToggle => "Snap to markers",
        _ => "Snap to selection edges and cursor",
    };
    Some(format!("{what} {}", if on { "on" } else { "off" }))
}

/// The row every `channel.toggle.<n>` action shares (the channel number is in the id).
const CHANNEL_ROW: Action = edit(ActionId::ToggleChannel(0), "channel.toggle", "Toggle Channel");

/// The table row for `id`.
pub fn action(id: ActionId) -> &'static Action {
    if matches!(id, ActionId::ToggleChannel(_)) {
        return &CHANNEL_ROW;
    }
    all_actions().find(|a| a.id == id).unwrap_or(&TABLE[0])
}

/// The action the control channel calls `name` (table rows only; see [`id_by_name`]).
pub fn by_name(name: &str) -> Option<&'static Action> {
    // The zoom actions also answer to a `view.zoom.*` name (the zoom panel's buttons).
    let name = match name {
        "view.zoom.in" => "view.zoomIn",
        "view.zoom.out" => "view.zoomOut",
        "view.zoom.selection" => "view.zoomToSelection",
        "view.zoom.full" => "view.zoomOutFull",
        n => n,
    };
    // `select.*` also names the two rows that live in the Edit group.
    let name = match name {
        "select.all" => "edit.selectAll",
        "select.none" => "edit.deselectAll",
        n => n,
    };
    all_actions().find(|a| a.name == name)
}

/// The id the control channel's `name` stands for: a table row, or `channel.toggle.<n>`.
pub fn id_by_name(name: &str) -> Option<ActionId> {
    if let Some(n) = name.strip_prefix("channel.toggle.") {
        return n.parse::<u8>().ok().filter(|&n| n < 64).map(ActionId::ToggleChannel);
    }
    by_name(name).map(|a| a.id)
}

/// What a run did, for the window to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The document changed: views and the player must read the new state.
    pub doc_changed: bool,
    /// The selection after the action (`start == end` is just the cursor).
    pub selection: Range<u64>,
    /// A message for the status bar.
    pub message: Option<String>,
}

/// What [`Editor::delete_silence`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SilenceDone {
    pub outcome: Outcome,
    /// Stretches shortened.
    pub stretches: usize,
    /// Frames removed in all.
    pub removed_frames: u64,
}

/// One open document's editing state: its history, the active channels and the clipboard.
pub struct Editor {
    pub history: History,
    /// The channels edits act on (Up / Down pick one, as in Audition).
    pub channels: ChannelMask,
    /// The five internal clipboards (Set Current Clipboard); each holds a clip until replaced.
    pub clipboards: [Option<Clip>; CLIPBOARDS],
    /// Which clipboard Cut, Copy and Paste use (0 to 4; the menu says 1 to 5).
    pub current_clipboard: usize,
    pub crossfades: Crossfades,
    /// What Repeat Last Command runs again (None until a repeatable command has run).
    pub last: Option<Repeat>,
    /// The numbers Amplify and Normalize use.
    pub params: EffectParams,
    /// Snapping (f09, f49): the master switch and what it pulls to.
    pub snap: SnapSettings,
}

/// How many internal clipboards there are.
pub const CLIPBOARDS: usize = 5;

/// A command Repeat Last Command can run again, with its settings.
#[derive(Debug, Clone, PartialEq)]
pub enum Repeat {
    /// An action from the table, on the selection at the time of the repeat.
    Action(ActionId),
    /// Insert › Silence of this many frames, at the cursor.
    InsertSilence(u64),
    /// A registered effect (its index in [`effects::all`]) with the settings it last ran with.
    Effect(u16, Vec<f64>),
    /// DeClipper with `[threshold_db, min_run]`.
    DeClip(Vec<f64>),
    /// DeClicker with `[sensitivity, max_width_ms]`.
    DeClick(Vec<f64>),
    /// Automatic Click Remover with `[threshold, complexity]`.
    AutoClick(Vec<f64>),
}

/// What [`ActionId::RepeatLastDialog`] (`F2` / `Ctrl+R`) opens or runs for the remembered command.
#[derive(Debug, Clone, PartialEq)]
pub enum RepeatDialog {
    /// No repeatable command has run yet, or it cannot run with the current selection.
    Unavailable,
    /// Reopen the effect dialog for effect `index` in [`effects::all`] pre-filled with these settings.
    Effect(u16, Vec<f64>),
    /// Reopen the Insert › Silence dialog pre-filled with this many frames.
    InsertSilence(u64),
    /// The last command has no dialog (a parameterless effect or a repeatable edit action): run [`ActionId::RepeatLast`] directly.
    RunNow,
}

/// The clipboard (0 to 4) a Set Current Clipboard action picks.
pub fn clipboard_index(id: ActionId) -> Option<usize> {
    match id {
        ActionId::Clipboard1 => Some(0),
        ActionId::Clipboard2 => Some(1),
        ActionId::Clipboard3 => Some(2),
        ActionId::Clipboard4 => Some(3),
        ActionId::Clipboard5 => Some(4),
        _ => None,
    }
}

/// Whether Repeat Last Command may run `id` again. Delete is not repeatable (Audition), nor are
/// Undo/Redo, the clipboard's Cut and Copy, selection moves and the view.
fn repeatable(id: ActionId) -> bool {
    matches!(
        id,
        ActionId::Silence
            | ActionId::Paste
            | ActionId::Crop
            | ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight
            | ActionId::Reverse
            | ActionId::Amplify
            | ActionId::Normalize
            | ActionId::FadeInLinear
            | ActionId::FadeInCosine
            | ActionId::FadeOutLinear
            | ActionId::FadeOutCosine
    )
}

/// Which way a selection edge moves to find a zero crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
}

/// The nearest zero crossing to `frame` on the side `toward`, at most `reach` frames away, or
/// None. A crossing at `i` means the signal changes sign between frames `i - 1` and `i` (or frame
/// `i` is exactly 0). `frame` itself never counts: an edge already on a crossing moves to the
/// next one, so pressing the key again steps on (Adobe: "to the next zero crossing"; Hyperion's
/// check of #7, 2026-10-19). The signal is the sum of the `channels` asked for,
/// so a stereo edge lands where the active channels' mix crosses (Audition's rule for stereo was
/// not seen; `docs/code-map.md`, Known differences).
pub fn zero_crossing(doc: &crate::doc::DocState, channels: ChannelMask, frame: u64, toward: Toward, reach: u64) -> std::io::Result<Option<u64>> {
    let len = doc.len_frames();
    if len < 2 {
        return Ok(None);
    }
    let frame = frame.min(len);
    // Frames [lo, hi) cover every pair (i - 1, i) the search can look at.
    let (lo, hi) = match toward {
        Toward::Left => (frame.saturating_sub(reach).saturating_sub(1), (frame + 1).min(len)),
        Toward::Right => (frame.saturating_sub(1), (frame + reach + 1).min(len)),
    };
    let mut mix = vec![0.0f64; (hi - lo) as usize];
    let mut buf = vec![0.0f32; mix.len()];
    for ch in (0..doc.channel_count()).filter(|&c| channels.contains(c)) {
        let got = doc.read(ch, lo, &mut buf)?;
        for (m, v) in mix.iter_mut().zip(&buf[..got]) {
            *m += f64::from(*v);
        }
    }
    let at = |i: u64| mix[(i - lo) as usize];
    let crosses = |i: u64| i > lo && i < hi && (at(i) == 0.0 || (at(i - 1) < 0.0) != (at(i) < 0.0));
    let found = match toward {
        Toward::Left => (frame.saturating_sub(reach)..frame).rev().find(|&i| crosses(i)),
        Toward::Right => (frame + 1..=(frame + reach).min(len - 1)).find(|&i| crosses(i)),
    };
    Ok(found)
}

impl Editor {
    pub fn new(history: History) -> Editor {
        Editor { history, channels: ChannelMask::ALL, clipboards: Default::default(), current_clipboard: 0, crossfades: Crossfades::default(), last: None, params: EffectParams::default(), snap: SnapSettings::default() }
    }

    /// The current clipboard's clip, if it holds one.
    pub fn clipboard(&self) -> Option<&Clip> {
        self.clipboards.get(self.current_clipboard).and_then(Option::as_ref)
    }

    /// Put `clip` in the current clipboard.
    pub fn set_clipboard(&mut self, clip: Clip) {
        if let Some(slot) = self.clipboards.get_mut(self.current_clipboard) {
            *slot = Some(clip);
        }
    }

    fn len(&self) -> u64 {
        self.history.current().len_frames()
    }

    fn channel_count(&self) -> usize {
        self.history.current().channel_count()
    }

    /// Whether `id` can run with `selection` (the menu greys it out otherwise).
    pub fn enabled(&self, id: ActionId, selection: &Range<u64>) -> bool {
        let has_sel = selection.start < selection.end;
        match id {
            ActionId::Undo => self.history.can_undo(),
            ActionId::Redo => self.history.can_redo(),
            // With no selection, Cut and Copy take the whole file (Audition).
            ActionId::Cut | ActionId::Copy => self.len() > 0,
            ActionId::Paste | ActionId::PasteToNew => self.clipboard().is_some(),
            ActionId::CopyToNew => self.len() > 0,
            ActionId::CloseAll | ActionId::SaveAll => true,
            ActionId::Clipboard1 | ActionId::Clipboard2 | ActionId::Clipboard3 | ActionId::Clipboard4 | ActionId::Clipboard5 => true,
            ActionId::Delete | ActionId::Silence | ActionId::Crop | ActionId::CaptureNoisePrint => has_sel,
            ActionId::SelectAll => self.len() > 0,
            ActionId::DeselectAll | ActionId::ClearTimeSelection => has_sel,
            ActionId::LeftChannel | ActionId::RightChannel | ActionId::BothChannels => self.channel_count() >= 2,
            ActionId::SelectCurrentView => self.len() > 0,
            ActionId::SelectToStart => self.len() > 0 && selection.start > 0,
            ActionId::SelectToEnd => self.len() > 0 && selection.end < self.len(),
            ActionId::InsertSilence | ActionId::InsertFile | ActionId::CursorToStart | ActionId::CursorToEnd => true,
            ActionId::MeterInputToggle | ActionId::MeterInputArm | ActionId::MicCheck => true,
            ActionId::TimeDecimal
            | ActionId::TimeCompact
            | ActionId::TimeSamples
            | ActionId::TimeSmpte24
            | ActionId::TimeSmpte25
            | ActionId::TimeSmpte2997Df
            | ActionId::TimeSmpte2997Ndf
            | ActionId::TimeSmpte30
            | ActionId::TimeSmpte5994
            | ActionId::TimeCustom
            | ActionId::TimeCd75
            | ActionId::TimeBarsBeats
            | ActionId::EditTempo
            | ActionId::SnapFrames
            | ActionId::SnapToggle
            | ActionId::SnapZeroToggle
            | ActionId::SnapRulerCoarseToggle
            | ActionId::SnapRulerFineToggle
            | ActionId::SnapEdgesToggle
            | ActionId::SnapMarkersToggle => true,
            ActionId::ConvertSampleType | ActionId::InterpretSampleRate => self.len() > 0,
            ActionId::ToggleChannel(n) => usize::from(n) < self.channel_count(),
            ActionId::StatsShow | ActionId::StatsGotoPeak | ActionId::StatsNextClip | ActionId::StatsPrevClip => self.len() > 0,
            ActionId::MarkerAdd => self.len() > 0,
            ActionId::MarkerNext => self.history.current().markers().next_after(selection.start).is_some(),
            ActionId::MarkerPrev => self.history.current().markers().prev_before(selection.start).is_some(),
            ActionId::MarkerDelete => !self.history.current().markers().touched(selection).is_empty(),
            ActionId::MarkerToRange => has_sel && !self.history.current().markers().touched_kind(selection, true).is_empty(),
            ActionId::MarkerToPoint => !self.history.current().markers().touched_kind(selection, false).is_empty(),
            ActionId::ExtractChannels => self.channel_count() >= 2 && self.len() > 0,
            ActionId::ExportRanges => !crate::export::exportable_ranges(self.history.current(), None).is_empty(),
            ActionId::GotoTime | ActionId::SetIn | ActionId::SetOut | ActionId::ZoomIn | ActionId::ZoomOut | ActionId::ZoomOutFull => self.len() > 0,
            ActionId::ZoomAmpIn | ActionId::ZoomAmpOut | ActionId::ZoomAmpReset | ActionId::ChannelsLayered | ActionId::ChannelsStacked => self.len() > 0,
            ActionId::ZoomInAtIn | ActionId::ZoomInAtOut => self.len() > 0,
            ActionId::ZoomPreset1 | ActionId::ZoomPreset2 | ActionId::ZoomPreset3 | ActionId::ZoomPreset4 | ActionId::ZoomPreset5 => self.len() > 0,
            ActionId::ToolSelect | ActionId::ToolHand | ActionId::ToolZoom | ActionId::ToolMarquee => true,
            ActionId::SpectralResolutionUp | ActionId::SpectralResolutionDown => true,
            ActionId::ZoomToSelection => has_sel,
            ActionId::Play
            | ActionId::Pause
            | ActionId::Stop
            | ActionId::ToggleLoop
            | ActionId::ToggleSkipSelection
            | ActionId::ToggleReturnOnStop
            | ActionId::ToggleAutoScroll
            | ActionId::FollowPage
            | ActionId::FollowCenter
            | ActionId::Rewind
            | ActionId::FastForward
            | ActionId::ToggleRoll => true,
            ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => has_sel,
            // An effect takes the selection, or the whole file when there is none.
            ActionId::Effect(_) | ActionId::DeleteSilence | ActionId::DeClip | ActionId::DeClick | ActionId::AutoClick => self.len() > 0,
            // Like Copy: with no selection the one-shot effects take the whole file.
            ActionId::Reverse
            | ActionId::Amplify
            | ActionId::Normalize
            | ActionId::FadeInLinear
            | ActionId::FadeInCosine
            | ActionId::FadeOutLinear
            | ActionId::FadeOutCosine => self.len() > 0,
            ActionId::ViewSyncSelection | ActionId::ViewSyncZoom | ActionId::ViewSyncPlayhead => true,
            ActionId::RepeatLast | ActionId::RepeatLastDialog => match self.last {
                Some(Repeat::Action(last)) => self.enabled(last, selection),
                Some(Repeat::InsertSilence(_) | Repeat::Effect(..) | Repeat::DeClip(_) | Repeat::DeClick(_) | Repeat::AutoClick(_)) => self.len() > 0,
                None => false,
            },
        }
    }

    /// The menu text for `id`: "Undo Cut", "Redo Paste", or the plain label.
    pub fn label(&self, id: ActionId) -> String {
        let a = action(id);
        match id {
            ActionId::Undo => self.history.undo_label().map_or_else(|| a.menu.to_string(), |l| format!("Undo {l}")),
            ActionId::Redo => self.history.redo_label().map_or_else(|| a.menu.to_string(), |l| format!("Redo {l}")),
            ActionId::SnapToggle | ActionId::SnapZeroToggle | ActionId::SnapRulerCoarseToggle | ActionId::SnapRulerFineToggle | ActionId::SnapEdgesToggle | ActionId::SnapMarkersToggle => {
                let on = snap_flag_value(&self.snap, id).unwrap_or(false);
                format!("{} ({})", a.menu, if on { "on" } else { "off" })
            }
            ActionId::LeftChannel if self.channels == ChannelMask::only(0) => "Both Channels".to_string(),
            ActionId::RightChannel if self.channels == ChannelMask::only(1) => "Both Channels".to_string(),
            ActionId::RepeatLast | ActionId::RepeatLastDialog => match self.last {
                Some(Repeat::Action(last)) => format!("Repeat {}", action(last).menu),
                Some(Repeat::Effect(i, _)) => {
                    let name = effects::all().get(usize::from(i)).map_or("Effect", |e| e.name());
                    // "Repeat Gain" runs at once; the dialog version keeps the "…" when it has one.
                    let has_dialog = effects::all().get(usize::from(i)).is_some_and(|e| !e.parameters().is_empty());
                    if id == ActionId::RepeatLastDialog && has_dialog { format!("Repeat {name}…") } else { format!("Repeat {name}") }
                }
                Some(Repeat::DeClip(_)) => "Repeat DeClipper".to_string(),
                Some(Repeat::DeClick(_)) => "Repeat DeClicker".to_string(),
                Some(Repeat::AutoClick(_)) => "Repeat Automatic Click Remover".to_string(),
                // "Repeat Insert Silence" runs it at once; the dialog version keeps the "…".
                Some(Repeat::InsertSilence(_)) if id == ActionId::RepeatLast => "Repeat Insert Silence".to_string(),
                Some(Repeat::InsertSilence(_)) => format!("Repeat {}", action(ActionId::InsertSilence).menu),
                None => a.menu.to_string(),
            },
            ActionId::Clipboard1 | ActionId::Clipboard2 | ActionId::Clipboard3 | ActionId::Clipboard4 | ActionId::Clipboard5 => {
                // The current one is marked, and one holding a clip says so.
                let n = clipboard_index(id).unwrap_or(0);
                let mark = if n == self.current_clipboard { "\u{2713} " } else { "" };
                let full = if self.clipboards.get(n).is_some_and(Option::is_some) { " (has audio)" } else { "" };
                format!("{mark}{}{full}", a.menu)
            }
            _ => a.menu.to_string(),
        }
    }

    /// Repeat Last Command: the last repeatable command again, with the same settings, on
    /// `selection` (Insert Silence: the same length at the cursor).
    pub fn repeat(&mut self, selection: Range<u64>) -> Result<Outcome, EditError> {
        match self.last.clone() {
            Some(Repeat::Action(id)) => self.run(id, selection),
            Some(Repeat::Effect(i, params)) => self.run_effect(i, params, selection),
            Some(Repeat::DeClip(params)) => self.run_declip(params, selection),
            Some(Repeat::DeClick(params)) => self.run_declick(params, selection),
            Some(Repeat::AutoClick(params)) => self.run_auto_click(params, selection),
            Some(Repeat::InsertSilence(frames)) => self.insert_silence(selection.start, frames),
            None => Ok(Outcome { doc_changed: false, selection, message: Some("There is no command to repeat yet".to_string()) }),
        }
    }

    /// What [`ActionId::RepeatLastDialog`] (`F2` / `Ctrl+R`) opens or runs for `selection`.
    #[allow(clippy::todo)]
    pub fn repeat_dialog(&self, selection: &Range<u64>) -> RepeatDialog {
        let _ = selection;
        todo!()
    }

    /// The zero-crossing adjusts: each moved edge goes to the nearest sign change within
    /// [`ZERO_CROSSING_REACH_MS`] on its side; an edge with none there stays.
    fn zero_crossings(&self, id: ActionId, sel: Range<u64>) -> Result<Outcome, EditError> {
        let doc = self.history.current();
        let reach = u64::from(doc.sample_rate()) * ZERO_CROSSING_REACH_MS / 1000;
        let channels = self.channels.within(doc.channel_count());
        let find = |frame: u64, toward: Toward| -> Result<u64, EditError> {
            Ok(zero_crossing(doc, channels, frame, toward, reach).map_err(EditError::from)?.unwrap_or(frame))
        };
        let (start, end) = match id {
            ActionId::ZeroInward => (find(sel.start, Toward::Right)?, find(sel.end, Toward::Left)?),
            ActionId::ZeroOutward => (find(sel.start, Toward::Left)?, find(sel.end, Toward::Right)?),
            ActionId::ZeroLeftToLeft => (find(sel.start, Toward::Left)?, sel.end),
            ActionId::ZeroLeftToRight => (find(sel.start, Toward::Right)?, sel.end),
            ActionId::ZeroRightToLeft => (sel.start, find(sel.end, Toward::Left)?),
            _ => (sel.start, find(sel.end, Toward::Right)?),
        };
        // An inward adjust never turns the selection inside out.
        let (selection, message) = if start < end {
            let moved = start != sel.start || end != sel.end;
            (
                start..end,
                Some(
                    if moved { "Selection edges moved to zero crossings" } else { "No other zero crossing within 10 ms; the edges stay" }.to_string(),
                ),
            )
        } else {
            (sel, Some("No zero crossings inside the selection within 10 ms".to_string()))
        };
        Ok(Outcome { doc_changed: false, selection, message })
    }

    /// Enable or disable channel `ch` for edits and the selection. At least one channel stays
    /// enabled; the mask is `ALL` again when every channel is.
    fn toggle_channel(&mut self, ch: usize, selection: Range<u64>) -> Outcome {
        let count = self.channel_count();
        let cur = self.channels.within(count);
        let next = if cur.contains(ch) { ChannelMask::from_bits(cur.bits() & !ChannelMask::only(ch).bits()) } else { cur.with(ch) };
        let message = if next == ChannelMask::NONE {
            "At least one channel stays enabled".to_string()
        } else {
            self.channels = if next.covers_all(count) { ChannelMask::ALL } else { next };
            format!("Channel {} {}", ch + 1, if self.channels.within(count).contains(ch) { "enabled" } else { "disabled" })
        };
        Outcome { doc_changed: false, selection, message: Some(message) }
    }

    /// One flag per channel, `1` enabled and `0` not (`"10"`: only the first of two).
    pub fn enabled_flags(&self) -> String {
        let mask = self.channels.within(self.channel_count());
        (0..self.channel_count()).map(|c| if mask.contains(c) { '1' } else { '0' }).collect()
    }

    /// The snap step selection and cursor gestures call: `frame` moved to the nearest snap
    /// target within a few pixels (`frames_per_px` is the view's zoom), or unchanged when
    /// snapping is off. `targets` are the frames to snap to besides the ruler's ticks: the
    /// selection's edges and the cursor. Zero crossings use the enabled channels.
    pub fn snap(&self, frame: u64, frames_per_px: f64, targets: &[u64]) -> u64 {
        let doc = self.history.current();
        snap::snap_frame(doc, &self.snap, self.channels.within(doc.channel_count()), frames_per_px, targets, frame)
    }

    /// [`Editor::snap`] with the ruler's tick steps given as `Some((major, minor))` seconds, for a
    /// time display whose ticks are not decimal seconds (see [`snap::snap_frame_steps`]).
    pub fn snap_with(&self, frame: u64, frames_per_px: f64, targets: &[u64], steps: Option<(f64, f64)>) -> u64 {
        let doc = self.history.current();
        snap::snap_frame_steps(doc, &self.snap, self.channels.within(doc.channel_count()), frames_per_px, targets, frame, steps)
    }

    /// Insert › Silence: `frames` of silence at `at` on every channel (Audition inserts across
    /// all channels); the inserted range ends up selected.
    pub fn insert_silence(&mut self, at: u64, frames: u64) -> Result<Outcome, EditError> {
        let doc = self.history.current().clone();
        let at = at.min(doc.len_frames());
        let edit = edit::insert_silence(&doc, at, frames, self.crossfades)?;
        self.last = Some(Repeat::InsertSilence(frames));
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        Ok(Outcome { doc_changed: changed, selection: at..at + frames, message: changed.then_some(label) })
    }

    /// Generate › Tones / Noise (f21): the generator at index `i` of [`generate::all`] with these
    /// settings, inserted at the cursor (`selection.start`) or replacing the selection, on every
    /// channel, as one undoable step. The new audio ends up selected. `seed` makes noise
    /// repeatable. Not remembered for Repeat Last Command.
    pub fn generate(&mut self, i: u16, params: &[f64], selection: Range<u64>, seed: u64) -> Result<Outcome, EditError> {
        let Some(generator) = generate::all().get(usize::from(i)) else {
            return Err(EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, "no such generator")));
        };
        generate::validate(*generator, params).map_err(|why| EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why)))?;
        let doc = self.history.current().clone();
        let len = doc.len_frames();
        let range = selection.start.min(len)..selection.end.min(len);
        let rate = doc.sample_rate();
        let frames = (generator.duration(params) * f64::from(rate)).round() as u64;
        let mut sources: Vec<_> = (0..doc.channel_count()).map(|c| generator.source(params, rate, c, seed)).collect();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let edit = edit::generate(&doc, range.clone(), frames, generator.name(), &cancel, |c, out| {
            sources[c].fill(out);
            Ok(())
        })?;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        Ok(Outcome { doc_changed: changed, selection: range.start..range.start + frames, message: changed.then_some(label) })
    }

    /// Set a number the effects use: `amplify.db` or `normalize.db`.
    pub fn set_param(&mut self, name: &str, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!("{name}: {value} is not a number"));
        }
        match name {
            "amplify.db" if value <= effects::MAX_GAIN_DB => self.params.amplify_db = value,
            "normalize.db" if (-144.0..=24.0).contains(&value) => self.params.normalize_db = value,
            "amplify.db" | "normalize.db" => return Err(format!("{name}: {value} is out of range")),
            _ => return Err(format!("no parameter named {name:?} (amplify.db, normalize.db)")),
        }
        Ok(())
    }

    /// Process › Stretch and Pitch (f127) on `selection` (the whole file when empty): the range
    /// becomes `ratio` times as long (pitch kept) and is then shifted by `semitones`. The
    /// result ends up selected. Not remembered by Repeat Last Command.
    pub fn stretch_pitch(&mut self, ratio: f64, semitones: f64, selection: Range<u64>) -> Result<Outcome, EditError> {
        let doc = self.history.current().clone();
        let len = doc.len_frames();
        let sel = selection.start.min(len)..selection.end.min(len);
        let range = if sel.start < sel.end { sel } else { 0..len };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let edit = crate::timepitch::stretch_pitch(&doc, range.clone(), ratio, semitones, self.crossfades, &cancel)?;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        let new_len = range.end - range.start;
        let new_len = (new_len as f64 * ratio.clamp(0.1, 10.0)).round() as u64;
        self.history.push_edit(edit);
        let end = (range.start + new_len).min(self.len());
        Ok(Outcome { doc_changed: changed, selection: range.start..end, message: changed.then_some(label) })
    }

    /// Edit > Delete Silence with these settings, on `selection` (the whole file when empty),
    /// scanning the active channels and cutting all of them: one undo step "Delete Silence".
    /// Nothing is changed when it is refused.
    pub fn delete_silence(&mut self, selection: Range<u64>, params: &SilenceParams) -> Result<SilenceDone, SilenceError> {
        let doc = self.history.current().clone();
        let len = doc.len_frames();
        let sel = selection.start.min(len)..selection.end.min(len);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let channels = self.channels.within(doc.channel_count());
        let done = silence::delete_silence(&doc, sel.clone(), channels, params, self.crossfades, &cancel)?;
        let changed = !done.edit.is_noop();
        let message = if changed {
            let secs = done.removed_frames as f64 / f64::from(doc.sample_rate().max(1));
            format!("Delete Silence: {} stretches shortened, {secs:.3} s removed", done.stretches)
        } else {
            "Delete Silence: no silent stretch is long enough".to_string()
        };
        self.history.push_edit(done.edit);
        let new_len = self.len();
        let end = if sel.start < sel.end { sel.end.saturating_sub(done.removed_frames) } else { sel.end };
        let selection = sel.start.min(new_len)..end.max(sel.start).min(new_len);
        Ok(SilenceDone {
            outcome: Outcome { doc_changed: changed, selection, message: Some(message) },
            stretches: done.stretches,
            removed_frames: done.removed_frames,
        })
    }

    /// Insert > Audio from File: `file` (opened into this document's store with
    /// `OpenOptions::into_store`) goes in at the cursor, or in place of the selection, as one
    /// step. The inserted audio ends up selected. The errors are messages for the status bar.
    pub fn insert_file(&mut self, file: &crate::doc::DocState, selection: Range<u64>) -> Result<Outcome, String> {
        let doc = self.history.current().clone();
        let len = doc.len_frames();
        let sel = selection.start.min(len)..selection.end.min(len);
        if !doc.store().same_store(file.store()) {
            return Err("the file was not opened into this document's session store".to_string());
        }
        let at = if sel.start < sel.end { PasteAt::Replace(sel.clone()) } else { PasteAt::Insert(sel.start) };
        let edit = edit::insert_file(&doc, at, file, self.crossfades).map_err(|e| match e {
            EditError::SampleRate { clip, doc } => format!(
                "the file is at {clip} Hz and this document at {doc} Hz; converting sample rates is not supported yet, so nothing was inserted"
            ),
            EditError::Channels { clip, doc } => format!("a {clip}-channel file can't be inserted into a {doc}-channel document"),
            e => e.to_string(),
        })?;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        let end = (sel.start + file.len_frames()).min(self.len());
        Ok(Outcome { doc_changed: changed, selection: sel.start..end, message: changed.then_some(label) })
    }

    /// Edit › Mix Paste at `at`: `source` is the audio to mix in (a file opened as a clip), or
    /// the clipboard when `None`. The pasted stretch ends up selected. This and
    /// [`Editor::clipboard_clip`] are the only places Mix Paste touches the clipboard.
    pub fn mix_paste(&mut self, at: u64, source: Option<Clip>, opts: &MixOptions, cancel: &AtomicBool) -> Result<Outcome, EditError> {
        let doc = self.history.current().clone();
        let at = at.min(doc.len_frames());
        let Some(clip) = source.or_else(|| self.clipboard_clip()) else {
            return Ok(Outcome { doc_changed: false, selection: at..at, message: Some("Mix Paste: the clipboard is empty".to_string()) });
        };
        let channels = self.channels.within(doc.channel_count());
        let pasted = edit::mix_paste_frames(&doc, at, &clip, channels, opts);
        let e = edit::mix_paste(&doc, at, &clip, channels, opts, cancel)?;
        let changed = !e.is_noop();
        let label = e.label.clone();
        self.history.push_edit(e);
        let new_len = self.len();
        Ok(Outcome { doc_changed: changed, selection: at.min(new_len)..(at + pasted).min(new_len), message: changed.then_some(label) })
    }

    /// The clipboard's audio (the one place its content is read).
    fn clipboard_clip(&self) -> Option<Clip> {
        self.clipboard().cloned()
    }

    /// Rename marker `id` (one history step, "Rename Marker"). Returns whether anything changed.
    pub fn rename_marker(&mut self, id: u64, name: &str) -> bool {
        let Some(markers) = self.history.current().markers().with_renamed(id, name) else { return false };
        self.history.push_markers("Rename Marker", Arc::new(markers));
        true
    }

    /// Merge markers `ids` into one range (one history step, "Merge Markers", f58). Returns
    /// whether anything changed.
    pub fn merge_markers(&mut self, ids: &[u64]) -> bool {
        let Some(markers) = self.history.current().markers().merged(ids) else { return false };
        self.history.push_markers("Merge Markers", Arc::new(markers));
        true
    }

    /// Give marker `id` colour `color` (0 clears; one history step, "Marker Colour", f58).
    pub fn set_marker_color(&mut self, id: u64, color: u8) -> bool {
        let Some(markers) = self.history.current().markers().with_color(id, color) else { return false };
        self.history.push_markers("Marker Colour", Arc::new(markers));
        true
    }

    /// Set marker `id`'s description (one history step, "Marker Description", f58).
    pub fn set_marker_description(&mut self, id: u64, text: &str) -> bool {
        let Some(markers) = self.history.current().markers().with_description(id, text) else { return false };
        self.history.push_markers("Marker Description", Arc::new(markers));
        true
    }

    /// Move marker `id` so it starts at `start`, keeping its length (one history step,
    /// "Move Marker"). Returns whether anything changed.
    pub fn move_marker(&mut self, id: u64, start: u64) -> bool {
        let Some(markers) = self.history.current().markers().with_moved(id, start, self.len()) else { return false };
        self.history.push_markers("Move Marker", Arc::new(markers));
        true
    }

    /// Run `id` on `selection`. A disabled action does nothing and says why.
    pub fn run(&mut self, id: ActionId, selection: Range<u64>) -> Result<Outcome, EditError> {
        self.run_with(id, selection, None)
    }

    /// Run the effect at index `i` of [`effects::all`] with these settings on `selection` (the
    /// whole file when empty), and remember both for Repeat Last Command. Settings outside the
    /// effect's ranges are refused and nothing changes.
    pub fn run_effect(&mut self, i: u16, params: Vec<f64>, selection: Range<u64>) -> Result<Outcome, EditError> {
        self.run_with(ActionId::Effect(i), selection, Some(params))
    }

    /// Run Diagnostics › DeClipper with `[threshold_db, min_run]` (both inside the ranges of
    /// [`declip::Params`], or the call is refused and nothing changes) on `selection` (the whole
    /// file when empty); one undo step "DeClipper".
    pub fn run_declip(&mut self, params: Vec<f64>, selection: Range<u64>) -> Result<Outcome, EditError> {
        self.run_with(ActionId::DeClip, selection, Some(params))
    }

    /// Run Diagnostics › DeClicker with `[sensitivity, max_width_ms]` (inside the ranges of
    /// [`declick::Params`], or the call is refused and nothing changes) on `selection` (the whole
    /// file when empty); one undo step "DeClicker".
    pub fn run_declick(&mut self, params: Vec<f64>, selection: Range<u64>) -> Result<Outcome, EditError> {
        self.run_with(ActionId::DeClick, selection, Some(params))
    }

    /// Run Diagnostics › Automatic Click Remover with `[threshold, complexity]` on `selection`
    /// (the whole file when empty); one undo step "Automatic Click Remover".
    pub fn run_auto_click(&mut self, params: Vec<f64>, selection: Range<u64>) -> Result<Outcome, EditError> {
        self.run_with(ActionId::AutoClick, selection, Some(params))
    }

    /// Process `selection` (the whole file when empty) through the Effects Rack as one undo step
    /// named "Effects Rack" (f31). An inactive rack changes nothing.
    pub fn run_rack(&mut self, rack: &crate::rack::RackConfig, selection: Range<u64>) -> Result<Outcome, EditError> {
        let len = self.len();
        let sel = selection.start.min(len)..selection.end.min(len);
        if len == 0 || !rack.is_active() {
            return Ok(Outcome { doc_changed: false, selection: sel, message: Some("The Effects Rack has nothing switched on".to_string()) });
        }
        let doc = self.history.current().clone();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let edit = crate::rack::apply(rack, &doc, sel.clone(), self.channels.within(doc.channel_count()), self.crossfades, &cancel)?;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        Ok(Outcome { doc_changed: changed, selection: sel, message: changed.then_some(label) })
    }

    /// Like [`Editor::run_rack`], but the rack rings out: `tail_frames` frames are added after the
    /// selection (the whole file when empty) and filled with what the rack does to silence
    /// ([`crate::rack::apply_with_tail`]). One undo step named "Effects Rack". The returned
    /// selection is the processed range plus the tail. An inactive rack changes nothing and adds
    /// no tail.
    pub fn run_rack_with_tail(&mut self, rack: &crate::rack::RackConfig, selection: Range<u64>, tail_frames: u64) -> Result<Outcome, EditError> {
        let len = self.len();
        let sel = selection.start.min(len)..selection.end.min(len);
        if len == 0 || !rack.is_active() {
            return Ok(Outcome { doc_changed: false, selection: sel, message: Some("The Effects Rack has nothing switched on".to_string()) });
        }
        let doc = self.history.current().clone();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let edit =
            crate::rack::apply_with_tail(rack, &doc, sel.clone(), self.channels.within(doc.channel_count()), self.crossfades, &cancel, tail_frames)?;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        let (start, end) = if sel.start < sel.end { (sel.start, sel.end) } else { (0, len) };
        Ok(Outcome { doc_changed: changed, selection: start..end + tail_frames, message: changed.then_some(label) })
    }

    fn run_with(&mut self, id: ActionId, selection: Range<u64>, params: Option<Vec<f64>>) -> Result<Outcome, EditError> {
        let len = self.len();
        let sel = selection.start.min(len)..selection.end.min(len);
        let unchanged = |selection: Range<u64>| Ok(Outcome { doc_changed: false, selection, message: None });
        if !self.enabled(id, &sel) {
            return Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("{} is not available now", action(id).menu)) });
        }
        if repeatable(id) {
            self.last = Some(Repeat::Action(id));
        }
        match id {
            ActionId::RepeatLast => return self.repeat(sel),
            ActionId::SnapToggle
            | ActionId::SnapZeroToggle
            | ActionId::SnapRulerCoarseToggle
            | ActionId::SnapRulerFineToggle
            | ActionId::SnapEdgesToggle
            | ActionId::SnapMarkersToggle => {
                return Ok(Outcome { doc_changed: false, selection: sel, message: toggle_snap(&mut self.snap, id) });
            }
            ActionId::ToggleChannel(n) => return Ok(self.toggle_channel(usize::from(n), sel)),
            ActionId::DeleteSilence => {
                return match self.delete_silence(sel.clone(), &SilenceParams::default()) {
                    Ok(done) => Ok(done.outcome),
                    Err(SilenceError::Edit(e)) => Err(e),
                    // A refusal changes nothing and says why.
                    Err(why) => Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("Delete Silence: {why}")) }),
                };
            }
            ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => return self.zero_crossings(id, sel),
            _ => {}
        }
        let doc = self.history.current().clone();
        let (xf, channels) = (self.crossfades, self.channels.within(doc.channel_count()));
        let whole_if_empty = |r: &Range<u64>| if r.start < r.end { r.clone() } else { 0..len };
        let edit = match id {
            ActionId::Undo | ActionId::Redo => {
                let moved = if id == ActionId::Undo { self.history.undo() } else { self.history.redo() };
                let new_len = self.len();
                let selection = sel.start.min(new_len)..sel.end.min(new_len);
                return Ok(Outcome { doc_changed: moved.is_some(), selection, message: None });
            }
            ActionId::Copy => {
                let clip = edit::copy(&doc, whole_if_empty(&sel), channels)?;
                self.set_clipboard(clip);
                return unchanged(sel);
            }
            ActionId::Cut => {
                let range = whole_if_empty(&sel);
                let (e, clip) = edit::cut(&doc, range.clone(), channels, xf)?;
                self.set_clipboard(clip);
                let at = if e.state.len_frames() < len { range.start..range.start } else { range };
                (e, at)
            }
            ActionId::Paste => {
                let Some(clip) = self.clipboard().cloned() else { return unchanged(sel) };
                let at = if sel.start < sel.end { PasteAt::Replace(sel.clone()) } else { PasteAt::Insert(sel.start) };
                let e = edit::paste(&doc, at, &clip, channels, xf)?;
                // The pasted audio ends up selected.
                let pasted = clip.len_frames();
                (e, sel.start..sel.start + pasted)
            }
            ActionId::Clipboard1 | ActionId::Clipboard2 | ActionId::Clipboard3 | ActionId::Clipboard4 | ActionId::Clipboard5 => {
                self.current_clipboard = clipboard_index(id).unwrap_or(0);
                let msg = format!("Clipboard {} is current", self.current_clipboard + 1);
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(msg) });
            }
            ActionId::Delete => {
                let e = edit::delete(&doc, sel.clone(), channels, xf)?;
                let at = if e.state.len_frames() < len { sel.start..sel.start } else { sel.clone() };
                (e, at)
            }
            ActionId::Silence => (edit::silence(&doc, sel.clone(), channels, xf)?, sel.clone()),
            ActionId::Crop => {
                let e = edit::crop(&doc, sel.clone(), channels, xf)?;
                let new_len = e.state.len_frames();
                let at = if new_len < len { 0..new_len } else { sel.clone() };
                (e, at)
            }
            ActionId::Effect(i) => {
                let Some(effect) = effects::all().get(usize::from(i)) else { return unchanged(sel) };
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let params = params.unwrap_or_else(|| effects::Params::defaults(*effect));
                let e = effects::apply(*effect, &params, &doc, sel.clone(), channels, xf, &cancel)?;
                self.last = Some(Repeat::Effect(i, params));
                (e, sel.clone())
            }
            ActionId::DeClip => {
                let defaults = declip::Params::default();
                let v = params.unwrap_or_else(|| vec![defaults.threshold_db, f64::from(defaults.min_run)]);
                let min_run = match v.get(1) {
                    Some(m) if m.fract() == 0.0 && *m >= 0.0 && *m <= f64::from(u32::MAX) => *m as u32,
                    _ => return Err(EditError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "min_run must be a whole number"))),
                };
                let p = declip::Params { threshold_db: v.first().copied().unwrap_or(f64::NAN), min_run };
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let r = declip::declip(&doc, sel.clone(), channels, &p, xf, &cancel)?;
                self.last = Some(Repeat::DeClip(v));
                (r.edit, sel.clone())
            }
            ActionId::DeClick => {
                let d = declick::Params::default();
                let v = params.unwrap_or_else(|| vec![d.sensitivity, d.max_width_ms]);
                let p = declick::Params::from_list(&v);
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let r = declick::declick(&doc, sel.clone(), channels, &p, xf, &cancel)?;
                self.last = Some(Repeat::DeClick(v));
                (r.edit, sel.clone())
            }
            ActionId::AutoClick => {
                let d = declick::AutoParams::default();
                let v = params.unwrap_or_else(|| vec![d.threshold, d.complexity]);
                let p = declick::AutoParams::from_list(&v);
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let r = declick::auto_click_remove(&doc, sel.clone(), channels, &p, xf, &cancel)?;
                self.last = Some(Repeat::AutoClick(v));
                (r.edit, sel.clone())
            }
            ActionId::Reverse
            | ActionId::Amplify
            | ActionId::Normalize
            | ActionId::FadeInLinear
            | ActionId::FadeInCosine
            | ActionId::FadeOutLinear
            | ActionId::FadeOutCosine => {
                let range = whole_if_empty(&sel);
                let e = match id {
                    ActionId::Reverse => effects::reverse(&doc, range, channels, xf)?,
                    ActionId::Amplify => effects::amplify(&doc, range, channels, self.params.amplify_db, xf)?,
                    ActionId::Normalize => effects::normalize(&doc, range, channels, self.params.normalize_db, xf)?,
                    ActionId::FadeInLinear => effects::fade(&doc, range, channels, FadeDir::In, FadeCurve::Linear, xf)?,
                    ActionId::FadeInCosine => effects::fade(&doc, range, channels, FadeDir::In, FadeCurve::RaisedCosine, xf)?,
                    ActionId::FadeOutLinear => effects::fade(&doc, range, channels, FadeDir::Out, FadeCurve::Linear, xf)?,
                    _ => effects::fade(&doc, range, channels, FadeDir::Out, FadeCurve::RaisedCosine, xf)?,
                };
                (e, sel.clone())
            }
            ActionId::CaptureNoisePrint => {
                let print = crate::noise::capture(&doc, sel.clone()).map_err(|why| EditError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, why)))?;
                let message = format!(
                    "Noise print captured: {:.2} s, {} channel(s), {} frames averaged",
                    (sel.end - sel.start) as f64 / f64::from(doc.sample_rate().max(1)),
                    print.density.len(),
                    print.frames
                );
                crate::noise::set_current(print);
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(message) });
            }
            ActionId::SelectAll => return unchanged(0..len),
            ActionId::SelectToStart => return unchanged(crate::view::select_to_start(&sel, len)),
            ActionId::SelectToEnd => return unchanged(crate::view::select_to_end(&sel, len)),
            ActionId::BothChannels => {
                self.channels = ChannelMask::ALL;
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some("Editing both channels".to_string()) });
            }
            ActionId::DeselectAll | ActionId::ClearTimeSelection => return unchanged(sel.start..sel.start),
            ActionId::CursorToStart => return unchanged(0..0),
            ActionId::CursorToEnd => return unchanged(len..len),
            ActionId::MarkerAdd => {
                // A selection makes a range marker; otherwise a point at the cursor.
                let Some(markers) = doc.markers().with_added(sel.clone(), None) else {
                    return Ok(Outcome { doc_changed: false, selection: sel, message: Some("There is a marker here already".to_string()) });
                };
                self.history.push_markers("Add Marker", Arc::new(markers));
                // Markers draw over the same audio: the views need no rebuild.
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some("Add Marker".to_string()) });
            }
            ActionId::MarkerDelete => {
                let ids = doc.markers().touched(&sel);
                let Some(markers) = doc.markers().without(&ids) else { return Ok(Outcome { doc_changed: false, selection: sel, message: Some("No marker here".to_string()) }) };
                self.history.push_markers("Delete Marker", Arc::new(markers));
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some("Delete Marker".to_string()) });
            }
            ActionId::MarkerToRange | ActionId::MarkerToPoint => {
                let to_range = id == ActionId::MarkerToRange;
                let found = doc.markers().touched_kind(&sel, to_range).first().copied();
                let extent = found.and_then(|i| doc.markers().iter().find(|m| m.id == i)).map(|m| if to_range { sel.clone() } else { m.start..m.start });
                let changed = found.zip(extent).and_then(|(i, r)| doc.markers().with_extent(i, r));
                let label = if to_range { "Convert Marker to Range" } else { "Convert Marker to Point" };
                let Some(markers) = changed else { return Ok(Outcome { doc_changed: false, selection: sel, message: Some(format!("{label} is not available now")) }) };
                self.history.push_markers(label, Arc::new(markers));
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(label.to_string()) });
            }
            ActionId::MarkerNext | ActionId::MarkerPrev => {
                let m = doc.markers();
                let to = if id == ActionId::MarkerNext { m.next_after(sel.start) } else { m.prev_before(sel.start) };
                return unchanged(to.map_or(sel.clone(), |f| f..f));
            }
            ActionId::CopyToNew
            | ActionId::PasteToNew
            | ActionId::CloseAll
            | ActionId::SaveAll
            | ActionId::InsertSilence
            | ActionId::InsertFile
            | ActionId::SelectCurrentView
            | ActionId::SetIn
            | ActionId::SetOut
            | ActionId::ZoomIn
            | ActionId::ZoomOut
            | ActionId::ZoomToSelection
            | ActionId::ZoomOutFull
            | ActionId::ViewSyncSelection
            | ActionId::ViewSyncZoom
            | ActionId::ViewSyncPlayhead
            | ActionId::ExtractChannels
            | ActionId::ExportRanges
            | ActionId::ToolSelect
            | ActionId::ToolHand
            | ActionId::ToolZoom
            | ActionId::ToolMarquee
            | ActionId::TimeDecimal
            | ActionId::TimeCompact
            | ActionId::TimeSamples
            | ActionId::TimeSmpte24
            | ActionId::TimeSmpte25
            | ActionId::TimeSmpte2997Df
            | ActionId::TimeSmpte2997Ndf
            | ActionId::TimeSmpte30
            | ActionId::TimeSmpte5994
            | ActionId::TimeCustom
            | ActionId::TimeCd75
            | ActionId::TimeBarsBeats
            | ActionId::EditTempo
            | ActionId::SnapFrames
            | ActionId::SnapToggle
            | ActionId::SnapZeroToggle
            | ActionId::SnapRulerCoarseToggle
            | ActionId::SnapRulerFineToggle
            | ActionId::SnapEdgesToggle
            | ActionId::SnapMarkersToggle
            | ActionId::ConvertSampleType
            | ActionId::InterpretSampleRate
            | ActionId::ToggleChannel(_)
            | ActionId::GotoTime
            // Repeat, Delete Silence and the zero-crossing adjusts returned above.
            | ActionId::DeleteSilence
            | ActionId::Play
            | ActionId::Pause
            | ActionId::Stop
            | ActionId::ToggleLoop
            | ActionId::ToggleSkipSelection
            | ActionId::ToggleReturnOnStop
            | ActionId::ToggleAutoScroll
            | ActionId::FollowPage
            | ActionId::FollowCenter
            | ActionId::Rewind
            | ActionId::FastForward
            | ActionId::ToggleRoll
            | ActionId::ZoomAmpIn
            | ActionId::ZoomAmpOut
            | ActionId::ZoomAmpReset
            | ActionId::ZoomInAtIn
            | ActionId::ZoomInAtOut
            | ActionId::ZoomPreset1
            | ActionId::ZoomPreset2
            | ActionId::ZoomPreset3
            | ActionId::ZoomPreset4
            | ActionId::ZoomPreset5
            | ActionId::SpectralResolutionUp
            | ActionId::SpectralResolutionDown
            | ActionId::ChannelsLayered
            | ActionId::ChannelsStacked
            // Repeat and the zero-crossing adjusts returned above.
            | ActionId::RepeatLastDialog
            | ActionId::MeterInputToggle
            | ActionId::MeterInputArm
            | ActionId::MicCheck
            | ActionId::RepeatLast
            | ActionId::StatsShow
            | ActionId::StatsGotoPeak
            | ActionId::StatsNextClip
            | ActionId::StatsPrevClip
            | ActionId::ZeroInward
            | ActionId::ZeroOutward
            | ActionId::ZeroLeftToLeft
            | ActionId::ZeroLeftToRight
            | ActionId::ZeroRightToLeft
            | ActionId::ZeroRightToRight => return unchanged(sel),
            ActionId::LeftChannel | ActionId::RightChannel => {
                let ch = usize::from(id == ActionId::RightChannel);
                self.channels =
                    if self.channels.within(doc.channel_count()) == ChannelMask::only(ch) { ChannelMask::ALL } else { ChannelMask::only(ch) };
                let msg = if self.channels == ChannelMask::ALL {
                    "Editing both channels".to_string()
                } else {
                    format!("Editing the {} channel only", ["left", "right"][ch])
                };
                return Ok(Outcome { doc_changed: false, selection: sel, message: Some(msg) });
            }
        };
        let (edit, selection) = edit;
        let changed = !edit.is_noop();
        let label = edit.label.clone();
        self.history.push_edit(edit);
        let new_len = self.len();
        Ok(Outcome { doc_changed: changed, selection: selection.start.min(new_len)..selection.end.min(new_len), message: changed.then_some(label) })
    }
}

pub const HUD_DB_PER_PX: f64 = 0.1;
pub const HUD_MIN_DB: f64 = -60.0;
pub const HUD_MAX_DB: f64 = 12.0;

/// New HUD gain after a vertical drag. `drag_up_px` is positive when the pointer moves up.
/// A non-finite `db` becomes 0.0. A non-finite `drag_up_px` leaves `db` unchanged (clamped).
/// The result is clamped to [HUD_MIN_DB, HUD_MAX_DB].
pub fn hud_drag_gain_db(db: f64, drag_up_px: f64) -> f64 {
    let db = if db.is_finite() { db } else { 0.0 };
    if !drag_up_px.is_finite() {
        return db.clamp(HUD_MIN_DB, HUD_MAX_DB);
    }
    (db + drag_up_px * HUD_DB_PER_PX).clamp(HUD_MIN_DB, HUD_MAX_DB)
}

/// Whether the HUD is drawn. Hidden always wins; "show only for selections" needs a selection.
pub fn hud_visible(hidden: bool, only_for_selection: bool, has_selection: bool) -> bool {
    !hidden && (!only_for_selection || has_selection)
}

#[cfg(test)]
mod hud_tests {
    use super::*;

    #[test]
    fn hud_drag_moves_by_rate_per_pixel() {
        assert!((hud_drag_gain_db(0.0, 100.0) - 10.0).abs() < 1e-9);
        assert!((hud_drag_gain_db(0.0, -100.0) + 10.0).abs() < 1e-9);
        assert!((hud_drag_gain_db(-12.5, 25.0) + 10.0).abs() < 1e-9);
    }

    #[test]
    fn hud_drag_clamps_to_the_range() {
        assert!((hud_drag_gain_db(10.0, 100.0) - 12.0).abs() < 1e-9);
        assert!((hud_drag_gain_db(-55.0, -100.0) + 60.0).abs() < 1e-9);
    }

    #[test]
    fn hud_drag_ignores_non_finite_input() {
        assert!((hud_drag_gain_db(-3.0, f64::NAN) + 3.0).abs() < 1e-9);
        assert!((hud_drag_gain_db(f64::NAN, 50.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn hud_visibility_table() {
        // (hidden, only_for_selection, has_selection) -> visible
        assert!(hud_visible(false, false, false));
        assert!(hud_visible(false, false, true));
        assert!(!hud_visible(false, true, false));
        assert!(hud_visible(false, true, true));
        assert!(!hud_visible(true, false, false));
        assert!(!hud_visible(true, false, true));
        assert!(!hud_visible(true, true, false));
        assert!(!hud_visible(true, true, true));
    }
}
