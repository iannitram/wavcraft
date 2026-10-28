# WavCraft roadmap: Audition parity (ranked)

**Purpose.** This file is the roadmap. It replaces the phase table that used to be in the README. It lists every Adobe Audition feature we researched, whether WavCraft has it, and, for everything missing, the order to build it in. The goal is a working editor first: cleaning a one to three hour spoken-word file, then the rest of the parity picture.

**Date:** 2026-10-20.

**Live tally:** the pinned issue's checklist; this file is not ticked by coding PRs.

## Reuse sources

Added 2026-10-20 from reuse research (read-only reads of local clones, the crates.io API, docs.rs and READMEs; nothing was built or listened to, so quality claims are unverified and sizes are judgement). Every source is checked against WavCraft's MIT OR Apache-2.0 licence before any code is reused. Apache-2.0-only crates are fine as dependencies, but code vendored from them stays Apache-2.0-only. MPL-2.0 crates are acceptable as unmodified dependencies only (file-level copyleft; never copy their source into WavCraft files). Reuse code, never vendor-derived labels, parameter sets or presets: name everything user-facing in our own words.

| project / crate | where | version or commit | licence | compatible with MIT OR Apache-2.0 | roadmap rows served | notes |
|---|---|---|---|---|---|---|
| soundcraft-audio-io | https://github.com/storytold/soundcraft `crates/audio-io` | 6bf9b1d (repin from dc253f2; tag a fork rev) | MIT OR Apache-2.0 | yes | A161, A163, A245-A247, A253, A256, A265, C296, A075 | MP3 export, iXML, AAC gapless still in open upstream PRs (#607, #644, #814) |
| soundcraft-dsp | https://github.com/storytold/soundcraft `crates/dsp` | 6bf9b1d | MIT OR Apache-2.0 | yes | B001, B002, B028, B034, B037, B042, B047, B057, B060, B081, B103, B107, B108, B117, B165, A303, A304, A313, A317, A332-A336 | Plugin trait, ~25 processors, offline ops, meters; catalogue "thin and unverified" upstream, measure first |
| soundcraft-time | https://github.com/storytold/soundcraft `crates/time` | 6bf9b1d | MIT OR Apache-2.0 | yes | A098-A104, A107, A120, A123-A129, A146, C088, C140, C199 | Time formats, TempoMap, grid and snap |
| soundcraft plugin hosts (vst3-host, clap-host, au-host) | https://github.com/storytold/soundcraft `crates/vst3-host`, `clap-host`, `au-host` | 6bf9b1d | MIT OR Apache-2.0 | yes | B148-B151, B155, C157, C177 | No sandbox upstream; keep the asio feature off |
| soundcraft model + mix + playback | https://github.com/storytold/soundcraft `crates/model`, `mix`, `playback` | 6bf9b1d | MIT OR Apache-2.0 | yes | C074-C101, C123, C127, C151-C160, C174-C178, C185-C189, A194-A210 | Whole-file RAM model: needs a disk-backed trait (C322); Pro Tools-shaped vocabulary, rename user-facing terms |
| soundcraft engine, video, ui-egui patterns | https://github.com/storytold/soundcraft `crates/engine`, `crates/video`, `ui-egui` | 6bf9b1d | MIT OR Apache-2.0 | yes | A154, A239, A297-A300, C312, A321, C101, C149, A001-A016, A082, A235 | Copy small patterns (palette, recent list, theme, autosave); do not depend on the UI crate |
| filmcraft-audio-dsp | https://github.com/storytold/filmcraft `crates/audio-dsp` | 4327be43 (branch read, diff against main before pinning) | MIT OR Apache-2.0 | yes | B027, B029, B034, B035, B038, B045, B046, B054-B058, B067, B072-B074, B125-B130, B161, C218, C247 | Zero deps; second parameter model to reconcile; effect names follow a commercial app, rename |
| filmcraft-aac + filmcraft-isobmff (or craft-media) | https://github.com/storytold/filmcraft `crates/aac`, `crates/isobmff`; https://github.com/storytold/craft-libs `crates/media` | craft-libs e36f189 | MIT OR Apache-2.0 | yes | C258, A229, A255 | AAC is patent-pooled: legal check before shipping |
| filmcraft opus / ogg / ac3 / mpa (decode) | https://github.com/storytold/filmcraft `crates/opus`, `ogg`, `ac3`, `mpa` | main (commit not pinned in report) | MIT OR Apache-2.0 | yes | A228 | Import only |
| effectcraft-opusenc | https://github.com/storytold/effectcraft `crates/opusenc` | 8168b6f2 | MIT OR Apache-2.0 | yes | C260 | 48 kHz mono or stereo only; needs resampling in front and an Ogg writer behind |
| craft-ui | https://github.com/storytold/craft-libs `crates/ui` | e36f189 | MIT OR Apache-2.0 | yes | A001-A011, A014, A218 | Docking, tabs, floating panels; pre-1.0 API; no OS-window detach |
| craft-settings, craft-atomic | https://github.com/storytold/craft-libs `crates/settings`, `crates/atomic` | e36f189 | MIT OR Apache-2.0 | yes | A017-A019, A239, A263, A273-A275, A278, A290, C324 | Settings that never fail to load; fsync-ordered atomic writes |
| Craft app UI patterns (shortcut editors, crash hook, update check, meters, EQ editors, edit history, workspaces) | https://github.com/storytold/filmcraft, lightcraft, vectorcraft, photocraft, pdfcraft (`ui-egui` files) | per report, no single pin | MIT OR Apache-2.0 | yes | A057, A291-A295, A345, A346, C032, A213-A217, B030, B058, B006, B169, A009-A013 | Copy and adapt; strong app coupling in the shortcut editors |
| rubato | https://github.com/HEnquist/rubato | 5.0.1 | MIT OR Apache-2.0 | yes | A160-A162, A205, C237 | Pin `rubato = "5"`; measure against the in-house resampler |
| ebur128 | https://github.com/sdroege/ebur128 | 0.1.10 | MIT | yes | A041, B125-B130, B161, C198 | Test oracle for in-house loudness meters; slow release cadence |
| fft-convolver | https://crates.io/crates/fft-convolver | 0.4.0 | MIT | yes | B084, B085 | Partitioned convolution engine |
| realfft, rustfft | https://crates.io/crates/realfft | realfft 3.5.0, rustfft 6.4.1 | MIT; MIT OR Apache-2.0 | yes | A061-A069, A309, B054, B135-B144 | Real-input FFT for STFT and spectral display |
| signalsmith-stretch | https://crates.io/crates/signalsmith-stretch | 0.1.3 | MIT (wrapper and C++ library); vendored submodule licence unconfirmed | check | B106-B108, B111, B112, C129, C130 | C++14 and libclang build, MSVC untested; behind a feature flag, WSOLA as fallback |
| nnnoiseless | https://github.com/jneem/nnnoiseless | 0.5.2 | BSD-3-Clause (with RNNoise) | yes | B069, C217, C218 | 48 kHz mono speech only |
| cathar | https://github.com/vbasky/cathar | 0.8.0 | MIT OR Apache-2.0 (crate); repo shows Apache-2.0 | yes | B045, B046, B066, B072-B077, B144 | Reference algorithms only; young, whole-buffer API |
| bwavfile | https://github.com/iluvcapra/bwavfile | 2.0.1 (repo has unreleased 2.0.2) | MIT | yes | A023, A135, A183, A184, A191, A230, A253, A256, A262, A265, C275, C296 | Old dependencies; vendor-and-trim is reasonable |
| lofty | https://crates.io/crates/lofty | 0.25.4 | MIT OR Apache-2.0 | yes | A036, A266, A267, C325 | General tag layer |
| id3 | https://crates.io/crates/id3 | 1.17.2 | MIT | yes | A266, C250 | Chapters, pictures, ID3 in WAV/AIFF |
| quick-xml, xmp_writer | https://crates.io/crates/quick-xml, https://crates.io/crates/xmp_writer | quick-xml 0.42, xmp_writer 0.3.3 | MIT; MIT OR Apache-2.0 | yes | A230, A268, A269, A271, C278 | iXML and XMP sidecar; wrap writers in temp, fsync, rename |
| ogg | https://crates.io/crates/ogg | 0.9.2 | BSD-3-Clause | yes | C260 | Ogg page writer for Opus |
| flac-codec | https://crates.io/crates/flac-codec | 1.3.3 | MIT OR Apache-2.0 | yes | A247, C273, C277, C325 | FLAC metadata editor and encoder cross-check |
| vorbis_rs | https://crates.io/crates/vorbis_rs | 0.5.6 | BSD-3-Clause (vendored libvorbis C) | yes | A250 | C build, high MSRV |
| rusty-opus | https://crates.io/crates/rusty-opus | 1.0.2 | BSD-3-Clause | yes | A228, C260 | Fallback only; conformance claims self-reported |
| riff | https://crates.io/crates/riff | 2.0.0 | MIT | yes | A272 | Preserve unknown chunks on save |
| atomic-write-file, tempfile | https://crates.io/crates/atomic-write-file, https://crates.io/crates/tempfile | 0.3.1, 3.27 | BSD-3-Clause; MIT OR Apache-2.0 | yes | A239, A243, A260, C022, C032, C324 | Only if craft-atomic is not used |
| rtrb, basedrop, creek | https://crates.io/crates/rtrb, https://crates.io/crates/basedrop, https://codeberg.org/Meadowlark/creek | 0.4.0, 0.1.3, 1.2.3 | MIT OR Apache-2.0 (rtrb, creek); MIT/Apache-2.0 (basedrop) | yes | C051, C060, C061, C091, C169 | creek writes WAV only |
| triple_buffer | https://crates.io/crates/triple_buffer | 9.0.0 | MPL-2.0 | check (unmodified dependency only) | C051, C060 | Meter and transport snapshots; never copy its source |
| egui_dock | https://crates.io/crates/egui_dock | 0.21.1 | MIT | yes | A001-A011 | Fallback if craft-ui docking is insufficient; no OS-window detach |
| egui_plot | https://crates.io/crates/egui_plot | 0.37.0 | MIT OR Apache-2.0 | yes | B030, B058, A213-A217 | Response and spectrum graphs, not waveform lanes |
| egui_table | https://crates.io/crates/egui_table | 0.10.0 | MIT OR Apache-2.0 | yes | A057, A291-A295 | Virtualised tables for Files, Markers, shortcut list |
| egui_knob, egui-bind | https://crates.io/crates/egui_knob, https://crates.io/crates/egui-bind | 0.6.3, 0.18.0 | MIT | yes | B030, A057, A291-A295 | Rotary controls; key-capture widget |
| egui_kittest | https://crates.io/crates/egui_kittest | per egui 0.36 | MIT OR Apache-2.0 | yes | UI tests | Headless UI test harness |
| undo | https://crates.io/crates/undo | 0.52.0 | MIT OR Apache-2.0 | yes | A297 | No disk-reclaim hook; disk-backed store still custom |
| clack-host (alternative CLAP host) | https://crates.io/crates/clack-host | 0.2.0 | MIT OR Apache-2.0 | yes | B148-B157 | Alternative to SoundCraft's clap-host |
| VST 3 SDK | https://forums.steinberg.net/t/vst-3-8-0-sdk-released/1011988 | 3.8.0 | MIT | yes | B148-B151 | Trademark and logo terms not verified |
| dawproject | https://crates.io/crates/dawproject | 0.11.0 | MIT OR Apache-2.0 | yes | C323 | Early but usable |
| symphonia (already in the tree) | https://github.com/pdeljanov/Symphonia | via soundcraft-audio-io | MPL-2.0 | check (unmodified dependency only) | decode | Do not copy source into WavCraft files |

Conditional, not yet adopted: OxiMedia crates (Apache-2.0 per crate but the repo shows no licence assertion, and its release notes admit paths that reported success without doing the work), `xaac-rs` (libxaac licence and patents unchecked), `maolan-plugin-host` (BSD-2-Clause; study its one-process-per-plugin pattern only).

### Build ourselves

No good permissive source exists, so these are WavCraft-owned: action registry, keymap and shortcut editor data (A291-A295, A154, B001); disk-backed undo store (A297, A298, A348); noise-shaped dither (A163, C191); phase-correlation meter (A040, A317); spectral selection masks and heal brush on our own STFT overlap-add (B135, B141, A056, B144); transient warping (B109) driven by SoundCraft's transient detection; multitrack timeline, clip drawing, track headers, mixer strips, faders and automation-lane UI; metronome and count-in (C201-C205); batch job runner with resumable manifests (B158-B162, C271); out-of-process plugin scanner (B154); OS-window panel detach (A004) and four-way divider drags (A005); OMF import (C280) and probably AAF (C279). MP3 export (A248, A266) waits on SoundCraft PR #607. MP2 and APE encoders (A249, A251) were not found, but the search was not exhaustive.

### Do not use

| item | why |
|---|---|
| Rubber Band; tdpsola (AGPL-3.0); sonic-sys, sonogram, autoeq-iir (GPL-3.0+); dhvani, naad (GPL-3.0); rockbox-dsp (GPL-2.0+); camilladsp (GPL-3.0 OR MPL-2.0 application) | Copyleft DSP |
| shine-rs (LGPL-2.0), mp3lame-encoder (LGPL-3.0), lame-sys, LAME | Copyleft MP3 |
| ffmpeg-next, rsmpeg, ac-ffmpeg, shipping an FFmpeg binary via ffmpeg-sidecar | Links or ships LGPL/GPL FFmpeg |
| fdk-aac and fdk-aac-sys (the C library) | Custom licence: no patent grant, source-with-binary and naming conditions |
| xmp_toolkit | Bundles Adobe's XMP SDK; Adobe code conflicts with the clean-room rule |
| nih-plug VST3 bindings and non-example plugins | GPL-3.0 |
| Meadowlark, Generic DAW, rusty-daw-engine, rusty_link, ableton-link-rs, aubio-rs, the `au` crate | GPL |
| yadaw (AGPL-3.0), Pro-Wave, ChipWaveStudio, aulos (GPL-3.0), hallucinator-daw (no licence) | Copyleft or unlicensed; do not read for copying |
| truce framework crate | Custom licence |
| dawproject-rs | Non-standard licence, dormant |
| vst crate and VST2 hosting (B152) | VST2 SDK licensing is not permissive |
| SoundCraft `asio` feature (Steinberg ASIO SDK) | Reported moved to GPLv3; keep off |
| plugin_host, audio_graph, deesser | No repository or untrusted provenance |
| aloe-wav and other JUCE-derived code | JUCE lineage, licence unchecked |
| oxideav-opus, oxideav-aac | Self-described scaffold or conflicting claims |
| craft-launcher | No LICENSE file in the clone |
| ixml crate | Invisible XML, not the audio iXML spec |
| Brand assets (SoundCraft `docs/brand/`, the "VST" logo until terms are checked), Whisper/Kokoro model weights in FilmCraft | Trademarks or unchecked model licences |
| Any Adobe or Avid code or assets; effect names, parameter sets and presets copied from commercial apps (including those named in FilmCraft and SoundCraft) | Clean-room rule: reuse code, rename everything user-facing |

## Method and source caveat

