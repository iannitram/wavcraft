//! WavCraft core: the on-disk sample store, the document model over it, streamed open, and what a
//! single-file waveform editor needs to draw a file. No UI code lives here, so everything is
//! testable without a window.
//!
//! - [`store`]: the session folder of append-only pack files, blocks, and the block cache.
//! - [`doc`]: the piece table ([`DocState`]) and its one read path; [`doc::edit`], the edits
//!   (pure functions from one state to the next), and [`doc::history`], undo with a disk cap.
//! - [`autosave`]: the journal of the piece table and history, and crash recovery from it.
//! - [`io`]: streamed open into the store.
//! - [`probe`]: a file's format, length and title from its header alone (no store).
//! - [`recent`]: the Open Recent list and the config folder's small files.
//! - [`space`]: the store's size for a file and the free space on its disk.
//! - [`record`]: recording a take: the ring buffer, the crash-safe WAV writer, the fake input.
//! - [`take_recovery`]: finding and repairing takes a crash left unfinished.
//! - [`settings`]: the settings file and where the session folder is.
//! - [`Document`]: the open file as the app still sees it (whole-channel buffers until task 05)
//!   and its waveform peaks.
//! - [`spectrogram`]: the spectral frequency display for any time range, at screen resolution.
//! - [`snap`]: snapping a frame to the ruler, selection edges, the cursor and zero crossings.
//! - [`transport`]: play modes, loop and skip arithmetic, and the view following the playhead.
//! - [`smpte`]: SMPTE timecode at 24 to 59.94 fps (drop-frame too) and snapping to frames.
//! - [`view`]: zoom and scroll arithmetic, selections and time formatting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod actions;
pub mod autosave;
pub mod clipboard;
pub mod convert;
pub mod dither;
pub mod doc;
pub mod document;
pub mod effects;
mod flac_enc;
pub mod generate;
pub mod io;
pub mod keyboard;
pub mod levels;
pub mod loudness;
pub mod markers;
pub mod match_loudness;
pub mod meta;
pub mod meta_view;
pub mod noise;
pub mod panels;
pub mod presets;
pub mod probe;
pub mod rack;
pub mod random;
pub mod recent;
pub mod record;
pub mod save;
pub mod settings;
pub mod shortcut_sets;
pub mod shortcuts;
pub mod smpte;
pub mod snap;
pub mod space;
pub mod spectrogram;
pub mod stats;
pub mod stft;
pub mod store;
pub mod take_recovery;
pub mod tools;
pub mod transport;
pub mod view;
pub mod workspaces;
pub mod xmp;

pub use doc::edit::{ChannelMask, Clip, Crossfades, Edit, EditError};
pub use doc::history::History;
pub use doc::{Channel, DocState, Piece, SourceFormat};
pub use markers::{Marker, MarkerPick, Markers, PALETTE};

pub use document::{Document, OpenError};
pub use smpte::SmpteRate;
pub use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};
pub use spectrogram::SpectrogramParams;
pub use tools::Tool;
pub use view::{AmpScale, AmpZoom, Edge, GRAB_PX, NavPart, Selection, TimeFormat, Viewport, format_time};
