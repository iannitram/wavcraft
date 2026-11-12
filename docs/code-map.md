# Code map

Describes `main` at `11e49a5`. Rewritten 2026-11-11 from the tree, file by file. What is built and what remains per v1 item: [ROADMAP.md](../ROADMAP.md). Update this file in the same pull request as any change that adds, removes, renames or rewires a file.

**Status words:** *present, wired* (on `main` and reached from the app or a binary); *present, core only* (on `main` with tests, but nothing in the app calls it yet); *present, out of v1* (on `main`, belongs to an after-1.0 feature); *stub* (the body is `todo!()` or `unimplemented!()`); *dead code* (no caller and no test).

**Counts:** 442 files and folders described: 283 present, wired, 111 present, core only, 43 present, out of v1, 4 stub, 1 dead code.

## Contents

- Workspace
- Data flow
- Files by area
- `apps/wavcraft/src/app.rs` by line range
- Tests
- Control verbs
- Command-line flags and environment variables
- Deeper notes
- Mac, PC and Linux
- Planned, not on main

## Workspace

The root `Cargo.toml` is one workspace (resolver 3, edition 2024, `rust-version` 1.95, licence MIT OR Apache-2.0). It has six members and builds five binaries.

| Member | Package | Kind | What it holds |
|---|---|---|---|
| `crates/core` | `wavcraft-core` | library | Document, store, file reading and saving, effects, analysis, multitrack model. No UI. |
| `crates/automation` | `wavcraft-automation` | library | `Request`, `Backend`, `Headless`, `Remote`. Its only dependency is `wavcraft-core`. |
| `crates/testkit` | `wavcraft-testkit` | library and 4 binaries | Test file generator and the benches. |
| `crates/craft-log` | `craft-log` | library | Logging and the system report. |
| `crates/craft-stage` | `craft-stage` | library | No dependencies. Used by `craft-log`. |
| `apps/wavcraft` | `wavcraft` | 1 binary | The egui app. |

The five binaries:

- `wavcraft` (`apps/wavcraft/src/main.rs`)
- `wavcraft-testkit` (`crates/testkit/src/main.rs`)
- `wavcraft-bench`, `wavcraft-store-bench`, `wavcraft-edit-bench` (`crates/testkit/src/bin/<name>/main.rs`)

### Lint levels

The workspace sets `unsafe_code = "forbid"` and warns on clippy `dbg_macro` and `todo`.

| Crate | `unsafe_code` | Where unsafe is allowed |
|---|---|---|
| `wavcraft-core` | forbid (inherited) | nowhere |
| `wavcraft-automation` | forbid (inherited) | nowhere |
| `craft-log` | forbid (inherited) | nowhere |
| `craft-stage` | forbid (inherited) | nowhere |
| `wavcraft-testkit` | deny in `Cargo.toml`. `src/lib.rs` and `src/main.rs` forbid it for themselves. | one `#[allow(unsafe_code)]` in `src/bin/wavcraft-bench/memory.rs` (`GetProcessMemoryInfo`, Windows only) |
| `wavcraft` (app) | deny | `apps/wavcraft/src/win_acl.rs` (`#![allow(unsafe_code)]`, Windows only). Two test helpers that set an environment variable also carry `#[allow(unsafe_code)]`: `issue377_tests.rs` and `f48_device_change_tests.rs`. |

`apps/wavcraft/src/main.rs` also denies clippy `unwrap_used`, `expect_used`, `panic`, `unimplemented`, `todo` and `unreachable`. `crates/core/src/lib.rs` and the testkit library, binary and benches carry the same deny list. The dev profile builds my code at opt-level 1 and dependencies at opt-level 3.

### Dependency graph

```
wavcraft (app)
  -> wavcraft-core
  -> craft-log -> craft-stage
  -> soundcraft-playback, -model, -mix, -audio-io, -dsp, -video
  -> eframe, egui, cpal, rfd, arboard, dirs, serde_json, log
  -> windows-sys            (Windows only)

wavcraft-core
  -> soundcraft-audio-io, -dsp, -model, -time, -mix, -vst3-host
  -> symphonia, vorbis_rs, rustfft, rayon, bytemuck
  -> dirs, sysinfo, getrandom, serde, postcard, serde_json, log
  -> rustix                 (unix only)

wavcraft-automation -> wavcraft-core
wavcraft-testkit    -> wavcraft-core, soundcraft-audio-io, rayon
                    -> windows-sys   (Windows only)
```

The app does not depend on `wavcraft-automation`. `apps/wavcraft/src/control.rs` parses its own requests.

### The soundcraft pin

All soundcraft crates come from one git revision of the soundcraft repo: `e3c5c672f7579c706071355a96826df19a7a1c40` (version 0.13.1 in `Cargo.lock`). The root `Cargo.toml` names eight crates at that revision: `soundcraft-audio-io`, `soundcraft-dsp`, `soundcraft-video`, `soundcraft-model`, `soundcraft-time`, `soundcraft-mix`, `soundcraft-playback`, `soundcraft-vst3-host`. `Cargo.lock` also holds `soundcraft-clap-host`, `soundcraft-au-host` and `soundcraft-midi`. `soundcraft-clap-host` and `soundcraft-au-host` come in through `soundcraft-mix`. `soundcraft-midi` comes in through `soundcraft-mix` and `soundcraft-model`. They compile, but my code loads nothing from them.

What I use each one for:

- `soundcraft-audio-io`: `detect_format` in `stream_file` (`crates/core/src/io.rs`) and in `crates/core/src/probe.rs`, the shared types (`AudioBuffer`, `AudioInfo`, `FileFormat`, `SampleFormat`, re-exported from `crates/core/src/lib.rs`; `BwfInfo` is used but not re-exported), `probe` in `multitrack.rs`, and `encode` and `decode` for whole buffers (`document.rs`, `multitrack.rs`; `session_file.rs` uses `encode` only, `mtrecord.rs` uses `decode` only) and the testkit.
- `soundcraft-dsp`: filters and processors for effects (`biquad` in `effects/eq.rs` and `deesser.rs`, `create` and `Plugin` in `effects/dsp_run.rs`, `PitchShifter` and `Saturator` in `effects/pitch_saturate.rs`, `FadeShape` and gain helpers in `effects/oneshot.rs`), the `Plugin` trait in `crates/core/src/vst3.rs`, `pitch_detect` in `timepitch.rs`, `LoudnessMeter` in `loudness.rs`, `PeakMeter` in `levels.rs`, and the pan law in `apps/wavcraft/src/mixer.rs`.
- `soundcraft-model`: the multitrack session types (`Session`, `Track`, `Clip`, the id types, `SampleSource`) in `crates/core/src/multitrack.rs`, `multitrack/`, `sc_source.rs`, `session_file.rs`, `mixdown.rs`, `mtplay.rs`, `mtrecord.rs`, `mtpunch.rs`, and in the app `multitrack_ui.rs`, `mixer.rs` and `app.rs`.
- `soundcraft-time`: `TempoMap`, `BarBeat` and `SampleRate` in `crates/core/src/tempo.rs` and `sc_source.rs`.
- `soundcraft-mix`: `MixEngine`, `ReadAheadSource` and `render_range` for multitrack playback (`crates/core/src/mtplay.rs`), `RangeRender` for mixdown (`mixdown.rs`), and the app mixer (`apps/wavcraft/src/mixer.rs`).
- `soundcraft-playback`: `Player` as the output device for multitrack only (`apps/wavcraft/src/multitrack_ui.rs`). The single-file editor talks to cpal directly in `apps/wavcraft/src/player.rs`.
- `soundcraft-video`: `Movie` and `VideoError` in `apps/wavcraft/src/video.rs`.
- `soundcraft-vst3-host`: plugin scan and load in `crates/core/src/vst3.rs` (`default_search_paths`, `add_search_dir`, `find_bundles`, `scan_paths`, `instantiate_plugin`, `Vst3Plugin::reactivate`).

### Other external crates

- `symphonia` 0.5.5, MPL-2.0. I use it unmodified to decode MP3, FLAC, Vorbis, AAC, ALAC, ADPCM, MP4, CAF and MKV (`open_symphonia` in `crates/core/src/io.rs`). `Cargo.lock` holds two versions: 0.5.5 (mine) and 0.6.1 (through `soundcraft-audio-io`). The comment in `Cargo.toml` that says both use the same version is out of date.
- `vorbis_rs` 0.5.6, BSD-3-Clause. It vendors libvorbis and libogg as C source (`aotuv_lancer_vorbis_sys` and `ogg_next_sys`, both built with `cc`). Every build needs a C compiler.
- `eframe` and `egui` 0.36 with features `wgpu`, `wayland`, `x11` and `default_fonts`. The bundled fonts are Hack (MIT and Bitstream Vera), Ubuntu Light (Ubuntu Font Licence 1.0), Noto Emoji (OFL 1.1) and emoji-icon-font (MIT). `THIRD-PARTY-NOTICES.txt` lists them. `tools/notices.py` makes that file.
- `cpal` 0.18 for audio devices. `rfd` 0.17 for file dialogs (XDG portal on Linux). `arboard` 3.6 for the clipboard, default features off.
- `rustfft`, `rayon`, `bytemuck` for analysis and the store. `serde` and `postcard` for peak cache files. `sysinfo` and `rustix` for free disk space. `dirs` for the config, cache and audio folders. `getrandom` for temp file names.
- `wgpu` 30 and `image` 0.25 arrive through `eframe` and `soundcraft-video`.

## Data flow

**Open.** `WaveApp::open` calls `open_as` (`apps/wavcraft/src/app.rs`), which starts a thread. The thread calls `Document::open_in` (or `open_raw_in`) with `OpenOptions` (`crates/core/src/document.rs`). `open_in` takes a `peakcache::Key::of_file`, then `io::open` runs `open_inner`: `Store::create_session`, then `stream_file`. `stream_file` picks `open_wav`, `open_aiff`, or `containers::open_w64`, `open_caf`, `open_au`, `open_raw`. Anything else goes through `detect_format` and `open_symphonia`. Samples land in the on-disk `Store` in blocks of 65,536 frames. `space::free_space` checks the disk first. The result is a `DocState`. `Document::from_state_cached` then loads peaks with `peakcache::load`, or builds them with `build_all` (one thread per channel) and saves them with `peakcache::store`. `finish_opening` polls the thread, parks the old document, calls `Player::set_audio`, makes a new `Editor` over a new `History`, and bumps `generation`.

**Edit and history.** The edit functions in `crates/core/src/doc/edit.rs` (`delete`, `cut`, `paste`, `silence`, `crop`, `replace_range`, `generate`, `mix_paste`) return an `Edit` with a new `DocState`. They change the piece table, not the samples. `Editor::run` and `Editor::run_effect` (`crates/core/src/actions.rs`) push the result with `History::push_edit`. Long effects run in `jobs::Job::spawn_replace` and push in `finish_job`. Undo, redo and `go_to` hand back an earlier `Arc<DocState>`. After any step `WaveApp::rebuild` starts `Document::after_step`. That calls `EditSpan::between` (`doc/span.rs`) to find what changed, then `WavePeaks::updated` per channel with the `PeakBlockCache` (256-frame summaries keyed by block, in `crates/core/src/document.rs`). It falls back to `build_all` when the change covers more than half the document. `finish_rebuild` calls `Player::replace_audio` and bumps `generation`. `Recovery::track` (`apps/wavcraft/src/recovery.rs`) hands a `Snapshot` to the `Autosaver`, which writes it with `write_journal`.

**Spectrogram worker.** `Renderer::new` (`apps/wavcraft/src/spectrum.rs`) starts one worker thread. Each frame the app calls `Renderer::ask` with a `Job` (generation, columns, rows, parameters). The worker keeps only the newest request. It reuses an `Overview` when `Overviews::serve` finds one for the same FFT size, window and dB range. Otherwise it builds one with `Overview::build_from`. On a new generation it calls `Overview::after_step`, which uses `spectral_dirty::dirty_blocks` to redo only the changed blocks (16 hops each), then `refresh_visible`. It keeps one retired overview so undo straight back is instant. It draws with `spectrogram::render_columns_from`, colours the result into an `egui::ColorImage`, and the app collects it with `Renderer::take`.

**Playback.** `Player::new` (`apps/wavcraft/src/player.rs`) starts the `wavcraft-readahead` thread, which runs `read_ahead`. `Player::set_audio` sends it the document. `Player::start` and `Player::play` call `begin_play`. The thread then runs `begin`, and `top_up` and `fill_all` read from `DocState::read` into a ring (`RING_FRAMES` is 131,072 frames, `RING_CHANNELS` is 8). `open_stream` and `build` open the cpal device. `device_callback` calls `render`, then `render_with`. That path never blocks and never allocates. It plays silence and counts an underrun when the ring is short. `rebuild_resampler` sets up `wavcraft_core::resample` when the device rate differs. `watch_default_device` and `Player::maintain` reopen the stream when the default device changes. With `WAVCRAFT_NO_AUDIO` set, `Sink` stands in for the device. Multitrack playback is separate: `PlaybackSession` in `crates/core/src/mtplay.rs` with `soundcraft_playback::Player`.

**Recording.** `WaveApp::start_recording` leads to `Recording::start_with` (`apps/wavcraft/src/recording.rs`). `record_dir` picks the folder. `record::create_take` creates the WAV file (exclusive create) and returns a `TakeWriter`. `rec_input::open` opens the cpal input stream. `FakeInput::spawn` (`crates/core/src/record.rs`) replaces it when `FakeSpec::from_env` finds a fake input. The device callback calls `Ring::push`, which never blocks. It drops and counts frames when the ring is full. `Recorder::start` runs a writer thread that drains `Ring::pop` into the `TakeWriter`. The writer rewrites and syncs the 44-byte header once a second, so a crash loses at most a second. An optional `MonitorRing` feeds `Player::set_monitor`. `finish_recording` calls `Recording::stop`, which returns a `Take`. The app then opens the take with `WaveApp::open`, or runs `integrate_take` (`io::open` plus `rec_edit::apply_take_with_offset`) and `finish_integration`, which pushes one edit.

**Save.** `WaveApp::save` calls `save_with`. It picks the path (an `rfd` dialog when needed), the `SaveFormat` from the extension (`Wav`, `Aiff`, `Flac`, `Vorbis`), or a separate `Container` value for W64, CAF and AU, and the `SampleType`. A thread calls `save_wav_with`, `save_aiff_kind` (AIFF and FLAC), `save_vorbis` or `save_container`. `save_container` is the app's name for `save_as` in `crates/core/src/save.rs`. In `crates/core/src/save.rs`, `save_inner` removes old temp files with `remove_leftover_temps`, makes a `TempFile` beside the target (`TempFile::create`, exclusive create), and runs `write_stream`. `write_stream` reads 64 K frames at a time through `DocState::read`. For WAV it copies the source's extra chunks (`plan_chunks`) and switches to RF64 above 4 GiB. FLAC uses my own encoder in `flac_enc.rs`. Vorbis uses `write_vorbis_stream`. The last step is `std::fs::rename` over the target. `finish_saving` updates the saved marker and path. `save_copy` does the same for Save Selection As and Export. There is no MP3 save.

## Files by area

Paths are from the repo root. "Items" are v1 item ids from the roadmap; "Uses" are the modules a file leans on.

### Store and document (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `crates/core/Cargo.toml` | The manifest of the wavcraft-core crate: its name, description and the libraries it builds against. **Platform:** rustix is a dependency on unix targets only. | `wavcraft-core` | soundcraft-audio-io, soundcraft-dsp, soundcraft-model, soundcraft-time, soundcraft-mix, soundcraft-vst3-host, rustfft, rayon, symphonia, vorbis_rs, bytemuck, dirs, sysinfo, log, getrandom, serde, serde_json, postcard; proptest for tests | present, wired |  | none |
| `crates/core/src/autosave.rs` | The session journal: writes the piece table, undo history and extras durably on a worker thread, lists recoverable sessions after a crash, and rebuilds or discards one. **Platform:** cfg(unix): the journal temp file is created owner-only (0600) and the folder is synced; other systems do neither. | `Autosaver`, `Snapshot`, `write_journal`, `recoverable_sessions`, `recover`, `discard` | store, doc, doc::history, markers, meta, soundcraft_audio_io | present, wired | I4 | autosave.rs, 990_autosave_markers.rs, journal_frame.rs, sessions.rs, sweep.rs. Called from apps/wavcraft/src/recovery.rs and app.rs. |
| `crates/core/src/channels.rs` | Speaker labels from a WAV extensible channel mask, the fold-down gains from many channels to stereo, named channel layouts and per-channel colours. | `labels`, `usable_mask`, `stereo_downmix`, `ChannelLayout`, `channel_color`, `Gains` | std only | present, wired | A1, B1, C5 | multichannel.rs, settings.rs; 5 in-file. Called from apps/wavcraft/src/player.rs and app.rs, io.rs, save.rs. |
| `crates/core/src/doc.rs` | The document model: a piece table over the store's unchanging blocks, with per-channel piece lists, the shared DocState (rate, source format, markers, metadata) and the single sample read path. | `DocState`, `Channel`, `Piece`, `SourceFormat`, `DocError`, `DocState::read` | store, markers, meta, soundcraft_audio_io | present, wired | A1, D1, D5, I7 | store_and_doc.rs, edits.rs, chunks.rs, testkit kit.rs and edits.rs. Used across the app and crates/automation headless.rs. |
| `crates/core/src/doc/append.rs` | Open Append: streams files into the document's session store and joins them to the end as one edit, or builds a new document from several files back to back. | `append_files`, `new_from_files`, `AppendReport`, `NewReport`, `APPEND` | doc::edit (paste), io (open_into), space | present, wired | after 1.0 | append.rs. Called from apps/wavcraft/src/app.rs. |
| `crates/core/src/doc/newdoc.rs` | New documents with their own session store: an empty one with a chosen format, and one streamed from a clip for Copy to New and Paste to New. | `empty`, `from_clip`, `NewDocOptions`, `NewDocError`, `supported`, `MAX_CHANNELS` | doc, doc::edit::Clip, store, space, io | present, wired | D2 | newdoc.rs; app issue369_tests.rs. Called from apps/wavcraft/src/app.rs. |
| `crates/core/src/doc/span.rs` | Finds what a change replaced by comparing the piece tables of two states, and maps a play position across that change. | `EditSpan`, `EditSpan::between`, `map_position` | doc (Channel, DocState) | present, wired | C5, B1, B2 | edit_span.rs, incremental_overviews.rs. Called from apps/wavcraft/src/player.rs, spectrum.rs and app.rs, document.rs and spectrogram.rs. |
| `crates/core/src/document.rs` | The open file as the views use it: the document in its session store plus per-channel waveform peaks, built and updated incrementally after an edit, with a peak block cache. | `Document`, `WavePeaks`, `PeakBlockCache`, `StoreChannel`, `PeakReport`, `Document::after_step` | doc, doc::span, io, peakcache, space, spectrogram::Samples, store, view (format_time), soundcraft_audio_io | present, wired | A1, B1, B3, I7 | incremental_overviews.rs, take_recovery.rs, testkit hostile.rs and kit.rs; 11 in-file. Called from apps/wavcraft/src/app.rs, spectrum.rs, multitrack_ui.rs and the wavcraft-bench binary in crates/testkit. |
| `crates/core/src/io.rs` | Streamed open: my own WAV, BWF, RF64 and AIFF chunk parser and symphonia for the other formats, written into a session store in 64 K-frame chunks, with progress, cancel, size limits and open errors. | `open`, `open_raw`, `open_into`, `OpenOptions`, `OpenError`, `store_encoding` | store, doc, markers, meta, channels, symphonia, soundcraft_audio_io; used by document.rs, app.rs, batch.rs, automation headless.rs; control verb `OPEN` | present, wired | A1, A6, I7 | 4 in-file tests; testkit open.rs, formats.rs, hostile.rs; multichannel.rs, issue1009_open_into_rate.rs |
| `crates/core/src/io/channel_mask.rs` | Conversions between a WAV channel mask, a speaker list and a named layout. The open path reads masks through channels.rs and meta.rs instead, so nothing calls this. | `Speaker`, `ChannelLayout`, `speakers_from_mask`, `mask_from_speakers`, `layout_from_mask`, `channel_count` | std only | present, core only | after 1.0 | 11 in-file tests |
| `crates/core/src/io/containers.rs` | Readers for Sony Wave64, Apple CAF (PCM and float), Sun/NeXT AU, and headerless raw data described by a RawSpec, all streamed with the same conversion as WAV. | `RawSpec`, `RawEncoding`, `RawEncoding::parse`, `RAW_MAX_CHANNELS`, `RAW_MAX_RATE_HZ` | io.rs internals, save (W64 GUIDs); used by io.rs, document.rs, app.rs; control verb `IMPORT_RAW` | present, wired | after 1.0 | testkit formats.rs |
| `crates/core/src/lib.rs` | The core crate root: declares every core module and re-exports the main document, edit, marker, view and audio buffer types. | `DocState`, `Document`, `History`, `Markers`, `Edit`, `OpenError` | all core modules, soundcraft_audio_io | present, wired | A1 to N4 (crate root) | none in file |
| `crates/core/src/meta.rs` | File metadata a WAV carries and a save keeps: the Broadcast Wave bext chunk, LIST/INFO tags, channel names from the mask and iXML, a field get and set API, and a Vorbis comment body. | `Metadata`, `Bext`, `InfoTags`, `FIELDS`, `mask_names`, `vorbis_comment_body` | xmp; used by io.rs, save.rs, doc.rs, meta_view.rs, app.rs; control verbs `METADATA`, `META get` | present, wired | A1, A2 | 13 in-file tests; chunks.rs, xmp.rs, source_link.rs |
| `crates/core/src/peakcache.rs` | Waveform overview cache files in the app's cache folder, keyed by source size, time and header hash, with checksum checks, stale and corrupt detection, and eviction under a size cap. | `Key`, `Miss`, `load`, `store`, `evict`, control verb `PEAKCACHE` | document::WavePeaks; used by document.rs and apps/wavcraft/src/app.rs | present, wired | A1, B1 | 7 in-file tests |
| `crates/core/src/probe.rs` | Reads what a file is from its header alone (format, sample type, rate, channels, length, title) without reading audio or making a session store. | `FileInfo`, `probe` | crate io, symphonia; used by apps/wavcraft/src/app.rs for File Import and Files panel columns | present, wired | A1, A5 | probe.rs (6 tests) |
| `crates/core/src/random.rs` | Random hex identifiers from the operating system's secure generator for temporary file names and the control channel token. It fails closed with no fallback. | `hex` | OS random source; used by save.rs, store.rs, session_file.rs, mixdown.rs, mtrecord.rs and apps/wavcraft/src/control.rs | present, wired | A2, N1 | 1 in-file test |
| `crates/core/src/sessions.rs` | Reports the disk used by session folders and removes old ones that are unlocked, hold no journal and have been idle for the days asked. | `Usage`, `Cleanup`, `usage`, `cleanup`, `removable`, `idle_for` | store | present, wired | I4, I7 | crates/core/tests/sessions.rs; 0 in-file |
| `crates/core/src/space.rs` | Works out how many bytes opening a file writes to the session store, guesses frames from a compressed file's size, reads free disk space and formats byte counts as text. **Platform:** Free space comes from statvfs on unix. Other systems report none. One test runs on Linux only. | `store_bytes`, `raw_store_bytes`, `frames_from_file_size`, `free_space`, `format_bytes`, `MARGIN_BYTES` | rustix (unix only); no other core module | present, wired | A1, I7, J4 | crates/testkit/tests/space.rs; 3 in-file |
| `crates/core/src/store.rs` | Owns the on-disk sample store: append-only pack files in a session folder, block handles, the byte-budgeted block cache, session locks, and the start-up sweep of stale sessions. **Platform:** Positioned reads use the unix or the Windows file extension. Big-endian targets swap bytes on read and write. Machine id and default sessions root differ per OS. | `Store`, `StoreOptions`, `BlockHandle`, `Encoding`, `stale_sessions`, `sweep_stale_sessions` | autosave, random, bytemuck | present, wired | A1, D1, I4, I7 | store_and_doc.rs, sweep.rs, autosave.rs, history_restore.rs and most core tests through open; 1 in-file |

### Edits and actions (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/mixpaste.rs` | Mix Paste in the window: parsing the MIXPASTE control line, the dialog's settings, and loading the source clip from a file. The edit itself is in the core crate. | `Request`, `parse`, `clip_from_file`, `Dialog`, `MIXPASTE` | wavcraft_core::doc::edit (mix_paste, MixMode, Clip), io; egui | present, out of v1 | after 1.0 | 3 in-file tests; crates/core/tests/mix_paste.rs for the core side |
| `crates/core/src/actions.rs` | The table of edit action ids and menu labels, and the Editor that runs them on a document History without a window: clipboards, snapping flags, zero-crossing adjust and Repeat Last. | `ActionId`, `Editor`, `Outcome`, `Repeat`, `by_name`, `zero_crossing` | doc::edit, doc::history, effects, diagnostics (silence, declick, declip), generate, snap, export, noise, rack, timepitch, view | present, wired | D1, D2, D3, D4, D5, D6, C3, E5, I3, N1 | actions.rs, e5_repeat_last.rs, clipboards.rs, clipboards_labels.rs, delete_silence.rs; 4 in-file. Note: `Editor::repeat_dialog` is a `todo!()` body with no caller; the test uses its own helper. |
| `crates/core/src/clipboard.rs` | The system clipboard side of Copy and Paste: turns a clip into a WAV file another program accepts and an audio file from the clipboard into a clip, with a size limit. | `write_clip_wav`, `wav_bytes`, `clip_from_file`, `is_audio_path`, `ClipboardError`, `MAX_FILE_BYTES` | doc::edit (Clip), io, save | present, wired | D1 | clipboards.rs, clipboards_labels.rs. Called from apps/wavcraft/src/oscb.rs. |
| `crates/core/src/diagnostics/silence.rs` | The silence scan by sample peak and Delete Silence, which shortens every quiet stretch to a kept length as one undo step, streamed in chunks. | `scan_silence`, `delete_silence`, `SilenceParams`, `Stretch`, `Deleted`, `SilenceError` | doc::edit, doc::DocState, store | present, wired | K4 (the dead-air fix tool); the command itself is not a v1 row | delete_silence.rs. Called from actions.rs (Editor) and apps/wavcraft/src/app.rs (DELETE_SILENCE verb). |
| `crates/core/src/doc/edit.rs` | The edits as pure functions from one DocState to the next: delete, cut, copy, paste, mix paste, silence, insert silence, crop, streamed replace of a range, channel masks and splice crossfades. | `Edit`, `Change`, `ChannelMask`, `Crossfades`, `Clip`, `replace_range` | doc (Channel, Piece, DocState), markers::Shift, store, soundcraft_audio_io | present, wired | D1, D3, D4, D7, C2, H4 | edits.rs, mix_paste.rs, clipboards.rs, testkit edits.rs; 2 in-file. Called through actions.rs, the app and crates/automation. |
| `crates/core/src/doc/history.rs` | The undo history: a list of document states with a cursor, per-step labels and changes, and a disk cap that drops the oldest steps and reports free-space refusals. | `History`, `Step`, `NoSpace`, `push_edit`, `undo`, `go_to` | doc::DocState, doc::edit, space, store::BlockId | present, wired | D5 | history_restore.rs, edits.rs, autosave.rs, testkit edits.rs. Called from apps/wavcraft/src/app.rs and crates/automation headless.rs. |
| `crates/core/src/fade_handles.rs` | Hit-testing the Fade In and Fade Out handles at the top corners of the active range and turning an inward drag into a fade range and curve. The app does not call it. | `FadeHandle`, `FadeGesture`, `handle_near`, `drag_fade`, `active_fade_range`, `HANDLE_HIT_PX` | dsp::fade, view (Selection, Viewport) | present, core only | E1 | e1_fade_handles.rs |
| `crates/core/src/jobs.rs` | The background job runner: a long range operation on a worker thread with progress, cancel, panic capture and a result the window thread polls and pushes as an undo step. | `Job`, `JobPoll`, `Job::spawn_replace`, `Job::spawn_with`, `Job::cancel`, `Job::poll` | doc, doc::edit; used by app.rs for effects and Convert Sample Type; control verb `JOB testslow` | present, wired | A4, E2, E3, G2 | jobs.rs |
| `crates/core/src/markers.rs` | Point and range markers as part of the document state: add, rename, move, delete, next and previous, shifting with edits, and reading and writing WAV cue and LIST/adtl chunks. | `Marker`, `Markers`, `Shift`, `Markers::moved_by`, `Markers::to_wav_chunks`, `Markers::from_wav_chunks` | std only; used by doc.rs, doc/edit.rs, io.rs, save, autosave.rs, export.rs, transport.rs, markers_panel.rs; control verb `MARKERS` | present, wired | A6, H1, H2, H3, H4 | 12 in-file tests; save.rs, snap.rs, 990_autosave_markers.rs, export.rs |
| `crates/core/src/snap.rs` | Moves a frame to the nearest snap target within 8 pixels: ruler major and minor ticks, listed frames, markers and zero crossings, each with its own switch. | `SnapSettings`, `snap_frame`, `snap_frame_steps`, `ruler_step`, `SNAP_PX`, control verbs `SNAP`, `SNAP_FRAME` | actions, doc | present, wired | C3 | crates/core/tests/snap.rs, settings.rs; 0 in-file |
| `crates/core/src/cut_edges.rs` | Moves each edge of a transcript cut from the word time to the quietest 2 ms within 20 ms, then onto a zero crossing within 5 ms, on a short mono window the caller reads. Ported from a peer (MIT, notice in the file). Pure arithmetic with no caller yet. | `Reach`, `Reach::at_rate`, `window`, `quiet_edge`, `quiet_ranges` | none | present, core only | L3, L4 | 12 in-file |
| `crates/core/src/transcript_cut.rs` | Turns word timings and the set of deleted words into the frame ranges to remove. Pure arithmetic with no caller outside its own tests. | `cut_ranges` | none | present, core only | L3 | 12 in-file |

