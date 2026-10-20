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
//! - [`view`]: zoom and scroll arithmetic, selections and time formatting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod actions;
pub mod doc;
pub mod document;
mod flac_enc;
pub mod io;
pub mod random;
pub mod save;
pub mod settings;
pub mod space;
pub mod spectrogram;
pub mod store;
pub mod view;

pub use doc::edit::{ChannelMask, Clip, Crossfades, Edit, EditError};
pub use doc::history::History;
pub use doc::{Channel, DocState, Piece, SourceFormat};

pub use document::{Document, OpenError};
pub use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};
pub use spectrogram::SpectrogramParams;
pub use view::{Selection, Viewport, format_time};
