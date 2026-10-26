//! WavCraft core: the on-disk sample store, the document model over it, streamed open, and what a
//! single-file waveform editor needs to draw a file. No UI code lives here, so everything is
//! testable without a window.
//!
//! - [`store`]: the session folder of append-only pack files, blocks, and the block cache.
//! - [`doc`]: the piece table ([`DocState`]) and its one read path; [`doc::edit`], the edits
//!   (pure functions from one state to the next), and [`doc::history`], undo with a disk cap.
//! - [`io`]: streamed open into the store.
//! - [`space`]: the store's size for a file and the free space on its disk.
//! - [`settings`]: the settings file and where the session folder is.
//! - [`Document`]: the open file as the app still sees it (whole-channel buffers until task 05)
//!   and its waveform peaks.
//! - [`spectrogram`]: the spectral frequency display for any time range, at screen resolution.
//! - [`smpte`]: SMPTE timecode at 24 to 59.94 fps (drop-frame too) and snapping to frames.
//! - [`view`]: zoom and scroll arithmetic, selections and time formatting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod actions;
pub mod atomic;
pub mod autosave;
pub mod cd;
pub mod diagnostics;
pub mod doc;
pub mod document;
pub mod dsp;
pub mod effects;
pub mod export_chunks;
pub mod fade_handles;
pub mod generate;
pub mod io;
pub mod jobs;
pub mod meta;
pub mod meters;
pub mod mix;
pub mod name_template;
pub mod noise;
pub mod panels;
pub mod mtplay;
pub mod mtpunch;
pub mod mtrecord;
pub mod multitrack;
pub mod mixer_sections;
pub mod levels;
pub mod loudness;
pub mod presets;
pub mod rack;
pub mod random;
pub mod recent_sessions;
pub mod ruler;
pub mod save;
pub mod sc_source;
pub mod session_file;
pub mod settings;
pub mod shortcuts;
pub mod smpte;
pub mod space;
pub mod spectral_dirty;
pub mod spectrogram;
pub mod stats;
pub mod stft;
pub mod store;
pub mod strip_eq;
pub mod take_readout;
pub mod take_recover;
pub mod take_repair;
pub mod tempo;
pub mod transcript_cut;
pub mod video_snap;
pub mod timepitch;
pub mod view;
pub mod vst3;

pub use doc::edit::{ChannelMask, Clip, Crossfades, Edit, EditError};
pub use doc::history::History;
pub use doc::{Channel, DocState, Piece, SourceFormat};

pub use document::{Document, OpenError};
pub use smpte::SmpteRate;
pub use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};
pub use spectrogram::SpectrogramParams;
pub use view::{Edge, GRAB_PX, Selection, TimeFormat, Viewport, format_time};