### Save, export and convert (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `crates/core/src/atomic.rs` | Crash-safe file writing: write to a temporary file beside the target, sync it, then rename it into place. **Platform:** cfg(unix): the parent folder is synced after the rename; other systems skip that step. | `write_atomic` | std only | present, out of v1 | after 1.0 | atomic.rs. The only caller is CD export in crates/core/src/cd/mod.rs. Save, settings, presets, the app panels and crates/automation do not call it. |
| `crates/core/src/batch.rs` | Batch Process: runs a chain of operations over a list of files one at a time, streamed, and saves each result as a WAV in an output folder without touching the inputs. | `Op`, `Job`, `Outcome`, `Event`, `output_name`, `run` | io, save, doc::DocState, store | present, out of v1 | after 1.0 | batch.rs, batch_cli.rs. Only app caller is batch_panel.rs, behind v1 Feature::Batch (hidden in release). |
| `crates/core/src/batch_cli.rs` | A pure parser from a `--batch` argument list to a checked batch description and the Job it means. It opens no file and prints nothing. | `BatchCli`, `parse_batch_args`, `parse_bits`, `usage`, `to_job` | batch, save::SampleType | present, core only | after 1.0 | batch_cli.rs. No caller in the app or crates/automation. |
| `crates/core/src/convert.rs` | Convert Sample Type and Interpret Sample Rate as one undo step each: a streamed windowed-sinc resampler, channel count change, target bit depth with dither, a size estimate, and the convert preset file. | `convert`, `interpret_rate`, `ConvertSettings`, `Quality`, `estimate_bytes`, `PresetFile` | doc, doc::edit, dither, io, store, rayon, soundcraft_audio_io | present, wired | A4 | convert.rs, dither.rs; 3 in-file. Called from apps/wavcraft/src/convert_ui.rs and app.rs. |
| `crates/core/src/dither.rs` | Dither and noise shaping for bit-depth reduction: rectangular and triangular noise, error-feedback shaping, and a seeded per-channel quantizer. Off by default, so plain rounding stays bit exact. | `DitherSettings`, `DitherType`, `Shaping`, `Ditherer`, `DEFAULT_SEED` | save::quantize | present, wired | A4, A2 | dither.rs. Used by convert.rs, save.rs, doc.rs and apps/wavcraft/src/convert_ui.rs. |
| `crates/core/src/export.rs` | Batch writes of parts of a document: Extract Channels to Mono Files and Export Audio of Range Markers. Files are streamed, written to a temporary name first, and never replace an existing file. | `extract_channels`, `export_ranges`, `exportable_ranges`, `ExportOptions`, `ExportError`, `sanitize_name` | doc, markers, save (save_slice); called from app.rs and actions.rs; control verb `EXPORT_RANGES` | present, wired | after 1.0 | export.rs, 991_export_whole_range.rs; 3 in-file tests |
| `crates/core/src/export_chunks.rs` | Arithmetic for a streamed export: how many frames fit a memory budget and the list of frame ranges to mix in turn. Nothing calls it. | `chunk_frames_for_budget`, `chunk_bytes`, `chunk_ranges`, `chunk_count`, `ALIGN_FRAMES` | std only | present, core only | after 1.0 | 8 in-file tests |
| `crates/core/src/export_presets.rs` | Named export presets (file format, sample type, AIFF kind) kept as a tab-separated text file beside the settings file, replaced whole through a temporary file. **Platform:** none in file; the path comes from the config dir lookup | `ExportChoice`, `ExportPreset`, `ExportPresets`, `clean_name`, `ExportPresets::load`, `ExportPresets::save` | save (SaveFormat, SampleType, AiffKind); used by app.rs Save As; control verb `EXPORTPRESET` | present, wired | A2, A4 | export_options.rs |
| `crates/core/src/flac_enc.rs` | A streaming FLAC encoder: fixed 4096-frame blocks, independent channels, the cheapest of constant, fixed-predictor or verbatim subframes, STREAMINFO first, no seeking. It lets a save write block by block. | `Encoder`, `Encoder::new`, `Encoder::write_block`, `BLOCK_SIZE` | std io; private module used by save.rs (Save As FLAC) | present, wired | A2, I7 | save.rs (the f35 FLAC tests) |
| `crates/core/src/name_template.rs` | Expands an output-name template such as {name}-{n:3} into a safe file name, and sanitises file names. | `NameFields`, `TemplateError`, `expand`, `sanitize_file_name` | std only | present, core only | after 1.0 | name_template.rs (10 tests) |
| `crates/core/src/save.rs` | Streaming save of a document to WAV (RF64 above 4 GiB), AIFF, FLAC, Ogg Vorbis, Wave64, CAF and AU through an exclusive temporary file renamed over the target, plus sample types, dither, size estimates and an atomic small-file write. **Platform:** On Unix the temporary file can be made owner-only (mode 0600) and the folder is synced after the rename. On Windows the file inherits its folder's access list and there is no folder sync. | `save`, `save_as`, `SaveFormat`, `SampleType`, `SaveOptions`, `write_file_atomic`; control verbs `SAVE`, `EXPORT` | crate doc state, dither, random; used by apps/wavcraft/src/app.rs, convert_ui.rs, recovery.rs, batch_panel.rs and crates/automation/src/headless.rs | present, wired | A2, A4, A6, I7, N1 | save.rs (24 tests), save_part.rs, a4_dither.rs, dither.rs, vorbis.rs, chunks.rs, 987_save_format_parse.rs; testkit formats.rs |
| `crates/core/src/xmp.rs` | Reads the XMP packet of a WAV into a property model with its own strict XML parser, and edits one property by splicing bytes so everything else comes back unchanged. | `Xmp`, `Xmp::parse`, `Xmp::set`, `Xmp::to_bytes`, `FIELDS`, control verbs `XMP`, `METASET` | none (callers: meta, meta_view, io) | present, wired | after 1.0 | crates/core/tests/xmp.rs (through open, edit and save); 8 in-file |

### View, panels and workspace model (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/app/spectral.rs` | The spectral display's controls on the window: the waveform and spectral divider, View menu entries for resolution, window, range and frequency scale, the frequency band and ruler, and the SPECTRAL and STATUSBAR control verbs. | `control_spectral` (`SPECTRAL`), `control_statusbar` (`STATUSBAR`), `divider`, `Divider`, `draw_freq_ruler`, `set_axis` | app (WaveApp, SpectrumCache), theme; wavcraft_core freq_axis, settings, spectrogram, statusbar, actions, shortcuts; egui | present, wired | B2, C4 | No in-file tests. Core logic is covered by crates/core/tests/settings.rs and spectral_selection.rs; tools/alpha/f52.py, f65.py and f92.py use the verbs |
| `apps/wavcraft/src/spectrum.rs` | The spectral display's worker thread: it renders spectrogram pictures off the window thread, keeps the open file's overviews for fast far views, and hands back each finished picture while the window keeps drawing the old one. | `Renderer`, `Job`, `Picture`, `Renderer::ask`, `Renderer::take` | wavcraft_core::spectrogram (Overview, Columns), DocState, SpectrogramParams; theme; egui | present, wired | B2, B1 | 1 in-file test; crates/core/tests/incremental_overviews.rs for the core side |
| `apps/wavcraft/src/theme.rs` | The one theme: a dark and a light palette of named colour roles that every panel reads when it paints, helpers for user-chosen colours, and a test that fails on any other colour constructor in the UI sources. Also the named sizes (`Metrics`) and the Large controls switch (M4): with it on, `control_height` is 44 and `handle_grab` 22. | `Palette`, `DARK`, `LIGHT`, `set_dark`, `pal`, `rgb`, `Metrics`, `DEFAULT_METRICS`, `metrics_for`, `metrics`, `set_large` | egui::Color32; std | present, partly wired: `metrics()` is read by markers_panel.rs; nothing outside tests calls `set_large` yet (M4.3 wires it from `apply_look`) | M3, M4 | 6 in-file tests (one scans every UI source file) |
| `crates/core/examples/spectrogram_cost.rs` | A timing example for the spectrogram on a long file. It builds a mono 48 kHz noise file with a one second tone, builds the overview, then renders at 640, 1280 and 2048 columns with and without the overview and prints the times. | `main`, `timed` | wavcraft_core::spectrogram (Overview, Columns, render_columns, BLOCK_HOPS), SpectrogramParams, rayon | present, core only | B2 | none; run by hand with cargo run --release -p wavcraft-core --example spectrogram_cost [MINUTES] |
| `crates/core/i18n/en.txt` | The English text catalogue: one key = text line per interface string, with {0} and {1} filled in by the caller. It is the source every other language is checked against. | `menu.file` and the other `key = text` lines | compiled into crates/core/src/i18n.rs with include_str | present, wired |  | the in-file test in crates/core/src/i18n.rs that scans the sources for keys missing from this file |
| `crates/core/src/appearance.rs` | The colour model of the Appearance preference page: hex colour parsing, the five overridable colour items, spectrogram palette stops and named colour presets. | `Colors`, `Preset`, `ITEMS`, `parse_hex`, `format_hex`, `stops` | settings::SpecPalette | present, wired | M3 | appearance.rs. Called from apps/wavcraft/src/app.rs (Preferences) and settings.rs. |
| `crates/core/src/freq_axis.rs` | The spectral display's frequency axis: the shown band on a linear or log scale, scroll, zoom and pan, frequency to picture row, ruler ticks and Hz formatting. | `FreqAxis`, `Scale`, `Tick`, `zoom_at`, `ticks`, `format_hz` | spectrogram (SpectrogramParams); used by app.rs, app/spectral.rs, settings.rs, readout.rs | present, wired | B2 | 18 in-file tests; settings.rs |
| `crates/core/src/monitors.rs` | The monitor set for floating panels: a name and screen rectangle per monitor, parsing a monitor list, finding the monitor a window belongs to, and clamping a window back inside one. | `Monitor`, `parse`, `monitor_at`, `clamp_into`, `clean_name`, control verb `MONITORS` | std only; used by panels.rs and apps/wavcraft/src/app.rs | present, wired | I2 | 4 in-file tests |
| `crates/core/src/panels.rs` | The panel registry and which panels are shown, the docking layout (zones, tab order, sizes, floating windows), panel menus, metadata display fields, and the list of panels a release build does not offer. | `REGISTRY`, `PanelSet`, `Layout`, `OUT_OF_V1`, `PanelSet::for_build`; control verbs `PANELS`, `PANEL` | monitors; used by settings.rs, workspaces.rs and apps/wavcraft/src/app.rs, v1.rs | present, wired | I1, I2, M5 | 32 in-file tests; workspace_keys.rs, settings.rs |
| `crates/core/src/playlist.rs` | A playlist of marker ranges by marker id, each with a loop count, and the ordered frame ranges to play. It belongs to the open session and is not saved in the file. | `Playlist`, `Item`, `sequence`, `set_loops`, `MAX_LOOPS`, control verb `PLAYLIST` | crate markers; used by apps/wavcraft/src/playlist_panel.rs and app.rs | present, wired | after 1.0 | 5 in-file tests |
| `crates/core/src/readout.rs` | Works out what is under the pointer in the editor (pane, channel, amplitude in dBFS, frequency in Hz) and formats that readout and operation durations as text. | `EditorLayout`, `Pane`, `Readout`, `locate`, `duration_text`, control verb `HOVER` | spectrogram::SpectrogramParams, view (Viewport, format_time), freq_axis; used by apps/wavcraft/src/app.rs | present, wired | B1, B2 | 11 in-file tests |
| `crates/core/src/recent.rs` | The Open Recent list of the last 10 files in a text file in the config folder, plus the config file naming and atomic write shared with other small remembered lists. | `Recent`, `MAX_RECENT`, `config_file`, `write_atomic`, control verb `RECENT` | std, config folder lookup; used by apps/wavcraft/src/app.rs and files.rs | present, wired | A1, A5 | recent.rs (6 tests) |
| `crates/core/src/smpte.rs` | Converts between sample positions and SMPTE timecode at the fixed rates and a custom rate, with drop-frame labels, and snaps a sample to the nearest timecode frame. Integer maths only. | `SmpteRate`, `SmpteRate::format`, `SmpteRate::parse`, `SmpteRate::snap`, `SmpteRate::custom`, `frame_of_label` | none | present, wired | B5 | crates/core/tests/video_snap.rs; 14 in-file |
| `crates/core/src/spectral_dirty.rs` | Works out which blocks of a spectrogram overview an edit makes stale, with integer arithmetic that mirrors the overview grid. | `dirty_blocks` | none (called by spectrogram) | present, wired | B2 | 11 in-file; crates/core/tests/incremental_overviews.rs covers it through the overview |
| `crates/core/src/spectrogram.rs` | Renders the spectral frequency display for any time range and pixel size, and keeps a per-file overview of block maxima so zoomed-out views stay fast and refresh after edits. | `SpectrogramParams`, `Overview`, `Columns`, `render`, `render_columns_from`, `step_fft_size` | doc, freq_axis, settings, spectral_dirty, rayon, rustfft | present, wired | B2, C4, K2 | incremental_overviews.rs, episode_check.rs, issue1005_apply_to_spectrogram.rs, testkit wavcraft-bench; 20 in-file |
| `crates/core/src/statusbar.rs` | Lists the status bar's fields, which are shown, their settings-file form, and the sample format label and uncompressed size the bar prints. | `StatusField`, `StatusFields`, `sample_label`, `uncompressed_bytes`, `bytes_per_sample`, control verb `STATUSBAR` | soundcraft_audio_io | present, wired | I2 | crates/core/tests/settings.rs; 5 in-file |
| `crates/core/src/tempo.rs` | Computes tap tempo, and holds a document's tempo and time signature with formatting and parsing of bars, beats and ticks times. | `Tempo`, `TapTempo`, `Tempo::format`, `Tempo::parse`, `Tempo::parse_meter`, `BPM_RANGE` | soundcraft_time | present, wired | B5 | crates/core/tests/993_unit_at_bars_beats.rs, record_count_in.rs; 33 in-file |
| `crates/core/src/view.rs` | Owns zoom and scroll arithmetic, the navigator bar, amplitude zoom and scales, time and spectral selections with edge grabbing, and time formats with their formatting, parsing and tick steps. | `Viewport`, `Selection`, `TimeFormat`, `SpectralSelection`, `AmpZoom`, `ZoomPresets` | doc, smpte, tempo | present, wired | B1, B3, B5, C1, C4, M4 | spectral_selection.rs, e1_fade_handles.rs, 993_unit_at_bars_beats.rs; 55 in-file |
| `crates/core/src/workspaces.rs` | Keeps named snapshots of the panel layout as one text file each, with four built-in workspaces that are code and can be reset but never saved over or deleted. | `Workspaces`, `Snapshot`, `BUILTIN`, `DEFAULT`, `is_builtin`, control verbs `WORKSPACE`, `WORKSPACES` | panels | present, wired | I1, I2 | crates/core/tests/workspace_keys.rs; 7 in-file |

### Analysis, meters and Episode Check (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/freq_panel.rs` | The Frequency Analysis panel: level against frequency at the cursor, or the average over a selection scanned on a worker thread, with up to eight held snapshots, and the FREQ control verb. It never changes the document. | `FreqPanel`, `FreqPanel::show`, `MAX_HOLDS`, `control`, `FREQ` | wavcraft_core::analysis::freq, doc::edit::ChannelMask; theme; egui | present, out of v1 | after 1.0 | 3 in-file tests; crates/core/tests/freq_analysis.rs for the core side |
| `apps/wavcraft/src/meters_ui.rs` | The Levels panel (a horizontal left and right peak meter with hold and clip lights) and the Amplitude/Loudness Statistics window, whose measurement runs on a worker thread. | `MetersUi`, `MetersUi::panel`, `MetersUi::stats_window`, `MetersUi::update`, `readings_line`, `METERS` | wavcraft_core::levels, loudness; theme; egui | present, wired | B4, G4 | no in-file tests; crates/core/tests/loudness.rs, peak_modes.rs for the core side |
| `apps/wavcraft/src/statspanel.rs` | The Sample Statistics window: a cancellable worker-thread scan streamed from the document, per-channel peak, RMS, DC and clipping numbers, and buttons that move the cursor to the peak or the next clipped run. | `StatsPanel`, `Nav`, `Measured`, `StatsPanel::start`, `StatsPanel::ui` | wavcraft_core::stats, DocState; theme; egui | present, wired | G4 | no in-file tests; crates/core/tests/stats.rs, rms_settings.rs for the core side |
| `crates/core/src/analysis/freq.rs` | The spectrum of one moment or the averaged spectrum of a range, in dB per channel and as a channel mean, read in chunks with a cancel flag and peak interpolation. | `spectrum_at`, `scan`, `scan_with_progress`, `Spectrum`, `FftWindow`, `Peak` | doc::DocState, doc::edit::ChannelMask, store, rustfft | present, out of v1 | after 1.0 | freq_analysis.rs. Only caller is apps/wavcraft/src/freq_panel.rs, and the freq panel is in panels.rs OUT_OF_V1. |
| `crates/core/src/analysis/mod.rs` | The module root for read-only analysis; it only declares the freq submodule. | `freq` |  | present, out of v1 | after 1.0 | none of its own |
| `crates/core/src/check/drift.rs` | The loudness drift detector: stretches where short-term loudness stays too far from the file's integrated loudness for a set time. | `drift_pass` (visible to the check module only; called by `scan` in check/mod.rs) | check (Config, Finding, Sink), loudness::BlockMeter, spectrogram::Samples | present, core only | K2 | episode_check.rs |
| `crates/core/src/check/hum.rs` | The mains hum detector: steady narrow peaks at 50 or 60 Hz and their harmonics found in long STFT windows. | `hum_pass` (visible to the check module only; called by `scan` in check/mod.rs) | check, stft::Stft, spectrogram::Samples, rustfft | present, core only | K2 | episode_check.rs |
| `crates/core/src/check/mod.rs` | The Episode Check detectors and their report: clipped runs, true peak over a ceiling, DC offset and dead air here, with drift, hum and plosives in submodules. Scans are chunked and change nothing. | `scan`, `Config`, `Finding`, `Report`, `Kind`, `Severity` | spectrogram::Samples, stats (CLIP_LEVEL, MIN_CLIP_RUN), loudness, stft | present, core only | K2 (K1, K3, K4, K5 have no caller yet) | episode_check.rs. No caller in apps/wavcraft/src or crates/automation. |
| `crates/core/src/check/plosive.rs` | The plosive detector: short bursts of low-frequency energy that stand well above the rest of the spectrum and the nearby low band. | `plosive_pass` (visible to the check module only; called by `scan` in check/mod.rs) | check, spectrogram::Samples | present, core only | K2 | episode_check.rs |
| `crates/core/src/dsp/speech_ranges.rs` | Splits a series of frame levels in dB into speech ranges, with a threshold and minimum speech and silence lengths. | `speech_ranges`, `SegmentError` | std only | present, core only | after 1.0 | 7 in-file tests |
| `crates/core/src/levels.rs` | Playback peak meters: a lock-free tap the audio callback writes, and the window-side meter with dBFS readings, peak hold modes, valleys and a clip light. | `LevelTap`, `Levels`, `Reading`, `PeakMode`, `HOLD_SECS`, `FLOOR_DB` | soundcraft_dsp meter (PeakMeter); used by player.rs, meters_ui.rs, input_meter.rs | present, wired | B4, J4 | 3 in-file tests; peak_modes.rs |
| `crates/core/src/levels/rms.rs` | A sliding-window RMS reading in dBFS per channel (300 ms default), kept as a ring of squares. The app's meters do not call it. | `RmsMeter`, `RmsLevels`, `RmsReading`, `mean_square_to_db`, `window_samples`, `RMS_WINDOW_MS` | levels (FLOOR_DB, METER_RATE) | present, core only | B4 | rms_reading.rs |
| `crates/core/src/loudness.rs` | Offline loudness of a range, read in chunks: integrated loudness, loudness range, true peak and sample peak. Also Match Volume, which the app does not call. | `Loudness`, `measure`, `range_of`, `match_gain_db`, `match_volume`, `MatchVolume` | soundcraft_dsp meter (LoudnessMeter), doc, doc::edit; used by match_loudness.rs, check/drift.rs, match_ui.rs, meters_ui.rs, app.rs; control verb `LOUDNESS` | present, wired | G3, G4, K2, K5 | loudness.rs, match_volume.rs, match_loudness.rs |
| `crates/core/src/match_loudness.rs` | Match Loudness as one undoable edit: gain to a target integrated loudness, with the true peak held under a ceiling by less gain or by a look-ahead true-peak limiter, repeated until it settles. | `Target`, `Plan`, `plan`, `run`, `apply`, `MatchResult` | loudness, doc, doc::edit replace_range; used by match_ui.rs, app.rs; control verb `MATCH` | present, wired | G3, K5 | match_loudness.rs; 7 in-file tests |
| `crates/core/src/meters/levels_scale.rs` | The Levels meter scale: a 60 dB range ending at 0 dBFS and colour zones crossing at -18 and -6 dBFS. The app's meter drawing keeps its own constants and does not call it. | `RANGE_DB`, `YELLOW_FROM_DB`, `RED_FROM_DB`, `Zone`, `bar_fraction`, `zone` | std only | present, core only | B4 | b4_levels_scale.rs |
| `crates/core/src/meters/mod.rs` | Declares the meter helper modules: levels_scale, phase, phase_bins and phase_scope. No app code imports from it. | `meters::levels_scale`, `meters::phase`, `meters::phase_bins`, `meters::phase_scope` | its child modules | present, core only | B4 | b4_levels_scale.rs, phase_scope.rs |
| `crates/core/src/meters/phase.rs` | Stereo phase correlation of two buffers, whole or per window with a hop. There is no running meter and no caller. | `phase_correlation`, `phase_correlation_windowed` | std only | present, core only | after 1.0 | 16 in-file tests |
| `crates/core/src/meters/phase_bins.rs` | The numbers behind a spectral phase display: the left/right phase difference per frequency bin, its mapping onto 0..1 for a colour ramp, and the share of bins in phase. It draws nothing. | `phase_difference`, `phase_to_unit`, `phase_row`, `in_phase_fraction` | std only | present, core only | after 1.0 | 10 in-file tests |
| `crates/core/src/meters/phase_scope.rs` | The data for a goniometer view: thinned (x, y) points per sample pair, left/right balance, and mid and side RMS levels in dB with a floor. | `scope_points`, `balance`, `mid_side_rms_db`, `FLOOR_DB` | std only | present, core only | after 1.0 | phase_scope.rs (30 tests) |
| `crates/core/src/meters/rms_histogram.rs` | Counts RMS levels in dB into equal-width bins over a range, dropping levels outside it and refusing invalid or oversized bin requests. | `rms_level_histogram` | std only | present, core only | after 1.0 | 12 in-file tests |
| `crates/core/src/stats.rs` | Scans a range in chunks for amplitude statistics per channel: peak and its place, min and max sample, clipped runs, DC offset, windowed RMS, dynamic range and used bit depth. | `scan`, `scan_with`, `Stats`, `ChannelStats`, `ChannelScan` | doc | present, wired | G4 | crates/core/tests/stats.rs, rms_settings.rs; 12 in-file |

