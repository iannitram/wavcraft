# Code map

*Status per v1 item (what is on main, what is still on a branch): [ROADMAP.md](../ROADMAP.md). Checked against `main` on 2026-10-26: every file named below is present on `main` (status: present); files `main` has that the long table does not describe are listed in "Also on main" after it. The merge plan's 43 unmerged branches are not on `main`, so nothing they add is listed here.*

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
| | `effects.rs` (f01) | `Effect` trait (`id`, `name`, `parameters`, `process` of one chunk), `REGISTRY` (add an effect here), `apply` (streams a range through `replace_range`; empty range = whole file; undo step named for the effect), `Invert` (reference). Since f14: `ParamDesc` has a `ParamKind` (`Db`, `Percent`, `Time`, `Choice` + `choices`) and const constructors; `parse_params` (`key=value` text over the defaults, units and choice names allowed), `validate` (also run by `apply`), `describe`; `effects/gain.rs` is the parametrised example (`gain_db`). Since f27: `ParamKind::Frequency`/`Number`; `Effect::begin(params, &RunContext) -> Option<Box<dyn Run>>` for effects that need memory between chunks, the sample rate or a pre-measurement (`apply` calls the run instead of `process`); `effects/eq.rs` holds Parametric EQ (low shelf, two bells, high shelf), High-Pass and Low-Pass (12/24 dB/oct), Notch and Remove DC Offset, on `soundcraft-dsp` biquads, one filter chain per channel carried across the 64 K-frame chunks (tests: `crates/core/tests/filters.rs`) `Editor::run_effect(i, params, sel)` runs one with settings and remembers `Repeat::Effect(i, params)` for Repeat Last Command. f124 added `effects/reverb.rs` (Reverb: room size, decay, high damping, pre-delay, wet) and `effects/delay.rs` (Delay: time, feedback, wet), both through `Effect::begin` onto `soundcraft-dsp` (`room_reverb`, `mod_delay`) via `effects/dsp_run.rs` (`PluginRun`); b086 added `effects/plate.rs` (Plate, `plate_reverb`); the tail past the selection's end is cut (length kept); no ping-pong yet. `effects/pitch_saturate.rs` (f125): Pitch Shifter (`semitones`, `cents`; length kept; `soundcraft-dsp`'s granular shifter, fed its own latency ahead so the result lines up) and Saturator (`drive`, `curve` Soft/Hard, `output` = the peak ceiling, `mix`; soft is `soundcraft-dsp`'s tanh saturator), both as `Run`s. Not yet: changing the length with the pitch. `effects/channel_mixer.rs` (B027) is the Channel Mixer (`l_from_l`, `l_from_r`, `r_from_l`, `r_from_r`, `invert_l`, `invert_r`; fewer than two active channels = unchanged; channels 3+ pass through). `effects/time_shift.rs` (B165) is a stateful effect on the `Run` seam: Time Shift (`shift_ms`, 0 to 10 000 ms) delays the range by a ring buffer per channel carried across chunks, length-preserving (the last `shift` frames are lost), 10 s x rate x channels floats at most; tests in `crates/core/tests/time_shift.rs`. Registered effects appear as `ActionId::Effect(i)` rows `effect.<id>` in `actions::all_actions()`/`by_name`, and in the app's Effects menu. `Normalize` (`effects/normalize.rs`; registered as `effect.normalize`, which replaces the B002 one-shot row; Settings: percent or dB target, unit, one shared gain or per-channel gains, DC bias; tests `crates/core/tests/normalize.rs`) measures the range's sample peak in one streamed pass and scales the samples to the target. |
| | `generate/mod.rs`, `generate/tones_noise.rs` (f21) | `Generator` trait (`id`, `name`, `parameters` as the same `ParamDesc` list effects use, `source` per channel, `duration`), `REGISTRY`: `Tones` (base frequency, sine/triangle/square/sawtooth, `amp` peak dBFS, `dur`, 2nd to 5th harmonics as % of the base, scaled so the sum stays under `amp`; harmonics past Nyquist dropped) and `Noise` (white/pink/brown, `amp` is a peak ceiling, tiny `XorShift` RNG, seeded; channels differ). `render` makes a short in-RAM buffer for Preview. `Editor::generate(i, params, selection, seed)` inserts at the cursor or replaces the selection on every channel through `doc::edit::generate` (a producer fills 64 K-frame chunks, one new block per chunk and channel; one undo step "Generate Tones"/"Generate Noise"; not remembered for Repeat Last Command). `ParamKind::Number` (Hz) and `effects::{validate_list, parse_list, describe_list}` serve both. App: Generate menu, the effect dialog with a Preview button (a 2 s loop through the player while the dialog is up), control `GENERATE`, `GENERATE_DIALOG` |
| | `effects/dynamics.rs` (B029) | `Dynamics` (`effect.dynamics`): gate/expander + soft-knee compressor + limiter on the `Run` seam; log-domain peak detector with attack/release in `f64`, linked or per-channel gain, optional lookahead delay (first frames delayed, last lost). `static_curve(params, in_db) -> out_db` is public for a curve editor. Tests: `tests/dynamics.rs` |
| | `effects/speech_leveler.rs` (B038) | Speech Volume Leveler: a stateful `Run` (linked RMS detector, 400 ms; gain toward `amount * (target - level)` within boost/cut limits, attack/release smoothing; below `gate_db` a cut is held and a boost eases to 0 dB). No lookahead. Registered as `effect.speech_leveler` |
| | `effects/graphic_eq.rs` (B055) | `GraphicEq` (`graphic_eq`, "Graphic EQ"): a stateful effect on the `Run` seam. `bands` choice 10/20/30 (`CENTRES_10/20/30`, `Q_10/20/30` consts), `g01_db`..`g30_db` (-24..+24, sliders past the band count ignored), `master_db`. One RBJ peaking biquad per band (f64, memory per channel across chunks); 0 dB bands skipped, so all-zero is bit-identical; centres at or above 0.45 x rate skipped. Own biquad code (TODO: share with `effects/eq.rs` once #135 lands) |
| | `effects/dehummer.rs` (B073) | DeHummer: mains hum and harmonics removed. A `Run` with one RBJ peaking cut (`depth_db` at the centre, width from `q`) per kept harmonic per channel, `f64` state across chunks. Settings `frequency`, `harmonics`, `q`, `depth_db`, `skip_low` (harmonics left alone from the bottom), `gain_db`; whole-number settings are rounded; harmonics at or above 0.45 of the sample rate are skipped. Tests: `tests/dehummer.rs` |
| | `rack.rs`, `presets.rs` (f31) | `RackConfig` (slots, master power, input/output gain, Mix), `RackProcessor` (allocated once; `run_chunk`/`process_block` allocate nothing, so the audio callback can run it), `rack::apply` (one "Effects Rack" edit via `replace_range`), `Editor::run_rack`. `effects::EffectStream`/`open_stream`/`Effect::stream`: the streaming, stateful block path (stateless effects are wrapped). `Presets`: per-effect and rack presets as small text files under the config folder's `presets/`. App: `rack_panel.rs` (window), `player.rs` `set_rack` (mailbox swap with `try_lock`, one hook in the output closure). Control: `RACK ...`, `PRESET ...` |
| | `timepitch.rs` (f127) | `stretch_pitch` (WSOLA time stretch then pitch shift from `soundcraft-dsp`, the selection held in memory up to `MAX_FRAMES`; length-changing, so it builds a `Clip::from_samples` and replaces the range through `paste`, one undo step "Stretch and Pitch"), `detect_pitch` (median YIN f0 over up to 200 frames of the selection). `Editor::stretch_pitch`; control `STRETCH ratio= pitch=`, `PITCHDETECT` |
| | `stft.rs` (seam split from f29) | `Stft::new(fft_size, 50 or 75)`: Hann analysis / overlap-add resynthesis on a stateless chunk interface (`process(input, out, Placement { input_abs, out_abs, grid, warmup }, modify)`): the frame grid is fixed to `grid`, every frame touching the output chunk is analysed, so any chunking gives the same output; unmodified it reproduces the input to float rounding (test: below -100 dB). `analyse` gives one frame's spectrum; `read_padded` reads document frames around a chunk (silence outside the document) for a run |
| | `effects/fft_filter.rs` (B054) | FFT Filter: `Effect` `fft_filter` with `fft_size`, `scale` (log/linear) and up to eight `fN_hz`/`gN_db` points (a point at 0 Hz is unused; gain holds beyond the first/last point). Its `Run` reads one FFT length of document audio either side of each chunk (clipped to the range) and uses `stft.rs` on a frame grid fixed to the range start, so chunked output equals a single pass; it counts document frames itself rather than trusting `first_frame` (kept as a guard: `replace_range` now passes absolute document frames to `apply`, so the old `range.start + at` offset is gone). Tests: `tests/fft_filter.rs` |
| | `effects/dereverb.rs` (B074) | `DeReverb` (`dereverb`): stateful effect on the STFT engine. Per bin, late-reverb power = decayed power one early-reflection delay back; gain = max(floor, 1 - amount * late/total), smoothed over bins and time. Each chunk re-analyses a bounded warm-up from the document, so output is independent of chunking. Params: `amount`, `decay_ms`, `floor_db`, `smoothing`, `fft_size`. Tests: `tests/dereverb.rs` |
| | `effects/phase_correction.rs` (B076) | `PhaseCorrection` ("Automatic Phase Correction"): lines up two channels recorded with a time offset. `measure_lag(doc, range, [l, r], max_lag)` reads the range in 16 K-frame blocks and sums FFT cross-correlations (rustfft), returning the integer-sample lag and whether the peak is negative. Modes `global` (one lag for the range, measured in `begin`) and `track` (one lag per window, crossfaded over a window); `polarity=auto` flips R when the peak is negative. The early channel is delayed through a history of the last `max_shift` frames, so chunk joins change nothing; the run counts its own frames from the range start. Whole-sample lags only; two channels (others copied) |
| | `diagnostics/silence.rs` (B047) | `scan_silence` (streamed 64 K chunks; level = sample peak over the active channels; stretches of at least `min_len_ms` under `threshold_db`; capped at `MAX_STRETCHES`), `delete_silence` (each stretch shortened to `keep_ms`, half at each end, deletes applied last to first on a working state and pushed as ONE step "Delete Silence"; whole-file silence refused), `SilenceParams` (-48 dB, 500 ms, 150 ms). `Editor::delete_silence`, `ActionId::DeleteSilence` (Edit menu, `edit.deleteSilence`), control `DELETE_SILENCE`. Markers are not on this base |
| | `diagnostics/` (B045) | `mod.rs`: `Region { channel, start, len, peak }`. `declip.rs`: `Params` (`threshold_db` -30..0, default -0.1; `min_run` 2..1000, default 3; `parse`), `scan` (streamed, 64 K chunks, up to `MAX_REGIONS` 100,000 then `truncated`), `declip` (scan, least-squares cubic over 4 good samples each side of each run, one `replace_range` from first to last run, undo step "DeClipper"; runs over `MAX_REPAIR_RUN` 256 or at a file edge are skipped). Action `diagnostics.declip` (`ActionId::DeClip`, `Editor::run_declip`), control `DECLIP`, Effects menu item |
| | `diagnostics/declick.rs` (B046, B072) | `Params` (`sensitivity` 1-100 default 50, `max_width_ms` 0.1-5 default 1; `parse`), `AutoParams` (`threshold` = the same percent, `complexity` 2-100 → `max_width_ms = complexity/20`), `scan` (streamed in 64 K chunks with state carried over joins: second difference vs a running median scale, high/low thresholds, runs wider than `max_width_ms` dropped; the first difference too, so the two edges of a flat-topped pulse pair into one click; absolute floors near -80 dBFS; up to `MAX_REGIONS`), repair by least-squares autoregressive interpolation (order 16, 256 samples of context; a line near file edges; edge/silence skipped; regions over `MAX_REPAIR_WIDTH` 256 frames reported but skipped), `declick` (undo step "DeClicker"), `auto_click_remover` as `auto_click_remove` ("Automatic Click Remover"). Shared `diagnostics::write_fixes` (DeClipper uses it too). Actions `diagnostics.declick`, `diagnostics.autoClickRemover`, controls `DECLICK`, `AUTOCLICK`, Effects menu items |
| | `apps/wavcraft/src/diagnostics_panel.rs` + `diagnostics::Tool` (B050) | Effects › Diagnostics… window (a modeless window opened from Effects, as the Effects Rack is; not a `panels.rs` registry entry): pick DeClipper or DeClicker and its settings, Scan on a worker thread (cancel; spinner, no percentage), list of regions (time, channel, length, peak, skipped), click a row to select it, Repair selected / Repair All as one undo step named for the tool, list marked stale once the document differs from the scanned one. Core: `Tool::{scan, skipped, repair}` repair any subset of regions through one `replace_range`. Control `DIAG scan <declip\|declick> [k=v]` → `OK regions=N skipped=M`, `DIAG repair <all\|i,j,k>` |
| | `autosave.rs` (A239, f10) | Auto Save and crash recovery: `write_journal` (store `sync`, then `journal.wcj.tmp` owner-only, fsync, rename, fsync folder), `Snapshot`, `Autosaver` (worker thread, newest snapshot only, pins the blocks of the journal last made durable so their packs are not deleted), `recoverable_sessions`, `recover` (takes the session lock, `Store::adopt`s the packs frozen, checks every block against its pack, rebuilds states and `History::restore`; on any error every file is as found), `discard` (the only thing that removes a session with a journal). `stale_sessions`/the sweep skip any folder with a journal or a half-written one |
| | `doc/history.rs` (task 04) | `History`: steps of `(label, Arc<DocState>, made_bytes, change)` and a cursor; `push`/`push_edit` (drops redo steps; a no-op records nothing), `undo`, `redo`, `go_to`, labels, `clear`; the disk cap (bytes the kept steps hold beyond the current state, kept by per-block step counts; drops the oldest steps and says how many), `default_cap`, `can_afford` (keeps 2 GB free) |
| | `space.rs` (item 03b) | `store_bytes(frames, channels)` (4 bytes a sample + 1 % + `MARGIN_BYTES` 64 MiB), `frames_from_file_size` (at `ASSUMED_BITRATE` 128 kbit/s), `free_space(path)` (since W7, 2026-10-16: on Unix `statvfs` of the folder itself through `rustix`, so a tmpfs `/tmp` or `/dev/shm` reports its own figure; on Windows, or if that fails, sysinfo's disk list, longest mount point; Linux-only test `a_tmpfs_folder_reports_its_own_free_space`), `FreeSpaceFn`, `format_bytes` |
| | `settings.rs` (item 03b) | `Settings` (`settings.txt` in `dirs::config_dir()/wavcraft`: `session_folder = …`), `check_folder` (make it, write a probe), `resolve_session_folder` (flag > `WAVCRAFT_SESSION_DIR` > settings file > default; a folder that fails falls back to the default with a warning), `HOW_TO_CHANGE` Since f20: also `crossfade_cut_join_ms`/`crossfade_edge_ms`, `output_device`/`input_device` (J5, also read by recording) (`get`/`set` by key, `KEYS`, `SYSTEM_DEFAULT`), and `reset_aside` (moves the file to `settings.txt.reset-N`; `--reset-preferences`, Shift at launch). The app's Edit › Preferences… dialog (`app.rs`) has General, Data, Markers and Metadata, Playback and Recording, Spectral Displays, Time Display, Loudness and Diagnostics, Appearance and Audio Hardware pages; control verbs `PREF get|set`, `DEVICES`; `player.rs` takes the output device by name (`set_output`) and lists devices on a worker thread (CoreAudio can block while listing). `hidden_panels` stays alongside; the file writes only values that differ from the defaults, so a default file keeps the old text Since f67/f83/f94: `src_quality`, `save_markers`, `save_metadata`, `marker_name_pattern`, `autoscroll`, `jkl_speed_steps`, `preroll_s`, `postroll_s`, `record_offset_ms`, `spec_window`, `spec_resolution`, `spec_db_range`, `spec_play_selected_freqs`, `loudness_target_lufs`, `true_peak_ceiling_dbtp`, `zoom_factor`, `extend_selection`, `context_channel_editing`, `theme`, `waveform_colour`, `ui_brightness`, `color.<item>`, `spec_palette`, `ui_contrast` and `appearance_preset` lines (one per saved preset) are typed fields; `time_format`/`frame_rate` were dropped for `time_display`, which the Time Display page sets through `set_time_format`; the file writes a new key only when it differs from its default |
| | `appearance.rs` (f94) | Colour parsing and text (`parse_hex`, `format_hex`, `parse_colour`, `colour_text`), `ITEMS` (waveform, selection, cursor, playhead, background), `Colors` (`get`/`slot`, per-item overrides), `Preset` (`to_value`/`from_value`), `MAX_PRESETS`, `check_name` |
| | `panels.rs` (A008) | `REGISTRY` of panels (`id`, `title`, `default_dock`: waveform, spectral, history, freq), `PanelSet` (which are shown; `apply_action` for `window.toggle.<id>` / `window.show.<id>` / `window.hide.<id>`; `report`). Hidden ids are kept in `settings.txt` as `hidden_panels`. The app's Window menu lists the registry; control verb `PANELS` and `ACTION window.*`. A048 (additive): panel menus (the ≡ button): `STANDARD_ITEMS` (Close Panel, Maximize; run by `close_panel` and `Layout::apply_command maximize`), `PANEL_ITEMS` (a panel's own, e.g. history `clear_history`), `FILE_LISTING_PANELS` (adds `metadata_display`), `items_for`, `menu(panel, maximized)`; `MetadataField`/`MetadataDisplay` (settings key `metadata_fields`); control verbs `PANELMENU`, `METADATA` |
| | `jobs.rs` (f06, B003) | `Job::spawn_replace`: a `replace_range` on a worker thread with progress (thousandths), cancel (the same flag), a wake callback, and the `Edit` handed back through `poll()`; the window pushes the undo step. A panic or producer error is an `Err`. Float headroom (B012) needs no code: the store is `f32`, nothing clamps, `save::quantize` clamps at Save (tests in `crates/core/tests/jobs.rs`). In the app: `WaveApp.running`, `start_job`/`finish_job`, status-bar progress and Cancel, `STATE` has `busy=1` and `progress=<0-100>`, control `JOB testslow <ms>` / `JOB cancel` (test-only) |
| | `io.rs` (task 03) | `open(path, opts, progress, cancel) -> DocState`: our own WAV/RF64/AIFF chunk reader converting exactly as audio-io, `symphonia` packet by packet for the rest; `OpenError`. Since item 03b: `OpenOptions::free_space` (injectable), a space check before the first block is written (`OpenError::NoSpace`), a full disk while writing (`OpenError::DiskFull`, session removed). Since the Legion Go review (W8a, 2026-10-16): a session folder that can't be made or written is `OpenError::SessionFolder { folder, reason }`, which names the folder and `HOW_TO_CHANGE`, not "cannot read the file" (tests: `crates/testkit/tests/hostile.rs`). W8b: numbers from the header are checked before anything is written: a sample rate of 0 or above `MAX_SAMPLE_RATE_HZ` (768,000) is refused (`check_rate`, all three readers), a WAV block-align that isn't channels × bytes a sample is refused (PCM and float only), a sample size under 8 bits opens as whole bytes with a warning (`small_sample_warning`); data shorter than the header claims already opened what is there with a warning in `SourceFormat::warnings`, now also shown by the app |
| | `panels.rs` docking (f24: A002, A003, A006, A007) | `Layout`: zone (left, right, bottom; editor panels stay in the centre) and tab order per panel, front tab, zone sizes, the maximised panel; `encode`/`decode` for `panel_layout` and `zone_sizes` in `settings.txt`; `apply_command` for control verb `PANEL move\|maximize\|close\|activate`. The app draws tab strips (drag a tab, right-click › Move to, wheel to change tab), ` maximises and Ctrl+W closes the panel under the pointer. Own layout code, no docking crate |
| | `panels.rs` floating (f50: A004) | `Float` (panel id, window position and size in screen points, the zone it came from), `Layout::float`/`float_group`/`dock`/`set_float_rect`/`floats`/`is_floating`; floats are the `float:` part of the `panel_layout` text (`id@x_y_w_h_zone`), so settings files and workspaces carry them. Control: `PANEL float <id>`, `PANEL dock <id> [zone]`; `PANELS` adds `floating=` and `float.<id>=x,y,w,h` (and `float_window=os\|embedded`). App: `floating_windows` uses `show_viewport_immediate` (own OS window; egui falls back to an in-window `Window` by itself); right-click a tab › Undock Panel / Undock Frame; the window's Dock button or closing it docks the panel back |
| | `workspaces.rs` (f41: A009, A010) | `Snapshot` (hidden panels + `Layout::encode` text), `Workspaces` (files `<config dir>/wavcraft/workspaces/<name>.workspace`; built-in `Default` never written, never replaced or deleted; `save`, `switch`, `reset`, `delete` (not the one in use), `is_modified`, `report`). The one in use is `workspace` in `settings.txt`. App: Window › Workspace; control verbs `WORKSPACES`, `WORKSPACE save\|switch\|reset\|delete <name>` |
| | `analysis/freq.rs` (A309-A312) | `spectrum_at` (one windowed FFT centred on a frame), `scan` / `scan_with_progress` (power average over a range, 50 % hop, streamed in 64 K-frame chunks, cancellable), `FftWindow` (Hann, Blackman, Rectangular), `Spectrum` (dB per bin per channel plus channel mean, `peak`, `graph_data`). 0 dB is a full-scale sine |
| | `flac_enc.rs` (f35) | Streaming FLAC encoder (private): 4096-frame blocks, CONSTANT/FIXED 0-4/VERBATIM subframes, partitioned Rice; STREAMINFO written first (total known), MD5 left zero |
| | `save.rs` (task 06, f35) | `save(doc, target, SaveFormat::{Wav,Aiff,Flac}, SampleType, progress)`: streamed 64 K frames at a time, temp file + sync + rename (see the module doc); WAV is RF64 above 4 GiB (`save_wav_rf64_over` lowers the threshold for tests); AIFF is big-endian, 32-bit float as AIFF-C `fl32`, refused above 4 GiB; FLAC 16/24 bit only (`SaveFormat::fit`). f87: `AiffKind` (classic AIFF, AIFF-C `NONE`, AIFF-C `sowt` little-endian; float stays `fl32`), `save_aiff_kind`, `estimate_size` (exact for WAV/AIFF from the header writer, sampled estimate for FLAC) |
| | `export_presets.rs` (f87) | `ExportChoice` (format, sample type, `AiffKind`), `ExportPresets` (named choices in `export-presets.txt` beside the settings file; tab-separated; atomic replace) |
| | `document.rs` | `Document`: the open file as the app sees it: streamed open, then read back whole (`audio`) until task 05, the `DocState` kept in `state`, per-channel waveform peaks, `waveform_columns()` for drawing, `warnings()` (the open's `SourceFormat::warnings`, since W8b) |
| | `spectrogram.rs` | The spectral display maths: `SpectrogramParams`, `Columns` (file-fixed column grid), `Overview` (coarse per-channel cache), `render()` / `render_columns()`. Private: `Analysis` (FFT plan, Hann window, dB→level table), `Analyser` (FFT scratch, per-bin max) |
| | `multitrack.rs` (f103) | `Multitrack` over `soundcraft_model::Session`: new session (name, rate, bit depth), add / remove / rename audio tracks, mute / solo / volume, clips from a file, a buffer or an open `Document` (in-RAM sources, capped at `MAX_IMPORT_SAMPLES`; long files wait for the fork's disk-backed sources, #302), `state_json`. All SoundCraft crates are pinned to one commit (root `Cargo.toml`). f110 (C084): session markers and ranges are the model's memory locations (`add_marker`, `rename_marker`, `delete_marker`, `next_marker` / `prev_marker` move the playhead, `set_playhead`, `set_selection`, `add_marker_at_playhead`, `add_marker_from_selection`; addressed by 1-based position in time order), shown in `state_json` and drawn on the session ruler; control verb `SMARKER` |
| | `mixdown.rs` (f111) | `mixdown(session, MixdownOptions { only_clips, normalize_db }, sessions_root, progress, cancel) -> DocState`: renders the session (or only the picked clips, the rest muted on a clone) through `soundcraft-mix`'s `RangeRender` in 4096-frame blocks into a new store, 65,536-frame blocks per channel; the optional normalise to a peak is a second streamed pass. App: Multitrack menu, click clips to pick them, control `CLIP select`, `MIXDOWN [selected] [normalize=<dB>]` (the result opens as a new unsaved document) |
| | `multitrack.rs` clip properties (f119) | `ClipProps`, `set_clip_props` (name, gain, original tempo, beats, key; checked before anything changes), `ClipTempo` kept in `Multitrack::tempo_meta` beside the session because the pinned model's `Clip` has no tempo field; `follow_ratio` (session BPM over clip BPM, reported only) |
| | `multitrack/notes.rs` (C094), `track_info.rs` (C105), `reorder.rs` (C104), `invert.rs` (C110) | Core models for the multitrack children; the app does not call them yet. `set_title` / `set_notes` validate (refused past the limit, never cut); `set_track_color`, `set_track_color_hex`, `parse_color`, `palette`, `set_track_notes`; `move_track(from, to)` reorders audio tracks with their ids and clips; `set_invert` / `invert` hold the model's `phase_invert`, and `apply_invert` flips sample buffers (mixdown does not call it yet) |
| | `mtplay.rs` (f114) | Playing the multitrack session: `render` (offline main mix over a frame range through `soundcraft-mix`), `PlaybackSession` (a snapshot with every source behind a `ReadAheadSource`; `underruns`), `SilentPlayer` (the device-free transport under `WAVCRAFT_NO_AUDIO=1`, a clock thread that renders through the mix engine). The device path is `soundcraft-playback`'s `Player`, used from `apps/wavcraft/src/multitrack_ui.rs` (`Transport`). All SoundCraft crates are pinned to one commit (root `Cargo.toml`) |
| | `mtrecord.rs` (f120) | Multitrack recording: `Multitrack::arm_track` (SoundCraft's `record_arm` + `Route::Hardware("In N")`), `plan_takes`, `place_take` (clip at the record start; a take over occupied range goes on a new active playlist, newest on top; `TakeInfo` numbers per track), `TakeWriter` (32-bit float WAV, header sizes patched and synced every 0.25 s so a killed run leaves a readable file), `Recorder` (clock-paced thread; only `WAVCRAFT_FAKE_INPUT=1` input is built; a real device is not yet) |
| | `mtrecord.rs` punch (f126) | `Punch` (range, on/off, pre-roll; `Multitrack::punch`), `clip_in_punch` (a take cut to the range: clip start, offset, length), `Multitrack::place_take_in` (old clips the range cuts through keep their parts outside it; the new take goes on a new playlist on top); `Recorder::punch`. Over the model, repeating SoundCraft's `io::add_recording_in` arithmetic (that works on its `Engine`, not used here). Control: `PUNCH set|on|off|preroll` |
| | `session_file.rs` (f117) | The session file: the model's own JSON, extension `.wcsession`. `Multitrack::save_to` (source paths relative to the session file when within 3 folders, else absolute; audio with no file behind it written to `<name>-media/` as float WAV; atomic, owner-only, synced: `save::write_file_atomic`), `Multitrack::open_from` (media missing, unreadable, or changed since the save opens that source offline with a message; `offline_sources`, `clip_offline`; `state_json` has `offline` and `gain_db` per clip). Control: `SESSION save <path>`, `SESSION open <path>` |
| | `multitrack/automation.rs` (f109) | Volume and pan automation lanes on a track, over the model's `AutomationLane`: `automation_add` / `_move` / `_delete` / `_thin`, `automation_value_at` (linear between points, held at the ends, the parameter's default with no lane), `automation_json`; `parse_param` accepts `volume` and `pan` only. The Multitrack view shows a lane per track (Auto toggle): click adds, drag moves, right-click deletes, Thin; control verb `AUTO` |
| | `multitrack/automation_shape.rs` (C179) | Segment shapes for automation lanes: `SegmentShape` (Linear / Step / Hold), `SegmentShapes`, `shape_value`, `set_segment_shape`. Core only: playback still ramps linearly and the shapes are not saved yet. |
| | `view.rs` | `Viewport` (zoom/scroll in frames), `Selection` (anchor/head), `format_time` |
| | `examples/spectrogram_cost.rs` | Measures overview build and far-view render times on a synthetic 3-hour mono file |
| `apps/wavcraft` (`wavcraft`, binary, egui/eframe 0.36, wgpu DX12 on Windows) | `main.rs` | Window options, DX12-only wgpu on Windows, `parse_args` (`[--session-dir DIR] [FILE]`), runs `WaveApp` |
| | `freq_panel.rs` (A309-A312) | Window › Frequency Analysis: log/linear axis, lines/area/bars, FFT and window combos, cursor-following spectrum, Scan Selection on a worker thread, up to 8 Hold overlays, Copy Graph Data; `control` is the `FREQ` verb |
| | `app.rs` | `WaveApp`: menu (File › Session Folder… / Use the Default Session Folder since item 03b), status bar, the editor painter (ruler, waveform lanes, spectral lanes, selection, cursor, playhead), input (wheel zoom, drag select, Space, Home, `\`, Ctrl+O, drop), opening on a worker thread; the status bar shows the open's first warning (all of them on hover) since W8b |
| | `effect_dialog.rs` (f14) | `EffectDialog`: draws any effect's `ParamDesc` list (slider per dB/percent/time, combo box per choice) with OK / Cancel / Reset; the app applies the values. Control: `EFFECT`, `EFFECT <name> key=value ...`, `EFFECT_DIALOG ...` |
| | `recovery.rs` (f10) | Drives Auto Save in the window: offers snapshots (at most one per 2 s, none for an unchanged unsaved-free document), scans for recoverable sessions then sweeps (one thread, so lock probes never overlap), the "Recover unsaved work" window; control verbs `AUTOSAVE`, `RECOVER`, `DISCARD_RECOVERY` |
| | `player.rs` | `Player`: cpal output stream, play/stop, playhead position |
| | `spectrum.rs` | `Renderer`: the spectrogram worker thread and its overview cache |
| | `mixer.rs` (f104, C152) | The Mixer panel (Window › Mixer panel): `MixerModel` holds only a `soundcraft_mix::MixEngine` that meters the multitrack session's `Session` (f113: every call is given `&mut Session` from `MtView`; one strip per audio track plus the master, which `Multitrack::new` adds; no session, no strips); fader in dB through the model's taper (`fader_pos_to_db`/`fader_db_to_pos`), pan through `soundcraft_dsp::pan` (constant power, -3 dB centre), mute and solo by the engine's rule (a solo mutes the others); the strip meter is the engine's, post-fader and before the pan. Control: `MIXER show\|hide`, `MIXER state`, `MIXER set <n\|master> vol= pan= mute= solo=`. Scripted run: `tools/alpha/f113.sh` |

### Also on main (added after the table above was written; one line each, from the file's own header)

All present on `main`. The first column is the file under `crates/core/src/` unless a path is given.

| File | What it holds |
|---|---|
| `actions.rs` | The edit actions: one table of ids and menu labels, and `Editor`, which runs them on a document's `History`; the one-shot effects, zero-crossing adjusts and Repeat Last Command run through it |
| `shortcuts.rs` | Keyboard shortcuts as data: one table of command to key chord, apart from the action table (I3) |
| `save.rs`, `atomic.rs` | Save: the document streamed out 64 K frames at a time (WAV); crash-safe writing (temp file beside the target, sync, rename) |
| `export_chunks.rs`, `name_template.rs` | The arithmetic for a streamed export in chunks; the output-name template for Save and Export (`{name}-{n:3}`) |
| `meta.rs`, `meta/source_link.rs` | WAV metadata a save keeps (`bext`, `LIST`/`INFO`); the link from a mixed-down file back to its session |
| `autosave.rs`, `random.rs` | The autosave journal an open session writes for crash recovery (I4); random names and tokens from the operating system's CSPRNG |
| `take_readout.rs`, `take_repair.rs`, `take_recover.rs` | J4 recording readouts as text; J3 header repair of a take left by a killed app; finding and repairing such takes on disk |
| `levels.rs`, `levels/rms.rs` | Peak level meter maths (bars, hold, clip light); RMS reading |
| `meters/mod.rs`, `meters/levels_meter.rs`, `meters/levels_scale.rs` | The Levels meter (B4): per-channel peak, hold, clip and RMS; its 60 dB scale and colour zones |
| `meters/amplitude_stats.rs`, `stats.rs`, `loudness.rs` | Amplitude Statistics (G4); per-channel statistics (peak, DC offset, clipped runs, RMS); offline loudness (integrated, range, true peak) |
| `meters/phase.rs`, `meters/phase_bins.rs`, `meters/phase_scope.rs`, `meters/rms_histogram.rs` | Phase correlation, spectral phase numbers, phase scope data, RMS histogram (core only, no panel) |
| `effects/oneshot.rs`, `effects/pitch_presets.rs`, `effects/compare.rs`, `effects/dynamics_graph.rs` | One-shot effects (Reverse, Amplify, Normalize, Fades, clip guard); pitch presets; A/B compare of two settings sets; the Dynamics transfer-curve points |
| `effects/delay.rs`, `effects/reverb.rs`, `effects/plate.rs`, `effects/eq.rs`, `effects/pitch_saturate.rs`, `effects/dsp_run.rs` | Effects wrapping `soundcraft-dsp` plugins (delay, reverb, plate, filters and EQ, pitch shifter, saturator) and their shared run plumbing |
| `dsp/mod.rs`, `dsp/pan.rs`, `dsp/midside.rs`, `dsp/stereo_width.rs`, `dsp/envelope.rs`, `dsp/fade_curve.rs` | Channel-layout and pan helpers, mid/side, stereo width, fade envelope and fade curve shapes |
| `dsp/echo.rs`, `dsp/chorus.rs`, `dsp/flanger.rs`, `dsp/phaser.rs`, `dsp/distortion.rs`, `dsp/bass_enhancer.rs`, `dsp/doppler.rs`, `dsp/varispeed.rs`, `dsp/convolution.rs`, `dsp/sci_filter.rs`, `dsp/eq_graph.rs`, `dsp/band_only.rs`, `dsp/speech_ranges.rs` | Further DSP building blocks (modulation effects, distortion, bass exciter, Doppler path, varispeed, convolution, analogue-prototype filters, EQ graph mapping, band-only filter, speech/silence segmentation); none is in the effects registry or called from the app yet |
| `generate/mod.rs`, `generate/tones_noise.rs`, `generate/dtmf.rs` | Generators: Tones, Noise, DTMF |
| `timepitch/fit.rs`, `tempo.rs`, `transcript_cut.rs` | Stretch to fit a duration; tempo calculations; edit-by-transcript frame ranges |
| `smpte.rs`, `video_snap.rs`, `spectral_dirty.rs`, `ruler.rs` | SMPTE timecode and frame snapping; snap tied to a video's frame rate; which spectrogram blocks an edit makes stale; the multitrack time ruler model |
| `io/channel_mask.rs`, `cd/mod.rs`, `cd/codes.rs` | WAV channel-mask utilities; ISRC and EAN-13 validators |
| `multitrack/timeline.rs`, `multitrack/track_height.rs`, `multitrack/properties.rs`, `multitrack/marker_export.rs` | Multitrack timeline view model, track-height presets, session properties, marker CSV export and import |
| `mix/mod.rs`, `mix/downmix.rs`, `mix/band_plan.rs`, `mixer_sections.rs`, `strip_eq.rs`, `mtpunch.rs` | Downmix, the frequency band splitter plan, show/hide mixer sections, the strip quick EQ, Punch Again memory |
| `recent_sessions.rs`, `sc_source.rs`, `settings/multitrack.rs`, `settings/video.rs` | Recent sessions list; a document as a SoundCraft clip source; Multitrack and Video preferences |
| `apps/wavcraft/src/control.rs`, `selflog.rs`, `reveal.rs`, `win_acl.rs` | The loopback control channel (off unless `WAVCRAFT_CONTROL_PORT`); the app's own log; reveal a file in the file manager; Windows owner-only token file |
| `apps/wavcraft/src/meters_ui.rs`, `statspanel.rs`, `video.rs`, `i3_shortcut_help_tests.rs` | The Levels panel and statistics window; Window > Sample Statistics; the Video panel; the locked I3 shortcut-help tests |
| `crates/craft-log`, `crates/craft-stage` | Self-logging for the Craft apps (log reader, crash and stall reports); stage markers so a stall can say where it stuck. Ours, never offered upstream |
| `tools/golden.py`, `tools/winlist.swift`, `tools/alpha/` | The golden task script; a macOS window lister; the scripted checks of the alpha features (`fNN.py` / `.sh`) |

Workspace: `Cargo.toml` at the root; `unsafe_code = "forbid"` for every crate that inherits the
workspace lints. Members: `crates/core`, `crates/testkit`, `crates/craft-log`, `crates/craft-stage`, `apps/wavcraft`. The test counts that stood here (21 in the core, 9 in the app) predate task 02 and were dropped; run `cargo test --workspace` for the count.

## Known differences from Audition

Choices made where Audition 26.5.0.82 was not seen (`plan/tasks/audition-check.md`, 2026-10-16
Result, has what was seen):

- **One-channel Paste near the end of the file:** the clip is cut short at the end; the length
  never grows. Audition was only seen pasting mid-file.
- **One-channel Paste over a selection:** the clip overwrites from the selection's start for the
  clip's length, whatever the selection's length. Audition was only seen pasting at a cursor.
- **Zero crossings on stereo (task 07c):** an edge moves to where the *sum of the active channels*
  changes sign (or is exactly 0), on its side, within 10 ms, never at the edge itself (an edge
  already on a crossing steps to the next one, Adobe's "next zero crossing"; not yet checked in
  Audition by eye); with none there it stays, and an
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

- `cpal` 0.16 **directly**, in `apps/wavcraft/src/player.rs`. The single-file editor does not use
  SoundCraft's `playback` crate (its `Player::new` takes a SoundCraft `Session`); the multitrack path does (`mtplay.rs`, `multitrack_ui.rs`).
- Default host, default output device, its default config (f32, i16, u16 or i32 samples). The
  stream is opened at start-up and reopened on Play after an error or when there was none.
- Shared state: `Mutex<Option<Arc<AudioBuffer>>>`, `pos` and `end` as `f64` bits in `AtomicU64`
  (file frames), `playing` and `failed` as `AtomicBool`.
- The callback converts the file's rate to the device's by linear interpolation, averages all
  channels onto a mono device, writes the new `pos` back and clears `playing` at `end`.
- **Playhead to the UI:** `Player::position()` reads `pos` atomically; while playing the editor draws
  the playhead there, keeps it on screen (`Viewport::reveal`) and calls `request_repaint()` each frame.

## soundcraft-audio-io (this section was written at `3527bb4`, tag `wavecraft-pin-2026-10-12`; `main` now pins `274f788`)

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
`serde`, `thiserror`). Both are dependencies on `main` now (root `Cargo.toml`, pinned `274f788`).

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