- Three research passes (editor and app shell; effects, repair and spectral; recording, metering, multitrack and output) produced 842 rows. Rows describing the same feature in more than one pass were merged: the merged ids are listed under "Merged duplicates" and the kept row carries the original stable id (A###, B### or C###).
- Adobe's help pages refused plain fetches. Area A was partly read through a browser; areas B and C rest mostly on the researchers' own knowledge. **Verify a row against Adobe's live guide before building it.** Some rows are from memory or older versions and say so.
- WavCraft status was found by reading the code and the issue list at the time of research, not by building or running anything. "have" means code and usually a test exist by reading; it is not measured.
- **Clean room:** names and observable behaviour only. No Adobe text, icons, assets or code. Behaviour cells are written in our own words; rewrite any that drift toward the vendor's phrasing.
- "SoundCraft" is the MIT/Apache-licensed storytold/soundcraft workspace. Its engine crates (dsp, audio-io, time, playback, mix) can be reused as libraries; its DAW UI crates cannot. A row marked "wait for upstream" has open SoundCraft work (PR or issue numbers in the note); rank it low and let that land first unless WavCraft needs it in T0 to T3 (none do today).

## Counts

Unique rows after merging: **685** (from 842; 161 duplicates merged; 4 rows added 2026-10-20 from reuse research: C322 to C325).

| Status | Rows |
|---|---|
| have | 46 |
| partial | 59 |
| missing | 523 |
| upstream-in-progress | 57 |

To build (not "have"): **639**. Careful-path (CP) rows: **76**.

| Tier | To build | missing | partial | upstream-in-progress |
|---|---|---|---|---|
| T0 Seams | 11 | 5 | 6 | 0 |
| T1 Core editing | 63 | 51 | 12 | 0 |
| T2 UI shell and layout | 93 | 73 | 20 | 0 |
| T3 Playback | 16 | 10 | 6 | 0 |
| T4 Recording | 29 | 29 | 0 | 0 |
| T5 Effects rack and core effects | 56 | 55 | 1 | 0 |
| T6 Metering and analysis | 18 | 18 | 0 | 0 |
| T7 Spectral tools and repair | 29 | 29 | 0 | 0 |
| T8 Output, export, batch | 55 | 43 | 9 | 3 |
| T9 Multitrack and everything else | 269 | 210 | 5 | 54 |

## How rows are ranked

Tiers, in order: **T0 Seams** (things many features plug into: the action table and effect trait, the panel slot, the multi-document model, the store-backed views, the job runner, the effect dialog scaffold), T1 core editing, T2 UI shell and layout, T3 playback, T4 recording, T5 effects rack and core effects, T6 metering and analysis, T7 spectral tools and repair, T8 output, export and batch, T9 multitrack and everything else. Inside a tier: what unblocks the most rows, then what a podcast-cleanup user needs most, then small before large. Rows waiting on SoundCraft sort to the end of their tier; Adobe-specific rows sort to the end of T9. A row never appears before a same-tier row it depends on. A row whose dependency lives in a later tier was moved down to that tier.

**Columns.** rank; id (stable); tier; feature; behaviour (one line, own words); status; CP (careful path: touches the user's files or audio integrity; owner's rule is a careful review before merge); reuse/upstream note; reuse (short source name and licence from the "Reuse sources" table; blank where none); size S/M/L; depends-on (ids still to build); old issue # (the v1 plan issues, all still open on GitHub even where code has landed).

**Careful-path set:** document model and store, Save/Save As/export, autosave and recovery, recording, MP3 export, sample-type conversion, Noise Reduction, Match Loudness, plus metadata and marker writes to user files, batch writers and the job runner that writes through the store.

## T0 Seams (11)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | B001 | T0 | Effect trait + action table + panel slot | One-file effects registering with menu, repeat-last, undo step, dialog description | missing |  |  | soundcraft-dsp (MIT/Apache) | M |  | #58 |
| 2 | A008 | T0 | Panel registry and Window menu | Show or hide every panel from a menu with check marks; one registry of panels | partial |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | M |  | #45, #58 |
| 3 | B002 | T0 | Add soundcraft-dsp dependency | Pin dsp crate at same rev as audio-io to reuse plugins/offline/meters | missing |  |  | soundcraft-dsp (MIT/Apache) | S |  | #34 |
| 4 | A224 | T0 | Multiple open documents (tabs / Files list) | Switch between files, each with own history | missing | CP |  |  | L |  | #13 |
| 5 | A348 | T0 | Views and player read from store, no whole-file RAM copy | Engine refactor | partial | CP |  |  | L |  | #64 |
| 6 | B003 | T0 | Offline job runner (worker, cancel, progress) | Run an effect over a selection on a worker with progress and cancel, writing via replace_range | partial | CP |  |  | M | B001 | #37 |
| 7 | B006 | T0 | Effect dialog UI (params, presets, preview, Apply) | Common dialog scaffold with sliders, curve editors and Apply/Close | missing |  |  | photocraft EditHistory, filmcraft workspaces (MIT/Apache) | M | B001 | #58 |
| 8 | B012 | T0 | Float headroom kept above 0 dBFS until save | Intermediate float audio not clipped between effects | partial | CP |  |  | S |  | #28, #65 |
| 9 | B166 | T0 | Effect undo step labels | Undo names such as 'Amplify' | partial |  |  |  | S | B001 |  |
| 10 | B167 | T0 | Process on selection / whole file / channel subset | Effects honour active channels and selection | partial |  |  |  | S | B001 |  |
| 636 | C322 | T0 | Disk-backed sample source trait for soundcraft-model (unlocks SoundCraft multitrack reuse) | A trait seam so the session model reads samples from disk-backed pieces instead of whole decoded buffers | missing | CP | added 2026-10-20 from reuse research; SoundCraft model SourcePool holds whole-file RAM buffers | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | L | A348 |  |

## T1 Core editing (63)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 11 | A170 | T1 | Marker and range data model (merged: C315, B049) | Point markers and range markers with name, start, end | missing | CP |  |  | M |  | #14, #40, #41, #42, #43 |
| 12 | A110 | T1 | Drag selection edges to adjust | Grab either end of selection and move it | missing |  |  |  | S |  | #20 |
| 13 | A111 | T1 | Shift-click to extend selection | Extend to clicked point (Extend Selection pref) | missing |  |  |  | S |  | #20 |
| 14 | A239 | T1 | Auto Save and crash recovery (merged: C018) | Periodic autosave, recover after crash | missing | CP | SoundCraft engine/autosave.rs (reusable); SoundCraft PR #810 | soundcraft-engine pattern (MIT/Apache); craft-settings, craft-atomic (MIT/Apache); atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | L |  | #47 |
| 15 | A171 | T1 | Add marker (M or *) | At playhead (point) or selection (range) | missing |  |  |  | S | A170 | #40 |
| 16 | A172 | T1 | Markers panel list (merged: A026) | See p_markers | missing |  |  |  | M | A008, A170 | #41 |
| 17 | A176 | T1 | Next/previous marker | Move the playhead to the next or previous marker | missing |  |  |  | S | A170 | #42 |
| 18 | A113 | T1 | Double-click selects visible range | Selects what is on screen | missing |  |  |  | S |  |  |
| 19 | A114 | T1 | Triple-click selects whole waveform | Selects all | missing |  |  |  | S |  |  |
| 20 | A161 | T1 | Convert Sample Type dialog (Shift+T) (merged: B121, C262, C263) | Change sample rate, channels, bit depth with quality, pre/post filter, dither, noise shaping, presets | missing | CP | dsp::offline::resample upstream (PR #643 faster resample); SoundCraft dsp::offline::resample reusable; SoundCraft PR #643 polyphase resampler | soundcraft-audio-io (MIT/Apache); rubato (MIT/Apache) | L |  | #12, #65 |
| 21 | A123 | T1 | Toggle snapping button | Master switch for snapping | missing |  |  | soundcraft-time (MIT/Apache) | S |  | #22 |
| 22 | A127 | T1 | Snap to zero crossings | Snap to nearest sign change | missing |  |  | soundcraft-time (MIT/Apache) | S | A123 | #22 |
| 23 | A237 | T1 | File > Close / Close All / Close Unused Media | Close current or all files, prompt for unsaved | partial |  |  |  | S | A224 |  |
| 24 | A147 | T1 | Copy to New (merged: B164) | Copy selection into a new file/tab | missing |  |  |  | M | A224 | #26 |
| 25 | A148 | T1 | Paste to New | Create new file from clipboard inheriting sample type | missing |  |  |  | M | A224 | #26 |
| 26 | A183 | T1 | Save/load markers in WAV (cue, adtl labl/ltxt, smpl) (merged: C276) | Persist markers via Include Markers and Other Metadata | missing | CP |  | bwavfile (MIT) | M | A170 | #14, #62 |
| 27 | A182 | T1 | Markers move with edits | Markers shift when audio is inserted or deleted before them | missing | CP |  |  | M | A170 | #43 |
| 28 | A149 | T1 | Mix Paste (merged: C142) | Mix clipboard into audio with volume %s for copied/existing, invert, crossfade | missing |  |  |  | M |  | #72 |
| 29 | A303 | T1 | Generate Tones (merged: B118) | Generate a test tone: waveform shape, frequency, optional sweep and overtones, level and length | missing |  | SoundCraft dsp::offline::signal_generator reusable | soundcraft-dsp (MIT/Apache) | L | B001 |  |
| 30 | A132 | T1 | Enable channels (Edit > Enable Channels) | Pick which channels edits and selections act on | partial |  |  |  | S |  | #21 |
| 31 | A163 | T1 | Dither types and noise shaping (merged: B122, C264) | Triangular/Gaussian, shaped noise with crossover/strength/adaptive | missing | CP | audio-io EncodeOptions has TPDF dither (code-map.md); audio-io EncodeOptions::dither exists; SoundCraft audio-io EncodeOptions dither (TPDF only) available but not... | soundcraft-audio-io (MIT/Apache) | M | A161 | #12 |
| 32 | A134 | T1 | Surround 5.1 / up to 32 channel files (merged: C298) | Edit multichannel files channel by channel | partial |  |  |  | M | A132 | #21 |
| 33 | A121 | T1 | Move playhead to previous/next marker, clip or selection edge | Jump the playhead to the previous or next marker or selection edge | missing |  |  |  | S | A170 | #42 |
| 34 | A122 | T1 | Keyboard selection extension | Extend the selection from the keyboard | missing |  |  |  | S |  |  |
| 35 | A152 | T1 | Five internal clipboards (Set Current Clipboard) | Cut/copy/paste use selectable one of five clipboards | missing |  |  |  | S |  |  |
| 36 | A153 | T1 | System clipboard interop | Audition uses internal clipboards; paste from other apps not documented | missing |  |  |  | S |  |  |
| 37 | A136 | T1 | Convert channels (mono/stereo/5.1) with left/right mix % (merged: C265, B123, B124) | Part of Convert Sample Type | missing | CP |  |  | M | A161 | #12 |
| 38 | A154 | T1 | Repeat Last Command (with/without dialog) (merged: A338, B010, C317) | Repeat the last edit or effect, with or without its dialog | partial |  |  | soundcraft-engine pattern (MIT/Apache); soundcraft ui-egui patterns (MIT/Apache) | M | B001 | #31 |
| 39 | A162 | T1 | Resample quality + pre/post filter | Quality slider and aliasing filter | missing | CP | soundcraft dsp resample (PR #643) | rubato (MIT/Apache) | M | A161 | #12 |
| 40 | A169 | T1 | Edit > Insert audio from file at cursor | Insert a file contents at the playhead (not in fetched pages; from memory) | missing |  |  |  | M |  |  |
| 41 | A304 | T1 | Generate Noise (merged: B119) | Brown, pink, white; styles spatial stereo, independent, mono, inverse; intensity 2-40; DC; duration | missing |  | SoundCraft signal_generator (partial) | soundcraft-dsp (MIT/Apache) | M | A303 |  |
| 42 | A049 | T1 | Time Selection tool | Drag to select a time range, adjust its edges, extend with a modifier key | partial |  |  |  | S |  | #20 |
| 43 | A058 | T1 | Context-sensitive channel editing | Dragging in the top or bottom strip of the editor selects only one stereo channel | missing |  |  |  | S | A132 |  |
| 44 | A112 | T1 | Right-click to extend/shorten | Optional: right-click moves the nearest selection edge | missing |  |  |  | S |  |  |
| 45 | A124 | T1 | Snap to markers | Selection and playhead jump to marker points | missing |  |  | soundcraft-time (MIT/Apache) | S | A123, A170 | #22 |
| 46 | A125 | T1 | Snap to ruler (coarse) | Only major divisions | missing |  |  | soundcraft-time (MIT/Apache) | S | A123 | #22 |
| 47 | A126 | T1 | Snap to ruler (fine) | Subdivisions | missing |  |  | soundcraft-time (MIT/Apache) | S | A123 | #22 |
| 48 | A129 | T1 | Snapping menu and ruler context menu | Snapping options reachable from the Edit menu and the ruler menu | missing |  |  | soundcraft-time (MIT/Apache) | S | A123 | #22 |
| 49 | A131 | T1 | Stereo zero-crossing rule matches Audition | Rule for multi-channel not verified against Audition | partial |  |  |  | S |  | #61, #71 |
| 50 | A156 | T1 | Select submenu variants | Select variants such as the current view or the span between markers (from memory) | missing |  |  |  | S |  |  |
| 51 | A157 | T1 | Edit menu: Enable Channels, Snapping, Marker submenus | Menus grouping these commands | missing |  |  |  | S |  |  |
| 52 | A160 | T1 | Interpret Sample Rate | Reinterpret the file at a different sample rate without resampling | missing | CP |  | rubato (MIT/Apache) | S | A161 |  |
| 53 | A164 | T1 | Conversion presets | Save/delete named settings | missing | CP |  |  | S | A161 |  |
| 54 | A173 | T1 | Rename marker | Edit name in panel | missing |  |  |  | S | A172 | #41 |
| 55 | A174 | T1 | Select markers (click, shift-click, ctrl-click, double-click) | Multi-select markers in the list; double-click selects the marker range | missing |  |  |  | S | A172 | #41 |
| 56 | A175 | T1 | Drag marker handles in timeline | Reposition by dragging; edit values in panel | missing |  |  |  | S | A170 | #41 |
| 57 | A177 | T1 | Convert point to range and range to point | Turn a point marker into a range and back | missing |  |  |  | S | A170 |  |
| 58 | A178 | T1 | Merge markers | Merged point markers become a range; keeps first name | missing |  |  |  | S | A170 |  |
| 59 | A179 | T1 | Delete marker | Panel button or context menu | missing |  |  |  | S | A170 | #41 |
| 60 | A180 | T1 | Marker colour/description fields | Optional per-marker metadata (not documented in fetched page) | missing |  |  |  | S | A170 |  |
| 61 | A181 | T1 | Markers Display columns | Show/hide Duration, Type | missing |  |  |  | S | A172 |  |
| 62 | A221 | T1 | Drag-and-drop open (merged: C318) | Drop files onto window | partial |  |  |  | S |  | #66 |
| 63 | A297 | T1 | Click state to revert, arrow keys to move through states | History list | partial |  |  | soundcraft-engine pattern (MIT/Apache); undo (MIT/Apache) | S |  |  |
| 64 | A298 | T1 | Delete single state, Clear History | Free disk by removing states | partial |  |  | soundcraft-engine pattern (MIT/Apache) | S |  |  |
| 65 | A300 | T1 | Audition-style step names | Crop Audio / Paste Audio etc. | partial |  |  | soundcraft-engine pattern (MIT/Apache) | S |  | #63 |
| 66 | A150 | T1 | Mix Paste types: Insert, Overlap, Overwrite, Modulate | Insert pushes audio right; Overlap sums; Overwrite replaces; Modulate multiplies | partial |  |  |  | M | A149 |  |
| 67 | A151 | T1 | Mix Paste source: clipboard or file; Loop Paste count | Source from internal clipboard or a file; repeat N times | missing |  |  |  | M | A149 |  |
| 68 | A184 | T1 | Markers in AIFF (MARK) and other formats | Same for AIFF/others | missing | CP |  | bwavfile (MIT) | M | A183 |  |
| 69 | A219 | T1 | File > New > Audio File | Dialog for name, sample rate, channels (mono, stereo, 5.1, last 5 custom), bit depth | missing | CP |  |  | M | A224 |  |
| 70 | A223 | T1 | Open many files at once | Several open files in Files panel | missing | CP |  |  | M | A224 |  |
| 71 | A225 | T1 | Open Append to current / to new | Append files, converting to match the open one | missing | CP |  |  | M | A161, A224 |  |
| 72 | A308 | T1 | Generator dialogs with preview | Hear before applying | missing |  |  |  | M | A303 | #32 |
| 73 | A347 | T1 | Memory fits an 8 GB Mac (merged: C302) | RAM budget | partial |  |  |  | M |  | #50, #64 |

## T2 UI shell and layout (93)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 74 | A273 | T2 | Preferences dialog (Edit > Preferences) (merged: C014) | Tabbed dialog | missing |  | SoundCraft PR #819 is egui Preferences in the DAW's UI crate (not reusable as library); SoundCraft PR #819 full Preferences window (reference) | craft-settings, craft-atomic (MIT/Apache) | M |  | #44, #45 |
| 75 | A099 | T2 | Time display format selector (View > Time Display) | Pick the unit (time, samples, frames, beats) used by the ruler and the time boxes | missing |  |  | soundcraft-time (MIT/Apache) | M |  |  |
| 76 | A090 | T2 | Zoom navigator bar | Overview strip: drag to scroll, drag edges to zoom | missing |  |  |  | M |  | #17 |
| 77 | A088 | T2 | On-panel zoom buttons | Zoom in/out, in at in-point, in at out-point, to selection, out full | missing |  |  |  | S |  | #17 |
| 78 | A071 | T2 | Amplitude ruler with scale modes | Choose the vertical ruler unit: decibels, percent, sample value or normalised | missing |  |  |  | M |  | #15 |
| 79 | A072 | T2 | Vertical (amplitude) zoom | Make the waveform taller or shorter without changing the audio | missing |  |  |  | S | A071 | #15, #17 |
| 80 | A020 | T2 | Files panel | List the open files with format, length and unsaved mark; click one to switch to it | missing |  |  |  | L | A224 | #13 |
| 81 | A291 | T2 | Keyboard Shortcuts editor (Edit > Keyboard Shortcuts) (merged: C304) | Assign, remove, conflict warning, save sets | missing |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache) | L | A295 | #46 |
| 82 | A295 | T2 | Audition default shortcuts for implemented commands (merged: C303) | Keys follow Audition defaults | partial |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache) | M |  | #46 |
| 83 | A009 | T2 | Save custom workspace | Store the current panel layout under a name | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache); photocraft EditHistory, filmcraft workspaces (MIT/Apache) | M | A008 | #45 |
| 84 | A329 | T2 | Status bar customization menu | Pick which status-bar items are shown | missing |  |  |  | S |  |  |
| 85 | A002 | T2 | Dock panels to edge zones | Drag a panel onto an edge of a group to place it beside the group and resize the others | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | L | A008 | #45 |
| 86 | A033 | T2 | Time panel | Large current-time readout as its own panel | missing |  |  |  | S | A099 |  |
| 87 | A057 | T2 | Tool switching keys and options bar | Keys switch tools (E marquee, D lasso, etc. per #23) and show tool options | missing |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache); egui_plot, egui_table, egui_knob, egui-bind (MIT/Apache; MIT) | S |  | #23 |
| 88 | A030 | T2 | Selection/View panel | Begin, End, Duration boxes for selection and for the visible view, typeable in current time format | missing |  |  |  | S | A099 | #19 |
| 89 | A098 | T2 | SMPTE formats (24, 25, 29.97 df/ndf, 30, 59.94) | Frame-based timecode incl. drop-frame | missing |  | soundcraft-time crate has TimeFormat/Timecode/FrameRate (code-map.md), not a dependency yet | soundcraft-time (MIT/Apache) | M | A099 |  |
| 90 | A128 | T2 | Snap to frames | For frame-based time formats | missing |  |  | soundcraft-time (MIT/Apache) | S | A098, A123 | #22 |
| 91 | A003 | T2 | Group panels as tabs | Drop a panel on the middle or tab strip of a group to stack it as a tab; tab order draggable | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | M | A002 | #45 |
| 92 | A069 | T2 | Scroll and zoom the frequency axis | Drag in the vertical spectral ruler to scroll; right-drag to zoom into a frequency range | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | M |  |  |
| 93 | A101 | T2 | Bars and Beats format with tempo | Musical grid using tempo and time signature (Edit Tempo in Properties) | missing |  | soundcraft-time TimeFormat::BarsBeats | soundcraft-time (MIT/Apache) | M | A099 |  |
| 94 | A004 | T2 | Undock panel or frame to floating window | Float a panel or a whole group as its own window | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | L | A002 | #45 |
| 95 | A120 | T2 | Typed selection start/end/length | Enter exact values in Selection/View panel | missing |  |  | soundcraft-time (MIT/Apache) | S | A030 | #19 |
| 96 | A006 | T2 | Maximise panel under pointer with accent key | A key toggles the hovered panel between full size and its place | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | S | A008 |  |
| 97 | A007 | T2 | Close panel (Ctrl+W) and scroll hidden tabs | Close a panel from its tab or by key; scroll through hidden tabs when the strip is narrow | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | S | A003 |  |
| 98 | A010 | T2 | Reset and delete workspace | Reset returns the named workspace to its saved layout; delete removes it (not the active one) | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache); photocraft EditHistory, filmcraft workspaces (MIT/Apache) | S | A009 |  |
| 99 | A012 | T2 | Keyboard shortcuts to switch workspace | Workspace commands can be assigned keys | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); photocraft EditHistory, filmcraft workspaces (MIT/Apache) | S | A009, A291 |  |
| 100 | A018 | T2 | Reset preferences with Shift at launch | A start-up key combination resets preferences to defaults | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | S | A273 |  |
| 101 | A021 | T2 | Files panel metadata columns and sorting | Choose, reorder and sort the metadata columns | missing |  |  |  | S | A020 | #13 |
| 102 | A022 | T2 | Import file into Files panel without opening in editor | Add a file to the list without changing which one is open in the editor | missing |  |  |  | S | A020 |  |
| 103 | A032 | T2 | Zoom panel | Zoom buttons in their own panel (in, out, to selection, in at in/out, out full, zoom to time) | missing |  |  |  | S | A088 | #17 |
| 104 | A048 | T2 | Panel menus and Metadata Display dialogs | Each panel has an options menu (display columns, etc.) | missing |  |  |  | S | A008 |  |
| 105 | A050 | T2 | Hand tool | Drag to scroll the view | missing |  |  |  | S |  |  |
| 106 | A051 | T2 | Zoom tool | Drag a region to zoom into it | missing |  |  |  | S |  |  |
| 107 | A061 | T2 | Show/hide spectral display, drag divider, double-click handle (merged: B146) | Resizable split between waveform and spectral lanes; instant show/hide | partial |  |  | realfft, rustfft (MIT; MIT/Apache) | S |  | #16 |
| 108 | A062 | T2 | Spectral log/linear frequency (partial and full) | Blend the frequency axis smoothly between linear and logarithmic, or choose either extreme | partial |  |  | realfft, rustfft (MIT; MIT/Apache) | S |  | #16 |
| 109 | A063 | T2 | Spectral resolution inc/dec (Shift+Ctrl+Up/Down) (merged: B133) | Changes FFT bands: more frequency accuracy vs time accuracy | partial |  |  | realfft, rustfft (MIT; MIT/Apache) | S |  | #16 |
| 110 | A064 | T2 | Spectral windowing function choice | Pick the window function used for the spectral transform | partial |  |  | realfft, rustfft (MIT; MIT/Apache) | S | A273 |  |
| 111 | A065 | T2 | Spectral decibel range | Adjust dB range that maps to colours | partial |  |  | realfft, rustfft (MIT; MIT/Apache) | S | A273 |  |
| 112 | A070 | T2 | Frequency ruler | Vertical ruler beside the spectral lane with Hz labels | missing |  |  |  | S |  |  |
| 113 | A073 | T2 | Layered / uniquely coloured channels | Option to draw stereo or surround channels in separate lanes or layered | missing |  |  |  | S |  |  |
| 114 | A074 | T2 | Channel indicators in vertical ruler (L, R, C, LFE...) | Shows which lane is which channel | missing |  |  |  | S | A071 |  |
| 115 | A079 | T2 | Data under cursor readout | Status bar shows frequency, time, channel and amplitude under pointer | missing |  |  |  | S | A329 |  |
| 116 | A082 | T2 | High-DPI rendering (merged: C306) | Crisp UI on hi-dpi screens | partial |  | SoundCraft gap G-19 | soundcraft ui-egui patterns (MIT/Apache) | S |  |  |
| 117 | A084 | T2 | Horizontal scroll (wheel/middle drag) | Scroll timeline sideways | partial |  |  |  | S |  |  |
| 118 | A089 | T2 | Zoom in at In point / Out point | Zoom around selection start or end | missing |  |  |  | S |  |  |
| 119 | A091 | T2 | Right-drag in ruler/navigator to zoom to range | Magnifier drag fills the view with that time range | missing |  |  |  | S |  |  |
| 120 | A092 | T2 | Zoom to Time presets (5 slots) | Save/recall zoom durations from the Zoom to Time menu | missing |  |  |  | S | A088 |  |
| 121 | A093 | T2 | Zoom factor preference | General pref sets how far each zoom step goes | missing |  |  |  | S | A273 |  |
| 122 | A095 | T2 | Synchronise selection, zoom and playhead across files | Pref keeps view state while switching between open files | missing |  |  |  | S | A224 |  |
| 123 | A096 | T2 | Frequency-axis scroll by dragging ruler | See ed_spec_vscroll | missing |  |  |  | S | A069 |  |
| 124 | A100 | T2 | Samples format | Position as sample count | missing |  | soundcraft-time supports Samples | soundcraft-time (MIT/Apache) | S | A099 |  |
| 125 | A102 | T2 | Custom frame rate format (2-1000 fps) | User-defined frames per second | missing |  |  | soundcraft-time (MIT/Apache) | S | A099 |  |
| 126 | A103 | T2 | Compact Disc 75 fps format | CD frames format (older versions; CD authoring removed in 26.3) | missing |  |  | soundcraft-time (MIT/Apache) | S | A099 |  |
| 127 | A104 | T2 | Jump to time (typed entry) | Type a time to move the playhead | missing |  |  | soundcraft-time (MIT/Apache) | S | A033, A099 |  |
| 128 | A105 | T2 | Drag across digits to scrub time | Dragging over the numbers changes the playhead | missing |  |  |  | S | A033 |  |
| 129 | A107 | T2 | Ruler follows time format; subdivision (fine) ticks | Ruler ticks show chosen format and fine subdivisions | partial |  |  | soundcraft-time (MIT/Apache) | S | A099 |  |
| 130 | A133 | T2 | Channel buttons in amplitude ruler | Per-channel toggles beside the amplitude ruler choose which channels edits affect | missing |  |  |  | S | A071 | #21 |
| 131 | A146 | T2 | Insert Silence length in current time format | Dialog accepts time-format entry | partial |  |  | soundcraft-time (MIT/Apache) | S | A099 |  |
| 132 | A165 | T2 | Open Convert dialog by double-clicking status bar sample type | Double-click the status-bar sample type to open the convert dialog | missing | CP |  |  | S | A161, A329 |  |
| 133 | A226 | T2 | File > Import > File | Add to Files panel without replacing editor | missing |  |  |  | S | A020 |  |
| 134 | A235 | T2 | Open Recent | Recent files menu | missing |  | soundcraft issue #421 relates (soundcraft is DAW) | soundcraft ui-egui patterns (MIT/Apache) | S |  |  |
| 135 | A236 | T2 | Reveal in Explorer/Finder | Show file location (from memory) | missing |  |  |  | S |  |  |
| 136 | A278 | T2 | Auto Save | Interval, location | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | S | A239 | #47 |
| 137 | A280 | T2 | Data (crossfade durations, sample rate conversion quality) | Edit crossfade length, resample settings | partial |  |  |  | S | A273 |  |
| 138 | A281 | T2 | Media & Disk Cache (temp folders, peak files, DLMS) (merged: C019) | Primary and secondary temp folder, peak files | partial |  |  |  | S | A273 |  |
| 139 | A282 | T2 | Markers & Metadata | Embed edit-original data, marker options | missing |  |  |  | S | A273 |  |
| 140 | A284 | T2 | Playback and Recording (scroll type, JKL speed, auto-scroll) (merged: C017) | Scroll mode, shuttle speed | missing |  | SoundCraft model Session.edit.pre_roll |  | S | A273 |  |
| 141 | A285 | T2 | Spectral Displays (window, resolution, dB range, play only selected frequencies) | Spectral tuning | partial |  |  |  | S | A273 |  |
| 142 | A286 | T2 | Time Display | Default format and custom frame rate | missing |  |  |  | S | A099, A273 |  |
| 143 | A288 | T2 | Loudness / Diagnostics prefs | Not in fetched pages | missing |  |  |  | S | A273 |  |
| 144 | A290 | T2 | Restore preferences defaults | Reset all preferences to their defaults | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | S | A273 |  |
| 145 | A294 | T2 | Conflict detection when assigning | Prompt to move shortcut | missing |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache) | S | A291 |  |
| 146 | A327 | T2 | Status bar (operation time, transport status) | Left: time to open/save/process, Playing/Stopped | partial |  |  |  | S |  |  |
| 147 | A328 | T2 | Right-side info: sample type, uncompressed size, duration, free space, free time | Choose which status items the right side of the bar shows | partial |  |  |  | S | A329 |  |
| 148 | A350 | T2 | Tooltips on tools with shortcuts | Hover text | partial |  |  |  | S |  |  |
| 149 | A005 | T2 | Resize panel groups by dividers | Dragging a divider resizes all groups sharing it; four-way at intersections | partial |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | M | A008 | #16, #45 |
| 150 | A011 | T2 | Built-in workspaces (merged: C096) | Predefined layouts (Default, Editing, Audio Post, Mixing-type) chosen from the Workspace menu and toolbar | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache); photocraft EditHistory, filmcraft workspaces (MIT/Apache) | M | A009, A273 | #44, #45 |
| 151 | A013 | T2 | Multiple monitors in workspace | Monitor placement of floating windows is stored in the workspace | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); photocraft EditHistory, filmcraft workspaces (MIT/Apache) | M | A004 |  |
| 152 | A014 | T2 | Toolbar docked under menu | Tools plus Workspace menu plus Waveform/Multitrack switch in a bar; can be undocked as Tools panel | missing |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | M | A057 |  |
| 153 | A016 | T2 | Appearance: colours, brightness, gradients, presets (merged: C307) | Prefs change waveform, selection and playhead colours, UI brightness, gradient on/off, saved colour presets | missing |  | SoundCraft issue #82 (Audition-like theme) | soundcraft ui-egui patterns (MIT/Apache) | M | A273 |  |
| 154 | A017 | T2 | Export / import application settings | Bundle preferences, effect settings and layouts into one file and load them elsewhere | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | M | A009, A273 |  |
| 155 | A023 | T2 | Files panel channel expander | Expand a multichannel file to show its channels; enable a single channel from the list | missing |  | iXML channel names need audio-io iXML (SoundCraft PR #644) | bwavfile (MIT) | M | A020, A134 |  |
| 156 | A027 | T2 | Properties panel | Shows properties and settings of the selected file or clip (also tempo/time signature edits) | missing |  |  |  | M |  |  |
| 157 | A035 | T2 | Tools panel | Tool buttons (time selection, hand, zoom, marquee, lasso, paintbrush, spot healing) with tool options | missing |  |  |  | M | A057 |  |
| 158 | A075 | T2 | Peak files for fast reopen | Optional peak files so big WAVs reopen fast (Media & Disk Cache pref) | partial |  |  | soundcraft-audio-io (MIT/Apache) | M |  |  |
| 159 | A274 | T2 | General (zoom factor, extend selection, context-sensitive channel editing, circular selection, default fade, HUD, sync files) | Editing behaviour options | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | M | A273 |  |
| 160 | A275 | T2 | Appearance | Colours, brightness | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | M | A273 |  |
| 161 | A289 | T2 | UI language (merged: C308) | Localised interface | missing |  | SoundCraft PR #783 ships a catalog for the DAW; SoundCraft PRs #783,#741 (zh) |  | M | A273 |  |
| 162 | A293 | T2 | Shortcut sets (default, ProTools read-only, custom) | Save As, Delete, Restore Default | missing |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache) | M | A291 |  |
| 163 | A351 | T2 | Right-click context menus on editor, ruler, panels | Menus for snapping, scale, etc. | missing |  |  |  | M |  |  |
| 164 | A001 | T2 | Default workspace layout | Editor in the middle, Files/Effects/Markers-type panels at the sides, transport and meters below | partial |  |  | soundcraft ui-egui patterns (MIT/Apache); craft-ui (MIT/Apache) | L |  | #44 |
| 165 | A029 | T2 | Editor panel | Waveform plus spectral display with ruler, channel buttons, zoom buttons, time display, transport buttons | partial |  |  |  | L |  |  |
| 166 | A292 | T2 | Visual keyboard layout editor | Shows keyboard with assigned keys; drag command onto key | missing |  |  | filmcraft/lightcraft shortcut editors (MIT/Apache) | L | A291 |  |