### Effects and DSP (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/rack_panel.rs` | The Effects Rack window: slots with effect, power and settings, master power, input and output gain, Mix, rack and effect presets, Preview and Apply. It edits a rack config; the app pushes it to the player and runs Apply. | `RackPanel`, `Request`, `RackPanel::show`, `RackPanel::command`, `RackPanel::preset`, `slot_effects`, `slot_plugins`, `RACK` / `PRESET` / `VST3` | wavcraft_core::rack, effects, presets, vst3; effect_dialog; v1 (a release build lists no out-of-v1 effect, no VST3 plugin and no Scan VST3 button); egui | present, wired | F1, F2, F3, M5 | 2 in-file tests (M5); crates/core/tests/rack.rs, rack_reset.rs, rack_tail.rs for the core side |
| `crates/core/src/diagnostics/declick.rs` | The DeClicker and Automatic Click Remover: a second-difference click detector with a robust noise scale, and a repair that redraws each click from the audio around it as one undo step. | `scan`, `declick`, `auto_click_remove`, `Params`, `AutoParams`, `Scan` | diagnostics (Region), diagnostics::declip::Repair, doc::edit, store | present, out of v1 | after 1.0 | declick.rs, diagnostics_panel.rs. Reached by control verb DECLICK, actions Repeat and diagnostics_panel.rs; menu entries sit behind v1 Feature::Diagnostics. |
| `crates/core/src/diagnostics/declip.rs` | The DeClipper: finds runs flattened against a threshold level and redraws each one as a fitted cubic through the good samples either side, as one undo step. | `scan`, `declip`, `Params`, `Scan`, `Repair`, `LABEL` | diagnostics (Region), doc::edit, doc::DocState, store | present, out of v1 | after 1.0 | declip.rs, diagnostics_panel.rs. Reached by control verb DECLIP, actions Repeat and diagnostics_panel.rs; menu entries sit behind v1 Feature::Diagnostics. |
| `crates/core/src/diagnostics/mod.rs` | The shared Diagnostics layer: the Region type, the tool enum with scan and repair entry points, and repairing a chosen subset of found regions. | `Region`, `Found`, `Subset`, `Tool`, `Tool::scan`, `Tool::repair` | doc::edit (replace_range, Edit), doc::DocState, declick, declip, silence | present, out of v1 | after 1.0 | diagnostics_panel.rs, declip.rs, declick.rs, delete_silence.rs. Called from apps/wavcraft/src/diagnostics_panel.rs and app.rs (control verb DIAG); the window is hidden in release. |
| `crates/core/src/dsp/band_only.rs` | A band filter for playing only a spectral selection: a Butterworth highpass at the low edge followed by a lowpass at the high edge. | `BandOnly`, `band_filter`, `design_band`, `validate_band`, `BandError` | dsp::sci_filter | present, core only | C4 (not connected) | band_only.rs. No caller outside its tests. |
| `crates/core/src/dsp/bass_enhancer.rs` | A one-pole bass exciter: splits off the low band, saturates it and adds it back. | `enhance` | std only | present, core only | after 1.0 | 5 in-file. No caller outside the file. |
| `crates/core/src/dsp/chorus.rs` | A bounded mono chorus made from several sine-modulated delay voices. | `chorus`, `MIN_DELAY_MS`, `MAX_DELAY_MS` | std only | present, core only | after 1.0 | 17 in-file. No caller outside the file. |
| `crates/core/src/dsp/convolution.rs` | FFT-based convolution of mono samples and a convolution reverb built on it. It loads no impulse response file. | `convolve`, `convolution_reverb` | rustfft | present, core only | after 1.0 | 19 in-file. No caller outside the file. |
| `crates/core/src/dsp/distortion.rs` | A distortion processor with several clipping modes and a one-pole tone lowpass. | `distort`, `Mode`, `ToneLp` | std only | present, core only | after 1.0 | 11 in-file. No caller outside the file. |
| `crates/core/src/dsp/doppler.rs` | The path model for a Doppler shifter: per-sample distance, propagation delay and distance gain of a source moving in a straight line past a listener. It touches no audio. | `Path`, `distances`, `delays_samples`, `gains`, `SPEED_OF_SOUND` | std only | present, core only | after 1.0 | 11 in-file. No caller outside the file. |
| `crates/core/src/dsp/echo.rs` | A feedback echo for mono samples, a variant that renders the decaying tail, and a stereo interleaved form with per-channel settings. | `echo`, `echo_with_tail`, `echo_stereo_interleaved`, `EchoChannel`, `ms_to_samples`, `MAX_FEEDBACK` | std only | present, core only | after 1.0 | 34 in-file. No caller outside the file. |
| `crates/core/src/dsp/envelope.rs` | Gain envelopes from breakpoints: a piecewise-linear fade envelope with input sanitising, and a gain envelope with a choice of interpolation. | `piecewise_linear`, `gain_envelope`, `Interp`, `GainError` | std only | present, core only | after 1.0 | 27 in-file. No caller outside the file. |
| `crates/core/src/dsp/eq_graph.rs` | Screen and value mapping for a parametric EQ graph: log frequency to x, gain to y, node dragging and nearest-node hit testing. | `GraphView`, `x_of_freq`, `freq_at_x`, `y_of_gain`, `node_drag`, `nearest_node` | std only | present, core only | F2 (not connected) | eq_graph.rs. No caller outside its tests. |
| `crates/core/src/dsp/fade.rs` | The fade gain formulas (linear, logarithmic, cosine) and the pointer-drag rule that picks a curve. | `FadeCurve`, `curve_from_drag`, `fade_in_gain`, `fade_out_gain`, `DRAG_LOG_THRESHOLD_PX` | std only | present, wired | E1 | e1_fade_curve.rs, e1_fade_effect.rs, e1_fade_handles.rs. Used by effects/fade.rs (FadeIn and FadeOut in the effect registry) and fade_handles.rs; fade_handles.rs itself has no app caller. |
| `crates/core/src/dsp/fade_curve.rs` | Four fade curve shapes (linear, logarithmic, cosine, exponential), in-place fade in and fade out over a buffer, and picking a curve from a drag point. | `CurveKind`, `fade_gain`, `fade_in`, `fade_out`, `curve_from_drag` | std only | present, core only | E1 | fade_curve.rs |
| `crates/core/src/dsp/flanger.rs` | A mono flanger: a short sine-modulated delay line with feedback and a dry/wet mix. No effect in the registry uses it. | `flanger`, `MIN_DELAY_MS`, `MAX_DELAY_MS` | std only | present, core only | after 1.0 | 13 in-file tests |
| `crates/core/src/dsp/midside.rs` | Left/right to mid/side conversion and back, per sample and in place on two slices. | `lr_to_ms`, `ms_to_lr`, `lr_to_ms_in_place`, `ms_to_lr_in_place` | std only; used by dsp/stereo_width.rs | present, core only | after 1.0 | 4 in-file tests |
| `crates/core/src/dsp/mod.rs` | Declares the dsp helper modules (band_only, bass_enhancer, chorus, convolution, distortion, doppler, echo, envelope, eq_graph, fade, fade_curve, flanger, midside, pan, phaser, sci_filter, speech_ranges, stereo_width, varispeed). | `dsp::fade`, `dsp::fade_curve`, `dsp::sci_filter`, `dsp::stereo_width` | none; only dsp::fade has callers outside dsp (effects/fade.rs, fade_handles.rs) | present, wired | E1 | none in file; children tested in e1_fade_curve.rs, fade_curve.rs, band_only.rs, eq_graph.rs, stereo_width.rs |
| `crates/core/src/dsp/pan.rs` | One function that turns a pan position into left and right gains. The app's mixer uses the pan law of soundcraft-dsp instead, so nothing calls this. | `pan_gains` | std only | present, core only | after 1.0 | 9 in-file tests |
| `crates/core/src/dsp/phaser.rs` | A mono phaser: cascaded all-pass stages swept by a sine LFO, with feedback and a dry/wet mix. No effect in the registry uses it. | `phaser` | std only | present, core only | after 1.0 | 18 in-file tests |
| `crates/core/src/dsp/sci_filter.rs` | Butterworth, Chebyshev I and II and Bessel low-pass and high-pass designs as biquad cascades, a function to run them, and their magnitude response. | `FilterFamily`, `ResponseType`, `BiquadSection`, `design_filter`, `process_filter`, `magnitude_response_db` | std only; used by dsp/band_only.rs | present, core only | after 1.0 | 28 in-file tests |
| `crates/core/src/dsp/stereo_width.rs` | Stereo width: scales the side signal of a left/right pair (0 is mono, 1 unchanged, up to 4) and measures the width of a pair. | `widen`, `widen_interleaved`, `measure_width`, `MAX_WIDTH` | dsp::midside | present, core only | after 1.0 | stereo_width.rs |
| `crates/core/src/dsp/varispeed.rs` | A linear-interpolation resampler for a speed change that also changes pitch and length. | `resample_linear` | std only | present, core only | after 1.0 | 5 in-file tests |
| `crates/core/src/effects.rs` | The effect seam: the Effect, Run and EffectStream traits, parameter descriptions with parsing and validation, the registry of 27 effects in menu order, the effects a release build leaves out, Invert, and apply, which streams an effect over a range as one undo step. | `Effect`, `ParamDesc`, `REGISTRY`, `find`, `apply`, `open_stream`, `OUT_OF_V1`, `listed` | doc, doc::edit; callers: actions.rs, rack.rs, app.rs Effects menu, effect_dialog.rs, rack_panel.rs, automation headless.rs; control verbs `EFFECT`, `EFFECT_DIALOG` | present, wired | E1, E2, E3, E4, E5, F1, F2, G2, M5, N1 | 1 in-file test; effects.rs, effect_params.rs, effect_run.rs, e5_repeat_last.rs, m5_effects_listed.rs |
| `crates/core/src/effects/amplify.rs` | The Amplify effect: gain in dB per channel from -80 to +40, default 0, with linked sliders on by default, and the gain arithmetic behind it. | `Amplify`, `amplify_gains`, `db_to_linear`, `MIN_GAIN_DB`, `MAX_GAIN_DB`, `DEFAULT_GAIN_DB` | effects seam; in the registry, so Effects menu and `EFFECT amplify` | present, wired | E2 | amplify.rs; 5 in-file tests |
| `crates/core/src/effects/card129_tests.rs` | Test-only fixtures for the effect preview tests: a small document on a temporary store, a sample reader, and two test effects. Compiled only under cfg(test). | `Fixture`, `samples`, `Frames`, `Accumulator` | doc, store, effects seam | present, core only | E6 | used by the 5 in-file tests of effects/preview.rs |
| `crates/core/src/effects/channel_mixer.rs` | The Channel Mixer effect: each output channel is a weighted sum of the left and right inputs with an optional polarity flip. Fewer than two active channels pass through unchanged. | `ChannelMixer` | effects seam; in the registry, so Effects menu and `EFFECT` | present, wired | after 1.0 | channel_mixer.rs |
| `crates/core/src/effects/compare.rs` | A/B compare state for one effect: two sets of settings, which one is active, copy one onto the other, and apply the active set. No dialog uses it yet. | `AbCompare`, `Side`, `toggle`, `set_params`, `copy_to_other`, `apply_active` | effects seam, doc::edit | present, core only | after 1.0 | ab_compare.rs |
| `crates/core/src/effects/deesser.rs` | The DeEsser effect, with a multiband mode (a dynamic shelf above the frequency) and a broadband mode. It keeps filter and envelope state across chunks and also offers a live stream for the rack. | `DeEsser` | soundcraft_dsp biquad and gain helpers, effects seam; in the registry | present, wired | F2 | deesser.rs, rack_reset.rs |
| `crates/core/src/effects/dehummer.rs` | The DeHummer effect: narrow peaking cuts at 50 or 60 Hz and its harmonics, per channel, with filter state carried from chunk to chunk. | `DeHummer` | effects seam; in the registry, listed in the Effects menu in every build | present, wired | after 1.0 | dehummer.rs |
| `crates/core/src/effects/delay.rs` | The Delay effect: a feedback delay wrapped from soundcraft-dsp. Echoes past the end of the selection are cut. | `Delay` | effects/dsp_run.rs, effects seam; in the registry | present, wired | after 1.0 | effect_reverb_delay.rs, effect_params.rs |
| `crates/core/src/effects/dereverb.rs` | The DeReverb effect: estimates the late room tail per STFT frame and bin from earlier power and an assumed reverb time, and turns that share down. | `DeReverb` | stft, doc, rustfft, effects seam; in the registry, listed in the Effects menu in every build | present, wired | after 1.0 | dereverb.rs |
| `crates/core/src/effects/dsp_run.rs` | Shared plumbing for effects that wrap a soundcraft-dsp plugin: one prepared plugin per run, fed the chunks in order so its memory carries across chunks. | `PluginRun`, `PluginRun::new` | soundcraft_dsp Plugin, effects seam; used by delay.rs, reverb.rs, plate.rs | present, wired | after 1.0 | effect_reverb_delay.rs, effect_plate.rs |
| `crates/core/src/effects/dynamics.rs` | The Dynamics effect: a gate/expander, a soft-knee compressor and a limiter in one stateful effect, plus the static transfer curve in dB. | `Dynamics`, `static_curve` | effects seam; in the registry | present, wired | F2 | dynamics.rs |
| `crates/core/src/effects/dynamics_graph.rs` | Sampled points for a Dynamics transfer-curve graph: the curve, the gain reduction and the unity line. No drawing, and no dialog calls it yet. | `curve_points`, `reduction_points`, `unity_line` | effects/dynamics.rs | present, core only | after 1.0 | 8 in-file tests |
| `crates/core/src/effects/eq.rs` | Five filter effects: Parametric EQ, High-Pass, Low-Pass, Notch and DC Offset removal. Each keeps every channel's biquad memory across chunks. | `ParametricEq`, `HighPass`, `LowPass`, `Notch`, `DcOffset` | soundcraft_dsp biquad, effects seam; in the registry; also used by strip_eq.rs | present, wired | F2 | filters.rs, e5_repeat_last.rs |
| `crates/core/src/effects/fade.rs` | The Fade In and Fade Out registry effects with a curve choice (linear, logarithmic, cosine), streamed in chunks over the selection or the whole file. | `FadeIn`, `FadeOut`, `process_fade_chunk`, `curve_from_param`, `CURVE_NAMES`, `CURVE_PARAMS` | dsp::fade, effects seam; in the registry, so Effects menu and `EFFECT` | present, wired | E1 | e1_fade_effect.rs |
| `crates/core/src/effects/fft_filter.rs` | The FFT Filter effect: a frequency response of up to eight frequency and gain points, interpolated on a log or linear scale and applied to each STFT frame. | `FftFilter` | stft, doc, effects seam; in the registry | present, wired | after 1.0 | fft_filter.rs |
| `crates/core/src/effects/gain.rs` | The Gain effect: scale the level by a number of decibels. It is the simplest effect with a setting. | `Gain` | effects seam; in the registry | present, wired | F2 | effect_params.rs, rack.rs, ab_compare.rs, e5_repeat_last.rs |
| `crates/core/src/effects/graphic_eq.rs` | The Graphic EQ effect with 10, 20 or 30 fixed bands and a master gain. Filter gains are fitted so the combined response hits the slider values at the band centres. | `GraphicEq`, `CENTRES_10`, `CENTRES_20`, `CENTRES_30` | effects seam; in the registry | present, wired | after 1.0 | graphic_eq.rs |
| `crates/core/src/effects/noise_reduction.rs` | The Noise Reduction (process) effect: a per-bin gain mask against the captured noise print, with frequency and time smoothing, run on the shared STFT engine. | `NoiseReduction` | noise (NoisePrint), stft, doc, effects seam; in the registry | present, wired | G2 | noise.rs |
| `crates/core/src/effects/normalize.rs` | The Normalize effect: peak to a target in percent or dB, one gain for all channels or one each, optional DC bias removal. It measures the range in one streamed pass first. | `Normalize`, `normalize_gains`, `target_peak` | effects seam; in the registry | present, wired | E3 | normalize.rs; 5 in-file tests |
| `crates/core/src/effects/oneshot.rs` | One-shot edits on a range as pure DocState to Edit functions: reverse, amplify, normalize, fade, plus peak, headroom and a clip-guarded amplify. The action table calls them for effect.reverse and the fixed fade actions. | `reverse`, `amplify`, `normalize`, `fade`, `amplify_guarded`, `FadeCurve` | soundcraft_dsp offline, doc::edit replace_range, effects/reverse.rs; called from actions.rs | present, wired | E1, E4 | e1_fade_effect.rs, reverse.rs, clip_guard.rs |
| `crates/core/src/effects/phase_correction.rs` | The Automatic Phase Correction effect: measures the lag between the two channels by cross-correlation, once for the range or per window, and delays the early channel. | `PhaseCorrection`, `measure_lag` | rustfft, doc, effects seam; in the registry | present, wired | after 1.0 | phase_correction.rs |
| `crates/core/src/effects/pitch_presets.rs` | A table of named pitch shifts (octave, fifth and so on) and their conversion to Pitch Shifter settings, checked against its ranges. No dialog lists them yet. | `PitchPreset`, `presets`, `names`, `lookup`, `params_for`, `check_shift` | effects (PitchShift) | present, core only | after 1.0 | pitch_presets.rs |
| `crates/core/src/effects/pitch_saturate.rs` | The Pitch Shifter and Saturator effects, wrapping the realtime plugins of soundcraft-dsp so plugin state carries across chunks. | `PitchShift`, `Saturator` | soundcraft_dsp plugins (pitch, harmonic), doc, effects seam; in the registry | present, wired | after 1.0 | pitch_saturate.rs |
| `crates/core/src/effects/plate.rs` | The Plate effect: plate reverb wrapped from soundcraft-dsp. The tail past the end of the selection is cut. | `Plate` | effects/dsp_run.rs, effects seam; in the registry | present, wired | after 1.0 | effect_plate.rs |
| `crates/core/src/effects/preview.rs` | A bounded, read-only looped render of a selected range through an effect for a dialog preview. It writes nothing to the store. The app does not call it; its dialog preview covers generators only. | `Preview`, `Preview::new`, `Preview::render`, `Preview::set_params` | effects seam, doc, doc::edit ChannelMask, store BLOCK_FRAMES | present, core only | E6 | 5 in-file tests (fixtures in effects/card129_tests.rs) |
| `crates/core/src/effects/reverb.rs` | The Reverb effect: algorithmic room reverb wrapped from soundcraft-dsp. The tail past the end of the selection is cut. | `Reverb` | effects/dsp_run.rs, effects seam; in the registry | present, wired | after 1.0 | effect_reverb_delay.rs, effect_plate.rs |
| `crates/core/src/effects/reverse.rs` | Pure helpers behind Reverse: which source frames supply an output chunk, and a bit-exact slice reversal. | `mirror_range`, `reverse_frames` | std only; used by effects/oneshot.rs, reached by action `effect.reverse` | present, wired | E4 | 5 in-file tests; reverse.rs |
| `crates/core/src/effects/speech_leveler.rs` | The Speech Volume Leveler effect: a linked level follower with a 400 ms window that pulls talkers toward a target, with cut and boost limits and attack and release times. | `SpeechLeveler` | effects seam; in the registry | present, wired | after 1.0 | speech_leveler.rs |
| `crates/core/src/effects/time_shift.rs` | The Time Shift effect: delays the masked channels inside the range by up to 10 s, keeping the length, with a ring buffer per channel carried across chunks. | `TimeShift`, `MAX_SHIFT_MS` | effects seam; in the registry | present, wired | after 1.0 | time_shift.rs |
| `crates/core/src/generate/dtmf.rs` | DTMF tone generation: the frequency pair for a key, one tone, and a key sequence with gaps. It is not in the generator registry. | `key_frequencies`, `tone`, `sequence` | std only | present, core only | after 1.0 | 6 in-file tests |
| `crates/core/src/generate/mod.rs` | Declares the dtmf and tones_noise modules and re-exports everything from tones_noise as the generate API. | `generate::dtmf`, `generate::tones_noise` | its two child modules | present, wired | after 1.0 | generate.rs |
| `crates/core/src/generate/tones_noise.rs` | The Generator seam and its two generators, Tones and Noise (white, pink, brown), with settings described like effect settings, streamed sources per channel, and a short render for the dialog preview. | `Generator`, `Source`, `Tones`, `Noise`, `render`, `find` | effects (ParamDesc); used by actions.rs, app.rs, effect_dialog.rs; control verbs `GENERATE`, `GENERATE_DIALOG` | present, wired | after 1.0 | generate.rs; 8 in-file tests |
| `crates/core/src/mix/band_plan.rs` | Plans a frequency band splitter: 2 to 8 bands with crossovers spaced evenly on a logarithmic axis, each band's range, and which band a frequency falls in. No filtering happens here. | `Band`, `crossover_points`, `band_plan`, `band_index`, `MIN_BANDS`, `MAX_BANDS` | std only | present, core only | after 1.0 | 10 in-file tests |
| `crates/core/src/noise.rs` | Noise prints: the average noise magnitude spectrum of a selection per channel, stored as a density independent of FFT size, with one current print held in memory for Noise Reduction. | `NoisePrint`, `capture`, `set_current`, `current`, `clear` | crate doc state, FFT; called from actions.rs, effects/noise_reduction.rs and apps/wavcraft/src/app.rs | present, wired | G1, G2 | noise.rs (8 tests); e5_repeat_last.rs |
| `crates/core/src/presets.rs` | Named presets for one effect or the whole Effects Rack, saved as small key=value text files under the config folder, with save, load, delete and list. | `Presets`, `save_effect`, `load_effect`, `save_rack`, `load_rack`, `RACK` | rack, recent config folder helpers; used by apps/wavcraft/src/rack_panel.rs | present, wired | F3 | 3 in-file tests |
| `crates/core/src/rack.rs` | The Effects Rack: slot configuration with power switches, input and output gain and dry/wet mix, an allocation-free block processor for the audio callback, and applying the rack to a document range as one undoable edit. | `RackConfig`, `SlotConfig`, `RackProcessor`, `apply`, `apply_with_tail`, `MAX_BLOCK` | crate effects, doc edit; used by apps/wavcraft/src/rack_panel.rs, player.rs, app.rs | present, wired | F1, F2, F3 | 6 in-file tests; rack.rs, rack_tail.rs, rack_reset.rs |
| `crates/core/src/resample.rs` | A stateless windowed-sinc (Kaiser) sample-rate converter for playback that returns the signal at any fractional frame, with quality levels and an exact identity path when rates match. | `Resampler`, `Quality`, `sample_with`, `render`, `doc_position`, `out_frames` | std only; used by apps/wavcraft/src/player.rs, settings.rs, convert.rs | present, wired | C5 | 7 in-file tests; convert.rs, settings.rs |
| `crates/core/src/stft.rs` | Provides the shared STFT analysis and overlap-add resynthesis engine on a frame grid fixed to the document, so chunked processing gives the same output as one pass. | `Stft`, `Placement`, `Stft::analyse`, `Stft::process`, `read_padded`, `MAX_FFT` | doc, effects, rustfft (callers: noise, check/hum) | present, wired | G2, K2 | crates/core/tests/noise.rs; 0 in-file |
| `crates/core/src/timepitch.rs` | Stretches and pitch-shifts a selection held whole in memory, capped at ten minutes a channel, and detects the pitch of a selection. | `stretch_pitch`, `detect_pitch`, `MAX_FRAMES`, `LABEL`, `fit` | doc, soundcraft_dsp | present, wired | after 1.0 | crates/core/tests/timepitch.rs, stretch_fit.rs; 0 in-file |
| `crates/core/src/timepitch/fit.rs` | Works out the stretch ratio that fits a selection to a target length, checks it against the supported 0.1 to 10 range, and applies it to a sample buffer. | `fit_ratio`, `stretch_to_duration`, `target_frames`, `Target`, `FitError`, `SUPPORTED` | doc, store, timepitch | present, core only | after 1.0 | crates/core/tests/stretch_fit.rs; 0 in-file |
| `crates/core/src/vst3.rs` | Hosts VST3 plugins in the Effects Rack: finds bundles, scans each in a separate worker process with a timeout, keeps the found list, and opens a plugin as a rack stream. It is not hidden in release builds. **Platform:** One cfg site: a test module gated `#[cfg(all(test, unix))]`. `default_dirs` passes straight through to the vst3-host crate's default search paths, which is where the per-OS folders are chosen. | `scan_with`, `ScanReport`, `Found`, `worker_lines`, `open_stream`, control verb `VST3` | effects, rack, soundcraft_dsp, vst3-host | present, out of v1 | after 1.0 | 10 in-file |

### Recording (`crates/core`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/rec_input.rs` | Opening the real input device for a take: the device named in the preferences or the system default. Its callback converts to f32 and pushes to the ring without blocking; the caller starts the stream. | `open`, `Opened`, `Lost`, `monitor_frames` | cpal; wavcraft_core::record (Ring, RecFormat, MonitorRing); input_monitor::find_input | present, wired | J1, J5 | 1 in-file test; crates/core/tests/j5_input_device.rs for the core side |
| `apps/wavcraft/src/recording.rs` | Recording a take in the window: the dialog, start options, timed and armed starts, pause, punch into an open document, the live input (real or fake), monitoring, and the status readout with level, time, size and dropped samples. | `Recording`, `StartOpts`, `parse_start_args`, `record_dir`, `Recording::start_with`, `RECORD` | wavcraft_core::record, doc::rec_edit, space; rec_input; egui | present, wired | J1, J3, J4, J5, J7 | 7 in-file tests; crates/core/tests/rec_edit.rs and the in-file tests of crates/core/src/record.rs for the core side |
| `crates/core/src/doc/rec_edit.rs` | Turning a finished take into one edit on an open document: record at the cursor, over the selection or punch-in, target channel choice, length limits and the latency offset. | `RecMode`, `RecTarget`, `RecPlan`, `apply_take`, `apply_take_with_offset`, `offset_frames` | doc::edit (paste, Edit), doc::DocState | present, wired | J1, J7 (punch-in and record-over go beyond the v1 rows) | rec_edit.rs. Called from apps/wavcraft/src/recording.rs and app.rs. |
| `crates/core/src/input_meter.rs` | Input metering: a lock-free tap the input callback feeds, per-channel peaks for the Levels panel, run totals with a verdict for the mic check, and a fake input for tests. | `InputTap`, `InputStats`, `Verdict`, `FakeInput`, `to_db`, `CLIP_LEVEL` | levels (LevelTap); used by input_monitor.rs, recording.rs, app.rs, record.rs; control verbs `MICCHECK`, `ACTION meter.input.toggle` | present, wired | J4 | 6 in-file tests |
| `crates/core/src/metronome.rs` | Metronome click timing from the document tempo (beat frames, accents, clicks in a range) and the synthesised click sounds the player mixes into playback only. | `ClickTrack`, `Click`, `click_sound`, `clicks_between`, `bars_frames`, `first_beat_at_or_after` | crate tempo; called by apps/wavcraft/src/player.rs; control verb METRONOME and the View toolbar checkbox | present, wired | after 1.0 | 6 in-file tests; record_count_in.rs |
| `crates/core/src/metronome/record_count_in.rs` | Plans a count-in before a recording: the pre-roll length, the frame the recording really starts at, and the clicks to sound during the pre-roll. It records and plays nothing. | `RecordCountIn`, `RecordPlan`, `CountInError`, `plan_record_count_in`, `pre_roll_frames`, `MAX_COUNT_IN_BARS` | metronome `ClickTrack` | present, core only | after 1.0 | record_count_in.rs (20 tests) |
| `crates/core/src/record.rs` | The audio side of recording a take: a non-blocking input ring that counts drops, a writer thread, a WAV take writer that keeps the header valid on disk, format and input channel choice, and a fake input for tests. | `Ring`, `Recorder`, `TakeWriter`, `RecFormat`, `create_take`, `FakeInput` | std threads and files; used by apps/wavcraft/src/recording.rs, rec_input.rs, player.rs and take_recovery.rs | present, wired | J1, J2, J3, J4, J5 | 26 in-file tests; take_recovery.rs |
| `crates/core/src/take_readout.rs` | Turns a take's numbers into the recording status text: elapsed time, file size, time left on disk and dropped samples. The app's recording panel does not call it. | `TakeStats`, `TakeReadout`, `take_readout` | space | present, core only | J4 | crates/core/tests/j4_take_readout.rs; 0 in-file |
| `crates/core/src/take_recover.rs` | Declares the file side of take repair: list takes in a folder whose header is out of date and repair one in place. Both function bodies are todo!(). | `repair_take_file`, `takes_needing_repair` | take_repair | stub | J3 | crates/core/tests/j3_take_recover.rs; 0 in-file |
| `crates/core/src/take_recovery.rs` | Lists takes a crash left unfinished by their marker files, repairs a take's 44-byte header without touching audio, and discards a take only when asked. | `UnfinishedTake`, `unfinished_takes`, `repair_take`, `discard_take` | record | present, wired | J3 | crates/core/tests/take_recovery.rs; 0 in-file |
| `crates/core/src/take_repair.rs` | Declares the pure header repair for a killed take: from the 44 header bytes and the file length to corrected sizes. The function body is todo!(). | `repair_header`, `HeaderRepair`, `HEADER_BYTES` | none | stub | J3 | crates/core/tests/j3_take_repair.rs, j3_take_recover.rs; 0 in-file |

