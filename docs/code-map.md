# Code map

*Written 2026-10-15 (task 02) from the tree at `3cdf38f`, for a session that has seen nothing.
Update it when the layout changes. The test kit and bench (task 02) are listed at the end.
Task 04 (2026-10-15) added `doc/edit.rs` and `doc/history.rs`, the test kit's `random_edit` and
`content_hash`, and the `wavcraft-edit-bench` binary.*

## Crates and modules

| Crate | Module | What it holds |
|---|---|---|
| `crates/core` (`wavcraft-core`, library, no UI) | `lib.rs` | Re-exports `Document`, `OpenError`, `SpectrogramParams`, `Viewport`, `Selection`, `format_time`, and audio-io's `AudioBuffer`, `AudioInfo`, `FileFormat`, `SampleFormat` |
| | `store.rs` (task 03) | `Store`: the session folder, append-only `pack-NNNN.f32` files (1 GiB rollover), `append` → `Arc<BlockHandle>` (≤ 65,536 frames of one channel), `read` through an LRU block cache with a byte budget (positioned reads, no mmap), live counts per pack (a sealed pack with none is deleted), the zero block, `bytes_on_disk`/`bytes_live`, `keep` |
| | `doc.rs` (task 03) | The piece table: `Piece` (block, offset, len), `Channel` (pieces + running ends; `locate`, `split_at`, `read`), `DocState` (one `Arc<Channel>` per channel, all one length; `read`, `read_frames`, `disk_bytes_referenced`, and the temporary `to_audio_buffer`), `SourceFormat` (format, chunks with offsets, warnings) |
| | `doc/edit.rs` (task 04) | The edits, pure functions `&DocState -> Edit { state, label, change }`: `delete` (a `ChannelMask`; with only some channels it silences in place, label "Delete Audio"), `copy` → `Clip` (pieces, no sample copy), `cut`, `paste` (`PasteAt::Insert`/`Replace`, a `ChannelMask`; equal channel counts, mono into any, any into mono as the mean; other sample rates refused; a clip from another store copied in; with only some channels it overwrites them from the insert frame or the range's start for the clip's length and keeps the length, clip frames past the end dropped), `silence`, `insert_silence`, `crop` (a `ChannelMask`; with only some channels it silences everything outside the range in them and keeps the length; an empty range is no edit), `replace_range` (a producer fills 64 K-frame chunks; cancel flag). `Crossfades { cut_join: 2 ms, edge: 5 ms }` (raised-cosine curve, one new block per fade, shortened where the audio runs out; the tests' click tolerance, `STEP_TOLERANCE_64`/`_96` in `crates/core/tests/edits.rs`, holds for a full-scale sine at 1 kHz and above only: at 220 Hz and 50 Hz the 2 ms fade's own slope exceeds it, which is not a click; W6, 2026-10-16). `Change` (`None`, `SameLength`, `LengthFrom`) for the views |
| | `effects.rs` (f01) | `Effect` trait (`id`, `name`, `parameters`, `process` of one chunk), `REGISTRY` (add an effect here), `apply` (streams a range through `replace_range`; empty range = whole file; undo step named for the effect), `Invert` (reference). Since f14: `ParamDesc` has a `ParamKind` (`Db`, `Percent`, `Time`, `Choice` + `choices`) and const constructors; `parse_params` (`key=value` text over the defaults, units and choice names allowed), `validate` (also run by `apply`), `describe`; `effects/gain.rs` is the parametrised example (`gain_db`). Since f27: `ParamKind::Frequency`/`Number`; `Effect::begin(params, &RunContext) -> Option<Box<dyn Run>>` for effects that need memory between chunks, the sample rate or a pre-measurement (`apply` calls the run instead of `process`); `effects/eq.rs` holds Parametric EQ (low shelf, two bells, high shelf), High-Pass and Low-Pass (12/24 dB/oct), Notch and Remove DC Offset, on `soundcraft-dsp` biquads, one filter chain per channel carried across the 64 K-frame chunks (tests: `crates/core/tests/filters.rs`) `Editor::run_effect(i, params, sel)` runs one with settings and remembers `Repeat::Effect(i, params)` for Repeat Last Command. Registered effects appear as `ActionId::Effect(i)` rows `effect.<id>` in `actions::all_actions()`/`by_name`, and in the app's Effects menu |
| | `generate.rs` (f21) | `Generator` trait (`id`, `name`, `parameters` as the same `ParamDesc` list effects use, `source` per channel, `duration`), `REGISTRY`: `Tones` (base frequency, sine/triangle/square/sawtooth, `amp` peak dBFS, `dur`, 2nd to 5th harmonics as % of the base, scaled so the sum stays under `amp`; harmonics past Nyquist dropped) and `Noise` (white/pink/brown, `amp` is a peak ceiling, tiny `XorShift` RNG, seeded; channels differ). `render` makes a short in-RAM buffer for Preview. `Editor::generate(i, params, selection, seed)` inserts at the cursor or replaces the selection on every channel through `doc::edit::generate` (a producer fills 64 K-frame chunks, one new block per chunk and channel; one undo step "Generate Tones"/"Generate Noise"; not remembered for Repeat Last Command). `ParamKind::Number` (Hz) and `effects::{validate_list, parse_list, describe_list}` serve both. App: Generate menu, the effect dialog with a Preview button (a 2 s loop through the player while the dialog is up), control `GENERATE`, `GENERATE_DIALOG` |
| | `doc/history.rs` (task 04) | `History`: steps of `(label, Arc<DocState>, made_bytes, change)` and a cursor; `push`/`push_edit` (drops redo steps; a no-op records nothing), `undo`, `redo`, `go_to`, labels, `clear`; the disk cap (bytes the kept steps hold beyond the current state, kept by per-block step counts; drops the oldest steps and says how many), `default_cap`, `can_afford` (keeps 2 GB free) |
| | `space.rs` (item 03b) | `store_bytes(frames, channels)` (4 bytes a sample + 1 % + `MARGIN_BYTES` 64 MiB), `frames_from_file_size` (at `ASSUMED_BITRATE` 128 kbit/s), `free_space(path)` (since W7, 2026-10-16: on Unix `statvfs` of the folder itself through `rustix`, so a tmpfs `/tmp` or `/dev/shm` reports its own figure; on Windows, or if that fails, sysinfo's disk list, longest mount point; Linux-only test `a_tmpfs_folder_reports_its_own_free_space`), `FreeSpaceFn`, `format_bytes` |
| | `settings.rs` (item 03b) | `Settings` (`settings.txt` in `dirs::config_dir()/wavcraft`: `session_folder = …`), `check_folder` (make it, write a probe), `resolve_session_folder` (flag > `WAVCRAFT_SESSION_DIR` > settings file > default; a folder that fails falls back to the default with a warning), `HOW_TO_CHANGE` |
| | `panels.rs` (A008) | `REGISTRY` of panels (`id`, `title`, `default_dock`: waveform, spectral, history), `PanelSet` (which are shown; `apply_action` for `window.toggle.<id>` / `window.show.<id>` / `window.hide.<id>`; `report`). Hidden ids are kept in `settings.txt` as `hidden_panels`. The app's Window menu lists the registry; control verb `PANELS` and `ACTION window.*` |
| | `io.rs` (task 03) | `open(path, opts, progress, cancel) -> DocState`: our own WAV/RF64/AIFF chunk reader converting exactly as audio-io, `symphonia` packet by packet for the rest; `OpenError`. Since item 03b: `OpenOptions::free_space` (injectable), a space check before the first block is written (`OpenError::NoSpace`), a full disk while writing (`OpenError::DiskFull`, session removed). Since the Linux handheld review (W8a, 2026-10-16): a session folder that can't be made or written is `OpenError::SessionFolder { folder, reason }`, which names the folder and `HOW_TO_CHANGE`, not "cannot read the file" (tests: `crates/testkit/tests/hostile.rs`). W8b: numbers from the header are checked before anything is written: a sample rate of 0 or above `MAX_SAMPLE_RATE_HZ` (768,000) is refused (`check_rate`, all three readers), a WAV block-align that isn't channels × bytes a sample is refused (PCM and float only), a sample size under 8 bits opens as whole bytes with a warning (`small_sample_warning`); data shorter than the header claims already opened what is there with a warning in `SourceFormat::warnings`, now also shown by the app |
| | `io.rs` (task 03) | `open(path, opts, progress, cancel) -> DocState`: our own WAV/RF64/AIFF chunk reader converting exactly as audio-io, `symphonia` packet by packet for the rest; `OpenError`. Since item 03b: `OpenOptions::free_space` (injectable), a space check before the first block is written (`OpenError::NoSpace`), a full disk while writing (`OpenError::DiskFull`, session removed). Since the Legion Go review (W8a, 2026-10-16): a session folder that can't be made or written is `OpenError::SessionFolder { folder, reason }`, which names the folder and `HOW_TO_CHANGE`, not "cannot read the file" (tests: `crates/testkit/tests/hostile.rs`). W8b: numbers from the header are checked before anything is written: a sample rate of 0 or above `MAX_SAMPLE_RATE_HZ` (768,000) is refused (`check_rate`, all three readers), a WAV block-align that isn't channels × bytes a sample is refused (PCM and float only), a sample size under 8 bits opens as whole bytes with a warning (`small_sample_warning`); data shorter than the header claims already opened what is there with a warning in `SourceFormat::warnings`, now also shown by the app |
| | `document.rs` | `Document`: the open file as the app sees it: streamed open, then read back whole (`audio`) until task 05, the `DocState` kept in `state`, per-channel waveform peaks, `waveform_columns()` for drawing, `warnings()` (the open's `SourceFormat::warnings`, since W8b) |
| | `spectrogram.rs` | The spectral display maths: `SpectrogramParams`, `Columns` (file-fixed column grid), `Overview` (coarse per-channel cache), `render()` / `render_columns()`. Private: `Analysis` (FFT plan, Hann window, dB→level table), `Analyser` (FFT scratch, per-bin max) |
| | `multitrack.rs` (f103) | `Multitrack` over `soundcraft_model::Session`: new session (name, rate, bit depth), add / remove / rename audio tracks, mute / solo / volume, clips from a file, a buffer or an open `Document` (in-RAM sources, capped at `MAX_IMPORT_SAMPLES`; long files wait for the fork's disk-backed sources, #302), `state_json`. The model is pinned separately from audio-io (see the root `Cargo.toml`) |
| | `view.rs` | `Viewport` (zoom/scroll in frames), `Selection` (anchor/head), `format_time` |
| | `examples/spectrogram_cost.rs` | Measures overview build and far-view render times on a synthetic 3-hour mono file |
| `apps/wavcraft` (`wavcraft`, binary, egui/eframe 0.36, wgpu DX12 on Windows) | `main.rs` | Window options, DX12-only wgpu on Windows, `parse_args` (`[--session-dir DIR] [FILE]`), runs `WaveApp` |
| | `app.rs` | `WaveApp`: menu (File › Session Folder… / Use the Default Session Folder since item 03b), status bar, the editor painter (ruler, waveform lanes, spectral lanes, selection, cursor, playhead), input (wheel zoom, drag select, Space, Home, `\`, Ctrl+O, drop), opening on a worker thread; the status bar shows the open's first warning (all of them on hover) since W8b |
| | | `effect_dialog.rs` (f14) | `EffectDialog`: draws any effect's `ParamDesc` list (slider per dB/percent/time, combo box per choice) with OK / Cancel / Reset; the app applies the values. Control: `EFFECT`, `EFFECT <name> key=value ...`, `EFFECT_DIALOG ...` |
| `player.rs` | `Player`: cpal output stream, play/stop, playhead position |
| | `spectrum.rs` | `Renderer`: the spectrogram worker thread and its overview cache |

Workspace: `Cargo.toml` at the root; `unsafe_code = "forbid"` for every crate that inherits the
workspace lints. Tests: 21 in the core, 9 in the app (before task 02).

## Known differences from Audition

Choices made where Audition 26.5.0.82 was not seen (`plan/tasks/audition-check.md`, 2026-10-16
Result, has what was seen):

- **One-channel Paste near the end of the file:** the clip is cut short at the end; the length
  never grows. Audition was only seen pasting mid-file.
- **One-channel Paste over a selection:** the clip overwrites from the selection's start for the
  clip's length, whatever the selection's length. Audition was only seen pasting at a cursor.
- **Zero crossings on stereo (task 07c):** an edge moves to where the *sum of the active channels*
  changes sign (or is exactly 0), on its side, within 10 ms; with none there it stays, and an
  inward adjust that would leave nothing changes nothing. Audition's rule for stereo was not seen.
- **Repeat Last Command (task 07c):** repeats Silence, Paste, Crop, Insert Silence (same length,
  at the cursor) and the six zero-crossing adjusts. Delete is not repeatable (Audition); Undo,
  Redo, Cut, Copy, selection moves and the view are not either. With the dialog (F2, Ctrl+R) Insert Silence
  and any effect with settings reopen their dialog on the last values (f14); the others run at once. An effect is repeated with the settings it last ran with, on the current selection.
- **History labels:** one-channel Crop, Cut and Paste are labelled "Crop", "Cut" and "Paste";
  Audition says "Crop Audio", "Cut Audio" and "Paste Audio" (one-channel Delete is "Delete Audio"
  in both).

## Where the samples live after open

*Since task 03 (2026-10-15):* `Document::open` streams the file into a session store
(`core::io::open`, see `store.rs`/`doc.rs` above) and keeps the `DocState` in `Document.state`.
It then reads the whole document back into the `AudioBuffer` below, because every consumer
listed here still wants whole channels. Task 05 moves them onto `DocState::read` and drops the
copy. What follows describes those consumers, unchanged.

- Type: `soundcraft_audio_io::AudioBuffer { sample_rate: u32, channels: Vec<Vec<f32>> }`: planar
  `f32`, **one contiguous `Vec<f32>` per channel for the whole file**. No chunks or blocks.
- Owner: `Document.audio: Arc<AudioBuffer>`; the app keeps `WaveApp.doc: Option<Document>`.
- Opening (`Document::open_with`): stream into the store (a few MB of RAM), read it back into one
  `Vec<f32>` per channel, then one `Peaks` per channel on scoped threads. (`Document::from_bytes`
  still decodes bytes with audio-io; tests use it.) The app runs it on a `std::thread::spawn` worker
  with a cancel flag and progress, cancels it when another file is opened, and polls an `mpsc`
  channel each frame (`finish_opening`).
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
- Binary `wavcraft-store-bench FILE [--cache-mb N] [--sessions DIR] [--huge]` (task 03): streamed
  open time and peak memory above rest, sequential reads, 10,000 random 4,096-frame reads.
- `--huge` writes `huge-4h10m-48k-stereo-24-rf64.wav` (4.32 GB, RF64: `WavWriter` writes RF64 above
  4 GiB); its right channel is a frame counter (`huge_counter`).
- Binary `wavcraft-edit-bench FILE [--cache-mb N] [--sessions DIR] [--edits N]` (task 04): cut an
  hour out of the middle, undo, paste back, `replace_range` over a minute and the whole file, N
  random edits (`random_edit`) with push times and memory, undo all and a bit-identical check.
  Test 10 (`crates/testkit/tests/edits.rs`, `#[ignore]`) runs the same random edits.
- Binary `wavcraft-bench FILE`: open time and peak memory, peaks, overview and picture times
  (the worker's own calls, channel by channel), a full ordered read. Results: `docs/baseline.md`.
  It reads process memory through `windows-sys` on Windows (the only `unsafe` in the tree, in
  `src/bin/wavcraft-bench/memory.rs`) and `/proc/self/status` on Linux.