## T3 Playback (16)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 167 | A195 | T3 | Loop playback | Button repeats cursor-to-end or selection | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | S |  | #24 |
| 168 | A194 | T3 | Pause | Pause and resume at same point | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | S |  | #24 |
| 169 | A196 | T3 | Return playhead to start on stop (Shift+X) | Toggle behaviour | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | S |  | #24 |
| 170 | A034 | T3 | Transport panel | Stop, Play, Pause, Loop, Skip back/forward, Rewind, Fast forward, Record buttons | partial |  |  |  | M | A195 | #24 |
| 171 | A080 | T3 | Playhead / current-time indicator | Draggable line shown at play position or cursor | partial |  |  |  | S |  | #25 |
| 172 | A201 | T3 | Scrub by dragging playhead | Audible preview while dragging | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | M | A080 | #25, #69 |
| 173 | A202 | T3 | Preroll / postroll (merged: C049) | Play a bit before/after selection for checking edits (not in fetched pages; from memory) | missing |  | SoundCraft session.rs pre_roll (default 2 s) | soundcraft-model/mix/playback (MIT/Apache) | S |  |  |
| 174 | A198 | T3 | Rewind / Fast Forward shuttle | Step through playback rates for rewind and fast forward | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | S |  | #24 |
| 175 | A199 | T3 | JKL shuttle with stepped speed | Backward, stop and forward keys; repeating raises the speed | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | M |  | #24 |
| 176 | A205 | T3 | Sample-rate conversion to device (merged: C007) | File rate to device rate on output | partial |  | SoundCraft devices.rs offers rates; SoundCraft #643 polyphase resampler (PR) | soundcraft-model/mix/playback (MIT/Apache); rubato (MIT/Apache) | S |  |  |
| 177 | A208 | T3 | Edit while playing / playback follows edits | Player reads updated doc after edit | partial |  |  | soundcraft-model/mix/playback (MIT/Apache) | M |  | #64 |
| 178 | A094 | T3 | Auto-scroll during playback (pagewise / centered) | Choose between page-wise scrolling and a centred playhead; manual scrolling pauses it | partial |  |  |  | S |  |  |
| 179 | A197 | T3 | Go to previous/next marker or start/end buttons | Skip buttons | partial |  |  | soundcraft-model/mix/playback (MIT/Apache) | S | A176 | #24 |
| 180 | A212 | T3 | Playhead sync across files | See zoom_sync_files | missing |  |  |  | S | A224 |  |
| 181 | A200 | T3 | Variable playback speed display | Speeds 0.1x, 0.5x, 1x, 2x and finer steps with speed readout | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | M | A199 |  |
| 182 | A203 | T3 | Skip selection playback | Play audio but skip the selected part (from memory; not in fetched pages) | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | M |  |  |

## T4 Recording (29)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 183 | A276 | T4 | Audio Hardware (merged: C001) | Device class, input, output, clock, buffer size, sample rate, ASIO settings, release ASIO | missing |  | SoundCraft playback/devices.rs AudioConfig+audio_systems (reusable) |  | L | A273 | #55 |
| 184 | C004 | T4 | Choose input device | Pick any input device for recording | missing |  | SoundCraft devices.rs; SoundCraft issues #42,#336,#402 (selection bugs) |  | S | A276 | #55 |
| 185 | C003 | T4 | Choose output device | Pick any output device and have playback reopen on it | missing |  | SoundCraft playback/devices.rs |  | S | A276 | #55 |
| 186 | C020 | T4 | Record into a new waveform file (merged: A342) | Record from input to a new file; stop to open it | missing | CP | SoundCraft playback/record.rs Recorder (reusable) |  | M | A276, C004 | #51, #57 |
| 187 | C022 | T4 | Crash-safe take writing | Header rewritten as it goes so an interrupted take survives | missing | CP | SoundCraft record.rs rewrites header each second (reusable) | atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | M | C020 | #53 |
| 188 | C021 | T4 | New-file recording dialog (rate, channels, bit depth) | Pick sample rate, channel layout and sample type before recording | missing | CP | SoundCraft Recorder writes WAV take at input rate |  | S | C020 | #51 |
| 189 | A213 | T4 | Peak meters L/R in dBFS (merged: A031, B129, C053, C058) | Meter with 1.5 s peak hold, clip lights | missing |  | soundcraft-dsp PeakMeter available; dsp::meter exists in soundcraft-dsp (PeakMeter, RmsMeter), not yet a dependency (code-map.md); SoundCraft PeakMeter/RmsMeter... | filmcraft meters + fx editors (MIT/Apache) | M | B002 | #18 |
| 190 | C023 | T4 | Recording status readout | Elapsed time, file size, disk time remaining, dropouts shown | missing |  | SoundCraft Recorder::lost_ms |  | S | C020 | #54 |
| 191 | C024 | T4 | No playback while recording / mute output | Stay silent during record, unless monitoring | missing |  |  |  | S | C020 | #52 |
| 192 | C035 | T4 | Record-ready dropped-sample warning (merged: A330) | Report lost frames after take | missing | CP | SoundCraft Recorder::lost_ms/Trouble |  | S | C023 | #54 |
| 193 | C029 | T4 | Pause recording | Pause/resume a take without ending it | missing | CP |  |  | S | C020 |  |
| 194 | C028 | T4 | Timed recording | Start/stop recording at a set time or duration | missing | CP |  |  | S | C020 |  |
| 195 | A216 | T4 | Meter input signal toggle (merged: C059) | Monitors recording input | missing |  | SoundCraft Recorder live_take | filmcraft meters + fx editors (MIT/Apache) | S | A213, C004 | #54 |
| 196 | A211 | T4 | Audio Channel Mapping (merged: A277, C008) | Map file channels to output ports | missing |  | SoundCraft #575 I/O Setup PR (Pro Tools style), G-01 gap |  | M | A273, A276 |  |
| 197 | C026 | T4 | Record into an existing file at cursor/selection | Overwrite or insert recording at cursor or over the selection | missing | CP |  |  | M | C020 | #72 |
| 198 | A019 | T4 | Machine-specific hardware defaults | Hardware prefs kept per machine for roaming profiles | missing |  |  | craft-settings, craft-atomic (MIT/Apache) | S | A276 |  |
| 199 | A206 | T4 | Follow default output device change (merged: C002, C009) | Reopen stream when OS default changes | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | S |  | #55, #67 |
| 200 | C013 | T4 | Hardware test / mic check (merged: A341) | Wizard that tests input level and output before work | missing |  |  |  | S | A213, C004 | #54 |
| 201 | C030 | T4 | Record target channel selection | Choose which input channels to record (mono left, right or a stereo pair) | missing | CP | SoundCraft #336; SoundCraft Recorder takes channel list per track |  | S | C021 |  |
| 202 | C031 | T4 | Start recording immediately with defaults | Hit record once and capture straight away | missing | CP | SoundCraft issue #737 shows the pitfall |  | S | C020 |  |
| 203 | C032 | T4 | Recovered take after crash | Reopen an interrupted take on next launch | missing | CP |  | filmcraft crash hook, photocraft crash_guard, pdfcraft updates (MIT/Apache); atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | S | A239, C022 | #53 |
| 204 | C033 | T4 | Test input levels before record (arm monitor meter) | Meter live while idle so level can be set | missing |  |  |  | S | A213, C004 |  |
| 205 | C010 | T4 | Latency compensation / record offset (merged: C050) | Measure or enter device latency and shift recorded audio to compensate | missing | CP | SoundCraft gap G-01 lists record-offset compensation missing; SoundCraft gap G-01 |  | M | A276 |  |
| 206 | C025 | T4 | Input monitoring (software) | Hear the input through the output while recording, with on/off | missing |  | SoundCraft PR #808, issues #42,#688 (input monitoring) |  | M | C020 | #72 |
| 207 | C027 | T4 | Punch-in recording (Waveform) | Record only inside a selection while existing audio plays around it | missing | CP |  |  | M | C026 | #72 |
| 208 | C034 | T4 | Record from several mics in Waveform | Multi-input to one multichannel file | missing |  |  |  | M | C020 | #72 |
| 209 | A210 | T4 | ASIO / CoreAudio / WASAPI choice and buffer size (merged: C005, C012, C006) | Device class, I/O buffer, latency | missing |  | SoundCraft #681 ASIO, #213, #196 JACK open; SoundCraft audio_systems() lists cpal hosts; SoundCraft #681 open; SoundCraft asio feature opt-in; SoundCraft devices.rs... | soundcraft-model/mix/playback (MIT/Apache) | L | A276 | #55 |
| 210 | C011 | T4 | Duplex low-latency stream | Input and output on one clock for monitoring and overdub | missing | CP | SoundCraft gap G-01 (separate cpal streams); SoundCraft #769,#808 |  | L | A276 |  |
| 211 | C015 | T4 | Sample-rate/clock sync (word clock/external) | External clock selection is by driver only [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-06.md] | missing |  | SoundCraft gap G-11 external sync missing |  | L | A276 |  |