### Multitrack, mix and CD (`crates/core`, out of v1)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/cd_ui.rs` | The CD Layout view: track list with pause, title, performer and ISRC, total time against the disc limit, export to one WAV plus a CUE sheet, and the CD control verb. It does not burn discs. | `CdView`, `CdView::ui`, `CdView::control`, `CD` | wavcraft_core::cd, io; multitrack_ui::tokens; theme; egui | present, out of v1 | after 1.0 | no in-file tests; the core side is in crates/core/tests/cd.rs and cd_fade.rs |
| `apps/wavcraft/src/mixer.rs` | The Mixer panel over the multitrack session: one channel strip per track plus a master, with fader, pan, mute, solo and meters, and the MIXER control verb. Its Window menu checkbox is not gated in release builds. | `MixerModel`, `StripState`, `SetCmd`, `parse_set`, `set`, `MIXER` | soundcraft_model (Session, Mixer), soundcraft_mix (StripMeter), soundcraft_dsp::pan; theme; egui | present, out of v1 | after 1.0 | 9 in-file tests |
| `apps/wavcraft/src/multitrack_ui.rs` | The Multitrack editor view: track headers, a timeline of clip boxes, the session transport, the new-session and clip-properties dialogs, session save and open, and the session control verbs. | `MtView`, `Transport`, `NewDialog`, `MtView::control`, `tokens`, `SESSION` / `TRACK` / `CLIP` / `SMARKER` / `SREC` / `AUTO` | wavcraft_core::multitrack; soundcraft_model, soundcraft_playback; video.rs; theme; egui, rfd | present, out of v1 | after 1.0 | 9 in-file tests; crates/core/tests/981_visible_clip_at.rs reads this file's source |
| `crates/core/src/cd/codes.rs` | Validators and normalisers for ISRC codes, EAN-13 and UPC-A check digits. | `is_valid_isrc`, `normalize_isrc`, `is_valid_ean13`, `ean13_check_digit`, `is_valid_upc_a`, `upc_a_check_digit` | std only | present, out of v1 | after 1.0 | cd.rs; 13 in-file. Used by cd/mod.rs and apps/wavcraft/src/cd_ui.rs (CD Layout, hidden in release). |
| `crates/core/src/cd/fade.rs` | Per-track fade-in and fade-out lengths for a CD layout, the linear gain applied to a track's samples, and track start positions with pauses counted. | `TrackFade`, `MAX_FADE_SECS`, `fade_gain`, `check_fit`, `apply_fades`, `track_starts` | cd (CdLayout, CdError, CD_RATE) | present, core only | after 1.0 | cd_fade.rs. Nothing calls it: the CD export and CUE sheet do not use it. |
| `crates/core/src/cd/mod.rs` | The CD Layout model: ordered tracks with pause, title, performer and ISRC, total time against the 74 or 80 minute limit, and a streamed export to one 44.1 kHz 16-bit stereo WAV plus a CUE sheet. | `CdLayout`, `CdTrack`, `DiscLimit`, `CdError`, `Exported`, `format_msf` | doc::DocState, save (quantize, write_header), atomic, store | present, out of v1 | after 1.0 | cd.rs, cd_fade.rs. Called from apps/wavcraft/src/cd_ui.rs and app.rs behind v1 Feature::CdLayout. |
| `crates/core/src/meta/source_link.rs` | A link from a mixed-down file back to the session that made it, stored as one escaped INFO tag. Writing it at mixdown and the Edit Original action do not exist. | `SourceLink`, `LINK_FIELD`, `write_source_link`, `read_source_link`, `SourceLink::encode`, `SourceLink::parse` | meta (Metadata) | present, core only | after 1.0 | source_link.rs |
| `crates/core/src/mix/downmix.rs` | Folds 5.1 audio down to stereo, raw or normalised, on planar or interleaved samples. | `downmix_51_to_stereo_raw`, `downmix_51_to_stereo`, `downmix_51_interleaved`, `MIX_GAIN`, `NORMALISE_GAIN` | std only; used only by multitrack/master_mode.rs | present, core only | after 1.0 | 10 in-file tests; master_mode.rs |
| `crates/core/src/mix/mod.rs` | Declares the two mixing helper modules, band_plan and downmix. It holds no code of its own. | `band_plan`, `downmix` | none | present, core only | after 1.0 | none of its own |
| `crates/core/src/mixdown.rs` | Renders a multitrack session, or only chosen clips, through the SoundCraft offline mix engine into a new waveform document, streamed in 65,536-frame blocks with an optional normalise pass. | `MixdownOptions`, `mixdown`, control verb `MIXDOWN` | soundcraft-mix (RangeRender, main_channels), soundcraft-model, crate doc and store, multitrack::MtError; random in tests only; called from apps/wavcraft/src/app.rs and multitrack_ui.rs | present, out of v1 | after 1.0 | 4 in-file tests |
| `crates/core/src/mixer_sections.rs` | Which of the four mixer strip sections (EQ, effects, sends, I/O) are shown, how to flip one, and a comma-separated text form for a settings file. | `MixerSection`, `MixerSections`, `to_text`, `from_text`, `toggle` | std only | present, core only | after 1.0 | 10 in-file tests |
| `crates/core/src/mtplay.rs` | Multitrack playback: an offline render of the main mix, the playback snapshot with read-ahead sources for the audio thread, and a silent real-time transport for runs with no audio device. | `render`, `PlaybackSession`, `SilentPlayer`, `SILENT_BLOCK`, control verbs `SPLAY`, `SSTOP` | soundcraft-mix, multitrack; called from apps/wavcraft/src/multitrack_ui.rs | present, out of v1 | after 1.0 | 5 in-file tests |
| `crates/core/src/mtpunch.rs` | Remembers the last completed punch range and re-arms it for another take. The command and key are not here. | `PunchAgain`, `record`, `last`, `rearm`, `clear` | mtrecord `Punch` | present, core only | after 1.0 | punch_again.rs (23 tests) |
| `crates/core/src/mtrecord.rs` | Multitrack recording: arm a track and pick its input, punch ranges with pre-roll, write each take straight to its own crash-safe float WAV, and stack takes as clips on new playlists. | `Recorder`, `TakeWriter`, `Punch`, `arm_track`, `plan_takes`, `place_take`; control verbs `SREC`, `PUNCH` | multitrack (Multitrack, MtError, MAX_IMPORT_SAMPLES), soundcraft-model, soundcraft_audio_io; random in tests only; called from apps/wavcraft/src/multitrack_ui.rs | present, out of v1 | after 1.0 | 11 in-file tests; 976_take_cut_keeps_gain_envelope.rs, 977_multichannel_arm_route.rs, punch_again.rs |
| `crates/core/src/multitrack.rs` | The multitrack session wrapper over the SoundCraft session model: make a session, add, remove and rename tracks, mute, solo and volume, and place a file or open document on a track as a capped in-RAM clip. | `Multitrack`, `MtError`, `AudioType`, `MAX_IMPORT_SAMPLES`, `parse_track_format`; control verbs `SESSION`, `TRACK`, `CLIP` | soundcraft-model, soundcraft_audio_io, serde_json, mtrecord (Punch, TakeInfo); random in tests only; called from apps/wavcraft/src/multitrack_ui.rs and app.rs | present, out of v1 | after 1.0 | 35 in-file tests; 981_visible_clip_at.rs, multitrack_prefs.rs and the per-submodule test files |
| `crates/core/src/multitrack/automation.rs` | Editing of per-track volume and pan automation lanes on the SoundCraft model: add, move, delete and thin points, read the value at a time, and report a lane as JSON. | `automation_add`, `automation_move`, `automation_delete`, `automation_thin`, `automation_value_at`, control verb `AUTO` | soundcraft-model `AutomationLane`; called from apps/wavcraft/src/multitrack_ui.rs | present, out of v1 | after 1.0 | 7 in-file tests |
| `crates/core/src/multitrack/automation_shape.rs` | Per-segment curve shapes kept beside an automation lane, and the shaped value between two points. Playback does not read them yet. | `SegmentShape`, `SegmentShapes`, `shape_value`, `set_segment_shape` | multitrack, soundcraft-model | present, core only | after 1.0 | automation_shape.rs (15 tests) |
| `crates/core/src/multitrack/invert.rs` | A polarity flag per track, stored in the model's mixer strip, and the sample operation that applies it. Nothing in the mix calls it yet. | `set_invert`, `invert`, `apply_invert` | multitrack, soundcraft-model | present, core only | after 1.0 | track_invert.rs (13 tests) |
| `crates/core/src/multitrack/marker_export.rs` | Exports the session's markers as RFC 4180 CSV text (name, start, end in seconds) and imports them back. | `MarkerRow`, `marker_rows`, `import_markers`, `markers_csv`, `parse_markers_csv` | multitrack | present, core only | after 1.0 | 13 in-file tests |
| `crates/core/src/multitrack/master_mode.rs` | The master output mode (mono, stereo or 5.1): its channel count, parsing of the mode name, and the folds from stereo and 5.1 down to mono. | `MasterMode`, `MODE_LIST`, `stereo_to_mono`, `downmix_51_to_mono`, `buffers_51_to_mono` | mix/downmix.rs, soundcraft-model `ChannelFormat` | present, core only | after 1.0 | master_mode.rs (18 tests) |
| `crates/core/src/multitrack/notes.rs` | Validated reading and changing of a session's title and notes: length limits, trimming, and refusal of control characters and over-long text. | `clean_notes`, `clean_title`, `set_notes`, `set_title`, `NOTES_MAX_CHARS`, `TITLE_MAX_CHARS` | multitrack, soundcraft-model `Session` | present, core only | after 1.0 | session_notes.rs (21 tests) |
| `crates/core/src/multitrack/properties.rs` | Changes a session's name, opening tempo, time signature, sample rate and bit depth after creation. Each setter returns the old value, and rate and depth change only while the session holds no audio. | `holds_audio`, `set_name`, `set_tempo_bpm`, `set_time_signature`, `set_rate`, `set_bit_depth` | multitrack, soundcraft-model | present, core only | after 1.0 | session_properties.rs (41 tests) |
| `crates/core/src/multitrack/reorder.rs` | Moves one audio track from one position to another, counting audio tracks only and keeping each track's id, clips and mixer settings. | `Multitrack::move_track` | multitrack | present, core only | after 1.0 | track_reorder.rs (14 tests) |
| `crates/core/src/multitrack/split.rs` | Splits a stereo track into two hard-panned mono tracks named L and R that take its place, copying its settings and refusing tracks that cannot be split. | `split_name`, `split_to_mono` | multitrack, soundcraft-model | present, core only | after 1.0 | split_to_mono.rs (11 tests) |
| `crates/core/src/multitrack/timeline.rs` | The view arithmetic for a multitrack timeline: visible seconds, zoom limits and zoom about a point, scroll clamping, track row height, and time to pixel conversion. | `TimelineView`, `fit_seconds_per_px`, `zoom_at`, `scroll_by`, `time_at`, `x_of` | std only | present, core only | after 1.0 | timeline_view.rs (31 tests) |
| `crates/core/src/multitrack/track_height.rs` | Reads and changes a track's height by track id on the model's own height presets: set an exact height, set one by name, or step one preset taller or shorter. | `track_height`, `track_height_points`, `set_track_height`, `set_track_height_named`, `step_track_height` | multitrack, soundcraft-model `TrackHeight` | present, core only | after 1.0 | 11 in-file tests |
| `crates/core/src/multitrack/track_info.rs` | Validated track colour and track notes: a named default palette, strict #RRGGBB parsing and formatting, and notes with a length limit and control-character refusal. | `palette`, `parse_color`, `format_color`, `set_track_color_hex`, `set_track_notes`, `TRACK_NOTES_MAX_CHARS` | multitrack, soundcraft-model | present, core only | after 1.0 | track_info.rs (34 tests) |
| `crates/core/src/recent_sessions.rs` | A newest-first list of multitrack session file paths with a maximum length, path normalisation, pruning of missing files, and a JSON round trip. It touches no disk or menu. | `RecentSessions`, `normalise_path`, `promote`, `prune`, `to_json`, `from_json` | serde_json | present, core only | after 1.0 | recent_sessions.rs (25 tests) |
| `crates/core/src/ruler.rs` | A pure model of the multitrack time ruler: picks a nice tick step for a visible range and width and lists major and minor ticks with labels in seconds or minutes:seconds. | `Ruler`, `Tick`, `RulerFormat`, `RulerError`, `nice_step`, `ruler_ticks` | std only | present, core only | after 1.0 | ruler.rs (35 tests) |
| `crates/core/src/sc_source.rs` | Serves an open document as a SoundCraft clip source through the store's block cache, builds a one-clip session from it, and reads it back in bounded chunks as a checksum. | `DocSource`, `session_with_document`, `read_checksum`, `CHUNK_FRAMES`, control verb `SCSOURCE test` | crate doc state, soundcraft-model `SampleSource`; called only from the SCSOURCE test verb in apps/wavcraft/src/app.rs | present, wired | after 1.0 | sc_source.rs (2 tests) |
| `crates/core/src/session_file.rs` | Saves and opens the multitrack session file (.wcsession, the SoundCraft model's JSON) with source paths relative to the session where close, audio without a file written beside it, a size cap, and offline source reporting. **Platform:** No per-OS code path of its own. One in-file test checks the owner-only file mode on Unix only. | `EXTENSION`, `save_to`, `open_from`, `SaveReport`, `relative_path`, `offline_sources` | multitrack, mtrecord (Punch), save `write_file_atomic`, soundcraft_audio_io, soundcraft-model; random in tests only; called from apps/wavcraft/src/multitrack_ui.rs | present, out of v1 | after 1.0 | 9 in-file tests; session_notes.rs |
| `crates/core/src/settings/multitrack.rs` | Holds the three Multitrack preferences (track height, clip crossfade length, clip colour mode) as extra lines in the settings file, keeping every line it does not own. | `MultitrackPrefs`, `ClipColorMode`, `KEYS`, `MultitrackPrefs::load`, `MultitrackPrefs::save`, `merge_into` | settings (parent constants only) | present, core only | after 1.0 | crates/core/tests/multitrack_prefs.rs; 0 in-file |
| `crates/core/src/settings/video.rs` | Holds the four Video preferences (timecode overlay on or off, its corner, its size, spot frame on scrub) as extra lines in the settings file. | `VideoPrefs`, `OverlayCorner`, `KEYS`, `VideoPrefs::load_from`, `VideoPrefs::serialize` | none | present, core only | after 1.0 | crates/core/tests/video_prefs.rs, multitrack_prefs.rs; 0 in-file |
| `crates/core/src/strip_eq.rs` | Maps a mixer strip's three-knob quick EQ (low shelf, one bell, high shelf) onto the Parametric EQ's ten settings and back. It processes no audio. | `StripEq`, `StripEq::to_params`, `StripEq::from_params`, `StripEq::clamped`, `StripEq::is_flat` | effects | present, core only | after 1.0 | 7 in-file |
| `crates/core/src/video_snap.rs` | Maps a video's frame rate to the nearest SMPTE rate on offer, refusing rates too far off, and snaps a time in seconds to that rate's frames. | `rate_from_fps`, `rate_from_ratio`, `snap_time_to_video`, `snap_time_to_rate`, `time_format_for`, `VideoSnapError` | smpte, view | present, core only | after 1.0 | crates/core/tests/video_snap.rs; 0 in-file |

### Automation (`crates/automation`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `crates/automation/Cargo.toml` | The manifest of the wavcraft-automation library crate; its only dependency is wavcraft-core. | `wavcraft-automation` | wavcraft-core | present, core only | N1 | none |
| `crates/automation/src/headless.rs` | The windowless backend: it opens a file into a session store, keeps one editor and a selection, and answers each request with the same core calls the window makes. | `Headless`, `Headless::new`, `Headless::with_crossfades`, `Headless::document`, `impl Backend for Headless` | wavcraft_core io::open, actions::Editor, doc::history::History, doc::edit::Crossfades, effects, save | present, core only | N1 | crates/automation/tests/headless.rs |
| `crates/automation/src/lib.rs` | The six control requests as one enum with a parser and a wire spelling, and the Backend trait that answers a request or a raw line with OK or ERR text. | `Request`, `Request::parse`, `Request::line`, `Backend`, `Backend::answer_line`, verbs `OPEN` `SELECT` `ACTION` `EFFECT` `SAVE` `STATE` | std only; modules headless, remote. No caller in apps/wavcraft: the window's control.rs still parses its own lines | present, core only | N1 | crates/automation/tests/request.rs |
| `crates/automation/src/remote.rs` | The backend for a running app: it connects to the loopback control port, presents the token first, sends one request per line, and after OPEN polls STATE until busy=0. | `Remote`, `Remote::connect`, `Remote::from_env`, `impl Backend for Remote`, env `WAVCRAFT_CONTROL_PORT`, `WAVCRAFT_CONTROL_TOKEN_FILE` | std::net; the wire format of apps/wavcraft/src/control.rs. Nothing outside its tests constructs it | present, core only | N1 | crates/automation/tests/remote.rs |
| `crates/automation/tests/headless.rs` | Tests of the headless backend on a 16-bit WAV written in the test: delete, invert, effects with and without a selection, crossfades and bad requests, each compared sample for sample after a save. | `delete_then_invert_then_save_matches_the_expected_samples`, `with_the_window_crossfades_only_the_edit_joins_differ`, `bad_requests_answer_err_and_change_nothing` | wavcraft_automation::headless, wavcraft_core | present, core only | N1 | 8 tests in the file |
| `crates/automation/tests/remote.rs` | Tests of the remote backend's protocol against a scripted stand-in listener: the lines sent, the replies read, the wait after OPEN, and a refused token. It does not start the app. | `stand_in`, `open_waits_for_busy_to_clear`, `a_refused_token_is_an_error` | wavcraft_automation::remote, std::net::TcpListener | present, core only | N1 | 3 tests in the file |
| `crates/automation/tests/request.rs` | Tests that every request round-trips through its line, that EFFECT accepts the effect. prefix, and that bad lines are refused with the usage text. | `every_request_round_trips_through_its_line`, `bad_lines_are_refused_with_the_usage` | wavcraft_automation::Request | present, core only | N1 | 3 tests in the file |

### The app: shell (`apps/wavcraft`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/Cargo.toml` | The app crate's manifest: its dependencies on the core crate, the SoundCraft crates, eframe, cpal, rfd, arboard and craft-log, and the lint rule that denies unsafe code outside the Windows ACL file. **Platform:** windows-sys with the Win32 security features is a dependency on Windows only; eframe is built with both wayland and x11 for Linux. | `wavcraft` package, `unsafe_code = "deny"`, `windows-sys` (Windows only) | wavcraft-core, soundcraft-playback, soundcraft-model, soundcraft-mix, soundcraft-audio-io, soundcraft-dsp, soundcraft-video, eframe, egui, cpal, rfd, arboard, dirs, log, craft-log, serde_json; windows-sys on Windows only | present, wired | I5 | none |
| `apps/wavcraft/src/app.rs` | The whole window as one struct: document slots, open, save, export, editor drawing and gestures, playback, recording glue, panels and docking, menus, preferences, dialogs, background job polling, and the control channel's verb table. **Platform:** `cfg!(target_os = "macos")` picks shortcut text (Cmd for Ctrl) in menus and tool labels. `reveal_label` names the file manager per OS (macOS, Windows, other). No cfg-gated code paths otherwise. | `WaveApp`, `WaveApp::new`, `control_request`, `run_action`, `DocSlot`, `impl eframe::App` (`logic`, `ui`); stubs: `shortcut_help_rows`, `shortcut_help_content` (both `unimplemented!()`, the Help shortcut list of I3) | Every app module (menus, player, recording, control, doclist, panels' UI files, v1, theme, app/spectral); most of wavcraft_core (actions, doc, save, export, settings, panels, transport, view, jobs); eframe, egui | present, wired | A1-A6, B1-B5, C1-C6, D1-D7, E1-E6, F1-F3, G1-G4, H1-H4, I1-I4, I6, J1-J7, M5, N1 | 30 in-file tests (tests, reset_tests, step_name_tests, look_tests); i3_shortcut_help_tests.rs (39), issue1000_tests.rs (7), issue1002_tests.rs (7), issue366_tests.rs (12), issue369_tests.rs (7), issue377_tests.rs (2), 74 tests in all; crates/automation/tests/request.rs and the tools/alpha scripts drive it over the control channel. The 39 I3 tests call two stub functions (`shortcut_help_rows`, `shortcut_help_content`, both `unimplemented!()`), so I expect them to panic. |
| `apps/wavcraft/src/doclist.rs` | The order of the open documents and which one is current. The current document lives in the window; the others are parked here, and a switch swaps them. | `DocList`, `Where`, `DocList::add`, `DocList::switch`, `DocList::close_current`, `DocList::parked` | std only | present, wired | A5, D2 | 4 in-file tests |
| `apps/wavcraft/src/main.rs` | The binary's entry point: declares the app's modules, parses the command line, runs the hidden VST3 scan worker mode, installs the log, picks the graphics backend and opens the window. **Platform:** Release builds on Windows use the windows subsystem (no console). On Windows wgpu is limited to DirectX 12 unless WGPU_BACKEND is set. `win_acl` is compiled on Windows only. | `main`, `parse_args`, `Args`, `configure_graphics`, `--session-dir`, `--reset-preferences` | app, selflog, every sibling module (mod list); wavcraft_core::vst3; eframe, egui, wgpu | present, wired | A1, I5, N2 (only the `wavcraft [FILE]` form; `run`, `convert`, `inspect` and `mcp` are not here) | 1 in-file test (flag and file order). Flags and variables are listed in cm-flags-env.md in the scratch folder. |
| `apps/wavcraft/src/menus.rs` | The right-click menus, the toolbar and the Tools panel as plain data. Each entry is an action table row or one of a few app commands; the app turns it into a click, a control line or a button. | `Cmd`, `Entry`, `Item`, `Menu`, `TOOLS`, `toolbar` | wavcraft_core::actions (ActionId, TABLE) | present, wired | I1, I3, C1, C4 | 5 in-file tests; v1.rs tests also read `TOOLS` |
| `apps/wavcraft/src/notices.rs` | The third-party notices text shown under Help, About. It is the repo-root THIRD-PARTY-NOTICES.txt compiled into the program. | `TEXT`. The `ABOUT` control verb that reads it is handled in app.rs. | THIRD-PARTY-NOTICES.txt (made by tools/notices.py) | present, wired | I6 | 3 in-file tests; tools/test_notices.py covers the generator |
| `apps/wavcraft/src/recovery.rs` | Auto Save and crash recovery in the window: keeps the session journal current while I edit, scans at start for sessions and takes a crashed run left, and shows the dialog that offers to recover or discard them. | `Recovery`, `Choice`, `Recovery::track`, `Recovery::show`, `roots`. The control verbs `AUTOSAVE`, `RECOVER` and `RECOVERTAKE` are handled in app.rs. | wavcraft_core::autosave, take_recovery, actions::Editor; egui | present, wired | I4, J3 | no in-file tests; crates/core/tests/autosave.rs, 990_autosave_markers.rs, take_recovery.rs, j3_take_recover.rs for the core side |
| `apps/wavcraft/src/v1.rs` | What a release build offers: the list of out-of-v1 features (Multitrack, CD Layout, Video, Batch, Mix Paste, Diagnostics, VST3 plugins) whose menus a release build hides, the rule that drops the planned Spot Healing tool, and the rule that leaves the Diagnostics commands and the out-of-v1 effects out of the Keyboard Shortcuts list. **Platform:** Build-type gate, not an OS one: `cfg!(debug_assertions)` decides `RELEASE`. | `RELEASE`, `Feature`, `offered`, `offered_in`, `tool_offered_in`, `command_offered_in` | wavcraft_core::effects (OUT_OF_V1); wavcraft_core::panels (PanelSet, tests only); menus, rack_panel, shortcut_editor (tests only) | present, wired | M5 | 5 in-file tests, among them `release_lists_nothing_deliberately_not_in_v1` |
| `crates/core/src/i18n.rs` | The interface text catalogue: English as the built-in source, a pseudo-locale made from it, key lookup with English fallback, and the process-wide current language. | `tr`, `tr_args`, `set_language`, `Translator`, `pseudo`, `LANGUAGES` | std only, the built-in i18n/en.txt; used by app.rs; control verb `PREF set language` | present, wired | after 1.0 | 9 in-file tests |

### The app: panels and dialogs

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/batch_panel.rs` | The Batch Process window: input file list, operation chain, output folder and name pattern, a worker thread that runs the files one by one, and the BATCH control verb. | `BatchPanel`, `BatchPanel::start`, `BatchPanel::poll`, `BatchPanel::control`, `BatchPanel::ui`, `BATCH` | wavcraft_core::batch, io, save; egui | present, out of v1 | after 1.0 | no in-file tests; the core side is in crates/core/tests/batch.rs and batch_cli.rs |
| `apps/wavcraft/src/convert_ui.rs` | The Convert Sample Type dialog and the Interpret Sample Rate dialog. They only collect settings; the window runs the conversion on a worker thread. | `ConvertDialog`, `ConvertDialog::show`, `Outcome`, `interpret_dialog` | wavcraft_core::convert, dither, save; egui | present, wired | A4 | no in-file tests; the core side is in crates/core/tests/convert.rs, a4_dither.rs and dither.rs |
| `apps/wavcraft/src/diagnostics_panel.rs` | The Diagnostics window: pick DeClipper or DeClicker, scan on a cancellable worker thread, list the regions found, and ask the app to repair chosen rows as one undo step. | `DiagPanel`, `Ask`, `Scanned`, `parse_tool`, `parse_rows`. The `DIAG` control verb is handled in app.rs. | wavcraft_core::diagnostics (declip, declick), doc::edit; theme; egui | present, out of v1 | after 1.0 | 1 in-file test; crates/core/tests/diagnostics_panel.rs, declip.rs, declick.rs for the core side |
| `apps/wavcraft/src/effect_dialog.rs` | The generic effect and generator dialog: it draws any effect's parameters from its descriptor list with OK, Cancel and reset, and only edits values. The app applies them. | `EffectDialog`, `Answer`, `EffectDialog::new`, `EffectDialog::new_generator`, `EffectDialog::show`. The `EFFECT_DIALOG` control verb is handled in app.rs. | wavcraft_core::effects (Effect, ParamDesc); egui | present, wired | E2, E3, E5, E6, F2 | 3 in-file tests; crates/core/tests/effect_params.rs for the core side |
| `apps/wavcraft/src/files.rs` | The Files panel's data and rules: one row per open document, the column set, sort, filter, expanded channel lines, saved column choice, and the command that reveals a file in the system file manager. **Platform:** `reveal_command` picks the program from the OS name at run time: `open -R` on macOS, `explorer /select,` on Windows, a file-manager call on Linux. No cfg sites. | `Row`, `Col`, `View`, `parse_columns`, `reveal_command`. The `FILES` control verb is handled in app.rs. | wavcraft_core::recent (config file); std | present, wired | A5 | 17 in-file tests |
| `apps/wavcraft/src/markers_panel.rs` | The Markers panel table (a click picks, a double-click goes to the row, a click on the only picked row renames it; columns) and the state for dragging a marker's flag in the ruler. | `MarkersUi`, `MarkersUi::panel`, `MarkersUi::click`, `MarkersUi::row_click`, `MarkersUi::handle_at` (grab width from `theme::metrics().handle_grab`), `Drag`, `PanelOut` | wavcraft_core::actions::Editor, settings, Markers, Viewport; theme; egui | present, wired | H2, H1, M4 | 3 in-file tests |
| `apps/wavcraft/src/match_ui.rs` | The Match Loudness panel: the open document in a list, Scan to measure, the target settings and presets, and Run on a worker thread. The app pushes the finished edit on the history as one step. | `MatchUi`, `Finished`, `MatchUi::start_scan`, `MatchUi::start_run`, `MatchUi::panel`, `MatchUi::take_finished` | wavcraft_core::match_loudness, loudness; theme; egui | present, wired | G3 | no in-file tests; crates/core/tests/match_loudness.rs and loudness.rs for the core side |
| `apps/wavcraft/src/playlist_panel.rs` | The Playlist panel and its runner: plays marker ranges in order with their loop counts through the player, and answers the PLAYLIST control verb. The list is session-only and is cleared when another file opens. | `PlaylistUi`, `PanelOut`, `PlaylistUi::start`, `PlaylistUi::tick`, `PlaylistUi::control`, `PLAYLIST` | wavcraft_core::Playlist, Markers; player::Player; egui | present, wired | after 1.0 | no in-file tests |
| `apps/wavcraft/src/shortcut_editor.rs` | The Keyboard Shortcuts dialog: a searchable command list with chords, key capture for a new chord, a prompt when another command has it, shortcut sets and the visual keyboard. It reports the change; the app applies and saves it. **Platform:** Takes a `mac` flag to show Mac key names; no cfg sites. | `ShortcutDialog`, `Change`, `ShortcutDialog::show`, `ShortcutDialog::capturing`, `listed` | wavcraft_core::shortcuts, shortcut_sets, keyboard; theme; v1 (a release build lists no out-of-v1 command); egui | present, wired | I3, M5 | 1 in-file test (M5); crates/core/tests/shortcuts.rs, shortcut_overrides.rs, shortcut_sets.rs for the core side |
| `apps/wavcraft/src/video.rs` | The Video panel: shows the frame of an attached MOV or MP4 at the playhead. A worker thread owns the decoder and a three-frame cache and scales frames down; the window only sends times and receives pictures. | `VideoPanel`, `Info`, `Status`, `probe`, `frame_index`. The `VIDEO` control verb is handled in app.rs. | soundcraft_video (Movie); egui | present, out of v1 | after 1.0 | 10 in-file tests; crates/core/tests/video_prefs.rs and video_snap.rs for the core side |
| `crates/core/src/keyboard.rs` | The visual keyboard's logic: an ANSI layout of key caps, a modifier set, and which keys of a keymap have a binding for those modifiers. | `KeyCap`, `ANSI_ROWS`, `Mods`, `Keymap::keys_with`, `Keymap::commands_on_key` | shortcuts (Command, Keymap); used by shortcut_editor.rs, app.rs; control verb `KEYBOARD` | present, wired | I3 | shortcut_sets.rs |
| `crates/core/src/meta_view.rs` | What the Metadata panel lists: rows from the metadata, a text filter, named display sets saved to a small text file, and the AIFF AUTH chunk to creator mapping. **Platform:** none in file; the sets file path comes from the config dir lookup | `Row`, `rows`, `filter_rows`, `DisplaySets`, `auth_chunk`, `creator_from_auth_into` | meta, xmp; used by app.rs (Metadata panel and its display-set commands) | present, wired | after 1.0 | 4 in-file tests |

### The app: audio in and out

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/input_monitor.rs` | The input monitor: an input stream opened only while input metering or a mic check is on, feeding the level tap and recording nothing. A fake input thread replaces the device when WAVCRAFT_FAKE_INPUT is set. | `InputMonitor`, `InputMonitor::start`, `InputMonitor::stop`, `InputMonitor::stale`, `find_input`. The `MICCHECK` control verb is handled in app.rs. | cpal; wavcraft_core::input_meter; player::device_name | present, wired | J4, J5 | 2 in-file tests |
| `apps/wavcraft/src/player.rs` | Playback of the open document: a non-blocking audio callback fed by a read-ahead ring in play order, sample rate conversion, loop, skip, reverse and speed, the rack and metronome on playback, device-change reopening, and a silent test sink. **Platform:** No cfg sites. `host_names` lists whatever audio hosts cpal offers on the system; `WAVCRAFT_NO_AUDIO=1` replaces the sound card everywhere. | `Player`, `OutputChoice`, `Metronome`, `Player::start`, `Player::replace_audio`, `Player::maintain` | cpal; wavcraft_core (DocState, resample, rack, record::MonitorRing, levels); soundcraft crates; log | present, wired | C5, C6, E6, F1, J2 | 39 in-file tests, plus f48_device_change_tests.rs (6) and issue377_tests.rs (2) |
| `crates/core/src/chanmap.rs` | The pure logic of audio channel mapping (document channel to device output, device input to record channel) and the choice of output buffer size. | `ChannelMap`, `MAX_CHANNELS`, `choose_buffer`, `buffer_choices` | std only | present, wired | C5, J5 | chanmap.rs. Called from apps/wavcraft/src/player.rs and app.rs, settings.rs and hwprofile.rs. |
| `crates/core/src/hwprofile.rs` | Audio hardware settings kept per device setup, keyed by the set of device names on offer. Switching setups restores the last settings for that setup, with a fallback to the system default. **Platform:** none; device lists are passed in | `Hardware`, `ProfileStore`, `Profile`, `Outcome`, `setup_key`, `ProfileStore::activate` | settings, chanmap; used by app.rs (device setup change, `DEVICE_CHANGED`) | present, wired | J5 | 5 in-file tests. No integration test file uses this module. |
| `crates/core/src/transport.rs` | Holds transport logic with no window or device: play plans, loop wrap, skip selection, shuttle speeds, scrub grains, marker jumps and how the view follows the playhead. | `Plan`, `Transport`, `Mode`, `Follow`, `Shuttle`, `Scrub` | view | present, wired | C5, C6, H3 | 27 in-file; crates/core/tests/settings.rs touches its options |

### The app: control channel

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/control.rs` | The loopback control channel: a TCP listener on 127.0.0.1 that checks the token line, forwards each request line to the window thread and writes back the answer. Its header comment is the verb reference. **Platform:** The token file check and creation differ: Unix uses mode 0600 and permission bits; Windows calls win_acl for an owner-only ACL. Tests are split by cfg(unix) and cfg(windows). | `Request`, `start_from_env`, `TOKEN`, `WAVCRAFT_CONTROL_PORT`, `WAVCRAFT_CONTROL_TOKEN_FILE` | std::net; wavcraft_core::random; win_acl (Windows); log | present, wired | N1 | 4 in-file tests (3 run on Unix, 2 on Windows) |
| `crates/core/src/bundle.rs` | Export and import of the settings folder (settings, shortcut overrides, saved workspaces) as one checked text file, and the preference reset that moves the settings file aside. | `Bundle`, `export`, `import`, `read_checked`, `reset_preferences`, `is_bundle_name` | settings::Settings | present, wired | I2, I3 | preferences.rs; 12 in-file. Called from apps/wavcraft/src/app.rs (control verb SETTINGS export/import, preference reset). |
| `crates/core/src/shortcut_sets.rs` | Stores named keyboard shortcut sets as files in the config folder, lists, loads, saves and deletes them, and remembers which set is active. The built-in default set is never stored. | `SetStore`, `DEFAULT_SET`, `valid_name`, `SetStore::startup`. The control verbs `SHORTCUTSETS` and `SHORTCUTSET` that use it are handled in apps/wavcraft/src/app.rs. | shortcuts | present, wired | I3 | crates/core/tests/shortcut_sets.rs; 0 in-file |
| `crates/core/src/shortcuts.rs` | Owns the default key chord table, the keymap in use with conflict checks, the editor operations on it, and the override file that keeps only what differs from the defaults. **Platform:** No cfg sites. A chord's cmd flag is spelled Command on a Mac and Ctrl elsewhere at run time. | `Shortcut`, `Command`, `BINDINGS`, `Keymap`, `Overrides`. The control verbs `KEY` and `SHORTCUTS` that use it are handled in apps/wavcraft/src/app.rs. | actions | present, wired | I3 | shortcuts.rs, shortcut_overrides.rs, shortcut_sets.rs, actions.rs, workspace_keys.rs, e5_repeat_last.rs; 0 in-file |

### The app: platform code

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/display.rs` | Screen maths for high-DPI drawing (one column per physical pixel, hairlines) and the window keeper, which saves the window rectangle in physical pixels and puts the window back on a monitor that is still connected. **Platform:** No cfg sites. It handles per-display scale factors (2.0 on a Retina Mac, 1.5 on a scaled Windows display) at run time. | `physical_columns`, `column_x`, `hairline`, `PxRect`, `clamp_to_monitors`, `WindowKeeper` | egui, eframe; std only for the maths | present, wired | B1, I2, I5 | 9 in-file tests |
| `apps/wavcraft/src/oscb.rs` | The system clipboard for Copy, Cut and Paste: Copy publishes the clip as a 24-bit WAV file entry in a temporary folder, and Paste takes an audio file another program put there. Large clips stay internal. **Platform:** No cfg sites; arboard supplies the file list on all three systems. | `OsClipboard`, `OsClipboard::publish`, `OsClipboard::incoming`, `OsClipboard::take`, `OsClipboard::peek`. The `OSCLIP` control verb is handled in app.rs. | arboard; wavcraft_core::clipboard, doc::edit::Clip, io | present, wired | D1 | no in-file tests; crates/core/tests/clipboards.rs for the core side |
| `apps/wavcraft/src/reveal.rs` | A second builder for the command that reveals a file in the system file manager. Nothing calls it; the Files panel uses `files::reveal_command` instead. The module is compiled (declared in main.rs under `#[allow(dead_code)]`), so its 12 in-file tests do run. It is unused code with tests, not untested code. **Platform:** Takes the OS as an argument: explorer on Windows, `open -R` on the Mac, xdg-open on the parent folder on Linux. No cfg sites. | `Os`, `RevealCommand`, `reveal_command` | std only | dead code | A5 | 12 in-file tests |
| `apps/wavcraft/src/win_acl.rs` | Windows only: makes the control channel's token file owner-only and checks that an existing one is, through the Win32 security API, using the process user's SID. It is the app's one place that allows unsafe code. **Platform:** The whole module is compiled on Windows only (`#[cfg(windows)] mod win_acl` in main.rs); Unix uses file mode bits in control.rs instead. | `set_owner_only`, `others_can_read` | windows-sys (Win32 Security, Authorization, Threading) | present, wired | N1, I5 | no in-file tests; the Windows-only test in control.rs exercises it |
| `crates/core/src/settings.rs` | Owns the settings text file: its keys, ranges and defaults, loading and saving it, and resolving the session folder from flag, environment variable, file or default. **Platform:** Path to bytes and back differs: raw bytes on unix, UTF-16 on Windows, lossy text elsewhere. Config folder differs per OS. Two symlink tests are unix only. | `Settings`, `KEYS`, `ENV_SESSION_DIR`, `SPEC_RESOLUTIONS`, `DEFAULT_LOUDNESS_TARGET_LUFS`. The control verbs `SETTINGS` and `PREF` that use it are handled in apps/wavcraft/src/app.rs, not here. | appearance, chanmap, channels, freq_axis, panels, resample, smpte, snap, statusbar, store, tempo, view, workspaces | present, wired | I2, A4, D7, J5, B2, G3, I4 | settings.rs, preferences.rs, pref_pages.rs, a4_dither.rs, j5_input_device.rs, locked_922_settings_newlines.rs, testkit hostile.rs; 9 in-file |

