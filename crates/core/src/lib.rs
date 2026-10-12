//! WaveCraft core: one audio file held in memory, plus what a single-file waveform editor needs
//! to draw it. No UI code lives here, so everything is testable without a window.
//!
//! - [`Document`]: the open file (decoded through SoundCraft's `audio-io`) and its waveform peaks.
//! - [`spectrogram`]: the spectral frequency display for any time range, at screen resolution.
//! - [`view`]: zoom and scroll arithmetic, selections and time formatting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod document;
pub mod spectrogram;
pub mod view;

pub use document::{Document, OpenError};
pub use soundcraft_audio_io::{AudioBuffer, AudioInfo, FileFormat, SampleFormat};
pub use spectrogram::SpectrogramParams;
pub use view::{Selection, Viewport, format_time};
