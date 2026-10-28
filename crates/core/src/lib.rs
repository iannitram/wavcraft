//! WavCraft core: the on-disk sample store, the document model over it, streamed open, and what a
//! single-file waveform editor needs to draw a file. No UI code lives here, so everything is
//! testable without a window.
//!
//! - [`store`]: the session folder of append-only pack files, blocks, and the block cache.
//! - [`doc`]: the piece table ([`DocState`]) and its one read path; [`doc::edit`], the edits
//!   (pure functions from one state to the next), and [`doc::history`], undo with a disk cap.
//! - [`autosave`]: the journal of the piece table and history, and crash recovery from it.
//! - [`batch`]: the Batch Process runner (a list of files through a chain of operations to WAV).
//! - [`cd`]: the CD layout (track order, pauses, ISRC), and its single-WAV-plus-CUE export.
//! - [`i18n`]: the interface text catalogue (English source, pseudo-locale, fallback).
//! - [`io`]: streamed open into the store.
//! - [`peakcache`]: saved waveform overviews, so a long file reopens fast.
//! - [`probe`]: a file's format, length and title from its header alone (no store).
//! - [`recent`]: the Open Recent list and the config folder's small files.
//! - [`space`]: the store's size for a file and the free space on its disk.
//! - [`resample`]: windowed-sinc sample-rate conversion for playback.
//! - [`hwprofile`]: audio hardware settings kept per device setup.
//! - [`chanmap`]: output/input channel maps and the output buffer size choice.
//! - [`record`]: recording a take: the ring buffer, the crash-safe WAV writer, the fake input.
//! - [`take_recovery`]: finding and repairing takes a crash left unfinished.
//! - [`take_recovery`]: finding and repairing takes a crash left unfinished.
//! - [`bundle`]: export/import of the settings files as one file, and the preference reset.
//! - [`settings`]: the settings file and where the session folder is.
//! - [`Document`]: the open file as the app sees it: the document in the store, read in
//!   chunks (no whole-file copy since task 05), and its waveform peaks.
//! - [`spectrogram`]: the spectral frequency display for any time range, at screen resolution.
//! - [`smpte`]: SMPTE timecode at 24 to 59.94 fps (drop-frame too) and snapping to frames.
//! - [`analysis`]: read-only analysis of a document's audio (the Frequency Analysis spectrum).
//! - [`transport`]: play modes, loop and skip arithmetic, and the view following the playhead.
//! - [`snap`]: snapping a frame to the ruler, selection edges, the cursor and zero crossings.
//! - [`freq_axis`]: the spectral display's frequency band, scale, zoom, scroll and ruler ticks.
//! - [`statusbar`]: the status bar's fields and which are shown.
//! - [`view`]: zoom and scroll arithmetic, selections and time formatting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod actions;
pub mod analysis;
pub mod appearance;
pub mod atomic;
pub mod autosave;
pub mod batch;
pub mod batch_cli;
pub mod bundle;
pub mod cd;
pub mod chanmap;
pub mod channels;
pub mod clipboard;
pub mod convert;
pub mod diagnostics;
pub mod dither;
pub mod doc;
pub mod document;
pub mod dsp;
pub mod effects;
pub mod export;
pub mod export_chunks;
pub mod export_presets;
pub mod fade_handles;
mod flac_enc;
pub mod freq_axis;
pub mod generate;
pub mod hwprofile;
pub mod i18n;
pub mod input_meter;
pub mod io;
pub mod jobs;
pub mod keyboard;
pub mod levels;
pub mod loudness;
pub mod markers;
pub mod match_loudness;
pub mod meta;
pub mod meta_view;
pub mod meters;
pub mod metronome;
pub mod mix;
pub mod mixdown;
pub mod mixer_sections;
pub mod monitors;
pub mod mtplay;
pub mod mtpunch;
pub mod mtrecord;
pub mod multitrack;
pub mod name_template;
pub mod noise;
pub mod panels;
pub mod peakcache;
pub mod playlist;
pub mod presets;
pub mod probe;
pub mod rack;
pub mod random;
pub mod readout;
pub mod recent;
pub mod recent_sessions;
pub mod record;
pub mod resample;
pub mod ruler;
pub mod save;
pub mod sc_source;
pub mod session_file;
pub mod sessions;
pub mod settings;
pub mod shortcut_sets;
pub mod shortcuts;
pub mod smpte;
pub mod snap;
pub mod space;
pub mod spectral_dirty;
pub mod spectrogram;
pub mod stats;
pub mod statusbar;
pub mod stft;
pub mod store;
pub mod strip_eq;
pub mod take_readout;
pub mod take_recover;
pub mod take_recovery;
pub mod take_repair;
pub mod tempo;
pub mod timepitch;
pub mod transcript_cut;
pub mod transport;
pub mod video_snap;
pub mod view;
pub mod vst3;
pub mod workspaces;
pub mod xmp;

pub use channels::{ChannelLayout, channel_color};
pub use doc::edit::{ChannelMask, Clip, Crossfades, Edit, EditError};
pub use doc::history::History;
pub use doc::{Channel, DocState, Piece, SourceFormat};
pub use markers::{Marker, MarkerPick, Markers, PALETTE};
pub use playlist::Playlist;

pub use document::{Document, OpenError, PeakReport};
pub use smpte::SmpteRate;
pub use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};
pub use spectrogram::SpectrogramParams;
pub use view::{
    AmpScale, AmpZoom, DocView, Edge, GRAB_PX, NavPart, Selection, SyncOptions, TimeFormat, Viewport, ZoomPresets, carry_over, convert_frame,
    format_time,
};