## T5 Effects rack and core effects (56)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 212 | B004 | T5 | Real-time effect Preview (looped) (merged: A340) | Audition the effect on the selection through the player, hear setting changes live | missing |  |  |  | L | B001, B002 | #32 |
| 213 | B005 | T5 | Real-time insert path in the player | Audio callback runs a chain of effects on the fly with parameter smoothing and tail handling | missing |  | SoundCraft dsp::Plugin trait is realtime-ready |  | L | B002 |  |
| 214 | B013 | T5 | Effects Rack panel (Waveform editor) (merged: A025) | Panel with up to 16 ordered slots acting on the open file | missing |  |  |  | L | B001, B004, B005 | #33, #58 |
| 215 | B007 | T5 | Effect presets (save/load/delete per effect) (merged: C173) | Named parameter sets per effect, stored as portable files | missing |  |  |  | M | B001 | #35 |
| 216 | B018 | T5 | Apply Effects Rack to selection/file (one undo step) | Render the chain into audio, with tail option | missing |  |  |  | M | B003, B013 | #33 |
| 217 | A336 | T5 | Fade In / Fade Out commands | Apply a fade in or fade out to the selection from the menu | missing |  |  | soundcraft-dsp (MIT/Apache) | S |  | #27 |
| 218 | B117 | T5 | DC Offset removal | Remove constant bias | missing |  | SoundCraft dsp::offline::remove_dc, dc_offset_removal plugin reusable | soundcraft-dsp (MIT/Apache) | S | B001 | #29 |
| 219 | B057 | T5 | Parametric EQ | Multi-band EQ with low/high shelf, peaks and curve view | missing |  | SoundCraft eq_7band + eq_1band reusable | soundcraft-dsp (MIT/Apache); filmcraft-audio-dsp (MIT/Apache) | M | B001, B002 | #34 |
| 220 | B060 | T5 | High-pass / low-pass quick filters | Simple cut filters as shortcut effects | missing |  | SoundCraft FilterType::HighPass/LowPass | soundcraft-dsp (MIT/Apache) | S | B057 | #34 |
| 221 | B037 | T5 | Single-band Compressor | Compressor with threshold, ratio, attack, release, makeup, lookahead | missing |  | SoundCraft dsp compressor plugin (10 ms lookahead) reusable | soundcraft-dsp (MIT/Apache) | M | B001, B002 | #34 |
| 222 | B034 | T5 | Hard Limiter | Brickwall peak limiter with lookahead, release, input boost, max amplitude | missing |  | SoundCraft dsp maximizer (1.5 ms lookahead, sample peak) reusable | soundcraft-dsp (MIT/Apache); filmcraft-audio-dsp (MIT/Apache) | M | B001, B002 | #34 |
| 223 | B028 | T5 | DeEsser | Cut sibilance by band-detected gain reduction with center frequency, threshold, bandwidth | missing |  | SoundCraft dsp de_esser plugin reusable | soundcraft-dsp (MIT/Apache) | M | B001, B002 | #34 |
| 224 | B125 | T5 | Loudness measurement (LUFS/true peak) (merged: A318, C064, C065) | Integrated, short-term, momentary and true peak | missing |  | SoundCraft dsp::meter LoudnessMeter reusable (reviewed within 0.02 LU); dsp::meter upstream; SoundCraft dsp/meter.rs LoudnessMeter (reusable); SoundCraft... | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | M | B002 | #39 |
| 225 | B011 | T5 | Shared STFT / overlap-add engine | One FFT analysis-synthesis framework for FFT Filter, NR, Sound Remover, spectral tools | partial |  | SoundCraft dsp::spectrum is analysis only |  | M |  |  |
| 226 | A337 | T5 | Capture noise print (Shift+P) (merged: B066) | #36 (G1), area C | missing |  |  |  | M | B011 | #36 |
| 227 | B067 | T5 | Noise Reduction (process) | Subtract noise-print from audio with amount, reduce-by, FFT size, Output Noise Only, advanced decay/smoothing/precision/transition | missing | CP |  | filmcraft-audio-dsp (MIT/Apache) | L | A337, B003, B011 | #37 |
| 228 | B126 | T5 | Match Loudness panel (merged: A042) | Batch-style matching of files to ITU/EBU/ATSC targets with limiter | missing | CP | SoundCraft LoudnessMeter + maximizer reusable | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | L | B034, B125 | #38 |
| 229 | B127 | T5 | Match Loudness: targets (ITU-R BS.1770-3, EBU R128, True Peak, ATSC A/85, Total RMS, Peak) | Selectable standards | missing | CP |  | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | M | B126 | #38 |
| 230 | B128 | T5 | Match Loudness: scan, results list, Run | Per-file measure then process with batch | missing | CP |  | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | M | B126 | #13, #38 |
| 231 | C247 | T5 | Podcast loudness presets | One-action loudness targets for common podcast and streaming platforms with pass/fail | missing |  |  | filmcraft-audio-dsp (MIT/Apache) | M | B125 | #38, #72 |
| 232 | B008 | T5 | Favorites menu / user-recorded actions (merged: A167, C232) | Menu of saved effect settings and recorded command sequences, with Start/Stop Recording | missing |  |  |  | M | B007 | #31, #35, #72 |
| 233 | A334 | T5 | Amplify / gain (merged: B025) | #28 (E2) | missing |  | SoundCraft dsp::offline::gain reusable | soundcraft-dsp (MIT/Apache) | S | B001 | #28 |
| 234 | B027 | T5 | Channel Mixer (merged: A138) | Matrix of input->output channel levels with invert and swap | missing |  |  | filmcraft-audio-dsp (MIT/Apache) | M | B001 |  |
| 235 | B029 | T5 | Dynamics (compressor, limiter, expander, gate in one with curve) | Multi-section dynamics with editable transfer curve and attack/release/lookahead | missing |  | SoundCraft compressor+expander_gate reusable but no combined curve editor | filmcraft-audio-dsp (MIT/Apache) | L | B001, B002 |  |
| 236 | B056 | T5 | Notch Filter | Narrow band-reject filters, multiple notches with Q and gain | missing |  | SoundCraft eq FilterType::Notch reusable | filmcraft-audio-dsp (MIT/Apache) | S | B002 |  |
| 237 | A077 | T5 | Fade in/out handles with linear, log, cosine curves (merged: B033) | Drag a corner handle to fade; the drag shape sets the curve; applied to the audio directly | missing |  | dsp::offline::fade upstream has shapes; SoundCraft dsp::offline::fade (FadeShape) reusable |  | M | B001 | #27 |
| 238 | B031 | T5 | Fade Envelope | Draw a volume envelope over the selection with keyframes | missing |  | SoundCraft offline::fade reusable |  | M | B001 | #27 |
| 239 | B058 | T5 | Parametric EQ graph / interactive display | Draggable band nodes with analyzer overlay | missing |  |  | filmcraft-audio-dsp (MIT/Apache); filmcraft meters + fx editors (MIT/Apache); egui_plot, egui_table, egui_knob, egui-bind (MIT/Apache; MIT) | M | B057 |  |
| 240 | B068 | T5 | NR graph with threshold curve | Edit per-band thresholds and see the noise print | missing | CP |  |  | M | B067 | #37 |
| 241 | B035 | T5 | Multiband Compressor | Several crossover bands each with compressor, solo, crossover editing, output limiter | missing |  | SoundCraft compressor reusable per band, no crossover/multiband plugin | filmcraft-audio-dsp (MIT/Apache) | L | B001, B002 |  |
| 242 | B038 | T5 | Speech Volume Leveler | Automatic leveling of speech volume with target, leveling amount, gate, noise floor | missing |  |  | filmcraft-audio-dsp (MIT/Apache) | L | B001, B125 |  |
| 243 | A078 | T5 | Default fade type pref and HUD pref | General pref chooses default fade curve and HUD behaviour | missing |  |  |  | S | A077, A273 |  |
| 244 | A332 | T5 | Invert (merged: B113) | Flip polarity of selection | missing |  | dsp::offline::invert upstream; SoundCraft dsp::offline::invert reusable | soundcraft-dsp (MIT/Apache) | S | B001 | #30, #58 |
| 245 | A333 | T5 | Reverse (merged: B114) | Reverse selection | missing |  | dsp::offline::reverse upstream; SoundCraft dsp::offline::reverse reusable | soundcraft-dsp (MIT/Apache) | S | B001 | #30 |
| 246 | A335 | T5 | Normalize (merged: B036) | #29 (E3) | missing |  | subnormal-peak fix SoundCraft PR #820; SoundCraft dsp::offline::normalize+remove_dc reusable | soundcraft-dsp (MIT/Apache) | S | B001 | #29 |
| 247 | B009 | T5 | Default Favorites (Fade In/Out, Normalize to 0 dB, Dynamics presets, etc.) | Ship a starter set of one-click saved effect settings [Audition check 2026-10-28, checked in Adobe Audition 26.5: differs; Normalize presets are -0.1 dB and -3 dB, no 0 dB preset and no Dynamics presets; see logs/2026-10-23/feature-verification/batch-03.md] | missing |  |  |  | S | B008 | #27 |
| 248 | B014 | T5 | Rack slot power / bypass | Per-slot enable; bypassed costs nothing | missing |  |  |  | S | B013 | #33 |
| 249 | B015 | T5 | Slot reorder / drag, replace, remove | Rearrange and swap effects in the chain | missing |  |  |  | S | B013 | #33 |
| 250 | B016 | T5 | Rack input and output gain | Gain before and after the chain | missing |  |  |  | S | B013 | #33 |
| 251 | B017 | T5 | Rack Mix (dry/wet) and per-effect wet | Blend processed with dry signal [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-03.md] | missing |  |  |  | S | B013 | #33 |
| 252 | B019 | T5 | Process-with-tail / Apply to extend length | Let reverb/delay tails extend past the selection [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-03.md] | missing |  | SoundCraft dsp::offline::apply_plugin_with_tail (reusable) |  | S | B018 |  |
| 253 | B021 | T5 | Rack presets (whole-chain) | Save/load chain with all settings | missing |  |  |  | S | B007, B013 | #35 |
| 254 | B022 | T5 | Master bypass of rack / A-B compare | Toggle all effects off to compare | missing |  |  |  | S | B013 |  |
| 255 | B023 | T5 | Rack Favorites / Rack to Favorite | Save a chain as a favorite [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-04.md] | missing |  |  |  | S | B008, B013 |  |
| 256 | B024 | T5 | Re-edit effect in slot from dialog | Open a slot's dialog from the rack | missing |  |  |  | S | B006, B013 | #33 |
| 257 | B026 | T5 | Amplify: clip-guard / Calculate Now | Show headroom-safe gain and avoid clipping [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-04.md] | missing |  |  |  | S | A334 | #28 |
| 258 | B041 | T5 | Match Volume (gain, selection to ref) (merged: C144) | Fit levels between clips [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-04.md] | missing |  | SoundCraft LoudnessMeter |  | S | B125 | #72 |
| 259 | B080 | T5 | Output noise only (monitor residue) | Listen to the removed noise to judge settings | missing | CP |  |  | S | B067 | #37 |
| 260 | B165 | T5 | Time Shift (Insert delay) | Shift audio later by a number of milliseconds | missing |  | SoundCraft time_shift plugin reusable | soundcraft-dsp (MIT/Apache) | S | B001 |  |
| 261 | B169 | T5 | A/B compare effect settings | Switch between two settings [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-06.md] | missing |  |  | photocraft EditHistory, filmcraft workspaces (MIT/Apache) | S | B006 |  |
| 262 | A076 | T5 | Amplitude HUD (heads-up gain knob) | Floating gain control: drag to change level of selection or whole file; pin; hide; show only for selections | missing |  |  |  | M |  | #28 |
| 263 | B030 | T5 | Dynamics graph editor (transfer curve) | Draggable curve defining level mapping | missing |  |  | filmcraft meters + fx editors (MIT/Apache); egui_plot, egui_table, egui_knob, egui-bind (MIT/Apache; MIT) | M | B029 |  |
| 264 | B032 | T5 | Gain Envelope | Time-varying gain with spline/linear handling over selection | missing |  |  |  | M | B031 |  |
| 265 | B055 | T5 | Graphic EQ (10, 20, 30 band) | Fixed band sliders with accuracy and range, plus presets | missing |  | SoundCraft eq_7band parametric only | filmcraft-audio-dsp (MIT/Apache) | M | B001, B002 |  |
| 266 | B069 | T5 | DeNoise (real-time) | Single-knob real-time noise reducer | missing | CP |  | nnnoiseless (BSD-3-Clause) | L | B005, B067 |  |
| 267 | B168 | T5 | Preview of destructive processes (NR) using partial render | Render small part for preview | missing | CP |  |  | L | B004, B067 | #32, #37 |