### `crates/craft-log`

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `apps/wavcraft/src/selflog.rs` | The app's own log: a fresh log file each start in the config folder with the two before it kept, a 16 MiB cap, the CRAFT_LOG filter, and a crash file written on a panic. Installed from main. **Platform:** No cfg sites; the config folder comes from `dirs` per system. | `APP`, `LOG_FILE`, `log_dir`, `install`, `open_fresh` | craft-log; dirs; log | present, wired | I5 | 4 in-file tests |
| `crates/craft-log/Cargo.toml` | The manifest of the self-logging crate: a build script, two dependencies, and one whole-process test target that has its own main. | `craft-log`, `build = "build.rs"`, `[[test]] process (harness = false)` | craft-stage, log | present, wired | I4 | none |
| `crates/craft-log/build.rs` | The build script that reads the git folder directly (no git program) to record the short commit, the build date and the uncommitted-work mark as compile-time environment values. | `CRAFT_LOG_COMMIT`, `CRAFT_LOG_TREE`, `CRAFT_LOG_BUILD_DATE`, `SOURCE_DATE_EPOCH` | src/build_tree.rs (included by path); std only | present, wired | I4 | none of its own; build_tree.rs has 1 in-file test |
| `crates/craft-log/rustfmt.toml` | The formatting settings for this crate: 160-column lines and the Max small-item heuristics, matching the other Craft apps. | `max_width = 160`, `use_small_heuristics = "Max"` |  | present, wired |  | none; the CI fmt job reads it |
| `crates/craft-log/src/build_tree.rs` | The guess at whether the build holds uncommitted work: a watched source file newer than the git index gives the +modified? suffix. | `suffix`, `MODIFIED`, `WATCHED` | std only; included by build.rs and by the crate's tests | present, wired | I4 | 1 in-file test |
| `crates/craft-log/src/cli.rs` | The logger for command-line programs: one shared appended file per app with a 16 MiB cap and a lock, secret masking of arguments and environment values, and the final log: line on failure. **Platform:** The non-blocking open flag is set only on Linux for five named CPU families; elsewhere only the regular-file check guards the open. One test is Unix only | `CliLogger`, `install`, `install_checked`, `mask_args`, `report_failure`, `MAX_FILE_BYTES` | filter, line, collapse; log. No WavCraft program calls it: the app has no command-line mode yet | present, core only | N2 | 14 in-file tests; crates/craft-log/tests/process.rs |
| `crates/craft-log/src/collapse.rs` | The repeat collapse (a run of identical lines becomes one line and a count note) and the in-memory ring of the last 200 log lines. | `Collapser`, `Collapser::observe`, `note_text`, `remember`, `recent`, `RING_LINES` | log; used by wrap.rs, cli.rs and crash.rs | present, wired | I4 | 3 in-file tests |
| `crates/craft-log/src/crash.rs` | Crash files: a panic hook that writes a stamped text file with the build line, the panic message, a backtrace and the recent log lines, at most five per run. | `install_hook`, `write`, `text`, `file_name`, `is_crash_file`, `MAX_FILES_PER_RUN` | collapse::recent, line; installed by apps/wavcraft/src/selflog.rs | present, wired | I4 | 4 in-file tests |
| `crates/craft-log/src/filter.rs` | Level filters: env_logger style directives, the CRAFT_LOG switch and its precedence over RUST_LOG, and the JSON switch. | `Filter`, `Filter::parse`, `Filter::level_for`, `spec`, `spec_from_env`, `json_from_env` | log; used by apps/wavcraft/src/selflog.rs | present, wired | I4 | 3 in-file tests |
| `crates/craft-log/src/lib.rs` | The crate root: the App identity and its build line, the build constants, the shared log path, and install_wrapped, which wraps the app's own logger and starts the stall watch. | `App`, `App::build_line`, `install_wrapped`, `set_log_path`, `log_path`, `BUILD_COMMIT` | modules cli, collapse, crash, filter, line, report, stall, testing, wrap; log. Called from apps/wavcraft/src/selflog.rs | present, wired | I4 | 1 in-file test |
| `crates/craft-log/src/line.rs` | UTC timestamps and the two log line formats, text and JSON, with escaping and an 8 KiB message cap. | `Line`, `Line::text`, `Line::json`, `timestamp`, `escape_text`, `MAX_MESSAGE_BYTES` | std only; used by apps/wavcraft/src/selflog.rs | present, wired | I4 | 4 in-file tests |
| `crates/craft-log/src/report.rs` | The diagnostic report: ordered section, key and value fields for app, system, processor, memory, switches, helpers and logs, rendered as text or JSON, with the home folder masked. **Platform:** System fields are read from Linux /proc and /etc and read unknown elsewhere; Windows has its own processor, version and home-folder helpers and starts helpers without a console window; Unix kills the helper's process group | `Report`, `Report::new`, `Report::add_switches`, `Report::add_logs`, `mask_home`, `set_memory_reader` | lib::App, log; runs only ffmpeg -version. No caller in apps/wavcraft | present, core only | after 1.0 | 19 in-file tests |
| `crates/craft-log/src/stall.rs` | The stall watch: a thread that listens to craft-stage scopes, warns when one runs past 10 s and every 30 s after, and logs each scope's stage times when it ends. | `Watch`, `start`, `STALL_AFTER`, `STALL_EVERY`, `INFO_AFTER` | craft-stage Hook, log; started by install_wrapped. It hears nothing today: no code in apps/wavcraft or crates/core opens a scope | present, wired | I4 | 3 in-file tests |
| `crates/craft-log/src/testing.rs` | A per-thread capture logger for tests that prints the test's last 50 log lines when the test panics. | `capture`, `captured`, `Capture`, `FAILURE_LINES` | log; no WavCraft test calls it | present, core only | after 1.0 | 2 in-file tests; crates/craft-log/tests/process.rs |
| `crates/craft-log/src/wrap.rs` | The wrapper around the window app's own logger that applies the repeat collapse and feeds the recent-line ring, leaving the inner logger's file and format alone. | `Wrapped`, `Wrapped::new`, `impl log::Log for Wrapped` | collapse, line, log; built by install_wrapped | present, wired | I4 | 1 in-file test |
| `crates/craft-log/tests/process.rs` | Whole-process checks of the command-line logger: the test binary starts copies of itself and checks odd arguments, a full standard error, a FIFO in place of the log, many runs at once, and secret masking. **Platform:** The non-Unicode argument case is built on Unix only | `a_fifo_in_place_of_the_log_does_not_hang`, `many_runs_at_once_on_a_full_log_lose_nothing`, `a_secret_argument_is_never_logged` | craft_log::cli, craft_log::testing | present, core only | N2 | 7 cases run from its own main (harness = false) |

### `crates/craft-stage`

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `crates/craft-stage/Cargo.toml` | The manifest of the craft-stage crate, which has no dependencies. | `craft-stage` | none | present, wired | I4 | none |
| `crates/craft-stage/rustfmt.toml` | The formatting settings for this crate: 160-column lines and the Max small-item heuristics. | `max_width = 160`, `use_small_heuristics = "Max"` |  | present, wired |  | none; the CI fmt job reads it |
| `crates/craft-stage/src/lib.rs` | Named scopes and stages for long operations: a scope guard, stage marks with optional numbers, a per-thread innermost scope, and one global hook that costs nothing when unset. | `scope`, `Scope`, `stage`, `stage_with`, `Hook`, `set_hook` | std only. craft-log's stall watch sets the hook, but no code in apps/wavcraft or crates/core calls `scope` | present, wired | I4 | 4 in-file tests; crates/craft-log/src/stall.rs tests |

### Test kit and benches (`crates/testkit`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `crates/testkit/Cargo.toml` | The manifest of the test kit crate: the kit writer binary and three bench binaries, with unsafe code denied instead of forbidden so the one Windows memory call can be allowed. **Platform:** windows-sys is a Windows-only dependency | `wavcraft-testkit`, `wavcraft-bench`, `wavcraft-store-bench`, `wavcraft-edit-bench` | wavcraft-core, soundcraft-audio-io, rayon; windows-sys on Windows | present, core only | A1, I7 | none |
| `crates/testkit/src/bin/wavcraft-bench/main.rs` | The wavcraft-bench command: on one file with no window it times open, waveform peaks, the spectrogram overview and whole-file pictures, and a full sample read, and prints peak and held memory. | `wavcraft-bench FILE` | wavcraft_core, rayon, memory.rs | present, wired | A1, B1, B2, I7 | none; tools/alpha/f05-bench.sh runs it |
| `crates/testkit/src/bin/wavcraft-bench/memory.rs` | The reading of this process's peak and current memory for the bench output. **Platform:** Windows reads the working-set counters through GetProcessMemoryInfo (the crate's one allowed unsafe call); Linux reads VmHWM and VmRSS from /proc; macOS asks ps for the current size and has no peak; other systems report nothing | `Memory`, `read` | windows-sys on Windows; std elsewhere | present, wired | I7 | none |
| `crates/testkit/src/bin/wavcraft-edit-bench/main.rs` | The wavcraft-edit-bench command: on a long file it times cutting and pasting an hour, a gain over a range, and a run of random edits, then undoes all and checks the samples are bit-identical. | `wavcraft-edit-bench FILE [--cache-mb N] [--sessions DIR] [--edits N]` | wavcraft_core, wavcraft_testkit::random_edit, content_hash | present, wired | D1, D5, I7 | none |
| `crates/testkit/src/bin/wavcraft-store-bench/main.rs` | The wavcraft-store-bench command: it times a streamed open into a session store, sequential reads cold and warm, and 10,000 random reads, and reports the store's size on disk and block encodings. | `wavcraft-store-bench FILE [--cache-mb N] [--sessions DIR] [--huge]` | wavcraft_core, wavcraft_testkit::huge_counter | present, wired | A1, I7 | none |
| `crates/testkit/src/lib.rs` | The synthetic test kit: seeded, byte-identical WAV, AIFF and FLAC files (tone, pink noise, clicks, ramp, tone list, 3-hour and RF64 files, speech mixes), a streaming WAV writer, and random-edit and content-hash helpers. | `write_kit`, `KitOptions`, `expected`, `WavWriter`, `random_edit`, `content_hash` | wavcraft_core, soundcraft-audio-io encoder. Callers are its own binaries and tests only | present, wired | A1, D1, D5, I7 | crates/testkit/tests/kit.rs, open.rs, edits.rs |
| `crates/testkit/src/main.rs` | The wavcraft-testkit command: it parses the output folder and the long, huge, seed, speech and noise switches and writes the kit there. | `wavcraft-testkit OUT_DIR [--long] [--huge] [--seed N] [--speech CLEAN.wav --noise NOISE.wav]` | wavcraft_testkit::write_kit | present, wired | A1 | none; tools/alpha/f05-bench.sh and f91-check.sh run it |
| `crates/testkit/tests/data/chained-two-streams-8k-mono.ogg` | A small binary fixture: an Ogg Vorbis file of two chained streams at 8 kHz mono, used to test opening a chained file. |  | embedded with include_bytes by crates/testkit/tests/open.rs | present, core only | A1 | crates/testkit/tests/open.rs |
| `crates/testkit/tests/edits.rs` | Edit acceptance tests on kit files: the ramp file, one-channel Delete and Cut on the left-right file, and an ignored long test that needs the 3-hour file in a release build. |  | wavcraft_testkit, wavcraft_core actions and history | present, core only | C2, D1, D5 | 4 tests in the file, 1 of them ignored |
| `crates/testkit/tests/formats.rs` | Tests that Wave64, CAF and AU files written by save_as read back sample for sample, and that raw headerless import of known bytes gives the expected samples. |  | wavcraft_core save and io | present, core only | after 1.0 | 5 tests in the file |
| `crates/testkit/tests/hostile.rs` | Tests that a session folder that cannot be written is named as the cause, and that hostile WAV headers are refused or opened with a warning and never panic. **Platform:** The unwritable-folder test is Unix only |  | wavcraft_core io::open | present, core only | A1 | 7 tests in the file |
| `crates/testkit/tests/kit.rs` | The kit's own acceptance tests: the same bytes on every run, every file opens through the app's open path, the ramp reads back as its formula, and each file holds what its name says. |  | wavcraft_testkit, wavcraft_core io::open | present, core only | A1 | 9 tests in the file |
| `crates/testkit/tests/mov.rs` | Tests of opening the audio track of MOV, MP4 and M4A files; the inputs are made with ffmpeg and each test skips itself when ffmpeg is missing. |  | wavcraft_core io::open; an ffmpeg program on the path | present, core only | after 1.0 | 5 tests in the file |
| `crates/testkit/tests/open.rs` | Streamed-open acceptance tests: kit files and other layouts compared with a whole-file decode, the ramp oracle, cancel, damaged files and a chained Ogg file. |  | wavcraft_testkit, wavcraft_core io::open, soundcraft-audio-io decode, tests/data/chained-two-streams-8k-mono.ogg | present, core only | A1 | 8 tests in the file |
| `crates/testkit/tests/space.rs` | Tests of the store's size estimate, of refusing an open when the session disk is too full (free space is injected), and of a disk that fills part way through. |  | wavcraft_core io and store | present, core only | A1, I7 | 4 tests in the file |

