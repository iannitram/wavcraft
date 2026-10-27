# Roadmap (pipeline v2)

The plan from here to 1.0. The spec is [SPEC.md](SPEC.md) (the 55 items of [docs/v1/v1.md](docs/v1/v1.md)); the code map is [docs/code-map.md](docs/code-map.md); the stage is in [PIPELINE.md](PIPELINE.md). The old fleet tracker (Qwen run log, 106 merged / 338 PR / 241 unwritten parity rows) is kept unchanged as history in [docs/archive/roadmap-v1-fleet.md](docs/archive/roadmap-v1-fleet.md); its rows outside v1 are after 1.0.

Written 2026-10-26 from `origin/main` at `a03cfa3` and the merge plan of 2026-10-26 (paused at 7:40 pm, on the build machine's scratch clone).

## Status of every v1 item

How to read it:

- **built**: the code is on `main`, with tests where the row asks for one. Most rows' "Done" also needs a measurement written in a report or a by-ear check; none of those is recorded yet, so every row still needs its Done check before it counts.
- **partial**: part is on `main`; the rest is on the named branch or not started.
- **on branch**: the code exists only on an unmerged branch of the merge plan (number = its row in the plan). "merged locally" means merged in the scratch clone, which is not pushed yet.
- **not started**: nothing found on `main` or on a merge-plan branch.
- **Issue**: the open GitHub issue for the item (open, v2-normalized 2026-10-26); `none` = no issue was ever filed (the item is built). A `#N` in the Where column is a row of the merge plan, not an issue number.

| # | Item | Issue | Status | Where |
|---|---|---|---|---|
| A1 | Open, streamed into the store | none | built | `core/io.rs` (WAV, BWF, RF64, AIFF, MP3, FLAC via symphonia), `core/store.rs`; tests `testkit/tests/open.rs` |
| A2 | Save / Save As, atomic | #10 | partial | WAV save on main (`core/save.rs`, `core/atomic.rs`, tests `atomic.rs`, `save.rs`); AIFF and FLAC on `feat/ap-f87-export-options` (#50) |
| A3 | Save As MP3 | #11 | not started | encoder is the `craft-mp3` crate (its own repo, heading for v1.0.0); not yet a dependency here |
| A4 | Convert Sample Type, dither | #12 | on branch | `fleet/wav-c-12-b` (#91, A4 dither) and the integration branch's A163 dither and f11 Convert Sample Type (#89); the plan reconciles them |
| A5 | Files panel, several files | #13 | on branch | f04 multiple documents in `feat/ap-f72-sync-files` (#87), f30/f57 in `feat/ap-f57-open-many-append` (#88) |
| A6 | Markers survive a save | #14 | on branch | `fleet/api-recut-markers` (#58), inside `feat/ap-f51-save-options`'s metadata guard (#53) |
| B1 | Waveform, amplitude ruler | #15 | partial | waveform and peaks on main (`core/document.rs`, `core/levels.rs`); amplitude ruler (f19) on `feat/ap-f68-channel-display` (#81) |
| B2 | Spectral Frequency Display | #16 | partial | display and show/hide on main (`core/spectrogram.rs`); scale, resolution and divider on `feat/ap-f65-spectral-display` (#84), `feat/ap-f92-spectral-scale` (#85) |
| B3 | Zoom and navigator | #17 | partial | zoom in/out, to selection, out full on main (`core/actions.rs`); navigator (f19) on #81, zoom presets on `feat/ap-f69-zoom-presets` (#83) |
| B4 | Levels meter | #18 | built | `core/levels.rs`, `core/meters/levels_meter.rs`; tests `b4_levels_meter.rs`, `b4_levels_scale.rs` |
| B5 | Selection/View panel | #19 | built | `apps/wavcraft/src/app.rs` (typed begin/end/duration), formats in `core/view.rs`, `core/smpte.rs`; Go to Time (f66) on `feat/ap-f66-time-entry` (#62) |
| C1 | Time Selection tool | #20 | built | `core/actions.rs` (Select All, Deselect), tests `i3_time_selection.rs` |
| C2 | One channel of stereo | #21 | partial | Left/Right Channel Only and `ChannelMask` on main (`core/doc/edit.rs`); Enable Channels submenu and context-sensitive drag on `feat/ap-f42-channel-select` (#80) |
| C3 | Snapping | #22 | partial | frame snapping only on main (`core/video_snap.rs`); snap to markers, ruler, zero crossings and key S on `feat/ap-f49-snap-ruler` (#77) |
| C4 | Spectral Marquee and Lasso | #23 | on branch | `feat/b135-spectral-selection` (#75, with the f16 tool model) |
| C5 | Transport | #24 | partial | play, playhead to start/end on main (`apps/wavcraft/src/player.rs`); loop, Shift+X, J/K/L on `feat/ap-f36-scrub-shuttle` (#69, carries f23) |
| C6 | Scrub | #25 | on branch | `feat/ap-f36-scrub-shuttle` (#69) |
| D1 | Cut / Copy / Paste / Delete | none | built | `core/doc/edit.rs`, `core/actions.rs`; tests `edits.rs`, `testkit/tests/edits.rs` |
| D2 | Copy to New / Paste to New | #26 | not started | not found on main; check whether f12 clipboards (`wip/api-recut-clipboard-convert`, #57) carries it |
| D3 | Silence, Insert Silence | none | built | `core/doc/edit.rs`, `core/actions.rs` |
| D4 | Crop | none | built | `core/doc/edit.rs`, `core/actions.rs` |
| D5 | Undo / Redo, History panel | none | built | `core/doc/history.rs`, History panel in `core/panels.rs`; tests `edits.rs`, `history_restore.rs` |
| D6 | Zero-crossing adjust | none | built | six Zero Crossings actions in `core/actions.rs`; edge fix `feat/07d-edit-rest` (#15) merged locally |
| D7 | Edit crossfades | none | built | `Crossfades` in `core/doc/edit.rs`; tests `edits.rs` |
| E1 | Fade In / Fade Out | #27 | partial | one-shot fades (linear, cosine) and `core/dsp/fade_curve.rs` on main; dragging the corner handles not found |
| E2 | Amplify | #28 | built | `core/effects/oneshot.rs`, `core/actions.rs`; tests `oneshot_effects.rs`, `clip_guard.rs` |
| E3 | Normalize | #29 | partial | one-shot Normalize on main; the E3 dialog (per channel, DC) on `fleet/wav-c-29-b` (#29, merged locally) |
| E4 | Invert and Reverse | #30 | built | `core/effects.rs` (Invert), `core/effects/oneshot.rs` (Reverse); tests `effects.rs`, `oneshot_effects.rs` |
| E5 | Repeat Last Command | #31 | built | `core/actions.rs`; tests `e5_repeat_last.rs` |
| E6 | Preview | #32 | partial | dialog preview in `apps/wavcraft/src/effect_dialog.rs`; the E6 preview engine on `fleet/card-wav-129` (#30, merged locally) |
| F1 | Effects Rack | #33 | built | `core/rack.rs`, `apps/wavcraft/src/rack_panel.rs`; tests `rack.rs`, `rack_tail.rs` |
| F2 | Built-in processors | #34 | built | `core/effects.rs` REGISTRY (gain, parametric EQ, high-pass, low-pass, notch, dynamics with compressor, limiter and gate); a DeEsser comes with the integration branch (#89) |
| F3 | Presets | #35 | built | `core/presets.rs`, `apps/wavcraft/src/rack_panel.rs` |
| G1 | Capture Noise Print | #36 | on branch | `fleet/api-fx-139-stft-port` (#20, merged locally) |
| G2 | Noise Reduction | #37 | on branch | `fleet/api-fx-139-stft-port` (#20, merged locally) |
| G3 | Match Loudness | #38 | partial | loudness measurement on main (`core/loudness.rs`, tests `loudness.rs`, `match_volume.rs`); the effect on the integration branch (f33, #89) and `fleet/card-wav-38` (#92) |
| G4 | Loudness analysis | #39 | built | `core/stats.rs`, `core/meters/amplitude_stats.rs`, `apps/wavcraft/src/statspanel.rs`; tests `g4_amplitude_stats.rs` |
| H1 | Add Marker | #40 | on branch | single-file markers: `fleet/api-recut-markers` (#58) (main has multitrack markers only) |
| H2 | Markers panel | #41 | on branch | f39 Markers panel in `feat/ap-f105-playlist` (#61) |
| H3 | Next / previous marker | #42 | on branch | #58 (main's next/previous are multitrack only) |
| H4 | Markers move with edits | #43 | on branch | #58 |
| I1 | Default layout | #44 | partial | three panels on main (`core/panels.rs`); docking and floating panels `feat/ap-f101-float-monitors` (#45, merged locally), panel menus `feat/ap-f78-panel-menus` (#46, merged locally) |
| I2 | Panels and workspaces | #45 | on branch | #45 (merged locally) |
| I3 | Default shortcuts and list | #46 | built | `core/shortcuts.rs` BINDINGS, Help list; tests `shortcuts.rs`, `apps/wavcraft/src/i3_shortcut_help_tests.rs` |
| I4 | Autosave and recovery | #47 | partial | the journal on main (`core/autosave.rs`, tests `journal_frame.rs`, `history_restore.rs`); recovery v1 and its preferences on `feat/ap-f70-autosave-cache-prefs` (#49) |
| I5 | Windows, Linux, macOS | #48 | partial | built and run on Windows and Linux; no Mac build or launch recorded yet; no CI |
| I6 | Notices file | #49 | on branch | `feat/ap-f80-notices-settings` (#94) |
| I7 | Fits an 8 GB Mac | #50 | partial | the on-disk store and bounded cache are on main (`core/store.rs`, `testkit` memory bench); the Mac measurement is not done |
| J1 | Record a take | #51 | partial | take files and their recordings folder on main (`core/take_*.rs`); the record path (f25, f34, f44) on `feat/ap-f54-latency-monitor` (#73) |
| J2 | No playback while recording | #52 | not started | no check found in the code or tests |
| J3 | A crash keeps the take | #53 | built | `core/take_repair.rs`, `core/take_recover.rs`; tests `j3_take_repair.rs`, `j3_take_recover.rs` |
| J4 | While recording readouts | #54 | partial | `core/take_readout.rs` (time, size, disk left, drops; tests `j4_take_readout.rs`); the input level meter on `feat/ap-f56-input-meter` (#72) |
| J5 | Input device | #55 | built | `core/settings.rs`; tests `j5_input_device.rs`; the unplug check by hand is not done |
| J6 | Every machine | #56 | not started | needs a mic on each machine; nothing recorded yet |
| J7 | Take opens in the editor | #57 | not started | not found on main |

**Totals (55):** built 20 · partial 17 · on branch only 13 · not started 5. No item has its Done check recorded yet.

## Stages

### Stage 0: finish the merge plan, then merge it (blocks everything else)

The owner's ruling, 2026-10-26: "Stage 0, finish on the build machine (Recommended)". The merge plan is finished on the build machine's scratch clone, where it can build and test; then the result is pushed as one branch and merged through the Project Manager's review chain (pipeline rule R25). Nothing new is built for v1 until stage 0 is merged.

- **Where it stands:** 94 branches planned, 76 to merge and 18 to archive. 38 are merged in the scratch clone (not pushed). 43 are left, starting with `feat/ap-f94-appearance`, and ending with the integration branch (#89) and the late branches after it (#90-#94). 9 archives are upheld; 4 are not checked yet (`fleet/wav-c-186-a`, `fleet/api-recut-snap-and-tools-note`, `fleet/wav-c-22-a`, `wip/api-recut-zoom-time`). The review of the 38 found 16 issues (14 Easy, 2 Medium), drafted but not filed: file them as issues after the push.
- **Then:** the 30 open PRs are triaged against v1 (merge, or close as superseded), and the remaining remote branches are swept (archive tags for unmerged ones, R33). Issues are v2-normalized (in progress, 2026-10-26).
- **Done when:** the stage-0 branch is merged into `main`, CI (once added) or the full local checks pass on it, the open PRs and branches are triaged, and PIPELINE.md moves to Dev Loop.

**The merge plan's judgement calls, decided by the owner (2026-10-26, question tool):**

1. Save options, metadata "off": "Everything, like Audition (Recommended)". Off drops markers and the INFO/BWF metadata, as Audition's "Include markers and other metadata" does. `fleet/wav-c-186-a` is archived; its markers-only tests are dropped.
2. Match Loudness (G3): "One: f33, with card-38's tests (Recommended)". The integration branch's f33 is the one implementation; `fleet/card-wav-38`'s arithmetic tests are ported onto it (its expected-red locked tests become G3's acceptance checks) and its duplicate module is not merged.
3. Dither (A4): "A4, as v1.md says (Recommended)". Where `fleet/wav-c-12-b` (A4) and the integration branch's A163 differ, A4 wins: off by default, Triangular first, Noise Shaping None, remembered when on, unedited 16-bit saves bit-identical. A163's extra options are after-1.0.
4. and 5. Playback ring and Dynamics: "Accept both (Recommended)". f74's windowed-sinc resampler is grafted onto f05's read-ahead ring; main's Dynamics stays, and only the integration branch's DeEsser is ported if its Compressor/HardLimiter are redundant.

### Stage 1: Editor (v1 order 1)

A1, A2, D1, D3, D4, D5, D7, I7 (the store and the edit model), C1, C2, C3, C5, H1, H2, H3, H4, A6. **Done when** a full rough-cut edit of an episode is saved, reopened and identical.

### Stage 2: Recording (v1 order 2)

J1-J7. **Done when** the owner records a real 10-minute take on each machine, kills the app once mid-take, and gets everything back.

### Stage 3: Repair (v1 order 3)

E1-E6, G4, G3, G1, G2, F1-F3, then A4. **Done when** the owner cleans a real episode end to end and prefers the result to the current tool's, by ear.

### Stage 4: Feel (v1 order 4)

B3, B4, B5, C4, C6, I1, I2, I3, I4, A5. **Done when** the owner works without hunting for things.

### Alongside: the MP3 encoder

The `craft-mp3` crate (its own repo, at Dev Loop) reaches v1.0.0; then WavCraft depends on it by git tag and A3 is wired in.

### Stage 5: Mac checks and release (v1 order 6)

I5, I7 and J6 on the Mac (the 8 GB target), the notices file I6, A3 working, then the owner calls v1. Items v1.md does not place in a stage (B1, B2, D2, D6) go with the stage that touches the same code: B1 and B2 in stage 4, D2 and D6 in stage 1.

## Mac, PC and Linux

- **Memory target:** an 8 GB Mac. All audio lives in the on-disk store; RAM holds a bounded cache; undo costs disk, not RAM (I7, [docs/v1/architecture.md](docs/v1/architecture.md)).
- **Platform-specific code:** audio output and input through `cpal` (WASAPI on Windows, CoreAudio on macOS, ALSA/PipeWire on Linux), including recording devices and their unplug handling (J5) and the macOS microphone permission prompt (J6); file dialogs through `rfd`; free disk space through `sysinfo` and `rustix` `statvfs` on Unix; folder access rules on Windows (`apps/wavcraft/src/win_acl.rs`); file reveal (`apps/wavcraft/src/reveal.rs`).
- **Machines:** Windows (the build machine; no mic, so recording is tested with a fake input), Linux (the SteamOS handhelds, Desktop Mode; the real mic tests), macOS Apple Silicon (a Mac mini first, then the 8 GB Mac).
- **CI:** none yet (no `.github/workflows`). Adding a workflow that runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on Linux, Windows and macOS is part of stage 0's follow-up, before Dev Loop.

## Open-source pieces

In use (all from the root `Cargo.toml`):

- SoundCraft's engine crates, all pinned to one commit of our fork (`274f788`): `soundcraft-audio-io` (decoding, through `symphonia`), `soundcraft-dsp` (biquad filter designs), `soundcraft-model` and `soundcraft-time` (the disk-backed `SampleSource`), `soundcraft-mix` (metering), `soundcraft-playback`, `soundcraft-video`. MIT OR Apache-2.0. Bump all together, deliberately.
- `symphonia` (MPL-2.0, decoders, unmodified), `rustfft`, `rayon`, `cpal`, `egui` / `eframe` (fonts OFL-1.1 and the Ubuntu Font Licence), `rfd`, `bytemuck`, `dirs`, `sysinfo`, `rustix`, `getrandom`, `serde_json`, `proptest` (tests). The MPL-2.0 and OFL pieces need the notices file (I6).
- Our own: `craft-log` (self-logging), `craft-stage`, and, for A3, `craft-mp3`.

Candidates: none needed for v1 beyond `craft-mp3`. The clean-room rule stands: no GPL, LGPL or AGPL code or dependency, and Audacity's code is never read.