## T6 Metering and analysis (18)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 268 | A313 | T6 | Amplitude statistics (peak, RMS, DC, clipped, bit depth, dynamic range, loudness) (merged: A038, B053, B131, C070, C071) | See p_ampstats | missing |  | SoundCraft dsp meter reusable | soundcraft-dsp (MIT/Apache) | L | B125 | #39 |
| 269 | A316 | T6 | Navigate to clipped sample / peak | Icons jump to locations | missing |  |  |  | S | A313 |  |
| 270 | B130 | T6 | Loudness range (LRA) (merged: C067) | Statistical spread of loudness | missing |  | SoundCraft meter does not list LRA; SoundCraft LoudnessMeter lacks LRA (verify) | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | M | B125 | #38, #39 |
| 271 | A309 | T6 | Frequency analysis (FFT graph) (merged: A037, B052, C068) | See p_freq; lines/area/bars, scale, FFT size, window, 0 dB reference, overall note readout | missing |  | SoundCraft dsp::spectrum::magnitude_db; SoundCraft dsp/spectrum.rs | realfft, rustfft (MIT; MIT/Apache) | L | B011 | #72 |
| 272 | A317 | T6 | Phase meter (merged: A039, C062) | See p_phase_meter | missing |  | no SoundCraft meter | soundcraft-dsp (MIT/Apache) | M | A213 | #72 |
| 273 | A041 | T6 | Loudness Radar panel (merged: B091, C066) | Radar-style loudness meter (momentary, short-term, integrated, LRA) [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-01.md] | missing |  | dsp::meter LoudnessMeter available upstream (code-map.md); SoundCraft dsp::meter LoudnessMeter reusable; none in SC | ebur128 (MIT; test oracle) | L | B125 | #39 |
| 274 | A214 | T6 | Dynamic/static peaks, valleys, LED, gradient, ranges (merged: C054, C057, C072) | Meter display options: peak mode, ranges and colour | missing |  | SoundCraft PeakMeter::peak_hold_db/set_hold_secs; SoundCraft PR #720 | filmcraft meters + fx editors (MIT/Apache) | S | A213 | #18 |
| 275 | A215 | T6 | Reset clip indicators (merged: C056, C168) | Clear the held clip lights on the meters | missing |  | SoundCraft PeakMeter::clipped/reset_clip; SoundCraft PeakMeter::reset_clip | filmcraft meters + fx editors (MIT/Apache) | S | A213 | #18 |
| 276 | A217 | T6 | RMS display (merged: C055) | Peak and RMS (#18 asks for both) [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-02.md] | missing |  | soundcraft-dsp RmsMeter; SoundCraft RmsMeter | filmcraft meters + fx editors (MIT/Apache) | S | A213 | #18 |
| 277 | A218 | T6 | Dock horizontally or vertically | Panel layout changes with dock shape | missing |  |  | craft-ui (MIT/Apache) | S | A002, A213 | #18 |
| 278 | A310 | T6 | Frequency hold snapshots (8) | Frozen overlays | missing |  |  |  | S | A309 |  |
| 279 | A311 | T6 | Scan selection | Average spectrum over a range | missing |  |  |  | S | A309 |  |
| 280 | A312 | T6 | Copy graph data | Text report to clipboard | missing |  |  |  | S | A309 |  |
| 281 | A315 | T6 | RMS settings (sine/square reference, account for DC, window width) | Calibration options | missing |  |  |  | S | A313 |  |
| 282 | A319 | T6 | Play only selected frequencies toggle | See tr_spectral_only | missing |  |  |  | S |  |  |
| 283 | A040 | T6 | Phase Analysis panel (merged: B051, C063) | Phase relationship display between channels (older-version panel; not in fetched pages) [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-01.md] | missing |  |  |  | M | A317 | #72 |
| 284 | A314 | T6 | RMS histogram | Graph of prevalence of levels | missing |  |  |  | M | A313 |  |
| 285 | C069 | T6 | Spectral/spectrogram meter during record | Live spectrogram while recording | missing |  |  |  | M | A309 |  |

## T7 Spectral tools and repair (29)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 286 | B047 | T7 | Delete Silence (merged: A158) | Find sections below threshold for a minimum time and cut/ mark them | missing |  | SoundCraft offline::non_silent_ranges + streaming scanner reusable | soundcraft-dsp (MIT/Apache) | M | B002 |  |
| 287 | B048 | T7 | Mark Audio (Mark Audio Pieces) (merged: A186) | Auto-place cue/range markers around speech vs silence | missing |  | SoundCraft offline::non_silent_ranges |  | M | A170 | #40, #43 |
| 288 | B046 | T7 | DeClicker (Diagnostics) | Detect and fix clicks/pops with threshold, complexity, Find All Levels | missing |  |  | filmcraft-audio-dsp (MIT/Apache); cathar (MIT/Apache; reference only) | L | B001 |  |
| 289 | B045 | T7 | DeClipper (Diagnostics) | Find and repair clipped peaks by interpolation, with scan + fix | missing |  |  | filmcraft-audio-dsp (MIT/Apache); cathar (MIT/Apache; reference only) | L | B001 |  |
| 290 | B050 | T7 | Diagnostics panel (scan list, Scan, Repair All, Repair) (merged: A046, C073) | Result list for DeClick/DeClip with Find/Fix | missing |  |  |  | M | B045, B046 | #72 |
| 291 | B073 | T7 | DeHummer | Notch bank at hum frequency (50/60 Hz) and harmonics, Q and gain | missing |  | SoundCraft eq notch building block | filmcraft-audio-dsp (MIT/Apache); cathar (MIT/Apache; reference only) | M | B056 |  |
| 292 | B074 | T7 | DeReverb | Reduce room reverb with amount and spectral controls | missing |  |  | filmcraft-audio-dsp (MIT/Apache); cathar (MIT/Apache; reference only) | L | B005, B011 |  |
| 293 | B071 | T7 | Hiss Reduction | Noise print + reduce above threshold across band with adaptive options | missing |  |  |  | L | A337, B011 |  |
| 294 | B072 | T7 | Automatic Click Remover (merged: B078) | Detect/remove clicks over the whole waveform with threshold/complexity | missing |  |  | filmcraft-audio-dsp (MIT/Apache); cathar (MIT/Apache; reference only) | M | B046 |  |
| 295 | B135 | T7 | Spectral selection (frequency + time) | Selection with a frequency band and time region | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | L |  | #23 |
| 296 | A052 | T7 | Marquee selection (merged: B136) | Rectangular time+frequency selection in the spectral display | missing |  |  |  | L | B135 | #23 |
| 297 | A053 | T7 | Lasso selection (merged: B137) | Free-form time+frequency selection | missing |  |  |  | L | A052, B135 | #23 |
| 298 | A055 | T7 | Spot Healing Brush (merged: B079, B139) | Brush over a click or pop in the spectral view to repair it automatically | missing |  |  |  | L | A052, B011, B135 |  |
| 299 | B077 | T7 | Auto Heal (spectral) (merged: A339, B143) | Selection fixes using surrounding spectrum automatically | missing |  |  | cathar (MIT/Apache; reference only) | L | B011, B135 |  |
| 300 | B075 | T7 | Sound Remover (merged: A047) | Learn an unwanted sound from selection and remove, with analysis modes/quality | missing |  |  | cathar (MIT/Apache; reference only) | L | A337, B011 |  |
| 301 | A067 | T7 | Spectral Pitch Display (older versions) (merged: B105, B145) | Alternate view showing pitch tracks; from memory of older Audition, not in fetched pages [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-01.md] | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | L |  |  |
| 302 | A115 | T7 | Move selection by dragging inside it | For spectral selections | missing |  |  |  | S | A052 |  |
| 303 | B141 | T7 | Spectral selection feather edges | Smooth selection edges to avoid clicks | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | S | B135 | #23 |
| 304 | A056 | T7 | Add/subtract spectral selection (Shift/Alt drag) | Add to or remove from a spectral selection with a modifier key | missing |  |  |  | M | A053 |  |
| 305 | B076 | T7 | Automatic Phase Correction | Fix time/phase drift between channels with auto/rate/size/global | missing |  |  | cathar (MIT/Apache; reference only) | M | B011 |  |
| 306 | B147 | T7 | Spectral Display fast refresh after edit | Picture updates within 0.5 s | missing |  |  |  | M |  | #16, #68 |
| 307 | A054 | T7 | Paintbrush selection (merged: B138) | Brush selection with size and opacity setting; intensity scales effect | missing |  |  |  | L | A052, B135 | #23 |
| 308 | A068 | T7 | Spectral Phase Display (older versions) | Alternate view showing inter-channel phase; from memory, not in fetched pages [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-01.md] | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | L |  |  |
| 309 | A204 | T7 | Play spectral selection only | Plays only chosen frequencies; toggleable via Play button menu | missing |  |  | soundcraft-model/mix/playback (MIT/Apache) | L | A052 | #23 |
| 310 | B054 | T7 | FFT Filter | Draw a response curve (log/linear, precision, windows) applied by FFT | missing |  |  | filmcraft-audio-dsp (MIT/Apache); realfft, rustfft (MIT; MIT/Apache) | L | B011 |  |
| 311 | B070 | T7 | Adaptive Noise Reduction | Tracks varying broadband noise without a print | missing |  |  |  | L | B005, B011 |  |
| 312 | B140 | T7 | Frequency band selection (limit effect to band) | Apply effects within a frequency range | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | L | B011, B135 | #23 |
| 313 | B142 | T7 | Effects acting only inside spectral selection | Amplify/gain/EQ on marquee | missing |  |  | realfft, rustfft (MIT; MIT/Apache) | L | A334, B135 | #23 |
| 314 | B144 | T7 | Spectral Repair / Frequency Repair | Repair pieces via neighbouring interpolation | missing |  |  | realfft, rustfft (MIT; MIT/Apache); cathar (MIT/Apache; reference only) | L | A055 |  |

## T8 Output, export, batch (55)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 315 | A272 | T8 | Preserve unknown chunks on save | Do not lose chunks when saving | missing | CP |  | riff (MIT) | M |  |  |
| 316 | A265 | T8 | BWF metadata (description, originator, time reference) | View/edit bext fields | partial | CP |  | soundcraft-audio-io (MIT/Apache); bwavfile (MIT) | M |  |  |
| 317 | A246 | T8 | Save as AIFF (merged: C255) | Uncompressed AIFF | missing | CP | audio-io encode supports AIFF (code-map.md); SoundCraft audio-io encode Aiff | soundcraft-audio-io (MIT/Apache) | S |  | #10 |
| 318 | A243 | T8 | Save As (Ctrl+Shift+S) | Save a copy under a new name and format; WavCraft then edits the new file | partial | CP |  | atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | M | A246 | #10 |
| 319 | A245 | T8 | Sample type on save (16/24/32int/32float) | Bit depth choice; keeps source depth | partial | CP |  | soundcraft-audio-io (MIT/Apache) | S |  | #12 |
| 320 | A253 | T8 | WAV options: 4GB+ via RF64 or multi-chunk, 32-bit float variants (merged: C254) | Large file support | partial | CP | audio-io writes RF64 over 4 GiB (code-map.md); SoundCraft audio-io encode Wav/RF64/BWF (reusable) | soundcraft-audio-io (MIT/Apache); bwavfile (MIT) | S |  | #12 |
| 321 | A247 | T8 | Save as FLAC (merged: C256) | Lossless; audio-io has built-in encoder | missing | CP | audio-io encode Flac (code-map.md); SoundCraft audio-io flac.rs encoder (reusable) | soundcraft-audio-io (MIT/Apache); flac-codec (MIT/Apache) | S |  | #10 |
| 322 | A257 | T8 | Save Selection As (merged: C267) | Write only selected range to new file | missing | CP |  |  | S | A243 |  |
| 323 | A260 | T8 | File > Export > File (keep current open) (merged: C269) | Export a copy | missing | CP | SoundCraft save_copy | atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | S | A243 |  |
| 324 | A262 | T8 | Include Markers and Other Metadata option (merged: C266) | Write markers/metadata on save | missing | CP | SoundCraft wav bext | bwavfile (MIT) | S | A183 | #14, #62 |
| 325 | A263 | T8 | Progress and cancel during save (merged: C272) | Percent in status bar | partial | CP | SoundCraft Bounce progress | craft-settings, craft-atomic (MIT/Apache) | S |  |  |
| 326 | A256 | T8 | Write BWF bext on save | Broadcast Wave metadata | partial | CP | audio-io EncodeOptions bwf flag (code-map.md) | soundcraft-audio-io (MIT/Apache); bwavfile (MIT) | S | A265 |  |
| 327 | B159 | T8 | Multi-file queue and progress (merged: C233) | Run on many files with cancel | missing | CP |  |  | M | B003, B158 | #72 |
| 328 | B158 | T8 | Batch Process panel (merged: A259, C231) | Run a favorite/effect over a list of files with output format and naming | missing | CP |  |  | L | A224, B008, B159 | #72 |
| 329 | A036 | T8 | Metadata panel (merged: C249) | BWF, ID3, RIFF and XMP tabs to view and edit file metadata, with schema display settings | missing |  | SoundCraft PR #644 iXML read; SoundCraft audio-io wav bext | lofty (MIT/Apache) | L | A265 | #14 |
| 330 | A268 | T8 | XMP metadata with schemas and custom schemas | Edit and search XMP; sidecar files where format cannot hold it | missing | CP |  | quick-xml, xmp_writer (MIT; MIT/Apache) | L | A036 |  |
| 331 | A345 | T8 | Windows, Linux, macOS builds (merged: C300) | Cross-platform [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-03.md] | partial |  | SoundCraft mac signing PR #793 | filmcraft crash hook, photocraft crash_guard, pdfcraft updates (MIT/Apache) | M |  | #48, #70 |
| 332 | A135 | T8 | Channel names (iXML, labels) | Show channel names from iXML metadata | missing |  | SoundCraft PR #644 (iXML read) in audio-io | bwavfile (MIT) | S | A265 |  |
| 333 | A254 | T8 | AIFF options: byte order, AIFC | Little vs big endian | missing | CP |  |  | S | A246 |  |
| 334 | A255 | T8 | Audio-only MOV | PCM in QuickTime | missing |  |  | filmcraft-aac + craft-media (MIT/Apache; patent check) | S |  |  |
| 335 | A258 | T8 | Save All (merged: C268) | Save every open file | missing | CP |  |  | S | A224 | #13 |
| 336 | A261 | T8 | Estimated file size in dialog | Updates as format changes | missing |  |  |  | S | A243 |  |
| 337 | A267 | T8 | RIFF INFO tags | Title, comments etc. | missing | CP |  | lofty (MIT/Apache) | S |  |  |
| 338 | A269 | T8 | Metadata Display sets (show/hide, save sets) | Customize shown properties | missing |  |  | quick-xml, xmp_writer (MIT; MIT/Apache) | S | A268 |  |
| 339 | A270 | T8 | Search within XMP | Filter properties by text | missing |  |  |  | S | A268 |  |
| 340 | A271 | T8 | AIFF author via Dublin Core Creator | Mapping | missing |  |  | quick-xml, xmp_writer (MIT; MIT/Apache) | S | A268 |  |
| 341 | A346 | T8 | Third-party notices file (merged: C313) | Licences for dependencies | missing |  |  | filmcraft crash hook, photocraft crash_guard, pdfcraft updates (MIT/Apache) | S |  | #49 |
| 342 | C273 | T8 | Export presets | Save export settings | missing |  |  | flac-codec (MIT/Apache) | S |  |  |
| 343 | C296 | T8 | WAV channel mask writing/reading | Speaker mask in WAV | missing |  | SoundCraft audio-io encode_with_channel_mask, wav_channel_mask | soundcraft-audio-io (MIT/Apache); bwavfile (MIT) | S | A253 |  |
| 344 | A137 | T8 | Extract Channels to Mono Files | Each channel written to its own mono file named with channel suffix and opened | missing | CP |  |  | M | A246 |  |
| 345 | A185 | T8 | Export audio of selected range markers to files (merged: C240, C248) | Batch save ranges with prefix, postfix start number, location, format | missing |  |  |  | M | A170, A246 |  |
| 346 | A227 | T8 | Import Raw Data | Specify rate (1 Hz-10 MHz), channels (1-32), encoding, byte order, start offset | missing |  |  |  | M |  |  |
| 347 | A249 | T8 | Save as MP2 | MPEG layer II | missing | CP |  |  | M |  |  |
| 348 | A250 | T8 | Save as OGG Vorbis (VBR quality) (merged: C259) | Vorbis export | missing | CP |  | vorbis_rs (BSD-3-Clause) | M |  | #72 |
| 349 | A251 | T8 | Save as Monkey's Audio (APE) | Lossless APE | missing | CP |  |  | M |  |  |
| 350 | A252 | T8 | Other libsndfile-type formats (AU, W64, CAF, VOC, etc.) | Many legacy containers | missing | CP |  |  | M |  |  |
| 351 | A248 | T8 | Save as MP3 (constant/variable bitrate) (merged: C257) | Encode MP3 | upstream-in-progress | CP | wait for upstream: SoundCraft PR #607 adds MP3 encoder in audio-io (open); SoundCraft PR #607, issue #648 |  | M |  | #11 |
| 352 | A266 | T8 | ID3 tags incl. album art for MP3 | Title, artist, cover image | missing | CP |  | lofty (MIT/Apache); id3 (MIT) | M | A248 |  |
| 353 | B160 | T8 | Batch: apply Favorites / Effects Rack (merged: C234) | Pick a rack or saved setting as the step to run on each file | missing | CP |  |  | M | B013, B158 | #72 |
| 354 | B161 | T8 | Batch: Match Loudness (merged: C236) | Normalise a folder to target | missing | CP |  | filmcraft-audio-dsp (MIT/Apache); ebur128 (MIT; test oracle) | M | B125, B126, B158 | #38, #72 |
| 355 | B162 | T8 | Batch: Noise Reduction with shared print | Process many files with one noise print | missing | CP |  |  | M | B067, B158 | #36, #37 |
| 356 | C235 | T8 | Output folder, format, name template | Pick format, sample type, rename pattern, folder | missing | CP |  |  | M | B158 | #72 |
| 357 | C237 | T8 | Batch sample-rate/bit-depth conversion | Convert many files | missing | CP |  | rubato (MIT/Apache) | M | A161, B158 | #72 |
| 358 | C239 | T8 | Command line batch | Headless run | missing |  | SoundCraft CLI convert exists; Audition check 2026-10-28: not in Audition (batch 14) |  | M | B158 |  |
| 359 | C260 | T8 | Opus export | Write Opus | missing |  | Audition check 2026-10-28: not found (batch 14) | effectcraft-opusenc (MIT/Apache); ogg (BSD-3-Clause); rusty-opus (BSD-3-Clause; fallback) | M |  | #72 |
| 360 | C261 | T8 | Other formats (WMA, AMR, APE, CAF write) | Less common formats | missing |  | Audition check 2026-10-28: differs (batch 14) |  | M |  |  |
| 361 | C277 | T8 | Streamed export for large files | Write without whole buffer in RAM | partial | CP | save is whole-buffer (SoundCraft encode needs whole buffer per code-map); Audition check 2026-10-28: not found (batch 14) | flac-codec (MIT/Apache) | M |  |  |
| 362 | A228 | T8 | Read formats: AAC/HE-AAC, AIFF/AIFC, AC-3, APE, AU, AVR, BWF, CAF, EC-3, FLAC, HTK, IFF, M4A, MAT, MPC, MP2, MP3, OGA/OGG, PAF, PCM, PVF, RAW, RF64, SD2, SDS, SF, SND, VOC, VOX, W64, WAV, WMA, WVE, XI | Very wide import list | partial |  |  | filmcraft opus/ogg/ac3/mpa (MIT/Apache); rusty-opus (BSD-3-Clause; fallback) | L |  |  |
| 363 | B163 | T8 | Scripting / macro recording of effect sequences (merged: C238) | Record command steps | missing | CP |  |  | L | B008 | #72 |
| 364 | C258 | T8 | AAC/M4A export | Write M4A AAC | missing |  | SoundCraft gap G-21; no encoder in SC; Audition check 2026-10-28: differs (batch 14) | filmcraft-aac + craft-media (MIT/Apache; patent check) | L |  | #72 |
| 365 | C275 | T8 | Immersive/ADM export | Atmos or ADM BWF | missing |  | SoundCraft gap G-12, issue #217; Audition check 2026-10-28: not found (batch 14) | bwavfile (MIT) | L |  |  |
| 366 | A229 | T8 | AAC gapless trimming and edit list | Correct AAC length and start [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-02.md] | upstream-in-progress |  | wait for upstream: SoundCraft PRs #814, #713, #753 (MP3 length), issues #707, #712; usable via audio-io if WavCraft's pin is bumped | filmcraft-aac + craft-media (MIT/Apache; patent check) | S |  |  |
| 367 | A230 | T8 | iXML field-recorder metadata (merged: C289) | Read iXML chunks [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-02.md] | upstream-in-progress |  | wait for upstream: SoundCraft PR #644 (audio-io ixml.rs); SoundCraft PR #644 open | bwavfile (MIT); quick-xml, xmp_writer (MIT; MIT/Apache) | S | A265 |  |
| 637 | C324 | T8 | Crash-safe export finalize step | Write export to a temp file, fsync, then rename into place so a crash never leaves a half-written output | missing | CP | added 2026-10-20 from reuse research; Audition check 2026-10-28: not found (batch 15) | craft-atomic (MIT/Apache); atomic-write-file, tempfile (BSD-3-Clause; MIT/Apache) | M |  |  |
| 638 | C325 | T8 | FLAC, Opus and AAC metadata round-trips | Read and write tags, pictures and chapters in FLAC, Opus and AAC/M4A files without damaging the audio | missing | CP | added 2026-10-20 from reuse research | flac-codec (MIT/Apache); lofty (MIT/Apache) | M |  |  |

## T9 Multitrack and everything else (269)

| rank | id | tier | feature | behaviour | status | CP | reuse / upstream note | reuse | size | depends-on | old issue # |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 368 | C074 | T9 | New multitrack session | Create session with name, folder, sample rate, bit depth, master type, template | missing |  | SoundCraft model/session.rs Session (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | L |  | #72 |
| 369 | C097 | T9 | Audio tracks | Mono/stereo/5.1 audio track holding clips | missing |  | SoundCraft model TrackKind::Audio | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 370 | C116 | T9 | Import audio as clips | Drop file on track makes a clip | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs import_audio_bytes; SoundCraft issue #822 multi-select import |  | M | C097 |  |
| 371 | C152 | T9 | Mixer panel | Channel strips with faders, pan, meters, I/O, inserts, sends | missing |  | SoundCraft mix crate + model Mixer (reusable); SoundCraft mix_window (egui, reference) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | L | C097 | #72 |
| 372 | C214 | T9 | Essential Sound panel (merged: A045) | Assign a clip a type and show type-specific tools | missing |  |  |  | L | C097, C116 | #72 |
| 373 | C215 | T9 | Audio types: Dialogue, Music, SFX, Ambience | Classify a clip; types are mutually exclusive | missing |  |  |  | S | C214 | #72 |
| 374 | B081 | T9 | Reverb (algorithmic, generic) | Generic reverb: decay, pre-delay, diffusion, perception, room size, damping | missing |  | SoundCraft room_reverb / plate_reverb reusable | soundcraft-dsp (MIT/Apache) | M | B001, B002 |  |
| 375 | B148 | T9 | VST3 plugin hosting (merged: C162) | Load third-party VST3 plugins as effects | missing |  | SoundCraft crates/vst3-host (reusable); SoundCraft PRs #806 scan cache, #796 editor open; SoundCraft vst3-host, clap-host, au-host (reusable | soundcraft plugin hosts (MIT/Apache) | L | B002, B005, C157 | #72 |
| 376 | B088 | T9 | Saturator / soft clip building block | Soft saturation for distortion, tube compressor | missing |  | SoundCraft saturator plugin reusable |  | S | B002 |  |
| 377 | B107 | T9 | Pitch Shifter (Effects > Time and Pitch) | Change pitch by semitone/cents/ratio with formant options | missing |  | SoundCraft dsp::offline::pitch_shift + realtime pitch_shifter reusable | soundcraft-dsp (MIT/Apache); signalsmith-stretch (MIT; submodule licence unverified) | M | B001, B002 |  |
| 378 | C174 | T9 | Automation lanes per track | Show a lane for volume, pan, mute, effect parameters | missing |  | SoundCraft model/automation.rs AutomationLane (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C097 |  |
| 379 | C178 | T9 | Edit points: add, drag, delete, thin | Mouse editing of keyframes | missing |  | SoundCraft thin/clear_range | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C174 |  |
| 380 | A321 | T9 | Video panel preview (merged: A043, C285) | Frame display synced to playhead | missing |  | soundcraft has video decode work (PR #585) but app-level | soundcraft-video (MIT/Apache) | L | A231, C284 | #72 |
| 381 | A188 | T9 | Playlist panel | Arrange marker ranges to play in any order | missing |  |  |  | M | A170 |  |
| 382 | B108 | T9 | Stretch and Pitch (process) | Pitch/time stretch with accuracy options, constant-pitch or Resample | missing |  | SoundCraft dsp::offline::time_stretch (WSOLA) + resample reusable | soundcraft-dsp (MIT/Apache); signalsmith-stretch (MIT; submodule licence unverified) | M | B001, B002 |  |
| 383 | C098 | T9 | Mono, stereo and 5.1 track formats | Per-track channel layout, with auto up/down mix | missing |  | SoundCraft model mixer.rs ChannelFormat (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C097 |  |
| 384 | C293 | T9 | 5.1 tracks and master | 6-channel track and master bus | missing |  | SoundCraft ChannelFormat 5.1+ |  | M | C098 |  |
| 385 | C101 | T9 | Video track | Single video track synchronized to audio | missing |  | SoundCraft TrackKind::Video + crates/video (reusable); SoundCraft issues #658,#728 bugs | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source; soundcraft-video (MIT/Apache) | L | C097 | #72 |
| 386 | C241 | T9 | CD Layout (Red Book) tracks | Order tracks, pregap, index points, burn-ready layout | missing |  |  |  | L |  | #72 |
| 387 | B042 | T9 | Delay | Single-tap delay in ms with feedback, mix and invert | missing |  | SoundCraft dsp mod_delay reusable | soundcraft-dsp (MIT/Apache) | S | B001, B002 |  |
| 388 | B103 | T9 | Pitch detection / tracking (YIN) | Monophonic pitch contour | missing |  | SoundCraft dsp::pitch_detect::yin reusable | soundcraft-dsp (MIT/Apache) | S | B002 |  |
| 389 | C060 | T9 | Per-track meters in Mixer and track headers (merged: C167) | Each strip shows its own peak meter | upstream-in-progress |  | wait for upstream: SoundCraft mix StripMeter (reusable); SoundCraft PR #720 meter colour bands; SoundCraft StripMeter | rtrb, basedrop, creek (MIT/Apache); triple_buffer (MPL-2.0) | M | C152 |  |
| 390 | C036 | T9 | Arm tracks for recording | R button per track enables record | upstream-in-progress |  | wait for upstream: SoundCraft model/track + engine plan_takes (reusable); SoundCraft #781 arm persistence bug, #701 machine arming |  | M | C004, C060 |  |
| 391 | C039 | T9 | Punch in/out with range | Record only within punch range; clips trimmed to it | missing |  | SoundCraft session.edit.quickpunch + Recording.punch (reusable) |  | M | C036 | #72 |
| 392 | C084 | T9 | Session markers and ranges | Markers on the multitrack ruler | missing |  | SoundCraft model/markers.rs | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 | #40, #43 |
| 393 | C199 | T9 | Session tempo and time signature | Session-wide BPM and meter | upstream-in-progress |  | wait for upstream: SoundCraft time/tempo.rs TempoMap (reusable) | soundcraft-time (MIT/Apache) | M | C074 |  |
| 394 | C201 | T9 | Metronome | Audible metronome with level, sound choice and count-in | missing |  | SoundCraft session.click |  | M | C199 |  |
| 395 | C129 | T9 | Clip time stretch | Change duration with or without pitch change | upstream-in-progress |  | wait for upstream: SoundCraft clip.stretch (model/clip.rs); SoundCraft issue #171 elastic audio depth | signalsmith-stretch (MIT; submodule licence unverified) | M | C116 |  |
| 396 | C204 | T9 | Clip tempo/beat metadata (Clip Properties tempo) | Mark a clip's tempo so it follows session tempo | missing |  | SoundCraft clip stretch |  | M | C129, C199 |  |
| 397 | C208 | T9 | Edit clip source in Waveform editor and return (merged: A159, C095, C147) | Open a clip's source in the single-file editor and return the edit to the session | missing |  | SoundCraft none |  | M | C074, C116 |  |
| 398 | C075 | T9 | Save/open session file | Session saved with its media; reopens intact | missing |  | SoundCraft engine/io.rs save_session/open_session (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | L | C074 |  |
| 399 | C218 | T9 | Dialogue: Clarity (Dynamics, EQ presets, Enhance Speech) | Compression and EQ quick controls and speech enhancement | missing |  |  | filmcraft-audio-dsp (MIT/Apache); nnnoiseless (BSD-3-Clause) | L | C215 | #72 |
| 400 | C100 | T9 | Master track | Final output track with its own fader/effects | missing |  | SoundCraft TrackKind::Master | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C097 |  |
| 401 | C224 | T9 | Essential Sound presets | Preset menu per type | missing |  |  |  | S | C215 | #72 |
| 402 | B098 | T9 | Stereo Expander (merged: B099) | Widen/narrow stereo image | missing |  |  |  | M | B001 |  |
| 403 | B153 | T9 | Audio Plug-in Manager | Scan paths, enable/disable, add folders, search, rescan | missing |  | SoundCraft scan.rs and registry.rs; SoundCraft PR #806 (scan without freezing UI) in-progress |  | M | B148 |  |
| 404 | C037 | T9 | Per-track input selection | Choose the input channels for each track | missing |  | SoundCraft #336 (select channels as input), SoundCraft #570; SoundCraft Recorder arm(Vec<(TrackId,Vec<usize>,PathBuf)>) |  | M | C036 |  |
| 405 | C038 | T9 | Overdub while playing existing tracks | Record new track while others play back | missing |  | SoundCraft engine/io.rs add_recording place_recording (reusable) |  | M | C036 |  |
| 406 | C043 | T9 | Take numbering and take stacking | Overlapping punch takes stack and are labelled by take number | missing |  | SoundCraft playlists per pass (model) |  | M | C038 |  |
| 407 | C077 | T9 | Session templates | Save session as template and start from it | missing |  | SoundCraft issue #499; SC | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 408 | C086 | T9 | Time selection and playhead in multitrack | Selection tool behaviours across tracks | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 409 | C091 | T9 | Multitrack transport and loop playback | Play, loop, return to start, shuttle | partial |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source; rtrb, basedrop, creek (MIT/Apache); triple_buffer (MPL-2.0) | M | C074 | #24 |
| 410 | C099 | T9 | Bus tracks | Sub-mix tracks that receive sends/outputs | upstream-in-progress |  | wait for upstream: SoundCraft TrackKind::Aux (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C097 |  |
| 411 | C112 | T9 | Track output routing | Each track outputs to master, a bus, or a hardware output | missing |  | SoundCraft Route/OutputPath in model/mixer.rs (reusable) |  | M | C099 |  |
| 412 | C146 | T9 | Clip effects (per-clip rack) | Effects applied to individual clips non-destructively | missing |  |  |  | L | C116 | #33 |
| 413 | C226 | T9 | Per-clip non-destructive application | Panel changes live on clip as applied effects | missing |  |  |  | M | C146 | #72 |
| 414 | C102 | T9 | MIDI tracks | MIDI clips sequenced to instruments | missing |  | SoundCraft midi crate |  | L | C097 | #72 |
| 415 | C195 | T9 | Remix: fit a music clip to a target duration (merged: C229) | Rearrange and stretch a music clip to a new length | missing |  | SoundCraft none |  | L | C129, C204 | #72 |
| 416 | C175 | T9 | Track and clip envelopes | Draw volume/pan envelopes with points | upstream-in-progress |  | wait for upstream: SoundCraft AutomationLane set_point/thin | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C174 | #72 |
| 417 | C223 | T9 | Auto-ducking | Lower music/ambience under other content with sensitivity, amount and fade times | missing |  | none in SC |  | L | C175, C215 | #72 |
| 418 | C185 | T9 | Export Multitrack Mixdown | Render time selection, entire session or selected clips to a file | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs Bounce/render_range (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 419 | C274 | T9 | Export video with mixed audio | Mix audio into video file | missing |  | SoundCraft video crate mux (isobmff mux) partial; Audition check 2026-10-28: differs (batch 14) |  | L | C101, C185 |  |
| 420 | C278 | T9 | Import/export Final Cut XML | Exchange sessions with video editors | missing |  | SoundCraft uses fcpxml? (not found) | quick-xml, xmp_writer (MIT; MIT/Apache) | L | C074 |  |
| 421 | C284 | T9 | Video playback in session | Show video in Video panel in sync | missing |  | SoundCraft video crate (decode H.264, ProRes) reusable |  | L | C101 | #72 |
| 422 | C216 | T9 | Dialogue: Loudness (Auto-Match) (merged: C145, C227) | Match dialogue clips to a target LUFS | missing |  |  |  | M | B125, C215 | #38, #39, #72 |
| 423 | C217 | T9 | Dialogue: Repair (reduce noise, rumble, DeHum, DeEss, reverb) | One-slider repairs for common problems | missing |  |  | nnnoiseless (BSD-3-Clause) | L | C215 | #37, #72 |
| 424 | A044 | T9 | Session Properties / Mixer / Playlist panels | Mixer and session properties belong to multitrack; Playlist is waveform-side | missing |  |  |  | S |  |  |
| 425 | A168 | T9 | Edit Tempo | Set tempo and time signature of the file for Bars and Beats | missing |  |  |  | S | A101 |  |
| 426 | A187 | T9 | CD track markers (merged: C242) | Removed from Audition in 26.3 [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-01.md] | missing |  |  |  | S | C084 | #40, #72 |
| 427 | A190 | T9 | Reorder and remove playlist items | Drag to reorder; remove | missing |  |  |  | S | A188 |  |
| 428 | A191 | T9 | Save playlist in WAV | Stored in WAV file | missing |  |  | bwavfile (MIT) | S | A183, A188 |  |
| 429 | A283 | T9 | Multitrack / Multitrack Clips (merged: C016) | Multitrack only | missing |  |  |  | S | A273 |  |
| 430 | A287 | T9 | Video (spot frame, timecode overlay) | Video panel options | missing |  |  |  | S | A273, A321 |  |
| 431 | A306 | T9 | Generate DTMF signals (older versions; not fetched) (merged: B120) | Phone tones | missing |  |  |  | S | A303 |  |
| 432 | A322 | T9 | Video panel scaling, best fit, full-screen monitor | Display options | missing |  |  |  | S | A321 |  |
| 433 | A323 | T9 | Timecode overlay | Overlay session or media time | missing |  |  |  | S | A321 |  |
| 434 | A324 | T9 | Snap to video frames and spot frame preview | Align audio to picture | missing |  |  |  | S | A128, A321 |  |
| 435 | A326 | T9 | Edit Original link data in mixdown | Reopen the session that made a file | missing |  |  |  | S |  |  |
| 436 | A349 | T9 | Help menu / About / tutorials (merged: C309) | Learn panel, help links | missing |  |  |  | S |  |  |
| 437 | B043 | T9 | Echo | Multi-repeat echo with decay, delay, lock-left-right | missing |  | SoundCraft mod_delay partial |  | S | B042 |  |
| 438 | B061 | T9 | EQ analysis overlay (Spectrum behind curves) | Spectrum shown beneath EQ curve | missing |  |  |  | S | B011, B058 |  |
| 439 | B062 | T9 | Chorus (merged: B065) | Multi-voice detuned delay modulation | missing |  | SoundCraft chorus plugin reusable (modulation.rs); SoundCraft chorus+flanger |  | S | B001, B002, B063 |  |
| 440 | B063 | T9 | Flanger | Short swept delay with feedback | missing |  | SoundCraft flanger plugin reusable |  | S | B001, B002 |  |
| 441 | B064 | T9 | Phaser | Allpass stage sweep | missing |  | SoundCraft phaser plugin reusable |  | S | B001, B002 |  |
| 442 | B083 | T9 | Studio Reverb | Lighter reverb with fewer controls | missing |  | SoundCraft room_reverb |  | S | B081 |  |
| 443 | B086 | T9 | Plate reverb | Plate-style preset [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-05.md] | missing |  | SoundCraft plate_reverb |  | S | B081 |  |
| 444 | B095 | T9 | Bass Enhancer / Exciter | Low frequency enhancement | missing |  |  |  | S | B088 |  |
| 445 | B100 | T9 | Mid/Side conversion | M/S encode/decode helper | missing |  |  |  | S | B027 |  |
| 446 | B101 | T9 | Pan/Expand (waveform) | Pan a stereo or mono selection | missing |  |  |  | S | B027 |  |
| 447 | B110 | T9 | Varispeed | Change speed and pitch together via resampling | missing |  | SoundCraft dsp::offline::resample reusable |  | S | B108 |  |
| 448 | B111 | T9 | Pitch presets (octave, formant shift) | Predefined settings [Audition check 2026-10-28, checked in Adobe Audition 26.5: differs; the Pitch Shifter has no formant control; see logs/2026-10-23/feature-verification/batch-05.md] | missing |  |  | signalsmith-stretch (MIT; submodule licence unverified) | S | B107 |  |
| 449 | B112 | T9 | Time-stretch to fit duration | Target a given length | missing |  |  | signalsmith-stretch (MIT; submodule licence unverified) | S | B108 |  |
| 450 | C041 | T9 | Punch Again | Repeat the same punch quickly for another take [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-07.md] | missing |  |  |  | S | C039 |  |
| 451 | C047 | T9 | Record-enable key and transport keys | Shortcuts for record/arm | missing |  |  |  | S | C036 |  |
| 452 | C052 | T9 | Recording trouble and error reporting | Report device loss and dropouts during multitrack record [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-07.md] | missing |  | SoundCraft Recorder::trouble |  | S | C036 |  |
| 453 | C061 | T9 | Master meter | Meter on the master output | missing |  | SoundCraft StripMeter | rtrb, basedrop, creek (MIT/Apache); triple_buffer (MPL-2.0) | S | C060 |  |
| 454 | C078 | T9 | Session properties | Rate, bit depth, master channel format, tempo; change later | missing |  | SoundCraft model Session | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C074 |  |
| 455 | C080 | T9 | Recent sessions list | Menu of recently opened sessions | missing |  | SoundCraft issue #421 | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C075 |  |
| 456 | C083 | T9 | Time ruler formats | Ruler units: time, samples, frames, bars and beats | partial |  | SoundCraft time/format.rs, grid.rs, timecode.rs (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C074 |  |
| 457 | C087 | T9 | Edit cursor vs playhead | Independent edit/playhead [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C086 |  |
| 458 | C088 | T9 | Snapping | Snap clips and cursor to grid, markers, clips, ruler | upstream-in-progress |  | wait for upstream: SoundCraft model/time grid.rs; SoundCraft PR #589 grid | soundcraft-time (MIT/Apache); SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 | #22 |
| 459 | C089 | T9 | Snap to options | Toggle snapping to ruler ticks, clip edges, markers, loop points, zero crossings | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C088 | #22 |
| 460 | C092 | T9 | Loop range playback | Loop selection or session | missing |  | SoundCraft session.edit.loop_playback | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C091 | #24 |
| 461 | C093 | T9 | Metering of preview/audition | Preview in context | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S |  |  |
| 462 | C094 | T9 | Session notes/metadata | Info fields on the session (title, notes) [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C074 |  |
| 463 | C104 | T9 | Reorder tracks by dragging | Drag track headers up/down | missing |  |  |  | S | C097 |  |
| 464 | C105 | T9 | Track names, colours, notes | Rename and colour tracks [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  | SoundCraft Track::new name/color |  | S | C097 |  |
| 465 | C106 | T9 | Track height presets | Small/medium/large track height [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  | SoundCraft model TrackHeight (reusable) |  | S | C097 |  |
| 466 | C110 | T9 | Invert phase per track | Polarity flip button on a strip | missing |  | SoundCraft mix lib has phase |  | S | C097 |  |
| 467 | C111 | T9 | Split stereo to mono tracks | Break a stereo track into two mono tracks | missing |  | SoundCraft engine split into mono (bug #663 fixed by PR) |  | S | C098 |  |
| 468 | C123 | T9 | Clip fade-in/out handles | Draggable fade handles on each clip | upstream-in-progress |  | wait for upstream: SoundCraft model clip.rs Fade/FadeShape (reusable) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C116 | #27 |
| 469 | C124 | T9 | Fade curve shapes (linear, cosine, logarithmic) | Pick the fade curve [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-09.md] | partial |  | SoundCraft FadeShape (reusable) |  | S | C123 | #27 |
| 470 | C125 | T9 | Crossfades between overlapping clips | Overlap two clips and fade them, with shape and length | upstream-in-progress |  | wait for upstream: SoundCraft edit.rs create_fades/delete_fades (crossfade); SoundCraft PR #811 |  | M | C123 |  |
| 471 | C126 | T9 | Auto-crossfade on overlap | Overlapping clips automatically crossfade | missing |  | SoundCraft edit.rs create_fades on range |  | S | C125 |  |
| 472 | C132 | T9 | Lock clips in time | Prevent moving of locked clip | missing |  | SoundCraft edit.rs locked() |  | S | C116 |  |
| 473 | C133 | T9 | Clip colour and naming | Colour clips | missing |  | SC |  | S | C116 |  |
| 474 | C135 | T9 | Mute clip | Mute individual clip | missing |  | SC |  | S | C116 |  |
| 475 | C153 | T9 | Mute | Silence a track | missing |  | SoundCraft mix strip mute | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C152 |  |
| 476 | C155 | T9 | Track volume fader | Fader in dB with taper | missing |  | SoundCraft mixer.rs fader_pos_to_db/db_to_pos | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C152 |  |
| 477 | C166 | T9 | Show/hide mixer sections | Toggle EQ, effects, sends, I/O rows | missing |  |  |  | S | C152 |  |
| 478 | C171 | T9 | Master output mode mono/stereo/5.1 | Master bus format | missing |  | SoundCraft main_format |  | S | C100 |  |
| 479 | C179 | T9 | Curve/hold shapes for keyframes | Linear/bezier/step segments | missing |  | SoundCraft AutomationPoint (linear) |  | S | C178 |  |
| 480 | C181 | T9 | Snap keyframes to grid | Snap points | missing |  | Audition check 2026-10-28: not found (batch 12) |  | S | C088, C178 |  |
| 481 | C176 | T9 | Automation modes (Off, Read, Write, Touch, Latch) | Record movements on faders/knobs | upstream-in-progress |  | wait for upstream: SoundCraft AutomationMode (Off/Read/Touch/Latch/TouchLatch/Write/Trim) reusable | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C152, C174 |  |
| 482 | C183 | T9 | Automation Read/write lanes bypass | Toggle automation on/off | missing |  | SoundCraft AutomationMode::Off |  | S | C176 |  |
| 483 | C188 | T9 | Mixdown of selected clips | Render only chosen clips | missing |  | SoundCraft bounce clips | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C185 |  |
| 484 | C193 | T9 | Normalize on export | Apply normalise at mixdown | missing |  |  |  | S | C185 | #29 |
| 485 | C194 | T9 | Render tail for effects | Add time to catch reverb tails | missing |  | Audition check 2026-10-28: not found (batch 12) |  | S | C185 |  |
| 486 | C202 | T9 | Count-in / pre-roll click | Clicks before recording | missing |  | SoundCraft session.pre_roll; Audition check 2026-10-28: differs (batch 12) |  | S | C201 |  |
| 487 | C207 | T9 | MIDI clip-to-audio for click | Metronome via MIDI | missing |  | Audition check 2026-10-28: not in Audition (batch 13) |  | S | C201 |  |
| 488 | C210 | T9 | Convert multitrack clip to new waveform | Extract a clip to a waveform file | missing |  | SoundCraft export_clips |  | S | C116 |  |
| 489 | C211 | T9 | Insert a waveform file into multitrack | Send an open waveform to a track | missing |  |  |  | S | C074 |  |
| 490 | C225 | T9 | Custom presets for the panel | Save own | missing |  |  |  | S | C224 | #72 |
| 491 | C230 | T9 | Reset panel | Clear applied changes | missing |  | Audition check 2026-10-28: not found (batch 14) |  | S | C226 |  |
| 492 | C245 | T9 | Track gap and fades | Set gap length | missing |  | Audition check 2026-10-28: not in Audition (batch 14) |  | S | C241 | #72 |
| 493 | C295 | T9 | Downmix 5.1 to stereo | Fold-down options | missing |  | SoundCraft mix itu_stereo_fold / fold_down_stereo |  | S | C293 |  |
| 494 | A024 | T9 | Media Browser panel | Browse folders and drives, filter by media type, preview and open files | missing |  |  |  | M | A020 |  |
| 495 | A189 | T9 | Play list items/loops | Play all or from item; per-item loop count | missing |  |  |  | M | A188 |  |
| 496 | A231 | T9 | Open audio of video files (AVI, MOV, MP4, MPEG, 3GP, FLV, WMV...) (merged: A320, C287, C213) | Opens soundtrack only | partial |  | SoundCraft issues #728,#658,#758; SoundCraft video demux |  | M | C101, C284 |  |
| 497 | B020 | T9 | Pre/post fader positioning of slots | Slot ordering relative to track fader (Multitrack-style); mostly relevant when multitrack exists | missing |  |  |  | M | B013 | #72 |
| 498 | B039 | T9 | Tube-modeled Compressor | Compressor with tube-like saturation and bias/character | missing |  | SoundCraft saturator+compressor could compose |  | M | B037, B088 |  |
| 499 | B040 | T9 | Sidechain / key input for dynamics | Detector driven from another source [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-04.md] | missing |  | SoundCraft issue #502 open (not a PR) |  | M | B037 |  |
| 500 | B044 | T9 | Dynamic Delay / Echo Chamber | Delay with room-style echoes from a chosen chamber size [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-04.md] | missing |  |  |  | M | B042, B081 |  |
| 501 | B059 | T9 | Scientific Filter | Bessel/Butterworth/Chebyshev/Elliptic LP/HP/BP/BS of chosen order | missing |  | SoundCraft biquad.rs partial building block |  | M | B002 |  |
| 502 | B082 | T9 | Full Reverb | Convolution-style quality algorithmic reverb with early-reflection controls | missing |  | SoundCraft FDN reverb partial |  | M | B081 |  |
| 503 | B087 | T9 | Distortion | Transfer-curve editor distortion with DC and harmonic controls | missing |  | SoundCraft saturator/lofi partial |  | M | B001, B088 |  |
| 504 | B089 | T9 | Doppler Shifter | Simulate moving source pitch and level change with path settings | missing |  |  |  | M | B107 |  |
| 505 | B093 | T9 | Vocal Enhancer | Single-slider improvement for male/female/music | missing |  |  |  | M | B028, B057 |  |
| 506 | B106 | T9 | Pitch Bender | Draw pitch envelope over time | missing |  |  | signalsmith-stretch (MIT; submodule licence unverified) | M | B107 |  |
| 507 | B109 | T9 | Stretch modes (Audition, iZotope Radius) | Mode with precision options | missing |  |  |  | M | B108 |  |
| 508 | B150 | T9 | CLAP plugin hosting | Load CLAP plugins [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-06.md] | missing |  | SoundCraft crates/clap-host (reusable); SoundCraft PR #625 | soundcraft plugin hosts (MIT/Apache) | M | B002, B005 |  |
| 509 | B151 | T9 | Audio Unit hosting (macOS) | Load AU plugins | missing |  | SoundCraft crates/au-host (reusable); SoundCraft #687 PR, #501 issue | soundcraft plugin hosts (MIT/Apache) | M | B002, B005 |  |
| 510 | B155 | T9 | Plugin state presets and Favorites | Save plugin state | missing |  | SoundCraft Plugin::save_state | soundcraft plugin hosts (MIT/Apache) | M | B007, B148 |  |
| 511 | B156 | T9 | Plugin latency compensation and tail | Account for latency when rendering [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-06.md] | missing |  |  |  | M | B148 |  |
| 512 | C040 | T9 | Punch and Roll with pre-roll and countdown | Play lead-in then record from the punch point, with visible count | missing |  | SoundCraft edit.pre_roll in session.rs |  | M | C039 |  |
| 513 | C051 | T9 | Record to disk (not RAM) for long takes | Stream take files to disk | missing | CP | SoundCraft gap G-06 | rtrb, basedrop, creek (MIT/Apache); triple_buffer (MPL-2.0) | M | C036 |  |
| 514 | C079 | T9 | Files panel: import into session | List imported files and drag onto timeline | partial |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 | #13 |
| 515 | C081 | T9 | Session-level undo/redo | Undo on timeline edits | missing |  | SoundCraft engine undo | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 516 | C082 | T9 | Timeline zoom and scroll | Zoom time ruler, vertical track zoom, zoom to session | missing |  | SoundCraft ui-egui reference | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 |  |
| 517 | C085 | T9 | Marker/cue list panel (Markers) | List, rename, jump to markers; export | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C084 | #41 |
| 518 | C090 | T9 | Scrubbing in multitrack | Scrub the playhead across all tracks | missing |  |  | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C074 | #25 |
| 519 | C107 | T9 | Collapse/hide tracks, track groups/folders | Group tracks into folders and hide them [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  | SoundCraft TrackKind::Folder |  | M | C097 |  |
| 520 | C108 | T9 | Bounce to new track | Render one track's clips (with effects) into a new clip | missing |  | SoundCraft engine/io.rs commit_track / consolidate |  | M | C097, C185 |  |
| 521 | C109 | T9 | Freeze track | Render a track's effects so it stops using CPU [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-08.md] | missing |  | SoundCraft mix freeze |  | M | C097 |  |
| 522 | C113 | T9 | Track input routing | Each track takes input from a device channel or bus | missing |  | SoundCraft Route |  | M | A211 |  |
| 523 | C114 | T9 | Track effect rack per track (inserts) | Chain of effects on each track | missing |  | SoundCraft model Insert, mix inserts |  | M | C097 | #33 |
| 524 | C121 | T9 | Copy, cut, paste, duplicate clips | Clipboard for clips; duplicate | missing |  | SoundCraft edit.rs copy_range/paste |  | M | C116 |  |
| 525 | C127 | T9 | Clip gain (on-clip handle) | Per-clip gain, dB, drag line on clip | upstream-in-progress |  | wait for upstream: SoundCraft Clip gain | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C116 |  |
| 526 | C128 | T9 | Clip gain envelope / volume keyframes | Clip-level automation of gain | missing |  | SoundCraft clip_gain_to_volume |  | M | C127, C175 |  |
| 527 | C130 | T9 | Clip pitch shift (Properties) | Per-clip pitch/tempo controls in Properties | missing |  | SoundCraft midi pitch only | signalsmith-stretch (MIT; submodule licence unverified) | M | C129 |  |
| 528 | C131 | T9 | Clip properties panel | Edit clip name, colour, gain, pitch, stretch, lock | missing |  |  |  | M | C116 |  |
| 529 | C136 | T9 | Selection tools: Move, Razor, Time selection, Slip | Tool set for clip operations | missing |  |  |  | M | C116 | #20 |
| 530 | C118 | T9 | Trim clip edges | Drag edges to trim start/end non-destructively | upstream-in-progress |  | wait for upstream: SoundCraft edit.rs trim_clip |  | M | C116 |  |
| 531 | C137 | T9 | Slip edit (change audio within clip) | Move source content inside a clip's boundaries | missing |  | SoundCraft clip source_offset |  | M | C118 |  |
| 532 | C141 | T9 | Overlapping clips and clip stacking | Show and edit overlaps (top clip wins) | missing |  | SoundCraft playlist |  | M | C116 |  |
| 533 | C150 | T9 | Merge clips / Convert to unique copy | Render selected clips into one, or make clip independent copy | missing |  | SoundCraft consolidate |  | M | C116 |  |
| 534 | C157 | T9 | Track effects inserts (pre/post fader) | Slots for effects chain per track with bypass | upstream-in-progress |  | wait for upstream: SoundCraft model Insert | soundcraft plugin hosts (MIT/Apache); SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C152 |  |
| 535 | C161 | T9 | Effects Rack for tracks | Rack panel showing track effects | missing |  |  |  | M | C157 | #33 |
| 536 | C164 | T9 | Track EQ section on strip | Quick EQ on every strip | missing |  | SoundCraft dsp plugins eq.rs, strip.rs |  | M | C152 |  |
| 537 | C165 | T9 | Mixer input/output selectors | Pick routes in mixer | missing |  | SoundCraft Route |  | M | C112 |  |
| 538 | C170 | T9 | Monitor/Control Room output | Separate monitor out | missing |  | Audition check 2026-10-28: not in Audition (batch 11) |  | M | A211 |  |
| 539 | C117 | T9 | Move clips (time and between tracks) | Drag or arrow-nudge clips | upstream-in-progress |  | wait for upstream: SoundCraft engine/edit.rs move_clips (reusable) |  | M | C116 |  |
| 540 | C180 | T9 | Move automation with clips | Envelopes follow clip moves/splits | missing |  | SoundCraft shift_from; Audition check 2026-10-28: differs (batch 12) |  | M | C117, C178 |  |
| 541 | C182 | T9 | Copy/paste automation | Copy lane ranges | missing |  | Audition check 2026-10-28: not found (batch 12) |  | M | C178 |  |
| 542 | C184 | T9 | Auto-generated volume keyframes (ducking) | Generate envelope from analysis | missing |  | Audition check 2026-10-28: differs (batch 12) |  | M | C223 |  |
| 543 | C187 | T9 | Mixdown channel layouts (mono/stereo/5.1 at once) | Render several layouts in one pass | missing |  | SoundCraft fold_down_stereo, main_format | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C185 |  |
| 544 | C190 | T9 | Mix down session to new waveform (Bounce to new file) (merged: C212) | Render mix and open as waveform | missing |  | SoundCraft bounce; SoundCraft Bounce |  | M | C185, C208 |  |
| 545 | C198 | T9 | Export with loudness target | Meet a LUFS target on export | missing |  | Audition check 2026-10-28: differs (batch 12) | ebur128 (MIT; test oracle) | M | B125, C185 | #38 |
| 546 | C200 | T9 | Tempo/meter changes along the timeline | Multiple tempo events | missing |  | SoundCraft TempoMap set_tempo/set_meter; Audition check 2026-10-28: not in Audition (batch 12) |  | M | C199 |  |
| 547 | C203 | T9 | Bars-and-beats ruler and snapping | Musical grid | missing |  | SoundCraft time/grid.rs |  | M | C199 |  |
| 548 | C205 | T9 | Tap tempo / detect tempo | Detect or tap tempo | missing |  | SoundCraft dsp pitch_detect |  | M | C199 |  |
| 549 | C209 | T9 | Apply effect to clip source file (destructive) | Process clip's source in Waveform and update all clips | missing |  | Audition check 2026-10-28: differs (batch 13) |  | M | C208 |  |
| 550 | C219 | T9 | Dialogue: Creative (Reverb) | Reverb amount slider | missing |  | SoundCraft dsp reverb |  | M | C215 |  |
| 551 | C220 | T9 | Music: Loudness, Ducking, Remix (duration), Stereo Width, Dynamics | Music-type tools | missing |  |  |  | M | C215 | #72 |
| 552 | C221 | T9 | SFX: Loudness, Reverb, panner, EQ | SFX tools | missing |  |  |  | M | C215 | #72 |
| 553 | C222 | T9 | Ambience: Loudness, Reverb, stereo width, dialogue-aware ducking | Ambience tools | missing |  |  |  | M | C215 | #72 |
| 554 | C244 | T9 | CD-Text and ISRC/UPC metadata | Titles, ISRC codes | missing |  |  |  | M | C241 | #72 |
| 555 | C246 | T9 | Podcast session templates | Start sessions for podcast with prepared tracks | missing |  | Audition check 2026-10-28: differs (batch 14) |  | M | C077 |  |
| 556 | C250 | T9 | Chapter markers for podcasts | Markers exported as chapters in MP3/M4A | missing |  | Audition check 2026-10-28: differs (batch 14) | id3 (MIT) | M | A248, C084 |  |
| 557 | C271 | T9 | Export queue / background export | Run exports without blocking | missing |  | SoundCraft Bounce::step progress; Audition check 2026-10-28: differs (batch 14) |  | M | C185 |  |
| 558 | C286 | T9 | Video/audio sync and frame rate display | Timecode frame rates, drop-frame | missing |  | SoundCraft time/timecode.rs reusable |  | M | C284 |  |
| 559 | C291 | T9 | Import/export MIDI files | SMF | missing |  | SoundCraft midi/smf.rs; Audition check 2026-10-28: not in Audition (batch 15) |  | M | C102 |  |
| 560 | C294 | T9 | Surround Panner | 2D panner for tracks | missing |  | SoundCraft model SurroundPan; Audition check 2026-10-28: differs (batch 15) |  | M | C293 |  |
| 561 | C297 | T9 | Surround meters | Meter per speaker | missing |  | Audition check 2026-10-28: not found (batch 15) |  | M | C060, C293 |  |
| 562 | A015 | T9 | Waveform / Multitrack editor switch (key 1) | Toggle between single-file and multitrack views | missing |  |  | soundcraft ui-egui patterns (MIT/Apache) | L |  |  |
| 563 | A166 | T9 | Frequency Band Splitter | Split selection into up to 8 band copies with crossovers (multitrack tracks) | missing |  |  |  | L |  |  |
| 564 | A232 | T9 | Import Premiere sequence (.prproj) | Imports sequences referencing media (multitrack) [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-02.md] | missing |  |  |  | L |  |  |
| 565 | A233 | T9 | Open session files (sesx, Audition XML, Premiere XML, FCP XML, OMF) | Opens in multitrack | missing |  |  |  | L |  |  |
| 566 | A279 | T9 | Control Surface (FaderPort V1, HUI) | MIDI control surface setup | missing |  |  |  | L | A273 |  |
| 567 | A305 | T9 | Generate Speech (OS voices) | Text-to-speech inserted as audio | missing |  |  |  | L |  |  |
| 568 | A353 | T9 | Accessibility (screen reader, high-contrast) (merged: C305) | Not in Audition docs fetched [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-03.md] | missing |  |  |  | L |  |  |
| 569 | B084 | T9 | Convolution Reverb | Impulse-response reverb with IR loading, mix, EQ | missing |  | no convolution in SoundCraft dsp | fft-convolver (MIT) | L | B001 |  |
| 570 | B085 | T9 | Surround Reverb | Multichannel reverb for 5.1 | missing |  |  | fft-convolver (MIT) | L | B081 |  |
| 571 | B090 | T9 | Guitar Suite | Chain of guitar effects (compressor, filter, modeling) | missing |  |  |  | L | B037, B057, B088 |  |
| 572 | B092 | T9 | Mastering effect | EQ/compress/limit/exciter/widener chain | missing |  |  |  | L | B034, B037, B057, B098 |  |
| 573 | B096 | T9 | Dynamic EQ (band-limited dynamic) | EQ with level-dependent bands; not in Audition core but Studio EQ [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-05.md] | missing |  |  |  | L | B037, B057 |  |
| 574 | B097 | T9 | Center Channel Extractor | Extract/attenuate center-panned content with frequency range and EQ | missing |  |  |  | L | B011 |  |
| 575 | B102 | T9 | Automatic Pitch Correction | Retune toward scale/key with attack, sensitivity, reference and presets | missing |  | SoundCraft dsp::pitch_detect YIN reusable for tracking |  | L | B103, B107 |  |
| 576 | B104 | T9 | Manual Pitch Correction (graphical) | Draw/edit pitch curve of a waveform | missing |  |  |  | L | A067, B103 |  |
| 577 | B149 | T9 | Plugin editor windows | Native plugin GUI | missing |  | SoundCraft ffi.rs; SoundCraft issues #598, #596, #722 show instability | soundcraft plugin hosts (MIT/Apache) | L | B148 |  |
| 578 | B152 | T9 | VST (2) plugin hosting | Legacy VST2 | missing |  | SoundCraft issue #30 open |  | L | B005 |  |
| 579 | B154 | T9 | Plugin scan crash isolation / blacklist | Do not die when a plugin crashes [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-06.md] | missing |  | SoundCraft states in-process |  | L | B153 |  |
| 580 | B157 | T9 | Plugin parameter automation | Automate parameters (multitrack) | missing |  | SoundCraft issue #600 |  | L | B148 |  |
| 581 | C044 | T9 | Composite takes / comping | Pick parts of different takes by range to build one composite clip [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-07.md] | missing |  | none in SoundCraft found (playlists exist |  | L | C043 |  |
| 582 | C169 | T9 | Time-aligned latency compensation across tracks | Plugin delay compensation | missing |  | SoundCraft PR #701 Delay Comp views; Audition check 2026-10-28: not found (batch 11) | rtrb, basedrop, creek (MIT/Apache); triple_buffer (MPL-2.0) | L | C157 |  |
| 583 | C196 | T9 | Remix segment detection and quality options | Find repeatable sections and set length limits for the remix | missing |  |  |  | L | C195 |  |
| 584 | C228 | T9 | Enhance Speech | AI speech clean-up | missing |  | SoundCraft issue named 'Enhance Speech'; Audition check 2026-10-28: differs (batch 13) |  | L | C218 | #72 |
| 585 | C243 | T9 | Burn audio CD | Write disc | missing |  | Audition check 2026-10-28, checked in Adobe Audition 26.5: differs, built-in CD burning removed in 26.3 but Extract Audio from CD remains (batch 14) |  | L | C241 | #72 |
| 586 | C251 | T9 | Voice-over edit tools | Edit by transcript | missing |  | SoundCraft PR #676 VO Cut Pipeline; SoundCraft G-18 |  | L |  | #72 |
| 587 | C252 | T9 | Speech to text transcription | Transcribe audio to text with search | missing |  | SoundCraft gap G-18, PR #676; Audition check 2026-10-28: not in Audition (batch 14) |  | L |  | #72 |
| 588 | C280 | T9 | OMF import/export | Open Media Framework exchange | missing |  | SoundCraft gap G-03 (no OMF) |  | L | C074 |  |
| 589 | C288 | T9 | Replace audio in exported video | Mux mix into a video copy | missing |  | SoundCraft isobmff mux; Audition check 2026-10-28: differs (batch 15) |  | L | C274 |  |
| 590 | C292 | T9 | Timecode sync and external sync | MTC/LTC | missing |  | SoundCraft gap G-11; Audition check 2026-10-28: differs (batch 15) |  | L |  |  |
| 591 | C299 | T9 | Dolby Atmos/immersive | Object audio | missing |  | SoundCraft issue #217, G-12; Audition check 2026-10-28: not found (batch 15) |  | L |  |  |
| 592 | C119 | T9 | Split clip at cursor/selection | Razor split at playhead or range | upstream-in-progress |  | wait for upstream: SoundCraft edit.rs split_clip/separate_at |  | M | C116 |  |
| 593 | C042 | T9 | Loop recording with takes | Each loop pass becomes its own take clip [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-07.md] | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs add_recording pass (one clip per pass, new playlist) |  | M | C036 |  |
| 594 | C076 | T9 | Session media: consolidate, relink, missing-file handling | Collect media into a folder, relink lost files [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-07.md] | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs relink_videos, session_bundle | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C075 |  |
| 595 | C158 | T9 | Sends | Per-track sends to buses with level/pre-post | upstream-in-progress |  | wait for upstream: SoundCraft SendSlot | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C099 |  |
| 596 | C138 | T9 | Heal separation after delete | Rejoin clips split by a cut [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft PR #780, issue #772 |  | S | C119 |  |
| 597 | C139 | T9 | Nudge clips by keys | Arrow keys nudge [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft PRs #786,#761,#736 |  | S | C117 |  |
| 598 | C140 | T9 | Clip snapping to edges and grid | Edges snap while dragging | upstream-in-progress |  | wait for upstream: SoundCraft time grid | soundcraft-time (MIT/Apache) | S | C088 |  |
| 599 | C148 | T9 | Offline media / missing file replace | Replace missing clip source [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft relink |  | S | C076 |  |
| 600 | C154 | T9 | Solo / Solo Safe | Solo tracks; safe-solo option [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft mix solo | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C152 |  |
| 601 | C156 | T9 | Track pan / balance | Pan knob, balance for stereo, pan law | upstream-in-progress |  | wait for upstream: SoundCraft dsp/pan.rs | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C152 |  |
| 602 | C189 | T9 | Mixdown of entire session or time range | Render range | upstream-in-progress |  | wait for upstream: SoundCraft render_range, PR #760 | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | S | C185 |  |
| 603 | C191 | T9 | Dither on mixdown | Dither options on bounce | upstream-in-progress |  | wait for upstream: SoundCraft issue #522 (cannot turn off), #548; Audition check 2026-10-28: differs (batch 12) |  | S | A163, C185 |  |
| 604 | C192 | T9 | Bounce report (peaks, LUFS) | Peak/loudness report after render | upstream-in-progress |  | wait for upstream: SoundCraft PRs #734, issue #717; Audition check 2026-10-28: not in Audition (batch 12) |  | S | B125, C185 |  |
| 605 | C197 | T9 | Bounce from command line | Headless bounce | upstream-in-progress |  | wait for upstream: SoundCraft PR #760,#745; Audition check 2026-10-28: not in Audition (batch 12) |  | S | C185 |  |
| 606 | C301 | T9 | Windows on ARM / Apple Silicon native | ARM builds | upstream-in-progress |  | wait for upstream: SoundCraft issue #699 |  | S | A345 | #48, #50 |
| 607 | C045 | T9 | Per-track input monitoring | Monitor button per track; auto vs manual monitor mode | upstream-in-progress |  | wait for upstream: SoundCraft PR #808, #782, #769; SoundCraft #42 opt-out request |  | M | C036 |  |
| 608 | C046 | T9 | Live clip drawing while recording | Waveform grows on the timeline during record | upstream-in-progress |  | wait for upstream: SoundCraft #403 open |  | M | C036 |  |
| 609 | C048 | T9 | Record several tracks at once | Multi-channel interface capture to separate tracks | upstream-in-progress |  | wait for upstream: SoundCraft Recorder arm takes Vec per track |  | M | C037 |  |
| 610 | C103 | T9 | Add/remove/duplicate tracks | Insert tracks of each type, multiple at once | upstream-in-progress |  | wait for upstream: SoundCraft issue #547 (several rows); SoundCraft engine/cmd/track.rs |  | M | C097 |  |
| 611 | C115 | T9 | Auto channel conversion between formats | Mix stereo into 5.1 etc. on routing | upstream-in-progress |  | wait for upstream: SoundCraft PR #574, #584 (conversion coefficients) |  | M | C098 |  |
| 612 | C120 | T9 | Delete/ripple delete clips | Remove clip or range and close the gap | upstream-in-progress |  | wait for upstream: SoundCraft edit.rs clear_range, shift_from, ripple |  | M | C116 |  |
| 613 | C122 | T9 | Loop clip | Stretch a clip's end to repeat its contents | upstream-in-progress |  | wait for upstream: SoundCraft clip.loop (issue #273 open detached copies) |  | M | C116 |  |
| 614 | C134 | T9 | Group clips | Move/trim grouped clips together | upstream-in-progress |  | wait for upstream: SoundCraft engine/clip_group_file.rs; SoundCraft PR #609 clip definitions |  | M | C116 |  |
| 615 | C143 | T9 | Strip silence / split at transients | Cut clips at silences or transients [Audition check 2026-10-28: differs; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs strip_silence, transients_in (reusable) |  | M | C119 |  |
| 616 | C149 | T9 | Extract audio from video clip | Pull audio out of video source to a track | upstream-in-progress |  | wait for upstream: SoundCraft issues #728,#658 import video | soundcraft-video (MIT/Apache) | M | C101 |  |
| 617 | C151 | T9 | Clip variations/playlists | Alternate playlists on a track [Audition check 2026-10-28, checked in Adobe Audition 26.5: exists, a Playlist panel exists; see logs/2026-10-23/feature-verification/batch-10.md] | upstream-in-progress |  | wait for upstream: SoundCraft model Playlist per pass (loop recording) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C042 |  |
| 618 | C159 | T9 | Sidechain routing | Use another track as compressor key | upstream-in-progress |  | wait for upstream: SoundCraft issue #502 open; Audition check 2026-10-28: differs (batch 11) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C158 |  |
| 619 | C160 | T9 | Track groups / VCA | Link fader of a group | upstream-in-progress |  | wait for upstream: SoundCraft TrackKind::Vca; Audition check 2026-10-28: not in Audition (batch 11) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C152 |  |
| 620 | C172 | T9 | Track presets | Save and recall a track's inserts and sends | upstream-in-progress |  | wait for upstream: SoundCraft PRs #670,#671; Audition check 2026-10-28: differs (batch 11) |  | M | C157 |  |
| 621 | C177 | T9 | Automate effect parameters | Any plugin parameter gets a lane | upstream-in-progress |  | wait for upstream: SoundCraft AutoParam | soundcraft plugin hosts (MIT/Apache); SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C157, C174 |  |
| 622 | C186 | T9 | Per-track/bus stems export | Write each track as its own file | upstream-in-progress |  | wait for upstream: SoundCraft engine/io.rs bounce_stems (reusable); Audition check 2026-10-28: differs (batch 12) | SoundCraft has it (model/mix/engine); blocked on disk-backed sample source | M | C185 |  |
| 623 | C206 | T9 | Clips follow session tempo | Stretch clips with tempo changes | upstream-in-progress |  | wait for upstream: SoundCraft PR #785 tempo-follow tracks, SoundCraft issue #171; Audition check 2026-10-28: not found (batch 13) |  | M | C204 |  |
| 624 | C283 | T9 | OpenTimelineIO exchange | Open interchange alternative | upstream-in-progress |  | wait for upstream: SoundCraft issue #725; Audition check 2026-10-28: not found (batch 15) |  | M | C074 |  |
| 625 | C312 | T9 | Scripting / automation API | Automate with scripts | upstream-in-progress |  | wait for upstream: SoundCraft automation crate MCP + CLI; Audition check 2026-10-28, checked in Adobe Audition 26.5: exists, File > Browse and Run Script... (batch 15) | soundcraft-engine pattern (MIT/Apache) | M |  |  |
| 626 | C163 | T9 | Built-in track effects set (EQ, comp, reverb, delay...) | Audition-class built-in effects usable as inserts | upstream-in-progress |  | wait for upstream: WavCraft #34 (F2) from SoundCraft dsp/plugins (dynamics, eq, reverb, modulation, pitch) |  | L | C157 | #34 |
| 627 | C279 | T9 | AAF import/export | Exchange sessions with Avid etc. | upstream-in-progress |  | wait for upstream: SoundCraft PR #799 (external-WAV AAF import), issue #40; Audition check 2026-10-28: not in Audition (batch 14) |  | L | C074 |  |
| 628 | C290 | T9 | Pro Tools session read | Open PTX | upstream-in-progress |  | wait for upstream: SoundCraft issue #528; Audition check 2026-10-28: not in Audition (batch 15) |  | L | C074 |  |
| 629 | C314 | T9 | AI features (generative, Enhance Speech, tagging) | AI-assisted audio | upstream-in-progress |  | wait for upstream: SoundCraft PR #603 Craft AI, #676; Audition check 2026-10-28: differs (batch 15) |  | L |  | #72 |
| 630 | A234 | T9 | Legacy ASND handling | Instruct user to unzip [Audition check 2026-10-28: not in Audition; see logs/2026-10-23/feature-verification/batch-02.md] | missing |  | Adobe-specific; replace or skip |  | S |  |  |
| 631 | A264 | T9 | Export with Adobe Media Encoder (merged: C270) | Multitrack export route; not applicable (no AME) | missing |  | Adobe-specific; replace or skip |  | S | C185 |  |
| 632 | C310 | T9 | Share / upload (Creative Cloud, social) | Share to cloud or social | missing |  | Adobe-specific; replace or skip: SoundCraft PR #774 CollabCraft share; Audition check 2026-10-28: not found (batch 15) |  | M |  |  |
| 633 | C281 | T9 | Premiere Pro round trip (Edit in Audition) (merged: A325) | Send Premiere sequence/clips and send mix back | missing |  | Adobe-specific; replace or skip |  | L | C278 | #72 |
| 634 | C282 | T9 | Dynamic Link | Live link between two applications; not replicable, skip | missing |  | Adobe-specific; replace or skip |  | L |  |  |
| 635 | C311 | T9 | Cloud sync / libraries | Cloud-hosted libraries and sync; out of scope | missing |  | Adobe-specific; replace or skip: Craft family: SoundCraft #173 backup; Audition check 2026-10-28: not found (batch 15) |  | L |  |  |
| 639 | C323 | T9 | DAWproject interchange | Import and export multitrack sessions in the open DAWproject format | missing |  | added 2026-10-20 from reuse research; Audition check 2026-10-28: not in Audition (batch 15) | dawproject 0.11.0 (MIT/Apache; early) | L | C074 |  |

## Merged duplicates

Kept id, then the ids merged into it (same feature found in more than one research pass).

| kept | merged into it |
|---|---|
| A011 Built-in workspaces | C096 |
| A016 Appearance: colours, brightness, gradients, presets | C307 |
| A028 History panel | C316 |
| A036 Metadata panel | C249 |
| A040 Phase Analysis panel | B051, C063 |
| A041 Loudness Radar panel | B091, C066 |
| A052 Marquee selection | B136 |
| A053 Lasso selection | B137 |
| A054 Paintbrush selection | B138 |
| A055 Spot Healing Brush | B079, B139 |
| A060 Spectral frequency display | B132, C319 |
| A061 Show/hide spectral display, drag divider, double-click handl | B146 |
| A063 Spectral resolution inc/dec (Shift+Ctrl+Up/Down) | B133 |
| A066 Spectral colour scheme | B134 |
| A067 Spectral Pitch Display (older versions) | B105, B145 |
| A077 Fade in/out handles with linear, log, cosine curves | B033 |
| A082 High-DPI rendering | C306 |
| A134 Surround 5.1 / up to 32 channel files | C298 |
| A136 Convert channels (mono/stereo/5.1) with left/right mix % | C265, B123, B124 |
| A144 Silence (mute selection) | B115 |
| A145 Insert Silence | B116, A302 |
| A147 Copy to New | B164 |
| A149 Mix Paste | C142 |
| A154 Repeat Last Command (with/without dialog) | A338, B010, C317 |
| A161 Convert Sample Type dialog (Shift+T) | B121, C262, C263 |
| A163 Dither types and noise shaping | B122, C264 |
| A170 Marker and range data model | C315, B049 |
| A172 Markers panel list | A026 |
| A183 Save/load markers in WAV (cue, adtl labl/ltxt, smpl) | C276 |
| A185 Export audio of selected range markers to files | C240, C248 |
| A187 CD track markers | C242 |
| A202 Preroll / postroll | C049 |
| A205 Sample-rate conversion to device | C007 |
| A206 Follow default output device change | C002, C009 |
| A210 ASIO / CoreAudio / WASAPI choice and buffer size | C005, C012, C006 |
| A211 Audio Channel Mapping | A277, C008 |
| A213 Peak meters L/R in dBFS | A031, B129, C053, C058 |
| A214 Dynamic/static peaks, valleys, LED, gradient, ranges | C054, C057, C072 |
| A215 Reset clip indicators | C056, C168 |
| A216 Meter input signal toggle | C059 |
| A217 RMS display | C055 |
| A220 File > Open (WAV, AIFF, FLAC, MP3, OGG, M4A, CAF) | C321 |
| A221 Drag-and-drop open | C318 |
| A230 iXML field-recorder metadata | C289 |
| A231 Open audio of video files (AVI, MOV, MP4, MPEG, 3GP, FLV, WM | A320, C287, C213 |
| A239 Auto Save and crash recovery | C018 |
| A242 Save (Ctrl+S) | C253 |
| A246 Save as AIFF | C255 |
| A247 Save as FLAC | C256 |
| A248 Save as MP3 (constant/variable bitrate) | C257 |
| A250 Save as OGG Vorbis (VBR quality) | C259 |
| A253 WAV options: 4GB+ via RF64 or multi-chunk, 32-bit float vari | C254 |
| A257 Save Selection As | C267 |
| A258 Save All | C268 |
| A260 File > Export > File (keep current open) | C269 |
| A262 Include Markers and Other Metadata option | C266 |
| A263 Progress and cancel during save | C272 |
| A264 Export with Adobe Media Encoder | C270 |
| A273 Preferences dialog (Edit > Preferences) | C014 |
| A276 Audio Hardware | C001 |
| A281 Media & Disk Cache (temp folders, peak files, DLMS) | C019 |
| A283 Multitrack / Multitrack Clips | C016 |
| A284 Playback and Recording (scroll type, JKL speed, auto-scroll) | C017 |
| A289 UI language | C308 |
| A291 Keyboard Shortcuts editor (Edit > Keyboard Shortcuts) | C304 |
| A295 Audition default shortcuts for implemented commands | C303 |
| A303 Generate Tones | B118 |
| A304 Generate Noise | B119 |
| A306 Generate DTMF signals (older versions; not fetched) | B120 |
| A309 Frequency analysis (FFT graph) | A037, B052, C068 |
| A313 Amplitude statistics (peak, RMS, DC, clipped, bit depth, dyn | A038, B053, B131, C070, C071 |
| A317 Phase meter | A039, C062 |
| A321 Video panel preview | A043, C285 |
| A332 Invert | B113 |
| A333 Reverse | B114 |
| A334 Amplify / gain | B025 |
| A335 Normalize | B036 |
| A337 Capture noise print (Shift+P) | B066 |
| A345 Windows, Linux, macOS builds | C300 |
| A346 Third-party notices file | C313 |
| A347 Memory fits an 8 GB Mac | C302 |
| A349 Help menu / About / tutorials | C309 |
| A353 Accessibility (screen reader, high-contrast) | C305 |
| B004 Real-time effect Preview (looped) | A340 |
| B007 Effect presets (save/load/delete per effect) | C173 |
| B008 Favorites menu / user-recorded actions | A167, C232 |
| B013 Effects Rack panel (Waveform editor) | A025 |
| B027 Channel Mixer | A138 |
| B041 Match Volume (gain, selection to ref) | C144 |
| B047 Delete Silence | A158 |
| B048 Mark Audio (Mark Audio Pieces) | A186 |
| B050 Diagnostics panel (scan list, Scan, Repair All, Repair) | A046, C073 |
| B062 Chorus | B065 |
| B072 Automatic Click Remover | B078 |
| B075 Sound Remover | A047 |
| B077 Auto Heal (spectral) | A339, B143 |
| B098 Stereo Expander | B099 |
| B125 Loudness measurement (LUFS/true peak) | A318, C064, C065 |
| B126 Match Loudness panel | A042 |
| B130 Loudness range (LRA) | C067 |
| B148 VST3 plugin hosting | C162 |
| B158 Batch Process panel | A259, C231 |
| B159 Multi-file queue and progress | C233 |
| B160 Batch: apply Favorites / Effects Rack | C234 |
| B161 Batch: Match Loudness | C236 |
| B163 Scripting / macro recording of effect sequences | C238 |
| C010 Latency compensation / record offset | C050 |
| C013 Hardware test / mic check | A341 |
| C020 Record into a new waveform file | A342 |
| C035 Record-ready dropped-sample warning | A330 |
| C060 Per-track meters in Mixer and track headers | C167 |
| C190 Mix down session to new waveform (Bounce to new file) | C212 |
| C195 Remix: fit a music clip to a target duration | C229 |
| C208 Edit clip source in Waveform editor and return | A159, C095, C147 |
| C214 Essential Sound panel | A045 |
| C216 Dialogue: Loudness (Auto-Match) | C145, C227 |
| C281 Premiere Pro round trip (Edit in Audition) | A325 |

Status notes: A060 (spectral display) is kept as "have"; the pass that called it partial was counting the toggle and key controls, which are rows A061 and A063. A154 (Repeat Last Command) stays partial: the have-rows C317 and B010 only cover the edit commands.

## Already have (46)

Evidence is from reading the code at research time; not measured.

| id | feature | evidence |
|---|---|---|
| A028 | History panel (merged: C316) | read: app.rs show_history window, History::go_to; core/doc/history.rs; commit d11c8de |
| A059 | Waveform display per channel | read: core/document.rs waveform_columns, audio-io Peaks; app.rs editor(); tests in core |
| A060 | Spectral frequency display (merged: B132, C319) | read: core/spectrogram.rs, apps spectrum.rs Renderer; View > Spectral frequency display; #16 (B2) |
| A066 | Spectral colour scheme (merged: B134) | read: app.rs palette() and test the_palette_runs_dark_to_light |
| A081 | Selection highlight | read: app.rs editor() selection painter; core/view.rs Selection |
| A083 | Mouse-wheel zoom around pointer | read: app.rs editor() zoom_at on scroll.y; Viewport::zoom_at in core/view.rs; tests there |
| A085 | Zoom in/out horizontally (= and -) | read: core/actions.rs ZoomIn/ZoomOut; tests/actions.rs |
| A086 | Zoom to selection | read: ActionId::ZoomToSelection (Shift+S in WavCraft) |
| A087 | Zoom out full | read: ActionId::ZoomOutFull (Ctrl+\) |
| A097 | Decimal time (mm:ss.ddd) | read: core/view.rs format_time (h:mm:ss.mmm) |
| A106 | Timeline ruler with adaptive tick spacing | read: app.rs draw_ruler, ruler_step test ruler_steps_are_round_and_wide_enough |
| A108 | Click places cursor | read: app.rs editor() response clicked; core/view.rs Selection::at |
| A109 | Drag to select a range | read: app.rs drag handling; Selection anchor/head |
| A116 | Select All | read: ActionId::SelectAll; tests/actions.rs select_all_and_clear_time_selection |
| A117 | Deselect All | read: ActionId::DeselectAll |
| A118 | Clear Time Selection | read: ActionId::ClearTimeSelection |
| A119 | Set In/Out point to playhead (I / O) | read: ActionId::SetIn/SetOut |
| A130 | Zero-crossing selection adjusts (6 commands) | read: ActionId::ZeroInward... in core/actions.rs; tests t6_zero_crossing_adjusts..., zero_crossing_on_stereo_uses_the_active_channels; repo commit 139bb3a |
| A139 | Cut | read: edit.rs cut, Editor Cut; tests t2_cut_and_copy_with_no_selection_take_the_whole_file |
| A140 | Copy | read: edit.rs copy→Clip |
| A141 | Paste (insert at cursor / replace selection) | read: edit.rs paste (PasteAt::Insert/Replace); #60 open question on edge cases |
| A142 | Delete | read: edit.rs delete; tests t9b_up_then_delete... |
| A143 | Crop | read: edit.rs crop; Ctrl+T |
| A144 | Silence (mute selection) (merged: B115) | read: edit.rs silence; Edit > Silence |
| A145 | Insert Silence (merged: B116, A302) | read: edit.rs insert_silence, app.rs Insert Silence dialog; tests t7_repeat_last... |
| A155 | Undo / Redo with named steps | read: Editor/History; tests t8_undo_and_redo_labels_name_the_step |
| A192 | Play / Stop (Space) | read: app.rs toggle_play; player.rs Player::play; test plays_the_range_then_stops_with_silence |
| A193 | Play selection only | read: Player::play(from,to) with selection |
| A207 | Playback via cpal stream with error recovery | read: player.rs reopen, failed flag |
| A209 | Channel downmix for mono device | read: player.rs (code-map.md Playback) |
| A220 | File > Open (WAV, AIFF, FLAC, MP3, OGG, M4A, CAF) (merged: C321) | read: app.rs pick_file filter; core/io.rs open(); tests crates/testkit/tests/open.rs |
| A222 | Large files (hours, > 4 GB RF64) | read: core/store.rs, io.rs; wavcraft-store-bench; README; huge test file via testkit --huge |
| A238 | Unsaved changes prompt on close/quit | read: app.rs line ~620 Unsaved changes window; control.rs QUIT discard |
| A240 | Session folder (disk store) for working files | read: core/settings.rs, File > Session Folder; README |
| A241 | Free-space check before open | read: core/space.rs; tests crates/testkit/tests/space.rs |
| A242 | Save (Ctrl+S) (merged: C253) | read: app.rs save(); core/save.rs save_wav; tests/save.rs t1..t5 |
| A244 | Atomic save, no temp file on failure | read: core/save.rs temp_path; test t5_a_save_that_fails_halfway... |
| A296 | Shortcuts shown in menus | read: Shortcut::text and shortcut_text in app.rs |
| A299 | History disk cap | read: core/doc/history.rs cap, tests/edits.rs |
| A301 | History cleared on close | read: History per Document |
| A307 | Streamed replace/insert producer for generators | read: edit.rs replace_range (producer fills chunks; cancel flag) |
| A331 | Open warnings shown | read: app.rs status (W8b) |
| A343 | Local control channel for scripted tests | read: apps/wavcraft/src/control.rs |
| A344 | Self-logging/diagnostics | read: apps/wavcraft/src/selflog.rs, crates/craft-log |
| A352 | Undo covers edits only, not view | read: History pushes edits; view not in History |
| C320 | Waveform playback with playhead | apps/wavcraft/src/player.rs; README Space/Home/End |
