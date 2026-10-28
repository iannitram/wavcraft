//! The window: menu, the editor (time ruler, waveform, spectral display) and the status bar.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use wavcraft_core::actions::{self, ActionId, Editor, Kind, Repeat, TABLE};
use wavcraft_core::appearance;
use wavcraft_core::bundle;
use wavcraft_core::chanmap::ChannelMap;
use wavcraft_core::convert::{self, ConvertSettings, PresetFile};
use wavcraft_core::doc::append::{self, AppendReport, NewReport};
use wavcraft_core::doc::edit as doc_edit;
use wavcraft_core::doc::newdoc::{self, NewDocOptions};
use wavcraft_core::doc::span::EditSpan;
use wavcraft_core::export::{self, DEFAULT_CHANNEL_PATTERN, DEFAULT_RANGE_PATTERN, ExportOptions};
use wavcraft_core::export_presets::{ExportChoice, ExportPresets};
use wavcraft_core::freq_axis::FreqAxis;
use wavcraft_core::hwprofile::{Hardware, Outcome, ProfileStore};
use wavcraft_core::i18n::{self, tr, tr_args};
use wavcraft_core::input_meter::InputStats;
use wavcraft_core::io::{OpenOptions, RawEncoding, RawSpec};
use wavcraft_core::jobs::{self, JobPoll};
use wavcraft_core::keyboard::Mods;
use wavcraft_core::match_loudness::Target;
use wavcraft_core::meta;
use wavcraft_core::monitors::{self, Monitor};
use wavcraft_core::panels::{self, DockSide, Layout, MetadataDisplay, MetadataField, PanelSet};
use wavcraft_core::probe::{self, FileInfo};
use wavcraft_core::readout::{self, EditorLayout, Readout};
use wavcraft_core::recent::{self, Recent};
use wavcraft_core::resample::Quality;
use wavcraft_core::save::{
    AiffKind, Container, ExtraChunk, FileKind, Part, SampleType, SaveError, SaveFormat, SaveOptions, SizeEstimate, VORBIS_DEFAULT_QUALITY,
    estimate_size, save_aiff_kind, save_as as save_container, save_vorbis, save_wav_with, write_file,
};
use wavcraft_core::settings::{self, FolderSource, SessionFolder, Settings};
use wavcraft_core::shortcut_sets::{DEFAULT_SET, SetStore};
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};
use wavcraft_core::snap::SnapSettings;
use wavcraft_core::spectrogram::Columns;
use wavcraft_core::statusbar::{self, StatusField};
use wavcraft_core::store::default_sessions_root;
use wavcraft_core::transport::{self, Follow, Mode, Scrub as DragTracker, Transport};
use wavcraft_core::view::{self, Band, SpectralSelection, nav_hit};

mod spectral;
use wavcraft_core::workspaces::{self, Snapshot, Workspaces};
use wavcraft_core::{
    AmpScale, AmpZoom, ChannelLayout, Clip, DocState, DocView, Document, FileFormat, GRAB_PX, History, NavPart, OpenError, SampleFormat, Selection,
    SpectrogramParams, TimeFormat, Viewport, channel_color, tempo::Tempo,
};

use crate::batch_panel::BatchPanel;
use crate::cd_ui::CdView;
use crate::control;
use crate::convert_ui::{self, ConvertDialog};
use crate::diagnostics_panel::{self, Ask, DiagPanel};
use crate::display;
use crate::doclist::{DocList, Where};
use crate::effect_dialog::{Answer, EffectDialog};
use crate::files;
use crate::freq_panel::{self, FreqPanel};
use crate::input::InputMonitor;
use crate::markers_panel::MarkersUi;
use crate::match_ui::MatchUi;
use crate::menus::{self, Cmd, Entry, Item, Menu};
use crate::meters_ui::{self, MetersUi};
use crate::mixer::MixerModel;
use crate::mixpaste;
use crate::multitrack_ui::MtView;
use crate::player::{Devices, Metronome, OutputChoice, Player};
use crate::playlist_panel::PlaylistUi;
use crate::rack_panel::{RackPanel, Request};
use crate::recording::{DEFAULT_PREROLL_MS, Max, Recording};
use crate::recovery::{self, Choice, Recovery};
use crate::shortcut_editor::{Change, ShortcutDialog};
use crate::spectrum::{Job, Renderer};
use crate::statspanel::{Nav, StatsPanel};
use crate::video::{VideoPanel, Waiters};
use wavcraft_core::doc::rec_edit::{self, RecMode, RecPlan, RecTarget};
use wavcraft_core::multitrack::soundcraft_model::{SourceId, TrackId};

const BACKGROUND: Color32 = Color32::from_rgb(18, 20, 24);
const BACKGROUND_LIGHT: Color32 = Color32::from_rgb(246, 247, 249);
const SELECTION: Color32 = Color32::from_rgba_premultiplied(70, 90, 130, 90);
const CURSOR: Color32 = Color32::from_rgb(235, 80, 70);
const PLAYHEAD: Color32 = Color32::from_rgb(250, 250, 250);
const ACCENT: Color32 = Color32::from_rgb(90, 140, 230);
const RULER_HEIGHT: f32 = 22.0;
/// Width of the frequency ruler on the left of the spectral display.
const FREQ_RULER_WIDTH: f32 = 54.0;
/// How long the free-space figure in the status bar is kept before it is read again.
const FREE_SPACE_TTL: std::time::Duration = std::time::Duration::from_secs(5);
/// The amplitude ruler's column, left of the editor.
const AMP_RULER_WIDTH: f32 = 46.0;
const NAV_HEIGHT: f32 = 34.0;
const BAND_FILL: Color32 = Color32::from_rgba_premultiplied(60, 70, 20, 60);
const BAND_OUTLINE: Color32 = Color32::from_rgb(240, 220, 90);

/// A Marquee drag in progress: drawing a new rectangle, or moving the one the drag began inside.
#[derive(Clone, Copy)]
enum MarqueeDrag {
    Draw { frame0: usize, hz0: f32, ch: usize },
    Move { from: SpectralSelection, frame0: usize, hz0: f32, ch: usize },
}

/// Longest silence Insert Silence accepts, in seconds (24 hours).
const MAX_SILENCE_SECS: f64 = 86_400.0;

/// Frames for an Insert Silence of `secs` seconds at `rate` Hz, or an error message when it is longer than 24 hours.
fn silence_frames(secs: f64, rate: u32) -> Result<u64, String> {
    if !secs.is_finite() || secs <= 0.0 || secs > MAX_SILENCE_SECS {
        return Err("Insert Silence: the length can be at most 24 hours".to_string());
    }
    Ok((secs * f64::from(rate)).round() as u64)
}

/// The Preferences dialog's pages (a page for each group of settings that exists today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefPage {
    General,
    Data,
    AutoSave,
    MediaCache,
    MarkersMetadata,
    PlaybackRecording,
    AudioHardware,
    SpectralDisplays,
    TimeDisplay,
    Loudness,
    Appearance,
    ChannelMapping,
}

impl PrefPage {
    const ALL: [PrefPage; 12] = [
        PrefPage::General,
        PrefPage::Appearance,
        PrefPage::Data,
        PrefPage::AutoSave,
        PrefPage::MediaCache,
        PrefPage::MarkersMetadata,
        PrefPage::PlaybackRecording,
        PrefPage::SpectralDisplays,
        PrefPage::TimeDisplay,
        PrefPage::Loudness,
        PrefPage::AudioHardware,
        PrefPage::ChannelMapping,
    ];

    fn title(self) -> &'static str {
        match self {
            PrefPage::General => "General",
            PrefPage::Data => "Data",
            PrefPage::AutoSave => "Auto Save",
            PrefPage::MediaCache => "Media and Disk Cache",
            PrefPage::MarkersMetadata => "Markers and Metadata",
            PrefPage::PlaybackRecording => "Playback and Recording",
            PrefPage::AudioHardware => "Audio Hardware",
            PrefPage::SpectralDisplays => "Spectral Displays",
            PrefPage::TimeDisplay => "Time Display",
            PrefPage::Loudness => "Loudness and Diagnostics",
            PrefPage::Appearance => "Appearance",
            PrefPage::ChannelMapping => "Audio Channel Mapping",
        }
    }
}

/// What a click in the Preferences dialog asks for (carried out after the dialog is drawn).
enum PrefAction {
    Set(&'static str, String),
    ChooseSessionFolder,
    Preset(&'static str, String),
    RefreshDevices,
    ResetAll,
    AskCleanup,
    CancelCleanup,
    CleanUp(u32),
    RefreshUsage,
    ClearMaps,
}

/// The output choice the preferences describe.
fn output_choice(s: &Settings) -> OutputChoice {
    OutputChoice { device: s.output_device.clone(), host: s.audio_host.clone(), buffer: s.buffer_frames }
}

/// The text of `map` with `from` (0-based) mapped to `to` (0-based; `None`: no pair). The first
/// time a map is set from the automatic layout, the file's other channels (`fill`, 0 for none)
/// keep their automatic place (channel n to output n) so setting one does not silence the rest.
fn map_text_with(map: &ChannelMap, from: usize, to: Option<usize>, fill: usize) -> String {
    let mut next = map.clone();
    if next.is_default() {
        for c in (0..fill).filter(|c| *c != from) {
            let _ = next.set(c, c);
        }
    }
    match to {
        Some(t) => {
            let _ = next.set(from, t);
        }
        None => next.remove(from),
    }
    next.to_string()
}

/// The settings a device setup's profile keeps (their change re-keys the setup's profile).
const HW_KEYS: [&str; 6] = ["audio_host", "output_device", "input_device", "buffer_frames", "output_map", "input_map"];

/// Frames at the start in which a held Shift means "reset the preferences".
const SHIFT_RESET_FRAMES: u32 = 30;

/// Dragging the playhead along the ruler (task f36): the drag asks for grains, the window plays them.
#[derive(Default)]
struct ScrubState {
    tracker: DragTracker,
    /// The drag is going on.
    dragging: bool,
    /// The latest frame the drag asked for and the window has not played yet.
    pending: Option<usize>,
    /// The drag ended; the window finishes it.
    ended: bool,
    /// Where the playhead is while scrubbing (and after, until something else moves it).
    pos: Option<usize>,
    /// The speed of the latest grain (signed).
    speed: f64,
}

/// A Rewind or Fast Forward button held down.
#[derive(Default)]
struct Hold {
    since: Option<f64>,
    engaged: bool,
    /// The release that ended a shuttle also clicks; that click is not a step.
    suppress_click: bool,
    /// K+J / K+L came from the control channel and lasts until `KEYJKL up`.
    ctl_nudge: bool,
}

/// A mic check listening (f56): the window answers when the time is up.
struct MicCheck {
    started: std::time::Instant,
    secs: f32,
    /// The control channel's `MICCHECK` waiting for the numbers.
    reply: Option<std::sync::mpsc::Sender<String>>,
}

/// The mic check dialog (Window > Mic Check...).
struct MicDialog {
    /// Seconds to listen, as typed.
    secs: f32,
    /// The last run: what was heard and the source it came from.
    result: Option<(InputStats, String)>,
}

/// A file being opened on a worker thread.
struct Opening {
    path: PathBuf,
    result: Receiver<Result<Document, String>>,
    /// Set to stop it (another file was opened meanwhile).
    cancel: Arc<AtomicBool>,
    /// How far the streaming has got, in thousandths.
    progress: Arc<AtomicU32>,
    /// Made here (a mixdown), not read from a file: it opens unsaved.
    fresh: bool,
    /// When it started, for the last operation's time (A327).
    started: Instant,
}

/// A background edit (roadmap B003) running on a worker thread: the window's thread pushes the
/// undo step when it finishes.
struct Running {
    job: jobs::Job,
}

/// What a shown size estimate was worked out for: the choice and the document step.
type SizeKey = (SaveFormat, SampleType, AiffKind, usize);

/// A file being streamed into the open document's session store for Insert > Audio from File.
struct Inserting {
    path: PathBuf,
    result: Receiver<Result<DocState, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    /// The document it belongs to ([`WaveApp::doc_id`] when it started): a result that arrives
    /// after another file was opened is dropped.
    doc_id: u64,
}

/// Extract Channels or Export Range Markers running on a worker thread (f79). The document is
/// not changed; the files are new ones beside it or in the chosen folder.
struct BatchWrite {
    what: &'static str,
    result: Receiver<Result<Vec<PathBuf>, String>>,
    progress: Arc<AtomicU32>,
}

/// A finished take being brought into the document on a worker thread (C026, C027, C030): the
/// take is opened from its own file by the normal streamed open, then one edit is built from it.
struct Integrating {
    result: Receiver<Result<wavcraft_core::Edit, String>>,
    base: Arc<DocState>,
    doc_id: u64,
    path: PathBuf,
    plan: RecPlan,
}

/// A new document being made from a clip on a worker thread (Copy to New, Paste to New).
struct Making {
    what: &'static str,
    result: Receiver<Result<DocState, String>>,
    progress: Arc<AtomicU32>,
}

/// Where Open Append puts the files.
enum AppendTo {
    /// The end of the document that was current, if it still is at the step it was at.
    Current { doc_id: u64, base: Arc<DocState> },
    /// A new document.
    New,
}

/// Files being added to a document, or made into a new one, on a worker thread (f57).
struct Appending {
    to: AppendTo,
    paths: usize,
    result: Receiver<Appended>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

/// What the Open Append worker found.
enum Appended {
    Current(AppendReport),
    New(NewReport),
}

/// File > New > Audio File…: the values in the dialog while it is up.
struct NewAudio {
    rate: u32,
    channels: usize,
    bits: u8,
}

impl Default for NewAudio {
    fn default() -> Self {
        NewAudio { rate: 48_000, channels: 2, bits: 32 }
    }
}

/// The sample type a bit depth in the dialog (or `bits=` of `NEWDOC`) means.
fn sample_format_for_bits(bits: u8) -> Option<SampleFormat> {
    match bits {
        16 => Some(SampleFormat::Int16),
        24 => Some(SampleFormat::Int24),
        32 => Some(SampleFormat::Float32),
        _ => None,
    }
}

/// File > Save All, working through the unsaved documents one save at a time.
struct SaveAllRun {
    /// `doc_id`s still to save.
    queue: Vec<u64>,
    /// May open a Save As dialog for a document with no file (the menu); a script's run skips it.
    interactive: bool,
    /// The document that was current when it started: made current again at the end.
    back_to: u64,
    /// Documents it could not save without a dialog.
    skipped: usize,
}

/// A save running on a worker thread.
struct Saving {
    path: PathBuf,
    /// The step being saved: it becomes the saved state when the save succeeds.
    state: Arc<DocState>,
    ty: SampleType,
    format: SaveFormat,
    aiff: AiffKind,
    /// Wave64, CAF or AU (f91) when the file is one of those: `format` is then WAV and unused.
    container: Option<Container>,
    result: Receiver<Result<(), SaveError>>,
    progress: Arc<AtomicU32>,
    /// Set by Cancel (status bar, `SAVE_CANCEL`) to stop a WAV save of the whole file: the temporary
    /// file is removed and the target stays as it was. `None` for the other formats and copies.
    cancel: Option<Arc<AtomicBool>>,
    /// Close the window once it is saved (Save from the close prompt).
    then_close: bool,
    /// Which open document it saves ([`WaveApp::doc_id`] when it started): a save that finishes
    /// after another file was opened must not give that file its path (Codex review CX-W1).
    doc_id: u64,
    /// A copy (Save Selection As, Export): the document keeps its own path and unsaved mark.
    copy: Option<&'static str>,
    /// When it started, for the last operation's time (A327).
    started: Instant,
}

/// An open document that is not the current one: the per-document fields of [`WaveApp`], parked
/// until it is switched to (the current document's own live in the window's fields).
struct DocSlot {
    doc: Option<Document>,
    editor: Option<Editor>,
    doc_path: PathBuf,
    doc_id: u64,
    saved: Option<Arc<DocState>>,
    view: Viewport,
    selection: Selection,
    rebuilding: Option<Receiver<Result<Document, OpenError>>>,
    tempo: Tempo,
    include_meta: bool,
}

/// Whether `editor`'s current step differs from `saved`, the one on disk.
fn is_unsaved(editor: &Option<Editor>, saved: &Option<Arc<DocState>>) -> bool {
    match (editor, saved) {
        (Some(e), Some(saved)) => !Arc::ptr_eq(e.history.current(), saved),
        (Some(_), None) => true,
        _ => false,
    }
}

/// A file added by File › Import (f63): listed in the Files panel, but not open. Only its header
/// was read (`info`); it gets a session store when it is first made current.
struct Import {
    path: PathBuf,
    info: FileInfo,
}

/// What the Files panel shows about an open document's file beyond its audio, from its header.
#[derive(Default, Clone)]
struct Meta {
    title: String,
    size: u64,
}

/// Whether two paths are the same file (equal, or the same after resolving links).
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// The Files panel's Bits text for a sample type.
fn bits_text(f: SampleFormat) -> &'static str {
    match f {
        SampleFormat::Int8 => "8",
        SampleFormat::Int16 => "16",
        SampleFormat::Int24 => "24",
        SampleFormat::Int32 => "32",
        SampleFormat::Float32 => "32f",
        SampleFormat::Float64 => "64f",
        SampleFormat::Compressed => "-",
    }
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Reveal in Explorer"
    } else {
        "Show in Folder"
    }
}

fn path_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned())
}

/// The newest spectrogram picture, one texture per channel, kept until the next one arrives.
struct SpectrumCache {
    job: Job,
    textures: Vec<TextureHandle>,
}

/// What a primary-button drag in the editor does (f64: a minimal switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    /// Click places the cursor, drag selects.
    #[default]
    Select,
    /// Drag scrolls the view.
    Hand,
    /// Click zooms in at the point (Alt: out), drag zooms to the range.
    Zoom,
    /// Drag draws a time x frequency rectangle on the spectral display (B135).
    Marquee,
}

#[derive(Default)]
struct HudState {
    gain_db: f64,
    pinned: bool,
    hidden: bool,
    only_for_selection: bool,
    drag_start_db: Option<f64>,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Hand => "hand",
            Tool::Zoom => "zoom",
            Tool::Marquee => "marquee",
        }
    }
}

pub struct WaveApp {
    /// The spectral divider moved or the display toggled and is not saved yet (saved once the pointer is up).
    spectral_dirty: bool,
    doc: Option<Document>,
    tool: Tool,
    /// Which context menu the last right-click in the editor's rectangle opened (A351).
    menu_area: Menu,
    /// The spectral selection's band, with the time range it was made on: it is live only while
    /// the selection is still that range (any other change of the selection drops it).
    band: Option<(Band, Selection)>,
    /// The Marquee tool's feather (Hz, ms): a new or changed band takes it.
    feather_hz: f32,
    feather_ms: f32,
    /// The Marquee drag in progress.
    marquee: Option<MarqueeDrag>,
    /// Where the current drag began (set when a drag starts, for the Zoom tool and the ruler).
    drag_origin: Option<Pos2>,
    /// Bumped on every open, so cached pictures of the previous file are dropped.
    generation: usize,
    view: Viewport,
    /// Vertical (amplitude) zoom of the waveform.
    amp_zoom: AmpZoom,
    /// The zoom navigator drag in progress: the part grabbed and the frames between the view's
    /// start and the pointer when it was grabbed.
    nav_drag: Option<(NavPart, f64)>,
    selection: Selection,
    player: Player,
    /// Play mode and options (loop, skip selection, return on stop, pre/post-roll).
    transport: Transport,
    head_scrub: ScrubState,
    hold: Hold,
    clock: Instant,
    params: SpectrogramParams,
    /// Which registered panels are shown (Window menu; kept in the settings file).
    panels: PanelSet,
    /// Where the panels are docked (kept in the settings file).
    layout: Layout,
    /// The panel under the pointer last frame, for the maximise and close keys.
    hover: Option<&'static str>,
    hover_next: Option<&'static str>,
    /// Where the docked tabs and zones were drawn this frame, for dropping a dragged tab.
    tab_rects: Vec<(String, Rect)>,
    zone_rects: Vec<(DockSide, Rect)>,
    /// The layout changed while the pointer was down (a zone being resized): save it on release.
    layout_dirty: bool,
    /// How the last floating window was made: "os" (own operating-system window) or "embedded"
    /// (an egui window inside the main one); reported by `PANELS`.
    float_kind: Option<&'static str>,
    /// The monitors, when they are faked (`MONITORS simulate`, `WAVCRAFT_MONITORS`); `None` means
    /// the real ones, which this build cannot list (egui reports only the size of the one
    /// monitor), so floating windows are then neither tagged nor moved.
    monitors: Option<Vec<Monitor>>,
    /// Saved workspaces (layout snapshots) and the one in use.
    workspaces: Workspaces,
    /// The name being typed in Window › Workspace › Save as New Workspace.
    workspace_name: String,
    /// Window › Frequency Analysis.
    freq: FreqPanel,
    spectrum: Option<SpectrumCache>,
    /// A drag on the frequency ruler, while the button is down.
    freq_drag: Option<spectral::FreqDrag>,
    /// Free space in the session folder's disk, when last read.
    free_space: Option<(Instant, Option<u64>)>,
    /// What the last operation was and how long it took (A327): open, save, or an edit.
    last_op: Option<(String, std::time::Duration)>,
    /// The editor's geometry as last painted, for the pointer readout and `HOVER` (A079).
    editor_layout: Option<EditorLayout>,
    /// What is under the pointer now (A079); `None` when it is not over the editor.
    pointer: Option<Readout>,
    renderer: Renderer,
    opening: Option<Opening>,
    /// Test hook (`HOLD_BUSY`, fake input only): counts as an open or save running until then.
    test_busy_until: Option<Instant>,
    /// Files still to open one after the other (Open with several files chosen, `OPEN_MANY`).
    open_queue: VecDeque<PathBuf>,
    /// The files of the queue that could not be opened, reported when the queue is empty.
    open_failures: Vec<String>,
    /// Open Append, while it runs.
    appending: Option<Appending>,
    inserting: Option<Inserting>,
    error: Option<String>,
    /// A message that is not an error (the session folder was changed).
    notice: Option<String>,
    /// Where opened files' session stores go, and how that was chosen.
    sessions: SessionFolder,
    settings: Settings,
    /// The settings file (`None` when the system has no config folder).
    settings_path: Option<PathBuf>,
    fit_on_next_frame: bool,
    /// The window title last sent. Sending one asks for a repaint, so it is only sent on change.
    title: String,
    /// The open file's editing state: its history, the active channels, the clipboard.
    editor: Option<Editor>,
    /// Where the open file came from (kept while an edit leaves the document empty).
    doc_path: PathBuf,
    /// Names the current document (the save in flight belongs to one).
    doc_id: u64,
    /// Counts the files opened in this window; the next `doc_id`.
    doc_serial: u64,
    /// The other open documents; the fields above and below hold the current one.
    docs: DocList<DocSlot>,
    /// The "save changes?" prompt is for closing the current document, not the window.
    closing_doc: bool,
    /// "Include markers and other metadata" for the open document (A262): starts as the setting,
    /// changed from File > Save As or `SAVE <path> meta=0|1`.
    include_meta: bool,
    /// The views' copy of the document, being read back after an edit (newest request only).
    rebuilding: Option<Receiver<Result<Document, OpenError>>>,
    /// Requests from the control channel, when `WAVCRAFT_CONTROL_PORT` turned it on.
    control: Option<Receiver<control::Request>>,
    /// The step last opened or saved: the file on disk holds it (no unsaved changes while the
    /// current step is this one; undoing back to it clears the mark, as in Audition).
    saved: Option<Arc<DocState>>,
    saving: Option<Saving>,
    /// Extract Channels / Export Range Markers (f79). Not the batch panel's `batch`.
    batch_write: Option<BatchWrite>,
    /// The background edit in progress, if any (no other edit, undo or save runs meanwhile).
    running: Option<Running>,
    /// File › Sample Type on Save: the type every save writes, or `None` to keep the file's own.
    save_type: Option<SampleType>,
    /// Ogg Vorbis VBR quality, -1 to 10 (`SAVE ... quality=`; sticky once given).
    vorbis_quality: f32,
    /// How AIFF files are written (the export dialog, an export preset, or `SAVE … aiff=`).
    aiff_kind: AiffKind,
    /// The Save As / Export dialog is up.
    export_open: bool,
    /// The control channel's answer to the next file dialog (`None` inside: the dialog was cancelled).
    scripted_pick: Option<Option<PathBuf>>,
    /// What the export dialog (and `SAVEOPTS`) has chosen.
    export_choice: ExportChoice,
    /// The named choices, and the file they are kept in.
    export_presets: ExportPresets,
    export_presets_path: Option<PathBuf>,
    /// The name typed in the dialog's preset box.
    export_preset_name: String,
    /// The estimated size shown, with what it was worked out for.
    size_cache: Option<(SizeKey, Result<SizeEstimate, String>)>,
    /// The format the next Save As dialog starts on (from the export dialog).
    dialog_format: Option<SaveFormat>,
    /// The "save changes before closing?" prompt is up.
    confirm_close: bool,
    /// Closing was confirmed (or nothing was unsaved): let the next close request through.
    allow_close: bool,
    /// The editor's width last frame, for zoom keys (which have no pointer to zoom around).
    editor_width: f32,
    /// A new document being made from a clip.
    making: Option<Making>,
    /// File > New > Audio File…, while its dialog is up.
    new_audio: Option<NewAudio>,
    /// File > Close All is going through the documents, asking about unsaved ones.
    close_all: bool,
    save_all: Option<SaveAllRun>,
    hud: HudState,
    /// Window › Mixer: the channel strips (C152). Its own flag, not a panels registry entry.
    show_mixer: bool,
    mixer: MixerModel,
    /// Insert › Silence…: the length being typed, in the time format, while the dialog is up.
    insert_silence: Option<String>,
    /// File › Import › Raw Data…: the settings being typed while the dialog is up.
    raw_import: Option<RawDialog>,
    /// Edit › Mix Paste…: the dialog's fields while it is up.
    mix_paste: Option<mixpaste::Dialog>,
    /// View › Time Display › Custom frame rate…: the rate being typed while the dialog is up (A102).
    custom_fps_dialog: Option<String>,
    /// Clear History asks first: it can't be undone.
    confirm_clear_history: bool,
    /// The saved window rectangle: restored and kept on a connected monitor (f76).
    keeper: display::WindowKeeper,
    /// The Files panel (f45, f63, f95; `files` in the panel registry).
    files: files::View,
    /// The window's egui context, for opening a file from a click that has none at hand.
    ctx: egui::Context,
    /// File › Open Recent (f63), and the file it is kept in (`None` when the system has no config folder).
    recent: Recent,
    recent_path: Option<PathBuf>,
    /// Files added by File › Import (f63), not open: listed after the open documents.
    imports: Vec<Import>,
    /// Header details of the open documents' files, by path (filled when the panel needs them).
    meta: HashMap<PathBuf, Meta>,
    /// Edit › Convert Sample Type… (A161): the dialog while it is up (the conversion itself runs
    /// as a background job, `running`).
    convert_dialog: Option<ConvertDialog>,
    /// Edit › Interpret Sample Rate… (A162): the rate being typed while the dialog is up.
    interpret: Option<String>,
    /// Named conversion settings (built in, and the user's from `convert-presets.txt`).
    convert_presets: PresetFile,
    /// Frames a selection drag snaps to besides the ruler: the edges of the selection that was
    /// there when the gesture began (the cursor is its start).
    snap_targets: Vec<u64>,
    /// Recording a take (File › New Recording…, the Record button, `RECORD` on the control channel).
    rec: Recording,
    /// A take being brought into the document.
    integrating: Option<Integrating>,
    /// Window › Markers: the panel, the picked markers, a handle being dragged.
    markers_ui: MarkersUi,
    /// The Playlist panel's list and its runner (session only).
    playlist_ui: PlaylistUi,
    /// Window › Metadata.
    show_metadata: bool,
    /// The Metadata panel's text boxes (one per `meta::FIELDS`) and the source they were filled
    /// from: refilled whenever the current step's metadata is another one.
    meta_draft: Vec<String>,
    meta_for: Option<Arc<wavcraft_core::SourceFormat>>,
    /// The Metadata panel's filter text, and the display sets (which fields it shows; f98).
    meta_filter: String,
    meta_sets: wavcraft_core::meta_view::DisplaySets,
    meta_set_name: String,
    /// Video › Attach video file…: the picture of a video file at the playhead (f122).
    video: Option<VideoPanel>,
    show_video: bool,
    /// Control-channel `VIDEO frame` requests waiting for their picture.
    video_waiters: Waiters,
    /// A History row had keyboard focus last frame: Up and Down move through the states then.
    history_focus: bool,
    /// The History row to give keyboard focus next frame (after an arrow key moved the cursor).
    history_refocus: Option<usize>,
    /// The effect parameter dialog (f14), while it is up.
    effect_dialog: Option<EffectDialog>,
    /// Effects › Diagnostics.
    diag: DiagPanel,
    /// A `DIAG scan` is waiting: the control request's reply goes to the scan, not the caller.
    diag_wait: bool,
    /// A Generate dialog's Preview is playing (f21): the player holds the preview, not the file.
    previewing: bool,
    /// The keyboard shortcuts in use (the defaults; the user's key file will override them).
    keymap: Keymap,
    /// Where the user's key file is (f46; `None`: the system has no config folder).
    shortcuts_path: Option<PathBuf>,
    /// The saved shortcut sets (f60, A293), in the config folder.
    shortcut_sets: Option<SetStore>,
    /// Edit › Keyboard Shortcuts…, while it is open.
    shortcut_dialog: Option<ShortcutDialog>,
    /// The Multitrack session and its editor view (f103).
    mtv: MtView,
    /// The CD Layout and its view (f121).
    cdv: CdView,
    /// Window › Levels and the statistics window (f26).
    meters: MetersUi,
    /// The Match Loudness panel (f33; `match` in the panel registry).
    matcher: MatchUi,
    /// The input monitor (f56): open only while `meter_input` is on or a mic check listens.
    input: InputMonitor,
    /// Window › Meter Input Signal: the Levels panel shows the input while nothing plays.
    meter_input: bool,
    mic: Option<MicCheck>,
    mic_dialog: Option<MicDialog>,
    /// Edit › Preferences…: the page showing while the dialog is up.
    prefs: Option<PrefPage>,
    /// Auto Save: the journal writer, and the offer to recover a crashed run's work.
    recovery: Recovery,
    /// The disk figures the Preferences pages show (Auto Save, Media and Disk Cache), made when
    /// one of them opens and refreshed after a change.
    prefs_usage: Option<wavcraft_core::sessions::Usage>,
    /// "Clean up old sessions" asked for and waiting for a yes.
    prefs_confirm_cleanup: bool,
    /// The look last given to egui: (dark, brightness in thousandths); `None` until the first frame.
    applied_look: Option<(bool, i32, i32)>,
    /// The spectrogram palette in use (name, brightness, contrast, as thousandths).
    applied_palette: Option<(settings::SpecPalette, i32, i32)>,
    /// Whether the dark theme is in force (from the Theme preference, or the system's), and the
    /// brightness: what the editor's own painting uses.
    dark: bool,
    brightness: f32,
    contrast: f32,
    /// The Appearance page's preset name box.
    preset_name: String,
    /// The devices the Audio Hardware page lists (read when the dialog opens or on Refresh).
    devices: Devices,
    /// The device list being read (the system can be slow to answer).
    devices_rx: Option<Receiver<Devices>>,
    /// The audio hardware settings kept per device setup (f97), and its file.
    profiles: ProfileStore,
    profiles_path: Option<PathBuf>,
    /// The system's devices being read, to pick the setup's profile (at launch).
    profile_rx: Option<Receiver<Vec<String>>>,
    /// Frames still to look at for a held Shift (Shift at launch resets the preferences).
    shift_frames: u32,
    /// Window › Sample Statistics.
    stats: StatsPanel,
    /// View › Time Display (kept in the settings file).
    time_format: TimeFormat,
    /// The open document's tempo and time signature (Bars and Beats).
    tempo: Tempo,
    /// Edit Tempo… dialog: the tempo and the time signature as typed.
    tempo_dialog: Option<(String, String)>,
    /// View › Snap to Frames: with a SMPTE time display, the cursor and selection edges set from
    /// the ruler land on frame boundaries. (PR #95 adds the general snapping switch; once both
    /// are merged this is "snapping is on" there, and `TimeFormat::snap_to_frame` is the call.)
    snap_frames: bool,
    /// View › Time panel.
    show_time_panel: bool,
    /// The Selection/View box being typed in: its name and its text.
    panel_edit: Option<(&'static str, String)>,
    /// The Effects Rack panel and its settings (f31).
    rack: RackPanel,
    /// A clip's source open in the Waveform editor, waiting for Return to session (f116).
    clip_edit: Option<ClipEdit>,
    /// Which metadata fields file-listing panels show (kept in the settings file).
    metadata: MetadataDisplay,
    /// The Metadata Display dialog is up.
    metadata_dialog: bool,
    /// File › Batch Process…
    batch: BatchPanel,
    /// Copy and Paste's side of the system clipboard.
    os_clipboard: crate::oscb::OsClipboard,
    /// The Time panel's readout being typed in (Go to Time), and a digit-drag in progress.
    goto: Option<String>,
    scrub: Option<Scrub>,
    /// Where saved waveform overviews live (`WAVCRAFT_PEAK_CACHE_DIR`, else the system cache).
    peak_dir: PathBuf,
    /// How the open file's overviews were made (cache hit or computed), for `PEAKCACHE info`.
    peak_report: Option<wavcraft_core::PeakReport>,
    /// Help › About is open (it shows the third-party notices).
    show_about: bool,
    /// Restore Preference Defaults asks first.
    confirm_reset_prefs: bool,
}

/// A clip's source open in the Waveform editor (f116).
struct ClipEdit {
    source: SourceId,
    name: String,
    /// The temp WAV the source was written to, and its folder (removed on return).
    path: PathBuf,
    dir: PathBuf,
    /// The file that was open before, opened again on return.
    back_to: Option<PathBuf>,
}

/// A drag across the digits of a time box: the frames one step is worth and the drag not yet used.
struct Scrub {
    name: &'static str,
    unit: usize,
    pending: f32,
}

/// Pixels of vertical drag for one step of the digit group.
const SCRUB_PX_PER_STEP: f32 = 6.0;

impl WaveApp {
    /// `session_dir`: the `--session-dir` flag, which wins over the environment variable and the
    /// settings file for this run.
    /// `reset_preferences`: the `--reset-preferences` flag, which moves the settings file aside
    /// before it is read.
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, session_dir: Option<PathBuf>, reset_preferences: bool) -> WaveApp {
        let settings_path = Settings::default_path();
        // `--reset-preferences`: the same reset as the menu's, before anything is read.
        let reset_note = match settings_path.as_deref().and_then(Path::parent) {
            Some(dir) if reset_preferences => Some(reset_preference_files(dir).unwrap_or_else(|e| e)),
            _ => None,
        };
        let settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        if let Some(code) = settings.language.as_deref()
            && !i18n::set_language(code)
        {
            log::warn!(target: "wavcraft", "the settings file names language {code:?}, which is not built in; using English");
        }
        let metadata = MetadataDisplay::from_ids(settings.metadata_fields.as_deref());
        let profiles_path = settings_path.as_deref().map(ProfileStore::default_path);
        let env = std::env::var_os(settings::ENV_SESSION_DIR);
        let sessions = settings::resolve_session_folder(session_dir, env, &settings, default_sessions_root(), settings::check_folder);
        // Folders left by runs that ended without closing their store (a crash, a kill, a
        // signal) are offered for recovery or swept in the background; this run's own store is
        // never touched.
        let roots = recovery::roots(&sessions.path, &default_sessions_root());
        let recovery = {
            let ctx = cc.egui_ctx.clone();
            Recovery::new(roots, crate::recording::record_dir().ok(), move || ctx.request_repaint())
        };
        // f46: the defaults with the user's key file applied; f60: the active shortcut set, when
        // there is one, is what the app starts with.
        let shortcuts_path = wavcraft_core::shortcuts::default_path();
        let (keymap, shortcuts_warning) = shortcuts_path.as_deref().map(wavcraft_core::shortcuts::load).unwrap_or_default();
        if let Some(w) = shortcuts_warning {
            log::warn!(target: "wavcraft", "{w}");
        }
        let shortcut_sets = SetStore::default_store();
        let (keymap, set_warning) = match &shortcut_sets {
            Some(store) => store.startup(keymap),
            None => (keymap, None),
        };
        if let Some(w) = set_warning {
            log::warn!(target: "wavcraft", "{w}");
        }
        if let Err(e) = keymap.check() {
            log::error!(target: "wavcraft", "keyboard shortcuts conflict: {e}");
        }
        let initial_palette = (settings.spec_palette, milli(settings.ui_brightness), milli(settings.ui_contrast));
        let initial_colours = palette(settings.spec_palette, settings.ui_brightness, settings.ui_contrast);
        let mut app = WaveApp {
            doc: None,
            generation: 0,
            view: Viewport { start: 0.0, frames_per_px: 1.0 },
            amp_zoom: AmpZoom::full(),
            nav_drag: None,
            tool: Tool::default(),
            menu_area: Menu::Editor,
            band: None,
            feather_hz: 0.0,
            feather_ms: 0.0,
            marquee: None,
            drag_origin: None,
            selection: Selection::default(),
            player: Player::new(output_choice(&settings), settings.output_map.clone(), settings.playback_quality),
            transport: Transport::default(),
            head_scrub: ScrubState::default(),
            hold: Hold::default(),
            clock: Instant::now(),
            params: {
                let mut p = SpectrogramParams::default();
                settings.apply_to_spectrogram(&mut p);
                if let Some(view) = settings.spectral_view {
                    view.apply_to(&mut p);
                }
                p
            },
            panels: PanelSet::from_hidden(&settings.hidden_panels),
            spectral_dirty: false,
            layout: Layout::decode(&settings.panel_layout, &settings.zone_sizes),
            hover: None,
            hover_next: None,
            tab_rects: Vec::new(),
            zone_rects: Vec::new(),
            layout_dirty: false,
            float_kind: None,
            monitors: std::env::var("WAVCRAFT_MONITORS").ok().and_then(|t| match monitors::parse(&t) {
                Ok(m) => Some(m),
                Err(e) => {
                    log::warn!(target: "wavcraft", "WAVCRAFT_MONITORS ignored: {e}");
                    None
                }
            }),
            workspaces: Workspaces::new(settings_path.as_deref().and_then(|p| p.parent()).map(|d| d.join("workspaces")), &settings.workspace),
            workspace_name: String::new(),
            freq: FreqPanel::default(),
            spectrum: None,
            freq_drag: None,
            free_space: None,
            last_op: None,
            editor_layout: None,
            pointer: None,
            renderer: Renderer::new(&cc.egui_ctx, initial_colours),
            opening: None,
            test_busy_until: None,
            open_queue: VecDeque::new(),
            open_failures: Vec::new(),
            appending: None,
            inserting: None,
            error: sessions.warning.clone(),
            notice: reset_note,
            time_format: settings.time_display,
            tempo: Tempo::default(),
            tempo_dialog: None,
            sessions,
            settings,
            settings_path,
            fit_on_next_frame: false,
            title: String::new(),
            editor: None,
            doc_path: PathBuf::new(),
            doc_id: 0,
            doc_serial: 0,
            docs: DocList::new(),
            closing_doc: false,
            include_meta: true,
            rebuilding: None,
            saved: None,
            saving: None,
            batch_write: None,
            running: None,
            save_type: None,
            vorbis_quality: VORBIS_DEFAULT_QUALITY,
            aiff_kind: AiffKind::Aiff,
            export_open: false,
            scripted_pick: None,
            export_choice: ExportChoice::default(),
            export_presets_path: ExportPresets::default_path(),
            export_presets: ExportPresets::default_path().as_deref().map(ExportPresets::load).unwrap_or_default(),
            export_preset_name: String::new(),
            size_cache: None,
            dialog_format: None,
            confirm_close: false,
            allow_close: false,
            os_clipboard: crate::oscb::OsClipboard::new(),
            editor_width: 800.0,
            making: None,
            new_audio: None,
            close_all: false,
            save_all: None,
            hud: HudState::default(),
            show_mixer: false,
            mixer: MixerModel::new(),
            snap_frames: true,
            show_time_panel: true,
            panel_edit: None,
            batch: BatchPanel::default(),
            goto: None,
            scrub: None,
            insert_silence: None,
            stats: StatsPanel::default(),
            raw_import: None,
            mix_paste: None,
            custom_fps_dialog: None,
            confirm_clear_history: false,
            keeper: display::WindowKeeper::new(display::WindowKeeper::default_path()),
            files: files::View { columns: files::load_columns().unwrap_or_else(|| files::Col::DEFAULT.to_vec()), ..files::View::default() },
            ctx: cc.egui_ctx.clone(),
            recent_path: recent::config_file("recent.txt"),
            recent: recent::config_file("recent.txt").map(|p| Recent::load(&p)).unwrap_or_default(),
            imports: Vec::new(),
            meta: HashMap::new(),
            convert_dialog: None,
            interpret: None,
            convert_presets: PresetFile::load(PresetFile::default_path()),
            snap_targets: Vec::new(),
            rec: Recording::default(),
            integrating: None,
            show_metadata: false,
            meta_draft: Vec::new(),
            meta_for: None,
            meta_filter: String::new(),
            meta_sets: wavcraft_core::meta_view::DisplaySets::default_path()
                .map(|p| wavcraft_core::meta_view::DisplaySets::load_file(&p))
                .unwrap_or_default(),
            meta_set_name: String::new(),
            video: None,
            show_video: true,
            video_waiters: Waiters::default(),
            history_focus: false,
            history_refocus: None,
            effect_dialog: None,
            diag: DiagPanel::new(),
            diag_wait: false,
            previewing: false,
            keymap,
            shortcuts_path,
            shortcut_sets,
            shortcut_dialog: None,
            mtv: MtView::default(),
            cdv: CdView::default(),
            meters: MetersUi::new(),
            matcher: MatchUi::new(),
            input: InputMonitor::new(),
            meter_input: false,
            mic: None,
            mic_dialog: None,
            prefs: None,
            recovery,
            prefs_usage: None,
            prefs_confirm_cleanup: false,
            applied_look: None,
            applied_palette: Some(initial_palette),
            dark: true,
            brightness: 1.0,
            contrast: 1.0,
            preset_name: String::new(),
            devices: Devices::default(),
            devices_rx: None,
            profiles: profiles_path.as_deref().map(ProfileStore::load).unwrap_or_default(),
            profiles_path,
            profile_rx: Some(crate::player::list_device_names_in_background(None)),
            shift_frames: SHIFT_RESET_FRAMES,
            rack: RackPanel::new(),
            clip_edit: None,
            metadata,
            metadata_dialog: false,
            markers_ui: MarkersUi::new(),
            playlist_ui: PlaylistUi::default(),
            peak_dir: std::env::var_os("WAVCRAFT_PEAK_CACHE_DIR").map_or_else(wavcraft_core::peakcache::default_dir, PathBuf::from),
            peak_report: None,
            show_about: false,
            confirm_reset_prefs: false,
            control: {
                let ctx = cc.egui_ctx.clone();
                control::start_from_env(move || ctx.request_repaint())
            },
        };
        app.mtv.take_dir = app.sessions.path.join("takes");
        if app.reconcile_floats() {
            app.save_panels();
        }
        if let Some(path) = file {
            app.open(path, &cc.egui_ctx);
        }
        app
    }

    /// Give egui the theme and brightness from the preferences (only when they changed, or when
    /// the system theme changed while the preference is "system").
    fn apply_look(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            settings::Theme::Dark => true,
            settings::Theme::Light => false,
            settings::Theme::System => ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark),
        };
        let key = (dark, milli(self.settings.ui_brightness), milli(self.settings.ui_contrast));
        self.dark = dark;
        self.brightness = self.settings.ui_brightness;
        self.contrast = self.settings.ui_contrast;
        let pal = (self.settings.spec_palette, key.1, key.2);
        if self.applied_palette != Some(pal) {
            self.applied_palette = Some(pal);
            self.renderer = Renderer::new(ctx, palette(pal.0, self.brightness, self.contrast));
            self.spectrum = None;
        }
        if self.applied_look == Some(key) {
            return;
        }
        self.applied_look = Some(key);
        let (b, k) = (self.brightness, self.contrast);
        let tint = |c: Color32| contrast(brighten(c, b), k);
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        for c in [&mut v.panel_fill, &mut v.window_fill, &mut v.extreme_bg_color, &mut v.faint_bg_color, &mut v.code_bg_color] {
            *c = tint(*c);
        }
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.bg_fill = tint(w.bg_fill);
            w.weak_bg_fill = tint(w.weak_bg_fill);
        }
        ctx.set_visuals(v);
    }

    /// Give the open file's editor the crossfade lengths from the preferences.
    fn apply_crossfades(&mut self) {
        let (cut_join, edge) = self.settings.crossfades();
        if let Some(e) = self.editor.as_mut() {
            e.crossfades = wavcraft_core::doc::edit::Crossfades { cut_join, edge };
        }
    }

    /// Give the open file's editor the snapping switches the owner left (they last from file to
    /// file and run to run).
    fn apply_snap(&mut self) {
        let snap = self.settings.snap;
        if let Some(e) = self.editor.as_mut() {
            e.snap = snap;
        }
    }

    /// Snapping's switches changed: remember them for the next file and the next run.
    fn keep_snap(&mut self, snap: SnapSettings) {
        if snap == self.settings.snap {
            return;
        }
        self.settings.snap = snap;
        if let Err(e) = self.save_settings() {
            log::warn!(target: "wavcraft", "{e}");
        }
    }

    /// Flip a snapping switch from a menu or key (works with no file open too).
    fn toggle_snap_menu(&mut self, id: ActionId) {
        let mut snap = self.settings.snap;
        self.notice = actions::toggle_snap(&mut snap, id);
        if let Some(e) = self.editor.as_mut() {
            e.snap = snap;
        }
        self.keep_snap(snap);
    }

    /// Write the settings file (the message says why not, in words).
    /// The visible range as `<first>-<last>` frames, for NAV.
    fn view_text(&self) -> String {
        let frames = self.doc.as_ref().map_or(0, Document::frames) as f64;
        format!("{}-{}", self.view.start.round(), self.view.end(self.editor_width).min(frames).round())
    }

    /// Use `scale` on the amplitude ruler and keep it in the settings file.
    fn set_amp_scale(&mut self, scale: AmpScale) {
        self.settings.amp_scale = scale;
        if let Err(e) = self.save_settings() {
            log::warn!(target: "wavcraft", "could not save the ruler scale: {e}");
        }
    }

    /// Put the time the window spans now in Zoom to Time slot `slot` (0-based); returns the seconds.
    fn set_zoom_slot(&mut self, slot: usize) -> Result<f64, String> {
        let rate = self.doc.as_ref().map_or(0, Document::sample_rate);
        if rate == 0 {
            return Err("no file is open".to_string());
        }
        // The view may show less than the window when the file is shorter than it.
        let secs = self.view.span_frames(self.editor_width.max(1.0)) / f64::from(rate);
        if !self.settings.zoom_presets.set(slot, secs) {
            return Err(format!("slot {} cannot hold {secs} s", slot + 1));
        }
        if let Err(e) = self.save_settings() {
            log::warn!(target: "wavcraft", "could not save the zoom slots: {e}");
        }
        Ok(self.settings.zoom_presets.0[slot])
    }

    /// Show the waveform's channels layered or stacked, and keep the choice in the settings file.
    fn set_channel_layout(&mut self, layout: ChannelLayout) {
        self.settings.channel_layout = layout;
        if let Err(e) = self.save_settings() {
            log::warn!(target: "wavcraft", "could not save the channel layout: {e}");
        }
    }

    fn save_settings(&self) -> Result<(), String> {
        match &self.settings_path {
            Some(file) => self.settings.save(file).map_err(|e| format!("could not save the settings to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so the setting lasts until WavCraft closes".to_string()),
        }
    }

    /// Set preference `key` (the dialog's and `PREF set`'s one way in): check the value, carry
    /// it out (reopen the output, change the crossfades, use the folder) and save the file.
    fn pref_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let mut next = self.settings.clone();
        next.set(key, value)?;
        if key == "session_folder" {
            let path = next.session_folder.clone();
            if let Some(p) = &path {
                settings::check_folder(p).map_err(|why| format!("the session folder {} can't be used: {why}", p.display()))?;
            }
            self.set_session_folder(path);
            return Ok(());
        }
        self.settings = next;
        if HW_KEYS.contains(&key) {
            self.remember_hardware();
        }
        match key {
            "crossfade_cut_join_ms" | "crossfade_edge_ms" => self.apply_crossfades(),
            "output_device" | "buffer_frames" => self.player.set_output(output_choice(&self.settings)),
            "audio_host" => {
                // The devices belong to the host: ask again, and reopen on it.
                self.player.set_output(output_choice(&self.settings));
                self.devices_rx = Some(crate::player::list_devices_in_background(self.settings.audio_host.clone()));
            }
            // sync_input reopens the monitor on the new device on the next frame.
            "input_device" => {}
            "output_map" => self.player.set_map(self.settings.output_map.clone()),
            "playback_quality" => self.player.set_quality(self.settings.playback_quality),
            "spec_resolution" | "spec_db_range" | "spec_window" => {
                self.settings.apply_to_spectrogram(&mut self.params);
                self.spectrum = None;
            }
            "time_display" => self.time_format = self.settings.time_display,
            // A new custom rate moves a live custom display with it (the setting itself is already stored).
            "custom_fps" => {
                if matches!(self.time_format, TimeFormat::Custom(_)) {
                    self.time_format = TimeFormat::Custom(self.settings.custom_fps);
                    self.panel_edit = None;
                }
            }
            _ => {}
        }
        self.save_settings()
    }

    /// `APPEARANCE preset save|load|delete <name>` and the page's buttons: change the presets
    /// (and, for load, the live appearance, which `apply_look` picks up) and save the file.
    fn preset_do(&mut self, verb: &str, name: &str) -> Result<(), String> {
        match verb {
            "save" => self.settings.save_preset(name)?,
            "load" => self.settings.load_preset(name)?,
            "delete" => self.settings.delete_preset(name)?,
            _ => return Err("APPEARANCE preset save|load|delete <name>".to_string()),
        }
        self.save_settings()
    }

    /// Keep the hardware settings as the active device setup's profile.
    fn remember_hardware(&mut self) {
        self.profiles.remember(&Hardware::of(&self.settings));
        self.save_profiles();
    }

    fn save_profiles(&self) {
        if let Some(p) = &self.profiles_path
            && let Err(e) = self.profiles.save(p)
        {
            log::warn!("could not save the audio profiles to {}: {e}", p.display());
        }
    }

    /// The device setup is now `outputs`/`inputs`: restore that setup's hardware settings (a
    /// stored device that is not on offer falls back to the system default) and reopen the output.
    fn activate_setup(&mut self, outputs: &[String], inputs: &[String]) -> Outcome {
        let (outcome, hw) = self.profiles.activate(outputs, inputs, &Hardware::of(&self.settings));
        if outcome != Outcome::Unchanged {
            hw.apply(&mut self.settings);
            self.player.set_output(output_choice(&self.settings));
            self.player.set_map(self.settings.output_map.clone());
            if let Err(e) = self.save_settings() {
                log::warn!("{e}");
            }
        }
        self.save_profiles();
        outcome
    }

    /// Turn the input meter on or off (opening or closing the input stream with it).
    fn set_meter_input(&mut self, on: bool) {
        self.meter_input = on;
        self.sync_input();
    }

    /// Keep the input stream open exactly while the meter is on or a mic check listens, and on
    /// the device the preferences name.
    fn sync_input(&mut self) {
        let want = self.meter_input || self.mic.is_some();
        let device = self.settings.input_device.clone();
        if self.input.is_open() && (!want || self.input.stale(device.as_deref())) {
            self.input.stop();
        }
        if want && !self.input.is_open() && self.input.error.is_none() {
            self.input.start(device.as_deref());
        }
        if !want {
            self.input.error = None;
        }
        self.meters.input_view = self.input.is_open() && !self.player.is_playing();
    }

    /// The peaks for this frame's bars: the input's while it is open and nothing plays, else the player's.
    fn meter_peaks(&self) -> [f32; 2] {
        let out = self.player.take_levels();
        if self.input.is_open() {
            let inp = self.input.tap.take_peaks();
            if !self.player.is_playing() {
                return inp;
            }
        }
        out
    }

    /// `MICCHECK <seconds>`: listen, then answer the control channel with the numbers.
    fn start_mic_check(&mut self, arg: &str, reply: std::sync::mpsc::Sender<String>) {
        let secs: f32 = if arg.is_empty() { 5.0 } else { arg.parse().unwrap_or(f32::NAN) };
        if !(0.1..=60.0).contains(&secs) {
            let _ = reply.send("ERR MICCHECK <seconds>, 0.1 to 60".to_string());
            return;
        }
        match self.begin_mic_check(secs, Some(reply.clone())) {
            Ok(()) => {}
            Err(e) => {
                let _ = reply.send(format!("ERR {e}"));
            }
        }
    }

    /// Start listening for `secs` (the dialog's button and `MICCHECK`).
    fn begin_mic_check(&mut self, secs: f32, reply: Option<std::sync::mpsc::Sender<String>>) -> Result<(), String> {
        if self.mic.is_some() {
            return Err("a mic check is already listening".to_string());
        }
        self.mic = Some(MicCheck { started: std::time::Instant::now(), secs, reply: None });
        self.input.error = None;
        self.sync_input();
        if !self.input.is_open() {
            self.mic = None;
            return Err(self.input.error.clone().unwrap_or_else(|| "the input could not be opened".to_string()));
        }
        self.input.tap.reset_stats();
        if let Some(m) = self.mic.as_mut() {
            m.started = std::time::Instant::now();
            m.reply = reply;
        }
        let dialog = self.mic_dialog.get_or_insert(MicDialog { secs, result: None });
        dialog.secs = secs;
        Ok(())
    }

    /// Finish a mic check whose time is up.
    fn poll_mic_check(&mut self) {
        let Some(m) = &self.mic else { return };
        if m.started.elapsed().as_secs_f32() < m.secs {
            return;
        }
        let stats = self.input.tap.stats();
        let source = self.input.source.clone();
        let m = self.mic.take();
        if let Some(reply) = m.as_ref().and_then(|m| m.reply.as_ref()) {
            let _ = reply.send(format!("OK seconds={:.1} {} source={}", m.as_ref().map_or(0.0, |m| m.secs), stats.line(), source.replace(' ', "_")));
        }
        if let Some(d) = self.mic_dialog.as_mut() {
            d.result = Some((stats, source));
        }
        self.sync_input();
    }

    /// The row above the bars: what they show, and the input switch.
    fn input_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut on = self.meter_input;
            if ui.toggle_value(&mut on, "Input").on_hover_text("Meter the input device while nothing plays (nothing is recorded)").changed() {
                self.set_meter_input(on);
            }
            let text = if let Some(e) = self.input.error.clone() {
                format!("Input unavailable: {e}")
            } else if self.meters.input_view {
                format!("Monitoring {} (armed, not recording)", self.input.source)
            } else if self.input.is_open() {
                "Input armed: the bars show playback".to_string()
            } else {
                "Output".to_string()
            };
            ui.weak(text);
            if ui.small_button("Mic Check…").clicked() {
                self.mic_dialog.get_or_insert(MicDialog { secs: 5.0, result: None });
            }
        });
    }

    /// Window › Mic Check…: listen for N seconds, then say how the level was.
    fn mic_check_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.mic_dialog.take() else { return };
        let mut open = true;
        let mut start = false;
        let listening = self.mic.as_ref().map(|m| (m.started.elapsed().as_secs_f32(), m.secs));
        egui::Window::new("Mic Check").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.label("Speak or play at your normal level. Nothing is recorded.");
            ui.horizontal(|ui| {
                ui.label("Listen for");
                ui.add_enabled(listening.is_none(), egui::DragValue::new(&mut dialog.secs).range(1.0..=30.0).suffix(" s"));
                if ui.add_enabled(listening.is_none(), egui::Button::new("Start")).clicked() {
                    start = true;
                }
            });
            if let Some((t, total)) = listening {
                ui.add(egui::ProgressBar::new((t / total).clamp(0.0, 1.0)).text(format!("Listening… {:.0} s", (total - t).max(0.0).ceil())));
            } else if let Some(e) = &self.input.error {
                ui.colored_label(CURSOR, format!("Input unavailable: {e}"));
            }
            if let Some((stats, source)) = &dialog.result {
                ui.separator();
                let f = |v: f32| if v.is_finite() { format!("{v:.1} dBFS") } else { "-inf".to_string() };
                ui.label(format!("Source: {source}"));
                ui.label(format!("Peak: {}   Average: {}", f(stats.peak_db()), f(stats.average_db())));
                let v = stats.verdict();
                let colour = match v {
                    wavcraft_core::input_meter::Verdict::Ok => Color32::from_rgb(64, 200, 140),
                    _ => CURSOR,
                };
                ui.colored_label(colour, v.advice());
            }
        });
        if start {
            let secs = dialog.secs;
            self.mic_dialog = Some(dialog);
            if let Err(e) = self.begin_mic_check(secs, None) {
                self.error = Some(format!("Mic check: {e}"));
            }
            return;
        }
        if open {
            self.mic_dialog = Some(dialog);
        }
    }

    /// Put every preference back to its default: the preference files are moved aside as
    /// `.reset-N` (see `reset_preference_files`), then the app reloads from them. The one reset for
    /// Preferences > Reset All, Shift at launch, File > Restore Preference Defaults and `PREF reset`.
    fn reset_preferences(&mut self) -> Result<String, String> {
        let msg = match self.settings_path.as_deref().and_then(Path::parent) {
            Some(dir) => reset_preference_files(dir)?,
            None => "Preferences reset to their defaults".to_string(),
        };
        // The shortcut file is gone, so the keys are the shipped ones again (as at launch).
        self.keymap = self.shortcuts_path.as_deref().map(wavcraft_core::shortcuts::load).unwrap_or_default().0;
        self.error = None;
        self.reload_settings();
        self.notice = Some(msg.clone());
        Ok(msg)
    }

    /// Edit › Preferences…
    fn open_preferences(&mut self) {
        self.devices_rx = Some(crate::player::list_devices_in_background(self.settings.audio_host.clone()));
        self.prefs.get_or_insert(PrefPage::General);
        self.prefs_usage = None;
        self.prefs_confirm_cleanup = false;
    }

    /// The Preferences dialog: a page list on the left, the page on the right.
    fn preferences_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut page) = self.prefs else { return };
        if let Some(rx) = &self.devices_rx {
            match rx.try_recv() {
                Ok(d) => {
                    self.devices = d;
                    self.devices_rx = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.devices_rx = None,
            }
        }
        if matches!(page, PrefPage::AutoSave | PrefPage::MediaCache) && self.prefs_usage.is_none() {
            self.prefs_usage = Some(self.session_usage());
        }
        let mut open = true;
        let mut action: Option<PrefAction> = None;
        egui::Window::new("Preferences").open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(130.0);
                    for p in PrefPage::ALL {
                        ui.selectable_value(&mut page, p, p.title());
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.heading(page.title());
                    ui.add_space(6.0);
                    match page {
                        PrefPage::General => {
                            ui.label("Session folder: where an opened file is kept while you work on it.");
                            ui.monospace(self.sessions.path.display().to_string());
                            ui.label(format!("({})", self.sessions.source.describe()));
                            ui.horizontal(|ui| {
                                if ui.button("Choose…").clicked() {
                                    action = Some(PrefAction::ChooseSessionFolder);
                                }
                                if ui.button("Use the default").clicked() {
                                    action = Some(PrefAction::Set("session_folder", String::new()));
                                }
                            });
                            ui.add_space(12.0);
                            pref_number(ui, "Zoom In / Out factor", "zoom_factor", self.settings.zoom_factor, settings::MIN_ZOOM_FACTOR..=settings::MAX_ZOOM_FACTOR, "x", &mut action);
                            pref_choice(ui, "Extend selection from", "extend_selection", self.settings.extend_selection.name(), settings::ExtendSelection::ALL.iter().map(|c| c.name()), &mut action);
                            ui.label("Stored only for now: Shift-click extension is not on this branch.");
                            pref_check(ui, "Context-sensitive channel editing", "context_channel_editing", self.settings.context_channel_editing, &mut action);
                            ui.label("Stored only for now: per-channel pointer editing is not on this branch.");
                            ui.add_space(12.0);
                            if ui.button("Reset all preferences").on_hover_text("The settings file is kept aside, not deleted").clicked() {
                                action = Some(PrefAction::ResetAll);
                            }
                        }
                        PrefPage::AutoSave => {
                            ui.label("Auto Save writes a recovery journal into the session folder while the document has unsaved work.");
                            ui.label("After a crash, the next start offers to recover that work.");
                            pref_check(ui, "Save recovery data while I work", "autosave_enabled", self.settings.autosave_enabled, &mut action);
                            ui.add_enabled_ui(self.settings.autosave_enabled, |ui| {
                                let mut secs = self.settings.autosave_interval_secs;
                                ui.horizontal(|ui| {
                                    ui.label("Write it at most every");
                                    if ui.add(egui::DragValue::new(&mut secs).range(settings::INTERVAL_RANGE).suffix(" s")).changed() {
                                        action = Some(PrefAction::Set("autosave_interval_secs", secs.to_string()));
                                    }
                                });
                            });
                            let mut days = self.settings.autosave_keep_days;
                            ui.horizontal(|ui| {
                                ui.label("Keep old sessions for");
                                if ui.add(egui::DragValue::new(&mut days).range(settings::KEEP_DAYS_RANGE).suffix(" days")).changed() {
                                    action = Some(PrefAction::Set("autosave_keep_days", days.to_string()));
                                }
                            });
                            ui.label("\"Clean up old sessions\" on the Media and Disk Cache page removes only sessions this old.");
                        }
                        PrefPage::MediaCache => {
                            ui.label("Session folder: where an opened file's session store is kept while you work on it.");
                            ui.monospace(self.sessions.path.display().to_string());
                            ui.label(format!("({})", self.sessions.source.describe()));
                            ui.horizontal(|ui| {
                                if ui.button("Choose…").clicked() {
                                    action = Some(PrefAction::ChooseSessionFolder);
                                }
                                if ui.button("Use the default").clicked() {
                                    action = Some(PrefAction::Set("session_folder", String::new()));
                                }
                            });
                            ui.add_space(8.0);
                            let mut custom = self.settings.cache_mb.is_some();
                            let current = self.settings.cache_mb.unwrap_or(self.settings.cache_bytes() >> 20);
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut custom, "Block cache (RAM)").changed() {
                                    let value = if custom { current.to_string() } else { "default".to_string() };
                                    action = Some(PrefAction::Set("cache_mb", value));
                                }
                                if custom {
                                    let mut mb = current;
                                    if ui.add(egui::DragValue::new(&mut mb).range(settings::CACHE_MB_RANGE).suffix(" MB")).changed() {
                                        action = Some(PrefAction::Set("cache_mb", mb.to_string()));
                                    }
                                } else {
                                    ui.label(format!("{current} MB (default)"));
                                }
                            });
                            ui.label("A new cache size applies from the next file opened.");
                            if let Some(u) = &self.prefs_usage {
                                ui.add_space(8.0);
                                ui.label(format!(
                                    "Sessions on disk: {} using {}. {} can be recovered (never removed here); {} old enough to clean up ({}).",
                                    u.sessions,
                                    wavcraft_core::space::format_bytes(u.bytes),
                                    u.recoverable,
                                    u.removable,
                                    wavcraft_core::space::format_bytes(u.removable_bytes)
                                ));
                                ui.horizontal(|ui| {
                                    if self.prefs_confirm_cleanup {
                                        ui.label(format!("Delete {} session(s) of closed runs, idle {}+ days?", u.removable, self.settings.autosave_keep_days));
                                        if ui.button("Delete").clicked() {
                                            action = Some(PrefAction::CleanUp(self.settings.autosave_keep_days));
                                        }
                                        if ui.button("Cancel").clicked() {
                                            action = Some(PrefAction::CancelCleanup);
                                        }
                                    } else {
                                        if ui.add_enabled(u.removable > 0, egui::Button::new("Clean up old sessions…")).clicked() {
                                            action = Some(PrefAction::AskCleanup);
                                        }
                                        if ui.button("Refresh").clicked() {
                                            action = Some(PrefAction::RefreshUsage);
                                        }
                                    }
                                });
                            }
                        }
                        PrefPage::Data => {
                            ui.label("Edit crossfades smooth the joins an edit makes (0 turns one off).");
                            for (label, key, value, default) in [
                                ("Delete / Cut join (ms)", "crossfade_cut_join_ms", self.settings.cut_join_ms, settings::DEFAULT_CUT_JOIN_MS),
                                ("Every other edit boundary (ms)", "crossfade_edge_ms", self.settings.edge_ms, settings::DEFAULT_EDGE_MS),
                            ] {
                                ui.horizontal(|ui| {
                                    let mut v = value;
                                    if ui.add(egui::DragValue::new(&mut v).range(0..=settings::MAX_CROSSFADE_MS).suffix(" ms")).changed() {
                                        action = Some(PrefAction::Set(key, v.to_string()));
                                    }
                                    ui.label(label);
                                    if value != default && ui.small_button("Default").clicked() {
                                        action = Some(PrefAction::Set(key, default.to_string()));
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Sample rate conversion quality");
                                egui::ComboBox::from_id_salt("src_quality").selected_text(self.settings.src_quality.name()).show_ui(ui, |ui| {
                                    for q in settings::SrcQuality::ALL {
                                        if ui.selectable_label(self.settings.src_quality == q, q.name()).clicked() {
                                            action = Some(PrefAction::Set("src_quality", q.name().to_string()));
                                        }
                                    }
                                });
                            });
                            ui.label("Stored only for now: nothing on this branch converts sample rates yet.");
                            ui.add_space(8.0);
                            ui.label("Float headroom: files are held as 32-bit float, so a level over 0 dBFS inside the editor is kept, not clipped; it is only limited when written to a fixed-point file.");
                        }
                        PrefPage::MarkersMetadata => {
                            ui.label("Saved files: what goes in by default (the save options dialog can override it).");
                            ui.label("Stored only for now: the save options that read these are not on this branch.");
                            for (label, key, on) in [
                                ("Include markers when saving", "save_markers", self.settings.save_markers),
                                ("Include metadata when saving", "save_metadata", self.settings.save_metadata),
                            ] {
                                let mut v = on;
                                if ui.checkbox(&mut v, label).changed() {
                                    action = Some(PrefAction::Set(key, v.to_string()));
                                }
                            }
                            ui.add_space(6.0);
                            ui.label("Marker name pattern ({n} is the marker's number):");
                            let id = ui.id().with("marker_pattern");
                            let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| self.settings.marker_name_pattern.clone());
                            let r = ui.text_edit_singleline(&mut text);
                            if r.lost_focus() {
                                action = Some(PrefAction::Set("marker_name_pattern", text.clone()));
                                ui.data_mut(|d| d.remove::<String>(id));
                            } else {
                                ui.data_mut(|d| d.insert_temp(id, text));
                            }
                            ui.label(format!("Next marker: {}", self.settings.marker_name(1)));
                            if self.settings.marker_name_pattern != settings::DEFAULT_MARKER_PATTERN && ui.small_button("Default").clicked() {
                                action = Some(PrefAction::Set("marker_name_pattern", settings::DEFAULT_MARKER_PATTERN.to_string()));
                                ui.data_mut(|d| d.remove::<String>(id));
                            }
                        }
                        PrefPage::PlaybackRecording => {
                            ui.label("Stored only for now: the transport, shuttle and recording features that read these are not on this branch.");
                            ui.horizontal(|ui| {
                                ui.label("Auto-scroll while playing");
                                egui::ComboBox::from_id_salt("autoscroll").selected_text(self.settings.autoscroll.name()).show_ui(ui, |ui| {
                                    for a in settings::AutoScroll::ALL {
                                        if ui.selectable_label(self.settings.autoscroll == a, a.name()).clicked() {
                                            action = Some(PrefAction::Set("autoscroll", a.name().to_string()));
                                        }
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label("J/K/L speed steps");
                                let id = ui.id().with("jkl_steps");
                                let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| self.settings.get("jkl_speed_steps").unwrap_or_default());
                                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
                                if r.lost_focus() {
                                    action = Some(PrefAction::Set("jkl_speed_steps", text.clone()));
                                    ui.data_mut(|d| d.remove::<String>(id));
                                } else {
                                    ui.data_mut(|d| d.insert_temp(id, text));
                                }
                                ui.label("(slowest first, e.g. 0.5,1,2,4)");
                            });
                            for (label, key, value) in [
                                ("Pre-roll (s)", "preroll_s", self.settings.preroll().as_secs_f32()),
                                ("Post-roll (s)", "postroll_s", self.settings.postroll().as_secs_f32()),
                            ] {
                                let mut v = value;
                                ui.horizontal(|ui| {
                                    if ui.add(egui::DragValue::new(&mut v).range(0.0..=settings::MAX_ROLL_S).speed(0.1).suffix(" s")).changed() {
                                        action = Some(PrefAction::Set(key, v.to_string()));
                                    }
                                    ui.label(label);
                                });
                            }
                            let mut off = self.settings.record_offset_ms;
                            ui.horizontal(|ui| {
                                let max = settings::MAX_RECORD_OFFSET_MS;
                                if ui.add(egui::DragValue::new(&mut off).range(-max..=max).suffix(" ms")).changed() {
                                    action = Some(PrefAction::Set("record_offset_ms", off.to_string()));
                                }
                                ui.label("Record offset (placeholder until recording lands)");
                            });
                        }
                        PrefPage::Appearance => {
                            pref_choice(ui, "Theme", "theme", self.settings.theme.name(), settings::Theme::ALL.iter().map(|c| c.name()), &mut action);
                            pref_choice(ui, "Waveform colour", "waveform_colour", self.settings.waveform_colour.name(), settings::WaveColour::ALL.iter().map(|c| c.name()), &mut action);
                            pref_number(ui, "UI brightness", "ui_brightness", self.settings.ui_brightness, settings::MIN_BRIGHTNESS..=settings::MAX_BRIGHTNESS, "x", &mut action);
                            pref_number(ui, "UI contrast", "ui_contrast", self.settings.ui_contrast, settings::MIN_CONTRAST..=settings::MAX_CONTRAST, "x", &mut action);
                            pref_choice(ui, "Spectrogram palette", "spec_palette", self.settings.spec_palette.name(), settings::SpecPalette::ALL.iter().map(|c| c.name()), &mut action);
                            ui.separator();
                            ui.label("Colours (a pick overrides the theme's own):");
                            for (item, key) in appearance::ITEMS.iter().zip(COLOR_KEYS) {
                                ui.horizontal(|ui| {
                                    let now = self.settings.colors.get(item).flatten();
                                    let mut rgb = now.unwrap_or_else(|| default_rgb(item, &self.settings, self.dark));
                                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                                        action = Some(PrefAction::Set(key, appearance::format_hex(rgb)));
                                    }
                                    ui.label(format!("{item} ({})", appearance::colour_text(now)));
                                    if now.is_some() && ui.small_button("Default").clicked() {
                                        action = Some(PrefAction::Set(key, "default".to_string()));
                                    }
                                });
                            }
                            ui.separator();
                            ui.label("Presets:");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("preset name").desired_width(140.0));
                                if ui.button("Save").clicked() {
                                    action = Some(PrefAction::Preset("save", self.preset_name.clone()));
                                }
                            });
                            for name in self.settings.preset_names() {
                                ui.horizontal(|ui| {
                                    ui.label(&name);
                                    if ui.small_button("Load").clicked() {
                                        action = Some(PrefAction::Preset("load", name.clone()));
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        action = Some(PrefAction::Preset("delete", name.clone()));
                                    }
                                });
                            }
                            ui.label("Everything on this page applies at once.");
                        }
                        PrefPage::SpectralDisplays => {
                            let res: Vec<String> = settings::SPEC_RESOLUTIONS.iter().map(u32::to_string).collect();
                            pref_choice(ui, "Resolution (FFT length)", "spec_resolution", &self.settings.spec_resolution.to_string(), res.iter().map(String::as_str), &mut action);
                            pref_number(ui, "dB range", "spec_db_range", self.settings.spec_db_range as f32, settings::MIN_SPEC_DB_RANGE as f32..=settings::MAX_SPEC_DB_RANGE as f32, "dB", &mut action);
                            ui.label("Resolution and dB range apply to the spectral display at once.");
                            pref_choice(ui, "Window function", "spec_window", self.settings.spec_window.name(), settings::SpecWindow::ALL.iter().map(|c| c.name()), &mut action);
                            ui.label("Stored only for now: the display always analyses with a Hann window.");
                            pref_check(ui, "Play only selected frequencies (placeholder)", "spec_play_selected_freqs", self.settings.spec_play_selected_freqs, &mut action);
                            ui.label("Stored only: there is no frequency selection to play yet.");
                        }
                        PrefPage::TimeDisplay => {
                            pref_choice(ui, "Default time format", "time_display", self.settings.time_display.name(), TimeFormat::ALL.iter().map(|c| c.name()), &mut action);
                            ui.label("The ruler, the status bar and the panels use it at once; View › Time Display sets the same.");
                        }
                        PrefPage::Loudness => {
                            pref_number(ui, "Default loudness target", "loudness_target_lufs", self.settings.loudness_target_lufs, settings::MIN_LOUDNESS_TARGET_LUFS..=settings::MAX_LOUDNESS_TARGET_LUFS, "LUFS", &mut action);
                            pref_number(ui, "Default true-peak ceiling", "true_peak_ceiling_dbtp", self.settings.true_peak_ceiling_dbtp, settings::MIN_TRUE_PEAK_DBTP..=settings::MAX_TRUE_PEAK_DBTP, "dBTP", &mut action);
                            ui.label("Stored only for now: loudness features read these when they are merged.");
                        }
                        PrefPage::AudioHardware => {
                            let hosts = crate::player::host_names();
                            let host_now = self.settings.audio_host.clone();
                            ui.horizontal(|ui| {
                                ui.label("Audio host");
                                let shown = if host_now.is_none() { format!("{} (default)", self.player.host) } else { self.player.host.clone() };
                                egui::ComboBox::from_id_salt("audio_host").width(300.0).selected_text(shown).show_ui(ui, |ui| {
                                    if ui.selectable_label(host_now.is_none(), settings::SYSTEM_DEFAULT).clicked() {
                                        action = Some(PrefAction::Set("audio_host", String::new()));
                                    }
                                    for name in &hosts {
                                        if ui.selectable_label(host_now.as_ref() == Some(name), name).clicked() {
                                            action = Some(PrefAction::Set("audio_host", name.clone()));
                                        }
                                    }
                                });
                                if hosts.len() < 2 {
                                    ui.weak("(the only one this system offers)");
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Buffer size");
                                let shown = self.settings.buffer_frames.map_or(settings::DEVICE_DEFAULT.to_string(), |n| format!("{n} frames"));
                                egui::ComboBox::from_id_salt("buffer_frames").width(300.0).selected_text(shown).show_ui(ui, |ui| {
                                    if ui.selectable_label(self.settings.buffer_frames.is_none(), settings::DEVICE_DEFAULT).clicked() {
                                        action = Some(PrefAction::Set("buffer_frames", String::new()));
                                    }
                                    for n in self.player.buffer_range.map(wavcraft_core::chanmap::buffer_choices).unwrap_or_default() {
                                        if ui.selectable_label(self.settings.buffer_frames == Some(n), format!("{n} frames")).clicked() {
                                            action = Some(PrefAction::Set("buffer_frames", n.to_string()));
                                        }
                                    }
                                });
                            });
                            ui.weak("The buffer size is applied when the output reopens (now, and when you change the device).");
                            for (label, key, list, now) in [
                                ("Output", "output_device", &self.devices.output, &self.settings.output_device),
                                ("Input (for recording)", "input_device", &self.devices.input, &self.settings.input_device),
                            ] {
                                let shown = now.clone().unwrap_or_else(|| settings::SYSTEM_DEFAULT.to_string());
                                let gone = now.as_ref().is_some_and(|n| !list.contains(n));
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(key)
                                        .width(300.0)
                                        .selected_text(if gone { format!("{shown} (not connected)") } else { shown.clone() })
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_label(now.is_none(), settings::SYSTEM_DEFAULT).clicked() {
                                                action = Some(PrefAction::Set(key, String::new()));
                                            }
                                            for name in list.iter() {
                                                if ui.selectable_label(now.as_ref() == Some(name), name).clicked() {
                                                    action = Some(PrefAction::Set(key, name.clone()));
                                                }
                                            }
                                        });
                                });
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Refresh the device lists").clicked() {
                                    action = Some(PrefAction::RefreshDevices);
                                }
                                if self.devices_rx.is_some() {
                                    ui.spinner();
                                    ui.label("Looking for devices…");
                                }
                            });
                            ui.add_space(6.0);
                            match &self.player.error {
                                Some(e) => ui.colored_label(CURSOR, format!("No sound: {e}")),
                                None => ui.label(format!(
                                    "Playing on: {} ({}, {} channels, buffer {})",
                                    self.player.device,
                                    self.player.host,
                                    self.player.out_channels,
                                    self.player.buffer.map_or("device default".to_string(), |n| format!("{n} frames"))
                                )),
                            };
                        }
                        PrefPage::ChannelMapping => {
                            let doc_channels = self.doc.as_ref().map_or(2, |d| d.state.channel_count()).max(1);
                            let out_channels = self.player.out_channels.max(2);
                            ui.label("Output: which device output plays each channel of the open file.");
                            if self.settings.output_map.is_default() {
                                ui.weak("Automatic (a mono file on both speakers, stereo on outputs 1 and 2). Choose any output to set a map.");
                            }
                            for c in 0..doc_channels {
                                let now = self.settings.output_map.get(c);
                                ui.horizontal(|ui| {
                                    ui.label(format!("File channel {}", c + 1));
                                    let shown = match (now, self.settings.output_map.is_default()) {
                                        (Some(t), _) => format!("Output {}", t + 1),
                                        (None, true) => "Automatic".to_string(),
                                        (None, false) => "Not played".to_string(),
                                    };
                                    egui::ComboBox::from_id_salt(("out_map", c)).selected_text(shown).show_ui(ui, |ui| {
                                        if ui.selectable_label(now.is_none(), "Not played").clicked() {
                                            action =
                                                Some(PrefAction::Set("output_map", map_text_with(&self.settings.output_map, c, None, doc_channels)));
                                        }
                                        for t in 0..out_channels {
                                            if ui.selectable_label(now == Some(t), format!("Output {}", t + 1)).clicked() {
                                                action = Some(PrefAction::Set(
                                                    "output_map",
                                                    map_text_with(&self.settings.output_map, c, Some(t), doc_channels),
                                                ));
                                            }
                                        }
                                    });
                                });
                            }
                            ui.add_space(10.0);
                            ui.label("Input: which record channel each device input is recorded to (used by recording).");
                            for i in 0..2usize {
                                let now = self.settings.input_map.get(i);
                                ui.horizontal(|ui| {
                                    ui.label(format!("Device input {}", i + 1));
                                    let shown = now.map_or("Automatic".to_string(), |t| format!("Channel {}", t + 1));
                                    egui::ComboBox::from_id_salt(("in_map", i)).selected_text(shown).show_ui(ui, |ui| {
                                        if ui.selectable_label(now.is_none(), "Automatic").clicked() {
                                            action = Some(PrefAction::Set("input_map", map_text_with(&self.settings.input_map, i, None, 0)));
                                        }
                                        for t in 0..2usize {
                                            if ui.selectable_label(now == Some(t), format!("Channel {}", t + 1)).clicked() {
                                                action = Some(PrefAction::Set("input_map", map_text_with(&self.settings.input_map, i, Some(t), 0)));
                                            }
                                        }
                                    });
                                });
                            }
                            ui.add_space(6.0);
                            if ui.button("Back to automatic").clicked() {
                                action = Some(PrefAction::ClearMaps);
                            }
                        }
                    }
                });
            });
        });
        self.prefs = open.then_some(page);
        if !open {
            self.prefs_usage = None;
            self.prefs_confirm_cleanup = false;
        }
        let result = match action {
            Some(PrefAction::Set(key, value)) => {
                let r = self.pref_set(key, &value);
                self.prefs_usage = None;
                r
            }
            Some(PrefAction::ChooseSessionFolder) => rfd::FileDialog::new()
                .set_directory(&self.sessions.path)
                .pick_folder()
                .map_or(Ok(()), |p| self.pref_set("session_folder", &p.to_string_lossy())),
            Some(PrefAction::Preset(verb, name)) => self.preset_do(verb, &name),
            Some(PrefAction::RefreshDevices) => {
                self.devices_rx = Some(crate::player::list_devices_in_background(self.settings.audio_host.clone()));
                Ok(())
            }
            Some(PrefAction::ClearMaps) => self.pref_set("output_map", "").and_then(|()| self.pref_set("input_map", "")),
            Some(PrefAction::ResetAll) => self.reset_preferences().map(|_| ()),
            Some(PrefAction::AskCleanup) => {
                self.prefs_confirm_cleanup = true;
                Ok(())
            }
            Some(PrefAction::CancelCleanup) => {
                self.prefs_confirm_cleanup = false;
                Ok(())
            }
            Some(PrefAction::CleanUp(days)) => {
                self.prefs_confirm_cleanup = false;
                self.cleanup_sessions(days);
                Ok(())
            }
            Some(PrefAction::RefreshUsage) => {
                self.prefs_usage = None;
                Ok(())
            }
            None => Ok(()),
        };
        if let Err(e) = result {
            self.error = Some(e);
        }
    }

    /// Stream `path` into a session store on a worker thread; the window stays responsive for
    /// long files. A file still opening is cancelled first.
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.rec.is_active() {
            self.error = Some("Stop recording before opening a file".to_string());
            return;
        }
        if self.integrating.is_some() {
            self.error = Some("Wait for the recording to be brought into the file".to_string());
            return;
        }
        self.open_as(path, None, ctx);
    }

    /// [`open`](Self::open), or with `raw` the headerless import (File › Import › Raw Data).
    fn open_as(&mut self, path: PathBuf, raw: Option<RawSpec>, ctx: &egui::Context) {
        if let Some(old) = self.opening.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(r) = self.running.take() {
            r.job.cancel();
        }
        // A file being inserted belongs to the document that is being replaced.
        if let Some(old) = self.inserting.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (ctx, job_path, job_cancel, job_progress) = (ctx.clone(), path.clone(), cancel.clone(), progress.clone());
        let store = wavcraft_core::store::StoreOptions { cache_bytes: self.settings.cache_bytes(), ..Default::default() };
        let opts =
            OpenOptions { sessions_root: Some(self.sessions.path.clone()), store, peak_cache: Some(self.peak_dir.clone()), ..OpenOptions::default() };
        std::thread::spawn(move || {
            let mut last = 0;
            let mut report = |p: f64| {
                let permille = (p.clamp(0.0, 1.0) * 1000.0) as u32;
                job_progress.store(permille, Ordering::Relaxed);
                if permille >= last + 10 {
                    last = permille;
                    ctx.request_repaint();
                }
            };
            let result = match &raw {
                Some(spec) => Document::open_raw_in(&job_path, spec, &opts, &mut report, &job_cancel),
                None => Document::open_in(&job_path, &opts, &mut report, &job_cancel),
            }
            .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.opening = Some(Opening { path, result: rx, cancel, progress, fresh: false, started: Instant::now() });
        self.error = None;
        self.notice = None;
    }

    /// Multitrack > Mixdown (f111): render the session, or the picked clips, offline through the
    /// mix engine on a worker thread; the result opens like a file does (as a new, unsaved waveform).
    fn start_mixdown(&mut self, selected_only: bool, normalize_db: Option<f32>, ctx: &egui::Context) -> Result<(), String> {
        let (session, opts) = self.mtv.mixdown_input(selected_only, normalize_db).map_err(|e| e.to_string())?;
        if let Some(old) = self.opening.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (ctx, job_cancel, job_progress) = (ctx.clone(), cancel.clone(), progress.clone());
        let root = self.sessions.path.clone();
        std::thread::spawn(move || {
            let mut report = |p: f64| job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
            let result = wavcraft_core::mixdown::mixdown(&session, &opts, Some(root.as_path()), &mut report, &job_cancel)
                .map_err(|e| e.to_string())
                .and_then(|state| Document::from_state(PathBuf::new(), state).map_err(|e| e.to_string()));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.opening = Some(Opening { path: PathBuf::from("Mixdown"), result: rx, cancel, progress, fresh: true, started: Instant::now() });
        self.error = None;
        self.notice = None;
        Ok(())
    }

    fn finish_opening(&mut self) {
        let Some(Opening { path, result: rx, fresh, started, .. }) = &self.opening else { return };
        let fresh = *fresh;
        let took = started.elapsed();
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            // The worker ended without an answer: say so instead of showing "Opening…" forever.
            Err(TryRecvError::Disconnected) => Err("the decoder stopped unexpectedly".to_string()),
        };
        match result {
            Ok(doc) => {
                self.last_op = Some(("Open".to_string(), took));
                // The document that was open stays open, parked; this one is added at the end.
                let path = path.clone();
                // The clipboards outlive the document they were copied from.
                let clips = self.editor.as_mut().map(|e| std::mem::take(&mut e.clipboards));
                let old = self.editor.is_some().then(|| self.take_slot());
                self.docs.add(old);
                let r = &doc.peak_report;
                log::info!(target: "wavcraft", "peaks for {}: {} in {:.1} ms", path.display(), r.result, r.ms);
                if let Some(e) = &r.write_error {
                    log::warn!(target: "wavcraft", "the peak cache could not be written: {e}");
                }
                self.peak_report = Some(r.clone());
                self.set_audio(Some(doc.state.clone()));
                self.meters.new_file();
                self.matcher.new_file();
                let cap = History::default_cap(&self.sessions.path);
                self.editor = Some(Editor::new(History::new(doc.state.clone(), cap)));
                if let (Some(clips), Some(e)) = (clips, self.editor.as_mut()) {
                    e.clipboards = clips;
                }
                self.apply_crossfades();
                self.apply_snap();
                self.saved = if fresh { None } else { self.editor.as_ref().map(|e| e.history.current().clone()) };
                self.doc_path = doc.path.clone();
                self.note_recent(&doc.path);
                self.imports.retain(|i| !same_file(&i.path, &doc.path));
                self.tempo = self.settings.tempo_for(&self.doc_path);
                self.doc_serial += 1;
                self.doc_id = self.doc_serial;
                self.include_meta = self.settings.save_include_metadata;
                self.stats.forget();
                self.playlist_ui.reset();
                self.rebuilding = None;
                self.doc = Some(doc);
                self.drop_video();
                self.generation += 1;
                self.selection = Selection::default();
                self.freq.reset();
                self.spectrum = None;
                self.fit_on_next_frame = true;
            }
            Err(e) => self.open_failures.push(format!("{}: {e}", path.display())),
        }
        self.opening = None;
    }

    /// Insert › Audio from File…: pick a file, then [`insert_file`](Self::insert_file).
    fn pick_insert_file(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        let picked =
            rfd::FileDialog::new().add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"]).pick_file();
        match picked {
            Some(path) => self.insert_file(path, ctx),
            None => Ok(()),
        }
    }

    /// Stream `path` into the open document's own session store on a worker thread (never the
    /// whole file in memory), then insert it at the cursor or over the selection as one step.
    fn insert_file(&mut self, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        let why = if self.saving.is_some() {
            Some("wait for the save to finish")
        } else if self.opening.is_some() {
            Some("wait for the file to finish opening")
        } else if self.inserting.is_some() {
            Some("another file is being inserted")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(why.to_string());
        }
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let opts = OpenOptions { into_store: Some(editor.history.current().store().clone()), ..OpenOptions::default() };
        let (tx, rx) = channel();
        let (cancel, progress) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (ctx, job_path, job_cancel, job_progress) = (ctx.clone(), path.clone(), cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            let mut last = 0;
            let mut report = |p: f64| {
                let permille = (p.clamp(0.0, 1.0) * 1000.0) as u32;
                job_progress.store(permille, Ordering::Relaxed);
                if permille >= last + 10 {
                    last = permille;
                    ctx.request_repaint();
                }
            };
            let result = wavcraft_core::io::open(&job_path, &opts, &mut report, &job_cancel).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.inserting = Some(Inserting { path, result: rx, cancel, progress, doc_id: self.doc_id });
        self.error = None;
        self.notice = None;
        Ok(())
    }

    fn finish_inserting(&mut self, ctx: &egui::Context) {
        let Some(Inserting { path, result: rx, doc_id, .. }) = &self.inserting else { return };
        let (path, doc_id) = (path.clone(), *doc_id);
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the decoder stopped unexpectedly".to_string()),
        };
        self.inserting = None;
        if doc_id != self.doc_id {
            return;
        }
        let sel = self.selection_range();
        let outcome = match (result, self.editor.as_mut()) {
            (Ok(file), Some(editor)) => editor.insert_file(&file, sel),
            (Err(e), _) => Err(e),
            (_, None) => Err("no file is open".to_string()),
        };
        match outcome {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.notice = out.message;
                self.error = None;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
            }
            Err(e) => {
                let why = format!("Insert Audio from File: {}: {e}", path.display());
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why);
            }
        }
    }

    /// Open the files chosen together one at a time (one decoder and one store being written at
    /// once, for the 8 GB budget). A failure on one is kept and does not stop the others; they
    /// are all reported when the last is done.
    fn open_many(&mut self, paths: Vec<PathBuf>) {
        self.open_queue.extend(paths);
    }

    /// Start the next queued file when none is opening; report the failures when none is left.
    fn pump_open_queue(&mut self, ctx: &egui::Context) {
        if self.opening.is_some() {
            return;
        }
        if let Some(path) = self.open_queue.pop_front() {
            self.open(path, ctx);
        } else if !self.open_failures.is_empty() {
            let failed = std::mem::take(&mut self.open_failures);
            log::warn!(target: "wavcraft", "could not open: {}", failed.join("; "));
            self.error =
                Some(if failed.len() == 1 { failed.join("") } else { format!("{} files could not be opened: {}", failed.len(), failed.join("; ")) });
            self.notice = None;
        }
    }

    /// File › Open Append: add `paths` to the end of the current document (`to_new` false) as one
    /// undo step, or make a new document of them back to back, on a worker thread.
    fn start_append(&mut self, to_new: bool, paths: Vec<PathBuf>, ctx: &egui::Context) -> Result<(), String> {
        if paths.is_empty() {
            return Err("Open Append: no file was chosen".to_string());
        }
        if self.appending.is_some() || self.making.is_some() || self.opening.is_some() || !self.open_queue.is_empty() {
            return Err("wait for the file being opened or added to finish".to_string());
        }
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let to = if to_new {
            AppendTo::New
        } else {
            let Some(editor) = self.editor.as_ref() else { return Err("Open Append › To Current: no file is open".to_string()) };
            AppendTo::Current { doc_id: self.doc_id, base: editor.history.current().clone() }
        };
        let (tx, rx) = channel();
        let (progress, cancel) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)));
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        let (job_progress, job_cancel, ctx, n) = (progress.clone(), cancel.clone(), ctx.clone(), paths.len());
        let base = match &to {
            AppendTo::Current { base, .. } => Some(base.clone()),
            AppendTo::New => None,
        };
        std::thread::spawn(move || {
            let mut report = |p: f64| job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
            let out = match base {
                Some(base) => Appended::Current(append::append_files(&base, &paths, opts.free_space, &mut report, &job_cancel)),
                None => Appended::New(append::new_from_files(&opts, &paths, &mut report, &job_cancel)),
            };
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        self.appending = Some(Appending { to, paths: n, result: rx, progress, cancel });
        self.error = None;
        self.notice = None;
        Ok(())
    }

    fn finish_appending(&mut self, ctx: &egui::Context) {
        let Some(a) = &self.appending else { return };
        let result = match a.result.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        let Some(Appending { to, paths, .. }) = self.appending.take() else { return };
        let Some(result) = result else {
            self.error = Some("Open Append: the worker stopped unexpectedly".to_string());
            return;
        };
        let refused = |list: &[(PathBuf, String)]| list.iter().map(|(p, why)| format!("{}: {why}", path_name(p))).collect::<Vec<_>>().join("; ");
        let mut problems = String::new();
        let mut done = None;
        match (to, result) {
            (AppendTo::Current { doc_id, base }, Appended::Current(r)) => {
                if !r.refused.is_empty() {
                    problems = format!("not added: {}", refused(&r.refused));
                }
                if r.cancelled {
                    problems = "Open Append was cancelled".to_string();
                } else if let Some(edit) = r.edit {
                    let found = (0..self.docs.len(self.editor.is_some())).find(|&i| self.doc_id_at(i).is_some_and(|(id, _)| id == doc_id));
                    let moved = match found {
                        None => Err("the document was closed meanwhile".to_string()),
                        Some(i) => self.switch_doc(i),
                    };
                    let still = self.editor.as_ref().is_some_and(|e| Arc::ptr_eq(e.history.current(), &base));
                    match moved {
                        Err(why) => problems = format!("{problems} Nothing was added: {why}.").trim().to_string(),
                        Ok(()) if !still => {
                            problems =
                                format!("{problems} Nothing was added: the document changed while the files were being added.").trim().to_string();
                        }
                        Ok(()) => {
                            let frames: u64 = r.added.iter().map(|(_, f)| f).sum();
                            if let Some(e) = self.editor.as_mut() {
                                e.history.push_edit(edit);
                            }
                            done = Some(format!("Appended {} file(s), {frames} frames", r.added.len()));
                            self.rebuild(ctx);
                        }
                    }
                }
            }
            (AppendTo::New, Appended::New(r)) => {
                if !r.refused.is_empty() {
                    problems = format!("not added: {}", refused(&r.refused));
                }
                if r.cancelled {
                    problems = "Open Append was cancelled".to_string();
                } else if let Some(state) = r.state {
                    let frames = state.len_frames();
                    self.add_document(state, ctx);
                    done = Some(format!("New file from {} of {paths} file(s), {frames} frames", r.added.len()));
                } else if problems.is_empty() {
                    problems = "nothing could be added".to_string();
                }
            }
            _ => problems = "the worker answered the wrong question".to_string(),
        }
        if !problems.is_empty() {
            let why = format!("Open Append: {problems}");
            log::warn!(target: "wavcraft", "{why}");
            self.error = Some(why);
        }
        self.notice = done;
    }

    /// Park the current document's fields (the window's are left empty).
    fn take_slot(&mut self) -> DocSlot {
        DocSlot {
            doc: self.doc.take(),
            editor: self.editor.take(),
            doc_path: std::mem::take(&mut self.doc_path),
            doc_id: self.doc_id,
            saved: self.saved.take(),
            view: self.view,
            selection: self.selection,
            rebuilding: self.rebuilding.take(),
            tempo: std::mem::take(&mut self.tempo),
            include_meta: self.include_meta,
        }
    }

    /// What the current document shows (for carrying it over to another).
    fn doc_view(&self) -> Option<DocView> {
        let doc = self.editor.as_ref()?.history.current();
        Some(DocView { view: self.view, selection: self.selection, sample_rate: doc.sample_rate(), frames: doc.len_frames() as usize })
    }

    /// After a switch: the new current document takes over what the View options ask for
    /// (selection, zoom and scroll, cursor) from the one just left, by time.
    fn carry_view_over(&mut self, from: Option<DocView>) {
        let sync = self.settings.sync;
        let (Some(from), Some(to)) = (from, self.doc_view()) else { return };
        if sync.any() {
            (self.view, self.selection) = wavcraft_core::carry_over(&from, &to, sync, self.editor_width.max(1.0));
        }
    }

    /// The sync options that are on, for `STATE` (`selection,zoom,playhead` or `-`).
    fn sync_text(&self) -> String {
        let s = self.settings.sync;
        let on: Vec<&str> =
            [(s.selection, "selection"), (s.zoom, "zoom"), (s.playhead, "playhead")].iter().filter(|(b, _)| *b).map(|(_, n)| *n).collect();
        if on.is_empty() { "-".to_string() } else { on.join(",") }
    }

    /// Toggle a View › Synchronise option and keep it in the settings file.
    fn toggle_sync(&mut self, id: ActionId) {
        let sync = &mut self.settings.sync;
        match id {
            ActionId::ViewSyncSelection => sync.selection = !sync.selection,
            ActionId::ViewSyncZoom => sync.zoom = !sync.zoom,
            _ => sync.playhead = !sync.playhead,
        }
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the setting to {}: {e}", file.display());
        }
    }

    /// Make a parked document the current one.
    fn put_slot(&mut self, s: DocSlot) {
        self.halt_play();
        (self.doc, self.editor, self.doc_path, self.doc_id) = (s.doc, s.editor, s.doc_path, s.doc_id);
        (self.saved, self.view, self.selection, self.rebuilding) = (s.saved, s.view, s.selection, s.rebuilding);
        (self.tempo, self.include_meta) = (s.tempo, s.include_meta);
        self.set_audio(self.doc.as_ref().map(|d| d.state.clone()));
        // The per-file panels and pictures of the document left are not carried to this one.
        self.stats.forget();
        self.drop_video();
        self.effect_dialog = None;
        self.previewing = false;
        self.freq.reset();
        self.playlist_ui.reset();
        self.meters.new_file();
        self.matcher.new_file();
        self.generation += 1;
        self.spectrum = None;
    }

    /// Switch to open document `i` (0 is the first opened).
    fn switch_doc(&mut self, i: usize) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.editor.is_none() || i >= self.docs.len(true) {
            return Err(format!("no document {i}"));
        }
        if i == self.docs.current() {
            return Ok(());
        }
        // One set of clipboards for all documents: a clip copied here can be pasted there.
        let clips = self.editor.as_mut().map(|e| (std::mem::take(&mut e.clipboards), e.current_clipboard));
        let from = self.doc_view();
        let old = self.take_slot();
        let next = self.docs.switch(i, old).unwrap_or_else(|old| old);
        self.put_slot(next);
        self.carry_view_over(from);
        if let (Some((clips, current)), Some(e)) = (clips, self.editor.as_mut()) {
            (e.clipboards, e.current_clipboard) = (clips, current);
        }
        Ok(())
    }

    /// Close the current document without asking; the next one (or the last) takes its place.
    fn remove_current(&mut self) {
        self.halt_play();
        // The clipboard outlives the document it was copied from.
        let clips = self.editor.as_mut().map(|e| std::mem::take(&mut e.clipboards));
        drop(self.take_slot());
        match self.docs.close_current() {
            Some(next) => {
                self.put_slot(next);
                if let (Some(clips), Some(e)) = (clips, self.editor.as_mut()) {
                    e.clipboards = clips;
                }
            }
            None => {
                self.set_audio(None);
                self.generation += 1;
                self.spectrum = None;
                self.selection = Selection::default();
            }
        }
    }

    /// The session folder and store setup for a new document.
    fn new_doc_options(&self) -> NewDocOptions {
        NewDocOptions { sessions_root: Some(self.sessions.path.clone()), ..NewDocOptions::default() }
    }

    /// Add `state` as a new untitled document and make it current. It has no file, so it counts
    /// as unsaved until Save As, and Close or Quit ask about it.
    fn add_document(&mut self, state: DocState, ctx: &egui::Context) {
        let clips = self.editor.as_mut().map(|e| std::mem::take(&mut e.clipboards));
        let old = self.editor.is_some().then(|| self.take_slot());
        self.docs.add(old);
        self.player.stop();
        self.player.set_audio(None);
        let frames = state.len_frames();
        let mut editor = Editor::new(History::new(state, History::default_cap(&self.sessions.path)));
        if let Some(clips) = clips {
            editor.clipboards = clips;
        }
        self.editor = Some(editor);
        (self.doc, self.saved, self.doc_path, self.rebuilding) = (None, None, PathBuf::new(), None);
        self.doc_serial += 1;
        self.doc_id = self.doc_serial;
        self.generation += 1;
        self.selection = Selection::default();
        self.spectrum = None;
        self.fit_on_next_frame = true;
        // The per-file panels and pictures of the document left are not carried to this one.
        self.drop_video();
        self.stats.forget();
        self.meters.new_file();
        self.matcher.new_file();
        self.apply_crossfades();
        if frames > 0 {
            self.rebuild(ctx);
        }
    }

    /// File › New › Audio File: an empty untitled document.
    fn new_audio_file(&mut self, rate: u32, channels: usize, bits: u8, ctx: &egui::Context) -> Result<(), String> {
        let fmt = sample_format_for_bits(bits).ok_or_else(|| "the bit depth must be 16, 24 or 32 (float)".to_string())?;
        let state = newdoc::empty(&self.new_doc_options(), rate, channels, fmt).map_err(|e| e.to_string())?;
        self.add_document(state, ctx);
        self.error = None;
        self.notice =
            Some(format!("New empty file: {rate} Hz, {channels} ch, {} bit", if bits == 32 { "32-bit float".to_string() } else { bits.to_string() }));
        Ok(())
    }

    /// Stream `clip` into a new document on a worker thread.
    fn start_making(&mut self, what: &'static str, clip: Clip, ctx: &egui::Context) -> Result<(), String> {
        if self.making.is_some() {
            return Err("a new document is already being made".to_string());
        }
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (opts, job_progress, ctx) = (self.new_doc_options(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
            let _ = tx.send(newdoc::from_clip(&opts, &clip, &mut report, &AtomicBool::new(false)).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.making = Some(Making { what, result: rx, progress });
        self.error = None;
        Ok(())
    }

    fn finish_making(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.making else { return };
        let result = match m.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the worker stopped unexpectedly".to_string()),
        };
        let what = m.what;
        self.making = None;
        match result {
            Ok(state) => self.add_document(state, ctx),
            Err(e) => self.error = Some(format!("{what}: {e}")),
        }
    }

    /// Edit › Copy to New: the selection (the whole file with none) becomes a new document.
    fn copy_to_new(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let doc = editor.history.current();
        let len = doc.len_frames();
        let range = if sel.start < sel.end { sel.start.min(len)..sel.end.min(len) } else { 0..len };
        if range.is_empty() {
            return Err("Copy to New: there is no audio to copy".to_string());
        }
        let clip = doc_edit::copy(doc, range, editor.channels.within(doc.channel_count())).map_err(|e| e.to_string())?;
        self.start_making("Copy to New", clip, ctx)
    }

    /// Edit › Paste to New: the clipboard becomes a new document.
    fn paste_to_new(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(clip) = self.editor.as_ref().and_then(|e| e.clipboard().cloned()) else {
            return Err("Paste to New: the clipboard is empty".to_string());
        };
        self.start_making("Paste to New", clip, ctx)
    }

    /// The `doc_id` and unsaved mark of open document `i`.
    fn doc_id_at(&self, i: usize) -> Option<(u64, bool)> {
        match self.docs.locate(i) {
            Where::Current => self.editor.as_ref().map(|_| (self.doc_id, self.unsaved())),
            Where::Parked(p) => self.docs.parked().get(p).map(|s| (s.doc_id, is_unsaved(&s.editor, &s.saved))),
        }
    }

    fn unsaved_count(&self) -> usize {
        (0..self.docs.len(self.editor.is_some())).filter(|&i| self.doc_id_at(i).is_some_and(|(_, u)| u)).count()
    }

    /// File › Close All. In the window it goes through the documents in order, asking about each
    /// unsaved one (Cancel stops). From a script (`interactive` false) it refuses while any
    /// document is unsaved, as `CLOSE` does.
    fn close_all_docs(&mut self, interactive: bool) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() || self.making.is_some() || self.save_all.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if interactive {
            self.close_all = true;
            return Ok(());
        }
        let n = self.unsaved_count();
        if n > 0 {
            return Err(format!("{n} document(s) have unsaved changes: SAVE them first, or CLOSE discard each"));
        }
        while self.editor.is_some() {
            self.remove_current();
        }
        Ok(())
    }

    /// Carries Close All on: closes documents from the first on, and stops at an unsaved one to
    /// ask. Called every frame; it waits for a prompt or a save in progress.
    fn pump_close_all(&mut self) {
        if !self.close_all || self.confirm_close || self.saving.is_some() {
            return;
        }
        loop {
            if self.editor.is_none() {
                self.close_all = false;
                return;
            }
            if self.docs.current() != 0 && self.switch_doc(0).is_err() {
                self.close_all = false;
                return;
            }
            if self.unsaved() {
                self.closing_doc = true;
                self.confirm_close = true;
                return;
            }
            self.remove_current();
        }
    }

    /// File › Save All: every unsaved document, one after another. A document with a WAV file
    /// saves to it; one without asks for a file name (the window) or is skipped (a script).
    fn save_all_docs(&mut self, interactive: bool) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() || self.making.is_some() || self.save_all.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let queue: Vec<u64> = (0..self.docs.len(true)).filter_map(|i| self.doc_id_at(i)).filter(|(_, u)| *u).map(|(id, _)| id).collect();
        if queue.is_empty() {
            self.notice = Some("Save All: nothing to save".to_string());
            return Ok(());
        }
        self.save_all = Some(SaveAllRun { queue, interactive, back_to: self.doc_id, skipped: 0 });
        Ok(())
    }

    fn pump_save_all(&mut self, ctx: &egui::Context) {
        if self.saving.is_some() || self.confirm_close {
            return;
        }
        let Some(mut run) = self.save_all.take() else { return };
        while !run.queue.is_empty() {
            let id = run.queue.remove(0);
            let n = self.docs.len(self.editor.is_some());
            let Some(i) = (0..n).find(|&i| self.doc_id_at(i).is_some_and(|(d, u)| d == id && u)) else { continue };
            if self.switch_doc(i).is_err() {
                break;
            }
            let own =
                !self.doc_path.as_os_str().is_empty() && self.editor.as_ref().is_some_and(|e| e.history.current().source().format == FileFormat::Wav);
            if !own && !run.interactive {
                run.skipped += 1;
                continue;
            }
            match self.save(ctx, false, None, false) {
                Ok(()) if self.saving.is_some() => {
                    self.save_all = Some(run);
                    return;
                }
                // The file dialog was cancelled: stop here.
                Ok(()) => {
                    run.queue.clear();
                    break;
                }
                Err(e) => {
                    self.error = Some(format!("Save All: {e}"));
                    run.queue.clear();
                    break;
                }
            }
        }
        let n = self.docs.len(self.editor.is_some());
        if let Some(i) = (0..n).find(|&i| self.doc_id_at(i).is_some_and(|(d, _)| d == run.back_to)) {
            let _ = self.switch_doc(i);
        }
        if run.skipped > 0 {
            self.error = Some(format!("Save All: {} document(s) have no file yet: use Save As", run.skipped));
        }
    }

    /// The File › New › Audio File dialog: sample rate, channels, bit depth.
    fn new_audio_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.new_audio.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("New Audio File").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("new_audio").num_columns(2).show(ui, |ui| {
                ui.label("Sample rate:");
                egui::ComboBox::from_id_salt("rate").selected_text(format!("{} Hz", d.rate)).show_ui(ui, |ui| {
                    for r in [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
                        ui.selectable_value(&mut d.rate, r, format!("{r} Hz"));
                    }
                });
                ui.end_row();
                ui.label("Channels:");
                egui::ComboBox::from_id_salt("channels")
                    .selected_text(match d.channels {
                        1 => "Mono".to_string(),
                        2 => "Stereo".to_string(),
                        6 => "5.1".to_string(),
                        n => format!("{n} channels"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.channels, 1, "Mono");
                        ui.selectable_value(&mut d.channels, 2, "Stereo");
                        ui.selectable_value(&mut d.channels, 6, "5.1");
                    });
                ui.end_row();
                ui.label("Bit depth:");
                egui::ComboBox::from_id_salt("bits")
                    .selected_text(if d.bits == 32 { "32-bit float".to_string() } else { format!("{}-bit", d.bits) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.bits, 16, "16-bit");
                        ui.selectable_value(&mut d.bits, 24, "24-bit");
                        ui.selectable_value(&mut d.bits, 32, "32-bit float");
                    });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                if let Some(d) = self.new_audio.take()
                    && let Err(e) = self.new_audio_file(d.rate, d.channels, d.bits, ctx)
                {
                    self.error = Some(e);
                }
            }
            Some(false) => self.new_audio = None,
            None => {}
        }
    }

    /// The actions that add, close or save documents: the window carries them out.
    /// `interactive` is false for the control channel, which cannot answer a dialog.
    fn run_doc_action(&mut self, id: ActionId, ctx: &egui::Context, interactive: bool) -> Result<bool, String> {
        match id {
            ActionId::CopyToNew => self.copy_to_new(ctx)?,
            ActionId::PasteToNew => self.paste_to_new(ctx)?,
            ActionId::CloseAll => self.close_all_docs(interactive)?,
            ActionId::SaveAll => self.save_all_docs(interactive)?,
            _ => return Err(format!("{} is not available now", actions::action(id).menu)),
        }
        Ok(false)
    }

    /// File › Close: close the current document, asking first when it has unsaved changes.
    fn close_doc(&mut self) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.unsaved() {
            self.closing_doc = true;
            self.confirm_close = true;
        } else {
            self.remove_current();
        }
        Ok(())
    }

    /// Name, unsaved mark and length of open document `i`.
    fn doc_info(&self, i: usize) -> (String, bool, u64) {
        let frames = |e: &Option<Editor>| e.as_ref().map_or(0, |e| e.history.current().len_frames());
        match self.docs.locate(i) {
            Where::Current => (path_name(&self.doc_path), self.unsaved(), frames(&self.editor)),
            Where::Parked(p) => self
                .docs
                .parked()
                .get(p)
                .map_or_else(|| (String::new(), false, 0), |s| (path_name(&s.doc_path), is_unsaved(&s.editor, &s.saved), frames(&s.editor))),
        }
    }

    /// The Files panel's rows, one per open document, in the order opened.
    fn file_rows(&self) -> Vec<files::Row> {
        let open = self.docs.len(self.editor.is_some());
        let imported = self.imports.iter().enumerate().map(|(k, imp)| files::Row {
            doc: open + k,
            name: path_name(&imp.path),
            unsaved: false,
            frames: imp.info.frames,
            rate: imp.info.sample_rate,
            channels: usize::from(imp.info.channels),
            bits: bits_text(imp.info.sample_format).to_string(),
            format: format!("{:?}", imp.info.format),
            path: imp.path.to_string_lossy().into_owned(),
            title: imp.info.title.clone(),
            size: imp.info.file_bytes,
            imported: true,
            mask: u64::MAX,
        });
        (0..open)
            .filter_map(|i| {
                let (path, editor, unsaved) = match self.docs.locate(i) {
                    Where::Current => (&self.doc_path, &self.editor, self.unsaved()),
                    Where::Parked(p) => {
                        let s = self.docs.parked().get(p)?;
                        (&s.doc_path, &s.editor, is_unsaved(&s.editor, &s.saved))
                    }
                };
                let state = editor.as_ref()?.history.current();
                let src = state.source();
                let meta = self.meta.get(path).cloned().unwrap_or_default();
                Some(files::Row {
                    doc: i,
                    name: path_name(path),
                    unsaved,
                    frames: state.len_frames(),
                    rate: state.sample_rate(),
                    channels: state.channel_count(),
                    bits: bits_text(src.sample_format).to_string(),
                    format: format!("{:?}", src.format),
                    path: path.to_string_lossy().into_owned(),
                    title: meta.title,
                    size: meta.size,
                    imported: false,
                    mask: editor.as_ref()?.channels.within(state.channel_count()).bits(),
                })
            })
            .chain(imported)
            .collect()
    }

    /// Read the header details of any open document's file not yet in the cache.
    fn refresh_meta(&mut self) {
        let paths: Vec<PathBuf> = std::iter::once(self.doc_path.clone()).chain(self.docs.parked().iter().map(|s| s.doc_path.clone())).collect();
        for p in paths {
            if p.as_os_str().is_empty() || self.meta.contains_key(&p) {
                continue;
            }
            let m = probe::probe(&p).map(|i| Meta { title: i.title, size: i.file_bytes }).unwrap_or_default();
            self.meta.insert(p, m);
        }
    }

    /// The document the shown row `i` stands for (rows are sorted and filtered).
    fn file_row_doc(&self, i: usize) -> Result<usize, String> {
        let rows = self.file_rows();
        self.files
            .arrange(&rows)
            .get(i)
            .map(|r| r.doc)
            .ok_or_else(|| format!("no row {i} (the table shows {} rows)", self.files.arrange(&rows).len()))
    }

    /// Make only channel `ch` of open document `doc` the one edits act on (the document is made current).
    fn select_channel(&mut self, doc: usize, ch: usize) -> Result<(), String> {
        let channels = self.file_rows().into_iter().find(|r| r.doc == doc && !r.imported).map(|r| r.channels).ok_or("open the file first")?;
        if ch >= channels {
            return Err(format!("the file has {channels} channels, not a channel {ch}"));
        }
        self.switch_doc(doc)?;
        if let Some(e) = self.editor.as_mut() {
            e.channels = wavcraft_core::ChannelMask::only(ch);
        }
        Ok(())
    }

    /// Reveal a document's file in the system file manager (Linux: its folder).
    fn reveal_file(&self, doc: usize) -> Result<(), String> {
        let path = self.file_rows().into_iter().find(|r| r.doc == doc).map(|r| r.path).unwrap_or_default();
        let (prog, args) = files::reveal_command(std::env::consts::OS, &path).ok_or("this document has no file yet: Save As first")?;
        std::process::Command::new(&prog).args(&args).spawn().map(drop).map_err(|e| format!("could not run {prog}: {e}"))
    }

    /// `path` was opened: it goes to the front of Open Recent, and the list is kept.
    fn note_recent(&mut self, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        self.recent.push(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
        self.save_recent();
    }

    fn save_recent(&self) {
        if let Some(file) = &self.recent_path
            && let Err(e) = self.recent.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the recent files list {}: {e}", file.display());
        }
    }

    /// File › Open Recent › entry `i`. A file that is gone is taken off the list instead, and
    /// the error says so.
    fn open_recent(&mut self, i: usize) -> Result<PathBuf, String> {
        let Some(path) = self.recent.paths().get(i).cloned() else { return Err(format!("no recent file {i}")) };
        if !path.is_file() {
            self.recent.remove(i);
            self.save_recent();
            return Err(format!("{} is missing, so it was removed from Open Recent", path.display()));
        }
        let ctx = self.ctx.clone();
        self.open(path.clone(), &ctx);
        Ok(path)
    }

    /// File › Import › File…: add `path` to the Files panel. Only its header is read: it is not
    /// made current, the editor is left on whatever it shows, and no session store is made until
    /// the row is first made current.
    fn import_file(&mut self, path: PathBuf) -> Result<(), String> {
        let path = std::path::absolute(&path).unwrap_or(path);
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let open_here = std::iter::once(&self.doc_path).chain(self.docs.parked().iter().map(|s| &s.doc_path));
        if open_here.chain(self.imports.iter().map(|i| &i.path)).any(|p| same_file(p, &path)) {
            return Err(format!("{} is already in the Files panel", path.display()));
        }
        let info = probe::probe(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.notice = Some(format!("Imported {} (it opens when you select it in the Files panel)", path_name(&path)));
        self.error = None;
        self.imports.push(Import { path, info });
        Ok(())
    }

    /// File › Import › File…, asking which files.
    fn pick_import(&mut self) {
        let picked =
            rfd::FileDialog::new().add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"]).pick_files();
        for path in picked.unwrap_or_default() {
            if let Err(e) = self.import_file(path) {
                self.error = Some(e);
            }
        }
    }

    /// Open the imported file `k` (the open runs in the background, as File › Open does).
    fn open_import(&mut self, k: usize) -> Result<(), String> {
        if k >= self.imports.len() {
            return Err(format!("no imported file {k}"));
        }
        let imp = self.imports.remove(k);
        let ctx = self.ctx.clone();
        self.open(imp.path, &ctx);
        Ok(())
    }

    /// Make Files panel row document `i` current: an open document is switched to, an imported
    /// file starts opening. `Some(i)` when `i` is now current, `None` while an import opens.
    fn activate_row(&mut self, i: usize) -> Result<Option<usize>, String> {
        let open = self.docs.len(self.editor.is_some());
        if i >= open {
            return self.open_import(i - open).map(|()| None);
        }
        self.switch_doc(i).map(|()| Some(i))
    }

    /// Window › Files: the open documents as a table.
    fn files_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Files");
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.text_edit_singleline(&mut self.files.filter);
        });
        self.refresh_meta();
        let rows = self.file_rows();
        let shown: Vec<(files::Row, Option<usize>)> = self
            .files
            .lines(&rows)
            .into_iter()
            .map(|l| match l {
                files::Line::File(r) => (r.clone(), None),
                files::Line::Channel { row, ch } => (row.clone(), Some(ch)),
            })
            .collect();
        let (mut toggle, mut pick_channel) = (None, None);
        let cur = self.docs.current();
        let busy = self.saving.is_some() || self.making.is_some() || self.save_all.is_some();
        let (mut go, mut open, mut close, mut reveal, mut import, mut unlist) = (None, false, false, false, false, None);
        let columns = self.files.columns.clone();
        egui::ScrollArea::both().max_height((ui.available_height() - 40.0).max(60.0)).show(ui, |ui| {
            egui::Grid::new("files_grid").striped(true).show(ui, |ui| {
                ui.label("");
                for &col in &columns {
                    let arrow = match self.files.sort {
                        Some((c, true)) if c == col => " ▲",
                        Some((c, false)) if c == col => " ▼",
                        _ => "",
                    };
                    if ui.button(format!("{}{arrow}", col.title())).clicked() {
                        self.files.click_header(col);
                    }
                }
                ui.end_row();
                for (r, channel) in &shown {
                    if let Some(ch) = *channel {
                        // A channel line: its name under the Name column, a mark if edits act on it.
                        ui.label("");
                        for k in 0..columns.len() {
                            let text = if k == 0 {
                                format!(
                                    "    {} {}",
                                    if r.channel_active(ch) && r.doc == cur { "●" } else { "○" },
                                    files::channel_label(r.channels, ch)
                                )
                            } else {
                                String::new()
                            };
                            if ui.selectable_label(false, text).on_hover_text("Edit only this channel").clicked() {
                                pick_channel = Some((r.doc, ch));
                            }
                        }
                        ui.end_row();
                        continue;
                    }
                    let on = !r.imported && r.doc == cur;
                    if r.imported {
                        ui.label("");
                    } else if ui.small_button(if self.files.is_expanded(r) { "▾" } else { "▸" }).on_hover_text("Show the channels").clicked() {
                        toggle = Some(r.clone());
                    }
                    for &col in &columns {
                        // An imported row is dimmed: it is listed, not open.
                        let text = egui::RichText::new(r.cell(col));
                        let resp = ui.selectable_label(on, if r.imported { text.weak() } else { text });
                        if resp.clicked() || resp.double_clicked() {
                            go = Some(r.doc);
                        }
                        if r.imported {
                            resp.context_menu(|ui| {
                                if ui.button("Open this file").clicked() {
                                    go = Some(r.doc);
                                    ui.close();
                                }
                                if ui.button("Remove from the list").clicked() {
                                    unlist = Some(r.doc);
                                    ui.close();
                                }
                            });
                            continue;
                        }
                        resp.context_menu(|ui| {
                            go = Some(r.doc);
                            if ui.button("Open…").clicked() {
                                open = true;
                                ui.close();
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Close")).clicked() {
                                close = true;
                                ui.close();
                            }
                            if ui.add_enabled(!r.path.is_empty(), egui::Button::new(reveal_label())).clicked() {
                                reveal = true;
                                ui.close();
                            }
                        });
                    }
                    ui.end_row();
                }
            });
        });
        ui.horizontal(|ui| {
            open |= ui.button("Open…").clicked();
            import |= ui.button("Import…").on_hover_text("Add files to this list without opening them").clicked();
            close |= ui.add_enabled(!busy && self.editor.is_some(), egui::Button::new("Close")).clicked();
            reveal |= ui.add_enabled(self.editor.is_some(), egui::Button::new(reveal_label())).clicked();
            ui.menu_button("Columns", |ui| {
                let mut cols = self.files.columns.clone();
                for c in files::Col::ALL {
                    let mut on = cols.contains(&c);
                    // At least one column stays.
                    if ui.add_enabled(!on || cols.len() > 1, egui::Checkbox::new(&mut on, c.title())).changed() {
                        if on {
                            cols.push(c);
                        } else {
                            cols.retain(|x| *x != c);
                        }
                    }
                }
                if cols != self.files.columns {
                    self.set_columns(cols);
                }
            });
        });
        // Clicking a row makes it current; Open, Close and Reveal then act on the current document.
        if let Some(i) = go
            && let Err(e) = self.activate_row(i)
        {
            self.error = Some(e);
        }
        if let Some(r) = toggle {
            let on = !self.files.is_expanded(&r);
            let _ = self.files.set_expanded(&r, on);
        }
        if let Some((doc, ch)) = pick_channel
            && let Err(e) = self.select_channel(doc, ch)
        {
            self.error = Some(e);
        }
        if let Some(i) = unlist {
            let open = self.docs.len(self.editor.is_some());
            if i >= open && i - open < self.imports.len() {
                self.imports.remove(i - open);
            }
        }
        if import {
            self.pick_import();
        }
        if open {
            self.pick_file(ui.ctx());
        }
        if close && let Err(e) = self.close_doc() {
            self.error = Some(e);
        }
        if reveal && let Err(e) = self.reveal_file(self.docs.current()) {
            self.error = Some(e);
        }
    }

    /// Show these columns, and keep the choice for the next run.
    fn set_columns(&mut self, cols: Vec<files::Col>) {
        if let Err(e) = files::save_columns(&cols) {
            log::warn!(target: "wavcraft", "could not save the Files panel columns: {e}");
        }
        self.files.columns = cols;
    }

    /// `RECENT …` from the control channel.
    fn recent_request(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        match words.next().map(str::to_ascii_lowercase).as_deref() {
            None => {
                let mut out = format!("OK count={}", self.recent.paths().len());
                for (i, p) in self.recent.paths().iter().enumerate() {
                    out.push_str(&format!(" | {i} exists={} path={}", u8::from(p.is_file()), p.display()));
                }
                out
            }
            Some("open") => match words.next().and_then(|w| w.parse::<usize>().ok()) {
                Some(i) => match self.open_recent(i) {
                    Ok(p) => format!("OK opening {}", p.display()),
                    Err(e) => {
                        self.error = Some(e.clone());
                        format!("ERR {e}")
                    }
                },
                None => "ERR RECENT open <i>, from RECENT".to_string(),
            },
            Some("clear") => {
                self.recent.clear();
                self.save_recent();
                "OK count=0".to_string()
            }
            _ => "ERR RECENT [open <i> | clear]".to_string(),
        }
    }

    /// `FILES …` from the control channel.
    fn files_request(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        match words.next().map(str::to_ascii_lowercase).as_deref() {
            None => {
                self.refresh_meta();
                let rows = self.file_rows();
                let shown = self.files.arrange(&rows);
                let mut out = format!(
                    "OK count={} total={} current={} sort={} filter={:?} columns={}",
                    shown.len(),
                    rows.len(),
                    self.docs.current(),
                    self.files.sort_text(),
                    self.files.filter.trim(),
                    files::columns_text(&self.files.columns)
                );
                let mut i = 0;
                for line in self.files.lines(&rows) {
                    let r = match line {
                        files::Line::File(r) => r,
                        files::Line::Channel { row, ch } => {
                            // Under its file: `<file row>.<channel>`, channels counted from 0.
                            out.push_str(&format!(
                                " | {}.{ch} channel label={} active={}",
                                i - 1,
                                files::channel_label(row.channels, ch),
                                u8::from(row.channel_active(ch))
                            ));
                            continue;
                        }
                    };
                    out.push_str(&format!(
                        " | {i} doc={} current={} unsaved={} frames={} rate={} ch={} bits={} format={} imported={} expanded={} size={} name={} path={}",
                        r.doc,
                        u8::from(!r.imported && r.doc == self.docs.current()),
                        u8::from(r.unsaved),
                        r.frames,
                        r.rate,
                        r.channels,
                        r.bits,
                        r.format,
                        u8::from(r.imported),
                        u8::from(self.files.is_expanded(r)),
                        r.size,
                        r.name,
                        r.path
                    ));
                    // Last, and only when there is one: a title can hold spaces.
                    if !r.title.is_empty() {
                        out.push_str(&format!(" title={}", r.title));
                    }
                    i += 1;
                }
                out
            }
            Some("sort") => {
                let (col, dir) = (words.next(), words.next().map(str::to_ascii_lowercase));
                match (col.map(str::to_ascii_lowercase).as_deref(), col.and_then(files::Col::parse), dir.as_deref()) {
                    (Some("none"), _, None) => self.files.sort = None,
                    (_, Some(c), Some("asc")) => self.files.sort = Some((c, true)),
                    (_, Some(c), Some("desc")) => self.files.sort = Some((c, false)),
                    _ => {
                        return "ERR FILES sort <name|unsaved|duration|rate|channels|bits|format|path|title|size> asc|desc, or FILES sort none"
                            .to_string();
                    }
                }
                format!("OK sort={}", self.files.sort_text())
            }
            Some("filter") => {
                let text = rest.trim().get("filter".len()..).unwrap_or("").trim();
                self.files.filter = text.to_string();
                format!("OK filter={text:?}")
            }
            Some("columns") => {
                let list = rest.trim().get("columns".len()..).unwrap_or("").trim();
                if !list.is_empty() {
                    match files::parse_columns(list) {
                        Ok(cols) => self.set_columns(cols),
                        Err(e) => return format!("ERR {e}"),
                    }
                }
                format!("OK columns={}", files::columns_text(&self.files.columns))
            }
            // Making an imported row current starts opening its file: poll STATE until busy=0.
            Some("select") if words.clone().next().is_some_and(|w| w.contains('.')) => {
                let w = words.next().unwrap_or_default();
                let (row, ch) = w.split_once('.').unwrap_or_default();
                match (row.parse::<usize>(), ch.parse::<usize>()) {
                    (Ok(i), Ok(ch)) => match self.file_row_doc(i).and_then(|d| self.select_channel(d, ch)) {
                        Ok(()) => format!("OK current={} channel={ch}", self.docs.current()),
                        Err(e) => format!("ERR {e}"),
                    },
                    _ => "ERR FILES select <row>.<channel>, channels counted from 0".to_string(),
                }
            }
            Some("select") => match words.next().and_then(|w| w.parse::<usize>().ok()) {
                Some(i) => match self.file_row_doc(i).and_then(|d| self.activate_row(d)) {
                    Ok(Some(d)) => format!("OK current={d}"),
                    Ok(None) => "OK opening".to_string(),
                    Err(e) => format!("ERR {e}"),
                },
                None => "ERR FILES select <row>, from FILES".to_string(),
            },
            Some(verb @ ("expand" | "collapse")) => match words.next().and_then(|w| w.parse::<usize>().ok()) {
                Some(i) => {
                    let rows = self.file_rows();
                    let shown = self.files.arrange(&rows);
                    match shown.get(i) {
                        Some(r) => {
                            let r = (*r).clone();
                            match self.files.set_expanded(&r, verb == "expand") {
                                Ok(()) => format!("OK expanded={}", u8::from(self.files.is_expanded(&r))),
                                Err(e) => format!("ERR {e}"),
                            }
                        }
                        None => format!("ERR no row {i} (the table shows {} rows)", shown.len()),
                    }
                }
                None => format!("ERR FILES {verb} <row>, from FILES"),
            },
            // What Reveal would run for row <i> (the current document without one). It is not run from here.
            Some("reveal") => {
                let doc = match words.next() {
                    Some(w) => w.parse::<usize>().map_err(|_| "FILES reveal [row]".to_string()).and_then(|i| self.file_row_doc(i)),
                    None => Ok(self.docs.current()),
                };
                match doc.map(|d| self.file_rows().into_iter().find(|r| r.doc == d).map(|r| r.path).unwrap_or_default()) {
                    Ok(path) => {
                        files::reveal_text(&path).map_or_else(|| "ERR this document has no file yet".to_string(), |c| format!("OK would run: {c}"))
                    }
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR FILES [sort <col> asc|desc | filter <text> | columns [<list>] | select <row>[.<channel>] | expand <row> | collapse <row> | reveal [row]]".to_string(),
        }
    }

    /// The tab strip above the editor, when more than one document is open.
    fn doc_tabs(&mut self, ui: &mut egui::Ui) {
        let (n, cur, mut go) = (self.docs.len(self.editor.is_some()), self.docs.current(), None);
        ui.horizontal_wrapped(|ui| {
            for i in 0..n {
                let (name, unsaved, _) = self.doc_info(i);
                let label = if unsaved { format!("*{name}") } else { name };
                if ui.selectable_label(i == cur, label).clicked() && i != cur {
                    go = Some(i);
                }
            }
        });
        if let Some(i) = go
            && let Err(e) = self.switch_doc(i)
        {
            self.error = Some(e);
        }
    }

    fn audio_dialog() -> rfd::FileDialog {
        rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "wave", "bwf", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "mp4", "mov", "caf", "w64", "au", "snd"])
    }

    /// File › Open: one or several files; several open one after the other, each as its own document.
    fn pick_file(&mut self, _ctx: &egui::Context) {
        if let Some(paths) = Self::audio_dialog().pick_files() {
            self.open_many(paths);
        }
    }

    /// File › Open Append › To Current… / To New…
    fn pick_append(&mut self, to_new: bool, ctx: &egui::Context) {
        if let Some(paths) = Self::audio_dialog().pick_files()
            && let Err(e) = self.start_append(to_new, paths, ctx)
        {
            self.error = Some(e);
        }
    }

    /// File › Session Folder…: pick a folder, check it, use it from the next open and save it.
    /// Show the movie at `path` in the Video panel (a Video track's clip, f129): attached again
    /// only when it is not the one already shown.
    fn sync_session_video(&mut self, path: &str, ctx: &egui::Context) {
        if self.video.as_ref().is_none_or(|v| v.path != Path::new(path)) {
            self.video = Some(VideoPanel::attach(PathBuf::from(path), ctx));
        }
        self.show_video = true;
    }

    /// Associate a video file with the open document (Video › Attach video file…).
    fn attach_video(&mut self, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("open a file first: the video goes with the open document".to_string());
        }
        self.drop_video();
        self.video = Some(VideoPanel::attach(path, ctx));
        self.show_video = true;
        Ok(())
    }

    fn drop_video(&mut self) {
        self.video_waiters.fail_all("ERR video detached");
        self.video = None;
    }

    fn pick_video(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Video", &["mov", "mp4", "m4v"]).pick_file()
            && let Err(e) = self.attach_video(path, ctx)
        {
            self.error = Some(e);
        }
    }

    /// Where the playhead is (or the cursor when stopped), in seconds.
    fn playhead_secs(&self) -> Option<f64> {
        let rate = self.editor.as_ref()?.history.current().sample_rate();
        let frame = if self.playhead_live() { self.player.position() } else { self.selection.start() };
        (rate > 0).then(|| frame as f64 / f64::from(rate))
    }

    /// Recover the `index`th session a crashed run left (Auto Save): its document and undo
    /// history come back as they were at the last journal. Refused while another file has
    /// unsaved changes, or one is opening.
    fn recover_session(&mut self, index: usize, ctx: &egui::Context) -> Result<(), String> {
        if self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some() {
            return Err("Wait for the current job to finish before recovering".to_string());
        }
        if self.unsaved() {
            return Err("Save or close the open file before recovering".to_string());
        }
        let dir = self.recovery.offers.get(index).map(|o| o.dir.clone()).ok_or_else(|| format!("no recoverable session {index}"))?;
        let store = wavcraft_core::store::StoreOptions { cache_bytes: self.settings.cache_bytes(), ..Default::default() };
        let rec = wavcraft_core::autosave::recover(&dir, store).map_err(|e| format!("Could not recover: {e}"))?;
        let cap = History::default_cap(&self.sessions.path);
        let history = rec.into_history(cap).ok_or("Could not recover: the journal holds no steps")?;
        self.doc_path = history.current().source().path.clone();
        self.editor = Some(Editor::new(history));
        self.apply_snap();
        self.saved = None;
        self.doc_serial += 1;
        self.doc_id = self.doc_serial;
        self.doc = None;
        self.set_audio(None);
        self.selection = Selection::default();
        self.spectrum = None;
        self.fit_on_next_frame = true;
        self.recovery.remove(index);
        self.notice = Some("Recovered the unsaved work (File › Save keeps it)".to_string());
        log::info!(target: "wavcraft", "recovery: restored the session in {}", dir.display());
        self.rebuild(ctx);
        Ok(())
    }

    /// Remove a crashed run's session folder the owner chose to discard.
    fn recover_take(&mut self, index: usize, ctx: &egui::Context) -> Result<(), String> {
        if self.rec.is_active() {
            return Err("Stop recording before recovering a take".to_string());
        }
        if self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some() {
            return Err("Wait for the current job to finish before recovering".to_string());
        }
        if self.unsaved() {
            return Err("Save or close the open file before recovering".to_string());
        }
        let take = self.recovery.takes.get(index).cloned().ok_or_else(|| format!("no recoverable take {index}"))?;
        let frames =
            wavcraft_core::take_recovery::repair_take(&take).map_err(|e| format!("Could not recover the take (it is kept as it is): {e}"))?;
        self.recovery.remove_take(index);
        log::info!(target: "wavcraft", "recovery: repaired take {} ({frames} frames), opening it", take.path.display());
        self.open(take.path, ctx);
        self.notice = Some("Recovered the interrupted take".to_string());
        Ok(())
    }

    /// Delete an unfinished take the owner chose to discard (after the window's second question).
    fn discard_take(&mut self, index: usize) -> Result<(), String> {
        let take = self.recovery.takes.get(index).cloned().ok_or_else(|| format!("no recoverable take {index}"))?;
        wavcraft_core::take_recovery::discard_take(&take).map_err(|e| format!("Could not discard: {e}"))?;
        log::info!(target: "wavcraft", "recovery: discarded take {} at the owner's request", take.path.display());
        self.recovery.remove_take(index);
        Ok(())
    }

    /// Remove a crashed run's session folder the owner chose to discard.
    fn discard_recovery(&mut self, index: usize) -> Result<(), String> {
        let dir = self.recovery.offers.get(index).map(|o| o.dir.clone()).ok_or_else(|| format!("no recoverable session {index}"))?;
        wavcraft_core::autosave::discard(&dir).map_err(|e| format!("Could not discard: {e}"))?;
        log::info!(target: "wavcraft", "recovery: discarded the session in {} at the owner's request", dir.display());
        self.recovery.remove(index);
        Ok(())
    }

    /// The disk figures the Preferences pages and `CACHE info` show.
    fn session_usage(&self) -> wavcraft_core::sessions::Usage {
        wavcraft_core::sessions::usage(&self.sessions.path, self.settings.autosave_keep_days, std::time::SystemTime::now())
    }

    /// Clean up old sessions: only those of dead runs, idle for `days`, with nothing to recover
    /// (`wavcraft_core::sessions` has the rules). Returns what was removed.
    fn cleanup_sessions(&mut self, days: u32) -> wavcraft_core::sessions::Cleanup {
        let done = wavcraft_core::sessions::cleanup(&self.sessions.path, days, std::time::SystemTime::now());
        self.prefs_usage = Some(self.session_usage());
        self.notice = Some(format!("Cleaned up {} old session(s), {}", done.removed, wavcraft_core::space::format_bytes(done.freed)));
        done
    }

    fn pick_session_folder(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_directory(&self.sessions.path).pick_folder() else { return };
        self.set_session_folder(Some(path));
    }

    /// Use `path` (the default when `None`) for the next opens, and keep it in the settings.
    fn set_session_folder(&mut self, path: Option<PathBuf>) {
        let (path, source) = match path {
            Some(p) => (p, FolderSource::SettingsFile),
            None => (default_sessions_root(), FolderSource::Default),
        };
        if let Err(why) = settings::check_folder(&path) {
            self.error = Some(format!("The session folder {} can't be used: {why}. Still using {}.", path.display(), self.sessions.path.display()));
            self.notice = None;
            return;
        }
        let overridden = matches!(self.sessions.source, FolderSource::CommandLine | FolderSource::Environment);
        self.settings.session_folder = (source == FolderSource::SettingsFile).then(|| path.clone());
        let saved = match &self.settings_path {
            Some(file) => self.settings.save(file).map_err(|e| format!("could not save the setting to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so it lasts until WavCraft closes".to_string()),
        };
        self.sessions = SessionFolder { path, source, warning: None };
        self.mtv.take_dir = self.sessions.path.join("takes");
        let mut msg = format!("Session folder: {} (from the next file opened)", self.sessions.path.display());
        match saved {
            Ok(()) if overridden => msg.push_str(". Saved, but --session-dir or WAVCRAFT_SESSION_DIR will win again at the next start"),
            Ok(()) => {}
            Err(e) => msg.push_str(&format!("; {e}")),
        }
        self.error = None;
        self.notice = Some(msg);
    }

    /// Show or hide a registered panel and keep the choice in the settings file.
    fn set_panel(&mut self, id: &str, shown: bool) {
        if !self.panels.set(id, shown) {
            return;
        }
        self.save_panels();
    }

    /// Keep which panels are shown and where they are docked in the settings file.
    fn save_panels(&mut self) {
        if let Some(m) = &self.monitors {
            self.layout.tag_monitors(m);
        }
        self.settings.hidden_panels = self.panels.hidden_ids();
        (self.settings.panel_layout, self.settings.zone_sizes) = self.layout.encode();
        self.settings.workspace = self.workspaces.current().to_string();
        self.layout_dirty = false;
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the panel layout to {}: {e}", file.display());
        }
    }

    /// Move floating windows whose monitor is gone onto the primary one and keep every window
    /// inside its monitor (only when the monitors are known: see `monitors`). True when any moved.
    fn reconcile_floats(&mut self) -> bool {
        let Some(m) = &self.monitors else { return false };
        let n = self.layout.reconcile_monitors(m);
        if n > 0 {
            log::info!(target: "wavcraft", "{n} floating window(s) moved onto a monitor that is there");
        }
        n > 0
    }

    /// The answer to `MONITORS` and `MONITORS simulate|real`.
    fn monitors_report(&self) -> String {
        match &self.monitors {
            None => "OK monitors=real".to_string(),
            Some(m) => {
                let list: Vec<String> = m.iter().map(|m| format!("{}@{},{},{},{}", m.name, m.pos[0], m.pos[1], m.size[0], m.size[1])).collect();
                format!("OK monitors=simulated:{}", list.join("|"))
            }
        }
    }

    /// The layout as a workspace would keep it.
    fn snapshot(&self) -> Snapshot {
        Snapshot::capture(&self.panels, &self.layout)
    }

    /// Put a workspace's panels and layout on screen and keep them in the settings file.
    fn apply_snapshot(&mut self, snap: &Snapshot) {
        (self.panels, self.layout) = snap.restore();
        self.reconcile_floats();
        self.save_panels();
    }

    /// `WORKSPACE save|switch|reset|delete <name>` (also the Window › Workspace menu). Returns
    /// the reason it was refused.
    fn workspace_command(&mut self, text: &str) -> Result<(), String> {
        let (verb, name) = text.trim().split_once(char::is_whitespace).map_or((text.trim(), ""), |(v, n)| (v, n.trim()));
        match verb {
            "save" => {
                let snap = self.snapshot();
                self.workspaces.save(name, &snap)?;
                self.save_panels();
            }
            "switch" => {
                let snap = self.workspaces.switch(name)?;
                self.apply_snapshot(&snap);
            }
            "reset" => {
                let snap = self.workspaces.reset(Some(name))?;
                self.apply_snapshot(&snap);
            }
            "delete" => self.workspaces.delete(name)?,
            _ => return Err("WORKSPACE save <name> | switch <name> | reset [name] | delete <name>".into()),
        }
        Ok(())
    }

    /// The answer to `WORKSPACES` and `WORKSPACE …`.
    fn workspaces_report(&self) -> String {
        format!("OK {}", self.workspaces.report(&self.snapshot()))
    }

    /// Close a panel (hide it in the registry; the Window menu brings it back).
    fn close_panel(&mut self, id: &str) {
        self.layout.apply_command(&mut self.panels, &format!("close {id}")).ok();
        self.save_panels();
    }

    /// The answer to `PANELS` and `PANEL …`: which panels are shown, then the zones.
    fn panels_report(&self) -> String {
        let kind = self.float_kind.map_or(String::new(), |k| format!(" float_window={k}"));
        format!("OK {} {}{kind}", self.panels.report(), self.layout.report(&self.panels))
    }

    /// The body of a docked panel.
    fn panel_body(&mut self, id: &str, ui: &mut egui::Ui) {
        match id {
            "history" => self.history_panel(ui),
            "tools" => self.tools_panel(ui),
            "selection" => self.selection_view_panel(ui),
            // Window › Frequency Analysis: a docked panel like the others (Window menu, zones, tabs).
            "freq" => {
                let doc = self.editor.as_ref().map(|e| e.history.current().clone());
                let channels = self.editor.as_ref().map_or(wavcraft_core::ChannelMask::ALL, |e| e.channels);
                let sel = self.selection_range();
                self.freq.show(ui, doc.as_ref(), channels, sel);
            }
            "markers" => self.markers_panel(ui),
            "playlist" => self.playlist_panel(ui),
            "properties" => self.properties_panel(ui),
            "transport" => self.transport_panel(ui),
            "match" => self.match_panel(ui),
            "files" => self.files_panel(ui),
            _ => {}
        }
    }

    /// One docked zone: a tab strip (drag a tab to move it, wheel to change tab, right-click for
    /// the menu) over the front tab's body. Nothing is drawn for a zone with no shown panel.
    fn dock_zone(&mut self, ui: &mut egui::Ui, zone: DockSide) {
        let tabs: Vec<&'static str> = self.layout.visible(zone, &self.panels).into_iter().filter_map(|id| panels::by_id(id).map(|p| p.id)).collect();
        let Some(front) = self.layout.active_in(zone, &self.panels).and_then(panels::by_id).map(|p| p.id) else { return };
        let size = self.layout.effective_size(zone, &self.panels, self.window_size(ui.ctx()));
        let panel = match zone {
            DockSide::Left => egui::Panel::left("zone-left"),
            DockSide::Bottom => egui::Panel::bottom("zone-bottom"),
            _ => egui::Panel::right("zone-right"),
        };
        let shown = panel.resizable(false).exact_size(size).show(ui, |ui| {
            // Fill the size the zone was given, so a short panel doesn't shrink the zone (A001: the size is part of the layout).
            ui.set_min_height(ui.available_height());
            let mut wheel = 0.0f32;
            let strip = egui::ScrollArea::horizontal().id_salt(("tabs", zone.name())).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for id in &tabs {
                        let title = panels::by_id(id).map_or(*id, |p| p.title);
                        let label = ui.selectable_label(*id == front, title);
                        let r = ui.interact(label.rect, label.id.with("tab"), Sense::click_and_drag());
                        self.tab_rects.push((id.to_string(), label.rect));
                        if r.clicked() {
                            self.layout.activate(id);
                            self.layout_dirty = true;
                        }
                        if r.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), id.to_string());
                        }
                        r.context_menu(|ui| self.panel_menu(ui, id));
                    }
                });
            });
            if ui.rect_contains_pointer(strip.inner_rect) {
                wheel =
                    ui.input(|i| i.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.x + delta.y } else { 0.0 }).sum());
            }
            if wheel != 0.0 {
                self.layout.cycle(zone, &self.panels, if wheel < 0.0 { 1 } else { -1 });
                self.layout_dirty = true;
            }
            ui.separator();
            self.panel_body(front, ui);
        });
        let rect = shown.response.rect;
        self.zone_rects.push((zone, rect));
        if shown.response.contains_pointer() {
            self.hover_next = Some(front);
        }
    }

    /// The window's (width, height) in points, for holding the zones inside it.
    fn window_size(&self, ctx: &egui::Context) -> (f32, f32) {
        let r = ctx.content_rect();
        (r.width(), r.height())
    }

    /// The dividers between the docked zones and the editor (f89, A005): drag one to resize its
    /// zone, drag where a side divider meets the bottom one to resize both, double-click to
    /// reset. Drawn after the editor so they are on top of the panel edges.
    fn dividers(&mut self, ui: &mut egui::Ui) {
        const GRAB: f32 = 5.0;
        let win = self.window_size(ui.ctx());
        let rect_of = |zone: DockSide, rects: &[(DockSide, Rect)]| rects.iter().find(|(z, _)| *z == zone).map(|(_, r)| *r);
        let rects = self.zone_rects.clone();
        let mut changed = false;
        for (zone, rect) in &rects {
            let (strip, icon) = match zone {
                DockSide::Left => {
                    (Rect::from_x_y_ranges(rect.right() - GRAB..=rect.right() + GRAB, rect.y_range()), egui::CursorIcon::ResizeHorizontal)
                }
                DockSide::Right => {
                    (Rect::from_x_y_ranges(rect.left() - GRAB..=rect.left() + GRAB, rect.y_range()), egui::CursorIcon::ResizeHorizontal)
                }
                _ => (Rect::from_x_y_ranges(rect.x_range(), rect.top() - GRAB..=rect.top() + GRAB), egui::CursorIcon::ResizeVertical),
            };
            let r = ui.interact(strip, egui::Id::new(("divider", zone.name())), Sense::click_and_drag());
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(icon);
            }
            if r.double_clicked() {
                changed |= self.layout.reset_divider(*zone);
            } else if r.dragged() {
                let d = r.drag_delta();
                changed |= self.layout.drag_divider(*zone, d.x, d.y, &self.panels, win);
            }
        }
        if let Some(bottom) = rect_of(DockSide::Bottom, &rects) {
            for side in [DockSide::Left, DockSide::Right] {
                let Some(sr) = rect_of(side, &rects) else { continue };
                let x = if side == DockSide::Left { sr.right() } else { sr.left() };
                let spot = Rect::from_center_size(pos2(x, bottom.top()), egui::vec2(GRAB * 4.0, GRAB * 4.0));
                let r = ui.interact(spot, egui::Id::new(("divider-corner", side.name())), Sense::click_and_drag());
                if r.hovered() || r.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                }
                if r.double_clicked() {
                    changed |= self.layout.reset_divider(side) | self.layout.reset_divider(DockSide::Bottom);
                } else if r.dragged() {
                    let d = r.drag_delta();
                    changed |= self.layout.drag_corner(side, d.x, d.y, &self.panels, win);
                }
            }
        }
        if changed {
            self.layout_dirty = true;
        }
    }

    /// Every floating panel in a window of its own: an operating-system window where the backend
    /// has more than one (the viewport), else egui's in-window floating window (the same call
    /// falls back by itself). Closing the window docks the panel back where it came from.
    fn floating_windows(&mut self, ctx: &egui::Context) {
        let floats: Vec<panels::Float> = self.layout.floats(&self.panels).into_iter().cloned().collect();
        let mut dock_back: Vec<String> = Vec::new();
        for f in floats {
            let title = panels::by_id(&f.id).map_or("Panel", |p| p.title);
            let vid = egui::ViewportId::from_hash_of(("float", f.id.as_str()));
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("WavCraft: {title}"))
                .with_position(egui::pos2(f.pos[0], f.pos[1]))
                .with_inner_size(vec2(f.size[0], f.size[1]))
                .with_min_inner_size(vec2(160.0, 100.0));
            let mut dock = false;
            let mut kind = "os";
            let mut rect: Option<([f32; 2], [f32; 2])> = None;
            ctx.show_viewport_immediate(vid, builder, |ui, class| {
                let mut body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Dock").on_hover_text("Put the panel back in a zone").clicked() {
                            dock = true;
                        }
                    });
                    ui.separator();
                    self.panel_body(&f.id, ui);
                };
                if class == egui::ViewportClass::EmbeddedWindow {
                    kind = "embedded";
                    body(ui);
                    // Embedded: the window is an egui area; its rect is in the main window.
                    let area = ui.ctx().memory(|m| m.area_rect(egui::Id::new(vid)));
                    rect = area.map(|r| ([r.min.x, r.min.y], [r.width(), r.height()]));
                } else {
                    egui::CentralPanel::default().show(ui, |ui| body(ui));
                    let (outer, inner, close) =
                        ui.ctx().input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
                    if close {
                        dock = true;
                    }
                    if let (Some(o), Some(i)) = (outer, inner) {
                        rect = Some(([o.min.x, o.min.y], [i.width(), i.height()]));
                    }
                }
            });
            self.float_kind = Some(kind);
            if dock {
                dock_back.push(f.id.clone());
            } else if let Some((pos, size)) = rect
                && self.monitors.is_none()
                && self.layout.set_float_rect(&f.id, pos, size)
            {
                self.layout_dirty = true;
            }
        }
        if self.layout.floats(&self.panels).is_empty() {
            self.float_kind = None;
        }
        for id in dock_back {
            self.layout.dock(&id, None).ok();
            self.save_panels();
        }
    }

    /// Window › Workspace: switch, save as new, reset to the saved layout, delete.
    fn workspace_menu(&mut self, ui: &mut egui::Ui) {
        let current = self.workspaces.current().to_string();
        let modified = self.workspaces.is_modified(&self.snapshot());
        let mut result: Result<(), String> = Ok(());
        let mut done = false;
        for (i, name) in self.workspaces.names().into_iter().enumerate() {
            let mut label = if name == current && modified { format!("{name} (changed)") } else { name.clone() };
            // f96: the first nine show their Alt+Shift+n key.
            if i < 9 {
                let keys = self.keymap.menu_text(Command::Workspace(i as u8 + 1), cfg!(target_os = "macos"));
                if !keys.is_empty() {
                    label = format!("{label}    {keys}");
                }
            }
            if ui.radio(name == current, label).clicked() && name != current {
                result = self.workspace_command(&format!("switch {name}"));
                done = true;
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.workspace_name).hint_text("Name").desired_width(120.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save as New Workspace").clicked() || enter) && !self.workspace_name.trim().is_empty() {
                result = self.workspace_command(&format!("save {}", self.workspace_name));
                if result.is_ok() {
                    self.workspace_name.clear();
                }
                done = true;
            }
        });
        if ui.add_enabled(modified, egui::Button::new("Reset to Saved Layout")).clicked() {
            result = self.workspace_command(&format!("reset {current}"));
            done = true;
        }
        ui.menu_button("Delete Workspace", |ui| {
            let others: Vec<String> = self.workspaces.names().into_iter().filter(|n| *n != current && !workspaces::is_builtin(n)).collect();
            if others.is_empty() {
                ui.add_enabled(false, egui::Label::new("No other saved workspaces"));
            }
            for name in others {
                if ui.button(&name).clicked() {
                    result = self.workspace_command(&format!("delete {name}"));
                    done = true;
                    ui.close();
                }
            }
        });
        if let Err(e) = result {
            self.error = Some(e);
        }
        if done {
            ui.close();
        }
    }

    /// Float a panel, or with `group` every tab of its zone, near the top left of the main window.
    fn float_panel(&mut self, ctx: &egui::Context, id: &str, group: bool) {
        let origin = ctx.input(|i| i.viewport().outer_rect).map_or([120.0, 120.0], |r| [r.min.x + 80.0, r.min.y + 100.0]);
        let ids: Vec<String> = match (group, self.layout.zone_of(id)) {
            (true, Some(z)) => self.layout.visible(z, &self.panels).into_iter().map(String::from).collect(),
            _ => vec![id.to_string()],
        };
        for (n, id) in ids.iter().enumerate() {
            let off = n as f32 * 28.0;
            self.layout.float(id, Some(([origin[0] + off, origin[1] + off], [320.0, 260.0]))).ok();
        }
        self.save_panels();
    }

    /// The right-click menu of a docked panel's tab.
    fn panel_menu(&mut self, ui: &mut egui::Ui, id: &str) {
        let here = self.layout.zone_of(id);
        for zone in panels::ZONES {
            if Some(zone) != here && ui.button(format!("Move to {}", zone.name())).clicked() {
                self.layout.move_to(id, zone, None).ok();
                self.save_panels();
                ui.close();
            }
        }
        if self.layout.is_floating(id) {
            if ui.button("Dock").clicked() {
                self.layout.dock(id, None).ok();
                self.save_panels();
                ui.close();
            }
        } else {
            if ui.button("Undock Panel (float)").clicked() {
                self.float_panel(ui.ctx(), id, false);
                ui.close();
            }
            if ui.button("Undock Frame (float all tabs)").clicked() {
                self.float_panel(ui.ctx(), id, true);
                ui.close();
            }
        }
        if ui.button("Maximize (`)").clicked() {
            self.layout.toggle_maximize(id);
            ui.close();
        }
        if ui.button("Close (Ctrl+W)").clicked() {
            self.close_panel(id);
            ui.close();
        }
    }

    /// While a tab is dragged: where it would land (a tab, a zone, or an edge of the window),
    /// shown as a highlight; letting go moves it there.
    fn dock_drag(&mut self, ctx: &egui::Context) {
        let Some(payload) = egui::DragAndDrop::payload::<String>(ctx) else { return };
        let id: String = (*payload).clone();
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
        let window = ctx.content_rect();
        let edge = |zone: DockSide| match zone {
            DockSide::Left => Rect::from_min_max(window.min, pos2(window.left() + window.width() * 0.2, window.bottom())),
            DockSide::Right => Rect::from_min_max(pos2(window.right() - window.width() * 0.2, window.top()), window.max),
            _ => Rect::from_min_max(pos2(window.left(), window.bottom() - window.height() * 0.25), window.max),
        };
        let mut target: Option<(DockSide, Option<usize>, Rect)> = None;
        if let Some((tab, rect)) = self.tab_rects.iter().find(|(_, r)| r.contains(pos))
            && let Some((zone, index)) = self.layout.position(tab)
        {
            target = Some((zone, Some(index), *rect));
        } else if let Some((zone, rect)) = self.zone_rects.iter().find(|(_, r)| r.contains(pos)) {
            target = Some((*zone, None, *rect));
        } else if let Some(zone) = [DockSide::Left, DockSide::Right, DockSide::Bottom].into_iter().find(|z| edge(*z).contains(pos)) {
            target = Some((zone, None, edge(zone)));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some((_, _, rect)) = target {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-drop")));
            painter.rect_filled(rect, 2.0, ACCENT.gamma_multiply(0.35));
            painter.rect_stroke(rect, 2.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        }
        if ctx.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            if let Some((zone, index, _)) = target
                && self.layout.move_to(&id, zone, index).is_ok()
            {
                self.save_panels();
            }
        } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx);
        }
    }

    /// View › Time Display: use `fmt` for the ruler, the status bar and the panels, and keep it.
    fn set_time_format(&mut self, fmt: TimeFormat) {
        self.time_format = fmt;
        self.panel_edit = None;
        self.settings.time_display = fmt;
        if let TimeFormat::Custom(n) = fmt {
            self.settings.custom_fps = n;
        }
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the time display to {}: {e}", file.display());
        }
    }

    /// `frame` pulled to the nearest snap target (Snapping; f09, f49): the ruler's ticks in the
    /// current time display, the listed `targets` and zero crossings, as the switches say.
    fn snap_at(&self, editor: &Editor, frame: u64, frames_per_px: f64, targets: &[u64]) -> u64 {
        let rate = editor.history.current().sample_rate();
        let seconds_per_px = frames_per_px / f64::from(rate.max(1));
        let major = self.time_format.tick_step_with(seconds_per_px * wavcraft_core::snap::MAJOR_TICK_PX, rate, &self.tempo);
        let minor = major / f64::from(self.time_format.subdivisions_with(major, rate, &self.tempo).max(1));
        editor.snap_with(frame, frames_per_px, targets, Some((major, minor)))
    }

    /// A frame set from the ruler, snapped to a timecode frame boundary when the display is SMPTE
    /// and snapping to frames is on.
    fn snap_gesture(&self, frame: usize) -> usize {
        match &self.doc {
            Some(d) if self.snap_frames => self.time_format.snap_to_frame(frame, d.sample_rate(), d.frames()),
            _ => frame,
        }
    }

    /// Keep `tempo` for the open document (View › Edit Tempo…, `PREF set tempo`), in the settings.
    fn set_tempo(&mut self, tempo: Tempo) {
        self.tempo = tempo;
        if self.doc_path.as_os_str().is_empty() {
            return;
        }
        self.settings.set_tempo_for(&self.doc_path, tempo);
        if let Some(file) = &self.settings_path
            && let Err(e) = self.settings.save(file)
        {
            log::warn!(target: "wavcraft", "could not save the tempo to {}: {e}", file.display());
        }
    }

    /// `frame` in the chosen time format at the open file's rate.
    /// The stored sample at `frame` of channel `ch` in the current step.
    fn sample_at(&self, ch: usize, frame: usize) -> Option<f32> {
        let state = self.editor.as_ref()?.history.current();
        let mut one = [0.0f32];
        (state.read(ch, frame as u64, &mut one).ok()? == 1).then_some(one[0])
    }

    /// The readout for editor pixel (`x`, `y`) from the editor's top-left corner: what `HOVER` answers.
    fn hover_at(&self, x: f32, y: f32) -> String {
        let (Some(layout), Some(doc)) = (&self.editor_layout, &self.doc) else { return "ERR no file is shown yet".to_string() };
        let rate = doc.sample_rate();
        match Readout::at(layout, &self.view, &self.params, rate, doc.frames(), x, y) {
            None => "OK pane=outside".to_string(),
            Some(r) => {
                let ch = r.channel().map_or_else(|| "-".to_string(), |c| c.to_string());
                let (pane, extra) = match r.pane {
                    readout::Pane::Ruler => ("ruler", String::new()),
                    readout::Pane::Outside => ("outside", String::new()),
                    readout::Pane::Wave { channel, amplitude } => {
                        let db = readout::dbfs(amplitude).map_or_else(|| "-inf".to_string(), |d| format!("{d:.2}"));
                        let sample = self.sample_at(channel, r.frame).map_or_else(|| "-".to_string(), |v| format!("{v:.6}"));
                        ("wave", format!(" amp={amplitude:.4} dbfs={db} sample={sample}"))
                    }
                    readout::Pane::Spectrum { hz, .. } => ("spectrum", format!(" hz={hz:.1}")),
                };
                format!("OK pane={pane} channel={ch} frame={} time={}{extra}", r.frame, wavcraft_core::view::format_time(r.frame, rate))
            }
        }
    }

    /// The last operation and how long it took, for STATE (A327).
    fn op_state(&self) -> String {
        match &self.last_op {
            Some((what, took)) => format!(" last_op={} last_op_ms={}", what.replace(' ', "_"), took.as_millis()),
            None => " last_op=- last_op_ms=0".to_string(),
        }
    }

    /// The `STATE` fields for the screen (f76): scale, the window rectangle and the primary
    /// monitor in physical pixels, and how start-up placed the window.
    fn window_state(&self, ctx: &egui::Context) -> String {
        let r = |r: Option<display::PxRect>| r.map_or_else(|| "-".to_string(), |r| r.to_text());
        format!(
            "pixels_per_point={:.3} window={} primary={} monitors={} startup={}",
            ctx.pixels_per_point(),
            r(self.keeper.info.rect),
            r(self.keeper.info.primary),
            self.keeper.info.monitors,
            self.keeper.startup.name(),
        )
    }

    fn fmt_time(&self, frame: usize) -> String {
        self.time_format.format_with(frame, self.doc.as_ref().map_or(0, |d| d.sample_rate()), &self.tempo)
    }

    /// Run a panel's menu item (A048). Close Panel and Maximize are the window's own commands (the
    /// tab's right-click menu runs the same ones); a panel's own items run here.
    fn run_panel_item(&mut self, panel: &str, item: &str) -> Result<(), String> {
        let maximized = self.layout.maximized(&self.panels) == Some(panel);
        if !panels::menu(panel, maximized).iter().any(|i| i.id == item) {
            return Err(if panels::by_id(panel).is_some() {
                format!("panel {panel:?} has no menu item {item:?}")
            } else {
                format!("no panel with the id {panel:?}")
            });
        }
        match item {
            "close" => {
                self.close_panel(panel);
                Ok(())
            }
            "maximize" => {
                self.layout.apply_command(&mut self.panels, &format!("maximize {panel}"))?;
                self.save_panels();
                Ok(())
            }
            "clear_history" => {
                if self.editor.as_ref().is_some_and(|e| e.history.steps().len() > 1) {
                    self.confirm_clear_history = true;
                    Ok(())
                } else {
                    Err("there is no history to clear".into())
                }
            }
            "metadata_display" => {
                self.metadata_dialog = true;
                Ok(())
            }
            _ => Err(format!("menu item {item:?} is not implemented")),
        }
    }

    fn set_metadata_field(&mut self, f: MetadataField, on: bool) {
        self.metadata.set(f, on);
        self.settings.metadata_fields = Some(self.metadata.ids());
        if let Err(e) = self.save_settings() {
            self.notice = Some(e);
        }
    }

    /// The Metadata Display dialog: which fields file-listing panels show.
    fn metadata_display_dialog(&mut self, ctx: &egui::Context) {
        if !self.metadata_dialog {
            return;
        }
        let mut changes = Vec::new();
        let mut done = false;
        egui::Window::new("Metadata Display").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                for &f in MetadataField::ALL {
                    let mut on = self.metadata.shows(f);
                    if ui.checkbox(&mut on, f.label()).changed() {
                        changes.push((f, on));
                    }
                }
                done = ui.button("Close").clicked();
            },
        );
        for (f, on) in changes {
            self.set_metadata_field(f, on);
        }
        if done {
            self.metadata_dialog = false;
        }
    }

    /// Space and the menu's Play button: stop while playing, resume while paused, else play.
    /// Hear the take's input through the output when it is being monitored (C025); say so if the
    /// output cannot (the take is recorded either way).
    fn attach_monitor(&mut self) {
        let Some(m) = self.rec.monitor_for_output() else { return };
        if let Err(why) = self.player.set_monitor(Some(m)) {
            log::warn!(target: "wavcraft", "recording: {why}");
            self.rec.set_monitor_audible(false);
            self.notice = Some(why);
        }
    }

    /// The record offset preference (C010) in milliseconds. A negative `record_offset_ms` (later
    /// is positive) cannot shift a take's start, so it counts as 0 here.
    fn offset_ms(&self) -> u32 {
        self.settings.record_offset_ms.clamp(0, rec_edit::MAX_OFFSET_MS as i32) as u32
    }

    /// The record offset for a take that just ended (C010). Returns (ms, frames at `rate`).
    fn record_offset(&mut self, rate: u32) -> (u32, u64) {
        let ms = self.offset_ms();
        let frames = rec_edit::offset_frames(ms, rate);
        let reported = self.rec.last_reported_latency_us().map_or_else(|| "none".to_string(), |us| format!("{} ms", (us + 500) / 1000));
        log::info!(target: "wavcraft", "record offset {ms} ms = {frames} frames at {rate} Hz (the device reported {reported})");
        self.rec.last_offset = Some((ms, frames));
        (ms, frames)
    }

    /// Start a take. Nothing plays while it runs (C024).
    fn start_recording(&mut self, fmt: wavcraft_core::record::RecFormat) -> Result<PathBuf, String> {
        self.start_recording_max(fmt, None)
    }

    /// Start a take that stops by itself after `max_secs` of recorded audio, if given (C028).
    fn start_recording_max(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) -> Result<PathBuf, String> {
        if self.op_running() || self.inserting.is_some() || self.running.is_some() {
            return Err("wait for the open, save or background job to finish".to_string());
        }
        self.begin_recording(fmt, max_secs)
    }

    /// An open or save is running (or the `HOLD_BUSY` test hook says so).
    fn op_running(&self) -> bool {
        self.opening.is_some() || self.saving.is_some() || self.test_busy_until.is_some_and(|t| std::time::Instant::now() < t)
    }

    /// A timed start whose time has come (0a64b5c). A take has its own writer and shares nothing
    /// with an open or save, so it starts right away even while one runs; it is never dropped.
    /// The delay and the overlap are logged, shown in the status bar and kept in `STATE`.
    fn start_timed(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) {
        let busy = self.op_running();
        match self.begin_recording(fmt, max_secs) {
            Ok(path) => {
                let late = self.rec.last_timed.map_or(0, |t| t.0);
                self.rec.last_timed = Some((late, busy));
                let msg = format!(
                    "Timed recording started {late} ms after its time{} -> {}",
                    if busy { ", while an open or save was running" } else { "" },
                    path.display()
                );
                log::info!(target: "wavcraft", "{msg}");
                if busy || late >= 1000 {
                    self.notice = Some(msg);
                }
            }
            Err(why) => log::error!(target: "wavcraft", "Timed recording could NOT start: {why}"),
        }
    }

    fn begin_recording(&mut self, fmt: wavcraft_core::record::RecFormat, max_secs: Option<u64>) -> Result<PathBuf, String> {
        self.halt_play();
        self.player.set_muted(true);
        self.rec.monitor_default = self.settings.monitor_input;
        match self.rec.start(fmt, max_secs) {
            Ok(path) => {
                self.error = None;
                self.notice = None;
                self.attach_monitor();
                Ok(path)
            }
            Err(e) => {
                self.player.set_muted(false);
                let why = format!("Recording: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Why the document may not be edited now: a recording is going into it, or has just been
    /// recorded and is being brought in.
    fn edit_lock(&self) -> Option<&'static str> {
        (self.rec.going_into_doc().is_some() || self.integrating.is_some()).then_some("wait for the recording to finish going into the file")
    }

    /// Record into the open document (C026 insert at the cursor or replace the selection, C027
    /// punch-in, C030 target channel). The take is written to its own file first, exactly like a
    /// take for a new file; only when it has stopped is it brought into the document, as one edit.
    /// Nothing in the document changes before then, and the edit is refused if it did meanwhile.
    fn start_recording_into(
        &mut self,
        want: wavcraft_core::record::RecFormat,
        mode: RecMode,
        target: RecTarget,
        preroll_ms: u64,
    ) -> Result<PathBuf, String> {
        let fail = |app: &mut WaveApp, e: String| {
            let why = format!("Recording: {e}");
            log::warn!(target: "wavcraft", "{why}");
            app.error = Some(why.clone());
            Err(why)
        };
        if self.opening.is_some()
            || self.saving.is_some()
            || self.rebuilding.is_some()
            || self.inserting.is_some()
            || self.running.is_some()
            || self.integrating.is_some()
        {
            return fail(self, "wait for the open, save or earlier recording to finish".to_string());
        }
        let Some(editor) = self.editor.as_ref() else { return fail(self, "open a file to record into first".to_string()) };
        let base = editor.history.current().clone();
        let sel = self.selection_range();
        let plan = RecPlan { mode, target, cursor: sel.start, selection: sel.clone() };
        if let Err(e) = plan.check(&base) {
            return fail(self, e);
        }
        let fmt = wavcraft_core::record::RecFormat { rate: base.sample_rate(), channels: plan.take_channels(&base), bits: want.bits, inputs: 0 };
        let punch = mode == RecMode::Punch;
        // A punch-in's take runs the preference's offset longer, so the range is still filled to its end.
        let pref_frames = rec_edit::offset_frames(self.offset_ms(), base.sample_rate());
        let limit = plan.limit_frames_with_offset(pref_frames).map(Max::Frames);
        let from = sel.start.saturating_sub(preroll_ms * u64::from(base.sample_rate()) / 1000);
        self.halt_play();
        self.player.set_muted(!punch);
        self.rec.monitor_default = self.settings.monitor_input;
        match self.rec.start_with(fmt, limit, punch) {
            Ok(path) => {
                self.rec.set_into(plan, base, self.doc_id, punch.then_some(from), fmt.rate);
                if punch {
                    // The preroll and the selection play from the original audio while the take waits.
                    self.player.play(from as usize, sel.end as usize);
                }
                self.error = None;
                self.notice = None;
                self.attach_monitor();
                Ok(path)
            }
            Err(e) => {
                self.player.set_muted(false);
                fail(self, e)
            }
        }
    }

    /// Open the finished take by the normal streamed open and build the one edit that puts it in
    /// the document; `finish_integration` applies it.
    fn integrate_take(&mut self, take: &wavcraft_core::record::Take, into: crate::recording::IntoDoc, offset_frames: u64, ctx: &egui::Context) {
        let Some(editor) = self.editor.as_ref() else { return };
        let (tx, rx) = channel();
        let (ctx, path, base, plan, xf) = (ctx.clone(), take.path.clone(), into.base.clone(), into.plan.clone(), editor.crossfades);
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let result = wavcraft_core::io::open(&path, &opts, &mut |_| {}, &cancel)
                .map_err(|e| e.to_string())
                .and_then(|take| rec_edit::apply_take_with_offset(&base, &take, &plan, xf, offset_frames).map_err(|e| e.to_string()));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.integrating = Some(Integrating { result: rx, base: into.base, doc_id: into.doc_id, path: take.path.clone(), plan: into.plan });
    }

    fn finish_integration(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.integrating else { return };
        let result = match job.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the worker stopped unexpectedly".to_string()),
        };
        let Some(Integrating { base, doc_id, path, plan, .. }) = self.integrating.take() else { return };
        let kept = format!("The take is kept in {}", path.display());
        let edit = match result {
            Ok(edit) => edit,
            Err(e) => {
                let why = format!("Recording could not be brought into the file: {e}. {kept}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why);
                return;
            }
        };
        let Some(editor) = self.editor.as_mut() else { return };
        if doc_id != self.doc_id || !Arc::ptr_eq(editor.history.current(), &base) {
            let why = format!("The file changed while recording, so the take was not put in. {kept}");
            log::warn!(target: "wavcraft", "{why}");
            self.error = Some(why);
            return;
        }
        if edit.is_noop() {
            self.notice = Some(format!("Nothing was recorded. {kept}"));
            return;
        }
        let (old_len, new_len) = (base.len_frames(), edit.state.len_frames());
        let selected = match &edit.change {
            wavcraft_core::doc::edit::Change::SameLength { range, .. } => range.clone(),
            wavcraft_core::doc::edit::Change::LengthFrom { frame } => {
                let replaced = if plan.mode == RecMode::Replace { plan.selection.end - plan.selection.start } else { 0 };
                *frame..(*frame + (new_len + replaced).saturating_sub(old_len)).min(new_len)
            }
            wavcraft_core::doc::edit::Change::None => plan.cursor..plan.cursor,
        };
        let label = edit.label.clone();
        editor.history.push_edit(edit);
        log::info!(target: "wavcraft", "{label}: frames {}..{} of {new_len}, one history step", selected.start, selected.end);
        self.selection = Selection { anchor: selected.start as usize, head: selected.end as usize };
        self.error = None;
        self.notice = Some(format!("{label}: {kept}"));
        self.rebuild(ctx);
    }

    /// Stop the take and, when `open` is set, open it in the editor by the normal path.
    fn finish_recording(&mut self, open: bool, ctx: &egui::Context) -> Option<wavcraft_core::record::Take> {
        let take = self.rec.stop()?;
        let into = self.rec.take_into();
        let _ = self.player.set_monitor(None);
        self.player.set_muted(false);
        if into.is_some() {
            self.player.stop();
        }
        if let Some(e) = &take.error {
            self.error = Some(format!("Recording stopped early: {e} The take so far is kept in {}", take.path.display()));
        }
        if take.dropped_frames > 0 {
            let ms = take.dropped_frames * 1000 / u64::from(take.fmt.rate.max(1));
            let why = format!(
                "Recording lost {} frames (about {ms} ms) because the disk could not keep up. The take is kept in {} but has gaps.",
                take.dropped_frames,
                take.path.display()
            );
            log::warn!(target: "wavcraft", "{why}");
            self.error.get_or_insert(why);
        }
        if let Some(into) = into {
            if open && take.frames > 0 {
                let (_, frames) = self.record_offset(take.fmt.rate);
                self.integrate_take(&take, into, frames, ctx);
            } else {
                log::info!(target: "wavcraft", "the take {} was not put in the file ({} frames); it is kept", take.path.display(), take.frames);
                if take.frames == 0 {
                    self.notice = Some("Nothing was recorded".to_string());
                }
            }
        } else if open && take.frames > 0 {
            self.open(take.path.clone(), ctx);
            if take.error.is_none() {
                self.notice = Some(format!("Recorded {}", take.path.display()));
            }
        }
        Some(take)
    }

    /// Record now with the last format used (the default the first time): no dialog (C031).
    fn quick_record(&mut self) -> Result<PathBuf, String> {
        let fmt = self.rec.last_format();
        self.start_recording(fmt)
    }

    /// The folder the settings files live in.
    fn config_dir(&self) -> Result<&Path, String> {
        self.settings_path.as_deref().and_then(Path::parent).ok_or_else(|| "this system has no settings folder".to_string())
    }

    /// File › Export Application Settings: the settings, shortcut overrides and workspaces in
    /// one file at `path`.
    fn export_settings(&mut self, path: &Path) -> Result<String, String> {
        let dir = self.config_dir()?.to_path_buf();
        bundle::export(&dir, path).map_err(|e| format!("could not export the settings to {}: {e}", path.display()))?;
        let msg = format!("Settings exported to {}", path.display());
        self.error = None;
        self.notice = Some(msg.clone());
        Ok(msg)
    }

    /// File › Import Application Settings: check the file at `path` whole, then replace the
    /// settings with it (the old files stay beside the new as `.old`).
    fn import_settings(&mut self, path: &Path) -> Result<String, String> {
        let dir = self.config_dir()?.to_path_buf();
        match bundle::import(&dir, path) {
            Ok(names) => {
                self.reload_settings();
                let msg = format!("Settings imported from {} ({}); the old ones are kept as .old files", path.display(), names.join(", "));
                self.error = None;
                self.notice = Some(msg.clone());
                Ok(msg)
            }
            Err(e) => {
                let why = format!("Settings not imported, nothing was changed: {e}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Read the settings file again and apply the session folder from the next open (the
    /// command-line flag and the environment variable still win, as at start).
    fn reload_settings(&mut self) {
        self.settings = self.settings_path.as_deref().map(Settings::load).unwrap_or_default();
        // Everything `WaveApp::new` derives from the settings, applied now rather than at the next start.
        self.panels = PanelSet::from_hidden(&self.settings.hidden_panels);
        self.time_format = self.settings.time_display;
        self.apply_crossfades();
        self.apply_snap();
        self.player.set_output(output_choice(&self.settings));
        self.player.set_map(self.settings.output_map.clone());
        self.player.set_quality(self.settings.playback_quality);
        self.settings.apply_to_spectrogram(&mut self.params);
        self.spectrum = None;
        if matches!(self.sessions.source, FolderSource::CommandLine | FolderSource::Environment) {
            return;
        }
        self.sessions = settings::resolve_session_folder(None, None, &self.settings, default_sessions_root(), settings::check_folder);
        self.mtv.take_dir = self.sessions.path.join("takes");
        if let Some(w) = self.sessions.warning.clone() {
            self.error = Some(w);
        }
    }

    fn reset_prefs_prompt(&mut self, ctx: &egui::Context) {
        if !self.confirm_reset_prefs {
            return;
        }
        let mut choice = None;
        egui::Window::new("Restore Preference Defaults")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    "Put your settings and shortcut changes back to the defaults? The old files are kept beside the new ones as settings.txt.reset-N and shortcuts.txt.reset-N.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Restore Defaults").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        if let Some(yes) = choice {
            self.confirm_reset_prefs = false;
            if yes && let Err(e) = self.reset_preferences() {
                self.error = Some(e);
            }
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About WavCraft").open(&mut open).default_size(vec2(640.0, 480.0)).show(ctx, |ui| {
            ui.heading("WavCraft");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("WavCraft is free software built on the open-source libraries below, each under its own licence.");
            ui.separator();
            ui.strong("Third-party notices");
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(crate::notices::TEXT).monospace().size(11.0)).wrap_mode(egui::TextWrapMode::Extend));
            });
        });
        self.show_about = open;
    }

    fn toggle_play(&mut self) {
        if self.rec.is_active() {
            // Space while recording pauses and resumes the take (C029).
            self.rec.set_paused(!self.rec.is_paused());
            return;
        }
        match self.transport.mode {
            Mode::Playing => self.stop_play(),
            Mode::Paused => self.resume_play(),
            Mode::Stopped => {
                if self.player.is_playing() {
                    // Something else is playing (the Playlist): Space stops it.
                    self.halt_play();
                } else {
                    self.start_play();
                }
            }
        }
    }

    /// Hand the player the document to play (or none); playback ends.
    fn set_audio(&mut self, state: Option<DocState>) {
        self.transport.mode = Mode::Stopped;
        self.head_scrub = ScrubState::default();
        // A file's speaker mask and channel count say how it folds down to the device (A134).
        if let Some(st) = &state {
            self.player.set_layout(st.source().channel_mask, st.channel_count());
        }
        self.player.set_audio(state);
    }

    /// Stop at once with the cursor left where it is (the file or the cursor changed under it).
    fn halt_play(&mut self) {
        self.player.stop();
        self.transport.shuttle.stop();
        self.transport.mode = Mode::Stopped;
    }

    /// How the view follows the playhead: the Playback preference (`autoscroll`), unless a manual
    /// scroll turned it off for this play.
    fn follow_mode(&self) -> Option<Follow> {
        if !self.transport.auto_scroll {
            return None;
        }
        match self.settings.autoscroll {
            settings::AutoScroll::Pagewise => Some(Follow::Page),
            settings::AutoScroll::Centred => Some(Follow::Center),
            settings::AutoScroll::Off => None,
        }
    }

    /// Play from the cursor, or the selection, as the Loop and Skip Selection options say.
    fn start_play(&mut self) {
        let Some(doc) = &self.doc else { return };
        let width = self.editor_width.max(1.0);
        let rate = doc.sample_rate();
        let req = transport::Request {
            selection: self.selection.start()..self.selection.end(),
            cursor: self.selection.start(),
            frames: doc.frames(),
            view: self.view.start..self.view.end(width),
            rate,
            looping: self.transport.looping,
            skip_selection: self.transport.skip_selection,
            preroll: self.roll_frames(self.settings.preroll().as_secs_f64(), rate),
            postroll: self.roll_frames(self.settings.postroll().as_secs_f64(), rate),
        };
        let Some((plan, from)) = transport::plan_for(&req) else {
            self.notice = Some("Nothing to play from here".to_string());
            return;
        };
        self.head_scrub.pos = None;
        self.transport.auto_scroll = true;
        if self.player.start(&plan, from) {
            self.transport.mode = Mode::Playing;
            self.transport.play_start = self.selection.start();
        }
    }

    /// Pre-roll or post-roll as frames: nothing while the option is off.
    fn roll_frames(&self, secs: f64, rate: u32) -> usize {
        if self.transport.roll { (secs.max(0.0) * f64::from(rate)) as usize } else { 0 }
    }

    /// The speeds the J and L keys step through (the `jkl_speed_steps` preference).
    fn shuttle_ladder(&self) -> Vec<f64> {
        transport::shuttle_ladder(self.settings.jkl_speed_steps())
    }

    /// The shuttle's signed speed now.
    fn shuttle_speed(&self) -> f64 {
        self.transport.shuttle.speed_in(&self.shuttle_ladder())
    }

    /// The JKL keys (`key` is `'j'`, `'k'` or `'l'`); `k_held` is true for K+J and K+L.
    fn shuttle_key(&mut self, key: char, k_held: bool) {
        if self.doc.is_none() {
            return;
        }
        let top = self.shuttle_ladder().len();
        match (key, k_held) {
            ('k', _) => {
                self.transport.shuttle.stop();
                self.stop_play();
            }
            ('l', true) => self.transport.shuttle.nudge_held(1),
            ('j', true) => self.transport.shuttle.nudge_held(-1),
            ('l', false) => self.transport.shuttle.forward_in(top),
            ('j', false) => self.transport.shuttle.backward_in(top),
            _ => return,
        }
        self.apply_shuttle();
    }

    /// Make the player run at the shuttle's speed, starting it if it is stopped.
    fn apply_shuttle(&mut self) {
        let speed = self.shuttle_speed();
        let Some(doc) = &self.doc else { return };
        if speed == 0.0 {
            return;
        }
        let (frames, backward, looping) = (doc.frames(), speed < 0.0, self.transport.looping);
        self.head_scrub.pos = None;
        let sel = self.selection.start()..self.selection.end();
        match self.transport.mode {
            Mode::Stopped => {
                let Some((plan, from)) = transport::shuttle_plan(&sel, self.selection.start(), frames, looping, backward) else {
                    self.transport.shuttle.stop();
                    self.notice = Some("Nothing to play that way from here".to_string());
                    return;
                };
                self.transport.auto_scroll = true;
                if self.player.start(&plan, from) {
                    self.transport.mode = Mode::Playing;
                    self.transport.play_start = self.selection.start();
                    self.player.set_speed(speed);
                }
            }
            Mode::Playing | Mode::Paused => {
                // The whole file (or the selection) so it can run either way from here.
                if let Some((plan, _)) = transport::shuttle_plan(&sel, self.player.position(), frames, looping, backward) {
                    self.player.replace_plan(&plan);
                }
                if self.transport.mode == Mode::Paused {
                    self.resume_play();
                } else {
                    self.player.set_speed(speed);
                }
            }
        }
    }

    /// K+J / K+L let go: the crawl ends and the playhead stays where it is.
    fn release_nudge(&mut self) {
        self.hold.ctl_nudge = false;
        if self.transport.shuttle.nudge != 0 {
            self.transport.shuttle.stop();
            self.pause_play();
        }
    }

    /// Rewind (`dir` -1) or Fast Forward (1) one step: the playhead while playing, else the cursor.
    fn step_playhead(&mut self, dir: i8) {
        let Some(doc) = &self.doc else { return };
        let (frames, step) = (doc.frames() as i64, (transport::STEP_SECS * f64::from(doc.sample_rate())) as i64);
        let delta = i64::from(dir) * step;
        if self.transport.mode == Mode::Stopped {
            let from = if dir < 0 { self.selection.start() } else { self.selection.end() } as i64;
            self.selection = Selection::at((from + delta).clamp(0, frames) as usize);
            self.head_scrub.pos = None;
        } else {
            self.player.seek((self.player.position() as i64 + delta).clamp(0, frames) as f64);
        }
    }

    /// Play the grain for the playhead the drag has reached, and finish a drag that ended.
    fn run_scrub(&mut self) {
        if let Some(frame) = self.head_scrub.pending.take() {
            self.scrub_to(frame);
        }
        if self.head_scrub.ended {
            self.head_scrub.ended = false;
            self.scrub_end();
        }
    }

    /// Scrub: put the playhead at `frame` and play a short grain there, at the speed of the drag.
    fn scrub_to(&mut self, frame: usize) {
        let Some(doc) = &self.doc else { return };
        let (frames, rate) = (doc.frames(), doc.sample_rate());
        if self.transport.mode != Mode::Stopped {
            self.stop_play();
        }
        let frame = frame.min(frames);
        let speed = self.head_scrub.tracker.touch(frame as f64, self.clock.elapsed().as_secs_f64(), rate);
        self.head_scrub.pos = Some(frame);
        self.head_scrub.speed = speed;
        if let Some((plan, from)) = transport::grain_plan(frame as f64, speed, rate, frames) {
            self.player.grain(&plan, from, speed);
        }
    }

    /// The drag is over: with nothing selected the cursor stays where the playhead was left.
    fn scrub_end(&mut self) {
        if let Some(p) = self.head_scrub.pos.take()
            && self.selection.is_empty()
        {
            self.selection = Selection::at(p);
        }
        self.head_scrub.tracker.end();
        self.head_scrub.dragging = false;
    }

    fn pause_play(&mut self) {
        match self.transport.mode {
            Mode::Playing => {
                self.player.pause();
                self.transport.mode = Mode::Paused;
            }
            Mode::Paused => self.resume_play(),
            Mode::Stopped => {}
        }
    }

    fn resume_play(&mut self) {
        let shuttle_speed = self.shuttle_speed();
        self.player.set_speed(if self.transport.shuttle.active() { shuttle_speed } else { 1.0 });
        if self.player.resume() {
            self.transport.mode = Mode::Playing;
        } else {
            self.stop_play();
        }
    }

    /// Stop. With nothing selected the cursor returns to where play started, or stays where
    /// playback stopped when Return to Start Position is off.
    fn stop_play(&mut self) {
        let was = self.transport.mode;
        let at = self.player.position();
        self.halt_play();
        if was != Mode::Stopped && self.selection.is_empty() {
            let frames = self.doc.as_ref().map_or(0, Document::frames);
            self.selection = Selection::at(self.transport.cursor_after_stop(at).min(frames));
        }
    }

    /// Notice when playback has run out by itself.
    fn poll_transport(&mut self) {
        if self.transport.mode == Mode::Playing && !self.player.is_playing() {
            self.stop_play();
        }
    }

    /// Whether a playhead is shown and moves: playing or paused (stopped, the cursor stands for it).
    fn playhead_live(&self) -> bool {
        self.player.is_playing() || self.transport.mode == Mode::Paused
    }

    /// The Transport panel: Play, Pause, Stop, Loop, go to start / end, the marker buttons, the
    /// options and the current time.
    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        let mut chosen: Option<ActionId> = None;
        let rate = self.doc.as_ref().map_or(0, Document::sample_rate);
        let enabled = self.doc.is_some() && self.saving.is_none();
        let t = self.transport;
        let auto_scroll = self.settings.autoscroll;
        // Rewind / Fast Forward: (direction held down, direction clicked).
        let mut held: Option<i8> = None;
        let mut stepped: Option<i8> = None;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let mut button = |ui: &mut egui::Ui, label: &str, tip: &str, id: ActionId, on: bool| {
                    if ui.add(egui::Button::new(label).selected(on)).on_hover_text(tip).clicked() {
                        chosen = Some(id);
                    }
                };
                button(ui, "|◀ Start", "Go to start (Home)", ActionId::CursorToStart, false);
                button(ui, "◀ Marker", "Playhead to the previous marker", ActionId::MarkerPrev, false);
                for (label, tip, dir) in [
                    ("◀◀", "Rewind: click to step back a second, hold to shuttle backward (J plays backward)", -1i8),
                    ("▶▶", "Fast Forward: click to step on a second, hold to shuttle forward (L plays forward)", 1),
                ] {
                    let r = ui.add(egui::Button::new(label)).on_hover_text(tip);
                    if r.is_pointer_button_down_on() {
                        held = Some(dir);
                    }
                    if r.clicked() {
                        stepped = Some(dir);
                    }
                }
                button(ui, "▶ Play", "Play (Space)", ActionId::Play, t.mode == Mode::Playing);
                button(ui, "❚❚ Pause", "Pause; again to resume", ActionId::Pause, t.mode == Mode::Paused);
                button(ui, "■ Stop", "Stop", ActionId::Stop, false);
                button(ui, "Marker ▶", "Playhead to the next marker", ActionId::MarkerNext, false);
                button(ui, "End ▶|", "Go to end (End)", ActionId::CursorToEnd, false);
                ui.separator();
                button(ui, "Loop", "Repeat the selection, or the view with nothing selected", ActionId::ToggleLoop, t.looping);
                button(
                    ui,
                    "Pre/Post-Roll",
                    "Play a selection from a little before it to a little after it (set the times in Preferences › Playback and Recording)",
                    ActionId::ToggleRoll,
                    t.roll,
                );
                button(
                    ui,
                    "Skip Selection",
                    "Play around the selection, skipping it, to hear a cut",
                    ActionId::ToggleSkipSelection,
                    t.skip_selection,
                );
                button(
                    ui,
                    "Return on Stop",
                    "Stop puts the cursor back where play started (Shift+X)",
                    ActionId::ToggleReturnOnStop,
                    t.return_on_stop,
                );
                button(
                    ui,
                    "Auto-Scroll",
                    "The view follows the playhead; moving it by hand turns this off for the play",
                    ActionId::ToggleAutoScroll,
                    auto_scroll != settings::AutoScroll::Off && t.auto_scroll,
                );
                button(
                    ui,
                    if auto_scroll == settings::AutoScroll::Centred { "Centered" } else { "Pagewise" },
                    "How auto-scroll follows: click to switch",
                    if auto_scroll == settings::AutoScroll::Centred { ActionId::FollowPage } else { ActionId::FollowCenter },
                    false,
                );
            });
            let live = self.playhead_live();
            let frame = if live { self.player.position() } else { self.head_scrub.pos.unwrap_or(self.selection.start()) };
            ui.separator();
            ui.monospace(self.time_format.format_with(frame, rate.max(1), &self.tempo));
            ui.label(t.mode.name());
            let speed = if self.player.is_playing() { self.player.speed() } else { 0.0 };
            ui.monospace(format!("Speed {}", transport::speed_label(speed))).on_hover_text("Playback speed; L, J and K shuttle");
        });
        let now = ui.input(|i| i.time);
        match held {
            Some(dir) => {
                let since = *self.hold.since.get_or_insert(now);
                if !self.hold.engaged && now - since > transport::HOLD_SECS {
                    self.hold.engaged = true;
                    self.transport.shuttle = transport::Shuttle { level: dir * transport::HOLD_LEVEL, nudge: 0 };
                    self.apply_shuttle();
                }
                ui.ctx().request_repaint();
            }
            None => {
                if self.hold.since.take().is_some() && self.hold.engaged {
                    self.hold.engaged = false;
                    self.hold.suppress_click = true;
                    self.transport.shuttle.stop();
                    self.pause_play();
                }
            }
        }
        if let Some(dir) = stepped {
            if std::mem::take(&mut self.hold.suppress_click) {
                // The button was held to shuttle; letting go is not a step.
            } else {
                self.step_playhead(dir);
            }
        }
        if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
        }
    }

    /// The selection as a frame range (`start == end` is just the cursor).
    fn selection_range(&self) -> Range<u64> {
        self.selection.start() as u64..self.selection.end() as u64
    }

    /// Run what a shortcut is bound to (a key press or the control channel's `KEY`). Returns
    /// whether the document changed.
    fn run_command(&mut self, command: Command, ctx: &egui::Context) -> Result<bool, String> {
        match command {
            Command::Action(id) => self.run_action(id, ctx),
            Command::Save => self.save(ctx, false, None, false).map(|()| false),
            // Save As opens the export dialog (format, sample type, AIFF kind, size, presets).
            Command::SaveAs => {
                self.open_export_dialog();
                Ok(false)
            }
            Command::PlayStop => {
                self.toggle_play();
                Ok(false)
            }
            Command::Open => {
                self.pick_file(ctx);
                Ok(false)
            }
            Command::Close => self.close_doc().map(|()| false),
            // f96: Alt+Shift+1..9 switch to the nth workspace, in menu order.
            Command::Workspace(n) => {
                let name = self.workspaces.nth(usize::from(n)).ok_or_else(|| format!("there is no workspace {n}"))?;
                self.workspace_command(&format!("switch {name}")).map(|()| false)
            }
        }
    }

    /// The open file's name without its folder and extension, for the names of files made from it.
    fn base_name(&self) -> String {
        self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned())
    }

    /// Edit › Extract Channels to Mono Files: one mono WAV per channel, in the file's own folder.
    fn start_extract(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(folder) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()).map(PathBuf::from) else {
            return Err("Save the file first: the channels are written beside it".to_string());
        };
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        if state.channel_count() < 2 {
            return Err("the file has one channel already; there is nothing to extract".to_string());
        }
        let (base, ty) = (self.base_name(), SampleType::for_source(state.source()));
        self.start_batch(ctx, "Extracting channels", move |progress| {
            export::extract_channels(&state, &folder, &base, &ExportOptions::new(ty, DEFAULT_CHANNEL_PATTERN), progress)
        })
    }

    /// Export Audio of Range Markers: each range marker (only those the selection touches when
    /// `only_selected`) as its own WAV in `folder`, named by `pattern`.
    fn start_export_ranges(&mut self, ctx: &egui::Context, folder: PathBuf, pattern: String, only_selected: bool) -> Result<(), String> {
        let sel = self.selection_range();
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let ids = (only_selected && sel.start < sel.end).then(|| state.markers().touched_kind(&sel, false));
        let (base, ty) = (self.base_name(), SampleType::for_source(state.source()));
        self.start_batch(ctx, "Exporting ranges", move |progress| {
            export::export_ranges(&state, &folder, ids.as_deref(), &base, &ExportOptions::new(ty, &pattern), progress)
        })
    }

    fn start_batch(
        &mut self,
        ctx: &egui::Context,
        what: &'static str,
        job: impl FnOnce(&mut dyn FnMut(f64)) -> Result<Vec<PathBuf>, export::ExportError> + Send + 'static,
    ) -> Result<(), String> {
        if self.batch_write.is_some() {
            return Err("a batch write is already running".to_string());
        }
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (job_progress, ctx) = (progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(job(&mut report).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.batch_write = Some(BatchWrite { what, result: rx, progress });
        Ok(())
    }

    fn finish_batch(&mut self) {
        let Some(batch) = &self.batch_write else { return };
        let result = match batch.result.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the batch write stopped unexpectedly".to_string()),
        };
        let what = batch.what;
        self.batch_write = None;
        match result {
            Ok(files) => {
                log::info!(target: "wavcraft", "{what}: wrote {} file(s)", files.len());
                let dir = files.first().and_then(|f| f.parent()).map_or_else(String::new, |d| d.display().to_string());
                self.error = None;
                self.notice = Some(format!("{what}: wrote {} file(s) to {dir}", files.len()));
            }
            Err(e) => {
                log::warn!(target: "wavcraft", "{what} failed: {e}");
                self.error = Some(format!("{what} failed: {e}"));
            }
        }
    }

    /// Run an action from the table on the open file (menu, key or control channel). Returns
    /// whether the document changed.
    fn run_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        // The snapping switches are settings: they flip with no file open too.
        if self.editor.is_none() && actions::snap_flag_value(&self.settings.snap, id).is_some() {
            self.toggle_snap_menu(id);
            return Ok(false);
        }
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        if let Some(why) = self.edit_lock() {
            return Err(why.to_string());
        }
        if matches!(id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
            return self.run_doc_action(id, ctx, true).inspect_err(|e| self.error = Some(e.clone()));
        }
        if actions::action(id).kind == Kind::Window {
            return self.run_window_action(id, ctx);
        }
        let mut sel = self.selection_range();
        // Markers go where play is, when it is playing (a point at the playhead).
        if matches!(id, ActionId::MarkerAdd | ActionId::MarkerNext | ActionId::MarkerPrev) && self.player.is_playing() {
            let at = self.player.position() as u64;
            // f53: Previous Marker steps back past a marker the playhead has only just passed.
            let from =
                if id == ActionId::MarkerPrev { transport::prev_search_from(at, self.doc.as_ref().map_or(0, Document::sample_rate)) } else { at };
            sel = from..from;
        }
        if id == ActionId::Paste {
            self.take_system_clipboard();
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let began = Instant::now();
        let outcome = editor.run(id, sel);
        let snap = editor.snap;
        match outcome {
            Ok(mut out) => {
                if matches!(id, ActionId::Copy | ActionId::Cut)
                    && let Some(clip) = editor.clipboard()
                    && let Err(why) = self.os_clipboard.publish(clip)
                {
                    log::info!(target: "wavcraft", "not on the system clipboard: {why}");
                    out.message = Some(format!("Copied to clipboard {} only: {why}", editor.current_clipboard + 1));
                }
                self.keep_snap(snap);
                if out.doc_changed {
                    self.last_op = Some((actions::action(id).menu.to_string(), began.elapsed()));
                }
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                if matches!(id, ActionId::CursorToStart | ActionId::CursorToEnd | ActionId::MarkerNext | ActionId::MarkerPrev) {
                    self.halt_play();
                    let frames = self.doc.as_ref().map_or(0, Document::frames);
                    self.view.reveal(self.selection.start() as f64, self.editor_width, frames);
                }
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{}: {e}", actions::action(id).menu);
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// What Scan would measure: the selection, or the whole file when there is none.
    fn stats_scope(&self) -> (Range<u64>, bool) {
        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames());
        if self.selection.is_empty() { (0..len, true) } else { (self.selection_range(), false) }
    }

    /// Start the panel's scan on a worker thread.
    fn start_stats(&mut self, ctx: &egui::Context) {
        let (range, whole) = self.stats_scope();
        let Some(state) = self.editor.as_ref().map(|e| e.history.current().clone()) else { return };
        self.stats.start(state, range, whole, ctx);
    }

    /// Move the cursor to the peak or to the next / previous possibly clipped run of the last scan.
    fn stats_nav(&mut self, nav: Nav) -> Result<(), String> {
        let current = self.editor.as_ref().map(|e| e.history.current().clone());
        let Some(m) = self.stats.measured.as_ref() else { return Err("scan first (Window › Sample Statistics)".to_string()) };
        if current.is_none_or(|c| !Arc::ptr_eq(&c, &m.state)) {
            return Err("the file changed since the last scan: scan again".to_string());
        }
        let cursor = self.selection.start() as u64;
        let found = match nav {
            Nav::Peak => m.stats.peak_location().map(|(_, f)| f),
            Nav::NextClip => m.stats.next_clip_after(cursor),
            Nav::PrevClip => m.stats.prev_clip_before(cursor),
        };
        let Some(frame) = found else {
            return Err(match nav {
                Nav::Peak => "nothing was measured".to_string(),
                Nav::NextClip => "no further possibly clipped run".to_string(),
                Nav::PrevClip => "no earlier possibly clipped run".to_string(),
            });
        };
        self.selection = Selection::at(frame as usize);
        self.halt_play();
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        self.view.reveal(frame as f64, self.editor_width, frames);
        Ok(())
    }

    /// Before a Paste: if another program put an audio file on the system clipboard since we last
    /// looked, it becomes the current internal clipboard's clip, and the Paste uses it.
    fn take_system_clipboard(&mut self) {
        let Some(path) = self.os_clipboard.incoming() else { return };
        let opts = OpenOptions { sessions_root: Some(self.sessions.path.clone()), ..OpenOptions::default() };
        match self.os_clipboard.take(&path, &opts) {
            Ok(clip) => {
                if let Some(e) = self.editor.as_mut() {
                    e.set_clipboard(clip);
                }
            }
            Err(why) => {
                log::warn!(target: "wavcraft", "audio on the system clipboard not used: {why}");
                self.notice = Some(format!("Audio on the system clipboard was not used: {why}"));
            }
        }
    }

    /// The actions the window carries out itself: zoom, In/Out points at the playhead, and the
    /// Insert Silence dialog.
    fn run_window_action(&mut self, id: ActionId, ctx: &egui::Context) -> Result<bool, String> {
        if matches!(id, ActionId::ViewSyncSelection | ActionId::ViewSyncZoom | ActionId::ViewSyncPlayhead) {
            self.toggle_sync(id);
            return Ok(false);
        }
        // The input meter needs no open file.
        match id {
            ActionId::MeterInputToggle => {
                self.set_meter_input(!self.meter_input);
                return Ok(false);
            }
            ActionId::MeterInputArm => {
                self.set_meter_input(true);
                return Ok(false);
            }
            ActionId::MicCheck => {
                self.mic_dialog.get_or_insert(MicDialog { secs: 5.0, result: None });
                return Ok(false);
            }
            ActionId::SpectralResolutionUp => self.step_resolution(1),
            ActionId::SpectralResolutionDown => self.step_resolution(-1),
            _ => {}
        }
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        let fmt = match id {
            ActionId::TimeDecimal => Some(TimeFormat::Decimal),
            ActionId::TimeCompact => Some(TimeFormat::Compact),
            ActionId::TimeSamples => Some(TimeFormat::Samples),
            ActionId::TimeSmpte24 => Some(TimeFormat::Smpte24),
            ActionId::TimeSmpte25 => Some(TimeFormat::Smpte25),
            ActionId::TimeSmpte2997Df => Some(TimeFormat::Smpte2997Df),
            ActionId::TimeSmpte2997Ndf => Some(TimeFormat::Smpte2997Ndf),
            ActionId::TimeSmpte30 => Some(TimeFormat::Smpte30),
            ActionId::TimeSmpte5994 => Some(TimeFormat::Smpte5994),
            ActionId::TimeCd75 => Some(TimeFormat::Cd75),
            ActionId::TimeBarsBeats => Some(TimeFormat::BarsBeats),
            _ => None,
        };
        if id == ActionId::TimeCustom {
            // Switch to the kept rate and offer to change it.
            self.set_time_format(TimeFormat::Custom(self.settings.custom_fps));
            self.custom_fps_dialog = Some(self.settings.custom_fps.to_string());
            return Ok(false);
        }
        if id == ActionId::SnapFrames {
            self.snap_frames = !self.snap_frames;
            return Ok(false);
        }
        if let Some(fmt) = fmt {
            self.set_time_format(fmt);
            return Ok(false);
        }
        if id == ActionId::GotoTime {
            if frames == 0 {
                return Err("no file is open".to_string());
            }
            self.show_time_panel = true;
            self.goto = Some(self.fmt_time(self.selection.start()));
            return Ok(false);
        }
        if frames == 0 && !matches!(id, ActionId::InsertSilence | ActionId::InsertFile | ActionId::EditTempo) {
            return Err("no file is open".to_string());
        }
        let width = self.editor_width.max(1.0);
        // Where play is, or the cursor when stopped.
        let playhead = if self.playhead_live() && !self.previewing { self.player.position() } else { self.selection.start() };
        match id {
            ActionId::ZoomIn => self.view.zoom_at(f64::from(self.settings.zoom_factor), width * 0.5, width, frames),
            ActionId::ZoomOut => self.view.zoom_at(1.0 / f64::from(self.settings.zoom_factor), width * 0.5, width, frames),
            ActionId::ZoomInAtIn => self.view.zoom_around_frame(f64::from(self.settings.zoom_factor), self.selection.start() as f64, width, frames),
            ActionId::ZoomInAtOut => self.view.zoom_around_frame(f64::from(self.settings.zoom_factor), self.selection.end() as f64, width, frames),
            id if actions::zoom_preset_slot(id).is_some() => {
                let slot = actions::zoom_preset_slot(id).unwrap_or(0);
                let rate = self.doc.as_ref().map_or(1, Document::sample_rate);
                self.view.zoom_to_span(self.settings.zoom_presets.0[slot] * f64::from(rate), width, frames);
            }
            ActionId::ZoomOutFull => self.fit_on_next_frame = true,
            ActionId::ZoomAmpIn => self.amp_zoom.by(2.0),
            ActionId::ZoomAmpOut => self.amp_zoom.by(0.5),
            ActionId::ZoomAmpReset => self.amp_zoom = AmpZoom::full(),
            ActionId::ChannelsLayered => self.set_channel_layout(ChannelLayout::Layered),
            ActionId::ChannelsStacked => self.set_channel_layout(ChannelLayout::Stacked),
            ActionId::ToolSelect => self.tool = Tool::Select,
            ActionId::ToolHand => self.tool = Tool::Hand,
            ActionId::ToolZoom => self.tool = Tool::Zoom,
            ActionId::ToolMarquee => self.tool = Tool::Marquee,
            ActionId::ZoomToSelection if !self.selection.is_empty() => {
                let (a, b) = (self.selection.start() as f64, self.selection.end() as f64);
                self.view.frames_per_px = ((b - a) / f64::from(width)).max(1.0 / 64.0);
                self.view.start = a;
                self.view.clamp(width, frames);
            }
            ActionId::SelectCurrentView => self.selection = Selection::visible(&self.view, width, frames),
            ActionId::SetIn => {
                let end = self.selection.end().max(playhead);
                self.selection = Selection { anchor: playhead, head: end };
            }
            ActionId::SetOut => {
                let start = self.selection.start().min(playhead);
                self.selection = Selection { anchor: start, head: playhead };
            }
            ActionId::Play => match self.transport.mode {
                Mode::Stopped => self.start_play(),
                Mode::Paused => self.resume_play(),
                Mode::Playing => {}
            },
            ActionId::Pause => self.pause_play(),
            ActionId::Stop => self.stop_play(),
            ActionId::ToggleLoop => {
                self.transport.looping = !self.transport.looping;
                self.player.set_looping(self.transport.looping);
            }
            ActionId::Rewind => self.step_playhead(-1),
            ActionId::FastForward => self.step_playhead(1),
            ActionId::ToggleRoll => self.transport.roll = !self.transport.roll,
            ActionId::ToggleSkipSelection => self.transport.skip_selection = !self.transport.skip_selection,
            ActionId::ToggleReturnOnStop => self.transport.return_on_stop = !self.transport.return_on_stop,
            // Auto-scroll is the Playback preference (`autoscroll`): pagewise, centred or off.
            ActionId::ToggleAutoScroll => {
                let next = if self.settings.autoscroll == settings::AutoScroll::Off { "pagewise" } else { "off" };
                let _ = self.pref_set("autoscroll", next);
                self.transport.auto_scroll = true;
            }
            ActionId::FollowPage => {
                let _ = self.pref_set("autoscroll", "pagewise");
                self.transport.auto_scroll = true;
            }
            ActionId::FollowCenter => {
                let _ = self.pref_set("autoscroll", "centred");
                self.transport.auto_scroll = true;
            }
            ActionId::EditTempo => self.tempo_dialog = Some((self.tempo.bpm_text(), self.tempo.meter_text())),
            ActionId::InsertSilence if self.editor.is_some() => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                self.insert_silence = Some(self.time_format.format_with(rate as usize, rate, &self.tempo));
            }
            ActionId::InsertFile => self.pick_insert_file(ctx)?,
            ActionId::ConvertSampleType if self.editor.is_some() => {
                let doc = self.editor.as_ref().map(|e| e.history.current().clone());
                self.convert_dialog = doc.map(|d| ConvertDialog::new(d.sample_rate(), d.channel_count()));
            }
            ActionId::InterpretSampleRate if self.editor.is_some() => {
                self.interpret = self.editor.as_ref().map(|e| e.history.current().sample_rate().to_string());
            }
            // f79: next to the file, as <name>_L.wav, <name>_R.wav (the menu and the control channel).
            ActionId::ExtractChannels => return self.start_extract(ctx).map(|()| false),
            ActionId::ExportRanges => return Err("Export Audio of Range Markers needs a folder: use the menu, or EXPORT_RANGES <folder>".to_string()),
            // Repeat Last Command with its dialog: Insert Silence opens its dialog with the last
            // length; the other repeatable commands have no dialog and simply run again.
            ActionId::RepeatLastDialog => {
                let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
                if !editor.enabled(id, &self.selection_range()) {
                    return Err(format!("{} is not available now", actions::action(id).menu));
                }
                if let Some(Repeat::InsertSilence(frames)) = editor.last {
                    let rate = editor.history.current().sample_rate();
                    self.insert_silence = Some(self.time_format.format_with(frames as usize, rate, &self.tempo));
                } else if let Some(Repeat::Effect(i, params)) = editor.last.clone().filter(|r| matches!(r, Repeat::Effect(_, p) if !p.is_empty())) {
                    // An effect with settings reopens its dialog on the last values.
                    self.effect_dialog = EffectDialog::new(i, Some(params));
                } else {
                    return self.run_action(ActionId::RepeatLast, ctx);
                }
            }
            ActionId::StatsShow => self.stats.show = true,
            ActionId::StatsGotoPeak => self.stats_nav(Nav::Peak)?,
            ActionId::StatsNextClip => self.stats_nav(Nav::NextClip)?,
            ActionId::StatsPrevClip => self.stats_nav(Nav::PrevClip)?,
            _ => return Err(format!("{} is not available now", actions::action(id).menu)),
        }
        Ok(false)
    }

    /// File › Import › Raw Data…: rate, channels, sample format, byte order and start offset.
    fn raw_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.raw_import.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Import Raw Data").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.label(d.path.display().to_string());
                egui::Grid::new("raw_import").num_columns(2).show(ui, |ui| {
                    ui.label("Sample rate (Hz):");
                    ui.text_edit_singleline(&mut d.rate);
                    ui.end_row();
                    ui.label("Channels (1-32):");
                    ui.text_edit_singleline(&mut d.channels);
                    ui.end_row();
                    ui.label("Sample format:");
                    egui::ComboBox::from_id_salt("raw_enc").selected_text(RAW_CHOICES[d.encoding].0).show_ui(ui, |ui| {
                        for (i, (name, _)) in RAW_CHOICES.iter().enumerate() {
                            ui.selectable_value(&mut d.encoding, i, *name);
                        }
                    });
                    ui.end_row();
                    ui.label("Byte order:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.big_endian, false, "Little-endian");
                        ui.radio_value(&mut d.big_endian, true, "Big-endian");
                    });
                    ui.end_row();
                    ui.label("Start offset (bytes):");
                    ui.text_edit_singleline(&mut d.offset);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            },
        );
        match choice {
            Some(true) => match d.spec() {
                Ok(spec) => {
                    let path = d.path.clone();
                    self.raw_import = None;
                    self.open_as(path, Some(spec), ctx);
                }
                Err(e) => self.error = Some(format!("Import Raw Data: {e}")),
            },
            Some(false) => self.raw_import = None,
            None => {}
        }
    }

    /// Edit › Mix Paste: paste the clipboard (or a file) at the cursor with the request's mode.
    /// One History step, "Mix Paste".
    fn run_mix_paste(&mut self, req: mixpaste::Request, ctx: &egui::Context) -> Result<(), String> {
        let at = self.selection.start() as u64;
        let source = match &req.file {
            Some(path) => Some(mixpaste::clip_from_file(path, &self.sessions.path)?),
            None => None,
        };
        let editor = self.editor.as_mut().ok_or("Mix Paste: no file is open")?;
        let out = editor.mix_paste(at, source, &req.opts, &AtomicBool::new(false)).map_err(|e| format!("Mix Paste: {e}"))?;
        self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
        self.notice = out.message;
        self.error = None;
        if out.doc_changed {
            self.rebuild(ctx);
        }
        Ok(())
    }

    fn mix_paste_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.mix_paste.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Mix Paste").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            choice = dialog.ui(ui);
        });
        match choice {
            Some(true) => match dialog.request() {
                Ok(req) => {
                    self.mix_paste = None;
                    if let Err(why) = self.run_mix_paste(req, ctx) {
                        self.error = Some(why);
                    }
                }
                Err(why) => self.error = Some(format!("Mix Paste: {why}")),
            },
            Some(false) => self.mix_paste = None,
            None => {}
        }
    }

    /// View › Edit Tempo…: the tempo and time signature Bars and Beats counts by, kept for this file.
    fn tempo_dialog(&mut self, ctx: &egui::Context) {
        let Some((bpm, meter)) = self.tempo_dialog.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Edit Tempo").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Tempo (beats a minute):");
                ui.text_edit_singleline(bpm);
            });
            ui.horizontal(|ui| {
                ui.label("Time signature (e.g. 4/4):");
                let r = ui.text_edit_singleline(meter);
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    choice = Some(true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                let Some((bpm, meter)) = self.tempo_dialog.take() else { return };
                let parsed = bpm.trim().parse::<f64>().ok().zip(Tempo::parse_meter(&meter)).and_then(|(b, (n, d))| Tempo::new(b, n, d));
                match parsed {
                    Some(t) => self.set_tempo(t),
                    None => self.error = Some("Edit Tempo: a tempo from 20 to 400 and a time signature such as 4/4 or 6/8".to_string()),
                }
            }
            Some(false) => self.tempo_dialog = None,
            None => {}
        }
    }

    /// Insert › Silence…: a length in the chosen time format, inserted at the cursor on every channel.
    fn insert_silence_dialog(&mut self, ctx: &egui::Context) {
        let time_format = self.time_format;
        let Some(text) = self.insert_silence.as_mut() else { return };
        let mut choice = None;
        egui::Window::new(tr("dialog.insert_silence")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Duration ({}):", time_format.menu()));
                    let r = ui.text_edit_singleline(text);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        choice = Some(true);
                    }
                    r.request_focus();
                });
                ui.horizontal(|ui| {
                    if ui.button(tr("button.ok")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr("button.cancel")).clicked() {
                        choice = Some(false);
                    }
                });
            },
        );
        match choice {
            Some(true) => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                let frames = self.insert_silence.take().and_then(|t| self.time_format.parse_with(&t, rate, &self.tempo)).filter(|f| *f > 0);
                let Some(frames) = frames else {
                    self.error = Some(format!("Insert Silence: type a length above 0 in the {} format", self.time_format.menu()));
                    return;
                };
                self.run_insert_silence(frames as u64, ctx);
            }
            Some(false) => self.insert_silence = None,
            None => {}
        }
    }

    /// The custom frame rate dialog: a whole number of frames a second, 2 to 1000.
    fn custom_fps_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = self.custom_fps_dialog.as_mut() else { return };
        let mut choice = None;
        egui::Window::new("Custom frame rate").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Frames per second (2 to 1000):");
                    let r = ui.text_edit_singleline(text);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        choice = Some(true);
                    }
                    r.request_focus();
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            },
        );
        match choice {
            Some(true) => {
                let typed = self.custom_fps_dialog.take().unwrap_or_default();
                match typed.trim().parse::<u32>().ok().and_then(wavcraft_core::smpte::SmpteRate::custom) {
                    Some(r) => self.set_time_format(TimeFormat::Custom(r.nominal() as u16)),
                    None => self.error = Some("Custom frame rate: type a whole number from 2 to 1000".to_string()),
                }
            }
            Some(false) => self.custom_fps_dialog = None,
            None => {}
        }
    }

    fn run_insert_silence(&mut self, frames: u64, ctx: &egui::Context) -> Option<String> {
        if let Some(why) = self.edit_lock() {
            return Some(why.to_string());
        }
        if self.running.is_some() || self.saving.is_some() {
            return Some("wait for the background job or the save to finish".to_string());
        }
        let at = self.selection.start() as u64;
        let editor = self.editor.as_mut()?;
        let rate = editor.history.current().sample_rate();
        // The 24-hour cap, whichever way the length was typed.
        if let Err(why) = silence_frames(frames as f64 / f64::from(rate.max(1)), rate) {
            self.error = Some(why.clone());
            return Some(why);
        }
        match editor.insert_silence(at, frames) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.notice = out.message;
                self.error = None;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                None
            }
            Err(e) => {
                let why = format!("Insert Silence: {e}");
                self.error = Some(why.clone());
                Some(why)
            }
        }
    }

    /// The selection as `<start>-<end>` in the chosen format (STATE reports it).
    fn selection_text(&self) -> String {
        format!("{}-{}", self.fmt_time(self.selection.start()), self.fmt_time(self.selection.end()))
    }

    /// Move the cursor to `frame` (kept inside the file), stop playing and bring it into view.
    fn goto_frame(&mut self, frame: usize) {
        let total = self.doc.as_ref().map_or(0, Document::frames);
        let frame = frame.min(total);
        self.halt_play();
        self.selection = Selection::at(frame);
        let width = self.editor_width.max(1.0);
        if (frame as f64) < self.view.start || (frame as f64) > self.view.end(width) {
            let half = (self.view.end(width) - self.view.start) / 2.0;
            self.view.set_start((frame as f64 - half).max(0.0) as usize, width, total);
        }
    }

    /// Window › Time panel: the playhead (or, stopped, the cursor) as a large readout.
    fn time_panel(&mut self, ui: &mut egui::Ui) {
        let frame = if self.playhead_live() { self.player.position() } else { self.selection.start() };
        let rate = self.doc.as_ref().map_or(0, Document::sample_rate);
        let text = self.time_format.format_with(frame, rate, &self.tempo);
        let mut jump = None;
        ui.horizontal(|ui| {
            ui.label("Time");
            if let Some(typed) = self.goto.as_mut() {
                // Go to Time: type a time in the chosen format, Enter moves the cursor, Escape drops it.
                let r = ui.add(egui::TextEdit::singleline(typed).font(egui::FontId::monospace(34.0)).desired_width(260.0));
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                if r.lost_focus() {
                    if !ui.input(|i| i.key_pressed(Key::Escape)) {
                        match self.time_format.parse_with(typed, rate, &self.tempo) {
                            Some(f) => jump = Some(f),
                            None => self.error = Some(format!("\"{typed}\" is not a time in the {} format", self.time_format.menu())),
                        }
                    }
                    self.goto = None;
                }
            } else {
                let r = ui.add(egui::Label::new(egui::RichText::new(&text).monospace().size(34.0)).sense(egui::Sense::click_and_drag()));
                let r = r.on_hover_text("Click to type a time; drag up or down across a digit group to scrub");
                if self.doc.is_some() {
                    if r.clicked() {
                        self.goto = Some(text.clone());
                    }
                    if let Some(f) = self.scrub_drag(&r, "time", &text, frame, rate) {
                        jump = Some(f);
                    }
                }
            }
            ui.label(self.time_format.menu());
        });
        if let Some(f) = jump {
            self.goto_frame(f);
        }
    }

    /// Dragging up or down across the digits of the time `text` shown in `r` steps the digit
    /// group under the pointer (A105). Returns the new frame when a step happened.
    fn scrub_drag(&mut self, r: &egui::Response, name: &'static str, text: &str, value: usize, rate: u32) -> Option<usize> {
        let total = self.doc.as_ref().map_or(0, Document::frames);
        if r.drag_started() {
            let char_w = (r.rect.width() / text.chars().count().max(1) as f32).max(1.0);
            let x = r.interact_pointer_pos().map_or(0.0, |p| p.x - r.rect.left());
            let unit = self.time_format.unit_at(text, (x / char_w).floor().max(0.0) as usize, rate);
            self.scrub = unit.map(|unit| Scrub { name, unit, pending: 0.0 });
        }
        if !r.dragged() {
            if r.drag_stopped() {
                self.scrub = None;
            }
            return None;
        }
        let scrub = self.scrub.as_mut().filter(|s| s.name == name)?;
        scrub.pending -= r.drag_delta().y;
        let steps = (scrub.pending / SCRUB_PX_PER_STEP).trunc();
        if steps == 0.0 {
            return None;
        }
        scrub.pending -= steps * SCRUB_PX_PER_STEP;
        let new = TimeFormat::scrub(value, steps as i64, scrub.unit, total);
        (new != value).then_some(new)
    }

    /// Window › Selection/View panel: begin, end and duration of the selection and of the visible
    /// view, in the chosen format. Type a value and press Enter (or leave the box) to apply it.
    fn selection_view_panel(&mut self, ui: &mut egui::Ui) {
        let (frames, rate) = self.doc.as_ref().map_or((0, 0), |d| (d.frames(), d.sample_rate()));
        let width = self.editor_width.max(1.0);
        let (vs, ve) = (self.view.start.round() as usize, (self.view.end(width).round() as usize).min(frames));
        let (ss, se) = (self.selection.start(), self.selection.end());
        let values =
            [("sel_start", ss), ("sel_end", se), ("sel_len", se - ss), ("view_start", vs), ("view_end", ve), ("view_len", ve.saturating_sub(vs))];
        let mut committed = None;
        egui::Grid::new("selection_view").num_columns(4).show(ui, |ui| {
            ui.label("");
            ui.strong("Begin");
            ui.strong("End");
            ui.strong("Duration");
            ui.end_row();
            for (row, label) in [(0, "Selection"), (3, "View")] {
                ui.label(label);
                for (name, value) in &values[row..row + 3] {
                    if let Some(frame) = self.time_box(ui, name, *value, rate) {
                        committed = Some((*name, frame));
                    }
                }
                ui.end_row();
            }
        });
        let Some((name, frame)) = committed else { return };
        if self.doc.is_none() {
            return;
        }
        match name {
            "sel_start" => self.selection = self.selection.with_start(frame, frames),
            "sel_end" => self.selection = self.selection.with_end(frame, frames),
            "sel_len" => self.selection = self.selection.with_len(frame, frames),
            "view_start" => self.view.set_start(frame, width, frames),
            "view_end" => self.view.set_end(frame, width, frames),
            _ => self.view.set_duration(frame, width, frames),
        }
        if name.starts_with("sel") {
            self.halt_play();
        }
    }

    /// One text box of the Selection/View panel. Shows `value` formatted until it is clicked into;
    /// returns the frame typed when Enter is pressed or the box is left (Escape drops the text).
    fn time_box(&mut self, ui: &mut egui::Ui, name: &'static str, value: usize, rate: u32) -> Option<usize> {
        let editing = matches!(&self.panel_edit, Some((n, _)) if *n == name);
        if !editing {
            // Shown as text: a click starts typing, a drag across the digits scrubs (A105).
            let shown = self.time_format.format_with(value, rate, &self.tempo);
            let r = ui.add(egui::Label::new(egui::RichText::new(&shown).monospace()).sense(egui::Sense::click_and_drag()));
            let r = r.on_hover_text("Click to type; drag up or down across a digit group to scrub");
            if r.clicked() && rate != 0 {
                self.panel_edit = Some((name, shown.clone()));
            }
            return if rate == 0 { None } else { self.scrub_drag(&r, name, &shown, value, rate) };
        }
        let mut text = self.panel_edit.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).font(egui::TextStyle::Monospace));
        if r.changed() {
            self.panel_edit = Some((name, text.clone()));
        }
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        if !r.lost_focus() {
            return None;
        }
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        self.panel_edit = None;
        if escaped || rate == 0 {
            return None;
        }
        match self.time_format.parse_with(&text, rate, &self.tempo) {
            Some(frame) if self.time_format.format_with(frame, rate, &self.tempo) != self.time_format.format_with(value, rate, &self.tempo) => {
                Some(frame)
            }
            Some(_) => None,
            None => {
                self.error = Some(format!("\"{text}\" is not a time in the {} format", self.time_format.menu()));
                None
            }
        }
    }

    /// Whether a menu or toolbar entry can be used now (the same test the Edit and View menus use).
    fn item_enabled(&self, item: &Item) -> bool {
        let idle = self.saving.is_none() && self.running.is_none();
        match item.cmd {
            Cmd::Action(id) => {
                idle && (self.editor.as_ref().is_some_and(|e| e.enabled(id, &self.selection_range()))
                    || actions::snap_flag_value(&self.settings.snap, id).is_some())
            }
            Cmd::Open => idle,
            Cmd::Save | Cmd::PlayStop => idle && self.editor.is_some(),
            Cmd::Tool(_) => true,
        }
    }

    /// Run a menu or toolbar entry. `scripted`: from the control channel, where a file dialog
    /// can't be answered. Returns whether the document changed.
    fn run_item(&mut self, item: Item, ctx: &egui::Context, scripted: bool) -> Result<bool, String> {
        if !self.item_enabled(&item) {
            return Err(format!("{} is not available now", item.label));
        }
        match item.cmd {
            Cmd::Action(id) => self.run_action(id, ctx),
            Cmd::Open if scripted => Err("Open asks for a file in a dialog: use OPEN <path>".to_string()),
            Cmd::Open => {
                self.pick_file(ctx);
                Ok(false)
            }
            Cmd::Save => self.save(ctx, false, None, false).map(|()| false),
            Cmd::PlayStop => {
                self.toggle_play();
                Ok(false)
            }
            Cmd::Tool(t) => {
                self.tool = t;
                Ok(false)
            }
        }
    }

    /// Draw entries as menu rows (enabled state and the keys from the keymap); returns the chosen one.
    fn menu_rows(&self, ui: &mut egui::Ui, entries: &[Entry]) -> Option<Item> {
        let mac = cfg!(target_os = "macos");
        let mut chosen = None;
        for e in entries {
            match e {
                Entry::Sep => {
                    ui.separator();
                }
                Entry::Item(item) => {
                    let label = match item.cmd {
                        Cmd::Action(id) => self.editor.as_ref().map_or_else(|| item.label.to_string(), |ed| ed.label(id)),
                        _ => item.label.to_string(),
                    };
                    let keys = match item.cmd {
                        Cmd::Action(id) => self.keymap.menu_text(Command::Action(id), mac),
                        _ => String::new(),
                    };
                    if ui.add_enabled(self.item_enabled(item), egui::Button::new(label).shortcut_text(keys)).clicked() {
                        chosen = Some(*item);
                        ui.close();
                    }
                }
            }
        }
        chosen
    }

    /// The toolbar under the menu bar: the common commands. Its tool entries are drawn by the
    /// tools bar below it (they stay in the table for `TOOLBAR run`).
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        ui.horizontal(|ui| {
            for e in menus::toolbar() {
                match e {
                    Entry::Sep => {
                        ui.separator();
                    }
                    Entry::Item(item) if matches!(item.cmd, Cmd::Tool(_)) => {}
                    Entry::Item(item) => {
                        let on = item.cmd == Cmd::PlayStop && self.player.is_playing();
                        let b = egui::Button::new(menus::glyph(&item)).selected(on);
                        if ui.add_enabled(self.item_enabled(&item), b).on_hover_text(item.label).clicked() {
                            chosen = Some(item);
                        }
                    }
                }
            }
        });
        if let Some(item) = chosen
            && let Err(e) = self.run_item(item, ui.ctx(), false)
        {
            self.notice = Some(e);
        }
    }

    /// Window › Tools: the tools by name, the active one selected; unbuilt ones greyed.
    fn tools_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tools");
        for t in menus::TOOLS {
            let on = t.tool.is_some_and(|x| x == self.tool);
            if ui
                .add_enabled(t.tool.is_some(), egui::Button::new(t.label).selected(on).min_size(vec2(ui.available_width(), 0.0)))
                .on_hover_text(t.hint)
                .clicked()
                && let Some(tool) = t.tool
            {
                self.tool = tool;
            }
        }
    }

    /// `CONTEXTMENU <editor|ruler|history> list` and `... run <item>`.
    fn control_contextmenu(&mut self, rest: &str, ctx: &egui::Context) -> String {
        let mut it = rest.split_whitespace();
        let (Some(menu), Some(verb)) = (it.next().and_then(Menu::by_name), it.next()) else {
            return "ERR CONTEXTMENU editor|ruler|history list|run <item>".to_string();
        };
        match verb {
            "list" => {
                let items: Vec<String> = menu
                    .entries()
                    .iter()
                    .filter_map(|e| if let Entry::Item(i) = e { Some(format!("{}:{}", i.name, u8::from(self.item_enabled(i)))) } else { None })
                    .collect();
                format!("OK items={}", items.join(","))
            }
            "run" => match it.next().and_then(|n| menus::menu_item(menu, n)) {
                Some(item) => self.run_item_reply(item, ctx),
                None => "ERR no such item in that menu (CONTEXTMENU <menu> list)".to_string(),
            },
            _ => "ERR CONTEXTMENU editor|ruler|history list|run <item>".to_string(),
        }
    }

    /// `TOOLBAR list` (toolbar buttons and the Tools panel) and `TOOLBAR run <item>`.
    fn control_toolbar(&mut self, rest: &str, ctx: &egui::Context) -> String {
        let mut it = rest.split_whitespace();
        match it.next() {
            Some("list") => {
                let items: Vec<String> = menus::toolbar()
                    .iter()
                    .filter_map(|e| if let Entry::Item(i) = e { Some(format!("{}:{}", i.name, u8::from(self.item_enabled(i)))) } else { None })
                    .collect();
                let tools: Vec<String> = menus::TOOLS.iter().map(|t| format!("{}:{}", t.name, u8::from(t.tool.is_some()))).collect();
                let active = menus::TOOLS.iter().find(|t| t.tool == Some(self.tool)).map_or("-", |t| t.name);
                format!("OK items={} tools={} active={active}", items.join(","), tools.join(","))
            }
            Some("run") => match it.next().and_then(menus::toolbar_item) {
                Some(item) => self.run_item_reply(item, ctx),
                None => "ERR no such toolbar item (TOOLBAR list)".to_string(),
            },
            _ => "ERR TOOLBAR list|run <item>".to_string(),
        }
    }

    fn run_item_reply(&mut self, item: Item, ctx: &egui::Context) -> String {
        match self.run_item(item, ctx, true) {
            Ok(changed) => format!("OK selection={}-{} changed={}", self.selection.start(), self.selection.end(), u8::from(changed)),
            Err(e) => format!("ERR {e}"),
        }
    }

    /// Window › History: every step, the current one highlighted; click one to go to it.
    fn history_panel(&mut self, ui: &mut egui::Ui) {
        // The panel's background menu: Undo and Redo (each row keeps its own Delete State menu).
        let background = ui.interact(ui.max_rect(), ui.id().with("history-menu"), Sense::click());
        let entries = Menu::History.entries();
        let mut chosen = None;
        background.context_menu(|ui| chosen = self.menu_rows(ui, &entries));
        if let Some(item) = chosen
            && let Err(e) = self.run_item(item, ui.ctx(), false)
        {
            self.notice = Some(e);
        }
        // The panel's ≡ menu: a row above its body (the tab strip names the panel).
        let maximized = self.layout.maximized(&self.panels) == Some("history");
        let mut chosen = None;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| chosen = panel_menu_button(ui, maximized, "history"));
        if let Some(item) = chosen
            && let Err(e) = self.run_panel_item("history", item)
        {
            self.notice = Some(e);
        }
        let Some(editor) = self.editor.as_ref() else {
            ui.label(tr("history.no_file"));
            return;
        };
        let (cursor, dropped) = (editor.history.cursor(), editor.history.dropped_total());
        let labels: Vec<String> = editor.history.steps().iter().map(|s| s.label().to_string()).collect();
        let (held, dir) = (editor.history.held_bytes(), editor.history.current().store().dir().to_path_buf());
        if dropped > 0 {
            ui.label(tr_args("history.dropped", &[&dropped.to_string(), &format!("{:.1}", editor.history.cap_disk_bytes() as f64 / 1e9)]));
        }
        let mut go = None;
        let mut delete = None;
        let mut focus = false;
        let refocus = self.history_refocus.take();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                let r = ui.selectable_label(i == cursor, label);
                if refocus == Some(i) {
                    r.request_focus();
                    r.scroll_to_me(None);
                }
                focus |= r.has_focus();
                if r.clicked() && i != cursor {
                    go = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Delete State")).on_hover_text("Removes this state and every later one").clicked() {
                        delete = Some(i);
                        ui.close();
                    }
                });
            }
        });
        self.history_focus = focus || refocus.is_some();
        ui.separator();
        let free = wavcraft_core::space::free_space(&dir).map_or_else(|| tr("history.unknown"), |b| format!("{:.1} GB", b as f64 / 1e9));
        ui.label(tr_args("history.session", &[&format!("{:.1}", held as f64 / 1e6), &free]));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(cursor > 0, egui::Button::new("Delete State"))
                .on_hover_text("Removes the highlighted state and every later one")
                .clicked()
            {
                delete = Some(cursor);
            }
            if ui.add_enabled(labels.len() > 1, egui::Button::new(tr("history.clear"))).clicked() {
                self.confirm_clear_history = true;
            }
        });
        if self.saving.is_none() && self.running.is_none() {
            if let Some(i) = delete {
                self.history_delete(i, ui.ctx());
            } else if let Some(i) = go {
                self.history_go(i, ui.ctx());
            }
        }
    }

    /// Go to History step `i` (a click, an arrow key or the control channel). False if there is none.
    fn history_go(&mut self, i: usize, ctx: &egui::Context) -> bool {
        let Some(editor) = self.editor.as_mut() else { return false };
        if editor.history.go_to(i).is_none() {
            return false;
        }
        self.history_changed(ctx);
        true
    }

    /// Delete History step `i` and the steps after it. The steps removed, or `None`.
    fn history_delete(&mut self, i: usize, ctx: &egui::Context) -> Option<usize> {
        let editor = self.editor.as_mut()?;
        let before = editor.history.current().clone();
        let gone = editor.history.delete_step(i)?;
        if !Arc::ptr_eq(&before, editor.history.current()) {
            self.history_changed(ctx);
        }
        Some(gone)
    }

    /// The current step changed: keep the selection inside it and redraw.
    fn history_changed(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.editor.as_ref() else { return };
        let len = editor.history.current().len_frames() as usize;
        self.selection = Selection { anchor: self.selection.start().min(len), head: self.selection.end().min(len) };
        self.rebuild(ctx);
    }

    /// Up or Down with the History panel focused: move one state. The cursor's new index.
    fn history_arrow(&mut self, down: bool, ctx: &egui::Context) -> Option<usize> {
        let cursor = self.editor.as_ref()?.history.cursor();
        let to = history_arrow_target(cursor, self.editor.as_ref()?.history.steps().len(), down)?;
        self.history_go(to, ctx).then_some(to)
    }

    /// Set metadata fields as ONE undoable step ("Edit Metadata"). Nothing changes (and no step
    /// is made) when every value is already what the file has.
    fn apply_metadata(&mut self, changes: &[(String, String)]) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let current = editor.history.current().clone();
        let mut meta = current.source().meta.clone();
        for (field, value) in changes {
            if meta.get(field).as_deref() != Some(value.as_str()) {
                meta.set(field, value)?;
            }
        }
        if meta == current.source().meta {
            return Ok(false);
        }
        editor.history.push_metadata("Edit Metadata", current.with_metadata(meta));
        self.error = None;
        Ok(true)
    }

    /// Window › Metadata: the Broadcast Wave fields and the INFO title, artist and comment of the
    /// current step. Apply makes one history step.
    fn metadata_panel(&mut self, ui: &mut egui::Ui) {
        self.metadata_panel_body(ui);
    }

    fn save_meta_sets(&self) {
        if let Some(p) = wavcraft_core::meta_view::DisplaySets::default_path()
            && let Err(e) = self.meta_sets.save_file(&p)
        {
            log::warn!(target: "wavcraft", "could not save the metadata sets: {e}");
        }
    }

    fn all_meta_keys() -> Vec<String> {
        meta::Metadata::panel_fields().iter().map(|f| f.to_string()).collect()
    }

    /// `METASET save|load|delete <name>`, `METASET fields <key,key|all>`, `METASET list` (f98).
    fn metaset_command(&mut self, rest: &str) -> String {
        let (verb, name) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
        let r = match verb {
            "save" => self.meta_sets.save(name, &Self::all_meta_keys()),
            "load" => self.meta_sets.load(name),
            "delete" => self.meta_sets.delete(name),
            "fields" => {
                let valid = Self::all_meta_keys();
                let keys: Vec<String> = name.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
                match keys.iter().find(|k| name.trim() != "all" && !valid.contains(k)) {
                    Some(bad) => Err(format!("no panel field {bad:?}")),
                    None => {
                        self.meta_sets.show_only(if name.trim() == "all" { None } else { Some(keys) });
                        Ok(())
                    }
                }
            }
            "list" => {
                let names: Vec<_> = self.meta_sets.sets.keys().map(|n| n.replace(' ', "_")).collect();
                let shown = self.meta_sets.shown.as_ref().map_or_else(|| "all".to_string(), |s| s.join(","));
                return format!(
                    "OK sets={} active={} shown={shown}",
                    names.join(","),
                    self.meta_sets.active.clone().unwrap_or_default().replace(' ', "_")
                );
            }
            _ => Err("METASET save|load|delete <name> | fields <key,key|all> | list".to_string()),
        };
        match r {
            Ok(()) => {
                self.save_meta_sets();
                "OK".to_string()
            }
            Err(e) => format!("ERR {e}"),
        }
    }

    fn metadata_panel_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("Metadata");
        let Some(editor) = self.editor.as_ref() else {
            ui.label("No file open.");
            return;
        };
        let src = editor.history.current().source().clone();
        // The file fields, the XMP fields (f62), then the channel names.
        let mut keys = src.meta.keys();
        let at = keys.iter().position(|k| k.starts_with("chname.")).unwrap_or(keys.len());
        keys.splice(at..at, wavcraft_core::xmp::FIELDS.iter().map(|f| f.name.to_string()));
        if self.meta_for.as_ref().is_none_or(|a| !Arc::ptr_eq(a, &src)) || self.meta_draft.len() != keys.len() {
            self.meta_draft = keys.iter().map(|f| src.meta.get(f).unwrap_or_default()).collect();
            self.meta_for = Some(src.clone());
        }
        let mut apply = false;
        // f98: a filter, and display sets (which fields the panel shows).
        let mut sets_changed = false;
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.add(egui::TextEdit::singleline(&mut self.meta_filter).hint_text("name or value").desired_width(f32::INFINITY));
        });
        let all_keys = Self::all_meta_keys();
        ui.horizontal(|ui| {
            let shown = self.meta_sets.active.clone().unwrap_or_else(|| "All fields".to_string());
            egui::ComboBox::from_id_salt("metaset").selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(false, "All fields").clicked() {
                    self.meta_sets.show_only(None);
                    sets_changed = true;
                }
                for name in self.meta_sets.sets.keys().cloned().collect::<Vec<_>>() {
                    if ui.selectable_label(false, &name).clicked() {
                        let _ = self.meta_sets.load(&name);
                        sets_changed = true;
                    }
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.meta_set_name).hint_text("set name").desired_width(80.0));
            if ui.button("Save").clicked() && self.meta_sets.save(&self.meta_set_name, &all_keys).is_ok() {
                sets_changed = true;
            }
            if ui.button("Delete").clicked() && self.meta_sets.delete(&self.meta_set_name).is_ok() {
                sets_changed = true;
            }
        });
        ui.collapsing("Fields shown", |ui| {
            for key in &all_keys {
                let mut on = self.meta_sets.is_shown(key);
                if ui.checkbox(&mut on, key.replace('_', " ")).changed() {
                    let mut now: Vec<String> = all_keys.iter().filter(|k| self.meta_sets.is_shown(k)).cloned().collect();
                    now.retain(|k| k != key);
                    if on {
                        now.push(key.clone());
                    }
                    self.meta_sets.show_only(Some(now));
                    sets_changed = true;
                }
            }
        });
        if sets_changed {
            self.save_meta_sets();
        }
        let needle = self.meta_filter.trim().to_lowercase();
        let visible: Vec<bool> = {
            let rows = wavcraft_core::meta_view::rows(&src.meta);
            keys.iter()
                .zip(self.meta_draft.iter())
                .map(|(f, draft)| {
                    let label = rows.iter().find(|r| r.key == *f).map(|r| r.label.to_lowercase()).unwrap_or_default();
                    // Channel names are not in the display sets: they always show.
                    (f.starts_with("chname.") || self.meta_sets.is_shown(f))
                        && (needle.is_empty()
                            || f.to_lowercase().contains(&needle)
                            || label.contains(&needle)
                            || draft.to_lowercase().contains(&needle))
                })
                .collect()
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            for ((field, text), show) in keys.iter().zip(self.meta_draft.iter_mut()).zip(visible.iter()) {
                if !*show {
                    continue;
                }
                match (field.strip_prefix("chname."), wavcraft_core::xmp::FIELDS.iter().find(|f| f.name == field.as_str())) {
                    (Some(n), _) => ui.label(format!("channel {n} name")),
                    (None, Some(x)) => ui.label(x.label),
                    (None, None) => ui.label(field.replace('_', " ")),
                };
                if *field == "coding_history" {
                    ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY));
                } else {
                    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                }
            }
            if let Some(why) = &src.meta.xmp_problem {
                ui.add_space(4.0);
                ui.colored_label(
                    Color32::LIGHT_RED,
                    format!("The XMP packet could not be read ({why}). It is kept as it is and cannot be edited here."),
                );
            } else if let Some(x) = &src.meta.xmp {
                let unknown = x.properties().iter().filter(|p| !p.known).count();
                if unknown > 0 {
                    ui.add_space(4.0);
                    ui.label(format!("{unknown} other XMP properties are kept as they are."));
                }
            }
            if !needle.is_empty() {
                let others: Vec<_> = wavcraft_core::meta_view::filter_rows(wavcraft_core::meta_view::rows(&src.meta), &needle)
                    .into_iter()
                    .filter(|r| r.key.starts_with("xmp:"))
                    .collect();
                for r in others {
                    ui.label(format!("{} = {}", r.label, r.value));
                }
            }
            ui.add_space(6.0);
            apply = ui.button("Apply").clicked();
        });
        if apply {
            let changes: Vec<(String, String)> = keys.iter().cloned().zip(self.meta_draft.iter().cloned()).collect();
            match self.apply_metadata(&changes) {
                Ok(_) => self.meta_for = None,
                Err(e) => self.error = Some(format!("Metadata: {e}")),
            }
        }
    }

    /// Draw the effect dialog, if up, and carry out its answer.
    fn effect_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.effect_dialog.as_mut() else { return };
        match dialog.show(ctx) {
            Some(Answer::Preview) => {
                if let Err(why) = self.start_preview() {
                    self.error = Some(why);
                }
            }
            Some(answer) => {
                let _ = self.finish_effect_dialog(answer == Answer::Ok, ctx);
            }
            None => {}
        }
        // The preview loops while the dialog is up.
        if self.previewing && !self.player.is_playing() {
            let _ = self.start_preview();
        }
        if self.previewing {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Play a short loop of the open Generate dialog's sound through the player (f21). The file
    /// is put back by [`WaveApp::end_preview`] when the dialog closes. Returns the preview's
    /// length in frames.
    fn start_preview(&mut self) -> Result<usize, String> {
        let Some(dialog) = self.effect_dialog.as_ref().filter(|d| d.generator) else { return Err("no generator dialog is open".to_string()) };
        let Some(doc) = self.doc.as_ref() else { return Err("no file is open".to_string()) };
        let g = wavcraft_core::generate::all()[usize::from(dialog.index)];
        wavcraft_core::generate::validate(g, &dialog.values)?;
        let rate = doc.sample_rate();
        let secs = g.duration(&dialog.values).min(PREVIEW_SECONDS);
        let frames = (secs * f64::from(rate)).round() as usize;
        let channels = wavcraft_core::generate::render(g, &dialog.values, rate, doc.channels().max(1), frames, PREVIEW_SEED);
        let preview = wavcraft_core::AudioBuffer { sample_rate: rate, channels };
        let state = wavcraft_core::Document::from_buffer(preview).map_err(|e| e.to_string())?.state;
        self.set_audio(Some(state));
        self.player.play(0, frames);
        self.previewing = true;
        Ok(frames)
    }

    /// Stop a preview and give the player the open file back.
    fn end_preview(&mut self) {
        if self.previewing {
            self.previewing = false;
            self.set_audio(self.doc.as_ref().map(|d| d.state.clone()));
        }
    }

    /// Generate › Tones / Noise: insert at the cursor, or replace the selection (f21).
    fn run_generator(&mut self, index: u16, params: Vec<f64>, seed: u64, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let name = wavcraft_core::generate::all().get(usize::from(index)).map_or("Generate", |g| g.name());
        match editor.generate(index, &params, sel, seed) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{name}: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// The Generate menu: one entry per generator, each opening its dialog.
    fn generate_menu(&mut self, ui: &mut egui::Ui) {
        let enabled = self.editor.is_some() && self.saving.is_none() && self.running.is_none();
        for (i, g) in wavcraft_core::generate::all().iter().enumerate() {
            if ui.add_enabled(enabled, egui::Button::new(format!("{}…", g.name().trim_start_matches("Generate ")))).clicked() {
                self.effect_dialog = EffectDialog::new_generator(i as u16, None);
                ui.close();
            }
        }
    }

    /// Close the effect dialog; on OK apply the effect with the dialog's values. Returns whether
    /// the document changed.
    fn finish_effect_dialog(&mut self, ok: bool, ctx: &egui::Context) -> Result<bool, String> {
        let Some(dialog) = self.effect_dialog.take() else { return Err("no effect dialog is open".to_string()) };
        self.end_preview();
        if let Some(slot) = dialog.rack_slot {
            // Opened from the rack: keep the settings in the slot, run nothing.
            if ok {
                self.rack.set_slot_params(slot, dialog.index, dialog.values);
            }
            return Ok(false);
        }
        if !ok {
            return Ok(false);
        }
        if dialog.generator {
            return self.run_generator(dialog.index, dialog.values, clock_seed(), ctx);
        }
        self.run_effect(dialog.index, dialog.values, ctx)
    }

    /// Run the effect at `index` with `params` on the selection (the whole file when none).
    fn run_effect(&mut self, index: u16, params: Vec<f64>, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let name = wavcraft_core::effects::all().get(usize::from(index)).map_or("Effect", |e| e.name());
        if !editor.enabled(ActionId::Effect(index), &sel) {
            return Err(format!("{name} is not available now"));
        }
        match editor.run_effect(index, params, sel) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{name}: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Process the selection (the whole file when none) through the Effects Rack as one step,
    /// then reset the rack to bypass (f31). Returns whether the document changed.
    fn apply_rack(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        if !self.rack.config.is_active() {
            return Err("the Effects Rack has nothing switched on".to_string());
        }
        match editor.run_rack(&self.rack.config, sel) {
            Ok(out) => {
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rack.config.bypass();
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("Effects Rack: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Draw the Effects Rack window, carry out what it asked for, and keep the player's copy of
    /// the rack current.
    fn rack_ui(&mut self, ctx: &egui::Context) {
        let can_apply = self.editor.is_some() && self.saving.is_none() && self.running.is_none();
        self.rack.show(ctx, self.player.is_playing(), can_apply);
        for request in std::mem::take(&mut self.rack.requests) {
            match request {
                Request::EditSlot(i) => self.effect_dialog = self.rack.dialog_for(i),
                Request::TogglePlay => self.toggle_play(),
                Request::Apply => {
                    let _ = self.apply_rack(ctx);
                }
            }
        }
        self.sync_rack();
    }

    /// Hand the player the rack when it differs from the one it has.
    fn sync_rack(&mut self) {
        if self.rack.pushed.as_ref() != Some(&self.rack.config) {
            match self.player.set_rack(&self.rack.config) {
                Ok(()) => self.rack.pushed = Some(self.rack.config.clone()),
                Err(why) => {
                    log::warn!(target: "wavcraft", "Effects Rack: {why}");
                    self.error = Some(format!("Effects Rack: {why}"));
                    self.rack.pushed = Some(self.rack.config.clone());
                }
            }
        }
    }

    /// Run DeClipper with `[threshold_db, min_run]` on the selection (the whole file when none).
    fn run_declip(&mut self, params: Vec<f64>, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        match editor.run_declip(params, sel) {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("DeClipper: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Run DeClicker (`[sensitivity, max_width_ms]`) or Automatic Click Remover
    /// (`[threshold, complexity]`) on the selection (the whole file when none).
    fn run_declick(&mut self, params: Vec<f64>, auto: bool, ctx: &egui::Context) -> Result<bool, String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let sel = self.selection_range();
        let name = if auto { "Automatic Click Remover" } else { "DeClicker" };
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let result = if auto { editor.run_auto_click(params, sel) } else { editor.run_declick(params, sel) };
        match result {
            Ok(out) => {
                self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                self.error = None;
                self.notice = out.message;
                if out.doc_changed {
                    self.rebuild(ctx);
                }
                Ok(out.doc_changed)
            }
            Err(e) => {
                let why = format!("{name}: {e}");
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Scan for the Diagnostics window with `tool` on the selection (the whole file when none).
    fn diag_scan(&mut self, tool: wavcraft_core::diagnostics::Tool, ctx: &egui::Context) -> Result<(), String> {
        let sel = self.selection_range();
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let doc = editor.history.current().clone();
        let channels = editor.channels.within(doc.channel_count());
        self.diag.set_tool(tool);
        self.diag.start_scan(tool, doc, sel, channels, ctx);
        Ok(())
    }

    /// Repair `rows` of the Diagnostics list as one undo step; returns (repaired, skipped).
    fn diag_repair(&mut self, rows: &[usize], ctx: &egui::Context) -> Result<(usize, usize), String> {
        if self.saving.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let (tool, doc, regions) = self.diag.plan(rows, editor.history.current())?;
        let cancel = AtomicBool::new(false);
        match tool.repair(&doc, &regions, editor.crossfades, &cancel) {
            Ok(r) => {
                let changed = !r.edit.is_noop();
                editor.history.push_edit(r.edit);
                self.error = None;
                self.notice = changed.then(|| tool.label().to_string());
                self.diag.say(format!("{}: {} repaired, {} skipped. The list is stale; scan again.", tool.label(), r.repaired, r.skipped));
                if changed {
                    self.rebuild(ctx);
                }
                Ok((r.repaired, r.skipped))
            }
            Err(e) => {
                let why = format!("{}: {e}", tool.label());
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why.clone());
                Err(why)
            }
        }
    }

    fn diagnostics_ui(&mut self, ctx: &egui::Context) {
        self.diag.poll();
        let busy = self.saving.is_some() || self.rebuilding.is_some();
        let current = self.editor.as_ref().map(|e| e.history.current().clone());
        match self.diag.ui(ctx, current.as_ref(), busy) {
            Some(Ask::Scan) => {
                let tool = self.diag.tool();
                let _ = self.diag_scan(tool, ctx);
            }
            Some(Ask::Cancel) => self.diag.cancel(),
            Some(Ask::Goto(r)) => {
                self.selection = Selection { anchor: r.start as usize, head: r.end() as usize };
                self.halt_play();
                let frames = self.doc.as_ref().map_or(0, Document::frames);
                self.view.reveal(r.start as f64, self.editor_width, frames);
            }
            Some(Ask::Repair(rows)) => {
                let _ = self.diag_repair(&rows, ctx);
            }
            None => {}
        }
    }

    fn markers_panel(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = self.editor.as_mut() else {
            ui.heading("Markers");
            ui.label("No file open.");
            return;
        };
        let out = self.markers_ui.panel(ui, editor, &mut self.settings);
        if let Some(span) = out.select {
            self.selection = Selection { anchor: span.start as usize, head: span.end as usize };
            self.halt_play();
            let frames = self.doc.as_ref().map_or(0, Document::frames);
            self.view.reveal(span.start as f64, self.editor_width, frames);
        }
        if out.columns_changed
            && let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            log::warn!(target: "wavcraft", "could not save the settings: {e}");
        }
    }

    fn playlist_panel(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = self.editor.as_ref() else { return };
        let doc = editor.history.current();
        let (markers, rate) = (doc.markers().clone(), doc.sample_rate());
        let out = self.playlist_ui.panel(ui, &markers, rate);
        if out.stop {
            self.playlist_ui.cancel();
            self.halt_play();
        }
        if out.play
            && let Err(e) = self.play_playlist()
        {
            self.notice = Some(e);
        }
    }

    /// Window › Properties (a registry panel: docked on the right by default, floats, hides from
    /// the Window menu like the others): the open file's path, format, rate, channels, depth,
    /// duration, size.
    fn properties_panel(&mut self, ui: &mut egui::Ui) {
        let rows = self.doc.as_ref().map(Document::properties);
        match &rows {
            Some(rows) => {
                egui::Grid::new("properties").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (label, value) in rows {
                        ui.label(*label);
                        ui.add(egui::Label::new(value).wrap().selectable(true));
                        ui.end_row();
                    }
                });
            }
            None => {
                ui.label("No file is open.");
            }
        }
    }

    fn clear_history_prompt(&mut self, ctx: &egui::Context) {
        if !self.confirm_clear_history {
            return;
        }
        let mut choice = None;
        egui::Window::new(tr("dialog.clear_history")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.label(tr("dialog.clear_history.body"));
                ui.horizontal(|ui| {
                    if ui.button(tr("button.clear")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr("button.cancel")).clicked() {
                        choice = Some(false);
                    }
                });
            },
        );
        match choice {
            Some(true) => {
                let idle = self.running.is_none();
                if let Some(e) = self.editor.as_mut().filter(|_| idle) {
                    e.history.clear();
                }
                self.confirm_clear_history = false;
            }
            Some(false) => self.confirm_clear_history = false,
            None => {}
        }
    }

    /// Show a failure from a menu or click (a control request gets it as its answer instead).
    fn report(&mut self, r: Result<(), String>) {
        if let Err(e) = r {
            self.error = Some(e);
        }
    }

    /// `CLIP edit <track> <i>` from the control channel.
    fn control_edit_clip(&mut self, rest: &str, ctx: &egui::Context) -> String {
        let w = match crate::multitrack_ui::tokens(rest) {
            Ok(w) if w.len() == 3 => w,
            _ => return "ERR usage: CLIP edit <track> <clip number, from 1>".to_string(),
        };
        let r = self.mtv.find_track(&w[1]).map_err(|e| e.0).and_then(|t| {
            let i = w[2].parse::<usize>().map_err(|_| "<clip number> is a number from 1".to_string())?;
            self.edit_clip_source(t, i, ctx)
        });
        match r {
            Ok(()) => "OK".to_string(),
            Err(e) => format!("ERR {e}"),
        }
    }

    /// Multitrack › a clip › Edit in Waveform editor: write the clip's source to a float WAV in a
    /// private temp folder, open it in the Waveform editor (which has one document at a time, so
    /// it replaces the current view) and remember the way back. The open document must have no
    /// unsaved changes; it is opened again on return (its undo history is not kept).
    fn edit_clip_source(&mut self, track: TrackId, i: usize, ctx: &egui::Context) -> Result<(), String> {
        if self.clip_edit.is_some() {
            return Err("a clip source is already open: Return to session first".into());
        }
        if self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some() {
            return Err("busy: wait for the open, edit or save to finish".into());
        }
        if self.doc.is_some() && (self.unsaved() || self.doc_path.as_os_str().is_empty()) {
            return Err("save the open document first: opening the clip source replaces it".into());
        }
        let (source, name, wav) = self.mtv.clip_source_wav(track, i).map_err(|e| e.0)?;
        let dir = std::env::temp_dir().join(format!("wavcraft-clip-source-{}-{}", std::process::id(), self.doc_id));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make a folder for the source: {e}"))?;
        let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        let path = dir.join(format!("{}.wav", stem.trim()));
        if let Err(e) = std::fs::write(&path, wav) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("cannot write the source: {e}"));
        }
        let back_to = self.doc.is_some().then(|| self.doc_path.clone());
        self.clip_edit = Some(ClipEdit { source, name, path: path.clone(), dir, back_to });
        self.mtv.show = false;
        self.open(path, ctx);
        Ok(())
    }

    /// Return to session: the edited audio becomes the source (clips that use it keep their
    /// start and are trimmed to the new length, see `Multitrack::replace_source_audio`), the
    /// Multitrack view is shown and the document that was open before comes back.
    fn return_to_session(&mut self, ctx: &egui::Context) -> Result<String, String> {
        let Some(edit) = self.clip_edit.take() else { return Err("no clip source is open: CLIP edit <track> <i> first".into()) };
        let busy = self.opening.is_some() || self.rebuilding.is_some() || self.saving.is_some();
        let ready = !busy && self.doc.is_some() && self.doc_path == edit.path;
        if !ready {
            let why = if busy {
                "busy: wait for the open or edit to finish"
            } else {
                "the clip source is not open (the open failed or another file was opened)"
            };
            if busy {
                self.clip_edit = Some(edit);
            } else {
                let _ = std::fs::remove_dir_all(&edit.dir);
                self.mtv.show = true;
            }
            return Err(why.into());
        }
        let Some(doc) = &self.doc else { return Err("no document".into()) };
        let audio = wavcraft_core::multitrack::ModelBuffer {
            sample_rate: doc.sample_rate(),
            channels: doc.audio_buffer().map_err(|e| e.to_string())?.channels,
        };
        let change = match self.mtv.replace_source(edit.source, audio) {
            Ok(c) => c,
            Err(e) => {
                self.clip_edit = Some(edit);
                return Err(e.0);
            }
        };
        let _ = std::fs::remove_dir_all(&edit.dir);
        self.mtv.show = true;
        self.halt_play();
        // A session playing now holds the old audio (its snapshot), so it stops, as after any change to the session.
        self.mtv.transport.stop();
        match edit.back_to {
            Some(p) => self.open(p, ctx),
            None => {
                self.set_audio(None);
                self.doc = None;
                self.editor = None;
                self.saved = None;
                self.doc_path = PathBuf::new();
                self.generation += 1;
                self.spectrum = None;
            }
        }
        Ok(format!("OK frames={} was={} trimmed={} removed={}", change.new_frames, change.old_frames, change.trimmed, change.removed))
    }

    /// Show the current step: build its peaks (reading the store a chunk at a time) on a worker
    /// thread. No samples are copied; the views and the player read the step from the store.
    fn rebuild(&mut self, ctx: &egui::Context) {
        self.matcher.audio_changed();
        let Some(editor) = &self.editor else { return };
        let (tx, rx) = channel();
        let (state, path, ctx) = (editor.history.current().as_ref().clone(), self.doc_path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Document::from_state(path, state));
            ctx.request_repaint();
        });
        // Playback goes on with the old audio until the edited one is ready, and then moves on
        // with it from the same place in the music (f48: edit while playing; see `finish_rebuild`).
        self.playlist_ui.cancel();
        self.rebuilding = Some(rx);
    }

    /// Start a background edit: `producer` fills the output for `range` on a worker thread
    /// (see `wavcraft_core::jobs`). The undo step is pushed by `finish_job` on this thread.
    fn start_job<F>(&mut self, label: &str, range: Range<u64>, ctx: &egui::Context, producer: F) -> Result<(), String>
    where
        F: FnMut(u64, &[&[f32]], &mut [&mut [f32]]) -> std::io::Result<()> + Send + 'static,
    {
        if self.running.is_some() {
            return Err("a background job is already running".to_string());
        }
        if self.saving.is_some() || self.rebuilding.is_some() {
            return Err("wait for the save to finish".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let base = editor.history.current().clone();
        let range = range.start.min(base.len_frames())..range.end.min(base.len_frames());
        let wake = {
            let ctx = ctx.clone();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
            wake
        };
        let channels = editor.channels.within(base.channel_count());
        let job = jobs::Job::spawn_replace(base, range, channels, label, editor.crossfades, Some(wake), producer);
        self.running = Some(Running { job });
        self.halt_play();
        self.error = None;
        self.notice = None;
        Ok(())
    }

    /// Convert Sample Type (A161) as a background job (see `start_job`): the result becomes one
    /// History step when it finishes, and `finish_job` keeps the selection's place in time.
    fn start_convert(&mut self, settings: &ConvertSettings, ctx: &egui::Context) -> Result<(), String> {
        if self.running.is_some() {
            return Err("a background job is already running".to_string());
        }
        if self.saving.is_some() || self.rebuilding.is_some() || self.opening.is_some() {
            return Err("wait for the current job to finish".to_string());
        }
        if let Some(why) = self.edit_lock() {
            return Err(why.to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let base = editor.history.current().clone();
        editor.history.can_afford(convert::estimate_bytes(&base, settings)).map_err(|e| format!("Convert Sample Type: {e}"))?;
        let wake = {
            let ctx = ctx.clone();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
            wake
        };
        log::info!(target: "wavcraft", "convert sample type: {}", settings.to_text());
        let settings = settings.clone();
        let job = jobs::Job::spawn_with(base, "Convert Sample Type", Some(wake), move |doc, cancel, progress| {
            convert::convert(doc, &settings, cancel, progress)
        });
        self.running = Some(Running { job });
        self.halt_play();
        self.error = None;
        self.notice = None;
        Ok(())
    }

    /// Interpret Sample Rate (A162): relabel the rate, one History step, no sample touched (the
    /// selection keeps its frames).
    fn run_interpret(&mut self, rate: u32, ctx: &egui::Context) -> Result<(), String> {
        if self.running.is_some() || self.saving.is_some() {
            return Err("wait for the current job to finish".to_string());
        }
        if let Some(why) = self.edit_lock() {
            return Err(why.to_string());
        }
        let Some(editor) = self.editor.as_mut() else { return Err("no file is open".to_string()) };
        let edit = convert::interpret_rate(editor.history.current(), rate).map_err(|e| format!("Interpret Sample Rate: {e}"))?;
        if edit.is_noop() {
            return Ok(());
        }
        editor.history.push_edit(edit);
        self.notice = Some(format!("Interpreting the file as {rate} Hz"));
        self.error = None;
        self.fit_on_next_frame = true;
        self.rebuild(ctx);
        Ok(())
    }

    /// The two conversion dialogs (A161, A162), when up.
    fn convert_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(d) = self.convert_dialog.as_mut() {
            match d.show(ctx, &mut self.convert_presets) {
                convert_ui::Outcome::Open => {}
                convert_ui::Outcome::Cancel => self.convert_dialog = None,
                convert_ui::Outcome::Apply(s) => {
                    self.convert_dialog = None;
                    if let Err(e) = self.start_convert(&s, ctx) {
                        self.error = Some(e);
                    }
                }
            }
        }
        if let Some(text) = self.interpret.as_mut() {
            let current = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
            if let Some(answer) = convert_ui::interpret_dialog(ctx, text, current) {
                self.interpret = None;
                if let Some(text) = answer {
                    match text.trim().parse::<u32>() {
                        Ok(rate) => {
                            if let Err(e) = self.run_interpret(rate, ctx) {
                                self.error = Some(e);
                            }
                        }
                        Err(_) => self.error = Some("Interpret Sample Rate: type a whole number of Hz".to_string()),
                    }
                }
            }
        }
    }

    /// A Match Loudness run from the panel finished (f33): one step on the history, unless the
    /// audio changed while it ran.
    fn finish_match(&mut self, ctx: &egui::Context) {
        self.matcher.tick();
        let Some(done) = self.matcher.take_finished() else { return };
        let Some(editor) = self.editor.as_mut() else { return };
        if !Arc::ptr_eq(&done.base, editor.history.current()) {
            self.matcher.say(false, "The audio changed while matching; nothing was applied. Run it again.".to_string());
            return;
        }
        let changed = !done.result.edit.is_noop();
        editor.history.push_edit(done.result.edit);
        self.matcher.say(true, done.result.message.clone());
        self.notice = Some(done.result.message);
        if changed {
            self.rebuild(ctx);
        }
    }

    /// The Match Loudness panel's body (f33).
    fn match_panel(&mut self, ui: &mut egui::Ui) {
        let file = self.editor.as_ref().map(|e| {
            let name = self.doc_path.file_name().map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned());
            (name, e.history.current().clone())
        });
        self.matcher.panel(ui, file);
    }

    fn start_test_job(&mut self, ms: u64, ctx: &egui::Context) -> Result<(), String> {
        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames());
        let range = if self.selection.is_empty() { 0..len } else { self.selection_range() };
        self.start_job("Test Job", range, ctx, move |_, input, output| {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            for (i, o) in input.iter().zip(output.iter_mut()) {
                o.copy_from_slice(i);
            }
            Ok(())
        })
    }

    /// Take a finished job's result: push its undo step, or say why there is none.
    fn finish_job(&mut self, ctx: &egui::Context) {
        let Some(running) = &mut self.running else { return };
        let result = match running.job.poll() {
            JobPoll::Running => return,
            JobPoll::Done(r) => r,
        };
        let Some(Running { job }) = self.running.take() else { return };
        match result {
            Ok(edit) => {
                let Some(editor) = self.editor.as_mut() else { return };
                if !Arc::ptr_eq(editor.history.current(), job.base()) {
                    log::warn!(target: "wavcraft", "{}: the document changed while the job ran; its result was dropped", job.label());
                    self.notice = Some(format!("{} was dropped: the document changed meanwhile", job.label()));
                    return;
                }
                let label = edit.label.clone();
                let changed = !edit.is_noop();
                // A conversion (A161) changes the rate: the selection keeps its place in time.
                let (old_rate, new_rate, new_len) = (job.base().sample_rate(), edit.state.sample_rate(), edit.state.len_frames() as usize);
                editor.history.push_edit(edit);
                self.notice = Some(label);
                if changed && old_rate != new_rate {
                    let scale = |f: usize| (wavcraft_core::doc::rescale_frame(f as u64, old_rate, new_rate) as usize).min(new_len);
                    self.selection = Selection { anchor: scale(self.selection.anchor), head: scale(self.selection.head) };
                    self.fit_on_next_frame = true;
                }
                if changed {
                    self.rebuild(ctx);
                }
            }
            Err(wavcraft_core::EditError::Cancelled) => self.notice = Some(format!("{} cancelled", job.label())),
            Err(e) => {
                let why = format!("{}: {e}", job.label());
                log::warn!(target: "wavcraft", "{why}");
                self.error = Some(why);
            }
        }
    }

    fn finish_rebuild(&mut self) {
        let Some(rx) = &self.rebuilding else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(OpenError::Io("the worker stopped unexpectedly".to_string())),
        };
        self.rebuilding = None;
        match result {
            Ok(doc) => {
                // Edit while playing (f48): the player takes the edited audio and the play goes on
                // from the same place in the music, moved across the edit. A Generate preview holds
                // the player with its own audio, so it keeps the plain hand-over.
                match self.doc.as_ref().filter(|_| !self.previewing) {
                    Some(old) => self.player.replace_audio(Some(doc.state.clone()), &EditSpan::between(&old.state, &doc.state)),
                    None => self.set_audio(Some(doc.state.clone())),
                }
                self.doc = Some(doc);
                self.generation += 1;
                self.spectrum = None;
            }
            Err(OpenError::Empty) => {
                self.set_audio(None);
                self.doc = None;
                self.notice = Some("The file is empty now (Edit › Undo brings the audio back)".to_string());
            }
            Err(e) => {
                log::warn!(target: "wavcraft", "could not show the edited document: {e}");
                self.error = Some(format!("Could not show the edited document: {e}"));
            }
        }
    }

    /// Whether the current step differs from the one on disk.
    fn unsaved(&self) -> bool {
        is_unsaved(&self.editor, &self.saved)
    }

    /// Whether any open document has unsaved changes.
    fn any_unsaved(&self) -> bool {
        self.unsaved() || self.docs.parked().iter().any(|s| is_unsaved(&s.editor, &s.saved))
    }

    /// File › Save: to the file's own path when it is a format a save writes (WAV, AIFF, FLAC),
    /// else as Save As. `to` skips the dialog (the control channel); the format follows its
    /// extension.
    fn save(&mut self, ctx: &egui::Context, save_as: bool, to: Option<PathBuf>, then_close: bool) -> Result<(), String> {
        self.save_with(ctx, save_as, to, then_close, self.save_type, false)
    }

    /// [`WaveApp::save`] with the sample type given (`explicit`: asked for by name, so a format
    /// that can't hold it refuses instead of using the nearest type).
    fn save_with(
        &mut self,
        ctx: &egui::Context,
        save_as: bool,
        to: Option<PathBuf>,
        then_close: bool,
        ty: Option<SampleType>,
        explicit: bool,
    ) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        if self.running.is_some() {
            return Err("wait for the background job to finish, or cancel it".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let own_format = SaveFormat::from_path(&self.doc_path).filter(|f| SaveFormat::from_file_format(state.source().format) == Some(*f));
        let path = match to {
            Some(p) => p,
            None if own_format.is_some() && !save_as => self.doc_path.clone(),
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let first = self.dialog_format.take().or(own_format).unwrap_or(SaveFormat::Wav);
                let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.{}", first.extension()));
                for f in [first, SaveFormat::Wav, SaveFormat::Aiff, SaveFormat::Flac, SaveFormat::Vorbis].iter().fold(Vec::new(), |mut v, f| {
                    if !v.contains(f) {
                        v.push(*f);
                    }
                    v
                }) {
                    let exts: &[&str] = if f == SaveFormat::Aiff { &["aiff", "aif"] } else { &[f.extension()] };
                    dialog = dialog.add_filter(f.name(), exts);
                }
                dialog = dialog.add_filter("Sony Wave64", &["w64"]).add_filter("Apple CAF", &["caf"]).add_filter("Sun/NeXT AU", &["au"]);
                if let Some(dir) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    dialog = dialog.set_directory(dir);
                }
                let picked = match self.scripted_pick.take() {
                    Some(answer) => answer,
                    None => dialog.save_file(),
                };
                match picked {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
        };
        // The extension picks the format; a name without one is a WAV.
        // Wave64, CAF and AU (f91) are a container of their own: `format` is WAV there and unused.
        let (path, format, container) = if let Some(f) = SaveFormat::from_path(&path) {
            (path, f, None)
        } else if let Some(c) = Container::from_path(&path).filter(|c| *c != Container::Wav) {
            (path, SaveFormat::Wav, Some(c))
        } else if path.extension().is_none() {
            (path.with_extension("wav"), SaveFormat::Wav, None)
        } else {
            return Err(format!("{} is not a format WavCraft saves: use .wav, .w64, .caf, .au, .aiff, .flac or .ogg", path.display()));
        };
        let wanted = ty.unwrap_or_else(|| SampleType::for_source(state.source()));
        if !format.accepts(wanted) && (explicit || ty.is_some()) {
            return Err(format!("{} cannot hold {} samples: use 16 or 24 bit", format.name(), wanted.describe()));
        }
        let ty = format.fit(wanted);
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let aiff = self.aiff_kind;
        let quality = self.vorbis_quality;
        // Cancel and the metadata switch (A262, A263) are for a WAV save of the whole file.
        let cancel = (format == SaveFormat::Wav && container.is_none()).then(|| Arc::new(AtomicBool::new(false)));
        let job_cancel = cancel.clone();
        let (include_metadata, extra_chunks, block_delay) = (self.include_meta, metadata_chunks(), test_block_delay());
        let (job_state, job_path, job_progress, ctx) = (state.clone(), path.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let done = match container {
                Some(c) => save_container(&job_state, &job_path, c, ty, &mut report),
                None if format == SaveFormat::Vorbis => save_vorbis(&job_state, &job_path, quality, &mut report),
                None if format == SaveFormat::Wav => {
                    let opts = SaveOptions { include_metadata, extra_chunks, cancel: job_cancel.as_deref(), block_delay };
                    save_wav_with(&job_state, &job_path, ty, &opts, &mut report)
                }
                None => save_aiff_kind(&job_state, &job_path, format, ty, aiff, &mut report),
            };
            let _ = tx.send(done);
            ctx.request_repaint();
        });
        self.halt_play();
        self.saving = Some(Saving {
            path,
            state,
            ty,
            format,
            aiff,
            container,
            result: rx,
            progress,
            cancel,
            then_close,
            doc_id: self.doc_id,
            copy: None,
            started: Instant::now(),
        });
        Ok(())
    }

    /// File › Save Selection As (`selection`: only the selected range of the enabled channels) and
    /// File › Export › File (the whole document): a new WAV beside whatever else, written by the
    /// save path. The open document keeps its own path, name and unsaved mark. `to` skips the
    /// dialog (the control channel).
    fn save_copy(&mut self, ctx: &egui::Context, selection: bool, to: Option<PathBuf>) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a save is already running".to_string());
        }
        let Some(editor) = &self.editor else { return Err("no file is open".to_string()) };
        let state = editor.history.current().clone();
        let part = if selection {
            let r = self.selection_range();
            if r.start >= r.end {
                return Err("select some audio first".to_string());
            }
            Part::range(&state, r, editor.channels)
        } else {
            Part::whole(&state)
        };
        let path = match to {
            Some(p) => p,
            None => {
                let stem = self.doc_path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
                let suffix = if selection { "selection" } else { "export" };
                let mut dialog = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(format!("{stem}-{suffix}.wav"));
                if let Some(dir) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(p) => p,
                    None => return Ok(()),
                }
            }
        };
        let ty = SampleType::for_source(state.source());
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicU32::new(0));
        let (job_state, job_path, job_progress, ctx) = (state.clone(), path.clone(), progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut report = |p: f64| {
                job_progress.store((p.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let _ = tx.send(write_file(&job_state, &job_path, FileKind::Wav(ty), &part, &mut report));
            ctx.request_repaint();
        });
        self.halt_play();
        let copy = Some(if selection { "selection" } else { "export" });
        self.saving = Some(Saving {
            path,
            state,
            ty,
            format: SaveFormat::Wav,
            aiff: AiffKind::Aiff,
            container: None,
            result: rx,
            progress,
            cancel: None,
            then_close: false,
            doc_id: self.doc_id,
            copy,
            started: Instant::now(),
        });
        Ok(())
    }

    /// Set "Include markers and other metadata" for the open document, and keep it as the default
    /// for the files opened next (A262).
    fn set_include_meta(&mut self, on: bool) {
        self.include_meta = on;
        self.settings.save_include_metadata = on;
        if let Err(e) = self.save_settings() {
            log::warn!(target: "wavcraft", "could not keep the save option: {e}");
        }
    }

    fn finish_saving(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.saving else { return };
        let result = match saving.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(SaveError::Io(std::io::Error::other("the save stopped unexpectedly"))),
        };
        let Some(Saving { path, state, ty, format, aiff, container, then_close, doc_id, copy, started, .. }) = self.saving.take() else { return };
        if result.is_ok() {
            self.last_op = Some(("Save".to_string(), started.elapsed()));
        }
        let label = container.map_or_else(|| format_label(format, aiff).to_string(), |c| c.describe().to_string());
        if let Some(what) = copy {
            match result {
                Ok(()) => {
                    self.error = None;
                    self.notice =
                        Some(format!("Wrote the {what} to {} ({}); the open file is unchanged", path.display(), FileKind::Wav(ty).describe()));
                }
                Err(e) => self.error = Some(format!("Not written: {e}. {} is unchanged.", path.display())),
            }
            return;
        }
        match result {
            // Another file was opened while this one was being saved: the file on disk is
            // written, but the open document is not the one saved, so its path, saved mark and
            // close request stay as they are.
            Ok(()) if doc_id != self.doc_id => {
                if let Some(slot) = self.docs.parked_mut().iter_mut().find(|s| s.doc_id == doc_id) {
                    slot.saved = Some(state);
                    slot.doc_path = path.clone();
                    if let Some(doc) = slot.doc.as_mut() {
                        doc.path = path.clone();
                    }
                    self.notice = Some(format!("Saved {} ({} {})", path.display(), ty.describe(), label));
                } else {
                    log::info!(target: "wavcraft", "a save of a closed file finished: {}", path.display());
                    self.notice = Some(format!("Saved {} ({} {}), a file closed meanwhile", path.display(), ty.describe(), label));
                }
            }
            Ok(()) => {
                self.saved = Some(state);
                self.doc_path = path.clone();
                // The tempo follows the document to its new path; it is written only when there is one to keep.
                if self.tempo != Tempo::default() || self.settings.tempos.contains_key(path.to_string_lossy().as_ref()) {
                    self.set_tempo(self.tempo);
                }
                if let Some(doc) = self.doc.as_mut() {
                    doc.path = path.clone();
                }
                self.error = None;
                self.notice = Some(format!("Saved {} ({} {})", path.display(), ty.describe(), label));
                if then_close && std::mem::take(&mut self.closing_doc) {
                    self.remove_current();
                } else if then_close {
                    // Not allow_close: another document may still have unsaved changes.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(SaveError::Cancelled) => {
                self.error = None;
                self.notice = Some(format!("Save cancelled. {} is unchanged and the file is still unsaved.", path.display()));
            }
            Err(e) => {
                self.close_all = false;
                self.closing_doc = false;
                self.error = Some(format!("Not saved: {e}. {} is unchanged.", path.display()));
            }
        }
    }

    /// Save As…: the export dialog first (format, sample type, AIFF kind, estimated size,
    /// presets); its button then runs the file dialog.
    fn open_export_dialog(&mut self) {
        if self.editor.is_none() || self.saving.is_some() {
            return;
        }
        if !self.export_open {
            let state = self.editor.as_ref().map(|e| e.history.current().clone());
            let own = state.as_ref().and_then(|st| SaveFormat::from_file_format(st.source().format));
            let ty = self.save_type.or_else(|| state.as_ref().map(|st| SampleType::for_source(st.source()))).unwrap_or(SampleType::Int24);
            self.export_choice = ExportChoice { format: own.unwrap_or(self.export_choice.format), ty, aiff: self.aiff_kind }.fitted();
        }
        self.export_open = true;
    }

    /// The dialog's Save As button: save once with the choice. The dialog edits a copy
    /// (`export_choice`); the document's own save type is not touched, so cancelling the file
    /// dialog (or any failure) leaves later plain saves as they were.
    fn export_go(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let choice = self.export_choice;
        self.dialog_format = Some(choice.format);
        let kept = self.aiff_kind;
        self.aiff_kind = choice.aiff;
        let r = self.save_with(ctx, true, None, false, Some(choice.ty), false);
        self.aiff_kind = kept;
        r
    }

    /// Make the export choice the one saves use (the type and AIFF kind; the format follows the
    /// file name). Only the control channel's `SAVEOPTS set` and preset loads do this.
    fn apply_export_choice(&mut self) {
        self.save_type = Some(self.export_choice.ty);
        self.aiff_kind = self.export_choice.aiff;
    }

    /// The estimated size of saving the open document with the export choice.
    fn export_estimate(&mut self) -> Option<Result<SizeEstimate, String>> {
        let state = self.editor.as_ref()?.history.current().clone();
        let c = self.export_choice;
        let key = (c.format, c.ty, c.aiff, Arc::as_ptr(&state) as usize);
        if self.size_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let est = estimate_size(&state, c.format, c.ty, c.aiff).map_err(|e| e.to_string());
            self.size_cache = Some((key, est));
        }
        self.size_cache.as_ref().map(|(_, e)| e.clone())
    }

    /// Keep the presets in their file; the message if that failed.
    fn store_presets(&self) -> Result<(), String> {
        match &self.export_presets_path {
            Some(file) => self.export_presets.save(file).map_err(|e| format!("could not save the presets to {}: {e}", file.display())),
            None => Err("this system has no settings folder, so presets last until WavCraft closes".to_string()),
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let est = self.export_estimate();
        let mut open = true;
        let (mut go, mut choice) = (false, self.export_choice);
        let mut save_preset = false;
        let mut delete = None;
        let mut load = None;
        egui::Window::new("Save As / Export")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    for f in [SaveFormat::Wav, SaveFormat::Aiff, SaveFormat::Flac] {
                        ui.radio_value(&mut choice.format, f, f.name());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Sample type:");
                    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
                        ui.add_enabled_ui(choice.format.accepts(ty), |ui| ui.radio_value(&mut choice.ty, ty, ty.describe()));
                    }
                });
                if choice.format == SaveFormat::Aiff {
                    ui.horizontal(|ui| {
                        ui.label("AIFF:");
                        for k in AiffKind::ALL {
                            ui.radio_value(&mut choice.aiff, k, k.describe());
                        }
                    });
                    if choice.ty == SampleType::Float32 {
                        ui.weak("32-bit float is always AIFF-C fl32, big-endian.");
                    }
                }
                match &est {
                    Some(Ok(e)) => ui.label(format_size_label(e)),
                    Some(Err(why)) => ui.colored_label(Color32::from_rgb(235, 120, 90), why),
                    None => ui.label(""),
                };
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("export_preset").selected_text("Choose…").show_ui(ui, |ui| {
                        for p in &self.export_presets.presets {
                            if ui.selectable_label(false, &p.name).clicked() {
                                load = Some(p.name.clone());
                            }
                        }
                    });
                    ui.add(egui::TextEdit::singleline(&mut self.export_preset_name).hint_text("name").desired_width(120.0));
                    if ui.button("Save preset").clicked() {
                        save_preset = true;
                    }
                    if ui.button("Delete").clicked() {
                        delete = Some(self.export_preset_name.clone());
                    }
                });
                ui.separator();
                if ui.button("Save As…").clicked() {
                    go = true;
                }
            });
        self.export_choice = choice.fitted();
        if let Some(name) = load
            && let Some(c) = self.export_presets.get(&name)
        {
            self.export_choice = c;
            self.export_preset_name = name;
        }
        if save_preset {
            let r = self.export_presets.set(&self.export_preset_name, self.export_choice).and_then(|()| self.store_presets());
            match r {
                Ok(()) => self.notice = Some(format!("Saved the export preset {}", self.export_preset_name.trim())),
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(name) = delete {
            if self.export_presets.remove(&name) {
                match self.store_presets() {
                    Ok(()) => self.notice = Some(format!("Deleted the export preset {}", name.trim())),
                    Err(e) => self.error = Some(e),
                }
            } else {
                self.error = Some(format!("No export preset named {}", name.trim()));
            }
        }
        if go || !open {
            self.export_open = false;
        }
        if go && let Err(e) = self.export_go(ctx) {
            self.error = Some(e);
        }
    }

    /// `SAVEOPTS estimate|set <fmt> <bits> [aiff=<kind>]` and `SAVEOPTS show` (control channel).
    fn control_saveopts(&mut self, ctx: &egui::Context, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        let verb = words.next().unwrap_or("");
        if verb == "show" {
            return format!("OK {}", self.export_choice.describe());
        }
        let (Some(f), Some(t)) = (words.next(), words.next()) else {
            return "ERR SAVEOPTS estimate|set <wav|aiff|flac> <16|24|32|float> [aiff=aiff|aifc|sowt]".to_string();
        };
        let (Some(format), Some(ty)) = (SaveFormat::parse(f), SampleType::parse(t)) else {
            return format!("ERR no format or sample type {f} {t}: wav|aiff|flac, 16|24|32|float");
        };
        let mut aiff = self.export_choice.aiff;
        let mut tail: Vec<&str> = words.collect();
        let words_last = if verb == "dialog" { tail.pop() } else { None };
        for w in tail {
            match w.strip_prefix("aiff=").and_then(AiffKind::parse) {
                Some(k) => aiff = k,
                None => return format!("ERR aiff=<aiff|aifc|sowt>, not {w}"),
            }
        }
        let choice = ExportChoice { format, ty, aiff };
        match verb {
            "set" if !format.accepts(ty) => format!("ERR {} cannot hold {} samples", format.name(), ty.describe()),
            "set" => {
                self.export_choice = choice;
                self.apply_export_choice();
                format!("OK {}", choice.describe())
            }
            // `SAVEOPTS dialog <fmt> <bits> [aiff=] cancel|<path>`: the Save As button with this
            // choice, the file dialog answered with the path or cancelled. Changes no saved option.
            "dialog" => {
                let Some(answer) = words_last else { return "ERR SAVEOPTS dialog <fmt> <bits> cancel|<path>".to_string() };
                self.export_choice = choice.fitted();
                self.scripted_pick = Some((answer != "cancel").then(|| PathBuf::from(answer)));
                let r = self.export_go(ctx);
                self.scripted_pick = None;
                match r {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            "estimate" => {
                let Some(editor) = &self.editor else { return "ERR no file is open".to_string() };
                match estimate_size(editor.history.current(), format, ty, aiff) {
                    Ok(e) => format!("OK bytes={} exact={}", e.bytes, u8::from(e.exact)),
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR SAVEOPTS estimate|set|show".to_string(),
        }
    }

    /// `EXPORTPRESET save|load|delete <name>` and `EXPORTPRESET list` (control channel).
    fn control_export_preset(&mut self, rest: &str) -> String {
        let (verb, name) = rest.split_once(' ').unwrap_or((rest, ""));
        match verb {
            "list" => format!("OK {}", self.export_presets.presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("|")),
            "save" => match self.export_presets.set(name, self.export_choice).and_then(|()| self.store_presets()) {
                Ok(()) => format!("OK {}", self.export_choice.describe()),
                Err(e) => format!("ERR {e}"),
            },
            "load" => match self.export_presets.get(name) {
                Some(c) => {
                    self.export_choice = c;
                    self.apply_export_choice();
                    self.export_preset_name = name.trim().to_string();
                    format!("OK {}", c.describe())
                }
                None => format!("ERR no export preset named {:?}", name.trim()),
            },
            "delete" => {
                if !self.export_presets.remove(name) {
                    return format!("ERR no export preset named {:?}", name.trim());
                }
                match self.store_presets() {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            _ => "ERR EXPORTPRESET save|load|delete <name>, or list".to_string(),
        }
    }

    /// The "save changes?" prompt when the window is closed with unsaved changes.
    fn close_prompt(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.any_unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
            self.closing_doc = false;
            // Ask about the current document first; else bring the first unsaved one up.
            if !self.unsaved()
                && let Some(p) = self.docs.parked().iter().position(|s| is_unsaved(&s.editor, &s.saved))
            {
                let _ = self.switch_doc(if p < self.docs.current() { p } else { p + 1 });
            }
        }
        if !self.confirm_close {
            return;
        }
        let name = path_name(&self.doc_path);
        let mut choice = None;
        egui::Window::new(tr("dialog.unsaved")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.label(tr_args("dialog.unsaved.body", &[&name]));
                ui.horizontal(|ui| {
                    if ui.button(tr("button.save")).clicked() {
                        choice = Some(0);
                    }
                    if ui.button(tr("button.dont_save")).clicked() {
                        choice = Some(1);
                    }
                    if ui.button(tr("button.cancel")).clicked() {
                        choice = Some(2);
                    }
                });
            },
        );
        match choice {
            Some(0) => {
                self.confirm_close = false;
                if let Err(e) = self.save(ctx, false, None, true) {
                    self.error = Some(e);
                    self.closing_doc = false;
                    self.close_all = false;
                }
            }
            // Drop this document's changes; closing the window goes on to the next unsaved one.
            Some(1) => {
                self.confirm_close = false;
                let only_doc = std::mem::take(&mut self.closing_doc);
                self.remove_current();
                if !only_doc {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Some(_) => {
                self.confirm_close = false;
                self.closing_doc = false;
                self.close_all = false;
            }
            None => {}
        }
    }

    /// Shift+Left/Right/Home/End extend the selection (taken before the action table, whose
    /// Home and End would otherwise also fire).
    fn selection_keys(&mut self, ctx: &egui::Context) {
        let Some(frames) = self.doc.as_ref().map(|d| d.frames()) else { return };
        let step = self.view.frames_per_px.round().max(1.0) as i64;
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, k));
        for (key, name) in [(Key::ArrowLeft, "shift_left"), (Key::ArrowRight, "shift_right"), (Key::Home, "shift_home"), (Key::End, "shift_end")] {
            if pressed(key) {
                self.gesture(name, step, frames);
            }
        }
    }

    /// The selection gestures by name, shared by the keys and the control channel.
    fn gesture(&mut self, name: &str, step: i64, frames: usize) -> bool {
        match name {
            "shift_left" => self.selection.extend_by(-step, frames),
            "shift_right" => self.selection.extend_by(step, frames),
            "shift_home" => self.selection.extend_edge_to(0, frames),
            "shift_end" => self.selection.extend_edge_to(frames, frames),
            _ => return false,
        }
        self.view.reveal(self.selection.head as f64, self.editor_width, frames);
        true
    }

    /// Answer `GESTURE <name> [args]` (see `control.rs`): the selection gestures at a frame, as
    /// the editor would see them, and the mouse gestures of the navigation tools
    /// (`hand_drag <dx_px>`, `zoom_click <frame> [out]`, `ruler_rdrag <start> <end>`), through
    /// the same code the pointer uses.
    fn control_gesture(&mut self, rest: &str) -> String {
        if let Some(answer) = self.pointer_gesture(rest) {
            return answer;
        }
        // `channel_drag <y_fraction> <start> <end>`: a drag from a height of the waveform area (0 top,
        // 1 bottom) over the frames, as the mouse would. Context-sensitive channel editing picks the channels.
        let mut head = rest.split_whitespace();
        if head.next() == Some("channel_drag") {
            let args: Vec<&str> = head.collect();
            let usage = "ERR GESTURE channel_drag <y_fraction> <start> <end>, with a file open".to_string();
            let y = args.first().and_then(|w| w.parse::<f32>().ok());
            let a = args.get(1).and_then(|w| w.parse::<usize>().ok());
            let b = args.get(2).and_then(|w| w.parse::<usize>().ok());
            let (Some(y), Some(a), Some(b)) = (y, a, b) else { return usage };
            let context = self.settings.context_channel_editing;
            let Some(editor) = self.editor.as_mut() else { return usage };
            let (len, channels) = (editor.history.current().len_frames() as usize, editor.history.current().channel_count());
            if context {
                editor.channels = view::mask_after_drag(y, channels, true, editor.channels);
            }
            let selch = view::channels_text(editor.channels, channels);
            self.selection = Selection { anchor: a.min(len), head: b.min(len) };
            return format!("OK selection={}-{} selch={selch}", self.selection.start(), self.selection.end());
        }
        let frames = self.doc.as_ref().map_or(0, Document::frames);
        let mut it = rest.split_whitespace();
        let name = it.next().unwrap_or("");
        let nums: Vec<usize> = it.map_while(|w| w.parse().ok()).collect();
        let at = |i: usize| nums.get(i).copied().map(|f| f.min(frames));
        // Navigation gestures answer with the view they leave.
        let nav_width = self.editor_width.max(1.0);
        let words: Vec<&str> = rest.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok()).filter(|n| n.is_finite());
        if frames == 0 && matches!(name, "hand_drag" | "zoom_click" | "ruler_rdrag") {
            return "ERR no file is open".to_string();
        }
        let nav = match (name, num(1), num(2)) {
            ("hand_drag", Some(dx), _) => {
                self.view.drag_by(dx as f32, nav_width, frames);
                true
            }
            ("zoom_click", Some(frame), _) => {
                self.view.zoom_click(frame, words.get(2) == Some(&"out"), nav_width, frames);
                true
            }
            ("ruler_rdrag", Some(a), Some(b)) => {
                self.view.zoom_to_range(a, b, nav_width, frames);
                true
            }
            _ => false,
        };
        if nav {
            return format!("OK view={:.0}-{:.0}", self.view.start, self.view.end(nav_width));
        }
        match (name, at(0)) {
            ("shift_click", Some(f)) => self.selection.extend_to(f),
            ("double_click", _) => self.selection = Selection::visible(&self.view, self.editor_width, frames),
            ("triple_click", _) => self.selection = Selection::all(frames),
            // Press near an edge at <from>, drag to <to>.
            ("edge_drag", Some(from)) => {
                let (Some(to), true) = (at(1), self.view.frames_per_px > 0.0) else { return "ERR GESTURE edge_drag <from> <to>".to_string() };
                match self.selection.edge_near(&self.view, self.view.x_of(from as f64), GRAB_PX) {
                    Some(edge) => {
                        self.selection.grab(edge);
                        self.selection.head = to;
                    }
                    None => return format!("ERR no selection edge near frame {from}"),
                }
            }
            ("shift_left" | "shift_right", _) => {
                let step = at(0).map_or(1, |n| n as i64);
                self.gesture(name, step, frames);
            }
            ("shift_home" | "shift_end", _) => {
                self.gesture(name, 0, frames);
            }
            _ => return "ERR GESTURE shift_click <frame> | double_click | triple_click | edge_drag <from> <to> | shift_left|shift_right [frames] | shift_home | shift_end | hand_drag <dx_px> | zoom_click <frame> [out] | ruler_rdrag <start> <end> | channel_drag <y_fraction> <start> <end>".to_string(),
        }
        format!("OK selection={}-{}", self.selection.start(), self.selection.end())
    }

    /// `PLAYBACK [PLAY|STOP|QUALITY fast|high]`: act, then report the playback state (f05) and the device and conversion (f74).
    fn playback_request(&mut self, rest: &str) -> String {
        let mut words = rest.split_whitespace();
        match (words.next().map(str::to_ascii_uppercase).as_deref(), words.next()) {
            (None, _) => {}
            (Some("PLAY"), _) => {
                if self.doc.is_none() {
                    return "ERR no file is open".to_string();
                }
                if !self.player.is_playing() {
                    self.toggle_play();
                }
            }
            (Some("STOP"), _) => self.player.stop(),
            (Some("QUALITY"), Some(word)) => match Quality::parse(word) {
                Some(q) => {
                    // With no settings folder the quality still applies (until WavCraft closes).
                    if let Err(e) = self.pref_set("playback_quality", q.as_str())
                        && self.settings_path.is_some()
                    {
                        return format!("ERR {e}");
                    }
                }
                None => return "ERR PLAYBACK QUALITY fast|high".to_string(),
            },
            _ => return "ERR PLAYBACK [PLAY|STOP|QUALITY fast|high]".to_string(),
        }
        let i = self.player.info();
        // The f05 fields first (`pos` is `position`, kept under both names), then f74's.
        format!(
            "OK playing={} pos={} underruns={} output={} device_rate={} doc_rate={} resampling={} quality={} position={} sink={}",
            u8::from(i.playing),
            i.position,
            i.underruns,
            if self.player.error.is_some() { "none" } else { "ok" },
            i.device_rate,
            i.doc_rate,
            u8::from(i.resampling),
            i.quality.as_str(),
            i.position,
            u8::from(i.test_sink)
        )
    }

    /// Set the metronome (View toolbar and the control channel).
    fn set_metronome(&mut self, m: Metronome) {
        self.player.set_metronome(m, self.tempo);
    }

    /// `METRONOME ...` on the control channel (roadmap C201).
    fn control_metronome(&mut self, rest: &str) -> String {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let mut m = self.player.metronome();
        let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
        let status = |m: &Metronome| format!("OK metronome={} countin={} volume={}", u8::from(m.on), m.count_in_bars, m.volume);
        match words.as_slice() {
            [] => status(&m),
            ["on"] | ["off"] => {
                m.on = words[0] == "on";
                self.set_metronome(m);
                status(&m)
            }
            ["countin", bars] => match bars.parse::<u32>() {
                Ok(b) if b <= 16 => {
                    m.count_in_bars = b;
                    self.set_metronome(m);
                    status(&m)
                }
                _ => "ERR METRONOME countin <bars>, 0 to 16".to_string(),
            },
            ["volume", v] => match v.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => {
                    m.volume = v;
                    self.set_metronome(m);
                    status(&m)
                }
                _ => "ERR METRONOME volume <0 to 1>".to_string(),
            },
            [verb @ ("clicks" | "render"), a, b] => {
                let (Ok(a), Ok(b)) = (a.parse::<f64>(), b.parse::<f64>()) else {
                    return format!("ERR METRONOME {verb} <start_s> <end_s>, in seconds");
                };
                if rate == 0 || !a.is_finite() || !b.is_finite() || b < a {
                    return format!("ERR METRONOME {verb} <start_s> <end_s>: needs an open file and start <= end");
                }
                let track = wavcraft_core::metronome::ClickTrack::new(self.tempo, rate);
                let join = |v: Vec<u64>| v.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
                if *verb == "clicks" {
                    let clicks = track.clicks_between(a, b);
                    let accents: Vec<String> = clicks.iter().map(|c| u8::from(c.accent).to_string()).collect();
                    format!("OK n={} frames={} accents={}", clicks.len(), join(clicks.iter().map(|c| c.frame).collect()), accents.join(","))
                } else {
                    // The frames where the player's own mixer puts a click, over silence (no sound device needed).
                    let to_frame = |s: f64| (s.max(0.0) * f64::from(rate)).round() as usize;
                    let onsets = self.player.render_clicks(to_frame(a), to_frame(b));
                    format!("OK n={} frames={}", onsets.len(), join(onsets.into_iter().map(|f| f as u64).collect()))
                }
            }
            _ => "ERR METRONOME on|off | countin <bars> | volume <0 to 1> | clicks <start_s> <end_s> | render <start_s> <end_s>".to_string(),
        }
    }

    /// Answer one control-channel line (see `control.rs`).
    fn control_request(&mut self, line: &str, ctx: &egui::Context) -> String {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let sel = |s: &Self| format!("{}-{}", s.selection.start(), s.selection.end());
        match cmd {
            "STATE" => {
                let busy = self.op_running()
                    || self.opening.is_some()
                    || !self.open_queue.is_empty()
                    || self.appending.is_some()
                    || self.inserting.is_some()
                    || self.rebuilding.is_some()
                    || self.saving.is_some()
                    || self.running.is_some()
                    || self.batch_write.is_some()
                    || self.integrating.is_some()
                    || self.making.is_some()
                    || self.save_all.is_some();
                let progress = self.running.as_ref().map_or(0, |r| r.job.permille() / 10);
                let unsaved = u8::from(self.unsaved());
                let win = self.window_state(ctx);
                let rec_state = self.rec.state_fields();
                let width = self.editor_width.max(1.0);
                let (vzoom, ampscale, layout) = (self.amp_zoom.get(), self.settings.amp_scale.name(), self.settings.channel_layout.name());
                let zfactor = f64::from(self.settings.zoom_factor);
                let spectral = format!("{} {} {}{}", self.spectral_state(), self.freq_band_state(), self.status_state(), self.op_state());
                let view = format!(
                    "tool={} right_click={} view={:.0}-{:.0} sync={}",
                    self.tool.name(),
                    if self.settings.right_click_menu { "menu" } else { "extend" },
                    self.view.start,
                    self.view.end(width),
                    self.sync_text()
                );
                self.poll_transport();
                let t = &self.transport;
                let playhead = if self.playhead_live() { self.player.position() } else { self.head_scrub.pos.unwrap_or(self.selection.start()) };
                let speed = if self.player.is_playing() { self.player.speed() } else { 0.0 };
                let output = if !self.player.has_output() {
                    "none"
                } else if self.player.info().test_sink {
                    "sink"
                } else {
                    "device"
                };
                let tstate = format!(
                    "play={} pos={playhead} speed={speed} scrubspeed={} roll={} loop={} skip={} return={} follow={} autoscroll={} wraps={} output={output}",
                    t.mode.name(),
                    self.head_scrub.speed,
                    u8::from(t.roll),
                    u8::from(t.looping),
                    u8::from(t.skip_selection),
                    u8::from(t.return_on_stop),
                    self.settings.autoscroll.name(),
                    u8::from(self.follow_mode().is_some()),
                    self.player.wraps()
                );
                match &self.editor {
                    Some(e) => {
                        let doc = e.history.current();
                        format!(
                            "OK frames={} channels={} rate={} selection={} selch={} steps={} cursor={} busy={} meta={} progress={progress} unsaved={unsaved}{rec_state} {win} playing={} position={} device={} reopens={} {view} vzoom={vzoom:.2} ampscale={ampscale} zfactor={zfactor:.2} layout={layout} {tstate} {spectral} time_display={} tempo={} meter={} selection_text={} metronome={} countin={} snap_frames={} snap={} snapzero={} snaprc={} snaprf={} snapedges={} snapmarkers={} enabled={} custom_fps={} cursor_frame={} cursor_text={} clipboard={} clip_frames={} msel={} labels={} active={} undo={} redo={} band={} path={}",
                            doc.len_frames(),
                            doc.channel_count(),
                            doc.sample_rate(),
                            sel(self),
                            view::channels_text(e.channels, doc.channel_count()),
                            e.history.steps().len(),
                            e.history.cursor(),
                            u8::from(busy),
                            u8::from(self.include_meta),
                            u8::from(self.player.is_playing()),
                            self.player.position(),
                            self.player.device.replace(' ', "_"),
                            self.player.reopens(),
                            self.time_format.name(),
                            self.tempo.bpm_text(),
                            self.tempo.meter_text(),
                            self.selection_text(),
                            u8::from(self.player.metronome().on),
                            self.player.metronome().count_in_bars,
                            u8::from(self.snap_frames),
                            u8::from(e.snap.enabled),
                            u8::from(e.snap.zero_crossings),
                            u8::from(e.snap.ruler_coarse),
                            u8::from(e.snap.ruler_fine),
                            u8::from(e.snap.edges),
                            u8::from(e.snap.markers),
                            e.enabled_flags(),
                            self.settings.custom_fps,
                            self.selection.start(),
                            self.fmt_time(self.selection.start()),
                            e.current_clipboard + 1,
                            e.clipboard().map_or(0, |c| c.len_frames()),
                            {
                                let picked: Vec<String> = self.markers_ui.pick.indices(doc.markers()).iter().map(usize::to_string).collect();
                                if picked.is_empty() { "-".to_string() } else { picked.join(",") }
                            },
                            wavcraft_core::channels::labels(doc.source().channel_mask, doc.channel_count()).join(","),
                            e.channels.active(doc.channel_count()).iter().map(usize::to_string).collect::<Vec<_>>().join(","),
                            e.history.undo_label().unwrap_or("-"),
                            e.history.redo_label().unwrap_or("-"),
                            self.band_text(),
                            self.doc_path.display()
                        )
                    }
                    None => format!(
                        "OK frames=0 channels=0 rate=0 selection=0-0 selch=- steps=0 cursor=0 busy={} meta={} progress={progress} unsaved=0 {win} playing={} position={} device={} reopens={} {view} vzoom={vzoom:.2} ampscale={ampscale} zfactor={zfactor:.2} layout={layout} {tstate} {spectral} time_display={} tempo={} meter={} selection_text=- metronome={} countin={} snap_frames={} snap={} snapzero={} snaprc={} snaprf={} snapedges={} snapmarkers={} enabled={} custom_fps={} cursor_frame=0 cursor_text=- clipboard=1 clip_frames=0 msel=- labels=- active=- undo=- redo=- band={} path=",
                        u8::from(busy),
                        u8::from(self.include_meta),
                        u8::from(self.player.is_playing()),
                        self.player.position(),
                        self.player.device.replace(' ', "_"),
                        self.player.reopens(),
                        self.time_format.name(),
                        self.tempo.bpm_text(),
                        self.tempo.meter_text(),
                        u8::from(self.player.metronome().on),
                        self.player.metronome().count_in_bars,
                        u8::from(self.snap_frames),
                        u8::from(self.settings.snap.enabled),
                        u8::from(self.settings.snap.zero_crossings),
                        u8::from(self.settings.snap.ruler_coarse),
                        u8::from(self.settings.snap.ruler_fine),
                        u8::from(self.settings.snap.edges),
                        u8::from(self.settings.snap.markers),
                        "",
                        self.settings.custom_fps,
                        self.band_text()
                    ),
                }
            }
            // Scripted checks of the system clipboard: what is on it, or put a file there as if
            // another program had.
            "OSCLIP" => match rest.split_once(' ') {
                Some(("set", path)) => match self.os_clipboard.put_foreign(Path::new(path)) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                },
                _ => {
                    let (first, ours) = self.os_clipboard.peek();
                    format!("OK ours={} path={}", u8::from(ours), first.map_or("-".to_string(), |p| p.display().to_string()))
                }
            },
            // The markers of the current state, in order: `OK count=<n> <id>,<start>,<end>,<name>;…`.
            "MARKERS" => {
                let list: Vec<String> = self.editor.as_ref().map_or_else(Vec::new, |e| {
                    e.history.current().markers().iter().map(|m| format!("{},{},{},{}", m.id, m.start, m.end, m.name)).collect()
                });
                format!("OK count={} {}", list.len(), list.join(";"))
            }
            // `MARKER rename <i> <name>`, `MARKER move <i> <frame>`, `MARKER select <i> [add]`: <i> is the
            // position in the MARKERS list (from 0).
            "MARKER" => self.control_marker(rest),
            // `PLAYLIST add <marker> [loops=<n>] | move <i> <j> | remove <i> | loops <i> <n> | play | stop | clear | state | sequence`.
            "PLAYLIST" => self.control_playlist(rest),
            "PEAKCACHE" => match rest.trim() {
                "info" => {
                    let (files, bytes) = wavcraft_core::peakcache::info(&self.peak_dir);
                    let last = self
                        .peak_report
                        .as_ref()
                        .map_or_else(|| "last=none last_ms=0".to_string(), |r| format!("last={} last_ms={:.1}", r.result, r.ms));
                    format!("OK files={files} bytes={bytes} cap={} {last}", wavcraft_core::peakcache::DEFAULT_CAP_BYTES)
                }
                "clear" => format!("OK removed={}", wavcraft_core::peakcache::clear(&self.peak_dir)),
                _ => "ERR PEAKCACHE info|clear".to_string(),
            },
            "PROPERTIES" => match &self.doc {
                Some(d) => {
                    let parts: Vec<String> =
                        d.properties().into_iter().map(|(k, v)| format!("{}={}", k.to_lowercase().replace(' ', "_"), v)).collect();
                    format!("OK {}", parts.join(" | "))
                }
                None => "ERR PROPERTIES needs a file open".to_string(),
            },
            // Play frames <from> <to> (scripted checks of playback from the store). With no
            // numbers, play from the selection (to its end, or the file's end for a cursor), as Space does.
            "PLAY" => {
                let mut it = rest.split_whitespace().map(str::parse::<usize>);
                match (it.next(), it.next(), &self.doc) {
                    _ if rest.trim().is_empty() => match &self.doc {
                        Some(doc) => {
                            let frames = doc.frames();
                            let to = if self.selection.is_empty() { frames } else { self.selection.end() };
                            self.player.play(self.selection.start().min(frames), to.min(frames));
                            format!("OK playing={}", u8::from(self.player.is_playing()))
                        }
                        None => "ERR PLAY needs a file open".to_string(),
                    },
                    (Some(Ok(a)), Some(Ok(b)), Some(doc)) => {
                        self.player.play(a.min(doc.frames()), b.min(doc.frames()));
                        "OK".to_string()
                    }
                    _ => "ERR PLAY <from> <to>, in frames, with a file open".to_string(),
                }
            }
            // Simulate the system's default output device changing (the next frame reopens it, at
            // the same place: f48).
            "DEVICE_CHANGED" => {
                self.player.simulate_device_change();
                "OK".to_string()
            }
            // METRONOME on|off | countin <bars> | volume <0..1> | clicks <start_s> <end_s> | render <start_s> <end_s>
            "METRONOME" => self.control_metronome(rest),
            // The pointer readout at pixel (x, y) from the editor's top-left corner (A079).
            "HOVER" => {
                let mut it = rest.split_whitespace().map(str::parse::<f32>);
                match (it.next(), it.next()) {
                    // No arguments: the editor's size and view, so a script can work out where to point.
                    (None, _) => match &self.editor_layout {
                        Some(l) => format!(
                            "OK width={} height={} ruler={} wave={} spectrum={} channels={} start={} frames_per_px={}",
                            l.width,
                            l.height,
                            l.ruler_height,
                            u8::from(l.show_waveform),
                            u8::from(l.show_spectrum),
                            l.channels,
                            self.view.start,
                            self.view.frames_per_px
                        ),
                        None => "ERR no file is shown yet".to_string(),
                    },
                    (Some(Ok(x)), Some(Ok(y))) => self.hover_at(x, y),
                    _ => "ERR HOVER <x_px> <y_px>, from the editor's top-left corner".to_string(),
                }
            }
            // Test hook: count as busy (open or save running) for <ms>. Fake input only.
            "HOLD_BUSY" if std::env::var_os("WAVCRAFT_FAKE_INPUT").is_some() => match rest.trim().parse::<u64>() {
                Ok(ms) => {
                    self.test_busy_until = Some(Instant::now() + std::time::Duration::from_millis(ms));
                    "OK".to_string()
                }
                Err(_) => "ERR HOLD_BUSY <ms>".to_string(),
            },
            "OPEN" if !rest.is_empty() => {
                self.open(PathBuf::from(rest), ctx);
                "OK".to_string()
            }
            // IMPORT_RAW <path> rate=<hz> ch=<n> fmt=<s16le|s24be|f32le|u8|...> [offset=<bytes>]
            "IMPORT_RAW" => match parse_import_raw(rest) {
                Ok((path, spec)) => {
                    self.open_as(path, Some(spec), ctx);
                    "OK".to_string()
                }
                Err(e) => format!("ERR {e}"),
            },
            // Insert › Audio from File at the cursor (or over the selection); poll STATE until busy=0.
            "INSERT_FILE" if !rest.is_empty() => match self.insert_file(PathBuf::from(rest), ctx) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {e}"),
            },
            "INSERT_FILE" => "ERR INSERT_FILE <path>".to_string(),
            "PLAYBACK" => self.playback_request(rest),
            // Which channels edits act on: `CHANNELS all` or `CHANNELS 0,2,5` (0-based).
            "CHANNELS" => match self.editor.as_mut() {
                Some(e) => {
                    let n = e.history.current().channel_count();
                    let mask = if rest.trim() == "all" {
                        Some(wavcraft_core::ChannelMask::ALL)
                    } else {
                        rest.split(',')
                            .map(|t| t.trim().parse::<usize>().ok().filter(|c| *c < n))
                            .try_fold(wavcraft_core::ChannelMask::NONE, |m, c| c.map(|c| m.with(c)))
                    };
                    match mask.filter(|m| *m != wavcraft_core::ChannelMask::NONE) {
                        Some(m) => {
                            e.channels = m;
                            "OK".to_string()
                        }
                        None => format!("ERR CHANNELS all | <0-based channel>,… below {n}"),
                    }
                }
                None => "ERR CHANNELS needs a file open".to_string(),
            },
            // Set the visible range the way the zoom navigator does, in frames.
            "NAV" => {
                let mut it = rest.split_whitespace().map(str::parse::<f64>);
                let frames = self.doc.as_ref().map_or(0, Document::frames);
                match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) if frames > 0 && a.is_finite() && b > a => {
                        let width = self.editor_width.max(1.0);
                        self.view.set_range(a, b, width, frames);
                        format!("OK view={}", self.view_text())
                    }
                    _ => "ERR NAV <start> <end>, in frames, with a file open".to_string(),
                }
            }
            // Choose the amplitude ruler's scale (the ruler's context menu does the same).
            "AMPSCALE" => match AmpScale::parse(rest) {
                Some(scale) => {
                    self.set_amp_scale(scale);
                    format!("OK ampscale={}", scale.name())
                }
                None => "ERR AMPSCALE db|percent|samples|normalized".to_string(),
            },
            // Zoom to Time slots: `ZOOMPRESET set <n>` stores the current view's time in slot n
            // (1-5); `ZOOMPRESET get <n>` reports it in seconds.
            "ZOOMPRESET" => {
                let mut it = rest.split_whitespace();
                let slot = it.nth(1).and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=wavcraft_core::view::ZOOM_SLOTS).contains(n));
                match (rest.split_whitespace().next(), slot) {
                    (Some("set"), Some(n)) => match self.set_zoom_slot(n - 1) {
                        Ok(secs) => format!("OK slot={n} secs={secs:.4}"),
                        Err(e) => format!("ERR {e}"),
                    },
                    (Some("get"), Some(n)) => format!("OK slot={n} secs={:.4}", self.settings.zoom_presets.0[n - 1]),
                    _ => "ERR ZOOMPRESET set|get <1-5>".to_string(),
                }
            }
            // Turn a channel on or off for edits (the amplitude ruler's buttons), 0-based.
            "CHANNEL" => match rest.split_once(' ') {
                Some(("toggle", n)) => match n.trim().parse::<u8>() {
                    Ok(ch) => {
                        // The channels edits act on, as a list (`-` with no file).
                        let active = |app: &WaveApp| {
                            app.editor.as_ref().map_or("-".to_string(), |e| {
                                e.channels.active(e.history.current().channel_count()).iter().map(usize::to_string).collect::<Vec<_>>().join(",")
                            })
                        };
                        let before = active(self);
                        match self.run_action(ActionId::ToggleChannel(ch), ctx) {
                            Ok(_) if active(self) != before => format!("OK enabled={}", active(self)),
                            Ok(_) => "ERR the channel did not change: at least one channel stays enabled".to_string(),
                            Err(why) => format!("ERR {why}"),
                        }
                    }
                    Err(_) => "ERR CHANNEL toggle <n>, n counted from 0".to_string(),
                },
                _ => "ERR CHANNEL toggle <n>".to_string(),
            },
            // Several files, each its own document, opened one at a time: `OPEN_MANY <p1> <p2> ...`
            // (separated by tabs when a path has a space in it).
            "OPEN_MANY" if !rest.trim().is_empty() => {
                let paths = split_paths(rest);
                let n = paths.len();
                self.open_many(paths);
                format!("OK queued={n}")
            }
            // `APPEND current|new <p1> [...]`: add files to the end of the current document as one
            // undo step, or make a new document of them back to back.
            "APPEND" => {
                let (to, files) = rest.trim_start().split_once(char::is_whitespace).unwrap_or((rest.trim(), ""));
                match to {
                    "current" | "new" => match self.start_append(to == "new", split_paths(files), ctx) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                    _ => "ERR APPEND current|new <path> [<path> ...]".to_string(),
                }
            }
            // What the last Open Append / OPEN_MANY reported (empty when nothing went wrong).
            "LASTERROR" => format!("OK {}", self.error.as_deref().unwrap_or("-")),
            "SELECT" => {
                let mut it = rest.split_whitespace().map(str::parse::<usize>);
                match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => {
                        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames() as usize);
                        self.selection = Selection { anchor: a.min(len), head: b.min(len) };
                        format!("OK selection={}", sel(self))
                    }
                    _ => "ERR SELECT <start> <end>, in frames".to_string(),
                }
            }
            // Window › <panel>: `window.toggle.<id>`, `window.show.<id>`, `window.hide.<id>`.
            "ACTION" if rest.starts_with("window.") => {
                let mut next = self.panels.clone();
                match next.apply_action(rest.trim()) {
                    Some(Ok(_)) => {
                        let ids: Vec<&str> =
                            panels::REGISTRY.iter().map(|p| p.id).filter(|id| next.is_shown(id) != self.panels.is_shown(id)).collect();
                        for id in ids {
                            self.set_panel(id, next.is_shown(id));
                        }
                        format!("OK selection={} changed=0 {}", sel(self), self.panels.report())
                    }
                    Some(Err(e)) => format!("ERR {e}"),
                    None => format!("ERR no action named {rest:?}"),
                }
            }
            // A panel's ≡ menu: `PANELMENU <panel> list` → `OK items=close,maximize,… maximized=<id|none>`;
            // `PANELMENU <panel> run <item>` → `OK maximized=<id|none> <panels report>`.
            "PANELMENU" => {
                let mut it = rest.split_whitespace();
                let (panel, verb, item) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next());
                let maximized = |s: &Self| -> String { s.layout.maximized(&s.panels).unwrap_or("none").to_string() };
                match (verb, item) {
                    _ if panels::by_id(panel).is_none() => format!("ERR no panel with the id {panel:?}"),
                    ("list", None) => {
                        let ids: Vec<&str> = panels::menu(panel, self.layout.maximized(&self.panels) == Some(panel)).iter().map(|i| i.id).collect();
                        format!("OK items={} maximized={}", ids.join(","), maximized(self))
                    }
                    ("run", Some(item)) => match self.run_panel_item(panel, item) {
                        Ok(()) => format!(
                            "OK maximized={} confirm_clear_history={} {}",
                            maximized(self),
                            u8::from(self.confirm_clear_history),
                            self.panels.report()
                        ),
                        Err(e) => format!("ERR {e}"),
                    },
                    _ => "ERR PANELMENU <panel> list | PANELMENU <panel> run <item>".to_string(),
                }
            }
            // The Metadata Display dialog: `METADATA` reports the fields shown; `METADATA set a,b`
            // chooses them; `METADATA open` shows the dialog.
            "METADATA" => {
                let (verb, arg) = rest.split_once(char::is_whitespace).map_or((rest.trim(), ""), |(v, a)| (v, a.trim()));
                match verb {
                    "" => format!("OK fields={} dialog={}", self.metadata.report(), u8::from(self.metadata_dialog)),
                    "open" => {
                        self.metadata_dialog = true;
                        "OK".to_string()
                    }
                    "set" => {
                        let ids: Vec<&str> = arg.split(',').map(str::trim).filter(|v| !v.is_empty() && *v != "none").collect();
                        match ids.iter().find(|i| MetadataField::from_id(i).is_none()) {
                            Some(bad) => format!("ERR no metadata field {bad:?}"),
                            None => {
                                for &f in MetadataField::ALL {
                                    self.set_metadata_field(f, ids.contains(&f.id()));
                                }
                                format!("OK fields={}", self.metadata.report())
                            }
                        }
                    }
                    _ => "ERR METADATA [open | set <field,field,…|none>]".to_string(),
                }
            }
            // Which panels are shown: `OK waveform=1 spectral=1 history=0`.
            "PANELS" => self.panels_report(),
            "GESTURE" => self.control_gesture(rest),
            // The snap step a selection gesture calls, for scripted checks: `SNAP <frame> [frames_per_px]`
            // (the view's zoom when omitted); the selection's edges are the extra targets.
            "SNAP" => {
                let mut it = rest.split_whitespace();
                let frame = it.next().and_then(|f| f.parse::<u64>().ok());
                let fpp = it.next().map_or(Some(self.view.frames_per_px), |f| f.parse::<f64>().ok());
                match (frame, fpp, self.editor.as_ref()) {
                    (Some(frame), Some(fpp), Some(e)) => {
                        let targets = [self.selection.start() as u64, self.selection.end() as u64];
                        format!("OK frame={}", self.snap_at(e, frame, fpp, &targets))
                    }
                    _ => "ERR SNAP <frame> [frames_per_px], with a file open".to_string(),
                }
            }
            // The step a ruler gesture calls, for scripted checks: `SNAP_FRAME <frame>`.
            "SNAP_FRAME" => match rest.trim().parse::<usize>() {
                Ok(f) => format!("OK frame={}", self.snap_gesture(f)),
                Err(_) => "ERR SNAP_FRAME <frame>".to_string(),
            },
            "SELECT_TIME" => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                let (fmt, tempo) = (self.time_format, self.tempo);
                let mut it = rest.split_whitespace().map(|t| fmt.parse_with(t, rate, &tempo));
                match (it.next(), it.next(), it.next()) {
                    (Some(Some(a)), Some(Some(b)), None) => {
                        let len = self.editor.as_ref().map_or(0, |e| e.history.current().len_frames() as usize);
                        self.selection = Selection { anchor: a.min(len), head: b.min(len) };
                        format!("OK selection={}", sel(self))
                    }
                    _ => format!("ERR SELECT_TIME <start> <end>, each written as {} (no spaces inside a time)", self.time_format.menu()),
                }
            }
            // `MONITORS` | `MONITORS simulate <name>:<x>,<y>,<w>,<h>[;…]` (the first is the primary;
            // floating windows on a monitor that is not listed move onto the primary) | `MONITORS real`.
            "MONITORS" => match rest.split_once(char::is_whitespace).map_or((rest, ""), |(v, a)| (v, a.trim())) {
                ("", _) => self.monitors_report(),
                ("simulate", list) => match monitors::parse(list) {
                    Ok(m) => {
                        self.monitors = Some(m);
                        self.reconcile_floats();
                        self.save_panels();
                        self.monitors_report()
                    }
                    Err(e) => format!("ERR {e}"),
                },
                ("real", _) => {
                    self.monitors = None;
                    self.monitors_report()
                }
                _ => "ERR MONITORS | MONITORS simulate <name>:<x>,<y>,<w>,<h>[;…] | MONITORS real".to_string(),
            },
            // `WORKSPACES` lists them; `WORKSPACE save|switch|reset|delete <name>`.
            "WORKSPACES" => self.workspaces_report(),
            "WORKSPACE" => match self.workspace_command(rest) {
                Ok(()) => self.workspaces_report(),
                Err(e) => format!("ERR {e}"),
            },
            // `PANEL move <id> <left|right|bottom>`, `PANEL maximize <id>` (again to restore),
            // `PANEL close <id>`, `PANEL activate <id>`.
            "PANEL" => match self.layout.apply_command(&mut self.panels, rest) {
                Ok(()) => {
                    self.save_panels();
                    self.panels_report()
                }
                Err(e) => format!("ERR {e}"),
            },
            // The spectrum's peaks: `FREQ [fft=4096] [window=hann] [start end]`.
            "FREQ" => match &self.editor {
                Some(e) => freq_panel::control(e.history.current(), e.channels, self.selection_range(), rest),
                None => "ERR no file open".to_string(),
            },
            // `DIVIDER drag <zone> <dx> <dy>`, `DIVIDER corner <dx> <dy> [left|right]`,
            // `DIVIDER reset <zone>` (f89): what dragging and double-clicking the dividers do.
            "DIVIDER" => {
                let win = self.window_size(ctx);
                match self.layout.divider_command(&self.panels, win, rest) {
                    Ok(()) => {
                        self.save_panels();
                        self.panels_report()
                    }
                    Err(e) => format!("ERR {e}"),
                }
            }
            "ACTION" => match actions::id_by_name(rest) {
                Some(id) => match if matches!(id, ActionId::CopyToNew | ActionId::PasteToNew | ActionId::CloseAll | ActionId::SaveAll) {
                    self.run_doc_action(id, ctx, false)
                } else {
                    self.run_action(id, ctx)
                } {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                None => format!("ERR no action named {rest:?}"),
            },
            // Press a shortcut: `KEY Ctrl+Z` runs what the table binds it to, as the key would.
            // (Open is refused: it would wait for a file dialog nobody can answer.)
            "KEY" => match self.keymap.resolve_text(rest) {
                Ok(Command::Open) => "ERR file.open opens a dialog: use OPEN <path>".to_string(),
                Ok(command) => match self.run_command(command, ctx) {
                    Ok(changed) => format!("OK command={} selection={} changed={}", command.name(), sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                Err(e) => format!("ERR {e}"),
            },
            // JKL shuttle: l, j, k, k+l and k+j (held until `up`).
            "KEYJKL" => {
                self.poll_transport();
                match rest.trim() {
                    "l" | "j" | "k" => self.shuttle_key(rest.trim().chars().next().unwrap_or('k'), false),
                    "k+l" | "k+j" => {
                        self.hold.ctl_nudge = true;
                        self.shuttle_key(if rest.trim() == "k+l" { 'l' } else { 'j' }, true);
                    }
                    "up" => self.release_nudge(),
                    _ => return "ERR KEYJKL l|j|k|k+l|k+j|up".to_string(),
                }
                format!("OK speed={} level={}", transport::speed_label(self.shuttle_speed()), self.transport.shuttle.level)
            }
            // Scrub: the playhead is dragged to <frame>; `SCRUB end` lets go.
            "SCRUB" => {
                if rest.trim() == "end" {
                    self.scrub_end();
                    return "OK".to_string();
                }
                match rest.trim().parse::<usize>() {
                    Ok(frame) if self.doc.is_some() => {
                        self.head_scrub.dragging = true;
                        self.scrub_to(frame);
                        format!("OK pos={} speed={}", self.head_scrub.pos.unwrap_or(0), self.head_scrub.speed)
                    }
                    _ => "ERR SCRUB <frame> (with a file open) or SCRUB end".to_string(),
                }
            }
            "CONTEXTMENU" => self.control_contextmenu(rest, ctx),
            "TOOLBAR" => self.control_toolbar(rest, ctx),
            // Minimise or restore the window (scripted checks of hidden-window behaviour).
            "SHORTCUTS" | "SHORTCUT" => self.shortcut_request(cmd, rest),
            "SHORTCUTSETS" | "SHORTCUTSET" | "KEYBOARD" => self.shortcut_set_request(cmd, rest),
            "WINDOW" => match rest {
                "minimize" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    "OK".to_string()
                }
                "restore" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    "OK".to_string()
                }
                _ => "ERR WINDOW minimize|restore".to_string(),
            },
            // Application settings as one file: `SETTINGS export <path>` / `SETTINGS import <path>`.
            "SETTINGS" => match rest.split_once(' ') {
                Some(("export", p)) if !p.trim().is_empty() => {
                    self.export_settings(Path::new(p.trim())).map_or_else(|e| format!("ERR {e}"), |m| format!("OK {m}"))
                }
                Some(("import", p)) if !p.trim().is_empty() => {
                    self.import_settings(Path::new(p.trim())).map_or_else(|e| format!("ERR {e}"), |m| format!("OK {m}"))
                }
                _ => "ERR SETTINGS export|import <path>".to_string(),
            },
            // Open Help › About; answers with the size of the notices it shows.
            "ABOUT" => {
                self.show_about = true;
                format!("OK notices_bytes={}", crate::notices::TEXT.len())
            }
            "NOISE_PRINT" => match wavcraft_core::noise::current() {
                Some(p) => format!("OK frames={} channels={} rate={}", p.frames, p.density.len(), p.sample_rate),
                None => "OK none".to_string(),
            },
            // `EFFECT` lists the effects and their settings; `EFFECT <name> key=value ...` runs one
            // on the selection (the whole file when none), the unset keys at their defaults.
            "EFFECT" if rest.is_empty() => {
                let all: Vec<String> =
                    wavcraft_core::effects::all().iter().map(|e| format!("{} [{}]", e.id(), wavcraft_core::effects::describe(*e))).collect();
                format!("OK {}", all.join("; "))
            }
            "EFFECT" => {
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let name = name.strip_prefix("effect.").unwrap_or(name);
                match wavcraft_core::effects::all().iter().position(|e| e.id() == name) {
                    None => format!("ERR no effect named {name:?}"),
                    Some(i) => match wavcraft_core::effects::parse_params(wavcraft_core::effects::all()[i], args) {
                        Err(e) => format!("ERR {e}"),
                        Ok(params) => match self.run_effect(i as u16, params, ctx) {
                            Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                            Err(e) => format!("ERR {e}"),
                        },
                    },
                }
            }
            // `DECLIP [threshold_db=.. min_run=..]` scans the selection (the whole file when none)
            // and repairs clipped peaks as one undo step "DeClipper".
            "DECLIP" => match wavcraft_core::diagnostics::declip::Params::parse(rest) {
                Err(e) => format!("ERR {e}"),
                Ok(params) => match self.run_declip(params, ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
            },
            // `DECLICK [sensitivity=.. max_width_ms=..]` and `AUTOCLICK [threshold=.. complexity=..]`
            // scan the selection (the whole file when none) and repair clicks as one undo step.
            "DECLICK" => match wavcraft_core::diagnostics::declick::Params::parse(rest) {
                Err(e) => format!("ERR {e}"),
                Ok(params) => match self.run_declick(params, false, ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
            },
            // `DIAG scan <declip|declick> [k=v ..]` fills the Diagnostics list (the answer comes
            // when the scan ends); `DIAG repair <all|i,j,k>` repairs rows as one undo step.
            "DIAG" => {
                let (sub, args) = rest.split_once(' ').unwrap_or((rest, ""));
                match sub {
                    "scan" => match diagnostics_panel::parse_tool(args) {
                        Err(e) => format!("ERR {e}"),
                        Ok(tool) => match self.diag_scan(tool, ctx) {
                            Ok(()) if self.diag.scanning() => {
                                // `logic` hands the requester's reply to the window; the scan answers it.
                                self.diag_wait = true;
                                String::new()
                            }
                            Ok(()) => "ERR the scan did not start".to_string(),
                            Err(e) => format!("ERR {e}"),
                        },
                    },
                    "repair" => match diagnostics_panel::parse_rows(args, self.diag.region_count()) {
                        Err(e) => format!("ERR {e}"),
                        Ok(rows) => match self.diag_repair(&rows, ctx) {
                            Ok((n, m)) => format!("OK repaired={n} skipped={m}"),
                            Err(e) => format!("ERR {e}"),
                        },
                    },
                    _ => "ERR DIAG scan <declip|declick> [k=v ..] or DIAG repair <all|i,j,k>".to_string(),
                }
            }
            "AUTOCLICK" => match wavcraft_core::diagnostics::declick::AutoParams::parse(rest) {
                Err(e) => format!("ERR {e}"),
                Ok(params) => match self.run_declick(params, true, ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
            },
            // The effect dialog: `EFFECT_DIALOG <name> [key=value ...]` opens it on those values,
            // `EFFECT_DIALOG` says what is open, `EFFECT_DIALOG OK|CANCEL` presses the button.
            "EFFECT_DIALOG" => match rest.trim() {
                "" => match &self.effect_dialog {
                    Some(d) => format!("OK open={} values={:?}", d.id(), d.values),
                    None => "OK open=none".to_string(),
                },
                "OK" | "CANCEL" => match self.finish_effect_dialog(rest.trim() == "OK", ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                _ => {
                    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    let name = name.strip_prefix("effect.").unwrap_or(name);
                    match wavcraft_core::effects::all().iter().position(|e| e.id() == name) {
                        Some(i) if self.editor.is_some() => match wavcraft_core::effects::parse_params(wavcraft_core::effects::all()[i], args) {
                            Err(e) => format!("ERR {e}"),
                            Ok(v) => {
                                self.effect_dialog = EffectDialog::new(i as u16, Some(v));
                                "OK".to_string()
                            }
                        },
                        _ => format!("ERR no effect named {name:?}, or no file is open"),
                    }
                }
            },
            // Generate (f21): `GENERATE` lists the generators; `GENERATE tone key=value ... [seed=N]`
            // inserts at the cursor or replaces the selection. `GENERATE_DIALOG <name> [key=value ...]`
            // opens the dialog, `GENERATE_DIALOG PREVIEW` starts its preview (`OK preview=<frames>`),
            // `GENERATE_DIALOG OK|CANCEL` presses a button.
            "GENERATE" if rest.is_empty() => {
                let all: Vec<String> = wavcraft_core::generate::all()
                    .iter()
                    .map(|g| format!("{} [{}]", g.id(), wavcraft_core::effects::describe_list(g.parameters())))
                    .collect();
                format!("OK {}", all.join("; "))
            }
            "GENERATE" => {
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let (seed, args) = split_seed(args);
                match wavcraft_core::generate::find(name) {
                    None => format!("ERR no generator named {name:?}"),
                    Some(g) => match (wavcraft_core::generate::parse_params(g, &args), seed) {
                        (Err(e), _) | (_, Err(e)) => format!("ERR {e}"),
                        (Ok(params), Ok(seed)) => {
                            let i = wavcraft_core::generate::all().iter().position(|x| x.id() == g.id()).unwrap_or(0);
                            match self.run_generator(i as u16, params, seed, ctx) {
                                Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                                Err(e) => format!("ERR {e}"),
                            }
                        }
                    },
                }
            }
            "GENERATE_DIALOG" => match rest.trim() {
                "OK" | "CANCEL" => match self.finish_effect_dialog(rest.trim() == "OK", ctx) {
                    Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                    Err(e) => format!("ERR {e}"),
                },
                "PREVIEW" => match self.start_preview() {
                    Ok(frames) => format!("OK preview={frames}"),
                    Err(e) => format!("ERR {e}"),
                },
                "" => match &self.effect_dialog {
                    Some(d) => format!("OK open={} values={:?} previewing={}", d.id(), d.values, u8::from(self.previewing)),
                    None => "OK open=none".to_string(),
                },
                _ => {
                    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    match wavcraft_core::generate::all().iter().position(|g| g.id() == name) {
                        Some(i) if self.editor.is_some() => match wavcraft_core::generate::parse_params(wavcraft_core::generate::all()[i], args) {
                            Err(e) => format!("ERR {e}"),
                            Ok(v) => {
                                self.effect_dialog = EffectDialog::new_generator(i as u16, Some(v));
                                "OK".to_string()
                            }
                        },
                        _ => format!("ERR no generator named {name:?}, or no file is open"),
                    }
                }
            },
            // f102: the open document as a SoundCraft clip source, read back in bounded chunks.
            "SCSOURCE" if rest.trim() == "test" => match &self.editor {
                Some(e) => match wavcraft_core::sc_source::session_with_document(e.history.current(), "document")
                    .and_then(|(session, _)| wavcraft_core::sc_source::read_checksum(&session))
                {
                    Ok((frames, sum)) => format!("OK frames={frames} checksum={sum:016x}"),
                    Err(e) => format!("ERR {e}"),
                },
                None => "ERR no document is open".to_string(),
            },
            // The Effects Rack (f31): `RACK` says its state, `RACK set <slot> <effect|none>
            // [key=value ...]`, `RACK power <slot|master> on|off`, `RACK in|out <dB>`,
            // `RACK mix <pct>`, `RACK apply`. Slots count from 1.
            "RACK" if rest.trim() == "apply" => match self.apply_rack(ctx) {
                Ok(changed) => format!("OK selection={} changed={}", sel(self), u8::from(changed)),
                Err(e) => format!("ERR {e}"),
            },
            "RACK" => self.rack.command(rest),
            // VST3 hosting (f128): `VST3 scan` (background), `VST3 list`.
            "VST3" => self.rack.vst3_command(rest),
            // `PRESET save|load|delete|list <effect|rack> [name]`.
            // PRESET list|save|delete: the conversion presets (A161); any other PRESET line is the
            // Effects Rack's (f31).
            "PRESET" if matches!(rest.split_whitespace().next(), Some("list" | "save" | "delete")) => {
                let (verb, arg) = rest.split_once(' ').unwrap_or((rest, ""));
                match verb {
                    "list" => format!(
                        "OK {}",
                        self.convert_presets.all().iter().map(|p| format!("{} | {}", p.name, p.settings.to_text())).collect::<Vec<_>>().join(" ; ")
                    ),
                    "save" => match arg.split_once('|') {
                        Some((name, settings)) => match ConvertSettings::parse(settings).and_then(|s| self.convert_presets.save(name, s)) {
                            Ok(()) => "OK".to_string(),
                            Err(e) => format!("ERR {e}"),
                        },
                        None => "ERR PRESET save <name> | <settings>".to_string(),
                    },
                    _ => match self.convert_presets.delete(arg) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                }
            }
            "PRESET" => self.rack.preset(rest, &mut self.effect_dialog),
            // Convert Sample Type with `key=value` settings (`rate=44100 channels=1 bits=16`), or
            // `preset=<name>`; it runs as a background job: poll STATE until busy=0.
            "CONVERT" => {
                let parsed = match rest.strip_prefix("preset=") {
                    Some(name) => self.convert_presets.find(name).map(|p| p.settings).ok_or_else(|| format!("no preset named {:?}", name.trim())),
                    None => ConvertSettings::parse(rest),
                };
                match parsed.and_then(|s| self.start_convert(&s, ctx)) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // Interpret Sample Rate: relabel the rate, no resampling.
            "INTERPRET" => match rest.trim().parse::<u32>() {
                Ok(rate) => match self.run_interpret(rate, ctx) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                },
                Err(_) => "ERR INTERPRET <hz>".to_string(),
            },
            // A number the effects use: `PARAM amplify.db 6`, `PARAM normalize.db -1`.
            "PARAM" => {
                let mut it = rest.split_whitespace();
                match (it.next(), it.next().and_then(|v| v.parse::<f32>().ok()), self.editor.as_mut()) {
                    (Some(name), Some(v), Some(e)) => match e.set_param(name, v) {
                        Ok(()) => "OK".to_string(),
                        Err(why) => format!("ERR {why}"),
                    },
                    _ => "ERR PARAM <amplify.db|normalize.db> <number>, with a file open".to_string(),
                }
            }
            // f127: `STRETCH ratio=<0.1-10> pitch=<semitones>` (Process › Stretch and Pitch on the
            // selection, the whole file when none; the result is selected) and `PITCHDETECT`
            // (median fundamental of the selection, in Hz).
            "STRETCH" => {
                let (mut ratio, mut pitch) = (1.0f64, 0.0f64);
                for kv in rest.split_whitespace() {
                    match kv.split_once('=') {
                        Some(("ratio", v)) => ratio = v.parse().unwrap_or(f64::NAN),
                        Some(("pitch", v)) => pitch = v.parse().unwrap_or(f64::NAN),
                        _ => return "ERR STRETCH ratio=<0.1-10> pitch=<semitones>".to_string(),
                    }
                }
                if self.saving.is_some() {
                    return "ERR wait for the save to finish".to_string();
                }
                if self.running.is_some() {
                    return "ERR wait for the background job to finish, or cancel it".to_string();
                }
                let sel_range = self.selection_range();
                let Some(editor) = self.editor.as_mut() else { return "ERR no file is open".to_string() };
                match editor.stretch_pitch(ratio, pitch, sel_range) {
                    Ok(out) => {
                        self.selection = Selection { anchor: out.selection.start as usize, head: out.selection.end as usize };
                        self.error = None;
                        self.notice = out.message;
                        if out.doc_changed {
                            self.rebuild(ctx);
                        }
                        format!("OK selection={} changed={}", sel(self), u8::from(out.doc_changed))
                    }
                    Err(e) => {
                        let why = format!("Stretch and Pitch: {e}");
                        log::warn!(target: "wavcraft", "{why}");
                        self.error = Some(why.clone());
                        format!("ERR {why}")
                    }
                }
            }
            "PITCHDETECT" => {
                let sel_range = self.selection_range();
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                match wavcraft_core::timepitch::detect_pitch(editor.history.current(), sel_range) {
                    Ok(Some(hz)) => {
                        self.notice = Some(format!("Pitch {hz:.1} Hz"));
                        format!("OK hz={hz:.2}")
                    }
                    Ok(None) => "OK hz=none".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // Edit › Mix Paste (see `mixpaste::parse` for the settings).
            "MIXPASTE" => match mixpaste::parse(rest) {
                Ok(req) => match self.run_mix_paste(req, ctx) {
                    Ok(()) => format!("OK selection={}", sel(self)),
                    Err(e) => format!("ERR {e}"),
                },
                Err(e) => format!("ERR {e}"),
            },
            "INSERT_SILENCE" => match rest.trim().parse::<f64>() {
                Ok(secs) if secs.is_finite() && secs > 0.0 => {
                    let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                    match silence_frames(secs, rate) {
                        Err(why) => format!("ERR {why}"),
                        Ok(frames) => match self.run_insert_silence(frames, ctx) {
                            None => format!("OK selection={}", sel(self)),
                            Some(e) => format!("ERR {e}"),
                        },
                    }
                }
                _ => "ERR INSERT_SILENCE <seconds>, above 0".to_string(),
            },
            // The same, with the length written in the chosen time format (A146).
            "INSERT_SILENCE_FMT" => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                match self.time_format.parse_with(rest, rate, &self.tempo).filter(|f| *f > 0) {
                    Some(frames) => match self.run_insert_silence(frames as u64, ctx) {
                        None => format!("OK selection={}", sel(self)),
                        Some(e) => format!("ERR {e}"),
                    },
                    None => format!("ERR INSERT_SILENCE_FMT <length>, above 0, written as {}", self.time_format.menu()),
                }
            }
            // Jump to time: the cursor goes to the typed time (clamped to the file) (A104).
            "GOTO" => {
                let rate = self.editor.as_ref().map_or(0, |e| e.history.current().sample_rate());
                match (self.time_format.parse_with(rest, rate, &self.tempo), self.editor.is_some()) {
                    (Some(frame), true) => {
                        self.goto_frame(frame);
                        format!("OK cursor={}", self.selection.start())
                    }
                    (_, false) => "ERR no file is open".to_string(),
                    (None, _) => format!("ERR GOTO <time>, written as {}", self.time_format.menu()),
                }
            }
            // Delete Silence with settings: one undo step; answers the count and seconds removed.
            "DELETE_SILENCE" => {
                use wavcraft_core::diagnostics::silence::SilenceParams;
                let mut p = SilenceParams::default();
                for kv in rest.split_whitespace() {
                    let ok = match kv.split_once('=') {
                        Some(("threshold_db", v)) => v.parse().map(|x| p.threshold_db = x).is_ok(),
                        Some(("min_len_ms", v)) => v.parse().map(|x| p.min_len_ms = x).is_ok(),
                        Some(("keep_ms", v)) => v.parse().map(|x| p.keep_ms = x).is_ok(),
                        _ => false,
                    };
                    if !ok {
                        return format!("ERR DELETE_SILENCE [threshold_db=<dB>] [min_len_ms=<ms>] [keep_ms=<ms>]; cannot use {kv:?}");
                    }
                }
                if self.saving.is_some() {
                    return "ERR wait for the save to finish".to_string();
                }
                let selection = self.selection_range();
                let Some(editor) = self.editor.as_mut() else { return "ERR no file is open".to_string() };
                let rate = f64::from(editor.history.current().sample_rate().max(1));
                match editor.delete_silence(selection, &p) {
                    Ok(done) => {
                        self.selection = Selection { anchor: done.outcome.selection.start as usize, head: done.outcome.selection.end as usize };
                        self.error = None;
                        self.notice = done.outcome.message;
                        if done.outcome.doc_changed {
                            self.rebuild(ctx);
                        }
                        format!("OK stretches={} removed_s={:.3} selection={}", done.stretches, done.removed_frames as f64 / rate, sel(self))
                    }
                    Err(e) => {
                        log::warn!(target: "wavcraft", "Delete Silence: {e}");
                        format!("ERR Delete Silence: {e}")
                    }
                }
            }
            // Amplitude statistics of a range (frames), or of the selection / whole file; the
            // numbers come back on one line and the panel and its navigation use them.
            "STATS" => {
                let Some(state) = self.editor.as_ref().map(|e| e.history.current().clone()) else { return "ERR no file is open".to_string() };
                let mut it = rest.split_whitespace().map(str::parse::<u64>);
                let (range, whole) = match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => (a..b, false),
                    (None, _) => self.stats_scope(),
                    _ => return "ERR STATS [<start> <end>], in frames".to_string(),
                };
                self.stats.cancel();
                match wavcraft_core::stats::scan(&state, range, wavcraft_core::stats::DEFAULT_RMS_WINDOW_MS, &mut |_| {}, &AtomicBool::new(false)) {
                    Ok(Some(stats)) => {
                        let line = format!("OK {}", stats.summary());
                        self.stats.measured = Some(crate::statspanel::Measured { state, stats, whole });
                        line
                    }
                    Ok(None) => "ERR cancelled".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // Auto Save: the journal writer's progress, and the sessions a crashed run left.
            "AUTOSAVE" => {
                let (offered, written, failed, pending) = self.recovery.stats();
                format!(
                    "OK offered={offered} written={written} failed={failed} pending={} scanned={} recoverable={} takes={}",
                    u8::from(pending),
                    u8::from(self.recovery.scanned),
                    self.recovery.offers.len(),
                    self.recovery.takes.len()
                )
            }
            // C032 (f55): unfinished takes. RECOVERTAKE [index] repairs and opens; DISCARDTAKE deletes.
            "RECOVERTAKE" | "DISCARDTAKE" => match rest.trim().parse::<usize>().or_else(|_| if rest.trim().is_empty() { Ok(0) } else { Err(()) }) {
                Ok(i) => {
                    let r = if cmd == "RECOVERTAKE" { self.recover_take(i, ctx) } else { self.discard_take(i) };
                    r.map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string())
                }
                Err(()) => format!("ERR {cmd} [index]"),
            },
            "RECOVER" | "DISCARD_RECOVERY" => match rest.trim().parse::<usize>().or_else(|_| if rest.trim().is_empty() { Ok(0) } else { Err(()) }) {
                Ok(i) => {
                    let r = if cmd == "RECOVER" { self.recover_session(i, ctx) } else { self.discard_recovery(i) };
                    r.map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string())
                }
                Err(()) => format!("ERR {cmd} [index]"),
            },
            // Media and Disk Cache: `CACHE info`, `CACHE cleanup <days>` (the owner's confirmation
            // is the dialog's; a script asking is the confirmation).
            "CACHE" => {
                let mut it = rest.split_whitespace();
                match (it.next(), it.next().map(str::parse::<u32>)) {
                    (Some("info"), None) => {
                        let u = self.session_usage();
                        format!(
                            "OK sessions={} bytes={} recoverable={} stale={} removable={} removable_bytes={} keep_days={} cache_mb={} cache_default_mb={} folder={}",
                            u.sessions,
                            u.bytes,
                            u.recoverable,
                            u.stale,
                            u.removable,
                            u.removable_bytes,
                            self.settings.autosave_keep_days,
                            self.settings.cache_bytes() >> 20,
                            wavcraft_core::store::default_cache_bytes() >> 20,
                            self.sessions.path.display()
                        )
                    }
                    (Some("cleanup"), Some(Ok(days))) => {
                        let done = self.cleanup_sessions(days);
                        format!("OK removed={} freed={}", done.removed, done.freed)
                    }
                    _ => "ERR CACHE info | CACHE cleanup <days>".to_string(),
                }
            }
            // The History panel's click: go to step <index> (0 is Open).
            "HISTORY" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true)
                    if self.saving.is_none() && self.running.is_none() && self.rec.going_into_doc().is_none() && self.integrating.is_none() =>
                {
                    if self.history_go(i, ctx) { "OK".to_string() } else { format!("ERR no step {i}") }
                }
                _ => "ERR HISTORY <step>, with a file open".to_string(),
            },
            "VIDEO" => {
                let (sub, arg) = rest.split_once(' ').unwrap_or((rest, ""));
                match sub {
                    "attach" if !arg.trim().is_empty() => match self.attach_video(PathBuf::from(arg.trim()), ctx) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                    "status" => match &self.video {
                        Some(v) => format!("OK {}", v.status.text()),
                        None => "ERR no video attached".to_string(),
                    },
                    "detach" => {
                        self.drop_video();
                        "OK".to_string()
                    }
                    _ => "ERR VIDEO attach <path> | frame <seconds> | status | detach".to_string(),
                }
            }
            // The panel's Delete State: step <index> and every later step go (0 can't).
            "HISTORY_DELETE" => match (rest.trim().parse::<usize>(), self.editor.is_some()) {
                (Ok(i), true) if self.saving.is_none() && self.running.is_none() => match self.history_delete(i, ctx) {
                    Some(n) => format!("OK removed={n}"),
                    None => format!("ERR no step {i} to delete (Open can't be)"),
                },
                _ => "ERR HISTORY_DELETE <step>, with a file open".to_string(),
            },
            // Clear History without the panel's confirmation: the current state is all that stays.
            "HISTORY_CLEAR" => match self.editor.as_mut() {
                Some(e) if self.saving.is_none() && self.running.is_none() => {
                    e.history.clear();
                    "OK".to_string()
                }
                _ => "ERR HISTORY_CLEAR, with a file open".to_string(),
            },
            // The panel's Up / Down keys.
            "HISTORY_KEY" if self.editor.is_some() && self.saving.is_none() && self.running.is_none() => match rest.trim() {
                k @ ("up" | "down") => match self.history_arrow(k == "down", ctx) {
                    Some(i) => format!("OK cursor={i}"),
                    None => "OK cursor=unchanged".to_string(),
                },
                _ => "ERR HISTORY_KEY up|down".to_string(),
            },
            // META get [field] | META set <field> <value>: the metadata of the current step.
            // XMP get [<ns>:<prop>] | XMP set <ns>:<prop> <value>: the XMP packet of the current step (f62).
            "XMP" => {
                let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                let meta = &editor.history.current().source().meta;
                match (verb, rest.trim()) {
                    ("get", "") => {
                        let props = meta.xmp.as_ref().map(|x| x.properties()).unwrap_or_default();
                        let status = match (&meta.xmp_problem, &meta.xmp) {
                            (Some(_), _) => "malformed",
                            (None, Some(_)) => "ok",
                            _ => "none",
                        };
                        let list: Vec<String> = props
                            .iter()
                            .map(|p| {
                                format!("{}:{}{}={}", p.ns, p.name, if p.known { "" } else { "?" }, meta::escape_lines(&p.value).replace(' ', "_"))
                            })
                            .collect();
                        format!("OK status={status} props={} {}", props.len(), list.join(" ")).trim_end().to_string()
                    }
                    ("get", key) => match meta.get(&format!("xmp:{key}")) {
                        Some(v) if meta.xmp.as_ref().and_then(|x| x.split_key(key).ok().and_then(|(n, p)| x.get(&n, &p))).is_some() => {
                            format!("OK {}", meta::escape_lines(&v))
                        }
                        Some(_) => "ERR no such XMP property".to_string(),
                        None => format!("ERR {key:?} is not <namespace>:<property>"),
                    },
                    ("set", args) => {
                        let (key, value) = args.split_once(' ').unwrap_or((args, ""));
                        match self.apply_metadata(&[(format!("xmp:{key}"), meta::unescape_lines(value))]) {
                            Ok(changed) => format!("OK changed={}", u8::from(changed)),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                    _ => "ERR XMP get [<ns>:<prop>] | XMP set <ns>:<prop> <value>".to_string(),
                }
            }
            "META" => {
                let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                let meta = &editor.history.current().source().meta;
                match (verb, rest.trim()) {
                    ("get", "") => format!("OK {}", meta.summary()),
                    ("get", field) => match meta.get(field) {
                        Some(v) => format!("OK {}", meta::escape_lines(&v)),
                        None => format!("ERR no metadata field {field:?}"),
                    },
                    ("set", args) => {
                        let (field, value) = args.split_once(' ').unwrap_or((args, ""));
                        match self.apply_metadata(&[(field.to_string(), meta::unescape_lines(value))]) {
                            Ok(changed) => format!("OK changed={}", u8::from(changed)),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                    // META filter <text>: the rows whose name, label or value contains the text; also sets the panel's filter (f98).
                    ("filter", text) => {
                        let hits = wavcraft_core::meta_view::filter_rows(wavcraft_core::meta_view::rows(meta), text);
                        let names: Vec<String> = hits.iter().map(|r| r.key.replace(' ', "_")).collect();
                        self.meta_filter = text.to_string();
                        format!("OK {} {}", names.len(), names.join(" "))
                    }
                    _ => "ERR META get [field] | META set <field> <value> | META filter <text>".to_string(),
                }
            }
            "METASET" => self.metaset_command(rest),
            // CHNAME get | CHNAME set <ch> <name>: channel names (<ch> counts from 1); one undo step.
            "CHNAME" => {
                let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
                match verb {
                    "get" => format!(
                        "OK {}",
                        editor
                            .history
                            .current()
                            .source()
                            .meta
                            .chnames
                            .iter()
                            .map(|n| meta::escape_lines(n).replace(' ', "_"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    "set" => {
                        let (ch, name) = rest.trim_start().split_once(' ').unwrap_or((rest.trim(), ""));
                        match ch.parse::<usize>() {
                            Ok(n) => match self.apply_metadata(&[(format!("chname.{n}"), name.to_string())]) {
                                Ok(changed) => format!("OK changed={}", u8::from(changed)),
                                Err(e) => format!("ERR {e}"),
                            },
                            Err(_) => "ERR CHNAME set <channel from 1> <name>".to_string(),
                        }
                    }
                    _ => "ERR CHNAME get | CHNAME set <channel from 1> <name>".to_string(),
                }
            }
            "CLIP" if rest.starts_with("edit") => self.control_edit_clip(rest, ctx),
            "SESSION" if rest.trim() == "return" => match self.return_to_session(ctx) {
                Ok(s) => s,
                Err(e) => format!("ERR {e}"),
            },
            "SESSION" | "TRACK" | "CLIP" | "CLIPTYPE" | "CLIPPROP" | "VIEW" | "SMARKER" | "SPLAY" | "SSTOP" | "SREC" | "PUNCH" | "MASTER"
            | "AUTO" => {
                if cmd == "VIEW" {
                    self.cdv.show = false;
                }
                self.mtv.control(cmd, rest, self.doc.as_ref())
            }
            "CD" => {
                let current = self.editor.as_ref().map(|e| e.history.current().clone());
                let sessions = self.sessions.path.clone();
                self.cdv.control(rest, current, &sessions)
            }
            "MIXER" if matches!(rest.trim(), "show" | "hide") => {
                self.show_mixer = rest.trim() == "show";
                "OK".to_string()
            }
            "MIXER" => self.mixer.control(rest, self.mtv.mt.as_mut().map(|m| &mut m.session)),
            // f26: the bars as numbers; `METERS PUSH <l> <r>` feeds the tap as the audio callback
            // would (linear peaks), for a run without an output device; `METERS RESET` clears the lights.
            "METERS" => {
                let mut words = rest.split_whitespace();
                match words.next() {
                    Some("PUSH") => match (words.next().map(str::parse::<f32>), words.next().map(str::parse::<f32>)) {
                        (Some(Ok(l)), Some(Ok(r))) => self.player.push_levels([l, r]),
                        _ => return "ERR METERS PUSH <left> <right>, linear peaks".to_string(),
                    },
                    Some("RESET") => self.meters.levels.reset_clip(),
                    Some(_) => return "ERR METERS [PUSH <l> <r> | RESET]".to_string(),
                    None => {}
                }
                let peaks = self.meter_peaks();
                self.meters.update(peaks);
                format!(
                    "{} view={} armed={} input_open={}",
                    meters_ui::readings_line(&self.meters.levels),
                    if self.meters.input_view { "input" } else { "output" },
                    u8::from(self.meter_input),
                    u8::from(self.input.is_open())
                )
            }
            // f26: loudness of the selection (the whole file when none), or of `<start> <end>` frames.
            "LOUDNESS" => {
                let Some(e) = &self.editor else { return "ERR LOUDNESS needs a file".to_string() };
                let state = e.history.current().clone();
                let len = state.len_frames();
                let mut it = rest.split_whitespace().map(str::parse::<u64>);
                let (range, scope) = match (it.next(), it.next()) {
                    (Some(Ok(a)), Some(Ok(b))) => (a..b, "range"),
                    (None, _) if self.selection.is_empty() => (0..len, "file"),
                    (None, _) => (self.selection.start() as u64..self.selection.end() as u64, "selection"),
                    _ => return "ERR LOUDNESS [<start> <end>], in frames".to_string(),
                };
                match wavcraft_core::loudness::measure(&state, range, &AtomicBool::new(false), &mut |_| {}) {
                    Ok(m) => format!("OK scope={scope} {}", m.line()),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // `PREF get <key>` / `PREF set <key> <value>` (keys: see `settings::KEYS`). The document's
            // tempo and meter are `PREF set tempo <bpm>`, `PREF set meter <n/d>`, `PREF get tempo|meter`.
            "PREF" => {
                let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
                let (key, value) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));
                let value = value.trim();
                match (verb, key) {
                    // `PREF reset`: Restore Preference Defaults, already confirmed (the menu asks first).
                    ("reset", "") => self.reset_preferences().map_or_else(|e| format!("ERR {e}"), |m| format!("OK {m}")),
                    // `PREF show`: the session folder now and where it came from (for scripted checks).
                    ("show", "") => format!(
                        "OK session_folder={} source={:?} saved_session_folder={}",
                        self.sessions.path.display(),
                        self.sessions.source,
                        self.settings.session_folder.as_deref().map_or_else(String::new, |p| p.display().to_string())
                    ),
                    ("set", "language") if i18n::is_language(value) => {
                        self.set_language(value);
                        format!("OK language={}", i18n::language())
                    }
                    ("set", "language") => format!("ERR no language {value:?}; built in: {}", i18n::language_codes().join(", ")),
                    ("get", "language") => format!("OK language={}", i18n::language()),
                    ("set", "tempo") => match value.parse::<f64>().ok().and_then(|b| Tempo::new(b, self.tempo.numerator, self.tempo.denominator)) {
                        Some(t) => {
                            self.set_tempo(t);
                            format!("OK tempo={}", t.bpm_text())
                        }
                        None => "ERR PREF set tempo <bpm>, 20 to 400".to_string(),
                    },
                    ("set", "meter") => match Tempo::parse_meter(value).and_then(|(n, d)| Tempo::new(self.tempo.bpm, n, d)) {
                        Some(t) => {
                            self.set_tempo(t);
                            format!("OK meter={}", t.meter_text())
                        }
                        None => "ERR PREF set meter <n/d>, such as 3/4".to_string(),
                    },
                    ("get", "tempo") => format!("OK tempo={}", self.tempo.bpm_text()),
                    ("get", "meter") => format!("OK meter={}", self.tempo.meter_text()),
                    _ => match verb {
                        "get" => self.settings.get(key).map_or_else(|e| format!("ERR {e}"), |v| format!("OK {v}")),
                        "set" => self.pref_set(key, value).map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string()),
                        _ => "ERR PREF get <key> | PREF set <key> <value> | PREF reset | PREF show".to_string(),
                    },
                }
            }
            // `APPEARANCE preset save|load|delete <name>`, `APPEARANCE preset list`.
            "APPEARANCE" => {
                let (what, args) = rest.split_once(' ').unwrap_or((rest, ""));
                let (verb, name) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));
                match (what, verb) {
                    ("preset", "list") => format!("OK {}", self.settings.preset_names().join("|")),
                    ("preset", _) => self.preset_do(verb, name).map_or_else(|e| format!("ERR {e}"), |()| "OK".to_string()),
                    _ => "ERR APPEARANCE preset save|load|delete <name> | APPEARANCE preset list".to_string(),
                }
            }
            // The output as it is: host, buffer, channels and the maps.
            "AUDIOINFO" => {
                let p = &self.player;
                format!(
                    "OK host={} hosts={} buffer={} buffer_range={} out_channels={} output_map={} input_map={} profile={} device={}",
                    if p.host.is_empty() { "none" } else { &p.host },
                    crate::player::host_names().join("|"),
                    p.buffer.map_or("default".to_string(), |n| n.to_string()),
                    p.buffer_range.map_or("unknown".to_string(), |(a, b)| format!("{a}-{b}")),
                    p.out_channels,
                    self.settings.output_map,
                    self.settings.input_map,
                    self.profiles.active.as_deref().unwrap_or("none"),
                    p.device
                )
            }
            // `CHANMAP out <file ch> <output ch|off>`, `CHANMAP in <input ch> <record ch|off>`
            // (channels count from 1), `CHANMAP reset`, or `CHANMAP` alone to show both maps.
            "CHANMAP" => {
                let words: Vec<&str> = rest.split_whitespace().collect();
                let number = |w: &str| w.parse::<usize>().ok().filter(|n| *n >= 1);
                let result = match words.as_slice() {
                    [] => Ok(()),
                    ["reset"] => self.pref_set("output_map", "").and_then(|()| self.pref_set("input_map", "")),
                    [dir @ ("out" | "in"), from, to] => match (number(from), *to == "off" || *to == "0" || number(to).is_some()) {
                        (Some(f), true) => {
                            let (key, map) =
                                if *dir == "out" { ("output_map", &self.settings.output_map) } else { ("input_map", &self.settings.input_map) };
                            let text = map_text_with(map, f - 1, number(to).map(|t| t - 1), 0);
                            self.pref_set(key, &text)
                        }
                        _ => Err("channels count from 1; the target may be off".to_string()),
                    },
                    _ => Err("CHANMAP out <file ch> <output ch|off> | CHANMAP in <input ch> <record ch|off> | CHANMAP reset".to_string()),
                };
                match result {
                    Ok(()) => format!("OK output_map={} input_map={}", self.settings.output_map, self.settings.input_map),
                    Err(e) => format!("ERR {e}"),
                }
            }
            // `AUDIOPROFILE list` / `AUDIOPROFILE simulate <devices...>` (treat these names as the
            // devices on offer, as when an interface is plugged in or out).
            "AUDIOPROFILE" => {
                let words: Vec<String> = rest.split_whitespace().map(String::from).collect();
                match words.first().map(String::as_str) {
                    Some("list") => format!(
                        "OK active={} count={} setups={}",
                        self.profiles.active.as_deref().unwrap_or("none"),
                        self.profiles.profiles.len(),
                        self.profiles.profiles.iter().map(|p| format!("{}={}", p.key, p.label.replace(' ', "_"))).collect::<Vec<_>>().join(";")
                    ),
                    Some("simulate") if words.len() > 1 => {
                        self.profile_rx = None;
                        let names: Vec<String> = words[1..].iter().map(|w| w.replace('_', " ")).collect();
                        let outcome = self.activate_setup(&names, &names);
                        format!("OK outcome={outcome:?} active={}", self.profiles.active.as_deref().unwrap_or("none"))
                    }
                    _ => "ERR AUDIOPROFILE list | AUDIOPROFILE simulate <device names...> (_ stands for a space)".to_string(),
                }
            }
            // The devices the system offers, and the one the player is on.
            "DEVICES" => {
                let d = crate::player::list_devices(self.settings.audio_host.clone());
                format!("OK output={} input={} playing_on={}", d.output.join("|"), d.input.join("|"), self.player.device)
            }
            // Test-only slow job (roadmap B003): `JOB testslow <ms per 64K-frame chunk>` copies the
            // selection (the whole file with none) on the worker, so progress and Cancel can
            // be driven; `JOB cancel` stops it.
            "JOB" => match rest.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["testslow", ms] => match ms.parse::<u64>() {
                    Ok(ms) => match self.start_test_job(ms, ctx) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    },
                    Err(_) => "ERR JOB testslow <milliseconds per chunk>".to_string(),
                },
                ["cancel"] => match &self.running {
                    Some(r) => {
                        r.job.cancel();
                        "OK".to_string()
                    }
                    None => "ERR no job is running".to_string(),
                },
                _ => "ERR JOB testslow <ms> | JOB cancel".to_string(),
            },
            "MIXDOWN" => {
                let (mut selected, mut norm) = (false, None);
                for w in rest.split_whitespace() {
                    match w.split_once('=') {
                        None if w == "selected" => selected = true,
                        Some(("normalize", v)) => match v.parse::<f32>() {
                            Ok(db) => norm = Some(db),
                            Err(_) => return "ERR usage: MIXDOWN [selected] [normalize=<dB, 0 or lower>]".to_string(),
                        },
                        _ => return "ERR usage: MIXDOWN [selected] [normalize=<dB, 0 or lower>]".to_string(),
                    }
                }
                match self.start_mixdown(selected, norm, ctx) {
                    Ok(()) => "OK".to_string(),
                    Err(e) => format!("ERR {e}"),
                }
            }
            "SAVE_CANCEL" => match &self.saving {
                Some(Saving { cancel: Some(cancel), .. }) => {
                    cancel.store(true, Ordering::Relaxed);
                    "OK".to_string()
                }
                Some(_) => "ERR this save cannot be cancelled (only a WAV save of the whole file can)".to_string(),
                None => "ERR no save is running".to_string(),
            },
            // `EXPORT_RANGES <folder> [pattern=<p>]`: every range marker as its own WAV (f79).
            "EXPORT_RANGES" => {
                let (folder, pattern) = match rest.split_once(" pattern=") {
                    Some((f, p)) => (f.trim(), p.trim()),
                    None => (rest.trim(), DEFAULT_RANGE_PATTERN),
                };
                if folder.is_empty() {
                    "ERR EXPORT_RANGES <folder> [pattern=<p>]".to_string()
                } else {
                    match self.start_export_ranges(ctx, PathBuf::from(folder), pattern.to_string(), false) {
                        Ok(()) => "OK".to_string(),
                        Err(e) => format!("ERR {e}"),
                    }
                }
            }
            "BATCH" => self.batch.control(rest, &self.sessions.path, ctx),
            "SPECTRAL" => self.control_spectral(rest),
            "STATUSBAR" => self.control_statusbar(rest),
            // The open documents, in the order opened, and the current one.
            "DOCS" => {
                let n = self.docs.len(self.editor.is_some());
                let mut out = format!("OK count={n} current={}", self.docs.current());
                for i in 0..n {
                    let (name, unsaved, frames) = self.doc_info(i);
                    out.push_str(&format!(" | {i} unsaved={} frames={frames} {name}", u8::from(unsaved)));
                }
                out
            }
            // A new empty document: `NEWDOC rate=48000 ch=2 bits=32` (all optional; bits 16, 24, 32 float).
            "NEWDOC" => {
                let mut d = NewAudio::default();
                let mut bad = None;
                for kv in rest.split_whitespace() {
                    match kv.split_once('=') {
                        Some(("rate", v)) => d.rate = v.parse().unwrap_or(0),
                        Some(("ch", v)) => d.channels = v.parse().unwrap_or(0),
                        Some(("bits", v)) => d.bits = v.parse().unwrap_or(0),
                        _ => bad = Some(kv),
                    }
                }
                match bad {
                    Some(kv) => format!("ERR NEWDOC rate=<hz> ch=<n> bits=<16|24|32>, not {kv:?}"),
                    None => match self.new_audio_file(d.rate, d.channels, d.bits, ctx) {
                        Ok(()) => format!("OK count={} current={}", self.docs.len(true), self.docs.current()),
                        Err(e) => format!("ERR {e}"),
                    },
                }
            }
            "FILES" => self.files_request(rest),
            "RECENT" => self.recent_request(rest),
            // Add a file to the Files panel without opening it (f63).
            "IMPORT" if !rest.is_empty() => match self.import_file(PathBuf::from(rest)) {
                Ok(()) => format!("OK listed={} open={}", self.imports.len(), self.docs.len(self.editor.is_some())),
                Err(e) => format!("ERR {e}"),
            },
            "SWITCH" => match rest.trim().parse::<usize>().map_err(|_| "SWITCH <index>, from DOCS".to_string()).and_then(|i| self.switch_doc(i)) {
                Ok(()) => format!("OK current={}", self.docs.current()),
                Err(e) => format!("ERR {e}"),
            },
            // Close the current document; with unsaved changes `ERR …` unless `CLOSE discard`.
            "CLOSE" if self.unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or CLOSE discard".to_string(),
            "CLOSE" if self.editor.is_none() => "ERR no file is open".to_string(),
            "CLOSE" if self.saving.is_some() => "ERR wait for the save to finish".to_string(),
            "CLOSE" => {
                self.remove_current();
                format!("OK count={}", self.docs.len(self.editor.is_some()))
            }
            // The zoom and scroll, for scripted checks: the start frame and frames per pixel.
            "VIEWPORT" => {
                let mut it = rest.split_whitespace().map(str::parse::<f64>);
                match (it.next(), it.next(), self.doc.as_ref().map(Document::frames)) {
                    (Some(Ok(start)), Some(Ok(fpp)), Some(frames)) if start.is_finite() && fpp.is_finite() && fpp > 0.0 => {
                        self.view = Viewport { start, frames_per_px: fpp };
                        self.view.clamp(self.editor_width.max(1.0), frames);
                        format!("OK view={:.3}+{:.6}", self.view.start, self.view.frames_per_px)
                    }
                    _ => "ERR VIEWPORT <start frame> <frames per pixel>, with a file open".to_string(),
                }
            }
            // f33: `MATCH [target=<LUFS>] [tp=<dBTP>] [limit=on|off] [tol=<LU>] [lookahead=<ms>]
            // [release=<ms>] [preset=podcast|broadcast]`: Match Loudness on the whole file, one step.
            "MATCH" => {
                if let Some(why) = self.edit_lock() {
                    return format!("ERR {why}");
                }
                if self.running.is_some() || self.saving.is_some() {
                    return "ERR wait for the current job to finish".to_string();
                }
                let Some(e) = &mut self.editor else { return "ERR MATCH needs a file".to_string() };
                let mut t = Target::PODCAST;
                for word in rest.split_whitespace() {
                    let (k, v) = word.split_once('=').unwrap_or((word, ""));
                    let num = v.parse::<f64>();
                    match (k, num) {
                        ("preset", _) => match v {
                            "podcast" => t = Target::PODCAST,
                            "broadcast" => t = Target::BROADCAST,
                            _ => return "ERR MATCH preset=podcast|broadcast".to_string(),
                        },
                        ("target", Ok(n)) => t.lufs = n,
                        ("tp", Ok(n)) => t.max_true_peak_dbtp = n,
                        ("tol", Ok(n)) => t.tolerance_lu = n,
                        ("lookahead", Ok(n)) => t.look_ahead_ms = n,
                        ("release", Ok(n)) => t.release_ms = n,
                        ("limit", _) if v == "on" || v == "off" => t.limit = v == "on",
                        _ => return format!("ERR MATCH: cannot read {word:?}; keys target tp tol limit lookahead release preset"),
                    }
                }
                let state = e.history.current().clone();
                match wavcraft_core::match_loudness::run(&state, &t, None, &AtomicBool::new(false), &mut |_| {}) {
                    Ok(r) => {
                        let line = format!(
                            "OK gain={:.2} limited={} on_target={} before={:.2} after={:.2} true_peak={:.2} changed={}",
                            r.gain_db,
                            u8::from(r.limited),
                            u8::from(r.on_target),
                            r.before.integrated_lufs,
                            r.after.integrated_lufs,
                            r.after.true_peak_dbtp,
                            u8::from(!r.edit.is_noop())
                        );
                        if !r.edit.is_noop() {
                            e.history.push_edit(r.edit);
                            self.notice = Some(r.message);
                            self.rebuild(ctx);
                        }
                        line
                    }
                    Err(err) => format!("ERR {err}"),
                }
            }
            "SAVE" => {
                // `SAVE <path> [bits=16|24|32] [float=0|1] [aiff=..] [quality=-1..10] [meta=0|1]`: the options end the line.
                let meta = match take_meta(rest) {
                    Ok(m) => m,
                    Err(e) => return format!("ERR {e}"),
                };
                let quality = match take_quality(rest) {
                    Ok(q) => q,
                    Err(e) => return format!("ERR {e}"),
                };
                match parse_save_args(rest) {
                    Err(e) => format!("ERR {e}"),
                    Ok((path, ty, aiff)) => {
                        if let Some(q) = quality {
                            self.vorbis_quality = q;
                        }
                        if let Some(m) = meta {
                            self.include_meta = m;
                        }
                        let to = (!path.is_empty()).then(|| PathBuf::from(path));
                        // `aiff=` is for this save only.
                        let kept = self.aiff_kind;
                        self.aiff_kind = aiff.unwrap_or(kept);
                        let r = self.save_with(ctx, to.is_some(), to, false, ty.or(self.save_type), ty.is_some());
                        self.aiff_kind = kept;
                        match r {
                            Ok(()) => "OK".to_string(),
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                }
            }
            "SAVE_SELECTION" | "EXPORT" if !rest.is_empty() => match self.save_copy(ctx, cmd == "SAVE_SELECTION", Some(PathBuf::from(rest))) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {e}"),
            },
            "SAVE_SELECTION" | "EXPORT" => format!("ERR {cmd} <path>"),
            "SAVEOPTS" => self.control_saveopts(ctx, rest),
            "EXPORTPRESET" => self.control_export_preset(rest),
            // A plain QUIT refuses with unsaved changes; `QUIT discard` closes anyway.
            // RECORD start [rate=48000 ch=1 bits=24] [inputs=1,2,3,4] [max=<s>] [in=<s>|at=<unix s>] | quick | pause | resume | cancel | stop
            "RECORD" => {
                let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
                match verb {
                    "start" => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                        let parsed = crate::recording::parse_start_args(args, now);
                        self.rec.monitor_override = parsed.as_ref().ok().and_then(|(_, o)| o.monitor);
                        match parsed {
                            Ok((f, o)) if o.delay_secs.is_some_and(|d| d > 0) => {
                                match self.rec.arm(f, std::time::Duration::from_secs(o.delay_secs.unwrap_or(0)), o.max_secs) {
                                    Ok(()) => format!("OK armed in={}", o.delay_secs.unwrap_or(0)),
                                    Err(e) => format!("ERR {e}"),
                                }
                            }
                            Ok((f, o)) if o.mode.is_some() => {
                                let mode = o.mode.unwrap_or(RecMode::Insert);
                                match self.start_recording_into(f, mode, o.target.unwrap_or_default(), o.preroll_ms.unwrap_or(DEFAULT_PREROLL_MS)) {
                                    Ok(path) => format!("OK path={}", path.display()),
                                    Err(e) => format!("ERR {e}"),
                                }
                            }
                            Ok((f, o)) => match self.start_recording_max(f, o.max_secs) {
                                Ok(path) => format!("OK path={}", path.display()),
                                Err(e) => format!("ERR {e}"),
                            },
                            Err(e) => format!("ERR {e}"),
                        }
                    }
                    "quick" => match self.quick_record() {
                        Ok(path) => format!("OK path={}", path.display()),
                        Err(e) => format!("ERR {e}"),
                    },
                    "pause" | "resume" => {
                        if !self.rec.is_active() {
                            "ERR not recording".to_string()
                        } else if self.rec.set_paused(verb == "pause") {
                            "OK".to_string()
                        } else {
                            format!("ERR already {}", if verb == "pause" { "paused" } else { "recording" })
                        }
                    }
                    "cancel" if self.rec.disarm() => "OK".to_string(),
                    "cancel" => "ERR nothing is waiting to start".to_string(),
                    "stop" => match self.finish_recording(true, ctx) {
                        Some(t) if t.error.is_none() => format!("OK frames={} dropped={} path={}", t.frames, t.dropped_frames, t.path.display()),
                        Some(t) => format!("ERR the take stopped early: {} (kept at {})", t.error.unwrap_or_default(), t.path.display()),
                        None => "ERR not recording".to_string(),
                    },
                    _ => "ERR RECORD start [rate=48000 ch=1 bits=24] [inputs=1,2,3,4] [max=<s>] [in=<s>|at=<unix s>] | quick | pause | resume | cancel | stop"
                        .to_string(),
                }
            }
            // `I18N missing` lists the keys the current language lacks; `I18N get <key>` is a key's text now.
            "I18N" => match rest.split_once(' ').unwrap_or((rest, "")) {
                ("missing", "") => {
                    let m = i18n::missing();
                    format!("OK {} {}", m.len(), m.join(","))
                }
                ("get", key) if !key.is_empty() => format!("OK {}", i18n::tr(key)),
                _ => "ERR I18N missing | I18N get <key>".to_string(),
            },
            "QUIT" if self.any_unsaved() && rest != "discard" => "ERR unsaved changes: SAVE first, or QUIT discard".to_string(),
            "QUIT" => {
                self.finish_recording(false, ctx);
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                "OK".to_string()
            }
            _ => format!("ERR unknown request {cmd:?}"),
        }
    }

    /// Start the playlist from its first item (panel button or control channel).
    fn play_playlist(&mut self) -> Result<(), String> {
        if self.editor.is_none() {
            return Err("no file is open".to_string());
        }
        self.halt_play();
        let Some(editor) = self.editor.as_ref() else { return Err("no file is open".to_string()) };
        let doc = editor.history.current();
        if self.playlist_ui.start(doc.markers(), doc.len_frames()) { Ok(()) } else { Err("the playlist has no range to play".to_string()) }
    }

    /// The `PLAYLIST` control verb (see `control_request`).
    fn control_playlist(&mut self, rest: &str) -> String {
        let Some(editor) = self.editor.as_ref() else { return "ERR no file is open".to_string() };
        if rest.trim() == "play" {
            return match self.play_playlist() {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {e}"),
            };
        }
        let doc = editor.history.current();
        let (markers, frames) = (doc.markers().clone(), doc.len_frames());
        self.playlist_ui.control(rest, &markers, frames)
    }

    /// The `MARKER` control verb (see `control_request`).
    fn control_marker(&mut self, rest: &str) -> String {
        const USAGE: &str =
            "ERR MARKER rename <i> <name> | move <i> <frame> | select <i> [add] | merge <i> <j> ... | color <i> <n> | desc <i> <text> | info <i>";
        if self.saving.is_some() {
            return "ERR wait for the save to finish".to_string();
        }
        if let Some(list) = rest.strip_prefix("merge ") {
            let Some(editor) = self.editor.as_mut() else { return "ERR no file is open".to_string() };
            let markers = editor.history.current().markers().clone();
            let ids: Option<Vec<u64>> = list.split_whitespace().map(|i| i.parse::<usize>().ok().and_then(|i| markers.id_at(i))).collect();
            return match ids {
                Some(ids) if editor.merge_markers(&ids) => "OK".to_string(),
                Some(_) => "ERR need two or more different markers, and no other marker may already have the merged extent".to_string(),
                None => USAGE.to_string(),
            };
        }
        let mut it = rest.splitn(3, ' ');
        let (verb, index, arg) = (it.next().unwrap_or(""), it.next().and_then(|i| i.parse::<usize>().ok()), it.next().unwrap_or("").trim());
        let Some(editor) = self.editor.as_mut() else { return "ERR no file is open".to_string() };
        let markers = editor.history.current().markers().clone();
        let Some(index) = index else { return USAGE.to_string() };
        let Some(id) = markers.id_at(index) else { return format!("ERR no marker {index} (there are {})", markers.len()) };
        match verb {
            "rename" if !arg.is_empty() => {
                if editor.rename_marker(id, arg) {
                    "OK".to_string()
                } else {
                    "ERR the name is empty or unchanged".to_string()
                }
            }
            "move" => match arg.parse::<u64>() {
                Ok(frame) if editor.move_marker(id, frame) => "OK".to_string(),
                Ok(_) => "ERR the marker would not move (same place, or another marker is there)".to_string(),
                Err(_) => USAGE.to_string(),
            },
            "color" => match arg.parse::<u8>() {
                Ok(n) if editor.set_marker_color(id, n) => "OK".to_string(),
                Ok(_) => format!("ERR colour is 0 (none) to {}, and must differ from the current one", wavcraft_core::PALETTE.len()),
                Err(_) => USAGE.to_string(),
            },
            "desc" => {
                if editor.set_marker_description(id, arg) {
                    "OK".to_string()
                } else {
                    "ERR the description is unchanged".to_string()
                }
            }
            "info" => {
                markers.iter().nth(index).map_or_else(String::new, |m| format!("OK color={} desc={}", m.color, m.description.replace('\n', " ")))
            }
            "select" if arg.is_empty() || arg == "add" => {
                if let Some(span) = self.markers_ui.click(&markers, index, false, arg == "add") {
                    self.selection = Selection { anchor: span.start as usize, head: span.end as usize };
                }
                format!("OK selection={}-{}", self.selection.start(), self.selection.end())
            }
            _ => USAGE.to_string(),
        }
    }

    /// The Edit menu, built from the action table: labels that name the step, Audition's keys.
    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let sel = self.selection_range();
        let mac = cfg!(target_os = "macos");
        let mut chosen = None;
        // The Select and Enable Channels groups are submenus; the rest stays flat.
        let grouped = |id: ActionId| {
            matches!(
                id,
                ActionId::SelectAll
                    | ActionId::DeselectAll
                    | ActionId::ClearTimeSelection
                    | ActionId::LeftChannel
                    | ActionId::RightChannel
                    | ActionId::BothChannels
            )
        };
        for a in TABLE.iter().filter(|a| a.name.starts_with("edit.") && !grouped(a.id)) {
            if matches!(a.id, ActionId::Cut | ActionId::Clipboard1 | ActionId::Delete | ActionId::Crop | ActionId::ZeroInward | ActionId::SetIn) {
                ui.separator();
            }
            let (enabled, label) =
                self.editor.as_ref().map_or((false, tr(&format!("action.{}", a.name))), |e| (e.enabled(a.id, &sel), Self::action_text(e, a)));
            let enabled = enabled && self.saving.is_none() && self.running.is_none();
            let keys = self.keymap.menu_text(Command::Action(a.id), mac);
            let tip = self.keymap.tooltip(Command::Action(a.id), &label, mac);
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).on_hover_text(tip).clicked() {
                chosen = Some(a.id);
            }
        }
        ui.separator();
        let enabled_for =
            |s: &Self, id: ActionId| s.saving.is_none() && s.running.is_none() && s.editor.as_ref().is_some_and(|e| e.enabled(id, &sel));
        let item = |s: &Self, ui: &mut egui::Ui, id: ActionId, chosen: &mut Option<ActionId>| {
            let a = actions::action(id);
            let label = s.editor.as_ref().map_or_else(|| a.menu.to_string(), |e| e.label(id));
            let keys = s.keymap.menu_text(Command::Action(id), mac);
            let tip = s.keymap.tooltip(Command::Action(id), &label, mac);
            if ui.add_enabled(enabled_for(s, id), egui::Button::new(label).shortcut_text(keys)).on_hover_text(tip).clicked() {
                *chosen = Some(id);
                ui.close();
            }
        };
        ui.menu_button("Select", |ui| {
            for id in [
                ActionId::SelectAll,
                ActionId::SelectCurrentView,
                ActionId::DeselectAll,
                ActionId::ClearTimeSelection,
                ActionId::SelectToStart,
                ActionId::SelectToEnd,
            ] {
                item(self, ui, id, &mut chosen);
            }
        });
        ui.menu_button("Enable Channels", |ui| {
            for id in [ActionId::LeftChannel, ActionId::RightChannel, ActionId::BothChannels] {
                item(self, ui, id, &mut chosen);
            }
            ui.separator();
            let mut on = self.settings.context_channel_editing;
            if ui.checkbox(&mut on, "Select Channel by Drag Position").changed() {
                self.settings.context_channel_editing = on;
                if let Err(e) = self.save_settings() {
                    log::warn!(target: "wavcraft", "{e}");
                }
            }
        });
        ui.separator();
        for a in TABLE.iter().filter(|a| a.name.starts_with("marker.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none();
            let keys = self.keymap.menu_text(Command::Action(a.id), mac);
            if ui.add_enabled(enabled, egui::Button::new(a.menu).shortcut_text(keys)).clicked() {
                chosen = Some(a.id);
            }
        }
        ui.separator();
        if ui.add_enabled(self.editor.is_some() && self.saving.is_none(), egui::Button::new("Mix Paste…")).clicked() {
            self.mix_paste = Some(mixpaste::Dialog::new());
            ui.close();
        }
        if chosen == Some(ActionId::ExportRanges) {
            let mut dialog = rfd::FileDialog::new().set_title("Export range markers to folder");
            if let Some(dir) = self.doc_path.parent().filter(|d| !d.as_os_str().is_empty()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(folder) = dialog.pick_folder()
                && let Err(e) = self.start_export_ranges(ui.ctx(), folder, DEFAULT_RANGE_PATTERN.to_string(), true)
            {
                self.error = Some(e);
            }
        } else if let Some(id) = chosen {
            let _ = self.run_action(id, ui.ctx());
        }
        ui.separator();
        if ui.button("Keyboard Shortcuts…").clicked() {
            self.shortcut_dialog = Some(ShortcutDialog::default());
        }
        if ui.add(egui::Button::new("Preferences…").shortcut_text(if mac { "⌘," } else { "Ctrl+," })).clicked() {
            self.open_preferences();
        }
    }

    /// Apply a shortcut change and save the key file (f46, f60). `Err` says what went wrong (a
    /// conflict, or the file not saved: the change still holds until the app closes).
    fn change_shortcuts(&mut self, change: Change) -> Result<String, String> {
        let edits_keys = matches!(change, Change::Assign { .. } | Change::Clear(_));
        let outcome = match change {
            Change::Assign { command, chord, reassign: false } => self
                .keymap
                .assign(command, chord)
                .map(|()| format!("{} is {}", command.name(), chord.to_ascii()))
                .map_err(|c| format!("conflict: {} is already {}", c.chord.to_ascii(), c.holder.name()))?,
            Change::Assign { command, chord, reassign: true } => match self.keymap.assign_reassign(command, chord) {
                Some(old) => format!("{} is {} (taken from {})", command.name(), chord.to_ascii(), old.name()),
                None => format!("{} is {}", command.name(), chord.to_ascii()),
            },
            Change::Clear(command) => {
                self.keymap.clear(command);
                format!("{} has no chord", command.name())
            }
            Change::Reset => {
                self.keymap.reset();
                self.set_active_set(None);
                "default shortcuts restored".to_string()
            }
            Change::SaveSet(name) => {
                let store = self.shortcut_sets.as_ref().ok_or("no config folder to keep sets in")?;
                let name = store.save(&name, &self.keymap)?;
                self.set_active_set(Some(&name));
                format!("saved the shortcut set {name}")
            }
            Change::SwitchSet(name) => {
                let store = self.shortcut_sets.as_ref().ok_or("no config folder to keep sets in")?;
                let map = store.load(&name)?;
                self.keymap = map;
                let active = (!name.eq_ignore_ascii_case(DEFAULT_SET)).then_some(name.as_str());
                self.set_active_set(active);
                format!("switched to the shortcut set {}", if active.is_some() { name.as_str() } else { DEFAULT_SET })
            }
            Change::DeleteSet(name) => {
                let store = self.shortcut_sets.as_ref().ok_or("no config folder to keep sets in")?;
                let was_active = store.active().as_deref() == Some(name.as_str());
                store.delete(&name)?;
                if was_active {
                    // The set in use is gone: back to the shipped table.
                    self.keymap.reset();
                    self.set_active_set(None);
                }
                format!("deleted the shortcut set {name}")
            }
        };
        // Edits to the set in use are kept in it.
        if edits_keys
            && let (Some(store), Some(active)) = (&self.shortcut_sets, self.shortcut_sets.as_ref().and_then(SetStore::active))
            && let Err(e) = store.save(&active, &self.keymap)
        {
            log::warn!(target: "wavcraft", "{e}");
        }
        if let Err(e) = self.keymap.check() {
            log::error!(target: "wavcraft", "keyboard shortcuts conflict: {e}");
        }
        if let Some(path) = &self.shortcuts_path
            && let Err(e) = wavcraft_core::shortcuts::save(path, &self.keymap)
        {
            return Err(format!("changed, but the key file {} could not be saved: {e}", path.display()));
        }
        Ok(outcome)
    }

    /// Remember which set is in use (`None`: the default). A failure is logged, not fatal.
    fn set_active_set(&self, name: Option<&str>) {
        if let Some(store) = &self.shortcut_sets
            && let Err(e) = store.set_active(name)
        {
            log::warn!(target: "wavcraft", "could not remember the active shortcut set: {e}");
        }
    }

    /// `SHORTCUTSETS`, `SHORTCUTSET save|switch|delete <name>` and `KEYBOARD <mods>` on the
    /// control channel (see `control.rs`).
    fn shortcut_set_request(&mut self, verb: &str, rest: &str) -> String {
        match verb {
            "SHORTCUTSETS" => {
                let Some(store) = &self.shortcut_sets else { return "ERR no config folder to keep sets in".to_string() };
                let active = store.active().unwrap_or_else(|| DEFAULT_SET.to_string());
                format!("OK active={active} sets={}", store.list().join("|"))
            }
            "KEYBOARD" => match Mods::parse(rest) {
                Ok(mods) => {
                    let keys: Vec<String> = self.keymap.keys_with(mods).iter().map(|(k, c)| format!("{k}={}", c.name())).collect();
                    format!("OK mods={} keys={}", mods.text(), keys.join(" "))
                }
                Err(e) => format!("ERR {e}"),
            },
            _ => {
                let (action, name) = rest.split_once(char::is_whitespace).map(|(a, n)| (a, n.trim())).unwrap_or((rest.trim(), ""));
                let change = match action {
                    "save" => Ok(Change::SaveSet(name.to_string())),
                    "switch" => Ok(Change::SwitchSet(name.to_string())),
                    "delete" => Ok(Change::DeleteSet(name.to_string())),
                    _ => Err("SHORTCUTSET save|switch|delete <name>".to_string()),
                };
                match change.and_then(|c| self.change_shortcuts(c)) {
                    Ok(m) => format!("OK {m}"),
                    Err(e) => format!("ERR {e}"),
                }
            }
        }
    }

    /// `SHORTCUTS` and `SHORTCUT set|clear|reset` on the control channel (see `control.rs`).
    fn shortcut_request(&mut self, verb: &str, rest: &str) -> String {
        if verb == "SHORTCUTS" {
            let mut out = String::from("OK");
            for command in Command::all() {
                let chords: Vec<String> = self.keymap.chords(command).map(|c| c.to_ascii()).collect();
                out.push_str(&format!(" {}={}", command.name(), if chords.is_empty() { "-".to_string() } else { chords.join("|") }));
            }
            return out;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        let find = |name: &str| Command::from_name(name).ok_or_else(|| format!("no command named {name:?}"));
        let change = match words.as_slice() {
            ["reset"] => Ok(Change::Reset),
            ["clear", name] => find(name).map(Change::Clear),
            ["set", name, chord] | ["set", name, chord, "reassign"] => {
                find(name).and_then(|command| Shortcut::parse(chord).map(|chord| Change::Assign { command, chord, reassign: words.len() == 4 }))
            }
            _ => Err("SHORTCUT set <command> <chord> [reassign] | clear <command> | reset".to_string()),
        };
        match change.and_then(|c| self.change_shortcuts(c)) {
            Ok(m) => format!("OK {m}"),
            Err(e) => format!("ERR {e}"),
        }
    }

    /// Edit › Keyboard Shortcuts… (f46, f60), while it is open.
    fn shortcut_dialog_ui(&mut self, ctx: &egui::Context) {
        if let Some(mut dialog) = self.shortcut_dialog.take() {
            let sets = self.shortcut_sets.as_ref().map(SetStore::list).unwrap_or_default();
            let active = self.shortcut_sets.as_ref().and_then(SetStore::active);
            let (open, change) = dialog.show(ctx, &self.keymap, cfg!(target_os = "macos"), &sets, active.as_deref());
            if let Some(change) = change
                && let Err(e) = self.change_shortcuts(change)
            {
                self.error = Some(e);
            }
            if open {
                self.shortcut_dialog = Some(dialog);
            }
        }
    }

    /// The Effects menu: one entry per registered effect, run on the selection (the whole file
    /// when nothing is selected) and the enabled channels.
    fn effects_menu(&mut self, ui: &mut egui::Ui) {
        let sel = self.selection_range();
        let mut chosen = None;
        for a in actions::all_actions().filter(|a| a.name.starts_with("effect.")) {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel)) && self.saving.is_none() && self.running.is_none();
            // An effect with settings opens its dialog (a "…" entry); one without runs at once.
            let has_dialog =
                matches!(a.id, ActionId::Effect(i) if wavcraft_core::effects::all().get(usize::from(i)).is_some_and(|e| !e.parameters().is_empty()));
            let text = if has_dialog { format!("{}…", a.menu) } else { a.menu.to_string() };
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                chosen = Some((a.id, has_dialog));
            }
        }
        ui.separator();
        if ui.button("Effects Rack…").clicked() {
            self.rack.open = !self.rack.open;
            ui.close();
        }
        if ui.button("Diagnostics…").clicked() {
            self.diag.open = true;
            ui.close();
        }
        // Diagnostics: DeClipper runs at once with its defaults (no dialog yet).
        ui.separator();
        let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(ActionId::DeClip, &sel)) && self.saving.is_none() && self.running.is_none();
        if ui.add_enabled(enabled, egui::Button::new("DeClipper (defaults)")).clicked() {
            chosen = Some((ActionId::DeClip, false));
        }
        for (id, text) in [(ActionId::DeClick, "DeClicker (defaults)"), (ActionId::AutoClick, "Automatic Click Remover (defaults)")] {
            let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(id, &sel)) && self.saving.is_none() && self.running.is_none();
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                chosen = Some((id, false));
            }
        }
        match chosen {
            Some((ActionId::Effect(i), true)) => {
                // Start from the settings this effect last ran with, if it was the last command.
                let last = self.editor.as_ref().and_then(|e| match &e.last {
                    Some(Repeat::Effect(j, p)) if *j == i => Some(p.clone()),
                    _ => None,
                });
                self.effect_dialog = EffectDialog::new(i, last);
                ui.close();
            }
            Some((id, _)) => {
                let _ = self.run_action(id, ui.ctx());
            }
            None => {}
        }
    }

    /// File › Open Recent: the last files opened, a missing one greyed (clicking it takes it
    /// off the list and says so), and Clear Recent.
    fn recent_menu(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        if self.recent.paths().is_empty() {
            ui.add_enabled(false, egui::Button::new("(none)"));
        }
        for (i, p) in self.recent.paths().iter().enumerate() {
            let text = egui::RichText::new(path_name(p));
            let missing = !p.is_file();
            let hover = if missing { format!("{} (missing)", p.display()) } else { p.display().to_string() };
            if ui.button(if missing { text.weak() } else { text }).on_hover_text(hover).clicked() {
                chosen = Some(i);
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(!self.recent.paths().is_empty(), egui::Button::new("Clear Recent")).clicked() {
            self.recent.clear();
            self.save_recent();
            ui.close();
        }
        if let Some(i) = chosen
            && let Err(e) = self.open_recent(i)
        {
            self.error = Some(e);
        }
    }

    /// Switch the interface language and keep it in the settings file.
    fn set_language(&mut self, code: &str) {
        if !i18n::set_language(code) {
            return;
        }
        self.settings.language = (code != i18n::ENGLISH).then(|| code.to_string());
        match &self.settings_path {
            Some(file) => {
                if let Err(e) = self.settings.save(file) {
                    log::warn!(target: "wavcraft", "could not save the language to {}: {e}", file.display());
                }
            }
            None => log::warn!(target: "wavcraft", "this system has no settings folder, so the language lasts until WavCraft closes"),
        }
    }

    /// An Edit-menu item's text in the current language: the table's text, or the step-naming
    /// forms ("Undo Cut", "Repeat Paste", "Both Channels") built from translated parts. The
    /// history step's own name stays as the history gives it.
    fn action_text(e: &actions::Editor, a: &actions::Action) -> String {
        let plain = tr(&format!("action.{}", a.name));
        let label = e.label(a.id);
        if label == a.menu {
            return plain;
        }
        if label == "Both Channels" {
            return tr("action.both_channels");
        }
        if label == "Repeat Insert Silence" {
            return tr("action.repeat_insert_silence");
        }
        if let Some(step) = label.strip_prefix("Undo ") {
            return tr_args("action.undo_step", &[step]);
        }
        if let Some(step) = label.strip_prefix("Redo ") {
            return tr_args("action.redo_step", &[step]);
        }
        if let Some(menu) = label.strip_prefix("Repeat ")
            && let Some(done) = TABLE.iter().find(|t| t.menu == menu)
        {
            return tr_args("action.repeat_step", &[&tr(&format!("action.{}", done.name))]);
        }
        label
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("menu.file"), |ui| {
                let open = format!("Open…  {}", self.keymap.menu_text(Command::Open, cfg!(target_os = "macos")));
                if ui.button(open).clicked() {
                    self.pick_file(ui.ctx());
                }
                ui.menu_button("Open Recent", |ui| self.recent_menu(ui));
                ui.menu_button("Open Append", |ui| {
                    let free = self.appending.is_none() && self.making.is_none() && self.saving.is_none();
                    if ui.add_enabled(self.editor.is_some() && free, egui::Button::new("To Current…")).clicked() {
                        ui.close();
                        self.pick_append(false, ui.ctx());
                    }
                    if ui.add_enabled(free, egui::Button::new("To New…")).clicked() {
                        ui.close();
                        self.pick_append(true, ui.ctx());
                    }
                });
                ui.menu_button("New", |ui| {
                    if ui.button("Audio File…").clicked() {
                        self.new_audio = Some(NewAudio::default());
                        ui.close();
                    }
                });
                let mac = cfg!(target_os = "macos");
                let has_doc = self.editor.is_some() && self.saving.is_none();
                let (save, save_as) = (self.keymap.menu_text(Command::Save, mac), self.keymap.menu_text(Command::SaveAs, mac));
                if ui.add_enabled(has_doc, egui::Button::new(tr("file.save")).shortcut_text(save)).clicked()
                    && let Err(e) = self.save(ui.ctx(), false, None, false)
                {
                    self.error = Some(e);
                }
                if ui.add_enabled(has_doc, egui::Button::new(tr("file.save_as")).shortcut_text(save_as)).clicked() {
                    self.open_export_dialog();
                    ui.close();
                }
                if ui.add_enabled(has_doc, egui::Button::new("Close").shortcut_text(self.keymap.menu_text(Command::Close, mac))).clicked() {
                    ui.close();
                    if let Err(e) = self.close_doc() {
                        self.error = Some(e);
                    }
                }
                ui.menu_button("Sample Type on Save", |ui| {
                    ui.radio_value(&mut self.save_type, None, "Keep the file's own");
                    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
                        ui.radio_value(&mut self.save_type, Some(ty), ty.describe());
                    }
                    ui.label("FLAC takes 16 or 24 bit; it uses 24 bit for the others.");
                });
                let mut meta = self.include_meta;
                if ui
                    .add_enabled(has_doc, egui::Checkbox::new(&mut meta, "Include markers and other metadata"))
                    .on_hover_text("Applies to Save and Save As for this file (WAV); off writes only the audio")
                    .changed()
                {
                    self.set_include_meta(meta);
                }
                let has_sel = has_doc && self.selection.start() < self.selection.end();
                if ui.add_enabled(has_sel, egui::Button::new("Save Selection As…")).clicked()
                    && let Err(e) = self.save_copy(ui.ctx(), true, None)
                {
                    self.error = Some(e);
                }
                if ui.add_enabled(has_doc, egui::Button::new("Close")).clicked()
                    && let Err(e) = self.close_doc()
                {
                    self.error = Some(e);
                }
                ui.menu_button("Import", |ui| {
                    if ui.button("File…").on_hover_text("Add files to the Files panel without opening them").clicked() {
                        self.pick_import();
                        ui.close();
                    }
                    if ui.button("Raw Data…").on_hover_text("Open a file with no header: say its rate, channels and sample format").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.raw_import = Some(RawDialog::new(path));
                        }
                        ui.close();
                    }
                });
                ui.menu_button("Export", |ui| {
                    if ui.add_enabled(has_doc, egui::Button::new("File…")).clicked()
                        && let Err(e) = self.save_copy(ui.ctx(), false, None)
                    {
                        self.error = Some(e);
                    }
                });
                ui.menu_button("New", |ui| {
                    if ui.button("Multitrack Session…").clicked() {
                        self.mtv.open_dialog();
                        ui.close();
                    }
                });
                let has_session = self.mtv.mt.is_some();
                if ui.add_enabled(has_session, egui::Button::new("Save Session")).clicked() {
                    self.mtv.save_session_dialog(false);
                    ui.close();
                }
                if ui.add_enabled(has_session, egui::Button::new("Save Session As…")).clicked() {
                    self.mtv.save_session_dialog(true);
                    ui.close();
                }
                if ui.button("Open Session…").clicked() {
                    self.mtv.open_session_dialog();
                    ui.close();
                }
                let busy = self.saving.is_some() || self.making.is_some() || self.save_all.is_some();
                if ui.add_enabled(self.editor.is_some() && !busy, egui::Button::new("Save All")).clicked()
                    && let Err(e) = self.save_all_docs(true)
                {
                    self.error = Some(e);
                }
                if ui.add_enabled(self.editor.is_some() && !busy, egui::Button::new("Close All")).clicked()
                    && let Err(e) = self.close_all_docs(true)
                {
                    self.error = Some(e);
                }
                ui.separator();
                if ui.button("Batch Process…").clicked() {
                    self.batch.open = true;
                }
                ui.separator();
                let idle = !self.rec.is_active() && self.opening.is_none() && self.saving.is_none();
                if ui.add_enabled(idle, egui::Button::new("New Recording…")).clicked() {
                    self.rec.dialog = Some(self.rec.last_format());
                }
                if ui.add_enabled(idle, egui::Button::new("Record Now").shortcut_text("Shift+Space")).clicked() {
                    let _ = self.quick_record();
                }
                ui.separator();
                let now = tr_args("file.session_folder_tip", &[&self.sessions.path.display().to_string()]);
                if ui.button(tr("file.session_folder")).on_hover_text(&now).clicked() {
                    self.pick_session_folder();
                }
                if ui.button(tr("file.default_session_folder")).on_hover_text(default_sessions_root().display().to_string()).clicked() {
                    self.set_session_folder(None);
                }
                ui.separator();
                if ui.button("Export Application Settings…").on_hover_text("Settings, shortcut changes and workspaces in one file").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_file_name("wavcraft-settings.wavset").save_file()
                    && let Err(e) = self.export_settings(&path)
                {
                    self.error = Some(e);
                }
                if ui.button("Import Application Settings…").on_hover_text("The file is checked whole before anything is replaced").clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("WavCraft settings", &["wavset"]).pick_file()
                {
                    let _ = self.import_settings(&path);
                }
                if ui.button("Restore Preference Defaults…").clicked() {
                    self.confirm_reset_prefs = true;
                }
                ui.separator();
                if ui.button(tr("file.quit")).clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(tr("menu.edit"), |ui| self.edit_menu(ui));
            ui.menu_button("Multitrack", |ui| {
                let has = self.mtv.mt.as_ref().is_some_and(|m| m.clip_count() > 0);
                let norm = self.mtv.mix_normalize.then_some(-1.0);
                ui.checkbox(&mut self.mtv.mix_normalize, "Normalize the mixdown to -1 dB");
                if ui.add_enabled(has, egui::Button::new("Mixdown Session to New File")).clicked() {
                    if let Err(e) = self.start_mixdown(false, norm, ui.ctx()) {
                        self.error = Some(e);
                    }
                    ui.close();
                }
                let picked = has && !self.mtv.sel_clips.is_empty();
                if ui
                    .add_enabled(picked, egui::Button::new("Mixdown Selected Clips to New File"))
                    .on_hover_text("Click clips in the Multitrack editor to pick them")
                    .clicked()
                {
                    if let Err(e) = self.start_mixdown(true, norm, ui.ctx()) {
                        self.error = Some(e);
                    }
                    ui.close();
                }
            });
            ui.menu_button("Window", |ui| {
                for p in panels::REGISTRY {
                    let mut shown = self.panels.is_shown(p.id);
                    // The Editor is always shown (A029): its entry is a fixed tick, and below it maximises.
                    if ui.add_enabled(panels::is_closable(p.id), egui::Checkbox::new(&mut shown, p.title)).changed() {
                        self.set_panel(p.id, shown);
                    }
                }
                let editor_max = self.layout.maximized(&self.panels) == Some(panels::EDITOR);
                if ui.button(if editor_max { "Restore Editor" } else { "Maximize Editor" }).clicked() {
                    self.layout.toggle_maximize(panels::EDITOR);
                    ui.close();
                }
                ui.checkbox(&mut self.show_mixer, "Mixer panel");
                ui.checkbox(&mut self.meters.show_levels, "Levels");
                let mut on = self.meter_input;
                if ui
                    .checkbox(&mut on, "Meter Input Signal")
                    .on_hover_text("Show the input device's level in the Levels panel while nothing plays. Nothing is recorded.")
                    .changed()
                {
                    self.set_meter_input(on);
                }
                if ui.button("Mic Check…").clicked() {
                    self.mic_dialog.get_or_insert(MicDialog { secs: 5.0, result: None });
                    ui.close();
                }
                ui.checkbox(&mut self.meters.show_stats, "Amplitude/Loudness Statistics");
                ui.add_enabled_ui(self.editor.is_some(), |ui| ui.checkbox(&mut self.stats.show, "Sample Statistics (RMS, clipping)"));
                ui.separator();
                ui.menu_button("Workspace", |ui| self.workspace_menu(ui));
                if ui.button("Reset Panel Layout").clicked() {
                    self.layout = Layout::default();
                    self.save_panels();
                    ui.close();
                }
            });
            ui.menu_button("Effects", |ui| self.effects_menu(ui));
            ui.menu_button("Generate", |ui| self.generate_menu(ui));
            ui.menu_button(tr("menu.view"), |ui| {
                if ui.radio_value(&mut self.mtv.show, false, "Waveform editor").clicked()
                    | ui.radio_value(&mut self.mtv.show, true, "Multitrack editor").clicked()
                {
                    self.cdv.show = false;
                }
                ui.radio_value(&mut self.cdv.show, true, "CD Layout");
                if self.clip_edit.is_some() && ui.button("Return to session").clicked() {
                    let r = self.return_to_session(ui.ctx()).map(|_| ());
                    self.report(r);
                }
                ui.separator();
                self.frequency_scale_menu(ui);
                self.spectral_menu(ui);
                ui.separator();
                ui.menu_button("Playback Resampling", |ui| {
                    for q in [Quality::High, Quality::Fast] {
                        let label = if q == Quality::High { "High quality" } else { "Fast" };
                        if ui
                            .radio(self.settings.playback_quality == q, label)
                            .on_hover_text("Used when the file's sample rate differs from the output device's")
                            .clicked()
                        {
                            let r = self.pref_set("playback_quality", q.as_str());
                            self.report(r);
                        }
                    }
                });
                ui.separator();
                let (sel, mac) = (self.selection_range(), cfg!(target_os = "macos"));
                let mut chosen = None;
                for a in TABLE.iter().filter(|a| {
                    (a.name.starts_with("view.")
                        && !a.name.starts_with("view.time_display.")
                        && !a.name.starts_with("view.snap.")
                        && !a.name.starts_with("view.zoom.preset."))
                        || a.name.starts_with("tool.")
                }) {
                    let enabled = self.editor.as_ref().is_some_and(|e| e.enabled(a.id, &sel));
                    let keys = self.keymap.menu_text(Command::Action(a.id), mac);
                    if ui.add_enabled(enabled, egui::Button::new(tr(&format!("action.{}", a.name))).shortcut_text(keys)).clicked() {
                        chosen = Some(a.id);
                    }
                }
                // Zoom to Time (f69): recall a slot, or store the view's time in one.
                let (open, mut set_slot) = (self.doc.is_some(), None);
                ui.menu_button("Zoom to Time", |ui| {
                    for (slot, id) in actions::ZOOM_PRESET_ACTIONS.into_iter().enumerate() {
                        let secs = self.settings.zoom_presets.0[slot];
                        if ui.add_enabled(open, egui::Button::new(format!("Slot {}: {}", slot + 1, slot_text(secs)))).clicked() {
                            chosen = Some(id);
                        }
                    }
                    ui.separator();
                    for slot in 0..wavcraft_core::view::ZOOM_SLOTS {
                        if ui.add_enabled(open, egui::Button::new(format!("Set Slot {} from View", slot + 1))).clicked() {
                            set_slot = Some(slot);
                        }
                    }
                });
                if let Some(slot) = set_slot {
                    let _ = self.set_zoom_slot(slot);
                }
                if let Some(id) = chosen {
                    let _ = self.run_action(id, ui.ctx());
                }
                // Snapping (f09, f49): one switch per target, shared with the ruler's menu.
                let mut snap_chosen = None;
                ui.menu_button("Snapping", |ui| snap_chosen = snap_items(ui, &self.settings.snap));
                if let Some(id) = snap_chosen {
                    self.toggle_snap_menu(id);
                }
                ui.separator();
                let mut picked = None;
                let mut open_custom = false;
                ui.menu_button("Time Display", |ui| {
                    for f in TimeFormat::ALL {
                        let shown = if matches!(f, TimeFormat::Custom(_)) { TimeFormat::Custom(self.settings.custom_fps) } else { f };
                        if ui.radio(self.time_format.name() == f.name(), shown.menu_with_rate()).clicked() {
                            if matches!(f, TimeFormat::Custom(_)) {
                                open_custom = true;
                            } else {
                                picked = Some(f);
                            }
                            ui.close();
                        }
                    }
                });
                if let Some(f) = picked {
                    self.set_time_format(f);
                }
                if open_custom {
                    let _ = self.run_action(ActionId::TimeCustom, ui.ctx());
                }
                ui.checkbox(&mut self.show_time_panel, "Time panel");
                ui.checkbox(&mut self.show_metadata, "Metadata panel");
                ui.separator();
                for (id, label) in [
                    (ActionId::ViewSyncSelection, "Synchronise Selection Across Files"),
                    (ActionId::ViewSyncZoom, "Synchronise Zoom and Scroll Across Files"),
                    (ActionId::ViewSyncPlayhead, "Synchronise Playhead Across Files"),
                ] {
                    let s = self.settings.sync;
                    let mut on = match id {
                        ActionId::ViewSyncSelection => s.selection,
                        ActionId::ViewSyncZoom => s.zoom,
                        _ => s.playhead,
                    };
                    if ui.checkbox(&mut on, label).changed() {
                        self.toggle_sync(id);
                    }
                }
                ui.separator();
                let mut chosen_language = None;
                ui.menu_button(tr("menu.language"), |ui| {
                    for code in i18n::language_codes() {
                        if ui.selectable_label(i18n::language() == code, tr(&format!("language.{code}"))).clicked() {
                            chosen_language = Some(code);
                            ui.close();
                        }
                    }
                });
                if let Some(code) = chosen_language {
                    self.set_language(code);
                }
            });
            ui.menu_button("Video", |ui| {
                if ui.add_enabled(self.editor.is_some(), egui::Button::new("Attach video file…")).clicked() {
                    self.pick_video(ui.ctx());
                }
                if ui.add_enabled(self.video.is_some(), egui::Button::new("Detach video")).clicked() {
                    self.drop_video();
                }
                ui.checkbox(&mut self.show_video, "Video panel");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About WavCraft…").clicked() {
                    self.show_about = true;
                }
            });
            let label = if self.player.is_playing() {
                tr("transport.stop")
            } else if self.transport.mode == Mode::Paused {
                "▶ Resume".to_string()
            } else {
                tr("transport.play")
            };
            if ui.add_enabled(!self.rec.is_active(), egui::Button::new(label)).on_hover_text("Space").clicked() {
                self.toggle_play();
            }
            let mut m = self.player.metronome();
            let mut changed =
                ui.checkbox(&mut m.on, "Metronome").on_hover_text("A click on each beat of the document's tempo, in the playback only").changed();
            if m.on {
                changed |= ui.add(egui::DragValue::new(&mut m.volume).range(0.0..=1.0).speed(0.01).fixed_decimals(2).prefix("vol ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut m.count_in_bars).range(0..=16).suffix(" bar count-in")).changed();
            }
            if changed {
                self.set_metronome(m);
            }
            if self.rec.is_active() {
                if ui.button("■ Stop Recording").clicked() {
                    self.finish_recording(true, ui.ctx());
                }
                let label = if self.rec.is_paused() { "● Resume" } else { "❚❚ Pause" };
                if ui.button(label).on_hover_text("Space").clicked() {
                    self.rec.set_paused(!self.rec.is_paused());
                }
            } else if self.rec.is_armed() {
                if ui.button("Cancel Timed Recording").clicked() {
                    self.rec.disarm();
                }
            } else if ui
                .add_enabled(self.opening.is_none() && self.saving.is_none(), egui::Button::new("● Record"))
                .on_hover_text("New Recording (Shift+Space records at once with the last settings)")
                .clicked()
            {
                self.rec.dialog = Some(self.rec.last_format());
            }
        });
    }

    /// Free space in the session folder's disk, read again at most every few seconds.
    fn free_space_now(&mut self) -> Option<u64> {
        if let Some((at, bytes)) = self.free_space
            && at.elapsed() < FREE_SPACE_TTL
        {
            return bytes;
        }
        let bytes = wavcraft_core::space::free_space(&self.sessions.path);
        self.free_space = Some((Instant::now(), bytes));
        bytes
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let mut cancel_job = false;
        // The whole bar answers a right-click with the menu of fields; registered first, so the
        // labels drawn after it keep their own hover text.
        let top = ui.available_rect_before_wrap();
        let bar = Rect::from_min_size(top.min, vec2(top.width(), ui.spacing().interact_size.y + 2.0));
        let menu = ui.interact(bar, ui.id().with("status-menu"), Sense::click());
        let mut changed = false;
        menu.context_menu(|ui| {
            ui.label("Show in the status bar");
            for field in StatusField::ALL {
                let mut shown = self.settings.status_fields.is_shown(field);
                if ui.checkbox(&mut shown, field.label()).changed() {
                    self.settings.status_fields.set(field, shown);
                    changed = true;
                }
            }
        });
        if changed {
            self.persist_settings();
        }
        let mut convert = false;
        let fields = self.settings.status_fields;
        let free = if self.doc.is_some() && fields.is_shown(StatusField::FreeSpace) { Some(self.free_space_now()) } else { None };
        ui.horizontal(|ui| {
            if let Some((text, warn)) = self.rec.status_text() {
                ui.colored_label(if warn { CURSOR } else { Color32::from_rgb(230, 230, 230) }, text);
            } else if let Some(o) = &self.opening {
                ui.spinner();
                let permille = o.progress.load(Ordering::Relaxed);
                ui.label(tr_args("status.opening", &[&o.path.display().to_string(), &(permille / 10).to_string()]));
            } else if let Some(r) = &self.running {
                ui.spinner();
                ui.label(format!("{}… {}%", r.job.label(), r.job.permille() / 10));
                if ui.button("Cancel").clicked() {
                    cancel_job = true;
                }
            } else if let Some(i) = &self.inserting {
                ui.spinner();
                ui.label(format!("Inserting {}… {}%", i.path.display(), i.progress.load(Ordering::Relaxed) / 10));
            } else if let Some(s) = &self.saving {
                ui.spinner();
                ui.label(tr_args("status.saving", &[&s.path.display().to_string(), &(s.progress.load(Ordering::Relaxed) / 10).to_string()]));
                if let Some(cancel) = &s.cancel
                    && ui.button("Cancel").on_hover_text("Stop saving: the file on disk stays as it was").clicked()
                {
                    cancel.store(true, Ordering::Relaxed);
                }
            } else if let Some(b) = &self.batch_write {
                ui.spinner();
                ui.label(format!("{}… {}%", b.what, b.progress.load(Ordering::Relaxed) / 10));
            } else if let Some(a) = &self.appending {
                ui.spinner();
                ui.label(format!("Open Append: {} file(s)… {}%", a.paths, a.progress.load(Ordering::Relaxed) / 10));
                if ui.button("Cancel").clicked() {
                    a.cancel.store(true, Ordering::Relaxed);
                }
            } else if let Some(m) = &self.making {
                ui.spinner();
                ui.label(format!("{}… {}%", m.what, m.progress.load(Ordering::Relaxed) / 10));
            } else if self.rebuilding.is_some() {
                ui.spinner();
                ui.label(tr("status.updating"));
            } else if let Some(e) = &self.error {
                ui.colored_label(CURSOR, e);
            } else if let Some(n) = &self.notice {
                ui.label(n);
            } else if let Some(doc) = &self.doc {
                let rate = doc.sample_rate();
                let mut first = true;
                let mut gap = |ui: &mut egui::Ui| {
                    if !first {
                        ui.separator();
                    }
                    first = false;
                };
                for field in StatusField::ALL {
                    if !fields.is_shown(field) {
                        continue;
                    }
                    let text = match field {
                        StatusField::Name => doc.name().to_string(),
                        StatusField::Format => format!("{:?}", doc.info.format),
                        StatusField::SampleRate => format!("{rate} Hz"),
                        StatusField::BitDepth => statusbar::sample_label(doc.info.sample_format).to_string(),
                        StatusField::Channels => format!("{} ch", doc.channels()),
                        StatusField::FileSize => format!(
                            "{} uncompressed",
                            wavcraft_core::space::format_bytes(statusbar::uncompressed_bytes(
                                doc.frames() as u64,
                                doc.channels() as u64,
                                doc.info.sample_format
                            ))
                        ),
                        StatusField::Duration => self.fmt_time(doc.frames()),
                        StatusField::FreeSpace => match free.flatten() {
                            Some(bytes) => format!("{} free", wavcraft_core::space::format_bytes(bytes)),
                            None => "free space unknown".to_string(),
                        },
                        StatusField::Selection if self.selection.is_empty() => tr_args("status.cursor", &[&self.fmt_time(self.selection.start())]),
                        StatusField::Selection => {
                            format!("Selection {} – {}", self.fmt_time(self.selection.start()), self.fmt_time(self.selection.end()))
                        }
                        StatusField::SelectionLength if self.selection.is_empty() => continue,
                        StatusField::SelectionLength => format!("Length {}", self.fmt_time(self.selection.end() - self.selection.start())),
                        StatusField::Output => {
                            gap(ui);
                            match &self.player.error {
                                Some(e) => ui.colored_label(CURSOR, tr_args("status.no_sound", &[e])),
                                None if self.player.failed() => ui.colored_label(CURSOR, tr("status.no_sound_stopped")),
                                None => {
                                    let i = self.player.info();
                                    if i.resampling {
                                        ui.label(format!(
                                            "Out: {} ({} → {} Hz, {})",
                                            self.player.device,
                                            i.doc_rate,
                                            i.device_rate,
                                            i.quality.as_str()
                                        ))
                                    } else {
                                        ui.label(tr_args("status.out", &[&self.player.device]))
                                    }
                                }
                            };
                            continue;
                        }
                    };
                    gap(ui);
                    if matches!(field, StatusField::Format | StatusField::BitDepth) {
                        // The sample type (f76): double-click opens Convert Sample Type.
                        let r = ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text("Double-click to convert the sample type");
                        convert |= r.double_clicked();
                    } else {
                        ui.label(text);
                    }
                }
                // What was wrong with the file that still opened: the first in the bar, all of
                // them on hover. Always shown, whichever fields are.
                // The transport and the last operation (A327), then what is under the pointer (A079).
                let transport = match self.transport.mode {
                    Mode::Playing => "Playing",
                    Mode::Paused => "Paused",
                    Mode::Stopped => "Stopped",
                };
                gap(ui);
                ui.label(transport);
                if let Some((what, took)) = &self.last_op {
                    gap(ui);
                    ui.label(format!("· {what}: {}", readout::duration_text(took.as_millis() as u64)))
                        .on_hover_text("How long the last operation took");
                }
                if let Some(r) = &self.pointer {
                    gap(ui);
                    let sample = r.channel().and_then(|c| self.sample_at(c, r.frame));
                    ui.label(r.text(rate, sample));
                }
                if let Some(first_warning) = doc.warnings().first() {
                    gap(ui);
                    ui.colored_label(CURSOR, format!("Warning: {first_warning}")).on_hover_text(doc.warnings().join("\n"));
                }
            } else if self.editor.is_some() {
                ui.label(format!("{} (no audio yet): paste into it, or File › Save As once it has some", path_name(&self.doc_path)));
            } else {
                ui.label(tr("status.hint"));
            }
        });
        if convert {
            let ctx = ui.ctx().clone();
            let _ = self.run_action(ActionId::ConvertSampleType, &ctx);
        }
        if cancel_job && let Some(r) = &self.running {
            r.job.cancel();
        }
    }

    /// Right-click in the waveform (the Select tool, menu setting off): the edge nearest `frame`
    /// moves there (Audition's default), so the selection grows or shrinks to the click. Returns
    /// whether the selection moved.
    fn right_click_at(&mut self, frame: usize, total: usize) -> bool {
        if self.settings.right_click_menu || self.tool != Tool::Select {
            return false;
        }
        self.selection = self.selection.extend_nearest_to(frame, total);
        true
    }

    /// The band, while the time range it was made on is still the selection.
    fn live_band(&self) -> Option<Band> {
        self.band.filter(|(_, at)| *at == self.selection).map(|(band, _)| band)
    }

    /// The spectral selection now: the time selection with the band, if there is a live one.
    fn spectral(&self) -> Option<SpectralSelection> {
        let rate = self.doc.as_ref()?.sample_rate();
        self.live_band().map(|band| SpectralSelection { time: self.selection, band, rate })
    }

    /// Set the band on the current time range, or clear it.
    fn set_band(&mut self, band: Option<Band>) {
        self.band = band.map(|b| (b, self.selection));
    }

    /// `lo-hi` in Hz, or `none`, for STATE and the control channel.
    fn band_text(&self) -> String {
        self.live_band().map_or_else(|| "none".to_string(), |b| b.to_string())
    }

    /// The control channel's pointer gestures without a pointer: `marquee <frame0> <frame1> <hz0>
    /// <hz1>`, `marquee_move <dframes> <dhz>` and `right_click <frame>`. `None` for any other gesture.
    fn pointer_gesture(&mut self, rest: &str) -> Option<String> {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let (total, rate) = self.doc.as_ref().map_or((0, 0), |d| (d.frames(), d.sample_rate()));
        let nyquist = rate as f32 / 2.0;
        let answer = |s: &Self| format!("OK selection={}-{} band={}", s.selection.start(), s.selection.end(), s.band_text());
        match words.as_slice() {
            ["marquee", f0, f1, h0, h1] => {
                let usage = "ERR GESTURE marquee <frame0> <frame1> <hz0> <hz1>".to_string();
                if self.editor.is_none() {
                    return Some("ERR no file is open".to_string());
                }
                let (Ok(f0), Ok(f1), Ok(h0), Ok(h1)) = (f0.parse::<usize>(), f1.parse::<usize>(), h0.parse::<f32>(), h1.parse::<f32>()) else {
                    return Some(usage);
                };
                if !h0.is_finite() || !h1.is_finite() {
                    return Some(usage);
                }
                self.tool = Tool::Marquee;
                self.selection = Selection { anchor: f0.min(total), head: f1.min(total) };
                let band = Band { feather_hz: self.feather_hz, feather_ms: self.feather_ms, ..Band::new(h0, h1, nyquist) };
                self.set_band(Some(band));
                Some(answer(self))
            }
            ["marquee_move", df, dh] => {
                let (Ok(df), Ok(dh)) = (df.parse::<i64>(), dh.parse::<f32>()) else {
                    return Some("ERR GESTURE marquee_move <dframes> <dhz>".to_string());
                };
                let Some(spectral) = self.spectral() else {
                    return Some("ERR no spectral selection to move".to_string());
                };
                let moved = spectral.moved(df, dh, total, nyquist);
                self.selection = moved.time;
                self.set_band(Some(moved.band));
                Some(answer(self))
            }
            ["right_click", frame] => {
                let Ok(frame) = frame.parse::<usize>() else {
                    return Some("ERR GESTURE right_click <frame>".to_string());
                };
                if self.editor.is_none() {
                    return Some("ERR no file is open".to_string());
                }
                let moved = self.right_click_at(frame, total);
                Some(format!("{} moved={}", answer(self), u8::from(moved)))
            }
            ["marquee" | "marquee_move" | "right_click", ..] => {
                Some("ERR GESTURE right_click <frame> | marquee <frame0> <frame1> <hz0> <hz1> | marquee_move <dframes> <dhz>".to_string())
            }
            _ => None,
        }
    }

    /// The tools bar above the editor: the pointer's tools with the current one marked, and the
    /// current tool's options (the Marquee's feather; the Select tool's right-click setting).
    fn tools_bar(&mut self, ui: &mut egui::Ui) {
        let tools = [(Tool::Select, "Time Selection (V)"), (Tool::Hand, "Hand (H)"), (Tool::Zoom, "Zoom (Z)"), (Tool::Marquee, "Marquee (E)")];
        ui.horizontal(|ui| {
            for (tool, label) in tools {
                if ui.selectable_label(self.tool == tool, label).clicked() {
                    self.tool = tool;
                }
            }
            ui.separator();
            if self.tool == Tool::Marquee {
                let (mut hz, mut ms) = (self.feather_hz, self.feather_ms);
                let a = ui
                    .add(egui::DragValue::new(&mut hz).range(0.0..=12_000.0).suffix(" Hz"))
                    .on_hover_text("Feather: soft edge above and below the band");
                let b = ui
                    .add(egui::DragValue::new(&mut ms).range(0.0..=5_000.0).suffix(" ms"))
                    .on_hover_text("Feather: soft edge before and after the time range");
                if a.changed() || b.changed() {
                    (self.feather_hz, self.feather_ms) = (hz, ms);
                    if let Some((band, _)) = self.band.as_mut() {
                        (band.feather_hz, band.feather_ms) = (hz, ms);
                    }
                }
            }
            if self.tool == Tool::Select {
                let mut menu = self.settings.right_click_menu;
                if ui
                    .checkbox(&mut menu, "Right-click opens a menu")
                    .on_hover_text("Off: right-click moves the nearest selection edge to the click")
                    .changed()
                {
                    self.settings.right_click_menu = menu;
                    match self.save_settings() {
                        Ok(()) => self.notice = None,
                        Err(e) => self.notice = Some(e),
                    }
                }
            }
        });
    }

    fn draw_hud(&mut self, ctx: &egui::Context) {
        if !actions::hud_visible(self.hud.hidden, self.hud.only_for_selection, !self.selection.is_empty()) {
            return;
        }
        egui::Area::new(egui::Id::new("amplitude-hud")).movable(!self.hud.pinned).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Gain {:.1} dB", self.hud.gain_db));
                    if ui.button(if self.hud.pinned { "Unpin" } else { "Pin" }).clicked() {
                        self.hud.pinned = !self.hud.pinned;
                    }
                    if ui.button("Hide").clicked() {
                        self.hud.hidden = true;
                    }
                });
                let drag = ui.add(egui::Label::new("Drag vertically to change").sense(Sense::drag()));
                if drag.drag_started() {
                    self.hud.drag_start_db = Some(self.hud.gain_db);
                }
                if drag.dragged() {
                    let start = self.hud.drag_start_db.unwrap_or(self.hud.gain_db);
                    self.hud.gain_db = actions::hud_drag_gain_db(start, -f64::from(drag.drag_delta().y));
                }
                if drag.drag_stopped() {
                    self.hud.drag_start_db = None;
                }
                ui.checkbox(&mut self.hud.only_for_selection, "Show only for selections");
            });
        });
    }

    /// Whether an editor display (waveform, spectral) is drawn: shown, and not hidden by another
    /// panel being maximised.
    fn editor_shows(&self, id: &str) -> bool {
        match self.layout.maximized(&self.panels) {
            Some(m) if !panels::is_movable(m) => m == id || m == panels::EDITOR,
            Some(_) => false,
            None => self.panels.is_shown(id),
        }
    }

    /// The zoom buttons and the navigator bar above the editor.
    fn zoom_panel(&mut self, ui: &mut egui::Ui) {
        let open = self.doc.is_some();
        ui.horizontal(|ui| {
            let buttons = [
                (ActionId::ZoomIn, "Zoom In", "Zoom in on time (=)"),
                (ActionId::ZoomOut, "Zoom Out", "Zoom out on time (-)"),
                (ActionId::ZoomToSelection, "Selection", "Zoom to the selection (Shift+S)"),
                (ActionId::ZoomInAtIn, "In @ In", "Zoom in, keeping the selection start where it is"),
                (ActionId::ZoomInAtOut, "In @ Out", "Zoom in, keeping the selection end where it is"),
                (ActionId::ZoomOutFull, "Full", "Zoom out to the whole file"),
                (ActionId::ZoomAmpIn, "Amp +", "Zoom in vertically (Alt+=, Alt+wheel)"),
                (ActionId::ZoomAmpOut, "Amp -", "Zoom out vertically (Alt+-, Alt+wheel)"),
            ];
            for (id, text, tip) in buttons {
                let usable = open && (id != ActionId::ZoomToSelection || !self.selection.is_empty());
                if ui.add_enabled(usable, egui::Button::new(text)).on_hover_text(tip).clicked() {
                    let _ = self.run_action(id, ui.ctx());
                }
            }
        });
        // Zoom to Time slots (click: recall; right-click: store the current view).
        ui.horizontal(|ui| {
            ui.label("Zoom to Time:");
            for (slot, id) in actions::ZOOM_PRESET_ACTIONS.into_iter().enumerate() {
                let secs = self.settings.zoom_presets.0[slot];
                let tip = format!("Show {} across the window. Right-click: store the current view in this slot.", slot_text(secs));
                let resp = ui.add_enabled(open, egui::Button::new((slot + 1).to_string())).on_hover_text(tip);
                if resp.clicked() {
                    let _ = self.run_action(id, ui.ctx());
                }
                resp.context_menu(|ui| {
                    if ui.button("Set from current view").clicked() {
                        let _ = self.set_zoom_slot(slot);
                        ui.close();
                    }
                });
            }
        });
        self.navigator(ui);
    }

    /// The whole file in a thin strip with the visible range as a handle: drag it to scroll,
    /// drag its edges to zoom, click outside it to jump there.
    fn navigator(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(vec2(ui.available_width(), NAV_HEIGHT), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 2.0, Color32::from_gray(24));
        let Some(doc) = &self.doc else { return };
        let (frames, channels) = (doc.frames(), doc.channels().max(1));
        let (nav_px, view_px) = (rect.width().max(1.0), self.editor_width.max(1.0));
        // The overview: the loudest of the channels in each column, from the peaks.
        let cols = nav_px.round().max(1.0) as usize;
        let mut merged = vec![(0.0_f32, 0.0_f32); cols];
        for ch in 0..channels {
            for (m, (lo, hi)) in merged.iter_mut().zip(doc.waveform_columns(ch, 0.0, frames as f64, cols)) {
                *m = (m.0.min(lo), m.1.max(hi));
            }
        }
        let (mid, half) = (rect.center().y, rect.height() * 0.5 - 2.0);
        for (i, (lo, hi)) in merged.into_iter().enumerate() {
            let x = rect.left() + i as f32 + 0.5;
            let (top, bottom) = (mid - hi.clamp(-1.0, 1.0) * half, mid - lo.clamp(-1.0, 1.0) * half);
            painter.line_segment([pos2(x, top.min(bottom - 1.0)), pos2(x, bottom)], Stroke::new(1.0, Color32::from_rgb(40, 110, 80)));
        }

        // Presses and drags.
        let frame_at = |x: f32| f64::from((x / nav_px).clamp(0.0, 1.0)) * frames as f64;
        let handle = self.view.nav_handle(view_px, nav_px, frames);
        let local_x = |p: Pos2| p.x - rect.left();
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let x = local_x(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
            self.nav_drag = Some(match nav_hit(handle, x, 6.0) {
                Some(part) => (part, frame_at(x) - self.view.start),
                None => {
                    // A press outside the handle jumps there and carries on as a drag of it.
                    self.view.nav_jump(x, view_px, nav_px, frames);
                    (NavPart::Body, self.view.frames_per_px * f64::from(view_px) * 0.5)
                }
            });
        }
        if let (Some((part, grab)), Some(pos)) = (self.nav_drag, resp.interact_pointer_pos()) {
            self.view.nav_drag(part, local_x(pos), grab, view_px, nav_px, frames);
        }
        if resp.drag_stopped() {
            self.nav_drag = None;
        }
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
            && nav_hit(handle, local_x(pos), 6.0).is_none()
        {
            self.view.nav_jump(local_x(pos), view_px, nav_px, frames);
        }
        if let Some(pos) = resp.hover_pos().or(resp.interact_pointer_pos()) {
            let part = self.nav_drag.map(|(p, _)| p).or_else(|| nav_hit(handle, local_x(pos), 6.0));
            match part {
                Some(NavPart::Left | NavPart::Right) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                Some(NavPart::Body) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                None => {}
            }
        }
        self.view.clamp(view_px, frames);

        // The handle, drawn from the view as it is now.
        let (l, r) = self.view.nav_handle(view_px, nav_px, frames);
        let r = r.max(l + 3.0);
        let handle_rect = Rect::from_min_max(pos2(rect.left() + l, rect.top()), pos2((rect.left() + r).min(rect.right()), rect.bottom()));
        painter.rect_filled(handle_rect, 2.0, Color32::from_rgba_premultiplied(60, 80, 120, 70));
        painter.rect_stroke(handle_rect, 2.0, Stroke::new(1.5, Color32::from_rgb(120, 160, 230)), egui::StrokeKind::Inside);
        if !self.selection.is_empty() {
            let (a, b) = (self.selection.start() as f32 / frames.max(1) as f32, self.selection.end() as f32 / frames.max(1) as f32);
            let strip = Rect::from_min_max(
                pos2(rect.left() + a * nav_px, rect.bottom() - 3.0),
                pos2(rect.left() + (b * nav_px).max(a * nav_px + 1.0), rect.bottom()),
            );
            painter.rect_filled(strip, 0.0, Color32::from_rgb(130, 150, 210));
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let full = response.rect;
        let c = &self.settings.colors;
        let ground = c.background.map_or(if self.dark { BACKGROUND } else { BACKGROUND_LIGHT }, |[r, g, b]| Color32::from_rgb(r, g, b));
        painter.rect_filled(full, 0.0, contrast(brighten(ground, self.brightness), self.contrast));
        let rgb = |o: Option<[u8; 3]>, d: Color32| o.map_or(d, |[r, g, b]| Color32::from_rgb(r, g, b));
        let (selection_colour, cursor_colour, playhead_colour) = (
            c.selection.map_or(SELECTION, |[r, g, b]| Color32::from_rgba_unmultiplied(r, g, b, 90)),
            rgb(c.cursor, CURSOR),
            rgb(c.playhead, PLAYHEAD),
        );
        let Some((frames, rate, channels)) = self.doc.as_ref().map(|d| (d.frames(), d.sample_rate(), d.channels().max(1))) else { return };
        // The amplitude ruler takes a column on the left; the time axis starts right of it.
        let amp_col = Rect::from_min_max(full.min, pos2(full.left() + AMP_RULER_WIDTH, full.bottom()));
        let rect = Rect::from_min_max(pos2(amp_col.right(), full.top()), full.max);
        let ruler_resp = ui.interact(amp_col, ui.id().with("amp-ruler"), Sense::click());
        let mut chosen_scale = None;
        let mut toggled = None;
        let chnames: Vec<String> = self.editor.as_ref().map(|e| e.history.current().source().meta.chnames.clone()).unwrap_or_default();
        let width = rect.width().max(1.0);
        self.editor_width = width;
        if self.fit_on_next_frame {
            self.view = Viewport::fit(frames, width);
            self.fit_on_next_frame = false;
        }
        // The editor's geometry as painted, and what is under the pointer (A079).
        self.editor_layout = Some(EditorLayout {
            width,
            height: rect.height(),
            ruler_height: RULER_HEIGHT,
            show_waveform: self.editor_shows("waveform"),
            show_spectrum: self.editor_shows("spectral"),
            channels,
        });
        self.pointer = response.hover_pos().and_then(|p| {
            let layout = self.editor_layout?;
            Readout::at(&layout, &self.view, &self.params, rate, frames, p.x - rect.left(), p.y - rect.top())
        });

        // The frequency ruler: a strip down the left of the spectral display, if it has room.
        let spectrum_zone = {
            let top = rect.top() + RULER_HEIGHT;
            let height = rect.bottom() - top;
            match (self.editor_shows("waveform"), self.editor_shows("spectral")) {
                (true, true) => Some(Rect::from_min_max(pos2(rect.left(), top + height * self.settings.spec_split_pct as f32 / 100.0), rect.max)),
                (false, true) => Some(Rect::from_min_max(pos2(rect.left(), top), rect.max)),
                _ => None,
            }
        };
        let freq_ruler =
            spectrum_zone.filter(|a| a.width() > 3.0 * FREQ_RULER_WIDTH).map(|a| Rect::from_min_size(a.min, vec2(FREQ_RULER_WIDTH, a.height())));
        let over_freq_ruler = |pos: Pos2| freq_ruler.is_some_and(|r| r.contains(pos));
        // Where in the lane under `y` (0 bottom, 1 top) and how tall that lane is.
        let lane_at = |y: f32| {
            spectrum_zone.map_or((0.5, 1.0), |a| {
                let ch = (((y - a.top()) / (a.height() / channels as f32)).floor().max(0.0) as usize).min(channels - 1);
                let l = lane(a, ch, channels).shrink2(vec2(0.0, 1.0));
                (((l.bottom() - y) / l.height().max(1.0)).clamp(0.0, 1.0), l.height().max(1.0))
            })
        };

        // Alt+wheel zooms the waveform vertically.
        let alt = ui.input(|i| i.modifiers.alt);
        if alt && (response.hovered() || ruler_resp.hovered()) {
            let scroll = ui.input(|i| i.smooth_scroll_delta());
            self.amp_zoom.by(((scroll.y + scroll.x) * 0.01).exp());
        }
        // Wheel zooms around the pointer; Shift+wheel and sideways scrolling move through time.
        if let Some(pointer) = response.hover_pos().filter(|_| !alt) {
            let (scroll, pinch, shift) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.modifiers.shift));
            let x = pointer.x - rect.left();
            // Over the frequency ruler the wheel scrolls the band and Ctrl+wheel (or a pinch) zooms it.
            let on_freq = over_freq_ruler(pointer);
            if on_freq {
                let (t, lane_height) = lane_at(pointer.y);
                let mut axis = self.freq_axis();
                if pinch != 1.0 {
                    axis = axis.zoom_at(1.0 / pinch, t, rate);
                }
                if scroll.y != 0.0 {
                    axis = axis.pan_by(scroll.y / lane_height, rate);
                }
                spectral::set_axis(&mut self.params, &mut self.spectrum, axis, rate);
            }
            if !on_freq && pinch != 1.0 {
                self.view.zoom_at(f64::from(pinch), x, width, frames);
            }
            if !on_freq && (shift || scroll.x != 0.0) && self.transport.mode != Mode::Stopped && (scroll.x != 0.0 || scroll.y != 0.0) {
                // Moving the view by hand during playback turns auto-scroll off for this play.
                self.transport.auto_scroll = false;
            }
            if on_freq {
                // The wheel moved the band above, not the view.
            } else if shift {
                self.view.scroll_by(-(scroll.x + scroll.y), width, frames);
            } else {
                if scroll.y != 0.0 {
                    // A wheel notch moves by the zoom factor preference (2 is the base rate).
                    let rate = f64::from(self.settings.zoom_factor).ln() / std::f64::consts::LN_2;
                    self.view.zoom_at((f64::from(scroll.y) * 0.01 * rate).exp(), x, width, frames);
                }
                if scroll.x != 0.0 {
                    self.view.scroll_by(-scroll.x, width, frames);
                }
            }
        }
        // Auto-scroll: the view follows the playhead (not while the pointer is busy in the editor).
        if self.transport.mode == Mode::Playing {
            if let Some(follow) = self.follow_mode()
                && !response.is_pointer_button_down_on()
            {
                transport::follow_view(follow, &mut self.view, self.player.position() as f64, width, frames);
            }
        } else if self.player.is_playing() && !self.previewing && !response.is_pointer_button_down_on() {
            // Another play (the Playlist): keep the playhead on screen.
            self.view.reveal(self.player.position() as f64, width, frames);
        }
        self.view.clamp(width, frames);

        // Select tool: click places the cursor, drag selects; near a selection edge a drag adjusts
        // it, Shift-click extends, double-click selects what is on screen, triple-click everything.
        let (snap_fmt, snap_on) = (self.time_format, self.snap_frames);
        let frame_at = |view: &Viewport, pos: Pos2| {
            let frame = view.frame_at(pos.x - rect.left()).clamp(0.0, frames as f64).round() as usize;
            if snap_on { snap_fmt.snap_to_frame(frame, rate, frames) } else { frame }
        };
        // Snapping (f09, f49) pulls a placed frame to the ruler, the selection's edges and, when on,
        // zero crossings, on top of the timecode-frame snap `frame_at` already does.
        let fpp = self.view.frames_per_px;
        let snap =
            |app: &WaveApp, frame: usize, targets: &[u64]| app.editor.as_ref().map_or(frame, |e| app.snap_at(e, frame as u64, fpp, targets) as usize);
        let edge_at = |sel: &Selection, view: &Viewport, pos: Pos2| sel.edge_near(view, pos.x - rect.left(), GRAB_PX);
        if let Some(pos) = response.hover_pos().or(response.interact_pointer_pos())
            && edge_at(&self.selection, &self.view, pos).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let ruler_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        if response.drag_started() {
            self.drag_origin = ui.input(|i| i.pointer.press_origin()).or_else(|| response.interact_pointer_pos());
        }
        // A drag that starts on the frequency ruler scrolls the band (right button: zooms it) and a
        // double-click there shows all of it; none of the time gestures take it.
        if over_freq_ruler(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if response.drag_started() {
            self.freq_drag = self.drag_origin.filter(|o| over_freq_ruler(*o)).map(|origin| {
                let (anchor_t, lane_height) = lane_at(origin.y);
                spectral::FreqDrag {
                    origin,
                    axis: self.freq_axis(),
                    zoom: response.dragged_by(egui::PointerButton::Secondary),
                    anchor_t,
                    lane_height,
                }
            });
        }
        let freq_busy = self.freq_drag.is_some();
        if let Some(drag) = self.freq_drag {
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let dy = pos.y - drag.origin.y;
                let axis = if drag.zoom {
                    drag.axis.zoom_at((-dy * 0.01).exp(), drag.anchor_t, rate)
                } else {
                    drag.axis.pan_by(dy / drag.lane_height, rate)
                };
                spectral::set_axis(&mut self.params, &mut self.spectrum, axis, rate);
            } else {
                self.freq_drag = None;
            }
        }
        let on_freq_click = (response.clicked() || response.double_clicked() || response.triple_clicked())
            && response.interact_pointer_pos().is_some_and(over_freq_ruler);
        if response.double_clicked() && on_freq_click {
            let full = FreqAxis::full_scale(self.freq_axis().scale(), rate);
            spectral::set_axis(&mut self.params, &mut self.spectrum, full, rate);
        }
        let drag_dx = response.drag_delta().x;
        // Middle-button drag scrolls whatever the tool (A084); the Hand tool does it with the
        // primary button (A050).
        if !freq_busy
            && (response.dragged_by(egui::PointerButton::Middle) || (self.tool == Tool::Hand && response.dragged_by(egui::PointerButton::Primary)))
        {
            self.view.drag_by(drag_dx, width, frames);
        }
        // A range drag zooms to it: right-drag in the ruler (A091), or the Zoom tool's drag (A051).
        let range_zoom = (response.drag_stopped_by(egui::PointerButton::Secondary) && self.drag_origin.is_some_and(|o| ruler_rect.contains(o)))
            || (self.tool == Tool::Zoom && !freq_busy && response.drag_stopped_by(egui::PointerButton::Primary));
        if range_zoom
            && let (Some(origin), Some(end)) = (self.drag_origin, ui.input(|i| i.pointer.latest_pos()).or_else(|| response.interact_pointer_pos()))
        {
            let (a, b) = (self.view.frame_at(origin.x - rect.left()), self.view.frame_at(end.x - rect.left()));
            self.view.zoom_to_range(a, b, width, frames);
        }
        if self.tool == Tool::Zoom
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let out = ui.input(|i| i.modifiers.alt);
            self.view.zoom_click(self.view.frame_at(pos.x - rect.left()), out, width, frames);
        }

        // The Marquee tool (B135) takes a drag that starts on the spectral display; every other
        // gesture is the Select tool's, as before.
        let body = Rect::from_min_max(pos2(rect.left(), ruler_rect.bottom()), rect.max);
        let spectrum_hit = match (self.editor_shows("waveform"), self.editor_shows("spectral")) {
            (true, true) => Some(Rect::from_min_max(pos2(body.left(), body.top() + body.height() * 0.5), body.max)),
            (false, true) => Some(body),
            _ => None,
        };
        let marquee_tool = self.tool == Tool::Marquee;
        let marquee_hit = spectrum_hit.filter(|_| marquee_tool);
        let marquee_starts = !freq_busy
            && marquee_hit.is_some_and(|a| {
                response.drag_started_by(egui::PointerButton::Primary) && ui.input(|i| i.pointer.press_origin()).is_some_and(|o| a.contains(o))
            });
        // Selection gestures: the Select tool's, and the Marquee tool's outside a marquee drag.
        let select = (self.tool == Tool::Select || (marquee_tool && self.marquee.is_none() && !marquee_starts)) && !freq_busy && !on_freq_click;
        let shift = ui.input(|i| i.modifiers.shift);
        if response.drag_started()
            && let Some(e) = &self.editor
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let markers = e.history.current().markers()
            && let Some(id) = self.markers_ui.handle_at(markers, &self.view, ruler_rect, origin)
        {
            // A marker's flag was grabbed: the drag moves it (one step, when released).
            let grabbed = frame_at(&self.view, origin) as u64;
            self.markers_ui.start_drag(markers, id, grabbed);
        }
        // A drag that starts on the ruler moves the playhead and is heard (scrubbing, f36).
        if select
            && self.markers_ui.drag.is_none()
            && response.drag_started_by(egui::PointerButton::Primary)
            && ui.input(|i| i.pointer.press_origin()).is_some_and(|o| ruler_rect.contains(o))
        {
            self.head_scrub.dragging = true;
        }
        if self.markers_ui.drag.is_some() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.markers_ui.drag_to(frame_at(&self.view, pos) as u64);
            }
            if response.drag_stopped()
                && let (Some(d), Some(e)) = (self.markers_ui.drag.take(), self.editor.as_mut())
                && e.move_marker(d.id, d.start)
                && let Some(i) = e.history.current().markers().index_of(d.id)
            {
                self.markers_ui.pick.click(e.history.current().markers(), i, false, false);
            }
        } else if self.head_scrub.dragging {
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.head_scrub.pending = Some(frame_at(&self.view, pos));
            }
            if response.drag_stopped() {
                self.head_scrub.ended = true;
            }
        } else if select
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            // A drag is only reported once the pointer has moved a few pixels: anchor the
            // selection where the button went down, not where the pointer has got to.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            self.snap_targets = vec![self.selection.start() as u64, self.selection.end() as u64];
            if let Some(edge) = edge_at(&self.selection, &self.view, origin) {
                self.selection.grab(edge);
                self.selection.head = snap(self, frame_at(&self.view, pos), &self.snap_targets);
            } else if shift {
                self.selection.extend_to(snap(self, frame_at(&self.view, pos), &self.snap_targets));
            } else {
                let (anchor, head) =
                    (snap(self, frame_at(&self.view, origin), &self.snap_targets), snap(self, frame_at(&self.view, pos), &self.snap_targets));
                self.selection = Selection { anchor, head };
                // Context-sensitive channel editing: where the drag began picks the channel(s).
                if self.settings.context_channel_editing
                    && let Some(editor) = self.editor.as_mut()
                {
                    let channels = editor.history.current().channel_count();
                    let top = rect.top() + RULER_HEIGHT;
                    let y = (origin.y - top) / (rect.bottom() - top).max(1.0);
                    editor.channels = view::mask_after_drag(y, channels, true, editor.channels);
                }
            }
        } else if select
            && response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selection.head = snap(self, frame_at(&self.view, pos), &self.snap_targets);
        } else if select && response.triple_clicked_by(egui::PointerButton::Primary) {
            self.selection = Selection::all(frames);
        } else if select && response.double_clicked_by(egui::PointerButton::Primary) {
            self.selection = Selection::visible(&self.view, width, frames);
        } else if select
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.head_scrub.pos = None;
            let targets = [self.selection.start() as u64, self.selection.end() as u64];
            let at = snap(self, frame_at(&self.view, pos), &targets);
            if shift {
                self.selection.extend_to(at);
            } else {
                self.selection = Selection::at(at);
            }
        }

        // The Marquee tool (B135): a drag on the spectral display draws a time x frequency
        // rectangle, or moves the one the drag began inside.
        if let Some(area) = marquee_hit {
            let axis = self.freq_axis();
            let nyquist = rate as f32 / 2.0;
            let lane_of = |pos: Pos2| -> usize {
                let h = area.height() / channels as f32;
                (((pos.y - area.top()) / h.max(1.0)).floor().max(0.0) as usize).min(channels - 1)
            };
            // The frequency at `pos` on lane `ch` (a drag keeps the lane it began in).
            let hz_at = |pos: Pos2, ch: usize| -> f32 {
                let l = lane(area, ch, channels).shrink2(vec2(0.0, 1.0));
                axis.hz_at_fraction((1.0 - (pos.y - l.top()) / l.height().max(1.0)).clamp(0.0, 1.0))
            };
            if marquee_starts && let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                let (ch, frame0) = (lane_of(origin), frame_at(&self.view, origin));
                let hz0 = hz_at(origin, ch);
                let inside =
                    self.spectral().filter(|s| (s.time.start()..=s.time.end()).contains(&frame0) && (s.band.lo_hz..=s.band.hi_hz).contains(&hz0));
                self.marquee = Some(match inside {
                    Some(from) => MarqueeDrag::Move { from, frame0, hz0, ch },
                    None => MarqueeDrag::Draw { frame0, hz0, ch },
                });
            }
            if let Some(drag) = self.marquee {
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let frame = frame_at(&self.view, pos);
                    match drag {
                        MarqueeDrag::Draw { frame0, hz0, ch } => {
                            let hz = hz_at(pos, ch);
                            self.selection = Selection { anchor: frame0, head: frame };
                            let band = Band { feather_hz: self.feather_hz, feather_ms: self.feather_ms, ..Band::new(hz0, hz, nyquist) };
                            self.set_band(Some(band));
                        }
                        MarqueeDrag::Move { from, frame0, hz0, ch } => {
                            let hz = hz_at(pos, ch);
                            let moved = from.moved(frame as i64 - frame0 as i64, hz - hz0, frames, nyquist);
                            self.selection = moved.time;
                            self.set_band(Some(moved.band));
                        }
                    }
                }
                if response.drag_stopped() {
                    self.marquee = None;
                }
            }
        }
        // Escape drops the band and any marquee in progress (not while a text field has focus).
        if ui.input(|i| i.key_pressed(Key::Escape)) && !ui.ctx().text_edit_focused() {
            self.set_band(None);
            self.marquee = None;
        }
        // Right-click: with the menu setting off, the nearest edge moves to the click (the Select
        // tool, off the ruler); with it on, the context menu opens.
        if self.tool == Tool::Select
            && response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && !ruler_rect.contains(pos)
        {
            self.right_click_at(frame_at(&self.view, pos), frames);
        }
        // With the menu setting on, a right-click opens the editor's menu, or the ruler's over the
        // ruler (A351). The menus are data in `menus.rs`.
        if self.settings.right_click_menu && self.doc.is_some() {
            if response.secondary_clicked() {
                let in_ruler = response.interact_pointer_pos().is_some_and(|p| ruler_rect.contains(p));
                self.menu_area = if in_ruler { Menu::Ruler } else { Menu::Editor };
            }
            let entries = self.menu_area.entries();
            let mut chosen = None;
            response.context_menu(|ui| chosen = self.menu_rows(ui, &entries));
            if let Some(item) = chosen
                && let Err(e) = self.run_item(item, ui.ctx(), false)
            {
                self.notice = Some(e);
            }
        }

        let Some(doc) = &self.doc else { return };
        let (start, end) = (self.view.start, self.view.end(width));
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(rect.left(), ruler.bottom()), rect.max);
        draw_ruler(&painter, ruler, &self.view, rate, self.time_format, self.dark, &self.tempo);
        if let Some(e) = &self.editor {
            let shown = self.markers_ui.shown(e.history.current().markers(), frames as u64);
            draw_markers(&painter, ruler, &self.view, &shown, &self.markers_ui.pick.ids);
            if response.hover_pos().is_some_and(|p| self.markers_ui.handle_at(&shown, &self.view, ruler, p).is_some()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }

        let mut show_spec = self.panels.is_shown("spectral");
        let mut split_pct = self.settings.spec_split_pct;
        let divider = spectral::divider(ui, body, self.editor_shows("waveform"), &mut show_spec, &mut split_pct);
        if show_spec != self.panels.is_shown("spectral") {
            self.panels.set("spectral", show_spec);
            self.spectral_dirty = true;
        }
        self.settings.spec_split_pct = split_pct;
        self.spectral_dirty |= divider.changed;
        let split = divider.y;
        let (wave_area, spectrum_area) = match (self.editor_shows("waveform"), self.editor_shows("spectral")) {
            (true, true) => {
                (Some(Rect::from_min_max(body.min, pos2(body.right(), split))), Some(Rect::from_min_max(pos2(body.left(), split), body.max)))
            }
            (true, false) => (Some(body), None),
            (false, true) => (None, Some(body)),
            (false, false) => (None, None),
        };

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if wave_area.is_some_and(|a| a.contains(pos)) {
                self.hover_next = Some("waveform");
            } else if spectrum_area.is_some_and(|a| a.contains(pos)) {
                self.hover_next = Some("spectral");
            } else if rect.contains(pos) {
                // The ruler and the empty parts of the Editor: the Editor itself (maximise, never close).
                self.hover_next = Some(panels::EDITOR);
            }
        }

        // High-DPI (f76): one column per physical pixel, each one pixel wide (display.rs).
        let ppp = ui.ctx().pixels_per_point();
        let hair = display::hairline(ppp);
        if let Some(area) = wave_area {
            let cols = display::physical_columns(area.width(), ppp);
            let [r, g, b] = self.settings.colors.waveform.unwrap_or_else(|| self.settings.waveform_rgb());
            let wave = Color32::from_rgb(r, g, b);
            let editing = self.editor.as_ref().map_or(wavcraft_core::ChannelMask::ALL, |e| e.channels.within(channels));
            let zoom = self.amp_zoom.get();
            let scale = self.settings.amp_scale;
            // Layered: one lane holds every channel (multichannel files only); stacked: a lane each.
            let layered = self.settings.channel_layout == ChannelLayout::Layered && channels > 1;
            let lanes = if layered { 1 } else { channels };
            for lane_index in 0..lanes {
                let lane = lane(area, lane_index, lanes);
                let mid = lane.center().y;
                painter.line_segment(
                    [pos2(lane.left(), mid), pos2(lane.right(), mid)],
                    Stroke::new(hair, Color32::from_gray(if self.dark { 50 } else { 200 })),
                );
                let half = lane.height() * 0.5 - 2.0;
                let (first, last) = if layered { (0, channels) } else { (lane_index, lane_index + 1) };
                if !layered {
                    let name = chnames.get(lane_index).filter(|n| !n.is_empty());
                    if channels > 1 {
                        // The channel's label chip; a click picks it as the only channel edits act on
                        // (a click on the only one picked gives all channels back). Shift or Ctrl adds or
                        // removes it.
                        let ch = lane_index;
                        let chip = Rect::from_min_size(pos2(lane.left() + 2.0, lane.top() + 1.0), vec2(34.0, 14.0)).intersect(lane);
                        let hit = ui.interact(chip, ui.id().with(("lane-label", ch)), Sense::click());
                        let lit = editing.contains(ch);
                        painter.rect_filled(chip, 2.0, Color32::from_black_alpha(if lit { 170 } else { 90 }));
                        if let Some(name) = name {
                            painter.text(
                                chip.center(),
                                egui::Align2::CENTER_CENTER,
                                name,
                                egui::FontId::monospace(10.0),
                                if lit { Color32::WHITE } else { Color32::GRAY },
                            );
                        }
                        if hit.clicked() {
                            let multi = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                            if let Some(e) = self.editor.as_mut() {
                                let now = e.channels.within(channels);
                                e.channels = if multi {
                                    wavcraft_core::ChannelMask::from_bits(now.bits() ^ wavcraft_core::ChannelMask::only(ch).bits()).within(channels)
                                } else if now == wavcraft_core::ChannelMask::only(ch) {
                                    wavcraft_core::ChannelMask::ALL
                                } else {
                                    wavcraft_core::ChannelMask::only(ch)
                                };
                                if e.channels.within(channels) == wavcraft_core::ChannelMask::NONE {
                                    e.channels = wavcraft_core::ChannelMask::ALL;
                                }
                            }
                        }
                    } else if let Some(name) = name {
                        painter.text(
                            pos2(lane.left() + 4.0, lane.top() + 2.0),
                            egui::Align2::LEFT_TOP,
                            name,
                            egui::FontId::proportional(12.0),
                            Color32::from_gray(170),
                        );
                    }
                }
                for ch in first..last {
                    // Multichannel files draw each channel in its palette colour; a channel turned
                    // off for edits is faint, and layered channels are see-through.
                    let [br, bg, bb] = if channels > 1 { channel_color(ch) } else { [wave.r(), wave.g(), wave.b()] };
                    let lit = editing.covers_all(channels) || editing.contains(ch);
                    let wave_color = match (lit, layered) {
                        (true, true) => Color32::from_rgba_unmultiplied(br, bg, bb, 190),
                        (true, false) => Color32::from_rgb(br, bg, bb),
                        (false, _) => Color32::from_rgba_unmultiplied(br, bg, bb, 70),
                    };
                    for (i, (lo, hi)) in doc.waveform_columns(ch, start, end, cols).into_iter().enumerate() {
                        let x = display::column_x(lane.left(), i, ppp);
                        let (top, bottom) = (mid - (hi * zoom).clamp(-1.0, 1.0) * half, mid - (lo * zoom).clamp(-1.0, 1.0) * half);
                        painter.line_segment([pos2(x, top.min(bottom - hair)), pos2(x, bottom)], Stroke::new(hair, wave_color));
                    }
                }
                let ruler_lane = Rect::from_min_max(pos2(amp_col.left(), lane.top()), pos2(amp_col.right(), lane.bottom()));
                draw_amp_ruler(&painter, ruler_lane, scale, self.amp_zoom);
                // The channel buttons on the ruler: a click turns the channel on or off for edits.
                for ch in first..last {
                    let row = (ch - first) as f32;
                    let button = Rect::from_min_size(pos2(ruler_lane.left() + 2.0, ruler_lane.top() + 2.0 + row * 18.0), vec2(26.0, 16.0));
                    if button.bottom() > ruler_lane.bottom() {
                        continue;
                    }
                    let on = editing.contains(ch);
                    let label = chnames.get(ch).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| (ch + 1).to_string());
                    let resp = ui.interact(button, ui.id().with(("channel-button", ch)), Sense::click());
                    let [r, g, b] = channel_color(ch);
                    painter.rect_filled(button, 3.0, if on { Color32::from_rgb(r, g, b) } else { Color32::from_gray(48) });
                    painter.text(
                        button.center(),
                        egui::Align2::CENTER_CENTER,
                        &label,
                        egui::FontId::monospace(10.0),
                        if on { Color32::BLACK } else { Color32::from_gray(150) },
                    );
                    if resp.on_hover_text(format!("Channel {label}: {} for edits (click to toggle)", if on { "on" } else { "off" })).clicked() {
                        toggled = Some(ch);
                    }
                }
            }
        }
        painter.rect_filled(Rect::from_min_max(amp_col.min, pos2(amp_col.right(), amp_col.top() + RULER_HEIGHT)), 0.0, Color32::from_gray(32));
        let current_scale = self.settings.amp_scale;
        ruler_resp.context_menu(|ui| {
            for s in AmpScale::ALL {
                if ui.radio(current_scale == s, s.menu()).clicked() {
                    chosen_scale = Some(s);
                    ui.close();
                }
            }
        });

        if let Some(area) = spectrum_area {
            // Half resolution across, capped rows: plenty for the eye. The columns sit on a grid
            // fixed to the file, so scrolling moves the picture without changing it.
            // Counted in physical pixels (f76), so a Retina display gets twice the columns and rows.
            let cols = (area.width() * display::sane_scale(ppp) * 0.5).round().clamp(1.0, 2048.0);
            let rows = (area.height() * display::sane_scale(ppp) / channels as f32).round().clamp(1.0, 768.0) as usize;
            let columns = Columns::covering(start, end, self.view.frames_per_px * f64::from(area.width() / cols), &self.params);
            self.renderer.ask(Job { generation: self.generation, columns, rows, params: self.params }, &doc.state);
            if let Some(picture) = self.renderer.take()
                && picture.job.generation == self.generation
                && picture.job.params == self.params
            {
                let texture = |(ch, image)| ui.ctx().load_texture(format!("spectrum-{ch}"), image, TextureOptions::LINEAR);
                self.spectrum = Some(SpectrumCache { job: picture.job, textures: picture.images.into_iter().enumerate().map(texture).collect() });
            }
            if let Some(cache) = &self.spectrum {
                // A picture is drawn where its own frames are on screen. While the next one is
                // being made, the last one still lines up with the ruler and the waveform.
                let (left, right) = (area.left() + self.view.x_of(cache.job.columns.start), area.left() + self.view.x_of(cache.job.columns.end));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                // The picture shows its own band. If the band has moved since, it is placed where
                // that band now falls (and clipped to the lane) until the new picture arrives; one
                // made on the other scale can't be, and waits.
                let (shown, drawn) = (self.freq_axis(), FreqAxis::from_params(&cache.job.params).clamped(rate));
                for (ch, texture) in cache.textures.iter().enumerate() {
                    let lane = lane(area, ch, channels).shrink2(vec2(0.0, 1.0));
                    let y_of = |hz: f32| lane.bottom() - shown.fraction_of_hz(hz) * lane.height();
                    let (top, bottom) = (y_of(drawn.max_hz), y_of(drawn.min_hz));
                    if shown.scale() == drawn.scale() && top.is_finite() && bottom.is_finite() && bottom > top {
                        painter.with_clip_rect(lane).image(
                            texture.id(),
                            Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                            uv,
                            Color32::WHITE,
                        );
                    }
                }
            }
            if Some(area).is_some_and(|a| a.width() > 3.0 * FREQ_RULER_WIDTH) {
                let strip = Rect::from_min_size(area.min, vec2(FREQ_RULER_WIDTH, area.height()));
                let axis = self.freq_axis();
                for ch in 0..channels {
                    let lane = lane(strip, ch, channels).shrink2(vec2(0.0, 1.0));
                    spectral::draw_freq_ruler(&painter, lane, &axis);
                }
            }
        }

        // The spectral band of a marquee, on each lane of the spectral display.
        if let (Some(spectral), Some(area)) = (self.spectral(), spectrum_area) {
            let (x0, x1) = (rect.left() + self.view.x_of(spectral.time.start() as f64), rect.left() + self.view.x_of(spectral.time.end() as f64));
            let axis = self.freq_axis();
            for ch in 0..channels {
                let l = lane(area, ch, channels).shrink2(vec2(0.0, 1.0));
                let y = |hz: f32| {
                    let f = axis.fraction_of_hz(hz);
                    l.bottom() - if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 } * l.height()
                };
                let r = Rect::from_min_max(pos2(x0, y(spectral.band.hi_hz)), pos2(x1, y(spectral.band.lo_hz)));
                let clipped = painter.with_clip_rect(l.intersect(rect));
                clipped.rect_filled(r, 0.0, BAND_FILL);
                clipped.rect_stroke(r, 0.0, Stroke::new(1.0, BAND_OUTLINE), egui::StrokeKind::Inside);
            }
        }
        divider.draw(&painter, body, self.editor_shows("waveform"));

        // Selection, cursor and playhead over everything.
        if !self.selection.is_empty() {
            let (a, b) = (rect.left() + self.view.x_of(self.selection.start() as f64), rect.left() + self.view.x_of(self.selection.end() as f64));
            let (x0, x1) = (a.max(rect.left()), b.min(rect.right()));
            // A disabled channel takes no part in the selection (A132): fill only the enabled lanes.
            let enabled = self.editor.as_ref().map_or(wavcraft_core::ChannelMask::ALL, |e| e.channels.within(channels));
            let areas: Vec<Rect> = [wave_area, spectrum_area].into_iter().flatten().collect();
            if areas.is_empty() {
                painter.rect_filled(Rect::from_min_max(pos2(x0, body.top()), pos2(x1, body.bottom())), 0.0, selection_colour);
            }
            for area in areas {
                for ch in (0..channels).filter(|&c| enabled.contains(c)) {
                    let l = lane(area, ch, channels);
                    painter.rect_filled(Rect::from_min_max(pos2(x0, l.top()), pos2(x1, l.bottom())), 0.0, selection_colour);
                }
            }
        }
        let line = |frame: usize, color: Color32| {
            let x = rect.left() + self.view.x_of(frame as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, color));
            }
        };
        line(self.selection.start(), cursor_colour);
        let head = if self.previewing {
            None
        } else if self.playhead_live() {
            Some(self.player.position())
        } else {
            self.head_scrub.pos
        };
        if let Some(at) = head {
            // The playhead: a heavier line from a marker on the ruler, with its time.
            let x = rect.left() + self.view.x_of(at as f64);
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment([pos2(x, ruler.top()), pos2(x, rect.bottom())], Stroke::new(2.0, playhead_colour));
                let top = ruler.top();
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(x - 6.0, top), pos2(x + 6.0, top), pos2(x, top + 10.0)],
                    playhead_colour,
                    Stroke::NONE,
                ));
                let galley = painter.layout_no_wrap(self.fmt_time(at), egui::FontId::monospace(11.0), playhead_colour);
                let label_at = pos2((x + 9.0).min(rect.right() - galley.size().x - 4.0), top + 1.0);
                painter.rect_filled(Rect::from_min_size(label_at, galley.size()).expand(2.0), 2.0, ground);
                painter.galley(label_at, galley, playhead_colour);
            }
        }
        if self.player.is_playing() && !self.previewing {
            ui.ctx().request_repaint();
        }

        if let Some(scale) = chosen_scale {
            self.set_amp_scale(scale);
        }
        if let Some(ch) = toggled
            && let Err(why) = self.run_action(ActionId::ToggleChannel(ch as u8), ui.ctx())
        {
            self.error = Some(why);
        }
        // Each editor panel's ≡ menu sits in the top right corner of its area.
        let mut chosen = None;
        for (id, area) in [("waveform", wave_area), ("spectral", spectrum_area)] {
            if let Some(area) = area {
                let at = Rect::from_min_size(pos2(area.right() - 28.0, area.top() + 2.0), vec2(26.0, 20.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                    if let Some(item) = panel_menu_button(ui, self.layout.maximized(&self.panels) == Some(id), id) {
                        chosen = Some((id, item));
                    }
                });
            }
        }
        if let Some((id, item)) = chosen
            && let Err(e) = self.run_panel_item(id, item)
        {
            self.notice = Some(e);
        }
    }
}

/// The ≡ button of a panel's header and its menu (A048); the id of the item clicked, if any.
fn panel_menu_button(ui: &mut egui::Ui, maximized: bool, id: &str) -> Option<&'static str> {
    let mut chosen = None;
    ui.menu_button("≡", |ui| {
        for item in panels::menu(id, maximized) {
            if ui.button(item.label).clicked() {
                chosen = Some(item.id);
                ui.close();
            }
        }
    });
    chosen
}

impl eframe::App for WaveApp {
    /// Runs before every frame, and also while the window is hidden, minimised or covered (eframe
    /// skips `ui` then, but still calls this on `request_repaint`): the workers' results and the
    /// control channel must not wait for the window to be shown (the 2026-10-19 by-eye check
    /// found the channel silent behind another window).
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.keeper.tick(ctx, frame);
        self.finish_opening();
        self.pump_open_queue(ctx);
        self.finish_appending(ctx);
        self.finish_inserting(ctx);
        if let Some(rx) = &self.profile_rx {
            match rx.try_recv() {
                Ok(names) => {
                    self.profile_rx = None;
                    let outcome = self.activate_setup(&names, &names);
                    log::info!("audio setup: {outcome:?}");
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.profile_rx = None,
            }
        }
        self.finish_rebuild();
        self.finish_job(ctx);
        self.finish_saving(ctx);
        self.finish_making(ctx);
        self.pump_save_all(ctx);
        self.pump_close_all();
        self.finish_batch();
        self.finish_integration(ctx);
        if self.rec.punch_waiting() {
            let at = self.player.is_playing().then(|| self.player.position());
            self.rec.punch_tick(at);
            ctx.request_repaint_after(std::time::Duration::from_millis(4));
        }
        if let Some(why) = self.rec.refresh() {
            log::warn!(target: "wavcraft", "recording: {why}");
            self.finish_recording(true, ctx);
        }
        if self.rec.max_reached() {
            log::info!(target: "wavcraft", "recording: the time limit is reached, stopping");
            self.finish_recording(true, ctx);
        }
        if let Some((fmt, max)) = self.rec.due() {
            self.start_timed(fmt, max);
        }
        if self.rec.is_active() || self.rec.is_armed() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.poll_transport();
        self.run_scrub();
        // Follow a change of the system's default output device (f48): reopen at the same place.
        self.player.maintain();
        self.diag.poll();
        self.sync_input();
        let peaks = self.meter_peaks();
        self.meters.update(peaks);
        self.finish_match(ctx);
        self.poll_mic_check();
        if self.meters.animating() || self.matcher.animating() || self.player.is_playing() {
            ctx.request_repaint();
        }
        if self.input.is_open() || self.mic.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        self.stats.poll();
        self.recovery.poll();
        let unsaved = self.unsaved();
        self.recovery.configure(self.settings.autosave_enabled, self.settings.autosave_interval_secs);
        self.recovery.track(self.editor.as_ref(), unsaved, ctx);
        self.batch.poll();
        self.playlist_ui.tick(&mut self.player);
        if self.playlist_ui.running() {
            ctx.request_repaint();
        }
        while let Some(req) = self.control.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if let Some(rest) = req.line.strip_prefix("MICCHECK") {
                // The answer comes when the time is up, not now.
                self.start_mic_check(rest.trim(), req.reply);
                continue;
            }
            // `VIDEO frame` is answered when the worker has the picture, not before. With a
            // multitrack session holding video clips the time is a session time (f129): the
            // picture comes from the clip under it, at the time into that clip's movie.
            if let Some(arg) = req.line.strip_prefix("VIDEO frame")
                && let Some(mt) = self.mtv.mt.as_mut().filter(|m| m.has_video())
                && let Ok(secs) = arg.trim().parse::<f64>()
            {
                let _ = mt.set_playhead(secs);
                match mt.video_at(secs) {
                    Some(shot) => {
                        self.sync_session_video(&shot.path, ctx);
                        if let Some(v) = self.video.as_mut() {
                            let seq = v.request(shot.movie_secs, true);
                            self.video_waiters.add(seq, req.reply);
                        }
                    }
                    None => {
                        let _ = req.reply.send(format!("ERR no video clip at {secs} s"));
                    }
                }
                continue;
            }
            if let Some(arg) = req.line.strip_prefix("VIDEO frame") {
                match (arg.trim().parse::<f64>(), self.video.as_mut()) {
                    (Ok(secs), Some(v)) if secs.is_finite() => {
                        let seq = v.request(secs, true);
                        self.video_waiters.add(seq, req.reply);
                    }
                    (_, None) => {
                        let _ = req.reply.send("ERR no video attached".to_string());
                    }
                    _ => {
                        let _ = req.reply.send("ERR VIDEO frame <seconds>".to_string());
                    }
                }
                continue;
            }
            let answer = self.control_request(&req.line, ctx);
            if std::mem::take(&mut self.diag_wait) {
                // A scan under way answers this request when it ends (before the next frame draws it).
                self.diag.reply = Some(req.reply);
                continue;
            }
            let _ = req.reply.send(answer);
        }
        if let Some(v) = self.video.as_mut() {
            for (seq, text) in v.poll(ctx) {
                self.video_waiters.answer(seq, text);
            }
        }
    }

    fn on_exit(&mut self) {
        self.keeper.save_now();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_look(&ctx);
        if self.rec.is_active() {
            // Closing the window ends the take cleanly; a kill is covered by the header.
            if ctx.input(|i| i.viewport().close_requested()) {
                self.finish_recording(false, &ctx);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if let Some(fmt) = self.rec.dialog(&ctx, self.editor.is_some()) {
            // Errors show in the status bar.
            match self.rec.dialog_mode {
                Some(mode) if self.editor.is_some() => {
                    let _ = self.start_recording_into(fmt, mode, self.rec.dialog_target, DEFAULT_PREROLL_MS);
                }
                _ => {
                    let _ = self.start_recording(fmt);
                }
            }
        }
        self.close_prompt(&ctx);
        self.export_window(&ctx);
        self.selection_keys(&ctx);
        // Up and Down belong to the History panel while one of its rows has focus (before the
        // action table gets them: there they pick the left or right channel).
        if self.history_focus && self.saving.is_none() && self.running.is_none() {
            let (up, down) =
                ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)));
            if let Some(to) = (up || down).then(|| self.history_arrow(down, &ctx)).flatten() {
                self.history_refocus = Some(to);
            }
        }
        // ` maximises the panel under the pointer (and restores); Ctrl+W closes it.
        let typing = ctx.egui_wants_keyboard_input();
        let (grave, close) =
            ctx.input_mut(|i| (!typing && i.consume_key(egui::Modifiers::NONE, Key::Backtick), i.consume_key(egui::Modifiers::COMMAND, Key::W)));
        if grave {
            if self.layout.maximized(&self.panels).is_some() {
                self.layout.restore();
            } else if let Some(id) = self.hover {
                self.layout.toggle_maximize(id);
            }
        }
        if close && let Some(id) = self.hover {
            self.close_panel(id);
        }
        // Shift+Space: record now (C031); while recording it stops the take.
        if !ctx.egui_wants_keyboard_input() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Space)) {
            if self.rec.is_active() {
                self.finish_recording(true, &ctx);
            } else if self.rec.is_armed() {
                self.rec.disarm();
            } else {
                let _ = self.quick_record();
            }
        }
        // While the shortcuts dialog waits for a chord, or its search box has the keyboard, keys
        // are not commands (f46).
        let keys_for_dialog =
            self.shortcut_dialog.as_ref().is_some_and(|d| d.capturing()) || (self.shortcut_dialog.is_some() && ctx.egui_wants_keyboard_input());
        let pressed = if keys_for_dialog { Vec::new() } else { pressed_commands(&ctx, &self.keymap) };
        for command in pressed {
            if let Err(e) = self.run_command(command, &ctx) {
                // A key that can't act now (no file open, a save running) is not an error to show,
                // except a failed save, which the old key handler reported.
                if matches!(command, Command::Save | Command::SaveAs) {
                    self.error = Some(e);
                }
            }
        }
        // The JKL shuttle (f36): J plays backward, K stops, L plays forward; K held with J or L crawls.
        if !typing {
            let (presses, k_down, j_down, l_down) = ctx.input(|i| {
                let presses: Vec<Key> = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }
                            if matches!(key, Key::J | Key::K | Key::L) && !modifiers.command && !modifiers.alt && !modifiers.shift =>
                        {
                            Some(*key)
                        }
                        _ => None,
                    })
                    .collect();
                (presses, i.key_down(Key::K), i.key_down(Key::J), i.key_down(Key::L))
            });
            for key in presses {
                let c = match key {
                    Key::J => 'j',
                    Key::L => 'l',
                    _ => 'k',
                };
                self.shuttle_key(c, k_down && c != 'k');
            }
            let nudge = self.transport.shuttle.nudge;
            if nudge != 0 && !self.hold.ctl_nudge && !(if nudge > 0 { l_down } else { j_down }) {
                self.release_nudge();
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Comma)) {
            self.open_preferences();
        }
        // Shift held as the window comes up: reset the preferences (once).
        if self.shift_frames > 0 {
            self.shift_frames -= 1;
            if ctx.input(|i| i.modifiers.shift) {
                self.shift_frames = 0;
                if let Err(e) = self.reset_preferences() {
                    self.error = Some(e);
                }
            }
        }
        let (dropped, hovering, dropped_without_path) = ctx.input(|i| {
            (
                dropped_path(&i.raw.dropped_files),
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty() && dropped_path(&i.raw.dropped_files).is_none(),
            )
        });
        if let Some(path) = dropped {
            self.open(path, &ctx);
        } else if dropped_without_path {
            self.error = Some("The dropped item has no file path here, so it was not opened (use File › Open)".to_string());
        }
        if hovering {
            // The OS is carrying a file over the window: say that dropping opens it.
            egui::Area::new(egui::Id::new("drop-hint")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
                &ctx,
                |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.heading("Drop a file to open it"));
                },
            );
        }
        let star = if self.unsaved() { "*" } else { "" };
        let title = if self.editor.is_some() { format!("{star}{} — WavCraft", path_name(&self.doc_path)) } else { "WavCraft".to_string() };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if self.docs.len(self.editor.is_some()) > 1 {
            egui::Panel::top("docs").show(ui, |ui| self.doc_tabs(ui));
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::top("tools").show(ui, |ui| self.tools_bar(ui));
        egui::Panel::top("zoom").show(ui, |ui| self.zoom_panel(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.meters.show_levels {
            egui::Panel::bottom("levels").show(ui, |ui| {
                self.input_strip(ui);
                self.meters.panel(ui);
            });
        }
        if self.meters.show_stats {
            let target = self.editor.as_ref().map(|e| {
                let state = e.history.current().clone();
                let (range, scope) = if self.selection.is_empty() {
                    (0..state.len_frames(), "the whole file")
                } else {
                    (self.selection.start() as u64..self.selection.end() as u64, "the selection")
                };
                (state, range, scope.to_string())
            });
            self.meters.stats_window(&ctx, target);
        }
        // In the Multitrack view the panel follows the session playhead through the Video track.
        let session_shot = if self.mtv.show {
            self.mtv.mt.as_ref().and_then(|m| {
                let secs =
                    if self.mtv.transport.is_playing() { self.mtv.transport.position() as f64 / f64::from(m.rate()) } else { m.playhead_seconds() };
                m.video_at(secs)
            })
        } else {
            None
        };
        if let Some(shot) = &session_shot {
            self.sync_session_video(&shot.path, &ctx);
        }
        if self.show_video && self.video.is_some() {
            let secs = if session_shot.is_some() { session_shot.as_ref().map(|s| s.movie_secs) } else { self.playhead_secs() };
            if let Some(v) = self.video.as_mut() {
                if let Some(secs) = secs {
                    v.follow(secs);
                }
                egui::Panel::right("video").default_size(360.0).show(ui, |ui| v.ui(ui));
            }
        }
        if self.show_time_panel {
            egui::Panel::bottom("time_panel").show(ui, |ui| self.time_panel(ui));
        }
        if self.show_metadata {
            egui::Panel::right("metadata").default_size(260.0).show(ui, |ui| self.metadata_panel(ui));
        }
        if self.show_mixer {
            egui::Panel::bottom("mixer")
                .resizable(true)
                .default_size(260.0)
                .show(ui, |ui| self.mixer.show(ui, self.mtv.mt.as_mut().map(|m| &mut m.session)));
        }
        self.mtv.dialog_ui(&ctx);
        self.mtv.props_ui(&ctx);
        if let Some((track, i)) = self.mtv.edit_request.take() {
            let r = self.edit_clip_source(track, i, &ctx);
            self.report(r);
        }
        if self.clip_edit.is_some() && !self.mtv.show {
            egui::Panel::top("clip_edit_bar").show(ui, |ui| {
                ui.horizontal(|ui| {
                    let name = self.clip_edit.as_ref().map(|c| c.name.clone()).unwrap_or_default();
                    ui.label(format!("Editing the source of clip \u{201c}{name}\u{201d}: its clips change when you return."));
                    if ui.button("Return to session").clicked() {
                        let r = self.return_to_session(&ctx).map(|_| ());
                        self.report(r);
                    }
                });
            });
        }
        self.tab_rects.clear();
        self.zone_rects.clear();
        self.hover_next = None;
        let maximized = self.layout.maximized(&self.panels).filter(|id| panels::is_movable(id)).map(String::from);
        if let Some(id) = maximized {
            // One docked panel fills the window; the tab strips are not drawn.
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(panels::by_id(&id).map_or("", |p| p.title));
                    if ui.small_button("Restore (`)").clicked() {
                        self.layout.restore();
                    }
                });
                ui.separator();
                self.panel_body(&id, ui);
                if ui.rect_contains_pointer(ui.max_rect()) {
                    self.hover_next = panels::by_id(&id).map(|p| p.id);
                }
            });
        } else {
            if self.layout.maximized(&self.panels).is_none() {
                self.dock_zone(ui, DockSide::Left);
                self.dock_zone(ui, DockSide::Right);
                self.dock_zone(ui, DockSide::Bottom);
            }
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                if self.cdv.show {
                    let current = self.editor.as_ref().map(|e| e.history.current().clone());
                    let (name, sessions) = (self.doc.as_ref().map(|d| d.name()).unwrap_or_default(), self.sessions.path.clone());
                    self.cdv.ui(ui, current, &name, &sessions);
                } else if self.mtv.show {
                    self.mtv.ui(ui, self.doc.as_ref());
                } else {
                    self.editor(ui);
                    // The divider or the display options changed: save once the pointer is up.
                    if self.spectral_dirty && !ui.ctx().input(|i| i.pointer.any_down()) {
                        self.save_spectral_now();
                    }
                }
            });
            if self.layout.maximized(&self.panels).is_none() {
                self.dividers(ui);
            }
        }
        self.floating_windows(&ctx);
        self.keep_freq_axis(&ctx);
        self.insert_silence_dialog(&ctx);
        self.convert_dialogs(&ctx);
        self.shortcut_dialog_ui(&ctx);
        self.new_audio_dialog(&ctx);
        self.tempo_dialog(&ctx);
        self.effect_dialog_ui(&ctx);
        self.mic_check_dialog(&ctx);
        self.preferences_dialog(&ctx);
        match self.recovery.show(&ctx) {
            Some(Choice::Recover(i)) => {
                if let Err(e) = self.recover_session(i, &ctx) {
                    self.error = Some(e);
                }
            }
            Some(Choice::Discard(i)) => {
                if let Err(e) = self.discard_recovery(i) {
                    self.error = Some(e);
                }
            }
            Some(Choice::RecoverTake(i)) => {
                if let Err(e) = self.recover_take(i, &ctx) {
                    self.error = Some(e);
                }
            }
            Some(Choice::DiscardTake(i)) => {
                if let Err(e) = self.discard_take(i) {
                    self.error = Some(e);
                }
            }
            None => {}
        }
        self.rack_ui(&ctx);
        self.diagnostics_ui(&ctx);
        self.raw_import_dialog(&ctx);
        self.mix_paste_dialog(&ctx);
        self.custom_fps_dialog(&ctx);
        self.clear_history_prompt(&ctx);
        self.reset_prefs_prompt(&ctx);
        self.about_window(&ctx);
        self.metadata_display_dialog(&ctx);
        self.batch.ui(&ctx);
        self.draw_hud(&ctx);
        {
            let current = self.editor.as_ref().map(|e| e.history.current().clone());
            let scope = (!self.selection.is_empty()).then(|| self.selection_range());
            let (scan, nav) = self.stats.ui(&ctx, current.as_ref(), scope);
            if scan {
                self.start_stats(&ctx);
            }
            if let Some(nav) = nav {
                match self.stats_nav(nav) {
                    Ok(()) => self.error = None,
                    Err(e) => self.error = Some(format!("Sample Statistics: {e}")),
                }
            }
        }
        self.dock_drag(&ctx);
        self.hover = self.hover_next;
        if self.layout_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.save_panels();
        }
    }
}

/// The one reset of the preferences on disk, for every path (the dialog's button, Shift at launch,
/// `--reset-preferences`, File › Restore Preference Defaults and `PREF reset`). The settings and
/// shortcut files in `dir` are moved aside as `.reset-N` (see `bundle::reset_preferences`), and the
/// active shortcut set is forgotten so no saved set comes back at the next start. Returns what
/// to say; an error leaves the files as they were.
fn reset_preference_files(dir: &Path) -> Result<String, String> {
    let moved = bundle::reset_preferences(dir).map_err(|e| {
        log::warn!(target: "wavcraft", "could not reset the preferences in {}: {e}", dir.display());
        format!("could not reset the preferences: {e}")
    })?;
    if let Err(e) = SetStore::new(dir).set_active(None) {
        log::warn!(target: "wavcraft", "could not forget the active shortcut set: {e}");
    }
    if moved.is_empty() {
        return Ok("Preferences were already the defaults".to_string());
    }
    log::info!(target: "wavcraft", "preferences reset in {}: {}", dir.display(), moved.join(", "));
    Ok(format!("Preferences reset to their defaults; the old files were kept beside them as .reset-N ({})", moved.join(", ")))
}

/// The sample formats the Raw Data dialog offers.
const RAW_CHOICES: [(&str, RawEncoding); 7] = [
    ("8-bit unsigned", RawEncoding::U8),
    ("8-bit signed", RawEncoding::S8),
    ("16-bit integer", RawEncoding::S16),
    ("24-bit integer", RawEncoding::S24),
    ("32-bit integer", RawEncoding::S32),
    ("32-bit float", RawEncoding::F32),
    ("64-bit float", RawEncoding::F64),
];

/// The Raw Data dialog's fields, as typed.
struct RawDialog {
    path: PathBuf,
    rate: String,
    channels: String,
    encoding: usize,
    big_endian: bool,
    offset: String,
}

impl RawDialog {
    fn new(path: PathBuf) -> RawDialog {
        RawDialog { path, rate: "44100".into(), channels: "2".into(), encoding: 2, big_endian: false, offset: "0".into() }
    }

    fn spec(&self) -> Result<RawSpec, String> {
        let rate = self.rate.trim().parse::<u32>().ok().filter(|r| (1..=wavcraft_core::io::RAW_MAX_RATE_HZ).contains(r));
        let channels = self.channels.trim().parse::<u16>().ok().filter(|c| (1..=wavcraft_core::io::RAW_MAX_CHANNELS).contains(c));
        let offset = self.offset.trim().parse::<u64>().ok();
        match (rate, channels, offset) {
            (Some(sample_rate), Some(channels), Some(offset)) => {
                Ok(RawSpec { sample_rate, channels, encoding: RAW_CHOICES[self.encoding].1, big_endian: self.big_endian, offset })
            }
            (None, ..) => Err(format!("the sample rate must be 1 to {} Hz", wavcraft_core::io::RAW_MAX_RATE_HZ)),
            (_, None, _) => Err(format!("channels must be 1 to {}", wavcraft_core::io::RAW_MAX_CHANNELS)),
            _ => Err("the start offset must be a whole number of bytes".to_string()),
        }
    }
}

/// `IMPORT_RAW <path> rate=<hz> ch=<n> fmt=<s16le|...> [offset=<bytes>]`: the path is everything
/// before the first ` key=` word, so it may hold spaces.
fn parse_import_raw(rest: &str) -> Result<(PathBuf, RawSpec), String> {
    const USAGE: &str = "IMPORT_RAW <path> rate=<hz> ch=<n> fmt=<u8|s8|s16|s24|s32|f32|f64><le|be> [offset=<bytes>]";
    let words: Vec<&str> = rest.split_whitespace().collect();
    let first_key = words.iter().position(|w| w.contains('=')).ok_or(USAGE)?;
    let path = words[..first_key].join(" ");
    if path.is_empty() {
        return Err(USAGE.to_string());
    }
    let (mut rate, mut ch, mut fmt, mut offset) = (None, None, None, 0u64);
    for w in &words[first_key..] {
        let (k, v) = w.split_once('=').ok_or(USAGE)?;
        match k {
            "rate" => rate = v.parse::<u32>().ok(),
            "ch" => ch = v.parse::<u16>().ok(),
            "fmt" => fmt = RawEncoding::parse(v),
            "offset" => offset = v.parse::<u64>().map_err(|_| "offset must be a whole number of bytes".to_string())?,
            _ => return Err(format!("unknown field {k:?}; {USAGE}")),
        }
    }
    let (Some(sample_rate), Some(channels), Some((encoding, order))) = (rate, ch, fmt) else { return Err(USAGE.to_string()) };
    Ok((PathBuf::from(path), RawSpec { sample_rate, channels, encoding, big_endian: order.unwrap_or(false), offset }))
}

/// The History step an Up or Down key moves to from `cursor` among `len` steps (none at an end).
fn history_arrow_target(cursor: usize, len: usize, down: bool) -> Option<usize> {
    if down { (cursor + 1 < len).then_some(cursor + 1) } else { cursor.checked_sub(1) }
}

/// The file to open from the files dropped on the window this frame: the first with a path. The
/// path is empty or missing where the platform hands over no file (a browser, some Wayland
/// compositors, #66); then nothing is opened.
fn dropped_path(files: &[Arc<dyn egui::DroppedFile + Send + Sync>]) -> Option<PathBuf> {
    files.iter().map(|f| f.path()).find(|p| !p.as_os_str().is_empty()).map(std::path::Path::to_path_buf)
}

/// The paths of a control line: tab-separated when there is a tab, else by spaces.
fn split_paths(s: &str) -> Vec<PathBuf> {
    let parts: Vec<&str> = if s.contains('\t') { s.split('\t').collect() } else { s.split_whitespace().collect() };
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from).collect()
}

/// The commands whose chords were pressed this frame. Chords with more modifiers are taken first,
/// so Cmd+Shift+Z (Redo) is not read as Cmd+Z (Undo).
fn pressed_commands(ctx: &egui::Context, keymap: &Keymap) -> Vec<Command> {
    // A text field being edited owns the keyboard: Backspace, Delete and Cmd+Z there edit the text, not the audio (#368).
    if ctx.text_edit_focused() {
        return Vec::new();
    }
    let mut rows: Vec<(Command, Shortcut)> = keymap.bindings().iter().map(|b| (b.command, b.chord)).collect();
    rows.sort_by_key(|(_, k)| std::cmp::Reverse(u8::from(k.cmd) + u8::from(k.shift) + u8::from(k.alt)));
    ctx.input_mut(|i| {
        // Alt+1 .. Alt+8 enable or disable channel 1 .. 8 (A132).
        let mut pressed: Vec<Command> = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8]
            .into_iter()
            .enumerate()
            .filter_map(|(n, key)| i.consume_key(egui::Modifiers::ALT, key).then_some(Command::Action(ActionId::ToggleChannel(n as u8))))
            .collect();
        pressed.extend(rows.iter().filter_map(|(command, k)| {
            let key = Key::from_name(k.key)?;
            let mut mods = egui::Modifiers::NONE;
            if k.cmd {
                mods |= egui::Modifiers::COMMAND;
            }
            if k.shift {
                mods |= egui::Modifiers::SHIFT;
            }
            if k.alt {
                mods |= egui::Modifiers::ALT;
            }
            // Play/Stop must not toggle again on auto-repeat during a long press.
            let fresh = *command != Command::PlayStop
                || i.events.iter().any(|e| matches!(e, egui::Event::Key { key: pressed, pressed: true, repeat: false, .. } if *pressed == key));
            (i.consume_key(mods, key) && fresh).then_some(*command)
        }));
        pressed
    })
}

/// A row with a combo box for a named choice (`current` is shown; a pick asks to set `key`).
fn pref_choice<'a>(
    ui: &mut egui::Ui,
    label: &str,
    key: &'static str,
    current: &str,
    all: impl Iterator<Item = &'a str>,
    action: &mut Option<PrefAction>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(key).selected_text(current).show_ui(ui, |ui| {
            for name in all {
                if ui.selectable_label(current == name, name).clicked() {
                    *action = Some(PrefAction::Set(key, name.to_string()));
                }
            }
        });
    });
}

/// A row with a number box (dragging or typing asks to set `key`).
fn pref_number(
    ui: &mut egui::Ui,
    label: &str,
    key: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    unit: &str,
    action: &mut Option<PrefAction>,
) {
    ui.horizontal(|ui| {
        let mut v = value;
        let speed = (range.end() - range.start()) / 400.0;
        if ui.add(egui::DragValue::new(&mut v).range(range).speed(speed).max_decimals(2).suffix(format!(" {unit}"))).changed() {
            *action = Some(PrefAction::Set(key, v.to_string()));
        }
        ui.label(label);
    });
}

/// A check box for a true/false preference.
fn pref_check(ui: &mut egui::Ui, label: &str, key: &'static str, on: bool, action: &mut Option<PrefAction>) {
    let mut v = on;
    if ui.checkbox(&mut v, label).changed() {
        *action = Some(PrefAction::Set(key, v.to_string()));
    }
}

/// `c` with each colour channel multiplied by `b` (1: unchanged), kept in range.
fn brighten(c: Color32, b: f32) -> Color32 {
    let m = |v: u8| (f32::from(v) * b).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

/// The check items for every snap target, shared by View › Snapping: the action whose item was
/// clicked.
fn snap_items(ui: &mut egui::Ui, snap: &SnapSettings) -> Option<ActionId> {
    let mut chosen = None;
    for a in TABLE.iter().filter(|a| a.name.starts_with("view.snap.")) {
        let mut on = actions::snap_flag_value(snap, a.id).unwrap_or(false);
        if ui.checkbox(&mut on, a.menu).changed() {
            chosen = Some(a.id);
        }
    }
    chosen
}

/// A Zoom to Time slot's span for a menu or tooltip: "250 ms", "5 s", "2 min".
fn slot_text(secs: f64) -> String {
    if secs < 1.0 {
        format!("{} ms", (secs * 1000.0).round())
    } else if secs < 60.0 {
        format!("{} s", (secs * 100.0).round() / 100.0)
    } else {
        format!("{} min", (secs / 60.0 * 100.0).round() / 100.0)
    }
}

/// The vertical slice of `area` for channel `ch` of `channels`.
fn lane(area: Rect, ch: usize, channels: usize) -> Rect {
    let h = area.height() / channels.max(1) as f32;
    Rect::from_min_max(pos2(area.left(), area.top() + h * ch as f32), pos2(area.right(), area.top() + h * (ch + 1) as f32))
}

/// The amplitude ruler for one lane: marks and numbers on both sides of the centre line, at the
/// current vertical zoom.
fn draw_amp_ruler(painter: &egui::Painter, lane: Rect, scale: AmpScale, zoom: AmpZoom) {
    painter.rect_filled(lane, 0.0, Color32::from_gray(28));
    let (mid, half) = (lane.center().y, lane.height() * 0.5 - 2.0);
    let font = egui::FontId::monospace(10.0);
    let ink = Color32::from_gray(170);
    for tick in scale.ticks(zoom, half, 16.0) {
        let offset = tick.amp * zoom.get() * half;
        for sign in [1.0_f32, -1.0] {
            if tick.amp == 0.0 && sign < 0.0 {
                continue;
            }
            let y = mid - sign * offset;
            painter.line_segment([pos2(lane.right() - 5.0, y), pos2(lane.right(), y)], Stroke::new(1.0, ink));
            let anchor = if y - lane.top() < 6.0 {
                egui::Align2::RIGHT_TOP
            } else if lane.bottom() - y < 6.0 {
                egui::Align2::RIGHT_BOTTOM
            } else {
                egui::Align2::RIGHT_CENTER
            };
            painter.text(pos2(lane.right() - 7.0, y), anchor, scale.label(sign * tick.amp), font.clone(), ink);
        }
    }
    painter.line_segment([pos2(lane.right() - 0.5, lane.top()), pos2(lane.right() - 0.5, lane.bottom())], Stroke::new(1.0, Color32::from_gray(60)));
}

/// Tick marks and times along the top, at a round step that leaves room for the labels.
fn draw_ruler(painter: &egui::Painter, rect: Rect, view: &Viewport, rate: u32, fmt: TimeFormat, dark: bool, tempo: &Tempo) {
    let (ground, tick, text) = if dark { (32, 140, 190) } else { (225, 110, 60) };
    painter.rect_filled(rect, 0.0, Color32::from_gray(ground));
    let seconds_per_px = view.frames_per_px / f64::from(rate.max(1));
    let step = fmt.tick_step_with(seconds_per_px * 110.0, rate, tempo);
    let first = (view.start / f64::from(rate.max(1)) / step).floor() as i64;
    // Fine ticks between the labelled ones, when they are at least 4 px apart (A107).
    let parts = fmt.subdivisions_with(step, rate, tempo);
    let minor_px = step / seconds_per_px.max(1e-12) / f64::from(parts);
    if parts > 1 && minor_px >= 4.0 {
        for k in first * i64::from(parts)..(first + 200) * i64::from(parts) {
            let t = k as f64 * step / f64::from(parts);
            let x = rect.left() + view.x_of(t * f64::from(rate));
            if x > rect.right() {
                break;
            }
            if x < rect.left() || t < 0.0 || k % i64::from(parts) == 0 {
                continue;
            }
            painter.line_segment([pos2(x, rect.bottom() - 3.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(90)));
        }
    }
    for k in first..first + 200 {
        let t = k as f64 * step;
        let x = rect.left() + view.x_of(t * f64::from(rate));
        if x > rect.right() {
            break;
        }
        if x < rect.left() || t < 0.0 {
            continue;
        }
        painter.line_segment([pos2(x, rect.bottom() - 6.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(tick)));
        painter.text(
            pos2(x + 3.0, rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            fmt.format_tick_with(t, rate, tempo),
            egui::FontId::monospace(11.0),
            Color32::from_gray(text),
        );
    }
}

/// Markers on the ruler: a point is a small flag with its name, a range a band with a flag at
/// each end of the name.
fn draw_markers(painter: &egui::Painter, rect: Rect, view: &Viewport, markers: &wavcraft_core::Markers, picked: &[u64]) {
    let painter = painter.with_clip_rect(rect);
    for m in markers.iter() {
        let own = m
            .color
            .checked_sub(1)
            .and_then(|n| wavcraft_core::PALETTE.get(usize::from(n)))
            .map_or(Color32::from_rgb(240, 190, 60), |(_, c)| Color32::from_rgb(c[0], c[1], c[2]));
        let color = if picked.contains(&m.id) { Color32::from_rgb(255, 255, 255) } else { own };
        let x0 = rect.left() + view.x_of(m.start as f64);
        let x1 = rect.left() + view.x_of(m.end.max(m.start) as f64);
        if x1 < rect.left() - 200.0 || x0 > rect.right() {
            continue;
        }
        if !m.is_point() {
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, rect.bottom() - 5.0), pos2(x1.max(x0 + 1.0), rect.bottom())),
                0.0,
                color.gamma_multiply(0.6),
            );
        }
        painter.line_segment([pos2(x0, rect.top()), pos2(x0, rect.bottom())], Stroke::new(1.0, color));
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(x0, rect.top()), pos2(x0 + 8.0, rect.top()), pos2(x0, rect.top() + 8.0)],
            color,
            Stroke::NONE,
        ));
        painter.text(pos2(x0 + 10.0, rect.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, &m.name, egui::FontId::proportional(10.0), color);
    }
}

/// The keys of the colour items, in the order of [`appearance::ITEMS`].
const COLOR_KEYS: [&str; 5] = ["color.waveform", "color.selection", "color.cursor", "color.playhead", "color.background"];

/// The colour an item has when nothing overrides it, as red, green, blue.
fn default_rgb(item: &str, settings: &Settings, dark: bool) -> [u8; 3] {
    let c = match item {
        "waveform" => return settings.waveform_rgb(),
        "selection" => SELECTION,
        "cursor" => CURSOR,
        "playhead" => PLAYHEAD,
        _ if dark => BACKGROUND,
        _ => BACKGROUND_LIGHT,
    };
    [c.r(), c.g(), c.b()]
}

/// `v` in thousandths, for comparing settings that are floats.
fn milli(v: f32) -> i32 {
    (v * 1000.0).round() as i32
}

/// `c` with its distance from mid grey multiplied by `k` (1: unchanged), kept in range.
fn contrast(c: Color32, k: f32) -> Color32 {
    let m = |v: u8| ((f32::from(v) - 128.0) * k + 128.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

/// 256 colours along `which`'s ramp (quiet to loud), with brightness and contrast applied.
fn palette(which: settings::SpecPalette, brightness: f32, contrast_k: f32) -> Vec<Color32> {
    let stops = which.stops();
    (0..256)
        .map(|i| {
            let t = i as f32 / 255.0;
            let hi = stops.iter().position(|s| s.0 >= t).unwrap_or(stops.len() - 1).max(1);
            let (a, b) = (stops[hi - 1], stops[hi]);
            let f = ((t - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
            let mix = |k: usize| (a.1[k] + (b.1[k] - a.1[k]) * f).round() as u8;
            contrast(brighten(Color32::from_rgb(mix(0), mix(1), mix(2)), brightness), contrast_k)
        })
        .collect()
}

/// How much of a generator a dialog's Preview plays, in seconds, looped.
const PREVIEW_SECONDS: f64 = 2.0;
/// Previews of noise are the same every time, so a changed setting is what you hear change.
const PREVIEW_SEED: u64 = 0x5EED;

/// A seed for noise that differs between runs.
fn clock_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

/// `seed=N` pulled out of a `GENERATE` argument string: the seed (the clock's when absent) and
/// the rest.
fn split_seed(args: &str) -> (Result<u64, String>, String) {
    let mut seed = Ok(clock_seed());
    let rest: Vec<&str> = args
        .split_whitespace()
        .filter(|w| match w.strip_prefix("seed=") {
            Some(n) => {
                seed = n.parse::<u64>().map_err(|_| format!("seed={n}: a whole number"));
                false
            }
            None => true,
        })
        .collect();
    (seed, rest.join(" "))
}

// I3 card stubs: remove these temporary dead-code allowances when wiring Help.
#[allow(dead_code, clippy::unimplemented)]
fn shortcut_help_rows(_mac: bool) -> Vec<(&'static str, &'static str, String)> {
    unimplemented!()
}

#[allow(dead_code, clippy::unimplemented)]
fn shortcut_help_content(_ui: &mut egui::Ui, _mac: bool) {
    unimplemented!()
}

/// "AIFF-C, little-endian (sowt)" for an AIFF save, else the format's name.
fn format_label(format: SaveFormat, aiff: AiffKind) -> &'static str {
    if format == SaveFormat::Aiff { aiff.describe() } else { format.name() }
}

/// The estimated size line of the export dialog.
fn format_size_label(e: &SizeEstimate) -> String {
    let human = wavcraft_core::space::format_bytes(e.bytes);
    if e.exact {
        format!("File size: {human} ({} bytes)", e.bytes)
    } else {
        format!("File size: about {human} (estimate: FLAC depends on the audio)")
    }
}

/// Reads the `quality=<-1..10>` option (Ogg Vorbis VBR) out of `SAVE`'s arguments, if given.
/// `parse_save_args` skips that token.
fn take_quality(rest: &str) -> Result<Option<f32>, String> {
    let mut quality = None;
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("quality=") {
            let q: f32 = v.parse().ok().filter(|q: &f32| (-1.0..=10.0).contains(q)).ok_or_else(|| format!("quality=<-1..10>, not {v}"))?;
            quality = Some(q);
        }
    }
    Ok(quality)
}

/// `SAVE`'s `meta=0|1` (A262): "Include markers and other metadata" for this document, if given.
fn take_meta(rest: &str) -> Result<Option<bool>, String> {
    let mut meta = None;
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("meta=") {
            meta = Some(match v {
                "0" => false,
                "1" => true,
                _ => return Err(format!("meta=<0|1>, not {v}")),
            });
        }
    }
    Ok(meta)
}

/// The extra chunks a save writes when "Include markers and other metadata" is on. This is
/// the one place the marker and metadata features add theirs. Test only: with
/// `WAVCRAFT_TEST_SAVE_CHUNK` set, a `TEST` chunk, so the switch can be checked on a file.
fn metadata_chunks() -> Vec<ExtraChunk> {
    let mut chunks = Vec::new();
    if std::env::var_os("WAVCRAFT_TEST_SAVE_CHUNK").is_some() {
        chunks.push(ExtraChunk { id: *b"TEST", data: b"wavcraft test chunk".to_vec() });
    }
    chunks
}

/// Test only: `WAVCRAFT_TEST_SAVE_DELAY_MS` slows a save down per 64 K-frame block.
fn test_block_delay() -> std::time::Duration {
    let ms = std::env::var("WAVCRAFT_TEST_SAVE_DELAY_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    std::time::Duration::from_millis(ms)
}

/// `SAVE`'s arguments: a path (may hold spaces) then `bits=<16|24|32>` and `float=<0|1>` in any
/// order. Returns the path and the sample type they name, if any.
fn parse_save_args(rest: &str) -> Result<(&str, Option<SampleType>, Option<AiffKind>), String> {
    let (mut path, mut bits, mut float, mut aiff) = (rest.trim(), None, None, None);
    while let Some((head, last)) = path.rsplit_once(char::is_whitespace).or(if path.contains('=') { Some(("", path)) } else { None }) {
        if let Some(v) = last.strip_prefix("bits=") {
            bits = Some(v.parse::<u32>().map_err(|_| format!("bits=<16|24|32>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("aiff=") {
            aiff = Some(AiffKind::parse(v).ok_or_else(|| format!("aiff=<aiff|aifc|sowt>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("float=") {
            float = Some(match v {
                "0" => false,
                "1" => true,
                _ => return Err(format!("float=<0|1>, not {v}")),
            });
        } else if last.starts_with("quality=") {
            // Read by `take_quality`.
        } else if last.starts_with("meta=") {
            // Read by `take_meta`.
        } else {
            break;
        }
        path = head.trim();
    }
    let ty = match (bits, float) {
        (None, None) => None,
        (Some(16), None | Some(false)) => Some(SampleType::Int16),
        (Some(24), None | Some(false)) => Some(SampleType::Int24),
        (Some(32), None | Some(false)) => Some(SampleType::Int32),
        (None | Some(32), Some(true)) => Some(SampleType::Float32),
        (None, Some(false)) => None,
        (b, f) => return Err(format!("no sample type for bits={b:?} float={f:?}: 16, 24, 32 bit integer or float=1 (32 bit)")),
    };
    Ok((path, ty, aiff))
}

#[cfg(test)]
mod tests {
    #[test]
    fn history_arrows_stop_at_the_ends() {
        use super::history_arrow_target as t;
        assert_eq!(t(0, 3, false), None);
        assert_eq!(t(0, 3, true), Some(1));
        assert_eq!(t(2, 3, true), None);
        assert_eq!(t(2, 3, false), Some(1));
        assert_eq!(t(0, 1, true), None);
    }

    use super::*;

    /// Runs three frames on a fresh context: frame 1 draws a text field and (when `focus`) asks for focus,
    /// frame 2 lets that focus settle, frame 3 delivers one key press and returns what `pressed_commands` reports.
    fn commands_for_key(focus: bool, key: Key, modifiers: egui::Modifiers) -> Vec<Command> {
        let ctx = egui::Context::default();
        let mut text = String::from("10");
        let keymap = Keymap::new();
        let mut frame = |events: Vec<egui::Event>, ask_focus: bool| -> Vec<Command> {
            let raw = egui::RawInput { events, ..Default::default() };
            let mut found = Vec::new();
            let mut output = ctx.run_ui(raw, |ui| {
                let r = ui.text_edit_singleline(&mut text);
                if ask_focus {
                    r.request_focus();
                }
                found = pressed_commands(ui.ctx(), &keymap);
            });
            output.textures_delta.clear();
            found
        };
        let _ = frame(Vec::new(), focus);
        let _ = frame(Vec::new(), false);
        let press = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        frame(vec![press], false)
    }

    #[test]
    fn brightness_scales_colours_and_stays_in_range() {
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 1.0), Color32::from_rgb(100, 200, 50));
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 0.5), Color32::from_rgb(50, 100, 25));
        assert_eq!(brighten(Color32::from_rgb(100, 200, 50), 1.5), Color32::from_rgb(150, 255, 75));
    }

    #[test]
    fn a_drop_opens_the_first_file_that_has_a_path() {
        #[derive(Debug)]
        struct Dropped(PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                Err("not a file".to_string())
            }
        }
        let file = |path: &str| -> Arc<dyn egui::DroppedFile + Send + Sync> { Arc::new(Dropped(PathBuf::from(path))) };
        assert_eq!(dropped_path(&[]), None);
        assert_eq!(dropped_path(&[file("")]), None);
        assert_eq!(dropped_path(&[file(""), file("/a/b.wav"), file("/c.wav")]), Some(PathBuf::from("/a/b.wav")));
    }

    #[test]
    fn tick_labels_are_exact_at_any_sample_rate() {
        let tick_label = |s: f64| TimeFormat::Decimal.format_tick(s, 44_100);
        let labels: Vec<String> = (0..8).map(|k| tick_label(f64::from(k) * 0.001)).collect();
        assert_eq!(labels, ["0:00.000", "0:00.001", "0:00.002", "0:00.003", "0:00.004", "0:00.005", "0:00.006", "0:00.007"]);
        assert_eq!(tick_label(3723.5), "1:02:03.500");
        // The old route, through a frame at 44.1 kHz, dropped a millisecond.
        assert_eq!(wavcraft_core::format_time((0.003f64 * 44_100.0).round() as usize, 44_100), "0:00.002");
    }

    #[test]
    fn the_palette_runs_dark_to_light() {
        let p = palette(settings::SpecPalette::Ember, 1.0, 1.0);
        assert_eq!(p.len(), 256);
        assert_eq!(p[0], Color32::from_rgb(0, 0, 0));
        assert_eq!(p[255], Color32::from_rgb(255, 250, 215));
        let brightness = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(p.windows(2).all(|w| brightness(w[1]) + 3 >= brightness(w[0])), "brightness never drops");
    }

    #[test]
    fn silence_longer_than_a_day_is_rejected() {
        assert_eq!(silence_frames(1.0, 44_100), Ok(44_100));
        assert_eq!(silence_frames(86_400.0, 48_000), Ok(4_147_200_000));
        assert!(silence_frames(86_400.5, 48_000).is_err());
        assert!(silence_frames(1e20, 44_100).is_err());
        assert!(silence_frames(f64::INFINITY, 44_100).is_err());
        assert!(silence_frames(0.0, 44_100).is_err());
    }

    #[test]
    fn every_palette_is_256_colours_and_contrast_widens() {
        for which in settings::SpecPalette::ALL {
            assert_eq!(palette(*which, 1.0, 1.0).len(), 256, "{}", which.name());
        }
        assert_eq!(contrast(Color32::from_rgb(100, 128, 200), 1.0), Color32::from_rgb(100, 128, 200));
        assert_eq!(contrast(Color32::from_rgb(100, 128, 200), 2.0), Color32::from_rgb(72, 128, 255));
        let mono = palette(settings::SpecPalette::Mono, 1.0, 1.0);
        assert_eq!((mono[0], mono[255]), (Color32::BLACK, Color32::WHITE));
        assert_ne!(palette(settings::SpecPalette::Glacier, 1.0, 1.0), palette(settings::SpecPalette::Ember, 1.0, 1.0));
    }

    #[test]
    fn lanes_stack_without_gaps() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 300.0));
        assert_eq!(lane(area, 0, 2), Rect::from_min_max(pos2(0.0, 100.0), pos2(800.0, 200.0)));
        assert_eq!(lane(area, 1, 2), Rect::from_min_max(pos2(0.0, 200.0), pos2(800.0, 300.0)));
        assert_eq!(lane(area, 0, 0), area);
    }

    #[test]
    fn backspace_in_a_focused_text_field_runs_no_action() {
        let got = commands_for_key(true, Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(got, Vec::<Command>::new());
    }

    #[test]
    fn cmd_z_in_a_focused_text_field_runs_no_action() {
        let got = commands_for_key(true, Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(got, Vec::<Command>::new());
    }

    #[test]
    fn delete_key_in_a_focused_text_field_runs_no_action() {
        let got = commands_for_key(true, Key::Delete, egui::Modifiers::NONE);
        assert_eq!(got, Vec::<Command>::new());
    }

    #[test]
    fn backspace_without_a_focused_text_field_runs_delete() {
        let got = commands_for_key(false, Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(got, vec![Command::Action(ActionId::Delete)]);
    }

    #[test]
    fn cmd_z_without_a_focused_text_field_runs_undo() {
        let got = commands_for_key(false, Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(got, vec![Command::Action(ActionId::Undo)]);
    }

    #[test]
    fn cmd_shift_z_without_a_focused_text_field_runs_redo_only() {
        let got = commands_for_key(false, Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        assert_eq!(got, vec![Command::Action(ActionId::Redo)]);
    }

    #[test]
    fn split_seed_pulls_the_seed_out() {
        let (seed, rest) = split_seed("freq=440 seed=7 dur=2");
        assert_eq!((seed, rest.as_str()), (Ok(7), "freq=440 dur=2"));
        assert!(split_seed("seed=x").0.is_err());
        assert!(split_seed("dur=2").0.is_ok());
    }

    #[test]
    fn save_arguments_name_the_path_and_the_sample_type() {
        assert_eq!(parse_save_args("/a b/c.wav"), Ok(("/a b/c.wav", None, None)));
        assert_eq!(parse_save_args("/a b/c.wav bits=24 float=0"), Ok(("/a b/c.wav", Some(SampleType::Int24), None)));
        assert_eq!(parse_save_args("x.aiff float=1"), Ok(("x.aiff", Some(SampleType::Float32), None)));
        assert_eq!(parse_save_args("x.aiff bits=32 float=1"), Ok(("x.aiff", Some(SampleType::Float32), None)));
        assert_eq!(parse_save_args("x.flac bits=16"), Ok(("x.flac", Some(SampleType::Int16), None)));
        assert_eq!(parse_save_args(""), Ok(("", None, None)));
        assert_eq!(parse_save_args("/a b/c.aiff aiff=sowt"), Ok(("/a b/c.aiff", None, Some(AiffKind::AifcSowt))));
        assert_eq!(parse_save_args("x.aiff bits=24 aiff=aifc"), Ok(("x.aiff", Some(SampleType::Int24), Some(AiffKind::Aifc))));
        assert!(parse_save_args("x.aiff aiff=big").is_err());
        assert_eq!(parse_save_args("a b.ogg quality=7.5 bits=16"), Ok(("a b.ogg", Some(SampleType::Int16), None)));
        assert_eq!(take_quality("a b.ogg quality=7.5 bits=16"), Ok(Some(7.5)));
        assert_eq!(take_quality("x.ogg"), Ok(None));
        assert!(take_quality("x.ogg quality=11").is_err() && take_quality("x.ogg quality=loud").is_err());
        assert!(parse_save_args("x.wav bits=20").is_err());
        assert!(parse_save_args("x.wav bits=24 float=1").is_err());
        assert!(parse_save_args("x.wav float=2").is_err());
    }
}

#[cfg(test)]
#[path = "i3_shortcut_help_tests.rs"]
mod i3_shortcut_help_tests;