### Tools (`tools/`)

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `tools/alpha` | 315 scripted feature runs. fNN.py drives the release app through the control channel in a scratch home; fNN.sh builds the app and runs it; -check, -test and -all scripts run fmt, clippy and tests. | `fNN.py --exe <release wavcraft> [--port N] [--keep]`, `bash tools/alpha/fNN.sh`, `fNN-check.sh`, `fix286.py`, `i935.py` | tools/golden.py Control; cargo; CARGO_TARGET_DIR for the .sh wrappers. 129 Python and 186 shell files: 125 plain fNN.py, 126 plain fNN.sh, the rest suffixed variants plus fix286 and i935. Not run by CI | present, wired | I5 | tools/checks/972, 975, 983, 985 and 986 lock five of the scripts as text |
| `tools/checks/1001-metronome-follows-tempo.sh` | Locks issue 1001: the document's tempo is handed to the player's metronome when a document loads and inside set_tempo, with exactly two lines added and none removed. | `bash tools/checks/1001-metronome-follows-tempo.sh` | grep and awk on apps/wavcraft/src/app.rs; git diff --numstat; tools/alpha/f106.py is the end-to-end run | present, core only | after 1.0 | not run by CI |
| `tools/checks/1003-panels-doc-lists-tools.sh` | Locks issue 1003: the PANELS doc line in control.rs lists the Tools panel in registry order, the registry still has that panel docked left, and no panels unit test is deleted or ignored. | `bash tools/checks/1003-panels-doc-lists-tools.sh` | text checks on apps/wavcraft/src/control.rs and panels.rs; runs the panels tests | present, core only | I1, I2 | not run by CI |
| `tools/checks/1006-code-map-freqband.sh` | Locks issue 1006: docs/code-map.md and the spectral view doc comment name the STATE keys and STATUSBAR verbs the code uses; only those two files change, and settings.rs only in comments. | `bash tools/checks/1006-code-map-freqband.sh` | text checks on docs/code-map.md and crates/core/src/settings.rs; git diff | present, core only | B2 | not run by CI; fails since the 2026-11-11 rewrite of docs/code-map.md, which no longer has the rows it reads |
| `tools/checks/1008-clipboards-belong-to-the-window.sh` | Locks issue 1008: the five clipboards and the current-clipboard index move with the window across Open, Switch and Close; it reads the three methods in app.rs. | `bash tools/checks/1008-clipboards-belong-to-the-window.sh` | source checks on apps/wavcraft/src/app.rs; git diff; tools/alpha/f04.py and f12.py are the end-to-end runs | present, core only | D1, A5 | not run by CI |
| `tools/checks/1010-stub-limiter.sh` | Locks issue 1010: the stub limiter is gone and nothing else moved, by comparing a formatting-normalised hash of the file with the locked value. | `bash tools/checks/1010-stub-limiter.sh` | rustfmt and a hash of one source file | present, core only | F2 | not run by CI |
| `tools/checks/970-reverse-mirror-range.sh` | Locks issue 970: the reverse one-shot takes its source frame from mirror_range, no inline copy of the mirror arithmetic is left, and Reverse behaves as before with no new warnings. | `bash tools/checks/970-reverse-mirror-range.sh` | grep and awk on the core oneshot and reverse sources; runs cargo | present, core only | E4 | not run by CI |
| `tools/checks/971-stft-duplicate.sh` | Locks issue 971: crates/core/tests/stft.rs, a byte copy of the STFT tests in noise.rs, is gone, the originals stay, and no other file or manifest changed. | `bash tools/checks/971-stft-duplicate.sh` | git diff against HEAD | present, core only | G2 | not run by CI |
| `tools/checks/972-f03-normalize-effect.sh` | Locks issue 972: tools/alpha/f03.py drives Normalize through the EFFECT verb and not the normalize.db parameter, and the control.rs doc line says where that parameter still applies. | `bash tools/checks/972-f03-normalize-effect.sh` | text checks on tools/alpha/f03.py and apps/wavcraft/src/control.rs | present, core only | E3 | not run by CI |
| `tools/checks/973-delete-silence-job-guard.sh` | Locks issue 973: the DELETE_SILENCE control arm refuses while a background job runs, with the same reply STRETCH gives, after the save check and before any edit. | `bash tools/checks/973-delete-silence-job-guard.sh` | reads the arm's source in apps/wavcraft/src/app.rs; git diff | present, core only | after 1.0 | not run by CI |
| `tools/checks/974-diagnostics-job-guard.sh` | Locks issue 974: DeClip, DeClick, Automatic Click Remover and the Diagnostics repair refuse during a background job, the Diagnostics buttons are disabled then, and scanning keeps no guard. | `bash tools/checks/974-diagnostics-job-guard.sh` | reads function sources in apps/wavcraft/src/app.rs; git diff | present, core only | after 1.0 | not run by CI |
| `tools/checks/975-alpha-state-parser.sh` | Locks issue 975: Control.state() in tools/alpha/f32.py and f03.py splits the STATE answer on the trailing path= first and handles band=, so redo is the bare label. | `bash tools/checks/975-alpha-state-parser.sh` | python3; lifts each state() out of its script and feeds it canned answers | present, core only | D5 | not run by CI |
| `tools/checks/979-session-gen.sh` | Locks issue 979: a pending multitrack clip edit remembers which session it came from, and Return refuses after the session changed. | `bash tools/checks/979-session-gen.sh` | source checks on the app's multitrack code; tools/alpha/f116-session.py is the end-to-end run | present, core only | after 1.0 | not run by CI |
| `tools/checks/980-click-selects-clip.sh` | Locks issue 980: a click on a multitrack clip box selects that clip (sel_clip), and the rest of the click handling is unchanged. | `bash tools/checks/980-click-selects-clip.sh` | source checks on the app's multitrack view | present, core only | after 1.0 | not run by CI |
| `tools/checks/983-history-menu-row.sh` | Locks issue 983: the History panel's menu button sits in a one-line row, and tools/alpha/f78.py reads the PANELS reply by token. | `bash tools/checks/983-history-menu-row.sh` | text checks on the History panel source and tools/alpha/f78.py | present, core only | D5 | not run by CI |
| `tools/checks/985-f83-time-display.sh` | Locks issue 985: tools/alpha/f83.py uses the time_display setting and no longer names time_format or frame_rate. | `bash tools/checks/985-f83-time-display.sh` | text checks on tools/alpha/f83.py | present, core only | B5 | not run by CI |
| `tools/checks/986-f70-pref-protocol.sh` | Locks issue 986: tools/alpha/f70.py speaks the generic PREF protocol, where PREF get answers OK and the value and PREF set answers OK. | `bash tools/checks/986-f70-pref-protocol.sh` | text checks on tools/alpha/f70.py | present, core only | I2 | not run by CI |
| `tools/checks/989-code-map-cd-ui-row.sh` | Locks issue 989: in docs/code-map.md the cd_ui.rs and input_meter.rs rows sit in the first table after mixer.rs, and the second table has two-cell rows only. | `bash tools/checks/989-code-map-cd-ui-row.sh` | text checks on docs/code-map.md | present, core only | after 1.0 | not run by CI; fails since the 2026-11-11 rewrite of docs/code-map.md, which no longer has the rows it reads |
| `tools/checks/993-scrub-uses-tempo.sh` | Locks the app half of issue 993: scrub_drag asks for the snap unit with the document's tempo. | `bash tools/checks/993-scrub-uses-tempo.sh` | one grep on the app source | present, core only | C6 | not run by CI |
| `tools/checks/995-pycache.sh` | Locks issue 995: no compiled Python cache file is tracked, .gitignore ignores such files while keeping its older rules, and nothing else changed. | `bash tools/checks/995-pycache.sh` | git ls-files, git status, .gitignore | present, core only |  | not run by CI |
| `tools/checks/996-f26-doc-comment.sh` | Locks issue 996: one doc comment sits on its own callback test and set_map keeps only its own comment; the file differs from HEAD by that moved line alone. | `bash tools/checks/996-f26-doc-comment.sh` | text checks, git status and git diff on apps/wavcraft/src/player.rs | present, core only |  | not run by CI |
| `tools/checks/999-input-monitor-rename.sh` | Locks issue 999: apps/wavcraft/src/input.rs is renamed to input_monitor.rs byte for byte, the core input_meter.rs keeps its name, and line endings are unchanged. | `bash tools/checks/999-input-monitor-rename.sh` | git blob hashes | present, core only | J4 | not run by CI |
| `tools/golden.py` | The golden task script: it starts the built app in a scratch home, drives open, edits, undo, save, reopen and quit through the control channel, and checks lengths, History labels, the log and the saved file. Its Control class is shared by the alpha scripts. **Platform:** The on-screen window check runs only on macOS when tools/winlist is built; it is skipped on other systems | `Control`, `Control.ask`, `Control.state`, `run`, `edits`, `--exe` | Python standard library; ffprobe; the control verbs TOKEN, STATE, SAVE, QUIT and ACTION; tools/winlist when built. Imported by tools/alpha/*.py; run by tools/alpha/f107.sh | present, wired | A1, A2, D1, D3, D5, I5 | none of its own; tools/checks/975-alpha-state-parser.sh covers copies of state() |
| `tools/licence-texts` | Four standard licence texts (Apache-2.0, BSL-1.0, MIT, MPL-2.0) that the notices generator falls back to when a crate ships no licence file of its own. | `Apache-2.0.txt`, `BSL-1.0.txt`, `MIT.txt`, `MPL-2.0.txt` | read by the fallback function in tools/notices.py | present, wired | I6 | none |
| `tools/notices.py` | The licence gate and notices generator: it reads cargo metadata, fails on a dependency whose licence is not on the allowed list, and writes or checks THIRD-PARTY-NOTICES.txt at the repo root. | `--check`, `--write`, `allowed`, `parse`, `notice_files`, `render` | cargo metadata --locked; tools/licence-texts; its output is embedded by apps/wavcraft/src/notices.rs for Help > About | present, wired | I6 | tools/test_notices.py; run by tools/alpha/f80-check.sh and f80-notices.sh, not by CI |
| `tools/test_notices.py` | Unit tests of the licence gate: which licence expressions pass or fail, how OR and AND combine, and how notice files are found in a crate folder. | `Allowed`, `Files` | tools/notices.py, unittest | present, core only | I6 | 2 test classes; run by hand with python3 -m unittest, not by CI |
| `tools/winlist.swift` | A small macOS program that prints how many on-screen windows a process id owns and the largest window's size. **Platform:** macOS only | `winlist <pid>` | CoreGraphics; called by tools/golden.py when the built binary exists (the binary is git-ignored) | present, wired | I5 | none |

### CI and repo config

| File | What it owns | Key names | Uses | Status | Items | Tests |
|---|---|---|---|---|---|---|
| `.github/workflows/check.yml` | The only CI workflow: on a push to main and on every pull request it runs cargo fmt check, clippy with warnings as errors, and the workspace tests, each on Ubuntu, Windows and macOS runners. **Platform:** Three-OS matrix; only the Linux runner installs the ALSA and Wayland development libraries | `fmt`, `clippy`, `test` jobs | actions/checkout, dtolnay/rust-toolchain, Swatinem/rust-cache; apt packages libasound2-dev, libwayland-dev | present, wired | I5 | none. It does not run tools/checks, tools/alpha, tools/notices.py or the Python unit tests |
| `Cargo.toml` | The workspace manifest: the six member crates, the shared package fields, every pinned dependency version (the SoundCraft crates all at one fork commit), the unsafe-code ban and the dev profile's optimisation levels. | `[workspace] members`, `[workspace.dependencies]`, `unsafe_code = "forbid"`, `clippy::todo = "warn"`, `[profile.dev]` | crates/core, crates/automation, crates/testkit, crates/craft-log, crates/craft-stage, apps/wavcraft; soundcraft-* (git, one rev), symphonia, egui/eframe 0.36, cpal 0.18, rustfft, rayon, vorbis_rs, rustix, arboard | present, wired | I5, I6 | none; the CI workflow builds from it |

## `apps/wavcraft/src/app.rs` by line range (main at 11e49a5, 13,184 lines)

One struct, `WaveApp`, holds the whole window. Almost everything is a method in one `impl WaveApp` block (lines 881 to 11678).

| Lines | What the range does |
|---|---|
| 1-99 | Module comment, imports, `mod spectral`, layout constants (ruler, navigator sizes). |
| 100-515 | Small private types and helpers: `MarqueeDrag`, `PrefPage`, `PrefAction`, `ScrubState`, `Hold`, `MicCheck`, `MicDialog`, the background-work records (`Opening`, `Running`, `Inserting`, `BatchWrite`, `Integrating`, `Making`, `Appending`, `SaveAllRun`, `Saving`), `DocSlot` (one parked document), `Import`, `Meta`, `SpectrumCache`, `HudState`, `NewAudio`, `AppendTo`, `Appended`, and `enum Tool` with its `impl`. |
| 516-844 | `pub struct WaveApp`: every field of the window (document slots, editor, view, selection, player, recorder, panels, settings, keymap, dialogs, worker handles, control channel receiver). |
| 845-880 | `ClipEdit`, `Scrub`, and the include-metadata setting helpers. |
| 886-1133 | `WaveApp::new`: loads settings, session folder (flag, then environment, then setting), keymap, panels, workspaces, recent files, hardware profiles, starts the control channel, opens the file from the command line. |
| 1134-1250 | Look and view settings: `apply_look` (theme, colours), crossfades, snap, amplitude scale, zoom slots, channel layout, `save_settings`. |
| 1251-1518 | Preferences plumbing: `pref_set` (the `PREF` verb and dialog write through it), presets, audio hardware profiles (`remember_hardware`, `activate_setup`), input meter sync, the mic check (start, poll, dialog), `reset_preferences`. |
| 1519-2002 | `preferences_dialog`: every Preferences page. |
| 2003-2442 | Opening audio: `open`, `open_as`, mixdown on open, `finish_opening`, Insert File, Open Many and its prompt queue, Open Append (`start_append`, `finish_appending`). |
| 2443-2862 | Several documents at once: `take_slot`/`put_slot`/`switch_doc`, view sync, New Audio File (and its dialog), Copy to New, Paste to New, Close All, Save All, `run_doc_action`. |
| 2863-3446 | Files panel and friends: `close_doc`, `file_rows`, metadata refresh, reveal in file manager, recent files, import (raw or other), `files_panel`, the `RECENT` and `FILES` request handlers, document tabs, file pickers. |
| 3447-3605 | Video attach and drop, session recovery (`recover_session`, `recover_take`, discard), session store usage and cleanup, session folder picker and setter. |
| 3606-4054 | Panels and docking: show or hide, save layout, floating windows, monitors report, workspaces (snapshot, apply, commands, menu), dock zones, dividers, panel menu, dock drag. |
| 4055-4234 | Time format, snapping of a frame, tempo, sample and hover readout, `op_state` and `window_state` for `STATE`, panel menu items, metadata display fields and dialog. |
| 4235-4503 | Recording glue: monitor attach, offset, `start_recording*`, timed start, `begin_recording`, record into the document (punch), `integrate_take`, `finish_recording`, `quick_record`. |
| 4504-4607 | Settings export, import and reload, the reset-preferences prompt, the About window. |
| 4608-4997 | Playback: `toggle_play`, `start_play`, follow mode, JKL shuttle, nudge, playhead step, scrub (`run_scrub`, `scrub_to`), pause, resume, stop, `poll_transport`, `transport_panel`. |
| 4998-5238 | Command dispatch: `run_command` (keyboard `Command`), extract channels, export ranges, batch run, `run_action` (the action table), statistics start and navigation, system clipboard. |
| 5239-5412 | `run_window_action`: the actions that need the window (dialogs, panels, view, tools). |
| 5413-5831 | Dialogs and small panels: Import Raw, Mix Paste, Tempo, Insert Silence, custom frame rate, `time_panel`, `selection_view_panel`, `time_box`. |
| 5832-5993 | Menu rows as data: `item_enabled`, `run_item`, `menu_rows`, toolbar, tools panel, and the `CONTEXTMENU` and `TOOLBAR` verbs. |
| 5994-6111 | History panel: draw, go to a step, delete a step, arrow keys. |
| 6112-6319 | Metadata: apply an edit, metadata sets (`METASET`), the Metadata panel body. |
| 6320-6746 | Effects: the effect dialog, preview start and end, generators and the Generate menu, `run_effect`, the Effects Rack (`apply_rack`, `rack_ui`, `sync_rack`), DeClip, DeClick, Diagnostics scan and repair, Markers, Playlist and Properties panels, the clear-history prompt. |
| 6747-6880 | `report` (error line), editing a multitrack clip's source in the waveform editor and returning to the session, `rebuild` (spectrogram and overview rebuild). |
| 6881-7112 | Background jobs: `start_job`, Convert Sample Type, Interpret Sample Rate, their dialogs, Match Loudness finish and panel, `finish_job`, `finish_rebuild`. |
| 7113-7406 | Saving: `unsaved`, `save`, `save_with` (format, sample type, metadata, worker thread), `save_copy`, `finish_saving`. |
| 7407-7707 | Export window (formats, size estimate, presets), the `SAVEOPTS` and `EXPORTPRESET` verbs, the close prompt (save, discard, cancel). |
| 7708-7919 | Selection keys, pointer gestures as data (`gesture`, the `GESTURE` verb), `playback_request` (the `PLAYBACK` verb), metronome and its verb. |
| 7920-9604 | `control_request`: the control channel's verb table, one match arm per verb (140 distinct first words, `STATE` first). `MICCHECK` and `VIDEO frame` are answered earlier, in `logic`. |
| 9605-9690 | Playlist playback and the `PLAYLIST` and `MARKER` verbs. |
| 9691-10074 | Edit menu, shortcut changes and shortcut sets (requests and dialog), Effects menu, Recent menu, language switch, `action_text`. |
| 10075-10487 | `menu`: the whole menu bar, with the v1 gates on multitrack, CD layout, batch and video entries. The top-level menus are File, Edit, Multitrack, Window, Effects, Generate, View, Video and Help. |
| 10488-10677 | Status bar: free disk space, `status` (fields, job progress, error line). |
| 10678-10965 | Editor chrome: right click, spectral band (`set_band`), pointer gesture pick, tools bar, the heads-up display, zoom panel, zoom navigator. |
| 10966-11678 | `editor`: the central view. Time ruler, amplitude ruler, waveform lanes, spectral display with frequency ruler and divider, markers, selection and edge drags, fade handles, marquee and lasso, scrubbing, wheel zoom. |
| 11680-11693 | `panel_menu_button` helper (a free function, after the `impl` block). |
| 11694-11838 | `impl eframe::App`: `logic` runs every frame, shown or not. It collects worker results, ticks recording, transport, meters, recovery, batch, playlist, and answers control channel requests. `on_exit` saves the window state. |
| 11839-12171 | `ui`: per-frame drawing. Keyboard handling (history arrows, panel keys, record key, JKL), Shift-held reset, drag and drop, then menu, status, panels, editor, and every dialog. |
| 12172-12740 | Free helpers: reset of preference files, Import Raw parsing, pressed-command lookup, preference widgets, ruler and marker drawing, colours and palette, preview seed, save-argument parsing (`parse_save_args`, `take_quality`, `take_meta`), test-only save hooks. |
| 12741-13184 | Tests: 30 in-file tests in four modules, plus six test files pulled in with `#[path]` (39 more tests) from `apps/wavcraft/src/`. |

## Tests

One row per test file. A test that is ignored is counted in "Tests"; a stub's tests are ignored until the stub is filled.

### App tests (`apps/wavcraft/src/*_tests.rs`)

| File | What it locks | Tests | Items | Covers |
|---|---|---|---|---|
| `apps/wavcraft/src/f48_device_change_tests.rs` | Locked tests for the player under the silent sink: each simulated device change renames the sink and counts a reopen, playback continues from the same place, and replacing the audio with nothing stops it. | `the_sink_name_counts_the_simulated_device_changes`, `a_simulated_device_change_keeps_the_play_going_from_the_same_position`, `replacing_with_nothing_stops_playback` | C5 | player.rs (declared there as a test-only module); wavcraft_core::DocState |
| `apps/wavcraft/src/i3_shortcut_help_tests.rs` | Locked tests for the shortcut list under Help: one test per default shortcut through the same action path, the rows in menu order with nothing unbound or extra, and the Windows and Mac key labels as drawn. The two functions they test (`shortcut_help_rows`, `shortcut_help_content`) are still stubs in app.rs, so the Help list itself is not built. | `i3_help_rows_are_in_menu_order_with_no_unbound_or_extra_rows`, `i3_help_content_draws_windows_labels_and_keys`, `i3_help_content_draws_mac_labels_and_keys` | I3 | app.rs (declared there as a test-only module); wavcraft_core::shortcuts, actions; egui |
| `apps/wavcraft/src/issue1000_tests.rs` | Locked tests that a fresh Shift+Space press starts a recording once, that held-key repeats do nothing and are consumed so Play/Stop never sees them, and that the record offset preference row has real text. | `a_fresh_shift_space_press_acts`, `a_held_shift_space_acts_once_over_a_press_and_many_repeats`, `a_plain_space_press_does_not_record_and_still_reaches_play_stop` | J1, C5 | app.rs (`consume_record_now`, `pressed_commands`); egui |
| `apps/wavcraft/src/issue1002_tests.rs` | Locked tests that the tools bar labels come from the keymap: the bar order, the default keys, Mac key text, a rebound or unbound tool, and no hard-coded key letters in the source. | `the_tools_come_in_the_bars_order`, `a_rebound_tool_shows_its_new_key`, `the_tools_bar_source_has_no_hard_coded_key_letters` | I3 | app.rs (`tool_labels`); wavcraft_core::shortcuts::Keymap |
| `apps/wavcraft/src/issue366_tests.rs` | Locked tests that opening another file asks before discarding unsaved edits. They read the app source to check that Ctrl+O and a dropped file go through the prompt and that OPEN_PICK and OPEN_ANSWER exist. | `OPEN_PICK`, `OPEN_ANSWER`, `request_open` | A1, I4 | app.rs and control.rs source text |
| `apps/wavcraft/src/issue369_tests.rs` | Tests that Save and close does not close a document that changed while its save ran. Three check the decision; four run the real window headless with a save on a worker thread. | `closes_after_save`, `finish_saving`, `an_edit_confirmed_while_save_and_close_runs_keeps_the_document_open_and_unsaved` | A2, I4 | app.rs (`WaveApp::new` on a headless context); wavcraft_core::DocState; egui |
| `apps/wavcraft/src/issue377_tests.rs` | Tests that Play pressed right after an edit plays the edited audio, not the buffer the player held before its background rebuild finished. | `silent_player`, `a_window` | C5, D1 | app.rs, player.rs; wavcraft_core |

### Integration tests (`crates/*/tests/`)

| File | What it locks | Tests | Items | Covers |
|---|---|---|---|---|
| `crates/core/tests/976_take_cut_keeps_gain_envelope.rs` | Locks that the head and tail left of a clip a take partly covers keep playing the old clip's gain envelope, punched or not. (present, out of v1) | 4 tests, 0 ignored | #976, after 1.0 | mtrecord, multitrack, soundcraft_model |
| `crates/core/tests/977_multichannel_arm_route.rs` | Locks the input route label when a track of n channels is armed at input c, and that the record plan takes n inputs in a row. (present, out of v1) | 9 tests, 0 ignored | #977, after 1.0 | mtrecord::input_label, multitrack |
| `crates/core/tests/981_visible_clip_at.rs` | Locks that a lane click only finds a clip the type filter shows, through Multitrack::visible_clip_at. (present, out of v1) | 6 tests, 0 ignored | #981, after 1.0 | multitrack |
| `crates/core/tests/987_save_format_parse.rs` | Locks that SaveFormat::parse reverses SaveFormat::key for all four formats, and that Cargo.toml keeps a comment above the symphonia line (reads source text). | 8 tests, 0 ignored | #987, A2 | save::SaveFormat |
| `crates/core/tests/990_autosave_markers.rs` | Locks that the autosave journal carries each undo step's markers, so crash recovery returns every step with its own markers and next marker id. | 7 tests, 0 ignored | #990, I4, H4, A6 | autosave, History, Markers, store |
| `crates/core/tests/991_export_whole_range.rs` | Locks that exporting a range marker writes fmt, data and the slice's chunks the same way whether or not the range covers the whole file. | 7 tests, 0 ignored | #991, A6 | export, io, Markers |
| `crates/core/tests/993_unit_at_bars_beats.rs` | Locks that dragging a digit group of a Bars and Beats time steps one bar, beat or tick at the document's tempo. | 8 tests, 0 ignored | #993, B5 | tempo, view::TimeFormat |
| `crates/core/tests/a4_dither.rs` | Locks TPDF dither on bit depth reduction, the dithered WAV save, and the settings remembering the dither choice, with hard-coded expected values. | 16 tests, 0 ignored | A4, #12 | save, settings, io |
| `crates/core/tests/ab_compare.rs` | Locks A/B compare of effect settings: two sets, the active side, flip, copy across, and applying the active set. | 17 tests, 0 ignored | roadmap B169, E6 | effects::AbCompare, store |
| `crates/core/tests/actions.rs` | Locks the action table and the windowless Editor: unique names, cut and copy rules, undo, selection moves, zero crossings, marker actions, Insert Audio from File and hover text keys. | 39 tests, 0 ignored | C1, C2, C5, D3, D5, D6, H1, H2, H3, H4 | actions, doc, store, shortcuts |
| `crates/core/tests/amplify.rs` | Locks Amplify: per-channel dB gain, linked sliders by default, gain exact to 0.001 dB across block joins, float headroom kept until save. Its header still says the cases are ignored, but none is. | 9 tests, 0 ignored | E2, #28 | effects::Amplify, actions, io, save |
| `crates/core/tests/appearance.rs` | Locks the Appearance colour settings: hex parsing, colour keys, and named presets saved, loaded, deleted and kept through the settings file. | 6 tests, 0 ignored | feature f94, M3 | appearance, settings |
| `crates/core/tests/append.rs` | Locks Open Append to the current or a new document as one edit, with the rate check and channel duplication rules. | 6 tests, 0 ignored | feature f57, A1 | doc::append, io, store |
| `crates/core/tests/atomic.rs` | Locks the crash-safe write helper: a new or replaced target gets all bytes, and a failed write keeps the old file and leaves no temp file. | 6 tests, 0 ignored | A2, roadmap C324 | atomic::write_atomic |
| `crates/core/tests/automation_shape.rs` | Locks the linear, step and hold segment shapes between automation points. (present, out of v1) | 15 tests, 0 ignored | roadmap C179, after 1.0 | multitrack::automation_shape |
| `crates/core/tests/autosave.rs` | Locks Auto Save and crash recovery: the journal of pieces and history is durable, a crashed session comes back whole, the sweep never removes a recoverable or live session, and a failed recovery changes nothing. | 14 tests, 0 ignored | I4 | autosave, History, doc::edit, store |
| `crates/core/tests/b4_levels_scale.rs` | Locks the Levels meter's 60 dB scale and its colour zones with crossovers at -18 and -6 dBFS. | 6 tests, 0 ignored | B4, #118 | meters::levels_scale |
| `crates/core/tests/band_only.rs` | Locks the band filter behind play only the selected frequencies, against Butterworth gains worked out by hand. | 25 tests, 0 ignored | roadmap A319, C4 | dsp::band_only |
| `crates/core/tests/batch.rs` | Locks Batch Process on three files with one unreadable: outcomes, no overwrite of inputs or existing files, cancel leaves no partial file, output names. (present, out of v1) | 6 tests, 0 ignored | feature f73, after 1.0 | batch, io, save |
| `crates/core/tests/batch_cli.rs` | Locks the batch command-line parser: flags in any order, the equals form, defaults, errors and the usage text. No file is touched. | 28 tests, 0 ignored | roadmap C239, N2 | batch_cli, batch |
| `crates/core/tests/cd.rs` | Locks the CD layout: CUE times to the frame, ISRC checks, total time against the disc limit, and the exported WAV. (present, out of v1) | 8 tests, 0 ignored | feature f121, after 1.0 | cd, store |
| `crates/core/tests/cd_fade.rs` | Locks CD track fades: length limits, linear gain, bit-identical output with no fades, and track starts with pauses counted. (present, out of v1) | 22 tests, 0 ignored | roadmap C245, after 1.0 | cd::fade, cd |
| `crates/core/tests/chanmap.rs` | Locks the channel map and buffer size logic behind the Audio Hardware and Channel Mapping preferences. | 6 tests, 0 ignored | J5 | chanmap, settings |
| `crates/core/tests/channel_mixer.rs` | Locks the Channel Mixer: identity, swap, mono sum, invert, mono pass-through, parsing, a selection and one undo. | 7 tests, 0 ignored | roadmap B027, after 1.0 | effects, actions::Editor |
| `crates/core/tests/chunks.rs` | Locks that unknown RIFF chunks and Broadcast Wave metadata survive a save in order, and that the metadata option drops them without losing them. | 11 tests, 0 ignored | A2, roadmap A272, A265, A256, A036 | save, io, actions |
| `crates/core/tests/clip_guard.rs` | Locks Amplify's clip guard and the Calculate Now headroom on a document longer than one chunk. | 8 tests, 0 ignored | E2, roadmap B026 | effects::amplify_guarded, headroom_db |
| `crates/core/tests/clipboards.rs` | Locks the five internal clipboards and their actions, and that a clip written as a WAV for the system clipboard reads back the same. | 5 tests, 0 ignored | D1, roadmap A152, A153 | clipboard, actions, doc::edit |
| `crates/core/tests/clipboards_labels.rs` | Locks clipboard menu labels, Paste disabled on an empty clipboard, single-channel clips, and that switching clipboards is not the repeated command. | 5 tests, 0 ignored | D1, roadmap A152, A153 | clipboard, actions |
| `crates/core/tests/convert.rs` | Locks Convert Sample Type, Interpret Sample Rate and the conversion presets: resampled sines keep level and length, tones above the new Nyquist are removed. | 14 tests, 0 ignored | A4 | convert, actions::Editor, store |
| `crates/core/tests/declick.rs` | Locks the DeClicker and Automatic Click Remover: the scan finds planted clicks within 2 ms, the repair cuts the error and leaves the rest bit-identical. | 8 tests, 0 ignored | roadmap B046, B072, after 1.0 | diagnostics::declick, actions |
| `crates/core/tests/declip.rs` | Locks the DeClipper: the scan finds every clipped run across a chunk join, and the repair cuts the error by 70 percent as one undo step. | 6 tests, 0 ignored | roadmap B045, K4 | diagnostics::declip, actions |
| `crates/core/tests/deesser.rs` | Locks the DeEsser through the effect seam: registered with settings, cuts its band, and keeps state across 64 K-frame chunks. | 4 tests, 0 ignored | F2, roadmap B028 | effects, store |
| `crates/core/tests/dehummer.rs` | Locks the DeHummer: 50 or 60 Hz and harmonics are cut, other tones stay, state runs across chunk joins, one undo step. | 6 tests, 0 ignored | roadmap B073, K4 | effects::DeHummer, actions::Editor |
| `crates/core/tests/delete_silence.rs` | Locks the silence scan and Delete Silence: only the long gaps are found, the excess is removed, every tone sample is kept, undo is bit-exact. | 5 tests, 0 ignored | roadmap B047, L4 | diagnostics::silence, actions |
| `crates/core/tests/dereverb.rs` | Locks DeReverb: the tail between bursts drops, bursts barely change, amount 0 passes through, and chunking does not change the result. | 5 tests, 0 ignored | roadmap B074, after 1.0 | effects, actions, rustfft |
| `crates/core/tests/diagnostics_panel.rs` | Locks repairing a chosen subset of the regions a diagnostics scan found, and that repairing all equals the whole-file repair. | 4 tests, 0 ignored | roadmap B050, K4 | diagnostics, actions::Editor |
| `crates/core/tests/dither.rs` | Locks dither and noise shaping measured on spectra and correlations, alone and through Convert Sample Type and a save. | 8 tests, 0 ignored | A4, roadmap A163 | dither, convert, rustfft |
| `crates/core/tests/dynamics.rs` | Locks Dynamics (gate, compressor, limiter): the static curve, attack and release time constants, and chunked runs equal to one pass. | 12 tests, 0 ignored | F2, roadmap B029 | effects, actions, store |
| `crates/core/tests/e1_fade_curve.rs` | Locks the linear, logarithmic and cosine fade gain formulas to 0.01 dB and the handle drag-to-curve rule. | 4 tests, 0 ignored | E1, #27 | dsp::fade |
| `crates/core/tests/e1_fade_effect.rs` | Locks Fade In and Fade Out as registered effects and actions: 0.01 dB accuracy, selection and channel masking, undo, repeat, seamless across blocks. | 5 tests, 0 ignored | E1, #27 | effects, dsp::fade, actions |
| `crates/core/tests/e1_fade_handles.rs` | Locks the top-corner waveform fade handles: hit test within 8 px and the range and curve an inward drag gives. | 5 tests, 0 ignored | E1, #27 | fade_handles, view |
| `crates/core/tests/e5_repeat_last.rs` | Locks Repeat Last Command keys and that repeats reuse the original settings. The dialog checks run against a copy of the logic in the test file, because Editor::repeat_dialog is a todo on main. | 7 tests, 0 ignored | E5, #31 | actions, effects, shortcuts |
| `crates/core/tests/edit_span.rs` | Locks where the playhead goes when the document is edited under it: EditSpan::between and map_position. | 9 tests, 0 ignored | C5, roadmap A208 | doc::span, doc::edit |
| `crates/core/tests/edits.rs` | Locks every edit against a reference model, undo and redo, the disk cap, silence writing nothing, streamed and cancellable replace_range, crossfades, and paste channel rules on 16-bit documents. | 18 tests, 0 ignored | D1, D3, D4, D5, D7, C2, I7 | doc::edit, History, actions, store, proptest |
| `crates/core/tests/effect_params.rs` | Locks effect parameter descriptions, key=value parsing, the Gain example effect, and Repeat Last Command carrying an effect's settings. | 9 tests, 0 ignored | E5, F3, roadmap B006, A154 | effects, actions |
| `crates/core/tests/effect_plate.rs` | Locks the plate reverb: six settings, range checks, wet 0 returns the input, and tail decay through the stateful run. | 10 tests, 0 ignored | roadmap B086, after 1.0 | effects, store |
| `crates/core/tests/effect_reverb_delay.rs` | Locks Reverb and Delay: the echo lands at the set time with the set feedback, also across a chunk join, and the reverb tail decays. | 5 tests, 0 ignored | feature f124, after 1.0 | effects, store |
| `crates/core/tests/effect_run.rs` | Locks that an effect Run keeps its state across the chunks of replace_range, and that the new parameter kinds describe themselves. | 2 tests, 0 ignored | F1, F2 | effects::{Effect, Run, RunContext} |
| `crates/core/tests/effects.rs` | Locks the effect seam with Invert as reference: the registry, action table entries, undo labels, and processing the selection, the whole file or only enabled channels. | 12 tests, 0 ignored | E4, C2 | effects, actions, store |
| `crates/core/tests/encodings.rs` | Locks 16-bit and 24-bit integer blocks beside float blocks in the same packs: edge values, cold and warm reads, byte counts and pack deletion. | 3 tests, 0 ignored | I7, A1 | store, doc |
| `crates/core/tests/episode_check.rs` | Locks the Episode Check detectors on synthetic speech: each planted fault is reported within 50 ms, clean audio reports nothing, runs across chunk joins come out whole. | 7 tests, 0 ignored | K2 | check, spectrogram::Samples |
| `crates/core/tests/eq_graph.rs` | Locks the EQ graph maths: frequency and dB to pixels and back, nearest node, and node drag. | 17 tests, 0 ignored | F2 | dsp::eq_graph |
| `crates/core/tests/export.rs` | Locks Extract Channels to Mono Files and Export Audio of Range Markers as sample-identical to the source, with their file names. | 6 tests, 0 ignored | feature f79, A6 | export, Markers, io |
| `crates/core/tests/export_options.rs` | Locks the estimated file size for every format, export presets, and the AIFF kinds. | 9 tests, 0 ignored | feature f87, A2 | save, export_presets, io |
| `crates/core/tests/fade_curve.rs` | Locks the four fade curve shapes, the in-place fades and the drag-to-curve pick. Core maths only. | 22 tests, 0 ignored | E1, roadmap A077 | dsp::fade_curve |
| `crates/core/tests/fft_filter.rs` | Locks the FFT Filter: a flat curve returns the input, a low-pass curve removes 6 kHz, chunked output equals one pass. | 3 tests, 0 ignored | roadmap B054, after 1.0 | effects, actions::Editor |
| `crates/core/tests/filters.rs` | Locks the measured frequency response of the parametric EQ, high-pass, low-pass and notch, DC removal, and filter memory across chunks. | 9 tests, 0 ignored | F2 | effects, soundcraft_dsp::biquad, actions |
| `crates/core/tests/fixtures/q367-full.flac` | Intact FLAC fixture for the truncated-file warning tests that live inside crates/core/src/io.rs. | 0 tests (fixture) | A1, #367 |  |
| `crates/core/tests/fixtures/q367-truncated.flac` | Cut-short FLAC fixture: opening it must warn and give fewer frames, in the tests inside crates/core/src/io.rs. | 0 tests (fixture) | A1, #367 |  |
| `crates/core/tests/freq_analysis.rs` | Locks Frequency Analysis: a full-scale sine peaks at 0 dB at its frequency, two tones at their levels, flat white noise, range limits and cancel. | 9 tests, 0 ignored | roadmap A309 to A312, after 1.0 | analysis::freq, store |
| `crates/core/tests/generate.rs` | Locks the tone and noise generators through the editor: insert at the cursor, replace the selection, one undo step, continuous across chunks. | 5 tests, 0 ignored | feature f21, after 1.0 | generate, actions::Editor |
| `crates/core/tests/graphic_eq.rs` | Locks the Graphic EQ: all zero is bit-identical, a band boost is local, ripple is bounded, master gain applies. | 10 tests, 0 ignored | roadmap B055, after 1.0 | effects, actions::Editor |
| `crates/core/tests/history_restore.rs` | Locks History::restore: the undo history rebuilt from journal steps keeps every step, the cursor and the disk count, and a new edit drops the redo steps. | 6 tests, 0 ignored | I4, D5, #86 | doc::history, store |
| `crates/core/tests/i3_time_selection.rs` | Locks that the Time Selection Tool action is named tool.select and bound to T by default. | 1 test, 0 ignored | I3, C1 | actions, shortcuts |
| `crates/core/tests/incremental_overviews.rs` | Locks that waveform and FFT overviews after an edit, undo or redo equal a full build, and that small edits read a bounded amount whatever the document length. | 7 tests, 0 ignored | B1, B2, I7 | document, spectrogram, doc::span, History |
| `crates/core/tests/issue1005_apply_to_spectrogram.rs` | Locks that Settings::apply_to_spectrogram sets the window once and that the window preference reaches the analysis parameters (also reads the source text). | 5 tests, 0 ignored | #1005, B2 | settings, SpectrogramParams |
| `crates/core/tests/issue1009_open_into_rate.rs` | Locks that Open Append refuses a W64, CAF or AU file at another sample rate before anything is written to the store. | 5 tests, 0 ignored | #1009, A1 | io, store |
| `crates/core/tests/j3_take_recover.rs` | Locked tests for repairing a killed take file in place. All are ignored, because repair_take_file and takes_needing_repair are todo stubs on main. (stub) | 6 tests, 6 ignored | J3, #116 | take_recover, take_repair |
| `crates/core/tests/j3_take_repair.rs` | Locked tests for rebuilding a take's 44-byte WAV header. All are ignored, because repair_header is a todo stub on main. (stub) | 8 tests, 8 ignored | J3, #116 | take_repair |
| `crates/core/tests/j4_take_readout.rs` | Locks the recording readout text: elapsed time as a clock, file size, time left from free space, unknown free space, dropped samples. | 7 tests, 0 ignored | J4, #116 | take_readout |
| `crates/core/tests/j5_input_device.rs` | Locks how the input device name is kept in the settings file: trimmed, Unicode and symbols kept, last line wins, empty restores the default. | 7 tests, 0 ignored | J5 | settings |
| `crates/core/tests/jobs.rs` | Locks the background job runner (progress, cancel, errors) and float headroom above 0 dBFS kept until save. | 8 tests, 0 ignored | E2, roadmap B003, B012 | jobs, doc::edit, History, save |
| `crates/core/tests/journal_frame.rs` | Locks the autosave journal's checksum framing: fnv1a against published vectors, seal, unseal, and refusal of a damaged or cut journal. | 4 tests, 0 ignored | I4, #86 | autosave::{fnv1a, seal, unseal} |
| `crates/core/tests/locked_922_settings_newlines.rs` | Locks that Settings::to_text writes newlines as escapes, so a CRLF checkout cannot change the text, and that its output is unchanged (reads the source text). | 3 tests, 0 ignored | #922, I5 | settings |
| `crates/core/tests/loudness.rs` | Locks loudness measurement against values the standards fix: a -20 dBFS stereo sine reads -20 LUFS, gating, loudness range, cancel. | 7 tests, 0 ignored | G3, G4, feature f26 | loudness, store |
| `crates/core/tests/master_mode.rs` | Locks the master output mode (mono, stereo, 5.1): channel counts, format mapping, parsing, and the folds down to mono. (present, out of v1) | 18 tests, 0 ignored | roadmap C171, after 1.0 | multitrack::master_mode, mix::downmix |
| `crates/core/tests/m5_effects_listed.rs` | Locks that a release build lists no DeHummer or DeReverb, a debug build lists every effect, and the listed indexes are the registry's. | 1 test, 0 ignored | #1051, M5 | effects |
| `crates/core/tests/match_loudness.rs` | Locks Match Loudness: the result measures within tolerance of the target and the true peak stays under the ceiling, with and without the limiter. | 11 tests, 0 ignored | G3, #38 | match_loudness, loudness, History |
| `crates/core/tests/match_volume.rs` | Locks Match Volume: the gain is the reference loudness minus the selection's, and no gain when either side has no loudness. | 10 tests, 0 ignored | roadmap B041, after 1.0 | loudness::{match_volume, match_gain_db} |
| `crates/core/tests/mix_paste.rs` | Locks Mix Paste sample by sample: every mode, volumes, invert, crossfade, loops, the history step and the streaming path. | 12 tests, 0 ignored | feature f22, D1 | doc::edit::{MixMode, MixOptions}, store |
| `crates/core/tests/multichannel.rs` | Locks surround and many-channel files: open and save keep the channel count, samples and speaker mask, and an edit on some channels leaves the others alone. | 7 tests, 0 ignored | C2, D1, roadmap A134 | channels, io, save, doc::edit |
| `crates/core/tests/multitrack_prefs.rs` | Locks the Multitrack and Multitrack Clips preferences in the settings file: defaults, ranges, get and set by key. (present, out of v1) | 25 tests, 0 ignored | feature a283, after 1.0 | settings::multitrack, settings::video |
| `crates/core/tests/name_template.rs` | Locks file name templates: padded index, date and format fields, doubled braces, illegal characters, reserved names and the length cut. | 10 tests, 0 ignored | after 1.0 | name_template |
| `crates/core/tests/newdoc.rs` | Locks new documents, empty or from a clip in their own store, and the actions that make them. | 6 tests, 0 ignored | D2, feature f30 | doc::newdoc, actions, soundcraft_audio_io |
| `crates/core/tests/noise.rs` | Locks the STFT engine, noise prints and Noise Reduction: analysis then resynthesis is the input to -100 dB, and chunking or grid origin does not change the result. | 8 tests, 0 ignored | G1, G2 | noise, effects, actions, shortcuts |
| `crates/core/tests/normalize.rs` | Locks Normalize: the peak equals the target to 0.01 dB in percent and dB, all channels equally or each alone, DC Bias Adjust, selections and masked channels. | 10 tests, 0 ignored | E3, #29 | effects, actions, store |
| `crates/core/tests/oneshot_effects.rs` | Locks Reverse, Amplify, Normalize, Fade In and Fade Out through the action table on documents longer than one chunk. | 7 tests, 0 ignored | E1, E2, E3, E4 | actions, History, store |
| `crates/core/tests/peak_modes.rs` | Locks static and dynamic peak modes, the settable hold time and valley tracking on Levels. | 15 tests, 0 ignored | B4, roadmap A214 | levels |
| `crates/core/tests/phase_correction.rs` | Locks Automatic Phase Correction: the lag between two channels is measured and removed, and a negative correlation is flipped. | 8 tests, 0 ignored | roadmap B076, after 1.0 | effects, actions |
| `crates/core/tests/phase_scope.rs` | Locks the phase scope numbers: goniometer points, balance, and mid and side levels. | 30 tests, 0 ignored | roadmap A040, after 1.0 | meters::phase_scope |
| `crates/core/tests/pitch_presets.rs` | Locks the Pitch Shifter preset table: fifteen named intervals, lookup rules, and a preset driving the real shifter end to end. | 16 tests, 0 ignored | roadmap B111, after 1.0 | effects::pitch_presets, effects |
| `crates/core/tests/pitch_saturate.rs` | Locks the Pitch Shifter (a 440 Hz tone up an octave reads 880 Hz, same length) and the Saturator staying under its ceiling. | 6 tests, 0 ignored | feature f125, after 1.0 | effects, soundcraft_dsp::plugins::pitch |
| `crates/core/tests/pref_pages.rs` | Locks the Data, Markers and Metadata, and Playback and Recording preference settings: defaults, typed getters, file round trip, bad values refused. | 5 tests, 0 ignored | J2, feature f67 | settings |
| `crates/core/tests/preferences.rs` | Locks the crossfade and device preferences in the settings file, get and set by key, and resetting the file aside. | 4 tests, 0 ignored | D7, J5, feature f20 | settings, bundle |
| `crates/core/tests/probe.rs` | Locks reading a file's header without opening it: format, length and the title from INFO or the BWF description. | 6 tests, 0 ignored | A5, feature f63 | probe |
| `crates/core/tests/punch_again.rs` | Locks Punch Again: the last completed punch range is remembered and armed again. (present, out of v1) | 23 tests, 0 ignored | roadmap C041, after 1.0 | mtpunch, mtrecord::Punch |
| `crates/core/tests/rack.rs` | Locks the Effects Rack applied to a document: one labelled undo step, only the selection and active channels, an inactive rack adds no step. | 3 tests, 0 ignored | F1 | rack::RackConfig, actions |
| `crates/core/tests/rack_reset.rs` | Locks that a rack reset on seek or restart leaves clean effect state, equal to a freshly built rack. | 4 tests, 0 ignored | F1, #931 | rack::RackProcessor, effects |
| `crates/core/tests/rack_tail.rs` | Locks Apply with tail: the rack rings out past the selection end, as one undo step that restores the length. | 8 tests, 0 ignored | F1, roadmap B019 | rack, actions, doc::edit |
| `crates/core/tests/rec_edit.rs` | Locks the edit a finished take makes in an open document: insert, replace a selection, overwrite at a cursor, and latency offset. | 18 tests, 0 ignored | J7, roadmap C026, C027, C030 | doc::rec_edit, History, store |
| `crates/core/tests/recent.rs` | Locks the Open Recent list: newest first, repeats move to the front, ten kept, survives a restart, remove and clear. | 6 tests, 0 ignored | A5, feature f63 | recent |
| `crates/core/tests/recent_sessions.rs` | Locks the recent sessions list: path normalising, newest first, capped, deduped, prunable, kept as JSON. | 25 tests, 0 ignored | roadmap C080, after 1.0 | recent_sessions |
| `crates/core/tests/record_count_in.rs` | Locks the count-in before a record: pre-roll length, the frame recording really starts at, and the clicks to sound. | 20 tests, 0 ignored | roadmap C202, after 1.0 | metronome::record_count_in, tempo |
| `crates/core/tests/reverse.rs` | Locks bit-exact Invert and Reverse over a selection or the whole file, across block joins, per active channel, with undo and repeat. | 7 tests, 0 ignored | E4, #30 | actions, doc::edit, store |
| `crates/core/tests/rms_reading.rs` | Locks the RMS level reading beside the peak meter: the dB conversion, the 300 ms default window, and the floor. | 20 tests, 0 ignored | B4, roadmap A217 | levels::rms |
| `crates/core/tests/rms_settings.rs` | Locks the RMS settings in Amplitude Statistics: sine or square reference offset, account for DC, and window width, on hand-computed signals. | 13 tests, 0 ignored | G4, roadmap A315 | stats, store |
| `crates/core/tests/ruler.rs` | Locks the multitrack time ruler model: nice steps, tick positions, labels and input limits, against hand-worked values. | 35 tests, 0 ignored | roadmap C083, after 1.0 | ruler |
| `crates/core/tests/save.rs` | Locks WAV save bit-identical per sample type, edits then save, failed saves keeping the old file, temp file safety, and AIFF, FLAC and RF64 saves. | 24 tests, 0 ignored | A2, A6, H4 | save, io, actions, History |
| `crates/core/tests/save_part.rs` | Locks Save Selection As and Export: the written samples equal the source range, only enabled channels are written, and a failed write leaves no file. | 6 tests, 0 ignored | A2, feature f43 | save::{Part, write_file}, io |
| `crates/core/tests/sc_source.rs` | Locks a document used as a multitrack clip source: it reads the same samples, and a failed read reports zero frames. | 2 tests, 0 ignored | feature f102, after 1.0 | sc_source, soundcraft_model |
| `crates/core/tests/session_notes.rs` | Locks a session's title and notes: length, trimming, control characters, Unicode, and that an older session file still loads. (present, out of v1) | 21 tests, 0 ignored | roadmap C094, after 1.0 | multitrack::notes, soundcraft_model |
| `crates/core/tests/session_properties.rs` | Locks changing a multitrack session's name, tempo, time signature, rate and bit depth later, and what counts as holding audio. (present, out of v1) | 41 tests, 0 ignored | roadmap C078, after 1.0 | multitrack, soundcraft_model |
| `crates/core/tests/sessions.rs` | Locks disk used by sessions and Clean up old sessions: never a live session, never a recoverable one, never one from another machine, nothing young. | 5 tests, 0 ignored | I4, I7, roadmap A281 | sessions, autosave, store |
| `crates/core/tests/settings.rs` | Locks the settings file: round trip, unknown keys ignored, panel layout kept, Auto Save and cache preferences, and the session folder choice with fall-back. | 43 tests, 0 ignored (42 compile on any one OS: one is cfg(unix), one is cfg(windows)) | I2, I1, C2, D7, H2 | settings, panels |
| `crates/core/tests/shortcut_overrides.rs` | Locks my shortcut overrides: assign, conflicts, clear, restore defaults, key file round trip, and a broken key file being ignored. | 11 tests, 0 ignored | I3, roadmap A291 | shortcuts, actions |
| `crates/core/tests/shortcut_sets.rs` | Locks saved shortcut sets (save, load, list, delete, name checks) and the visual keyboard's logic. | 13 tests, 0 ignored | I3, roadmap A292, A293 | shortcut_sets, keyboard, shortcuts |
| `crates/core/tests/shortcuts.rs` | Locks the default shortcut table: each chord resolves to the right command, no chord on two commands, every bound action exists. | 8 tests, 0 ignored | I3 | shortcuts, actions::TABLE |
| `crates/core/tests/snap.rs` | Locks snapping to the ruler, markers and zero crossings, the coarse and fine ruler targets, and channel enable, without a window. | 15 tests, 0 ignored | C3, C2, D6 | snap, actions, doc::edit |
| `crates/core/tests/source_link.rs` | Locks the link from a mixed-down file back to its session in one INFO tag: field limits in bytes, encode, parse, write and read. | 30 tests, 0 ignored | roadmap A326, after 1.0 | meta::source_link, meta |
| `crates/core/tests/spectral_selection.rs` | Locks the time and frequency selection: the feather mask, moves clamped to the file and Nyquist, and the Marquee tool's action and key. | 8 tests, 0 ignored | C4 | spectral selection in view, actions, shortcuts |
| `crates/core/tests/speech_leveler.rs` | Locks the Speech Volume Leveler: quiet and loud talkers land near the target, noise gaps are not raised, gain stays under the ceiling, state carries across chunks. | 7 tests, 0 ignored | roadmap B038, after 1.0 | effects, actions::Editor |
| `crates/core/tests/split_to_mono.rs` | Locks splitting a stereo track into two mono tracks: each channel on its own track, names, clip times, and refusals that change nothing. (present, out of v1) | 11 tests, 0 ignored | roadmap C111, after 1.0 | multitrack::split, soundcraft_model |
| `crates/core/tests/stats.rs` | Locks Amplitude Statistics as a streamed scan: exact numbers on a synthetic signal, a selection, clamping, cancel and progress. | 9 tests, 0 ignored | G4 | stats, store |
| `crates/core/tests/stereo_width.rs` | Locks stereo width maths on mid and side: width 1 is identity, 0 is mono, 2 doubles the side, and the width measurement. | 16 tests, 0 ignored | after 1.0 | dsp::stereo_width |
| `crates/core/tests/store_and_doc.rs` | Locks the on-disk store and piece table: bit-exact blocks across pack rollovers, the cache budget, readers beside a writer, pack deletion, locate and split, and a 3-hour document without allocating it. | 10 tests, 0 ignored | I7, A1, C1 | store, doc, proptest |
| `crates/core/tests/stretch_fit.rs` | Locks stretch to fit a duration: the target in frames, the ratio and its limits, and applying it to a buffer. | 22 tests, 0 ignored | roadmap B112, after 1.0 | timepitch::fit, store |
| `crates/core/tests/sweep.rs` | Locks the stale session sweep: a dead run's folder is removed, but never a live store, a foreign folder, another machine's session, or one with a journal. | 10 tests, 0 ignored | I4, I7, #86 | store::{stale_sessions, sweep_stale_sessions}, autosave |
| `crates/core/tests/take_recovery.rs` | Locks that takes a crash left behind are found, repaired with every written frame kept, and deleted only by a confirmed discard. | 9 tests, 0 ignored | J3, I4, roadmap C032 | take_recovery, record, Document |
| `crates/core/tests/time_shift.rs` | Locks Time Shift: an impulse lands exactly the set delay later, also across three chunks, only on masked channels, with length kept. | 7 tests, 0 ignored | after 1.0 | effects::TimeShift, store |
| `crates/core/tests/timeline_view.rs` | Locks the multitrack timeline view model: fit, zoom, scroll limits and row height, on exact hand-worked values. (present, out of v1) | 31 tests, 0 ignored | roadmap C082, after 1.0 | multitrack::timeline |
| `crates/core/tests/timepitch.rs` | Locks Stretch and Pitch: a stretch changes length not pitch, a pitch shift keeps length, bad settings are refused, and the pitch readout. | 6 tests, 0 ignored | feature f127, after 1.0 | timepitch, actions::Editor |
| `crates/core/tests/track_info.rs` | Locks track colour and notes: the eight-colour palette, colour text parsing, the checked setter and notes limits. (present, out of v1) | 34 tests, 0 ignored | roadmap C105, after 1.0 | multitrack::track_info |
| `crates/core/tests/track_invert.rs` | Locks the per-track invert phase flag and the sample flip. (present, out of v1) | 13 tests, 0 ignored | roadmap C110, after 1.0 | multitrack::invert |
| `crates/core/tests/track_reorder.rs` | Locks Multitrack::move_track: clips, names and mixer state travel with the track, the selection is kept by id, non-audio tracks keep their slots. (present, out of v1) | 14 tests, 0 ignored | roadmap C104, after 1.0 | multitrack |
| `crates/core/tests/video_prefs.rs` | Locks the Video preferences (timecode overlay, spot frame) in the settings text: defaults, get, set and round trip. (present, out of v1) | 31 tests, 0 ignored | feature a287, after 1.0 | settings::video |
| `crates/core/tests/video_snap.rs` | Locks snap to an attached video's frame rate: mapping a rate to the nearest SMPTE rate and snapping a time to its frames. (present, out of v1) | 21 tests, 0 ignored | roadmap A324, after 1.0 | video_snap, smpte |
| `crates/core/tests/vorbis.rs` | Locks Ogg Vorbis save: a tone decodes to the same length and frequency, quality changes size, bad quality is refused, temp-file safety holds. | 4 tests, 0 ignored | feature f93, after 1.0 | save::save_vorbis, io |
| `crates/core/tests/workspace_keys.rs` | Locks the nine workspace switch commands: stable names, Alt+Shift+digit by default, rebindable, in menu order. | 4 tests, 0 ignored | I2, roadmap A012 | workspaces, shortcuts, panels |
| `crates/core/tests/xmp.rs` | Locks XMP in the _PMX chunk: an unedited packet is copied byte for byte, and an edit is one undo step that changes only that property on save. | 6 tests, 0 ignored | A2, roadmap A268 | io, save, History |

## WavCraft control verbs (main at 11e49a5)

How a line travels: `apps/wavcraft/src/control.rs` listens on loopback (only when `WAVCRAFT_CONTROL_PORT` and `WAVCRAFT_CONTROL_TOKEN_FILE` are set), checks the `TOKEN <token>` first line, and hands each later line to the window thread. `WaveApp::control_request` in `apps/wavcraft/src/app.rs` matches the first word (140 distinct first words). `MICCHECK` and `VIDEO frame` are answered earlier, in `logic`. Some arms answer in place, some pass the rest of the line to another file. `control.rs` itself handles only `TOKEN`.

Verbs marked "not in header" have a match arm but no line in the `control.rs` header comment.

The headless path in `crates/automation/src/lib.rs` parses only six of these (`OPEN`, `SELECT`, `ACTION`, `EFFECT`, `SAVE`, `STATE`) and runs them with no window (`headless.rs`) or sends them to a running window (`remote.rs`).

### Connection

| Verb | Meaning | Handled in |
|---|---|---|
| `TOKEN <token>` | First line of every connection; must match the owner-only token file | control.rs (`serve`) |

### State and inspection

| Verb | Meaning | Handled in |
|---|---|---|
| `STATE` | One line with frames, rate, selection, undo steps, busy, playing, snapping, clipboard, last op, path | app.rs |
| `HOVER [x y]` | Pointer readout at an editor pixel, or the editor size and view | app.rs |
| `LASTERROR` | The error text the window shows now (not in header) | app.rs |
| `PROPERTIES` | The Properties panel rows for the open file | app.rs |
| `MARKERS` | The markers of the current state | app.rs |
| `DOCS` | The open documents in the order opened | app.rs |
| `FILES [sort, filter, columns, select, expand, collapse, reveal]` | The Files panel table and its commands | app.rs (`files_request`), rules in files.rs |
| `RECENT`, `RECENT open <i>`, `RECENT clear` | The recent files list, open one entry, or empty the list (not in header) | app.rs (`recent_request`) |
| `PANELS` | Which panels are shown, dock zones, sizes, floating windows | app.rs (`panels_report`) |
| `WORKSPACES` | Saved workspaces and the one in use | app.rs (`workspaces_report`) |
| `SHORTCUTS` | Every command with its chords | app.rs (`shortcut_request`) |
| `SHORTCUTSETS` | The saved shortcut sets and the active one | app.rs (`shortcut_set_request`) |
| `KEYBOARD <mods>` | The visual keyboard's bound keys for those modifiers | app.rs (`shortcut_set_request`) |
| `AUDIOINFO` | Host, buffer, channel maps, device, hardware profile | app.rs |
| `DEVICES` | Output and input devices on offer, and the one playing (not in header) | app.rs, list from player.rs |
| `MONITORS [simulate <spec>, real]` | Report the monitor layout for floating panels, simulate one, or go back to the real one (not in header) | app.rs |
| `NOISE_PRINT` | Size of the session's captured noise print | app.rs |
| `PEAKCACHE info, clear` | The peak cache folder and how the open file's overviews were made | app.rs |
| `CACHE info, cleanup <days>` | Session store usage (sessions, bytes, recoverable, stale, removable, keep days, cache size, folder), and removal of old sessions (not in header) | app.rs |
| `AUTOSAVE` | Auto Save journal counters and recoverable sessions | app.rs, counters from recovery.rs |
| `SCSOURCE test` | Read the open document back as a SoundCraft clip source | app.rs |
| `ABOUT` | Open Help, About and report the notices size | app.rs, text from notices.rs |
| `I18N missing, get <key>` | Missing translation keys, or one string's current text | app.rs |

### Files and documents

| Verb | Meaning | Handled in |
|---|---|---|
| `OPEN <path>` | Open a file in the background | app.rs |
| `OPEN_PICK <path>...` | Open as Ctrl+O or a drop does, behind the unsaved-edits prompt | app.rs |
| `OPEN_ANSWER save, discard, cancel` | Answer that prompt | app.rs (`answer_open_prompt`) |
| `OPEN_MANY <paths>` | Open several files, each as its own document. Paths are split on spaces and tabs. Answers `OK queued=<n>` (not in header) | app.rs |
| `APPEND current, new <paths>` | Append files to the current document or make a new one of them (not in header) | app.rs |
| `IMPORT <path>` | Add a file to the Files panel without opening it (not in header) | app.rs |
| `IMPORT_RAW <path> rate= ch= fmt=` | Import headerless raw data | app.rs |
| `INSERT_FILE <path>` | Insert audio from a file at the cursor or over the selection | app.rs |
| `NEWDOC [rate= ch= bits=]` | A new empty document (not in header) | app.rs |
| `SWITCH <i>` | Make document `i` the current one | app.rs |
| `CLOSE [discard]` | Close the current document | app.rs |
| `SAVE [path] [bits= float= aiff= quality= meta=]` | Save, or Save As with the format from the extension | app.rs |
| `SAVE_CANCEL` | Stop the running save | app.rs |
| `SAVE_SELECTION <path>` | Save only the selection to a new WAV | app.rs |
| `EXPORT <path>` | Export the whole document to another WAV | app.rs |
| `EXPORT_RANGES <folder> [pattern=]` | Each range marker as its own WAV | app.rs |
| `SAVEOPTS estimate, set, show, dialog <fmt> <bits> cancel or <path>` | The Save As export options and a size estimate | app.rs (`control_saveopts`) |
| `EXPORTPRESET save, load, delete, list` | Named export option presets | app.rs (`control_export_preset`) |
| `CONVERT rate= channels= bits= ...` | Convert Sample Type in the background | app.rs, settings type in convert_ui.rs |
| `INTERPRET <hz>` | Relabel the sample rate without resampling | app.rs |
| `PRESET list, save, delete` | Conversion presets | app.rs |
| `RECOVER [i]`, `DISCARD_RECOVERY [i]` | Recover or drop a crashed session | app.rs, list in recovery.rs |
| `RECOVERTAKE [i]`, `DISCARDTAKE [i]` | Repair and open, or delete, an unfinished take | app.rs, list in recovery.rs |
| `QUIT [discard]` | Close the window | app.rs |

### Selection, view and navigation

| Verb | Meaning | Handled in |
|---|---|---|
| `SELECT <start> <end>` | Set the time selection in frames | app.rs |
| `SELECT_TIME <start> <end>` | The same, written in the chosen time format | app.rs |
| `GESTURE marquee, marquee_move, right_click, shift_click, double_click, triple_click, edge_drag, shift_left, shift_right, shift_home, shift_end, channel_drag, hand_drag <dx_px>, zoom_click <frame> [out], ruler_rdrag <start> <end>` | The editor's selection, hand, zoom and ruler gestures | app.rs (`control_gesture`; the marquee and right-click ones go on to `pointer_gesture`) |
| `CHANNELS all, <list>` | Which channels edits act on | app.rs |
| `CHANNEL toggle <n>` | Turn one channel on or off for edits; `n` counts from 0 (not in header) | app.rs |
| `SNAP <frame> [frames_per_px]` | The snapped frame a selection gesture would get | app.rs |
| `SNAP_FRAME <frame>` | The snapped frame a ruler click would get | app.rs |
| `GOTO <time>` | Move the cursor to a time | app.rs |
| `VIEWPORT <start> <frames_per_px>` | Set zoom and scroll | app.rs |
| `NAV <start> <end>` | Set the visible range as the zoom navigator does (not in header) | app.rs |
| `ZOOMPRESET set, get <1-5>` | Zoom to Time slots 1 to 5 (not in header) | app.rs |
| `AMPSCALE db, percent, samples, normalized` | Choose the amplitude ruler's scale (not in header) | app.rs |
| `SPECTRAL show, hide, toggle, fft <size> or up or down, window, range, split, view, scale, scroll <hz>, reset` | Spectral display settings (not in header) | app/spectral.rs (`control_spectral`) |
| `STATUSBAR show, hide <field>, list` | Status bar fields (not in header) | app/spectral.rs (`control_statusbar`) |
| `APPEARANCE preset save, load, delete, list` | Appearance presets (not in header) | app.rs |
| `WINDOW minimize, restore` | Minimise or restore the window | app.rs |

### Commands, keys, menus and panels

| Verb | Meaning | Handled in |
|---|---|---|
| `ACTION <name>` | Run an action table row or a one-shot effect by name | app.rs |
| `ACTION window.toggle, show, hide.<panel>` | Show or hide a panel | app.rs |
| `KEY <chord>` | Run what the shortcut table binds to the chord | app.rs |
| `PARAM amplify.db, normalize.db <n>` | Set the gain the Amplify and internal Normalize one-shots use | app.rs |
| `CONTEXTMENU <editor, ruler, history> list, run <item>` | A right-click menu's entries (not in header) | app.rs (`control_contextmenu`), entries in menus.rs |
| `TOOLBAR list, run <item>` | The toolbar and Tools panel entries (not in header) | app.rs (`control_toolbar`), entries in menus.rs |
| `PANEL move, maximize, close, activate <id>` | Dock layout changes | app.rs |
| `PANELMENU <panel> list, run <item>` | A panel's own menu | app.rs |
| `DIVIDER drag, corner, reset` | Move the dividers between dock zones | app.rs |
| `WORKSPACE save, switch, reset, delete <name>` | Workspace commands | app.rs (`workspace_command`) |
| `SHORTCUT set, clear, reset` | Edit the user's key file | app.rs (`shortcut_request`) |
| `SHORTCUTSET save, switch, delete <name>` | Named shortcut sets | app.rs (`shortcut_set_request`) |
| `PREF set, get, reset, show` | Preferences (custom fps, language, recording offset, monitoring, tempo, meter, defaults). Any other key goes through the general `get <key>` and `set <key> <value>` | app.rs |
| `SETTINGS export, import <path>` | The settings as one file | app.rs |
| `METADATA [set, open]` | The Metadata Display field list and dialog | app.rs |

### Transport and audio hardware

| Verb | Meaning | Handled in |
|---|---|---|
| `PLAY [from to]` | Play a range, or from the selection as Space does | app.rs, engine in player.rs |
| `PLAYBACK [PLAY, STOP, QUALITY fast, high]` | Playback state, start, stop and resampling quality | app.rs (`playback_request`), engine in player.rs |
| `KEYJKL l, j, k, k+l, k+j, up` | The JKL shuttle | app.rs |
| `SCRUB <frame>, end` | Drag the playhead and hear a grain | app.rs |
| `DEVICE_CHANGED` | Simulate the default output device changing | app.rs, player.rs (`simulate_device_change`) |
| `METRONOME on, off, countin <bars>, volume, clicks, render` | The metronome and where its clicks land (not in header) | app.rs (`control_metronome`), player.rs |
| `CHANMAP out, in, reset` | Output and input channel maps | app.rs |
| `AUDIOPROFILE list, simulate` | Per-device hardware setups | app.rs |
| `METERS [PUSH l r, RESET]` | The Levels panel readings | app.rs, line from meters_ui.rs |
| `MICCHECK [seconds]` | Listen to the input, then report peak, average, clips and a verdict | app.rs (taken before the match, in the request loop) |
| `PLAYLIST add, move, remove, loops, play, stop, clear, state, sequence` | The Playlist panel (not in header) | app.rs (`control_playlist`), playlist_panel.rs |

### Editing

| Verb | Meaning | Handled in |
|---|---|---|
| `INSERT_SILENCE <seconds>` | Insert silence at the cursor | app.rs |
| `INSERT_SILENCE_FMT <length>` | The same, length in the chosen time format | app.rs |
| `DELETE_SILENCE [threshold_db= min_len_ms= keep_ms=]` | Remove silent stretches as one undo step | app.rs |
| `MIXPASTE mode= ...` | Mix Paste at the cursor, from the clipboard or a file | app.rs, parsing in mixpaste.rs |
| `OSCLIP [set <path>]` | What is on the system clipboard, or put a file there | app.rs, oscb.rs |
| `MARKER rename, move, select, merge, color, desc, info` | Edit one marker by its position in `MARKERS` (not in header) | app.rs (`control_marker`) |
| `HISTORY <step>` | Go to a history step | app.rs |
| `HISTORY_DELETE <step>` | Remove a step and every later one | app.rs |
| `HISTORY_CLEAR` | Clear the history | app.rs |
| `HISTORY_KEY up, down` | The History panel's arrow keys | app.rs |
| `META get, set` | File metadata fields, one undo step per set | app.rs |
| `METASET save, load, delete, fields, list` | Named sets of metadata fields (not in header) | app.rs (`metaset_command`) |
| `CHNAME get, set` | Channel names | app.rs |
| `XMP get, set` | XMP properties | app.rs |
| `JOB testslow <ms>, cancel` | A test-only background job, for busy and cancel checks | app.rs |
| `HOLD_BUSY <ms>` | Test hook: report busy for a while; fake input only (not in header) | app.rs |

### Effects, generators, rack and repair

| Verb | Meaning | Handled in |
|---|---|---|
| `EFFECT` | List every effect and its settings | app.rs |
| `EFFECT <name> key=value...` | Run an effect on the selection or whole file | app.rs |
| `EFFECT_DIALOG <name>, OK, CANCEL` | Open an effect dialog, read it, or press a button | app.rs, dialog in effect_dialog.rs |
| `GENERATE [tone, noise ...]` | List the generators, or insert generated audio | app.rs |
| `GENERATE_DIALOG <name>, PREVIEW, OK, CANCEL` | The Generate dialog | app.rs, dialog in effect_dialog.rs |
| `STRETCH ratio= pitch=` | Stretch and Pitch on the selection | app.rs |
| `PITCHDETECT` | Median pitch of the selection | app.rs |
| `RACK`, `RACK set <slot> <effect or none or vst3:name> [key=value], power <slot or master> on or off, in <n>, out <n>, mix <n>` | The Effects Rack state and slots (not in header) | rack_panel.rs (`command`) |
| `RACK apply` | Write the rack into the selection or file (not in header) | app.rs (`apply_rack`) |
| `PRESET save, load, delete, list <effect, rack>` | Effect and rack presets (any `PRESET` line that is not a conversion preset) | rack_panel.rs (`preset`) |
| `VST3 scan`, `VST3 list` or bare `VST3` | Scan for VST3 plugins and list them (not in header) | rack_panel.rs (`vst3_command`) |
| `MATCH [target= tp= tol= limit= ...]` | Match Loudness on the whole file as one step | app.rs |
| `LOUDNESS [start end]` | Integrated loudness, range, true peak, sample peak | app.rs |
| `STATS [start end]` | Sample statistics per channel, kept for the goto actions | app.rs, panel state in statspanel.rs |
| `FREQ [fft= window= start end]` | Frequency analysis peak per channel | freq_panel.rs (`control`) |
| `DECLIP [...]` | DeClipper in one step | app.rs |
| `DECLICK [...]` | DeClicker in one step | app.rs |
| `AUTOCLICK [...]` | Automatic Click Remover in one step | app.rs |
| `DIAG scan, repair` | The Diagnostics window's scan list and repair | app.rs, parsing and scan in diagnostics_panel.rs |

### Recording

| Verb | Meaning | Handled in |
|---|---|---|
| `RECORD start, quick, pause, resume, cancel, stop` | Record a take; `start` takes `rate=`, `ch=`, `bits=`, `inputs=`, `max=`, `in=`, `at=`, `mode=`, `channel=`, `preroll=` and `monitor=` | app.rs, argument parsing and take state in recording.rs |

### Out of v1: multitrack, mixer, CD, video, batch

| Verb | Meaning | Handled in |
|---|---|---|
| `SESSION new <name> [rate=] [depth=] [master=stereo or 5.1], state, save <path>, open <path>` | The multitrack session | multitrack_ui.rs |
| `SESSION return` | Bring an edited clip source back into the session | app.rs (`return_to_session`) |
| `TRACK add [name] [fmt=mono or stereo or 5.1], add video, format, remove, rename, mute, solo, volume, arm, disarm` | Session tracks | multitrack_ui.rs |
| `CLIP add, adddoc, select <track> <n>, select none` | Session clips | multitrack_ui.rs |
| `CLIP edit <track> <i>` | Open a clip's source in the Waveform editor | app.rs (`control_edit_clip`) |
| `CLIPPROP <track> <i> ...` | Clip tempo, beats, key, name, gain | multitrack_ui.rs |
| `CLIPTYPE ...` | Clip audio types and the type filter | multitrack_ui.rs |
| `VIEW waveform, multitrack` | Switch the centre view (also leaves CD Layout) | app.rs, then multitrack_ui.rs |
| `SMARKER add, rename, delete, next, prev, playhead, select, addhere, addsel, state` | Session markers and ranges | multitrack_ui.rs |
| `SPLAY [seconds]`, `SSTOP` | Play and stop the session | multitrack_ui.rs |
| `SREC start [seconds], stop` | Record the armed tracks while the rest plays | multitrack_ui.rs |
| `PUNCH set <in_s> <out_s>, on, off, preroll <s>` | Punch in and out | multitrack_ui.rs |
| `MASTER vol=<dB> mute=0 or 1` | The session's master bus (not in header) | multitrack_ui.rs |
| `AUTO add, move, delete, thin, show, state` | Automation lanes | multitrack_ui.rs |
| `MIXDOWN [selected] [normalize=]` | Mix the session down to a new document | app.rs |
| `MIXER state, set` | The Mixer panel's strips | mixer.rs (`control`) |
| `MIXER show, hide` | The Mixer window checkbox | app.rs |
| `CD add, adddoc, pause, meta, move, remove, limit, view, state, export` | CD Layout and its WAV plus CUE export | cd_ui.rs (`control`) |
| `VIDEO attach, detach, status` | The Video panel's file | app.rs, panel in video.rs |
| `VIDEO frame <seconds>` | Decode the frame at a time; answered when the worker is done | app.rs (request loop), video.rs |
| `BATCH add, op (normalize peak=, gain db=, mono, clear), out, overwrite on or off, clear, run, cancel, status` | The Batch Process panel | batch_panel.rs (`control`) |

## Command-line flags and environment variables (main at 11e49a5)

Found by grepping the workspace for `env::var`, `var_os`, `args_os`, `args()` and `--` literals.

### The app binary: `wavcraft` (apps/wavcraft/src/main.rs)

| Flag | What it does | Read in |
|---|---|---|
| `FILE` (first argument that is not a known flag) | Opens that file in the window. Later ones are ignored. An unknown `--flag` counts as a file name too. | `main.rs` `parse_args` |
| `--session-dir DIR` or `--session-dir=DIR` | Session store folder for this run. Wins over `WAVCRAFT_SESSION_DIR` and the saved setting. | `main.rs` `parse_args`, used in `app.rs` `WaveApp::new` |
| `--reset-preferences` | Moves the settings file aside and starts with defaults. | `main.rs` `parse_args` |
| `--vst3-scan BUNDLE` | Hidden worker mode. Must be the first argument. Prints one VST3 bundle's report on standard output and exits, with no window and no log. The app starts itself this way to scan plugins. | `main.rs`, constant `WORKER_FLAG` in `crates/core/src/vst3.rs` |

Not in the binary: `wavcraft run`, `wavcraft convert`, `wavcraft inspect` and `wavcraft mcp` (v1 item N2 and N3) have no code in `main.rs`. The batch parser in `crates/core/src/batch_cli.rs` (`--batch`, `--out`, `--name`, `--bits`, `--step`, `--`) is built and tested but nothing in the app calls it.

### Environment variables the app reads

| Variable | Value | Effect | Read in |
|---|---|---|---|
| `WAVCRAFT_SESSION_DIR` | folder | Session store folder, over the saved setting, under `--session-dir`. | `app.rs` (`settings::ENV_SESSION_DIR` in `crates/core/src/settings.rs`) |
| `WAVCRAFT_LOG_DIR` | folder | Where the log and crash files go. Default is `wavcraft/logs` in the config folder, or in the temp folder when there is no config folder. An empty value is ignored. | `selflog.rs` |
| `WAVCRAFT_CONTROL_PORT` | port number | Turns the control channel on at 127.0.0.1 on that port. Needs the token file too. | `control.rs`, `crates/automation/src/remote.rs` |
| `WAVCRAFT_CONTROL_TOKEN_FILE` | file path | File holding the control channel's token. | `control.rs`, `crates/automation/src/remote.rs` |
| `WAVCRAFT_RECORD_DIR` | folder | Where takes are written. Default is `WavCraft Recordings` in the audio folder, else the documents folder, else the home folder. | `recording.rs` |
| `WAVCRAFT_PEAK_CACHE_DIR` | folder | Where saved waveform overviews live. Default is `wavcraft/peaks` in the system cache folder. | `app.rs` |
| `WAVCRAFT_VST3_PATH` | path list (the OS's own separator) | Folders to scan for VST3 plugins, in place of the standard ones. For tests. | `rack_panel.rs` |
| `WAVCRAFT_MONITORS` | `name:x,y,w,h;name:...` | Fakes the monitor list (first is the primary). | `app.rs`, parser in `crates/core/src/monitors.rs` |
| `WAVCRAFT_NO_AUDIO` | any value but empty, `0`, `false` | Swaps the sound card for a silent test sink. The multitrack transport only accepts exactly `1`. | `player.rs`, `multitrack_ui.rs` |
| `WAVCRAFT_NO_AUDIO_RATE` | rate in Hz | Rate of the silent sink. Default 48000. | `player.rs` |
| `WAVCRAFT_FAKE_INPUT` | `sine:440`, `sine:440+660`, with optional `,ring=<ms>` and `,stall=<ms>` | Replaces the input device with generated tones. Also unlocks the `HOLD_BUSY` control verb. The input monitor parses the same variable its own way: `sine:<hz>[:<dBFS>]` or `silence`. The multitrack recorder only accepts exactly `1`. | `crates/core/src/record.rs` (`ENV_FAKE_INPUT`), `input_monitor.rs`, `app.rs`, `crates/core/src/mtrecord.rs` |
| `WAVCRAFT_MONITOR_SINK` | `null` | While recording with monitoring, drains the monitor queue in a thread, not through an output device. | `recording.rs` |
| `WAVCRAFT_TEST_SAVE_CHUNK` | set | Test only. A save adds a `TEST` chunk to the file. | `app.rs` `metadata_chunks` |
| `WAVCRAFT_TEST_SAVE_DELAY_MS` | milliseconds | Test only. Slows a save down per 64 K-frame block. | `app.rs` `test_block_delay` |
| `WGPU_BACKEND` | wgpu backend name | On Windows the app forces DirectX 12 unless this is set. | `main.rs` `configure_graphics` |
| `CRAFT_LOG` | filter text | Log filter. Wins over `RUST_LOG`. | `crates/craft-log/src/filter.rs`, used by `selflog.rs` |
| `RUST_LOG` | filter text | Log filter when `CRAFT_LOG` is unset. | `crates/craft-log/src/filter.rs` |
| `CRAFT_LOG_JSON` | `1`, `true`, `yes` | One JSON object per log line, in craft-log's own command-line logger. The app's logger (`selflog.rs`) does not call it. | `crates/craft-log/src/filter.rs` |
| `CRAFT_LOG_STDERR` | `1`, `true`, `yes` | Log to standard error too, in craft-log's own command-line logger. The app's logger does not call it. | `crates/craft-log/src/cli.rs` |

Two mismatches I saw in the code: `WAVCRAFT_NO_AUDIO` and `WAVCRAFT_FAKE_INPUT` are each tested two ways (any truthy value or a tone spec in the waveform editor, exactly `1` in the multitrack code). `WAVCRAFT_FAKE_INPUT` also has two tone parsers: `FakeSpec::parse` in `crates/core/src/record.rs` for recording and `FakeInput::parse` in `crates/core/src/input_meter.rs` for the input monitor.

craft-log's system report (`crates/craft-log/src/report.rs`) and its command-line logger (`cli.rs`) read more variables: `XDG_CURRENT_DESKTOP`, `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`, `GAMESCOPE_WAYLAND_DISPLAY`, `SteamGamepadUI`, `LC_ALL`, `LANG`, `PROCESSOR_IDENTIFIER`, `HOME`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `PATH`, and a scan of the whole environment for switches and secrets. The app does not reach that code. It only calls `install_wrapped`, `set_log_path` and `crash::install_hook` (`selflog.rs`), so of craft-log's variables only `CRAFT_LOG` and `RUST_LOG` act on the app.

### Variables used only by tests, tools and the build

| Variable | Effect | Read in |
|---|---|---|
| `WAVCRAFT_KIT` | Folder holding the 3-hour test kit file. The default is a hard-coded Windows folder. | `crates/testkit/tests/edits.rs` |
| `WAVCRAFT_SCRATCH` | Scratch folder for scripted checks. | five `tools/alpha/*.py` scripts, `tools/alpha/f70.sh`, `tools/alpha/f128.sh` |
| `ALPHA_SCRATCH`, `F122_SCRATCH`, `F129_SCRATCH`, `F76_SCRATCH_BASE`, `F82_SCRATCH_ROOT` | Scratch folders for single scripts. | `tools/alpha/` |
| `CARGO_TARGET_DIR` | Where the scripts look for the built binaries. | `tools/alpha/*.sh` |
| `KEEP`, `F97_SAMPLE`, `F82_PORT`, `F84_ARGS`, `FIXTURE` | Per-script switches. | `tools/alpha/` |
| `WAVCRAFT_SCRATCH_ROOT` | Same idea, older name, three scripts. | `tools/alpha/f19.py`, `f68.py`, `f69.py` |
| `SOUNDCRAFT_DIR` | Where the sibling engine checkout is. | `tools/alpha/f128.sh` |
| `CRAFT_LOG_COMMIT`, `CRAFT_LOG_TREE`, `CRAFT_LOG_BUILD_DATE` | Compile-time values set by the build script. | `crates/craft-log/build.rs`, `crates/craft-log/src/lib.rs` |
| `SOURCE_DATE_EPOCH` | Fixes the build date. | `crates/craft-log/build.rs` |
| `CARGO_MANIFEST_DIR` | Set by cargo. The build script uses it to find the git folder. | `crates/craft-log/build.rs` |
| `CRAFT_LOG_TEST_CHILD`, `CRAFT_LOG_TEST_DIR`, `CRAFT_LOG_TEST_TAG`, `CRAFT_LOG_TEST_START_AT` | Child-process test harness. | `crates/craft-log/tests/process.rs` |

### Other binaries' flags

| Binary | Flags | File |
|---|---|---|
| `wavcraft-testkit` | `--long`, `--huge`, `--seed N`, `--speech FILE`, `--noise FILE` (the last two go together), `-h`, `--help`, and one bare argument: the output folder (required). Any other `--` option is an error. | `crates/testkit/src/main.rs` |
| `wavcraft-bench` | one bare argument: the file | `crates/testkit/src/bin/wavcraft-bench/main.rs` |
| `wavcraft-store-bench` | `--cache-mb N`, `--sessions DIR`, `--huge` | `crates/testkit/src/bin/wavcraft-store-bench/main.rs` |
| `wavcraft-edit-bench` | `--cache-mb N`, `--edits N`, `--sessions DIR` | `crates/testkit/src/bin/wavcraft-edit-bench/main.rs` |
| example `spectrogram_cost` | one bare argument: minutes (1 to 180) | `crates/core/examples/spectrogram_cost.rs` |
| `tools/alpha/*.py` check scripts | `--exe` (all), `--keep`, `--port`, and in a few `--scratch`, `--scratch-root`, `--minutes`, `--wav`, `--plugins`, `--device`, `--core` | `tools/alpha/` |

## Deeper notes

Everything below was checked against the code at 11e49a5.

### Known differences from Audition

These are choices I made where Audition 26.5.0.82 was not seen. What was seen is in `docs/v1/audition-behaviour.md`.

- **One-channel Paste near the end of the file.** The clip is cut short at the end. The length never grows. Audition was only seen pasting mid-file (`paste` in `crates/core/src/doc/edit.rs`).
- **One-channel Paste over a selection.** The clip overwrites from the selection's start for the clip's length, whatever the selection's length. The step is labelled "Paste Audio".
- **Zero crossings on stereo.** An edge moves to where the sum of the enabled channels changes sign, or is exactly 0 (`zero_crossing` in `crates/core/src/actions.rs`). It looks on one side only, within 10 ms (`ZERO_CROSSING_REACH_MS`). The edge's own frame never counts, so an edge already on a crossing steps to the next one. With no crossing in reach the edge stays. An inward adjust that would empty the selection changes nothing. Audition's rule for stereo was not seen.
- **Repeat Last Command.** `repeatable` in `crates/core/src/actions.rs` allows Silence, Paste, Crop, the six zero-crossing adjusts, Reverse, Amplify, Normalize and the four fades. Insert Silence, a registered effect, DeClipper, DeClicker and the Automatic Click Remover repeat with their last settings (`Repeat`). Delete, Cut, Copy, Undo, Redo, selection moves and the view do not repeat. The keys are F3 and Shift+R. The dialog form (F2 and Ctrl+R) reopens Insert Silence or an effect with settings on the last values, and runs the others at once. The app does this itself in `run_window_action` (`apps/wavcraft/src/app.rs`). `Editor::repeat_dialog` in core is still a `todo!()` and nothing calls it.
- **History labels.** One-channel Crop, Cut, Paste and Delete are labelled "Crop Audio", "Cut Audio", "Paste Audio" and "Delete Audio" (constants in `doc/edit.rs`). Those four names were checked in Audition. `every_step_name_is_the_history_label_and_the_undo_text` (`crates/core/tests/edits.rs`) pins each label. `undo_menu_and_history_agree_for_each_step` (`apps/wavcraft/src/app.rs`) pins the Undo menu text. "Mix Paste", "Silence", "Insert Silence" and "Insert Audio from File" are my wording and are not checked against Audition.

### The store after open

Nothing holds the whole file in RAM. `Document.state` is a `DocState`, and the session store behind it is the only copy of the samples.

- **Store** (`crates/core/src/store.rs`). Samples sit in append-only pack files in a session folder. A pack rolls over at 1 GiB (`DEFAULT_PACK_BYTES`). A block never changes once written. A block is stored as `f32`, or as the 16-bit or 24-bit integers of the source, and every read hands back `f32`. The zero block is never on disk.
- **Block cache.** RAM holds only a block cache with a byte budget. The default is 1/16 of physical memory, at least 256 MiB and at most 1 GiB (`default_cache_bytes`). A miss reads one whole block with one positioned read. There is no `mmap`.
- **Lifetime.** Blocks are handed out as `Arc<BlockHandle>`. A sealed pack with no live blocks is deleted. Dropping the last handle and the last `Store` removes the session folder, unless `Store::set_keep` was called.
- **Readers.** Every consumer reads bounded chunks through `DocState::read`. `Document::audio_buffer` still reads the whole document into one `AudioBuffer`. Tests, the bench and multitrack clips use it. The waveform, the spectrogram and the player never do.
- **In-memory documents.** `Document::from_bytes` and `from_buffer` write decoded audio into a store under the system temp folder (`wavcraft-memory-sessions`). `from_bytes` is the only editor path that calls `soundcraft_audio_io::decode`.
- **Memory that still grows with the file.** The waveform peaks and the spectrogram overviews (sizes below).

### Waveform peaks

I do not use the peaks type from `soundcraft-audio-io`. `WavePeaks` in `crates/core/src/document.rs` is my own, built from chunked store reads.

- Level 0 holds one `(min, max)` pair per 256 frames (`PEAK_BLOCK`). Each further level merges 4 (`FANOUT`). A 3-hour 48 kHz channel costs about 21.6 MB.
- Level-0 blocks follow each piece's source grid, with short blocks at splices. A shifted suffix is never reread.
- Levels are kept in `Arc` chunks of 4,096 entries. A step allocates only the chunks it changes.
- `PeakBlockCache` holds level-0 summaries keyed by store block and is shared by every step of one history. It holds only weak references, so it never keeps audio alive. Its budget is 64 MiB (`BLOCK_CACHE_BYTES`).
- `Document::waveform_columns` uses the peaks when a column spans 2,048 frames or more (`PEAK_MIN_SPAN`). Closer in it reads only the frames on screen from the store and folds them itself. A failed read draws silence and logs a warning.
- The app calls `waveform_columns` while it draws, so a zoomed-in view can read a block from disk on the UI thread when the block is not in the cache.
- `Document.peak_report` says how the peaks came about: `off`, `hit`, `miss`, `stale` or `corrupt`, the time taken, and whether a step did a full rebuild.

### Spectrogram and its overview

The maths is in `crates/core/src/spectrogram.rs`. It uses `rustfft` directly.

- **Sources.** Everything takes a `spectrogram::Samples`: a slice, or `document::StoreChannel` over the store. `StoreChannel` gives silence and one log line when a read fails. The slice functions (`Overview::build`, `render`, `render_columns`) remain and give the same pictures.
- **Job.** `Job { generation, columns, rows, params }`. `Renderer::ask` sends a job only when it differs from the last one sent, with a cheap clone of the `DocState`.
- **Sizes the app asks for.** Columns are half the lane width in physical pixels, from 1 to 2,048. Rows are the lane height per channel in physical pixels, up to 768. `Columns::covering` puts the columns on a grid fixed to the file. Zoomed out, a column is a whole number of FFT hops.
- **Overview.** One per channel. For each block of 16 FFT hops (`BLOCK_HOPS`) it keeps the loudest level per bin, one byte per bin. That is about 1 byte per 16 frames: 10.8 MB per channel-hour at 48 kHz. Its levels sit in `Arc` chunks of 64 blocks, so a step shares what it does not change.
- **What an overview depends on.** FFT length, window and dB range. It does not depend on the frequency axis or the picture size. `Overview::serves` also checks the length in frames. `render_columns_from` ignores an overview that does not serve and runs every FFT itself.
- **Drawing.** A column reads whole fresh blocks from the overview and runs FFTs only for its ragged ends. `rayon` spreads columns, and an overview's chunks, across cores. The worker does channels one after another.
- **Undo straight back.** The worker re-installs the retired overview only when the piece tables match (`same_table` in `apps/wavcraft/src/spectrum.rs`).
- **The app's picture.** `SpectrumCache { job, textures }` is the newest picture, one texture per channel. The app drops it in `finish_opening` and in `set_axis` (`apps/wavcraft/src/app/spectral.rs`), among other places.

### Playback

All of this is `apps/wavcraft/src/player.rs`.

- **Device.** The chosen host and device, or the system default, with the device's default config. Sample formats f32, i16, u16 and i32. `Player::new` opens the stream at start-up. `begin_play` reopens it when there is no output or the stream failed.
- **Ring.** 2^17 frames (about 2.7 s at 48 kHz) of up to 8 channels, stored as `f32` bits in atomics. The read-ahead thread fills it in 8,192-frame chunks (`FILL_CHUNK`). The ring holds the audio in play order, not file order. The thread walks the play's `Plan` (loop, skipped part, forward or backward), so a loop wrap or a skip costs the callback nothing. A file with more than 8 channels plays its first 8.
- **Play state.** One `Mutex<Playback>` (generation, position, end, plan, direction, speed). The callback only ever uses `try_lock`. When the lock is busy it plays silence for that buffer.
- **Generations.** Every play, stop, seek, plan change and direction change starts a new generation. The callback sounds only once the thread has primed 16,384 frames (`PRIME_FRAMES`) for that generation. It writes the position back only when the generation is unchanged, so a stale callback cannot overwrite a restart.
- **Rate conversion.** `wavcraft_core::resample::Resampler` is a Kaiser windowed sinc. `fast` is 16 taps and `high` is 64 taps. The setting is `playback_quality`. The read-ahead thread builds the kernel and parks it under a mutex. The callback interpolates linearly until the kernel is there, when the kernel is wider than the ring's 1,024-frame margin, and at any speed other than 1. The playhead stays in file frames.
- **Channels.** Mono plays on both speakers. A one-channel device gets the mean of the file's channels. Files with more than two channels fold down through a fixed `Downmix` table. A `ChannelMap` can route channels to other outputs without a new stream.
- **Also in the callback.** The Effects Rack runs after the render. The UI builds a rack and parks it in a mailbox, and the callback swaps it in with `try_lock`. The metronome click and the recording monitor are mixed in here too. `Player::set_muted` silences playback while a take records.
- **Edit while playing.** `Player::replace_audio` moves the position, end and skipped part across the `EditSpan`, so the play goes on from the same place in the music.
- **Device changes.** A watcher thread checks the default output once a second. It only flags a change when the output follows the default.
- **No device.** `WAVCRAFT_NO_AUDIO=1` runs the same callback on a `wavcraft-test-sink` thread: stereo, 480 frames at a time, 48 kHz unless `WAVCRAFT_NO_AUDIO_RATE` says otherwise. The control verb is `PLAYBACK`.
- **Playhead.** `Player::position` returns the file frame from a snapshot of the play state. `Player::underruns` counts the dry buffers.

### What I take from the soundcraft crates

The crate-by-crate list is in the Workspace section. These are the limits and the things I do myself.

- **`soundcraft-audio-io` works on whole buffers.** `decode(bytes, ext_hint)` and `encode(&AudioBuffer, &EncodeOptions)` take and return everything in memory. `decode` refuses more than 2^30 samples in total. So I use them only where audio is in memory by design: multitrack sources and clips (`multitrack.rs`, `session_file.rs`, `mtrecord.rs`), `Document::from_bytes`, tests and the testkit.
- **Opening and saving a file in the editor are my own streaming code.** `detect_format` is the only audio-io call on the open path (`crates/core/src/io.rs`, and `crates/core/src/probe.rs`). `multitrack.rs` also calls `probe`.
- **Integer samples.** The store converts 16-bit and 24-bit blocks to `f32` with the same arithmetic audio-io uses at open, so a block reads back bit-identical.
- **`soundcraft-dsp` offline calls work on whole channels.** I call `offline::reverse` and `offline::gain` (`effects/oneshot.rs`), and `offline::time_stretch` and `offline::pitch_shift` (`timepitch.rs`).
- **Things I do not take.** The spectrogram uses `rustfft` directly. Peaks are `WavePeaks`. Playback rate conversion is `wavcraft_core::resample`. The single-file player is cpal.

## Mac, PC and Linux

### Platform-specific sites

| File | Function or item | What differs per OS |
|---|---|---|
| `crates/core/src/store.rs` | `read_exact_at` | Unix uses `FileExt::read_exact_at`. Windows loops on `seek_read`. There is no version for any other target. |
| `crates/core/src/space.rs` | `folder_free_space` | Unix asks `rustix::fs::statvfs`. Elsewhere it returns nothing and `mount_free_space` (sysinfo disks) answers. |
| `crates/core/src/save.rs` | `TempFile::create` | Unix sets mode 0600 for owner-only files. Windows inherits the folder's ACL. |
| `crates/core/src/save.rs` | `write_file_atomic` | Unix also syncs the parent folder. Windows does not. |
| `crates/core/src/atomic.rs` | `write_atomic` | The same: only unix syncs the parent folder. |
| `crates/core/src/session_file.rs` | a test of the session save | The mode 0600 check runs on unix only. |
| `crates/core/src/vst3.rs` | one test module | Built on unix only (`cfg(all(test, unix))`). |
| `crates/core/tests/sessions.rs` | one test | It has a Windows version and a version for the others. |
| `crates/core/src/autosave.rs` | `write_journal`, `sync_dir` | Unix sets mode 0600 and syncs the folder. Windows does neither. |
| `crates/core/src/settings.rs` | `path_to_native`, `path_from_native` | Paths are stored as raw bytes on unix, UTF-16LE on Windows, lossy text elsewhere. |
| `crates/core/src/settings.rs`, `store.rs`, `peakcache.rs` | `dirs::config_dir`, `cache_dir`, `data_local_dir` | Settings, presets, shortcuts, sessions and peak files live in each OS's own config and cache folders. |
| `crates/core/src/vst3.rs` | `default_dirs` | The VST3 folders come from `default_search_paths` in `soundcraft-vst3-host`, which differ per OS. |
| `crates/core/src/shortcuts.rs` | `Shortcut::text`, `menu_text`, `tooltip` | Mac shows key glyphs. The others show `Ctrl+`. The callers that pass `cfg!(target_os = "macos")` are in `app.rs` and `app/spectral.rs`. `shortcut_editor.rs` takes the flag from `app.rs`. |
| `apps/wavcraft/src/main.rs` | crate attribute | Windows release builds use `windows_subsystem = "windows"` (no console). |
| `apps/wavcraft/src/main.rs` | `configure_graphics` | Windows forces the DX12 backend unless `WGPU_BACKEND` is set. Mac and Linux take the wgpu default. |
| `apps/wavcraft/src/control.rs` | `check_private`, `write_private` | Unix checks and sets mode bits. Every other target (`cfg(not(unix))`) calls `win_acl::others_can_read` and `win_acl::set_owner_only`. |
| `apps/wavcraft/src/win_acl.rs` | whole file | Windows only. Win32 security calls through `windows-sys`. |
| `apps/wavcraft/src/files.rs` | `reveal_command` | Chosen at run time from `std::env::consts::OS`, not by `cfg`. Mac runs `open -R`. Windows runs `explorer /select,`. Any other OS runs `xdg-open` on the parent folder. |
| `apps/wavcraft/src/app.rs` | `reveal_label` | The menu says Finder, Explorer or Show in Folder. |
| `apps/wavcraft/src/reveal.rs` | `Os`, `reveal_command` | A second copy of the reveal logic. Nothing outside its own tests calls it. |
| `apps/wavcraft/src/recording.rs`, `batch_panel.rs` | `record_dir` | The default folder comes from `dirs::audio_dir`, which differs per OS. |
| `crates/craft-log/src/cli.rs` | `nonblocking` | Sets `O_NONBLOCK` on Linux only, and only on the processor types the function lists. A no-op elsewhere. |
| `crates/craft-log/src/report.rs` | `Report::new` | Linux reads `/etc/os-release` and `/proc`. Windows uses `windows_version_in_background` and `windows_processor`. Mac gets neither, so operating system, kernel and processor model read "unknown". Memory reads "unknown" too unless the app gave a reader with `set_memory_reader`. No code in this workspace outside craft-log's own tests builds a report: the app only calls `install_wrapped`, `set_log_path` and `crash::install_hook`. |
| `crates/craft-log/src/lib.rs`, `report.rs` | the build line, the architecture field | Print `std::env::consts::OS` and `ARCH`. |
| `crates/craft-log/src/report.rs` | `session` | Reads the XDG, Wayland, X11 and gamescope variables. Only Linux sets them. With none set, a Windows build answers "Windows" and the others "unknown". |
| `crates/craft-log/src/report.rs` | `find_on_path`, `first_line_of`, `kill_tree` | Windows adds `.exe` and hides the console window. Unix uses a process group and kills the group. |
| `crates/craft-log/src/report.rs` | `home_folders`, `mask_homes` | Windows masks `USERPROFILE`, `HOMEDRIVE` plus `HOMEPATH`, and `HOME`, each with both slash kinds. The others mask `HOME`. |
| `crates/testkit/src/bin/wavcraft-bench/memory.rs` | `read` | Windows calls `GetProcessMemoryInfo`. Linux reads `/proc/self/status`. Mac runs `ps -o rss=` and has no peak figure. |
| `crates/testkit/tests/edits.rs` | `long_file` | Without `WAVCRAFT_KIT` the long test looks in a hard-coded Windows folder. |
| `crates/core/Cargo.toml` | `rustix` | Unix only. |
| `apps/wavcraft/Cargo.toml`, `crates/testkit/Cargo.toml` | `windows-sys` | Windows only. |
| `apps/wavcraft/Cargo.toml` | `eframe` features `wayland`, `x11` | Only Linux uses them. |
| `tools/winlist.swift` | whole file | A Mac-only window lister for scripted checks. |

Some tests run on one OS family only. Unix only: the link test in `crates/core/tests/save.rs`, the mode check in `crates/core/tests/autosave.rs`, the non-UTF-8 folder test in `crates/core/tests/settings.rs`, two symlink tests inside `crates/core/src/settings.rs`, the read-only folder test in `crates/testkit/tests/hostile.rs`, the body of the non-Unicode argument test in `crates/craft-log/tests/process.rs`, some unit tests in `crates/craft-log/src/cli.rs` and `report.rs`, and the symlink and mode tests in `control.rs`. Windows only: the unpaired-surrogate test in `crates/core/tests/settings.rs`, one test in `crates/craft-log/src/report.rs`, and the `icacls` test in `control.rs`. Linux only: the `/proc/mounts` test in `space.rs`.

### Build prerequisites

| OS | What a build needs |
|---|---|
| All three | Rust stable 1.95 or later with rustfmt and clippy. A C compiler, because `vorbis_rs` builds libvorbis and libogg. There is no `rust-toolchain` file. |
| Linux | `libasound2-dev`, `libwayland-dev` and `pkg-config` (for cpal's ALSA binding and winit's Wayland binding). |
| Windows | The MSVC build tools supply the C compiler. Nothing else is listed in the workflow. |
| Mac | The Xcode command line tools supply the C compiler. Nothing else is listed in the workflow. |

### What CI runs

`.github/workflows/check.yml` is the only workflow. It runs on every push to `main` and on every pull request. It has three jobs, and each job runs on all three of `ubuntu-latest`, `windows-latest` and `macos-latest` (fail-fast off, 60 minute limit):

| Job | Command | Linux | Windows | Mac |
|---|---|---|---|---|
| `fmt` | `cargo fmt --all -- --check` | yes | yes | yes |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` | yes | yes | yes |
| `test` | `cargo test --workspace` | yes | yes | yes |

### What CI does not cover

On every OS:

- No release build and no packaging.
- No real audio device. The tests use the `WAVCRAFT_NO_AUDIO` and fake-input paths.
- No window and no GPU. The app's drawing code never runs.
- No real VST3 plugin, no file-manager reveal, no OS clipboard.
- The `tools/alpha` scripts and the three benches do not run.
- The long edit test in `crates/testkit/tests/edits.rs` needs the 3-hour kit file, which the runners do not have.

Per OS:

- **Linux:** the runner is a headless x86 server. Neither Wayland nor X11 is exercised. The Linux handhelds and their gamescope session are not tested.
- **Windows:** the DX12 backend choice in `configure_graphics` is never exercised. The `windows_subsystem` attribute only applies to release builds, which CI does not make.
- **Mac:** the hosted runner is not the 8 GB Mac, so memory fit there is unmeasured. The craft-log report fields for operating system, kernel and processor read "unknown" on Mac, and no test checks them.

## Planned, not on main

These belong to open v1 issues (open, not labelled after-1.0). I checked each path with `ls` and each name with `grep` at HEAD. None exist yet.

| Planned piece | Issue | What is on main today |
|---|---|---|
| `crates/automation/src/mcp.rs` and an `mcp` subcommand | #1108 | The file is absent. `crates/automation/src` holds only `lib.rs`, `headless.rs` and `remote.rs`. |
| `crates/automation/src/script.rs` and File > Run Script | #879 | The file is absent. No Run Script entry in `apps/wavcraft/src/menus.rs`. |
| `apps/wavcraft/src/cli.rs` (`run`, `convert`, `inspect`) | #845 | The file is absent. `parse_args` in `main.rs` knows only a file, `--session-dir` and `--reset-preferences`. `crates/core/src/batch_cli.rs` exists, but only its own test file (`crates/core/tests/batch_cli.rs`) calls it. |
| The app driven through `wavcraft-automation` | #1107 | The crate exists. The app does not depend on it, and `control.rs` still parses its own requests. |
| MP3 save | #11 | No MP3 variant in `SaveFormat` (`crates/core/src/save.rs`). No MP3 encoder crate in `Cargo.toml` or `Cargo.lock`. |
| Transcription engine in `crates/core` | #1057 (library choice: #1056) | No transcript module and no speech model dependency. Only `crates/core/src/transcript_cut.rs` (`cut_ranges`) exists, and nothing calls it. |
| `apps/wavcraft/src/transcript_panel.rs` | #1058 | Absent. |
| Text edits applied as audio edits | #1059 | Absent. |
| Filler word and pause finder | #1060 | Absent. |
| Transcript search and text or SRT export | #1061 | Absent. |
| Episode Check runner with progress and cancel | #1049 | `check::scan` exists in `crates/core/src/check/mod.rs`. Nothing in `apps/` calls `check::`. |
| `apps/wavcraft/src/findings_panel.rs` | #1053 | Absent. |
| Suggested fixes for findings | #1054 | Absent. |
| Ready to publish summary | #1055 | Absent. |
| `apps/wavcraft/src/faces/` (Studio and Podcast switch) | #1063 | The folder is absent. |
| Episode Map | #1052 | Absent. Only the existing `WaveApp::navigator` is there. |
| Large controls preference | #1062 | Partly built. The `large_controls` key is in `crates/core/src/settings.rs` and `theme::metrics_for`/`set_large` in `apps/wavcraft/src/theme.rs`, but `apply_look` does not call `set_large` yet, Preferences has no checkbox, and the navigator in `app.rs` still passes a fixed 6.0 to `nav_hit`. |
| Hide out-of-v1 panels in release builds | #1051 | Partly built. `apps/wavcraft/src/v1.rs` hides the menus and Tools entries of seven features (multitrack, CD layout, video, batch, Mix Paste, diagnostics, VST3 plugins) in release builds, and the Keyboard Shortcuts list and the Effects Rack leave out the Diagnostics commands and DeHummer and DeReverb (`effects::OUT_OF_V1`). Still shown in a release build: DeHummer and DeReverb in the Effects menu, Window › Mixer panel, and Ogg Vorbis in the Save As filters (each needs an `app.rs` change). Its module comment says the panels are gated in `PanelSet` in `crates/core/src/panels.rs`. |

