# Code map

*Written 2026-10-15 (task 02) from the tree at `3cdf38f`, for a session that has seen nothing.
Update it when the layout changes. The test kit and bench (task 02) are listed at the end.*

## Crates and modules

| Crate | Module | What it holds |
|---|---|---|
| `crates/core` (`wavcraft-core`, library, no UI) | `lib.rs` | Re-exports `Document`, `OpenError`, `SpectrogramParams`, `Viewport`, `Selection`, `format_time`, and audio-io's `AudioBuffer`, `AudioInfo`, `FileFormat`, `SampleFormat` |
| | `document.rs` | `Document`: open/decode a file, its samples and per-channel waveform peaks, `waveform_columns()` for drawing |
| | `spectrogram.rs` | The spectral display maths: `SpectrogramParams`, `Columns` (file-fixed column grid), `Overview` (coarse per-channel cache), `render()` / `render_columns()`. Private: `Analysis` (FFT plan, Hann window, dB→level table), `Analyser` (FFT scratch, per-bin max) |
| | `view.rs` | `Viewport` (zoom/scroll in frames), `Selection` (anchor/head), `format_time` |
| | `examples/spectrogram_cost.rs` | Measures overview build and far-view render times on a synthetic 3-hour mono file |
| `apps/wavcraft` (`wavcraft`, binary, egui/eframe 0.36, wgpu DX12 on Windows) | `main.rs` | Window options, DX12-only wgpu on Windows, runs `WaveApp` |
| | `app.rs` | `WaveApp`: menu, status bar, the editor painter (ruler, waveform lanes, spectral lanes, selection, cursor, playhead), input (wheel zoom, drag select, Space, Home, `\`, Ctrl+O, drop), opening on a worker thread |
| | `player.rs` | `Player`: cpal output stream, play/stop, playhead position |
| | `spectrum.rs` | `Renderer`: the spectrogram worker thread and its overview cache |

Workspace: `Cargo.toml` at the root; `unsafe_code = "forbid"` for every crate that inherits the
workspace lints. Tests: 21 in the core, 9 in the app (before task 02).

## Where the samples live after open

- Type: `soundcraft_audio_io::AudioBuffer { sample_rate: u32, channels: Vec<Vec<f32>> }`: planar
  `f32`, **one contiguous `Vec<f32>` per channel for the whole file**. No chunks or blocks.
- Owner: `Document.audio: Arc<AudioBuffer>`; the app keeps `WaveApp.doc: Option<Document>`.
- Opening (`Document::open`): `std::fs::read` the whole file into a `Vec<u8>`, then
  `soundcraft_audio_io::decode(bytes, ext)`, then one `Peaks` per channel on scoped threads. The
  byte buffer is dropped when `open` returns, so for a moment the process holds the file's bytes
  and the decoded `f32` together. The app runs `open` on a `std::thread::spawn` worker and polls an
  `mpsc` channel each frame (`finish_opening`).
- How the parts reach the samples:
  - **UI (waveform):** `doc.waveform_columns(ch, start, end, cols)` borrows
    `self.audio.channels[ch]` and `self.peaks[ch]` directly, every frame.
  - **Spectrogram worker:** `Renderer::ask` sends `(Job, Arc<AudioBuffer>)` (an `Arc` clone, no
    copy) down an `mpsc` channel; the worker passes `&audio.channels[ch]` (a whole-channel
    `&[f32]`) to `Overview::build` and `render_columns`.
  - **Playback:** `Player::set_audio(Some(doc.audio.clone()))` puts the `Arc` into a
    `Mutex<Option<Arc<AudioBuffer>>>`. The audio callback `try_lock`s it (silence if the UI holds
    it) and only borrows; it never owns or drops a reference.
- There is **no single read function**: each consumer indexes the channel `Vec`s itself.

## Waveform peaks

`soundcraft_audio_io::peaks::Peaks` **is used**. `Document::build` makes one per channel, eagerly,
at open (`Peaks::build(&[f32])`, scoped thread per channel). Its layout: level 0 is `(min, max)` of
each 32 samples (`BLOCK`), each further level merges 4 (`FANOUT`), up to one block. About 0.33
bytes per sample (a 3-hour stereo file: about 345 MB). `waveform_columns` asks
`Peaks::columns(start, end, cols)` when a column spans 64 frames or more, and folds the samples
itself when zoomed closer. Nothing is cached per view; columns are recomputed every frame
(O(log n) per column). `Peaks::build` needs the whole channel as one slice.

## Spectrogram and its overview

- Maths in `core::spectrogram`; the worker and its cache in `apps/wavcraft/src/spectrum.rs`.
- **Worker:** one long-lived `std::thread` started by `Renderer::new`. It reads `(Job, Arc<AudioBuffer>)`
  from its inbox, drops all but the newest, and per channel calls
  `spectrogram::render_columns(samples, rate, job.columns, job.rows, &params, overview)`, colours
  the levels with the palette into an `egui::ColorImage`, sends a `Picture` back and requests a
  repaint. Channels are done one after another; inside each call `rayon` spreads columns (and an
  overview's blocks) across all cores.
- **Job:** `{ generation, columns: Columns, rows, params }`. `generation` is bumped by the app on
  every open. The app asks every frame; `Renderer::ask` sends only when the job differs from the
  last one. App sizes: columns = half the editor width (1..2048), rows = lane height (max 384).
  `Columns::covering` puts columns on a grid fixed to the file (whole FFT hops when zoomed out).
- **Overview cache:** `Overviews { generation, params, channels: Vec<Overview> }`, one per channel.
  **Key:** `(generation, fft_size, floor_db bits, ceil_db bits)`; the frequency axis, row count and
  column count are not part of it. **Rebuild trigger:** a job whose generation (a new file) or FFT
  size or dB scale differs; the old overviews are dropped first, then all channels are rebuilt,
  before that job's picture. An overview keeps, per block of 16 FFT hops, the loudest level per
  bin (one byte per bin): 10.8 MB per channel-hour at 48 kHz. `render_columns` reads whole blocks
  from it and runs FFTs only for a column's two ragged ends. `Overview::serves` also checks the
  channel length, so an overview of a different-length buffer is ignored (falls back to full FFTs).
- **The app's picture cache:** `SpectrumCache { job, textures }`, the last picture, drawn at its own
  frame range until the next one arrives. Cleared on open and on a log/linear axis change.
- Everything takes the whole channel as one `&[f32]` (`Analyser::add` slices `samples[at..at+n]`).

## Playback

- `cpal` 0.16 **directly**, in `apps/wavcraft/src/player.rs`. SoundCraft's `playback` crate is not
  used, and can't be on its own: its `Player::new` takes a SoundCraft `Session` (engine/mix model).
- Default host, default output device, its default config (f32, i16, u16 or i32 samples). The
  stream is opened at start-up and reopened on Play after an error or when there was none.
- Shared state: `Mutex<Option<Arc<AudioBuffer>>>`, `pos` and `end` as `f64` bits in `AtomicU64`
  (file frames), `playing` and `failed` as `AtomicBool`.
- The callback converts the file's rate to the device's by linear interpolation, averages all
  channels onto a mono device, writes the new `pos` back and clears `playing` at `end`.
- **Playhead to the UI:** `Player::position()` reads `pos` atomically; while playing the editor draws
  the playhead there, keeps it on screen (`Viewport::reveal`) and calls `request_repaint()` each frame.

## soundcraft-audio-io (pinned `3527bb4`, tag `wavecraft-pin-2026-10-12`)

**Called today:** `decode(bytes, ext_hint) -> (AudioInfo, AudioBuffer)` (document.rs),
`peaks::Peaks::{build, columns}`, and the types `AudioBuffer`, `AudioInfo`, `FileFormat`,
`SampleFormat`. Tests also call `encode`, `EncodeOptions`, `BitDepth`.

**Present, unused:** `probe`, `detect_format`, `encode`, `encode_format_for`, `ENCODE_EXTENSIONS`,
`encode_wav_rf64`, `encode_with_channel_mask`, `wav_channel_mask`, `wav_float_header`,
`AudioBuffer::{interleaved, from_interleaved, peak}`, `Peaks::{minmax, len_samples, num_levels}`.

**Writing** (all "bytes in, bytes out": no file access, no streaming, the whole output in one `Vec<u8>`):

| Call | Writes |
|---|---|
| `encode(&AudioBuffer, &EncodeOptions { format, bit_depth, dither, bwf })` | `Wav`: PCM 16/24/32 or float 32, BWF `bext` if `bwf` is set, RF64 automatically above 4 GiB. `Aiff`: integer AIFF, or AIFF-C `fl32` for float; errors above 4 GiB. `Flac`: built-in encoder, 16/24/32-bit integer only (float is `Unsupported`), up to 8 channels. Other formats: `Unsupported` |
| `encode_wav_rf64` | WAV forced to RF64 |
| `encode_with_channel_mask(.., mask)` | WAV with a `WAVE_FORMAT_EXTENSIBLE` speaker mask |
| `wav_float_header(channels, rate, frames)` | Just the header of a float WAV, same length at any size, for a writer that streams samples after it |
| `EncodeOptions::dither` | TPDF dither when quantising; deterministic per channel. Integer depths clamp to [-1, 1]; quantising is `round(x * 2^(bits-1))`, the exact inverse of decode, so integer files round-trip bit for bit without dither |

`encode` needs the whole document as one `AudioBuffer` plus the whole output in memory.

**Markers / cue chunks: none.** No `cue `, `LIST`/`adtl`/`labl`/`note`, `smpl` or AIFF `MARK`
support in reading or writing. The WAV reader skips chunks it doesn't know (only `fmt `, `data`,
`bext`, `ds64` are read), so a file with markers opens fine and loses them.

**Limits:** decode refuses more than 2^30 samples in total (`MAX_DECODE_SAMPLES`, 4 GiB of `f32`):
3 h 06 min of 48 kHz stereo. Decoding WAV/AIFF PCM is parallel (up to 16 threads).

## soundcraft-dsp and soundcraft-time at the same pin

Both are in the same fork at the same commit, so adding them as git dependencies with the same
`rev` resolves (their own dependencies: `dsp` needs `serde`, `log`, `rustfft`; `time` needs
`serde`, `thiserror`). Not added yet.

- `dsp::meter`: `LoudnessMeter::new(rate, channels)`, `process(&[Vec<f32>], frames)` (planar, in
  chunks; parallel per channel for long input), `momentary_lufs`, `short_term_lufs`,
  `max_momentary_lufs`, `integrated_lufs` (gated), `true_peak_db` (4x oversampled),
  `set_channel_weight`, `reset`. Also `PeakMeter` (hold, release, clip) and `RmsMeter` (window ms).
- `dsp::spectrum`: `magnitude_db(samples, fft_size)` (one Hann-windowed FFT, plans a new FFT per
  call, so slower than our own reuse of a plan) and `bin_hz`. Our `core::spectrogram` uses
  `rustfft` directly and should keep doing so.
- `dsp::offline`: `normalize`, `gain`, `invert`, `reverse`, `remove_dc`, `fade` (`FadeShape`:
  Linear, EqualPower, SCurve, Exponential, Logarithmic), `resample`, `time_stretch`: all over a
  whole `&mut [Vec<f32>]`.
- `dsp` plugins (trait `Plugin`, `create(id)`): `compressor` (up to 10 ms lookahead),
  `expander_gate`, `de_esser`, `maximizer` (brickwall, 1.5 ms lookahead, sample peaks, not true
  peak), `gain`, `trim`, `invert`, `time_shift`, EQ, reverb and others.
- `time`: `SampleRate`, `Range`, `TimeFormat` (BarsBeats, MinSecs, Timecode, FeetFrames, Samples),
  `format_position`/`format_length`/`parse_position` (each takes a `TempoMap` and a `FrameRate`),
  `Timecode`, `FrameRate`.

## Test kit and bench (task 02)

- `crates/testkit` (`wavcraft-testkit`): `lib.rs` writes the synthetic test files
  (deterministic; its own streaming WAV writer, audio-io's `encode` for the AIFF and FLAC copies).
  Binary `wavcraft-testkit OUT_DIR [--long] [--seed N] [--speech WAV --noise WAV]`.
  `tests/kit.rs` holds acceptance tests 1-3 (byte-identical runs, every file opens through
  `Document::open`, the ramp oracle).
- Binary `wavcraft-bench FILE`: open time and peak memory, peaks, overview and picture times
  (the worker's own calls, channel by channel), a full ordered read. Results: `docs/baseline.md`.
  It reads process memory through `windows-sys` on Windows (the only `unsafe` in the tree, in
  `src/bin/wavcraft-bench/memory.rs`) and `/proc/self/status` on Linux.
