# Roadmap

Updated: 2026-11-11

features:
- files: Open, save, convert and manage audio files (A)
- display: Waveform, spectral display, zoom and the Selection/View panel (B1-B3, B5)
- analysis: Level meters and loudness analysis (B4, G4)
- selection: Time, channel and spectral selection, and snapping (C1-C4)
- transport: Playback transport and scrub (C5, C6)
- editing: Cut, copy, paste, silence, crop, undo and crossfades (D)
- effects: One-shot effects, the Effects Rack and presets (E, F)
- repair: Noise print, Noise Reduction and Match Loudness (G1-G3)
- markers: Markers and the Markers panel (H)
- shell: Layout, panels, workspaces and shortcuts (I1-I3)
- platform: Autosave, the three operating systems, notices, memory, CI and logging (I4-I7)
- recording: Record a take (J)
- episode-check: Episode Check: findings, fixes and ready to publish (K)
- transcript: Local transcription and text-based editing (L)
- faces: Studio and Podcast faces, the Episode Map and the theme (M)
- automation: Automation core, command line, MCP server and scripts (N)
- multitrack: Multitrack sessions, mixer and CD layout (after 1.0)
- export-batch: Batch processing and extra export formats (after 1.0)
- plugins: Third-party plugin hosting (after 1.0)
- video: Video import and preview (after 1.0)

The plan from here to 1.0, rewritten 2026-11-11 from an audit of `main` at `11e49a5` and all 744 issues. The spec is [SPEC.md](SPEC.md) (the items of [docs/v1/v1.md](docs/v1/v1.md), sections A to N); the code map is [docs/code-map.md](docs/code-map.md); the stage is in [PIPELINE.md](PIPELINE.md). The old fleet tracker is kept as history in [docs/archive/roadmap-v1-fleet.md](docs/archive/roadmap-v1-fleet.md).

## How to read it

- **complete**: the code is on `main`, its tests pass, and the item's Done check is recorded where the row says.
- **built, check not recorded**: the code is on `main`, but part of the Done check (a measurement, a test or a by-hand check) is not written down yet. The steps say which part.
- **partial**: part of the item is on `main`; the steps build the rest.
- **not started**: nothing of the item is on `main`.
- A **step** is one change with one check. Its id is `<item>.<n>` (for example `C5.3`) and is the start of its issue's title. **Verify** names the test, measurement or by-hand check that proves it. A number dropped during the audit is not reused, so H1.2, J3.4 and F3.1 do not exist.
- **By**: `script` means an automated test or tool proves it; `hand` means someone runs the app; `hand, me` means only I can do it (by ear, or on a machine only I have). An operating system after it means the step needs that system.
- Sizes (`easy`, `medium`, `hard`) and kinds follow the pipeline's rubric. Each step gets one issue on the `1.0` milestone with one `feature:` label from the block above; the Issue column is filled when it is filed.

## Where v1 stands

**75 items** (v1.md's 74 plus L0, the speech-to-text choice)**:** complete 14 · built, check not recorded 14 · partial 32 · not started 15.

**333 steps remain:** 242 easy, 86 medium, 5 hard. 262 are proved by a script, 71 by hand (44 of those only by me).

| Stage | Items | Complete | Steps left | Done when |
|---|---|---|---|---|
| Stage 0: merge plan | - | done 2026-10-27 | 0 | the old branches are merged into `main` and CI exists |
| Stage 1: Editor | 19 | 9 | 32 | a full rough-cut edit of an episode is saved, reopened and identical. |
| Stage 1b: Automation | 4 | 0 | 28 | an AI agent completes 10 real editing tasks through the MCP server with no window. |
| Stage 2: Recording | 7 | 0 | 27 | I record a real 10-minute take on each machine, kill the app once mid-take, and get everything back. |
| Stage 3: Repair | 14 | 3 | 49 | I clean a real episode end to end and prefer the result to my current tool's, by ear. |
| Stage 4: Feel | 12 | 0 | 55 | I work without hunting for things. |
| Stage 4b: Episode Check, transcript and faces | 16 | 1 | 124 | the Podcast face's user cleans a real episode in it without help. |
| Alongside: the MP3 encoder | 1 | 0 | 12 | a 1-hour episode exports to MP3 and passes A3's Done check. |
| Stage 5: Platform checks | 2 | 1 | 6 | the app is built, launched and checked by hand on Windows, Linux and the 8 GB Mac, and every hand step in the earlier stages that names a machine is recorded. |
| Stage 6: Design pass | - | not started | filed when it runs | every `design-pass` issue is closed or moved after 1.0 |

## Status of every v1 item

| # | Item | Feature | Stage | Status | Steps left | Open issues |
|---|---|---|---|---|---|---|
| A1 | Open WAV, AIFF, MP3, FLAC into the store | files | 1 | partial | 3 | #395 |
| A2 | Save and Save As (WAV, AIFF, FLAC) | files | 1 | complete | 0 | #10 |
| A3 | Save As MP3 with the craft-mp3 encoder | files | mp3 | not started | 12 | #11 |
| A4 | Convert Sample Type and dither | files | 3 | partial | 5 | #12 |
| A5 | Files panel with several files open | files | 4 | built, check not recorded | 2 | #13 |
| A6 | Markers survive a save | files | 1 | built, check not recorded | 1 | #14, #62 |
| B1 | Waveform, amplitude ruler, vertical zoom | display | 4 | built, check not recorded | 5 | #15, #69 |
| B2 | Spectral Frequency Display toggle, keys, refresh | display | 4 | partial | 8 | #16, #68 |
| B3 | Zoom keys and the navigator bar | display | 4 | built, check not recorded | 3 | #17 |
| B4 | Levels meter: peak, RMS, hold, clip light | analysis | 4 | partial | 8 | #18 |
| B5 | Selection/View panel and time formats | display | 4 | built, check not recorded | 1 | #19 |
| C1 | Time Selection tool gestures | selection | 1 | complete | 0 | #20, #69 |
| C2 | One channel of a stereo file | selection | 1 | partial | 1 | #21 |
| C3 | Snapping: markers, ruler, zero crossings, key S | selection | 1 | partial | 2 | #22 |
| C4 | Spectral selection: Marquee and Lasso | selection | 4 | partial | 8 | #23 |
| C5 | Transport | transport | 1 | partial | 10 | #24, #67, #1081, #69 |
| C6 | Scrub by dragging the playhead | transport | 4 | built, check not recorded | 2 | #25 |
| D1 | Cut, Copy, Paste, Delete | editing | 1 | complete | 0 | none |
| D2 | Copy to New and Paste to New | editing | 1 | complete | 0 | #26 |
| D3 | Silence and Insert Silence | editing | 1 | complete | 0 | none |
| D4 | Crop to selection (Ctrl+T) | editing | 1 | complete | 0 | none |
| D5 | Undo, Redo and the History panel | editing | 1 | built, check not recorded | 2 | #63 |
| D6 | Zero-crossing adjust of selection edges | editing | 1 | complete | 0 | #61 |
| D7 | Edit crossfades (2 ms and 5 ms) | editing | 1 | complete | 0 | none |
| E1 | Fade In and Fade Out handles | effects | 3 | partial | 4 | #27 |
| E2 | Amplify | effects | 3 | complete | 0 | none |
| E3 | Normalize | effects | 3 | complete | 0 | none |
| E4 | Invert and Reverse | effects | 3 | complete | 0 | none |
| E5 | Repeat Last Command | effects | 3 | partial | 3 | #31 |
| E6 | Preview in every effect dialog | effects | 3 | partial | 7 | #32 |
| F1 | Effects Rack panel with Apply | effects | 3 | partial | 7 | #33 |
| F2 | Built-in processors from SoundCraft dsp | effects | 3 | partial | 2 | #34 |
| F3 | Presets for effects and racks | effects | 3 | partial | 2 | #35 |
| G1 | Capture Noise Print | repair | 3 | partial | 2 | #36 |
| G2 | Noise Reduction (process) | repair | 3 | partial | 11 | #37 |
| G3 | Match Loudness panel | repair | 3 | partial | 5 | #38 |
| G4 | Loudness analysis | analysis | 3 | built, check not recorded | 1 | #39, #927 |
| H1 | Add Marker with M or * | markers | 1 | partial | 2 | #40 |
| H2 | Markers panel | markers | 1 | partial | 6 | #41 |
| H3 | Next and previous marker | markers | 1 | partial | 4 | #42 |
| H4 | Markers move with edits | markers | 1 | complete | 0 | #43 |
| I1 | Audition's default layout | shell | 4 | partial | 4 | #44 |
| I2 | Panels show, hide, resize; layout remembered | shell | 4 | partial | 4 | #45 |
| I3 | Audition's default shortcuts and Help list | shell | 4 | partial | 6 | #46 |
| I4 | Never lose work: autosave and recovery | platform | 4 | built, check not recorded | 4 | #47 |
| I5 | Runs on Windows, Linux and macOS | platform | 5 | built, check not recorded | 6 | #48 |
| I6 | Third-party notices under Help, About | platform | 5 | complete | 0 | #49 |
| I7 | Memory fits an 8 GB Mac | platform | 1 | built, check not recorded | 1 | #50 |
| J1 | Record a take into a new WAV | recording | 2 | partial | 7 | #51 |
| J2 | No playback while recording | recording | 2 | partial | 3 | #52 |
| J3 | A crash keeps the take | recording | 2 | built, check not recorded | 3 | #53 |
| J4 | Readouts and level meter while recording | recording | 2 | partial | 5 | #54 |
| J5 | Input device preference and clean unplug | recording | 2 | built, check not recorded | 1 | #55 |
| J6 | Recording works on every machine | recording | 2 | partial | 7 | #56 |
| J7 | After Stop the take opens in the editor | recording | 2 | built, check not recorded | 1 | none |
| K1 | Run Episode Check with progress and Cancel | episode-check | 4b | not started | 9 | #1049 |
| K2 | Episode Check detectors (seven kinds) | episode-check | 4b | partial | 5 | none |
| K3 | Findings panel, map pins, next and previous keys | episode-check | 4b | not started | 8 | #1053 |
| K4 | Suggested fix per finding, with Before/After | episode-check | 4b | not started | 10 | #1054 |
| K5 | Ready to publish summary | episode-check | 4b | not started | 5 | #1055 |
| L0 | Speech-to-text library, model and licence | transcript | 4b | complete | 0 | #1056 |
| L1 | Transcribe the open file, saved with it | transcript | 4b | not started | 17 | #1057 |
| L2 | Transcript panel and lane | transcript | 4b | not started | 9 | #1058 |
| L3 | Select text to select and cut audio | transcript | 4b | not started | 5 | #1059 |
| L4 | Filler and pause finder | transcript | 4b | not started | 7 | #1060 |
| L5 | Transcript search, text and SRT export | transcript | 4b | not started | 7 | #1061 |
| M1 | Studio and Podcast face switch | faces | 4b | not started | 12 | #1063 |
| M2 | Episode Map replaces the zoom navigator | faces | 4b | not started | 8 | #1052 |
| M3 | One theme from named design values | faces | 4b | partial | 10 | none |
| M4 | Large controls preference, 44 px minimum | faces | 4b | partial | 4 | #1062 |
| M5 | Out-of-v1 panels hidden in release | faces | 4b | partial | 8 | #1051 |
| N1 | Automation core: one Backend, two ways | automation | 1b | partial | 8 | #1107 |
| N2 | Command line: run, convert, inspect | automation | 1b | not started | 5 | #845 |
| N3 | MCP server (wavcraft mcp) | automation | 1b | not started | 7 | #1108 |
| N4 | Scripts: a text file of commands | automation | 1b | not started | 8 | #879 |

## Stages

### Stage 0: the merge plan (done 2026-10-27)

The old branches went into `main` as 44 merge commits; the unmerged ones are kept as `archive/` tags. Its follow-ups are closed: `main` passes its own checks (#1012, #1010) and CI runs on every push and pull request (#1013). The rulings I made then still decide behaviour:

1. Save with metadata off drops markers and the INFO/BWF metadata, as Audition does.
2. Match Loudness (G3) has one implementation, `crates/core/src/match_loudness.rs`.
3. Dither (A4) is off by default, Triangular first, Noise Shaping None, remembered when on; an unedited 16-bit save is bit-identical.
4. Playback uses the read-ahead ring with the windowed-sinc resampler; the Dynamics effect already on `main` (`crates/core/src/effects/dynamics.rs`) stays.

### Stage 1: Editor

The store, the edit model, selection, transport and markers. I7's Mac measurement is taken in stage 5. D2 and D6 are not in v1.md's order of work; I place them here with the rest of section D.

**Done when** a full rough-cut edit of an episode is saved, reopened and identical.

**Complete:**

- **A2 Save and Save As (WAV, AIFF, FLAC).** crates/core/src/save.rs and atomic.rs write WAV, AIFF and FLAC to a temporary file and swap it in. The window has Save, Save As, the title star and the close prompt in apps/wavcraft/src/app.rs. Tests: crates/core/tests/save.rs, atomic.rs, apps/wavcraft/src/issue369_tests.rs. Merged 2026-11-12: A2.1 (#1165), A2.2 (#1182). Done check: Unedited save is bit-identical for 16/24-bit and float WAV and AIFF (tested). A failed save leaves the original whole. Title shows * when unsaved. Closing unsaved asks first. Recorded: crates/core/tests/save.rs: t1_round_trip_is_bit_identical_for_each_sample_type, f35_aiff_round_trips_each_sample_type, t5_a_save_that_fails_halfway_leaves_the_target_whole. Title star: apps/wavcraft/src/a2_title_star_tests.rs window_title_marks_unsaved and the_title_sent_to_the_window_follows_edit_and_save (A2.1, #1165). Close prompt: apps/wavcraft/src/a2_close_prompt_tests.rs closing_with_unsaved_changes_asks_first and a_clean_document_closes_without_a_prompt (A2.2, #1182).
- **C1 Time Selection tool gestures.** All gestures and Edit > Select (Select All, Deselect All) are on main: crates/core/src/view.rs (Selection), apps/wavcraft/src/app.rs (pointer wiring, GESTURE control verb), crates/core/src/actions.rs. Tests: view.rs unit tests per gesture, crates/core/tests/actions.rs select_all_and_clear_time_selection. Merged 2026-11-12: C1.1 (#1194). Done check: A test on each gesture, and positions in a 3-hour file exact to the sample. Recorded: Gesture half: unit tests in crates/core/src/view.rs. 3-hour half: crates/core/tests/c1_selection_three_hours.rs gestures_are_exact_at_three_hours (C1.1, #1194).
- **D1 Cut, Copy, Paste, Delete.** crates/core/src/doc/edit.rs has delete, copy and paste (insert at the cursor or replace the selection). Actions and keys are in crates/core/src/actions.rs and shortcuts.rs. Tests: crates/core/tests/edits.rs, clipboards.rs, crates/testkit/tests/edits.rs. Bench: crates/testkit/src/bin/wavcraft-edit-bench. Done check: Cutting an hour from the middle of a 3-hour file takes under 50 ms and uses no extra sample memory (measured). Paste goes at the cursor or over the selection. Recorded: docs/baseline.md, Task 04: cut of 1 hour 0.34 to 0.36 ms with nothing written (0.94 to 0.99 ms and 768 bytes with the 2 ms join); paste back 0.55 to 0.67 ms.
- **D2 Copy to New and Paste to New.** Actions edit.copy_to_new and edit.paste_to_new are in crates/core/src/actions.rs. crates/core/src/doc/newdoc.rs builds the document. apps/wavcraft/src/app.rs copy_to_new, paste_to_new, finish_making and add_document add it as an untitled row. Tests: crates/core/tests/newdoc.rs (core only). Merged 2026-11-12: D2.1 (#1188). Done check: Each command makes a new untitled file in the Files panel. Recorded: apps/wavcraft/src/d2_new_from_clip_tests.rs copy_and_paste_to_new_add_untitled_rows (D2.1, #1188).
- **D3 Silence and Insert Silence.** crates/core/src/doc/edit.rs has silence and insert_silence. crates/core/src/actions.rs has the Silence action and the Insert Silence dialog action. Tests: crates/core/tests/edits.rs (silence_writes_nothing_and_is_exact_zeros and the model tests), crates/core/tests/actions.rs. Done check: Bit-exact zeros. Length unchanged for Silence, exactly longer for Insert Silence. Recorded: crates/core/tests/edits.rs silence_writes_nothing_and_is_exact_zeros (every sample has bits 0, length 150,000 after inserting 100,000 into 50,000).
- **D4 Crop to selection (Ctrl+T).** crates/core/src/doc/edit.rs has crop. The Crop action is in crates/core/src/actions.rs with Ctrl+T in shortcuts.rs. Tests: crates/core/tests/edits.rs (each_edit_at_every_kind_of_place_matches_the_model, crop_of_an_empty_range_changes_nothing), crates/core/tests/edit_span.rs. Done check: Only the selection remains. Recorded: crates/core/tests/edits.rs each_edit_at_every_kind_of_place_matches_the_model (Crop compared bit for bit with the plain-vector model).
- **D6 Zero-crossing adjust of selection edges.** crates/core/src/actions.rs has zero_crossing, a 10 ms reach and six actions (inward, outward and four single-edge moves). Keys Shift+I, O, H, J, K, L are in shortcuts.rs. Tests: crates/core/tests/actions.rs, crates/core/tests/snap.rs. Done check: Edges land on the nearest sign change within 10 ms. Recorded: crates/core/tests/actions.rs t6_zero_crossing_adjusts_land_on_sign_changes_of_the_1khz_tone and zero_crossing_finds_none_beyond_10_ms_and_never_turns_the_selection_inside_out (480 frames at 48 kHz). Open issue to close: #61.
- **D7 Edit crossfades (2 ms and 5 ms).** Crossfades (cut_join, edge) are in crates/core/src/doc/edit.rs with defaults 2 and 5 ms in settings.rs. Preferences > Data in apps/wavcraft/src/app.rs sets both; 0 turns one off. Tests: crates/core/tests/edits.rs (default_crossfades_leave_no_click_and_have_their_lengths, crossfaded_delete_join_has_no_click). Done check: A cut, paste, silence and effect inside a full-scale 1 kHz sine leave no jump bigger than the sine's own (measured). With both at 0, splices are exact. Recorded: crates/core/tests/edits.rs default_crossfades_leave_no_click_and_have_their_lengths (fades of 96 and 240 frames, worst step at most 1.035 times the sine's own) and the bit-exact model tests with crossfades at 0.
- **H4 Markers move with edits.** crates/core/src/markers.rs (Shift, Markers::moved_by) applied by the edit and history code. Tests: markers.rs moved_by_covers_each_kind_of_edit; crates/core/tests/actions.rs m3 to m8; crates/core/tests/save.rs; crates/core/tests/990_autosave_markers.rs. Done check: Tests on each case: a cut before a marker moves it left; a marker inside a cut is deleted and returns on undo. Recorded: crates/core/tests/actions.rs m3_a_marker_inside_a_deleted_range_goes_and_undo_brings_it_back and m4_delete_and_cut_shift_trim_and_remove_markers; they run on Linux, Windows and macOS in .github/workflows/check.yml. Open issue to close: #43.

**Remains:**

#### A1: Open WAV, AIFF, MP3, FLAC into the store (partial)

- **On main:** Streamed open for WAV (all depths, BWF, RF64), AIFF, FLAC and MP3 is in crates/core/src/io.rs and store.rs. The core open already stops on a cancel flag and removes its session folder (test a_cancelled_open_leaves_nothing_behind in crates/testkit/tests/open.rs). Other tests: crates/testkit/tests/open.rs, crates/core/tests/store_and_doc.rs, multichannel.rs. MP3 is tested only with hand-built silent frames. The window opens on a worker thread in apps/wavcraft/src/app.rs. The status bar shows a spinner and a Cancel control while opening; the control channel has no cancel line for an open yet (A1.6). Merged 2026-11-12: A1.1 WaveApp::cancel_opening with its window test (#1164); A1.3 the refusal of more than 32 channels in io.rs (#1162); A1.4 a real MP3 fixture and its open test (#1191).
- **Done check:** Each format matches a reference tool. A 3-hour 48 kHz stereo WAV opens under 10 s on the Windows build machine, adding at most 300 MB. No length limit. Open can be cancelled.
- **Recorded so far:** docs/baseline.md, Task 03 (1.75 to 1.92 s, 5.3 MB above rest, 4 h 10 min RF64 opens). Cancel in the window and an outside reference tool are not recorded.
- **Item issue:** #395

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| A1.2 | Draw a Cancel button beside the opening spinner in the status bar. A click calls cancel_opening. | Same test file, test the_status_bar_offers_cancel_while_opening: the opening branch of the status bar source holds a Cancel button and the frame calls self.cancel_opening(). Then, once by hand and not part of the script: make the kit with wavcraft-testkit <kit> --long, open long-3h-48k-stereo-16.wav, click Cancel in the status bar within 1 second. Expected: the spinner goes, the status bar reads "Open cancelled", the Files panel has no new row, and the sessions folder has no new folder. | easy | ui | script | A1.1 | #1151 |
| A1.5 | Record the reference-tool comparison: run ffprobe on the kit WAV, AIFF, FLAC, an RF64, a BWF and the MP3 fixture, and write the table into docs/checklists/A1.md. | docs/checklists/A1.md has one row per file with ffprobe's sample rate, channel count and duration in samples beside WavCraft's values. Files: from wavcraft-testkit <kit> --long --huge, the tone WAV, AIFF and FLAC (48000, 2, 480000 each), pink-48k-mono-24.wav (48000, 1, 2880000), lr-different-48k-stereo-float.wav (48000, 2, 1440000) and the RF64 file (48000, 2, 720000000); plus a 32-bit integer WAV and a BWF WAV written by ffmpeg, and the MP3 fixture. WavCraft's values for the kit files are the ones crates/testkit/src/lib.rs expects; for the others I read them from the Files panel. Every pair is equal; the MP3 row may differ by encoder padding, and the row says by how many samples. | easy | docs | script | A1.4, ffprobe installed | #1169 |
| A1.6 | Add a control line OPEN_CANCEL that calls cancel_opening: it answers OK when a file was opening and ERR nothing is opening otherwise. Document it in the control.rs header beside SAVE_CANCEL. | apps/wavcraft/src/a1_open_cancel_tests.rs, test open_cancel_over_the_control_channel: headless window, start opening a WAV, send OPEN_CANCEL. Asserts the answer is "OK", opening is None, file_rows() is empty and STATE has busy=0. A second OPEN_CANCEL answers a line starting with "ERR". | easy | wiring | script | A1.1 | #1153 |

#### A6: Markers survive a save (built, check not recorded)

- **On main:** crates/core/src/markers.rs writes and reads WAV cue and LIST/adtl chunks and AIFF MARK. io.rs reads them on open and save.rs writes them. Tests: crates/core/tests/save.rs (m1, m2, m3, f100), crates/core/tests/990_autosave_markers.rs. Merged 2026-11-12: A6.1 (#1166).
- **Done check:** Save with 5 markers, reopen, all 5 are back at the same sample with the same names. Audition reads them back too, checked on one file.
- **Recorded so far:** crates/core/tests/save.rs m1_markers_round_trip_through_save_and_reopen_for_every_sample_type uses 3 markers, not 5. The Audition check is not recorded (docs/v1/audition-behaviour.md line 218 says still open).
- **Item issue:** #14, #62

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| A6.2 | Hand check on the Windows build machine with Audition: save a five-marker WAV from WavCraft and open it in Audition. Also open an Audition-saved file with markers in WavCraft. | In WavCraft: a 10 s 48 kHz file, markers named One to Five at 1 s, 2 s, 3 s to 3.5 s (a range), 4 s and 5 s, saved as WAV. In Audition: open it; the Markers panel must list the 5 names at 0:01.000, 0:02.000, 0:03.000 (duration 0:00.500), 0:04.000 and 0:05.000. Then in Audition add 2 markers to a file, save it with Include Markers and Other Metadata ticked, and open it in WavCraft: the Markers panel lists both names at the same times. I write what I saw, the Audition version and the date into docs/checklists/A6.md, and replace item 7 under "Things to check in Audition itself" in docs/v1/audition-behaviour.md. | easy | docs | hand, me, windows | A6.1, a machine with Audition installed |  |

#### D5: Undo, Redo and the History panel (built, check not recorded)

- **On main:** crates/core/src/doc/history.rs holds steps on disk with a disk cap. Undo and Redo labels name the step (actions.rs). The History panel in apps/wavcraft/src/app.rs has click to go, Up and Down, Clear History and a line that says how many old steps were removed. That line shows only while the History panel is open; nothing is said in the status bar when a step drops. Tests: crates/core/tests/edits.rs (disk_cap_drops_the_oldest_steps_and_their_packs), actions.rs (t8_undo_and_redo_labels_name_the_step), history_restore.rs. Merged 2026-11-12: D5.2 (#1187).
- **Done check:** 200 steps on a 3-hour file, undo all, bit-identical (a test). Undo of a cut under 50 ms. History capped by disk; oldest steps drop with a notice.
- **Recorded so far:** docs/baseline.md, Task 04 (undo all 200 bit-identical, undo of the cut 0.58 to 0.60 ms), run on 2026-10-15. The test long_file_200_random_edits_then_undo_all is ignored and has no recorded run on current main.
- **Item issue:** #63

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| D5.1 | Run the ignored 200-edit test on current main on the Windows build machine with the 3-hour kit file and append the dated result to docs/baseline.md, Task 04. | Make the kit with wavcraft-testkit <kit> --long (about 2 GB). With WAVCRAFT_KIT set to that folder: cargo test --release -p wavcraft-testkit --test edits -- --ignored --nocapture. long_file_200_random_edits_then_undo_all passes and prints "dropped by the cap: 0". I append the date, the commit, the undo-all time and the slowest undo to Task 04 in docs/baseline.md. | easy | docs | script | none |  |
| D5.3 | Each frame, compare the current document's dropped_total with the last seen value and put drop_notice's text in the status-bar notice. Reset the seen value when the document changes. | Same file, test a_drop_by_the_cap_reaches_the_status_bar: headless window, a new mono document of 262,144 frames, editor.history.set_cap_disk_bytes(1 << 20), then push three edit::replace_range edits over the whole file (each writes 1 MiB) so dropped_total() is at least 1. Run the new per-frame check once: the notice starts with "Oldest " and contains "removed to stay under". Run it again with no new drop: the notice is not set again. Switch to another document: the seen count resets and no notice appears. | easy | wiring | script | D5.2 | #1160 |

#### I7: Memory fits an 8 GB Mac (built, check not recorded)

- **On main:** All audio lives in the on-disk store with a bounded block cache (crates/core/src/store.rs). Undo steps cost disk and are capped (crates/core/src/doc/history.rs). Views read from the store. Tests: crates/core/tests/edits.rs, store_and_doc.rs, incremental_overviews.rs, crates/testkit/tests/space.rs. Script: tools/alpha/f05-mem.py. Merged 2026-11-12: I7.1 (#1163).
- **Done check:** 3-hour stereo file, 20 edits, history kept: memory under 1.5 GB, measured on the Windows build machine then the 8 GB Mac. History capped by disk; old steps drop with a notice.
- **Recorded so far:** docs/baseline.md, Task 04, has the Windows core figure (0.633 GB working set after 200 edits, block cache 512 MB). The M1 Mac mini figures are only in comments on #50, #1037 and #1045, not in the repo. The "old steps drop with a notice" part is the same gap as D5 and is closed by D5.2 and D5.3.
- **Item issue:** #50

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I7.2 | Change the Done text of the I7 row in docs/v1/v1.md to say which number is judged on the Mac (physical footprint from vmmap, as tools/alpha/f05-mem.py reports) and which on Windows (working set), and link the baseline section. | The I7 row in docs/v1/v1.md names physical footprint for the Mac and working set for Windows, keeps the 1.5 GB limit, and links to the "I7 on the 8 GB Mac" section of docs/baseline.md. git diff shows only that row changed. | easy | docs | script | I7.1, my yes to changing the wording of the Done row |  |

#### C2: One channel of a stereo file (partial)

- **On main:** Up/Down keys, Edit > Enable Channels, amplitude-ruler channel buttons, drag-position channel pick and one-channel Delete (silences, keeps length) are on main: crates/core/src/actions.rs, shortcuts.rs, view.rs, apps/wavcraft/src/app.rs. Tests: crates/core/tests/actions.rs t4, t9b, f42, f68; crates/testkit/tests/edits.rs. Merged 2026-11-12: C2.1 (#1192) and C2.2, the left-only Amplify test (#1198).
- **Done check:** Delete or Amplify on a left-only selection leaves the right channel bit-identical, and Delete leaves the length unchanged.
- **Recorded so far:** Delete half: crates/core/tests/actions.rs t9b_up_then_delete_silences_the_left_range_and_keeps_the_length. Amplify half is not recorded: the only mask test is right-only and compares within 1e-6.
- **Item issue:** #21

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| C2.3 | Keep the alpha script for the channel drag working after the default changes. Send PREF set context_channel_editing true before the channel drags in tools/alpha/f42.py. | python3 tools/alpha/f42.py against a debug build: the check that a drag in the top strip replies selch=L passes, and the whole script exits 0. With the PREF line removed and the new default, the same check fails. | easy | wiring | script | C2.1 |  |

#### C3: Snapping: markers, ruler, zero crossings, key S (partial)

- **On main:** crates/core/src/snap.rs (snap_frame, six switches), key S in shortcuts.rs, View > Snapping submenu and ruler context menu (apps/wavcraft/src/app.rs, menus.rs); clicks and selection drags snap. Tests: crates/core/tests/snap.rs. The playhead drag along the ruler snaps too (apps/wavcraft/src/c3_playhead_snap_tests.rs). Missing: toolbar toggle button. Merged 2026-11-12: C3.3, the Edit > Snapping submenu sharing snap_items with the ruler menu (#1200); C3.4, the playhead drag snap (#1201).
- **Done check:** Edges snap to within 0 samples of a marker when snapping is on.
- **Recorded so far:** crates/core/tests/snap.rs frames_snap_to_point_markers_and_range_edges_when_asked (7_000 snaps to 7_300, 11_500 to 12_000, 16_600 to 16_000).
- **Item issue:** #22

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| C3.1 | Add the Snapping toggle to the toolbar table, with the short text "Snap". | apps/wavcraft/src/menus.rs tests, new fn the_toolbar_has_the_snapping_toggle: toolbar_item("view.snap.toggle").map(\|i\| i.cmd) == Some(Cmd::Action(ActionId::SnapToggle)) and glyph(&item) == "Snap". The existing toolbar_has_the_common_commands_and_unique_names still passes. | easy | ui | script | none |  |
| C3.2 | Show the toolbar Snap button as pressed while snapping is on. Pull the pressed rule into a small function used by the toolbar drawing. | apps/wavcraft/src/app.rs tests, new fn the_snap_button_is_pressed_while_snapping_is_on: toolbar_item_on(snap_item, playing=false, snap_on=true) == true; (snap_item, false, false) == false; (play_item, true, false) == true. Control line TOOLBAR run view.snap.toggle replies OK. | easy | ui | script | C3.1 |  |

#### C5: Transport (partial)

- **On main:** Space play/stop, play from cursor or selection, Loop Playback, Home/End, marker jumps, Return on Stop (Shift+X) and J/K/L shuttle are on main: crates/core/src/transport.rs, apps/wavcraft/src/app.rs, player.rs. Tests: transport.rs and player.rs unit tests. Missing: the move to a selection edge. Merged 2026-11-12: C5.1, the pure playhead_stop rule in transport.rs (#1199); the window actions are C5.2.
- **Done check:** With Shift+X on, Space twice returns to where I started. Loop plays the selection with no gap at the seam, by ear.
- **Recorded so far:** not recorded. Only the pure rule is tested (transport.rs stop_returns_the_cursor_to_the_start_unless_turned_off). No by-ear loop result is written anywhere.
- **Item issue:** #24, #67, #1081, #69

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| C5.2 | Add window actions "Move Playhead to Previous" and "Move Playhead to Next" to the action table, bound to Alt+Left and Alt+Right. | crates/core/tests/shortcuts.rs: a new test finds Alt+ArrowLeft and Alt+ArrowRight on the two new actions; every_row_of_the_table_has_a_test_above and the_default_table_has_no_chord_on_two_commands pass. every_action_in_the_table_has_its_menu_text_in_english (crates/core/src/i18n.rs) passes. i3_help_rows_are_in_menu_order_with_no_unbound_or_extra_rows passes with the two new rows in its list. cargo test --workspace passes. | medium | data-shape | script | my leave to edit the locked I3 test file |  |
| C5.3 | Run the two actions in the window: seek while playing or paused; when stopped, move the drawn playhead and keep the selection. Unknown: a stopped playhead exists only as head_scrub.pos. | New apps/wavcraft/src/c5_playhead_move_tests.rs, fn alt_right_moves_the_playhead_to_the_selection_end_and_keeps_the_selection: 10_000 frames, marker 1_000, selection 4_000..6_000, stopped. PlayheadNext: playhead 6_000, selection still 4_000..6_000. PlayheadPrev twice from there: 4_000, then 1_000. | medium | wiring | script | C5.1, C5.2 |  |
| C5.4 | Correct the key in the v1.md C5 row from Ctrl+Left/Right to Alt+Left/Right, citing the Audition check already recorded in audition-behaviour.md. Needs my approval, since v1.md is the approved scope. | grep -n "Alt+Left" docs/v1/v1.md prints the C5 row, and the row no longer contains "Ctrl+Left/Right". It matches docs/v1/audition-behaviour.md line 20. | easy | docs | script | C5.2, my approval of the key change |  |
| C5.5 | Add a window-level test of the Done sentence: Space, then Space, with Return on Stop on and then off. | New apps/wavcraft/src/c5_space_return_tests.rs on the headless window from issue377_tests.rs, with WAVCRAFT_NO_AUDIO=1. Send the control line for the Space key through control_request, so the key table is in the path. With Return on Stop on: Space starts playback, Space again stops it and the playhead is back at the start frame. With Return on Stop off: the second Space leaves the playhead where it stopped. The Space ruling in docs/v1/audition-behaviour.md line 49 holds in both cases. | easy | state | script | none | #1176 |
| C5.6 | Issue 1081: Space plays when a button, checkbox or slider has focus and fires no click. In the main window the app takes its keys at the top of the frame (app.rs line 11902), before any widget is built, and egui 0.36 reads a focused widget's Space at widget creation. So start with a failing-or-passing test in the main window. Fix only what the test shows broken. The open case is a floated panel in its own window. | apps/wavcraft/src/app.rs tests, new fn space_on_a_focused_button_plays_and_does_not_click: a focused button plus a Space press gives vec![Command::PlayStop] and clicked() false. space_in_a_focused_text_field_does_not_toggle_playback still passes. | medium | ui | script | none | #1177 |
| C5.7 | By-ear loop check on the Windows build machine: loop a 2 s selection of a 1 kHz tone (0.5 s to 2.5 s) and of speech, 20 passes each. | Hand check, Windows: Loop on, Space. I must hear no click and no gap at the seam on either selection. I write the date, commit, output device and what I heard into docs/checklists/C5.md under "Windows". | easy | docs | hand, me, windows | none |  |
| C5.8 | The same by-ear loop check on a Linux handheld. | Hand check, Linux: Loop on, Space, 20 passes of the 1 kHz selection (0.5 s to 2.5 s) and of speech. No click and no gap at the seam. Result, date, commit and output device written into docs/checklists/C5.md under "Linux". | easy | docs | hand, me, linux | none |  |
| C5.9 | The same by-ear loop check on the 8 GB Mac. | Hand check, macOS: Loop on, Space, 20 passes of the 1 kHz selection (0.5 s to 2.5 s) and of speech. No click and no gap at the seam. Result, date, commit and output device written into docs/checklists/C5.md under "macOS". | easy | docs | hand, me, mac | none |  |
| C5.10 | Issue 67 hand check on the Windows build machine: plug in headphones during playback. The code is on main and only this check is open. | Hand check, Windows: play a file on the speakers, plug in headphones. Sound must move to them within one second and the playhead keeps moving. Written into docs/checklists/C5.md under "Device change, Windows". | easy | docs | hand, me, windows | none |  |
| C5.11 | Issue 67 hand check on a Linux handheld: plug in headphones or connect Bluetooth during playback. | Hand check, Linux: play a file on the speakers, plug in headphones. Sound must move to them within one second and the playhead keeps moving. Written into docs/checklists/C5.md under "Device change, Linux". | easy | docs | hand, me, linux | none |  |

#### H1: Add Marker with M or * (partial)

- **On main:** M adds a point at the cursor, a range for a selection, and a point at the playhead while playing; flags draw on the ruler: crates/core/src/markers.rs, actions.rs, shortcuts.rs, apps/wavcraft/src/app.rs (draw_markers). Tests: crates/core/tests/actions.rs m1, crates/core/tests/shortcuts.rs marker_add_m. Merged 2026-11-12: H1.1, a typed * adds a marker when no text field has the keyboard (#1203).
- **Done check:** One key press; the marker appears on the ruler.
- **Recorded so far:** not recorded
- **Item issue:** #40

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| H1.3 | Keep my selection when I add a marker during playback. Today run_action replaces the selection with a cursor at the playhead after Add Marker. | apps/wavcraft/src/h1_star_marker_tests.rs on the headless window with WAVCRAFT_NO_AUDIO=1: 3 s of 48 kHz audio, loop on, selection 24_000..72_000. Start playback, run Add Marker. One point marker exists and its frame is inside 24_000..72_000. The selection is still 24_000..72_000. Playback is still running. The same test fails on main, where the selection becomes a cursor at the playhead. | easy | state | script | none | #1179 |
| H1.4 | Hand check on the Windows build machine with a full keyboard: M and numpad * while stopped, with a selection, and while playing. | Hand check, Windows: M at a cursor shows a point flag on the ruler; M with a selection shows a range flag; numpad * and Shift+8 during playback each add a flag at the moving playhead and playback continues. Written into docs/checklists/H1.md. | easy | docs | hand, windows | H1.1, H1.3 |  |

#### H2: Markers panel (partial)

- **On main:** apps/wavcraft/src/markers_panel.rs: table with name, start, end, duration; rename by double-click; Merge button; a single click picks the row and a double-click goes to the marker's span (H2.6). Core: Editor::rename_marker, merge_markers, move_marker in crates/core/src/actions.rs. Tests: markers.rs unit tests, crates/core/tests/actions.rs m6, m9, m10. The panel has no Delete, Convert or typed Start/End yet. Merged 2026-11-12: H2.1 Editor::delete_markers (#1193); H2.2 Editor::set_marker_extent (#1196); H2.6 the panel click rule (#1202).
- **Done check:** Edits made in the panel move the marker on the ruler.
- **Recorded so far:** not recorded
- **Item issue:** #41

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| H2.3 | Pure function in the panel module that says which buttons are enabled for the picked rows: Delete, Merge, To Range, To Point. | apps/wavcraft/src/markers_panel.rs tests, new fn buttons_follow_the_picked_rows: nothing picked gives all false; one point gives delete and to_range; one range gives delete and to_point; a point and a range picked give delete and merge only. | easy | pure-fn | script | none | #1183 |
| H2.4 | Add Delete, To Range and To Point buttons beside Merge. To Range makes a 1 second range from the point (clamped to the file); To Point keeps the start. | Run the app, any platform, file with two markers: pick a point row, To Range: Type reads Range, Duration 0:01.000. To Point: Type reads Point. Delete: the count reads "1 markers" and the flag leaves the ruler. Ctrl+Z brings it back. | easy | ui | script | H2.1, H2.2, H2.3, my ruling on the To Range length |  |
| H2.5 | Pure function that turns typed Start or End text into the marker's new extent. Parse the same m:ss.mmm form the panel shows today. Refuse bad text, an end before the start, or a time past the file. | apps/wavcraft/src/markers_panel.rs tests, new fn typed_start_and_end_give_the_new_extent: 48 kHz, range 48_000..96_000. Start "0:00.500" gives 24_000..96_000; End "0:03.000" gives 48_000..144_000; End "0:00.500" gives None; "abc" gives None; a time past the length gives None. | easy | pure-fn | script | none | #1184 |
| H2.7 | Wire the double-click in the panel: move the playhead to the marker and select a range marker's span. Rename moves to a click on the name of the picked row. | Headless window test: with a range marker 30_000..40_000, the control line MARKER select 1 replies OK selection=30000-40000 and the playhead is at 30_000. For a point marker at 10_000 the reply shows a cursor at 10_000. A single pick through the new rule leaves the selection unchanged. tools/alpha/f39.py still passes. | easy | ui | script | H2.6 |  |
| H2.8 | Make the Start and End cells editable for the picked row; Enter commits through set_marker_extent, Escape cancels. | Run the app, any platform, 48 kHz file with a point marker at 0:01.000: click its row, click Start, type 0:02.000, Enter. The flag on the ruler moves to 2 s, History lists "Move Marker", and Ctrl+Z moves it back. | medium | ui | hand | H2.2, H2.5, H2.6 |  |
| H2.9 | Hand check of the Done line on a Linux handheld: rename, typed Start, Delete, Merge, To Range and To Point, each watched on the ruler. | Hand check, Linux: after each of the six edits the flag on the ruler changes to match the panel row (name, position, gone, one range, range, point). Each result is written into docs/checklists/H2.md with the date and commit. | easy | docs | hand, linux | H2.4, H2.7, H2.8 |  |

#### H3: Next and previous marker (partial)

- **On main:** Ctrl+Alt+Left/Right (Cmd+Option on a Mac) jump the cursor to the neighbouring marker, searching from the playhead while playing: crates/core/src/markers.rs, actions.rs, shortcuts.rs, transport.rs, apps/wavcraft/src/app.rs. Tests: crates/core/tests/actions.rs m2, shortcuts.rs marker_prev and marker_next, transport.rs previous_marker_during_play_skips_the_marker_just_passed.
- **Done check:** The playhead jumps, and it works while playing.
- **Recorded so far:** not recorded. The jump itself is tested (crates/core/tests/actions.rs m2); the path during playback has no test and no hand check.
- **Item issue:** #42

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| H3.1 | During playback, keep playing from the marker after a jump. Use transport::seek_target and Player::seek; stop as today only when the marker is outside what is being played. | New apps/wavcraft/src/h3_marker_jump_tests.rs on the headless window with WAVCRAFT_NO_AUDIO=1: 3 s of 48 kHz audio, point markers at 24_000 and 72_000, no selection. Case 1, fresh start: play from 0, run Next Marker. Playback is still running and the playhead is in 24_000..48_000. Case 2, carrying on: run Next Marker again, the playhead is at or after 72_000 and still playing; then Previous Marker lands in 24_000..48_000, still playing. Case 3: with a selection 0..12_000 playing, Next Marker to 24_000 stops playback as today. | medium | state | script | none | #1186 |
| H3.2 | Hand check on the 8 GB Mac: Cmd+Option+Left and Cmd+Option+Right reach the app, stopped and playing. | Hand check, macOS, file with three markers: each press moves the playhead to the neighbouring flag; during playback the sound carries on from that marker. Written into docs/checklists/H3.md under "macOS", including any key the system kept for itself. | easy | docs | hand, me, mac | H3.1 |  |
| H3.3 | Hand check on the Windows build machine: Ctrl+Alt+Left and Ctrl+Alt+Right, stopped and playing. | Hand check, Windows, file with three markers: each press moves the playhead to the neighbouring flag and the screen does not rotate; during playback the sound carries on from that marker. Written into docs/checklists/H3.md under "Windows". | easy | docs | hand, me, windows | H3.1 |  |
| H3.4 | Hand check on a Linux handheld in desktop mode: Ctrl+Alt+Left and Ctrl+Alt+Right, stopped and playing. | Hand check, Linux, file with three markers: each press moves the playhead to the neighbouring flag and the desktop does not switch workspace; during playback the sound carries on. Written into docs/checklists/H3.md under "Linux". | easy | docs | hand, me, linux | H3.1 |  |

### Stage 1b: Automation

My ruling, 2026-11-03: "In v1, after Editor (Recommended)". Order: N1, then N2.1 to N2.3, then N3 and N4; N2.4 waits for N4.4. Once this stage is in, every later stage's checks can be run by script. Bounce from the command line stays after 1.0.

**Done when** an AI agent completes 10 real editing tasks through the MCP server with no window.

**Remains:**

#### N1: Automation core: one Backend, two ways (partial)

- **On main:** crates/automation is on main: lib.rs (Request parse and line for OPEN, SELECT, ACTION, EFFECT, SAVE, STATE; Backend trait), headless.rs (Headless over wavcraft-core), remote.rs (Remote over the control socket). Tests: tests/request.rs, tests/headless.rs, tests/remote.rs. Nothing else depends on the crate.
- **Done check:** The same steps give the same saved file, sample for sample, with and without the window.
- **Recorded so far:** not recorded
- **Item issue:** #1107

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| N1.1 | Remote waits for a save to finish and reports how it ended: after SAVE answers OK, poll STATE until busy=0 (as OPEN already does), then ask LASTERROR and return its text as the error unless it is '-'. | crates/automation/tests/remote.rs, two new tests. save_waits_for_busy_to_clear: the stand-in answers SAVE with OK, STATE with busy=1 twice, then busy=0, then LASTERROR with 'OK -'. execute returns Ok and the stand-in received exactly TOKEN, SAVE out.wav, STATE, STATE, STATE, LASTERROR. a_failed_save_is_an_error: the same script ending 'OK Not saved: disk full. out.wav is unchanged.' makes execute return Err containing 'Not saved'. | easy | wiring | script | none |  |
| N1.2 | Request::Save carries an optional path and the window's trailing options (bits=, float=, aiff=, quality=, meta=), parsed as parse_save_args, take_quality and take_meta in app.rs do. A bare SAVE (no path) is valid: the window saves in place. | crates/automation/tests/request.rs, new test save_options_are_not_part_of_the_path: 'SAVE my take.wav bits=16' gives path 'my take.wav' and 16-bit; 'SAVE a.ogg quality=7' gives quality 7; 'SAVE' gives no path and no options; 'SAVE bits=24' gives no path and 24-bit; 'SAVE a.wav float=2' is Err containing 'float=<0\|1>'; 'SAVE a.ogg quality=11' is Err containing 'quality=<-1..10>'; line() round-trips each. The existing assertion that 'SAVE' is an error (request.rs line 36) is removed. | easy | data-shape | script | none |  |
| N1.3 | Headless honours the SAVE options: sample type from bits/float, AIFF kind, Vorbis quality, meta=0 through save_wav_with, and a SAVE with no path writes the file that was opened. | crates/automation/tests/headless.rs, new tests. save_bits_24_writes_a_24_bit_file: the 16-bit source saved with bits=24 reopens as 24-bit, without the option as 16-bit; no file named 'out.wav bits=24' exists. save_flac_float_is_refused: 'SAVE out.flac float=1' answers ERR containing 'FLAC cannot hold'. save_aiff_sowt: 'SAVE o.aiff aiff=sowt' writes a file whose bytes contain 'sowt'. save_quality_changes_the_ogg: quality=8 gives a larger file than quality=0. save_meta_0_writes_only_audio: a source with one extra chunk saved with meta=0 is exactly 44 bytes plus the data. save_with_no_path_overwrites_the_source: after an edit, 'SAVE' changes the opened file's samples. | easy | wiring | script | N1.2 |  |
| N1.4 | Headless runs the one-shot effects by EFFECT name (reverse, fadeIn.linear and the rest of actions::ONE_SHOT), as the window does, and refuses settings on them. | crates/automation/tests/headless.rs, new test effect_reverse_runs_the_one_shot, with Headless::with_crossfades(Crossfades::NONE): a 4-frame mono file 1,2,3,4, SELECT 0 4 then EFFECT reverse gives 4,3,2,1 and answers 'selection=0-4 changed=1'; 'EFFECT reverse x=1' answers 'ERR reverse takes no settings here: set them with PARAM'; 'EFFECT nope' still answers ERR containing 'no effect named'. | easy | wiring | script | none |  |
| N1.5 | Parity test: the same six requests through the real window code (WaveApp.control_request, built as issue366_tests.rs does) and through Headless, then compare the two saved files. | New apps/wavcraft/src/n1_parity_tests.rs, test the_window_and_headless_save_the_same_samples. The test sets the same crossfades on both sides: 'PREF set crossfade_cut_join_ms 2' and 'PREF set crossfade_edge_ms 5' in the window, Crossfades::default() in Headless. Steps on both: OPEN, SELECT 1000 2000, ACTION edit.delete, SELECT 0 500, EFFECT invert, SAVE, with settle() after OPEN and SAVE in the window. Both WAVs read back equal, are 1000 frames shorter than the source, and frames 0 to 499 are not equal to the source. Dropping the invert on one side fails it. | medium | wiring | script | none |  |
| N1.6 | The window uses the shared parser: control_request's OPEN, SELECT, ACTION, EFFECT <name> and SAVE arms call Request::parse; delete app.rs parse_save_args, take_quality and take_meta; update the code map. The window keeps its own arms for a bare EFFECT (the list) and for ACTION window.*. | Existing app tests and N1.5's test pass unchanged. New tests in n1_parity_tests.rs: select_with_a_third_number_is_refused ('SELECT 1 2 3' answers 'ERR SELECT <start> <end>, in frames'); a_bare_effect_still_lists ('EFFECT' answers OK containing 'gain [gain_db='); a_bare_save_still_saves_in_place (after an edit, 'SAVE' answers OK and STATE unsaved becomes 0 after settle). grep -c 'fn parse_save_args' apps/wavcraft/src/app.rs prints 0; grep -c 'parses its own' docs/code-map.md prints 0. | medium | wiring | script | N1.2, N1.5 |  |
| N1.7 | Run the same steps over the real socket: a small example program drives Remote::from_env and Headless and compares the files; a script starts the app and runs it; record the result. | On a Linux handheld with a display: bash tools/checks/1107-headless-window-parity.sh prints 'same=1 frames=<source frames minus 1000>' and exits 0. The line, the date and the commit are written into docs/checklists/N1.md. | medium | wiring | script, linux | N1.1, N1.6 |  |
| N1.8 | Remote::open reports a failed background open: after busy=0, ask LASTERROR and return its text as the error unless it is '-'. | crates/automation/tests/remote.rs, new test a_failed_open_is_an_error: the stand-in answers OPEN with OK, STATE with busy=0, LASTERROR with 'OK could not open missing.wav'. execute returns Err containing 'could not open'. The existing open_waits_for_busy_to_clear gets one more scripted reply 'OK -' and one more expected line LASTERROR. | easy | wiring | script | N1.1 |  |

#### N2: Command line: run, convert, inspect (not started)

- **On main:** Nothing usable. apps/wavcraft/src/main.rs parse_args knows only --session-dir, --reset-preferences and FILE. It takes the first other argument as the file, even an unknown flag, and ignores the rest without a message. crates/core/src/batch_cli.rs with crates/core/tests/batch_cli.rs is on main, but nothing calls it.
- **Done check:** Each command runs on a machine with no display; a WAV converted to FLAC and back is identical; a bad flag is an error with a message.
- **Recorded so far:** not recorded
- **Item issue:** #845

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| N2.1 | Pure parser cli::parse: Window(today's Args), Run{script,file,out}, Convert{input,output}, Inspect{file}. After a subcommand, an unknown flag or missing operand is Err with a usage line. | Unit tests in apps/wavcraft/src/cli.rs: convert_takes_in_and_out; a_bad_flag_is_an_error_with_a_message ('convert a.wav b.flac --nope' is Err containing '--nope'); a_bad_flag_with_no_subcommand_is_an_error ('--nope' alone is Err containing '--nope'; today it becomes the file name); a_missing_operand_is_an_error ('convert a.wav' is Err containing 'convert <in> <out>'); no_subcommand_is_the_window ('a.wav --session-dir=D' equals today's Args); run_takes_o ('run s.txt a.wav -o b.wav'). main.rs test the_session_dir_flag_and_the_file_in_any_order passes unchanged. | easy | pure-fn | script | none |  |
| N2.2 | wavcraft convert <in> <out>: main.rs dispatches before the log and window start; open and save through Headless; exit 0, or exit 2 with the reason on standard error. | New apps/wavcraft/tests/cli.rs, run with DISPLAY and WAYLAND_DISPLAY removed and --session-dir set to a scratch folder: wav_to_flac_and_back_is_identical (16-bit WAV to FLAC to WAV, samples equal the source, exit 0); a_bad_flag_exits_2_with_a_message (stderr contains '--nope'); an_unknown_extension_is_refused (out.xyz: exit 2, stderr contains 'no save format'); no_session_is_left_behind (the scratch session folder is empty after a good run and after a failed run). | medium | wiring | script | N2.1 |  |
| N2.3 | wavcraft inspect <file>: print the rows of Document::properties (crates/core/src/document.rs), one per line as 'Key: value'. | apps/wavcraft/tests/cli.rs, test inspect_prints_the_properties_rows: for a 1 s, 48 kHz, stereo, 16-bit WAV, standard output has the lines 'Sample rate: 48000 Hz', 'Channels: 2' and 'Duration: 0:01.000 (48000 frames)'; exit 0. A missing file exits 2 with its name on standard error. | easy | wiring | script | N2.2 |  |
| N2.4 | wavcraft run <script> <file> [-o <out>]: open the file in Headless, run the script with N4's runner, save to <out> only when every line passed. | apps/wavcraft/tests/cli.rs: run_applies_a_two_step_script ('SELECT 1000 2000' and 'ACTION edit.delete' with -o out.wav: out.wav is 1000 frames shorter, exit 0); run_matches_headless_run_script (the five-line script of N4.6 with -o gives the same samples as Headless::run_script called in the test); a_bad_line_writes_nothing (bad line 2: exit 2, stderr contains 'line 2', out.wav does not exist). | easy | wiring | script | N2.2, N4.4 |  |
| N2.5 | Windows release build: make a subcommand's output reach the terminal it was started from (the build is windowed), then check by hand and record it. | Hand check on the Windows build machine, release build, in PowerShell: 'wavcraft.exe inspect kit.wav' shows the line 'Channels: 2' and $LASTEXITCODE is 0; 'wavcraft.exe convert a.wav b.flac --nope' shows '--nope' and 2. Written into docs/checklists/N2.md with date and commit. | hard | wiring | hand, windows | N2.3 |  |

#### N3: MCP server (wavcraft mcp) (not started)

- **On main:** Nothing. There is no crates/automation/src/mcp.rs and no mcp subcommand in apps/wavcraft/src/main.rs. The Backend it would sit on is in crates/automation/src/lib.rs, and effects::all with effects::describe already give the effect list.
- **Done check:** An agent completes 10 real editing tasks through it, each checked, both ways (no window, and attached to a running one).
- **Recorded so far:** not recorded
- **Item issue:** #1108

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| N3.1 | mcp::handle(line, backend) -> Option<String>: newline-delimited JSON-RPC 2.0 answering initialize and tools/list with seven tools: open, select, action, effect, state, save, list_effects. | New crates/automation/tests/mcp.rs: initialize_answers_as_mcp_expects (result has protocolVersion equal to the one asked for, capabilities.tools is an object, serverInfo.name is 'wavcraft'); tools_list_names_the_seven_tools (exactly open, select, action, effect, state, save, list_effects, each with an inputSchema of type object); a_notification_gets_no_answer ('notifications/initialized' gives None); bad_json_is_error_minus_32700; an_unknown_method_is_error_minus_32601; the_id_is_echoed (number and string ids). | medium | data-shape | script | none |  |
| N3.2 | tools/call: turn each tool's arguments into a Request, run it on the Backend, return the reply as text; a Backend error is an isError result; list_effects returns effects::describe for every effect. | crates/automation/tests/mcp.rs over Headless: a_call_returns_the_backend_reply (select start=10 end=20 gives text 'selection=10-20'); a_backend_error_is_an_is_error_result (action 'edit.nope': isError true, text contains 'no action named'); list_effects_names_gain (text contains 'gain [gain_db='). | easy | wiring | script | N3.1 |  |
| N3.3 | Ten editing tasks through the handler with no window, each checked: open, select, delete, silence, gain, normalize, fade in, invert, add marker, save. | New crates/automation/tests/mcp_tasks.rs, test ten_tasks_headless, Crossfades::NONE, one assertion per task read from Headless::document() or the saved file: (1) open: state text has 'frames=20000 channels=2 rate=48000'; (2) select 1000 2000 answers 'selection=1000-2000'; (3) action edit.delete leaves 19000 frames; (4) action edit.silence on 0..500 gives all 0 there; (5) effect gain gain_db=-6.0206 on 500..1000 halves samples within 1 LSB; (6) effect normalize 'unit=dB db=-2' gives a peak of -2 dBFS within 0.01 dB; (7) action effect.fadeIn.linear on 0..1000 gives 0 at frame 0 and half the old value within 1 LSB at frame 500; (8) effect invert negates 2000..3000; (9) action marker.add at 5000 gives one marker at frame 5000; (10) save: the file reads back equal to the document. | medium | wiring | script | N3.2 |  |
| N3.4 | The same ten tasks through the handler against the real window code: a test Backend over WaveApp.control_request (N1.5's harness) that waits for saves. | New apps/wavcraft/src/n3_mcp_window_tests.rs, test ten_tasks_through_the_window: the same ten tools/call lines and the same ten assertions as N3.3 pass, with the app's edit crossfades set to none. | medium | wiring | script | N3.3, N1.5 |  |
| N3.5 | wavcraft mcp and wavcraft mcp --attach: a loop reading standard input lines into mcp::handle, over Headless, or over Remote::from_env with --attach. | apps/wavcraft/tests/cli.rs, test mcp_answers_tools_list_on_stdio: start the binary with 'mcp', write an initialize line and a tools/list line, read two JSON lines; the second lists seven tools; closing standard input ends the process with code 0. 'mcp --attach' with no port set exits 2 naming WAVCRAFT_CONTROL_PORT. | easy | wiring | script | N2.2, N3.2 |  |
| N3.6 | A real agent run, recorded: connect an MCP client to wavcraft mcp, then to wavcraft mcp --attach with the app open, and have the agent do the ten tasks each way. | Hand check on a Linux handheld with a display. The agent is told to save one file after each task, named 01-open.wav to 10-save.wav, into one folder per way. For each folder, the checker from the added step (cargo run -p wavcraft-automation --example ten_tasks -- check <folder>) prints 'same=10' and exits 0. docs/checklists/N3.md lists 20 rows (task, way, pass), the agent used, the date and the commit. | easy | docs | hand, linux | N3.5, N1.1, N3.7 |  |
| N3.7 | Checker for the ten tasks: an example program that writes the ten reference files through Headless (the N3.3 requests, Crossfades::NONE) and compares a folder of ten files against them, sample for sample. | crates/automation/tests/mcp_tasks.rs, new test the_checker_accepts_its_own_files_and_refuses_a_changed_one: 'ten_tasks write <dir>' then 'ten_tasks check <dir>' prints 'same=10' and exits 0; after one sample of 05-gain.wav is changed it prints 'same=9 differs=05-gain.wav' and exits 1. | easy | wiring | script | N3.3 |  |

#### N4: Scripts: a text file of commands (not started)

- **On main:** Nothing. There is no crates/automation/src/script.rs and no Run Script entry anywhere in apps/wavcraft/src. Request::parse and Backend::answer_line in crates/automation/src/lib.rs are what a script line would use.
- **Done check:** A 5-step script gives the same file both ways; a bad line stops the script, names the line and changes nothing; in the window one Undo reverts it.
- **Recorded so far:** not recorded
- **Item issue:** #879

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| N4.1 | script::parse(text): one Request per line with its 1-based line number; '#' comments and blank lines skipped; CRLF accepted; OPEN and SAVE lines refused; the first bad line reported as 'line N: reason'. | New crates/automation/tests/script.rs: comments_and_blank_lines_are_skipped (7 lines, 2 comments, 1 blank give 4 requests numbered 1, 3, 4, 7); a_bad_line_is_named (error text starts 'line 3:'); windows_line_endings_parse; save_is_not_a_script_line ('line 2: SAVE is not allowed in a script'). | easy | pure-fn | script | none |  |
| N4.2 | script::run(backend, script): execute the parsed lines in order on any Backend; stop at the first error and return its line number and reason. | crates/automation/tests/script.rs over Headless with Crossfades::NONE: five_steps_give_the_expected_samples (SELECT 1000 2000, ACTION edit.delete, SELECT 0 500, EFFECT invert, EFFECT gain gain_db=-6.0206: 19000 frames; frames 0 to 499 equal minus half the source within 1 LSB; frames 500 on equal the source with 1000..2000 removed); a_failing_line_stops_the_run (line 3 'ACTION edit.nope': error is 'line 3: no action named "edit.nope"'; line 4's edit did not happen). | easy | wiring | script | N4.1 |  |
| N4.3 | History::squash_since(cursor, label): replace every step after 'cursor' with one step holding the current state under 'label'. | New crates/core/tests/history_squash.rs, test squash_makes_one_step: three edits from cursor 0, then squash_since(0, "Run Script"): steps().len() is 2, undo_label() is Some("Run Script"), one undo() gives the original content hash, one redo() the final one, held_bytes() is no larger than before. | easy | state | script | none |  |
| N4.4 | Headless::run_script(text): remember the history cursor and selection, run the script, squash to one step 'Run Script' on success; on a failed line go back and delete the steps it made. | crates/automation/tests/script.rs: a_failing_line_changes_nothing (bad line 3: samples, selection in STATE and step count equal the values before the run); a_good_script_is_one_step (five lines add exactly one step; ACTION edit.undo gives the source samples back). | easy | state | script | N4.2, N4.3 |  |
| N4.5 | Window: control verb 'RUN_SCRIPT <path>' and WaveApp::run_script. Runs each line on the open document, squashes to 'Run Script'; a bad line restores the starting step and answers 'ERR line N: reason'. | New apps/wavcraft/src/n4_run_script_tests.rs: one_undo_reverts_the_script (STATE steps grows by exactly 1; after ACTION edit.undo a SAVE equals the source); a_bad_line_3_changes_nothing (answer starts 'ERR line 3:'; STATE steps, selection and unsaved are unchanged). | medium | wiring | script | N4.2, N4.3 |  |
| N4.6 | Both-ways test: the same 5-line script through RUN_SCRIPT in the window code and through Headless::run_script, both saved and compared. | apps/wavcraft/src/n4_run_script_tests.rs, test five_steps_give_the_same_file_both_ways: the test sets the same crossfades on both sides ('PREF set crossfade_cut_join_ms 2' and 'PREF set crossfade_edge_ms 5' in the window, Crossfades::default() in Headless). The two saved WAVs have equal samples and equal length (19000 frames). Removing one script line on one side fails it. | easy | wiring | script | N4.4, N4.5 |  |
| N4.7 | File > Run Script...: a File menu entry that opens a file picker (text files) and calls run_script; a bad line shows 'line N: reason' in the status line. | n4_run_script_tests.rs, test the_menu_entry_runs_the_picked_script: with the scripted pick set to five.txt, the entry's handler adds exactly 1 step; with bad3.txt the app's error text starts 'line 3:' and STATE steps is unchanged; with the pick cancelled nothing changes and there is no error. | easy | ui | script | N4.5 |  |
| N4.8 | Hand check of File > Run Script... on a real window, recorded. | Hand check on a Linux handheld with a display, debug build, a 20 s file open: File shows 'Run Script...' and it is greyed with no file open. I pick five.txt: the waveform gets shorter and the History panel gains one row 'Run Script'. One Ctrl+Z brings the original length back. I pick bad3.txt: the status line shows 'line 3:' and the waveform and History do not change. What I saw, the date and the commit are written into docs/checklists/N4.md. | easy | docs | hand, linux | N4.7 |  |

### Stage 2: Recording

**Done when** I record a real 10-minute take on each machine, kill the app once mid-take, and get everything back.

**Remains:**

#### J1: Record a take into a new WAV (partial)

- **On main:** Take writer and names in crates/core/src/record.rs (tests inside it). App side in apps/wavcraft/src/recording.rs, rec_input.rs and app.rs: Record button, Record Now, Shift+Space (issue1000_tests.rs). Missing: recordings folder preference, mic channel count by default.
- **Done check:** A 1-hour take on the 64 GB Linux tablet: dropped counter reads 0, and the file opens in WavCraft and in another player.
- **Recorded so far:** not recorded
- **Item issue:** #51

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J1.1 | Add a recordings_folder key to Settings: parsed, trimmed, empty means default, written only when set. Follow the input_device key. | New crates/core/tests/j1_recordings_folder.rs, test j1_recordings_folder_reads_writes_and_clears: parse of 'recordings_folder = /tmp/My Takes' gives Some("/tmp/My Takes"); an empty value gives None; default to_text() has no recordings_folder line; set, write, parse gives the same path. Also set("recordings_folder", "/tmp/x") then get("recordings_folder") is "/tmp/x", and settings::KEYS contains "recordings_folder". | easy | data-shape | script | none |  |
| J1.2 | Add pure record_dir_from(env, preference, system_default) to recording.rs. Order: environment variable, then preference, then default. Make record_dir take the preference and pass it from its three callers. | New test in apps/wavcraft/src/recording.rs, the_recordings_folder_is_env_then_preference_then_default: (Some("/e"), Some("/p"), Some("/d")) gives /e; (None, Some("/p"), Some("/d")) gives /p; (None, None, Some("/d")) gives /d; all None is an Err. | easy | wiring | script | J1.1 |  |
| J1.3 | Preferences, Playback and Recording page: a 'Recordings folder' row showing the path, a Choose button with a folder picker, and a Default button. Saved through pref_set like the session folder. | By hand with WAVCRAFT_RECORD_DIR unset: open Preferences, Playback and Recording. Click Choose and pick an empty folder: the row shows that folder. Press Shift+Space, wait 3 seconds, press Shift+Space: the new Take file is in that folder. Click Default: the row shows the 'WavCraft Recordings' folder again. | easy | ui | hand | J1.2 |  |
| J1.4 | Add pure first_format(device_channels: Option<u16>) in rec_input.rs: the default format with the device's channel count, kept within 1 to 8. Add default_channels(wanted) that reads the input device's default config. | New test in apps/wavcraft/src/rec_input.rs, the_first_take_has_the_mics_channel_count: first_format(Some(2)) is 48000 Hz, 2 channels, 24-bit; None gives 1 channel; Some(0) gives 1; Some(12) gives 8. | easy | pure-fn | script | none |  |
| J1.5 | Add Recording::start_format(device_channels): the last format used, else first_format(device_channels). Record Now and the New Recording dialog call it in place of last_format(). With the fake input the device count is None, so the take stays mono. | New test in apps/wavcraft/src/recording.rs, the_first_take_follows_the_mic_and_later_takes_keep_the_last_format: on a new Recording, start_format(Some(2)) is 48000 Hz, 2 channels, 24-bit and start_format(None) is 1 channel; with the last format set to 44100 Hz, 1 channel, 16-bit, start_format(Some(2)) returns that format unchanged. tools/alpha/f25.py still passes 'the take is open in the editor, mono 48 kHz'. | easy | wiring | script | J1.4 |  |
| J1.6 | Hand check on the 64 GB Linux tablet: record one hour from the USB mic into a folder chosen in Preferences, then open the take in WavCraft and in a second player. | Must see: on a fresh start the status bar shows '48000 Hz <n> ch 24-bit' with n equal to the mic's channel count; the status bar never shows DROPPED; log line 'take ... finished: N frames, 0 dropped' with N near 172800000; the file is in the chosen folder and plays in WavCraft and in mpv. Write date, commit, N, the channel count and both players into docs/checklists/J1.md. | easy | docs | hand, me, linux | J1.3, J1.5 |  |
| J1.7 | Add a run to the scripted recording check with WAVCRAFT_RECORD_DIR unset: set the recordings folder through PREF, record, restart, record again. | tools/alpha/f25.sh prints status pass with two new checks. 'PREF set recordings_folder <tmp>' answers OK and 'RECORD quick' answers 'OK path=<tmp>/Take ...wav'. After QUIT and a new start, 'RECORD quick' again answers a path in <tmp>. | easy | wiring | script | J1.2, J6.9 |  |

#### J2: No playback while recording (partial)

- **On main:** App::begin_recording in apps/wavcraft/src/app.rs halts play and mutes the player. apps/wavcraft/src/player.rs refuses a play while muted and renders zeros (test muted_is_silence). Play button is disabled during a take. The output stream opened at start stays open during a take and plays silence. Two paths reopen it mid-take: Player::maintain after a device change, and Player::set_output when I change the output device or buffer in Preferences.
- **Done check:** Checked in code and by ear: nothing comes out of the computer's speakers while recording.
- **Recorded so far:** not recorded
- **Item issue:** #52

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J2.1 | Make Player::maintain do nothing while muted and leave the device-change flag set, so the output reopens only after the take ends. | New apps/wavcraft/src/j2_no_output_tests.rs (declared from player.rs, with its own silent-player helper that sets WAVCRAFT_NO_AUDIO=1 once), test a_device_change_during_a_take_waits_for_the_take_to_end: set_muted(true), simulate_device_change(), maintain() is false, reopens() is 0 and the device name is still 'test sink (no audio)'; after set_muted(false), maintain() is true, reopens() is 1 and the name is 'test sink (no audio) 1'. | easy | state | script | none |  |
| J2.2 | By-ear check on the 64 GB Linux tablet: speakers at full volume, a file playing, then press Shift+Space and record one minute with the USB mic. During the take, plug headphones into the computer's own jack and pull them out again. | Must see and hear: playback stops at once; nothing from the speakers or the headphones for the whole take; the take keeps running through the headphone plug and unplug; Play button greyed; status bar has no '(monitoring)'. After Stop, Space plays again. Write date, commit and result into docs/checklists/J2.md. | easy | docs | hand, me, linux | J2.1, J2.3 |  |
| J2.3 | Make Player::set_output keep the new choice but not reopen the output while muted. It sets the device-change flag, so the output reopens on the new choice after the take. | In apps/wavcraft/src/j2_no_output_tests.rs, test an_output_change_during_a_take_waits_for_the_take_to_end: silent player, set_muted(true), set_output(OutputChoice::default()), maintain() is false and reopens() is 0; after set_muted(false), maintain() is true and reopens() is 1. The test is red on main, where set_output reopens at once and clears the flag. | easy | state | script | J2.1 |  |

#### J3: A crash keeps the take (built, check not recorded)

- **On main:** crates/core/src/record.rs syncs the header every second and keeps a .recording marker. crates/core/src/take_recovery.rs finds and repairs; apps/wavcraft/src/recovery.rs and App::recover_take offer and open. Tests: crates/core/tests/take_recovery.rs (9 tests, three CI systems). Scripted run tools/alpha/f55.py kills the app mid-take, restarts, sends RECOVERTAKE and checks the length, the tone and that the marker is gone. Merged 2026-11-12: J3.1, the fake-input test waits for the tap instead of sleeping (#1155).
- **Done check:** Kill the app mid-take. After restart the take holds everything but at most the last second and plays. Tested automatically on every machine, and once by hand.
- **Recorded so far:** not recorded
- **Item issue:** #53

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J3.2 | Delete the unused stubs take_repair.rs and take_recover.rs, their two ignored test files and their two lines in lib.rs. In docs/code-map.md remove their four rows and fix the recovery.rs row that names take_recover.rs. | 'ls crates/core/src/take_repair.rs crates/core/src/take_recover.rs crates/core/tests/j3_take_repair.rs crates/core/tests/j3_take_recover.rs' reports all four missing. 'grep -n "take_repair\\|take_recover;" crates/core/src/lib.rs' prints nothing. 'grep -c "take_repair\\|take_recover\.rs" docs/code-map.md' prints 0. 'cargo test -p wavcraft-core --test take_recovery' reports 9 passed, 0 ignored. | easy | rename | script | my approval to delete two ignored test files |  |
| J3.3 | Add a test that kills a real process mid-take: the test starts a copy of itself that records the fake sine, waits for the header to claim 96000 frames, kills it, then repairs. | New crates/core/tests/j3_process_kill.rs, test a_killed_process_leaves_a_take_that_repairs: unfinished_takes lists 1 take; repair_take returns at least 96000 frames and equals (file length - 44) / 3; the marker is gone; the file opens as 48000 Hz mono. Green on the three CI systems. | medium | state | script | J3.1 |  |
| J3.5 | Hand kill once on the 64 GB Linux tablet: record with the USB mic, note the elapsed time at about 1:00, run kill -9 on the app, start it again, click Recover, play. | Must see: the recovery window lists the take; after Recover its length is within 1 second of the noted time; it plays to the end; no .recording file is left. Write date, commit and both times into docs/checklists/J3.md. | easy | docs | hand, me, linux | none |  |

#### J4: Readouts and level meter while recording (partial)

- **On main:** Recording::status_text in apps/wavcraft/src/recording.rs shows elapsed, size, time left on disk, peak dB, the word CLIP and the dropped count in the status bar. crates/core/src/take_readout.rs with crates/core/tests/j4_take_readout.rs exists but has no caller.
- **Done check:** Visible and right.
- **Recorded so far:** not recorded
- **Item issue:** #54

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J4.1 | Build the status bar's elapsed, size, left and dropped texts through wavcraft_core::take_readout::take_readout, in a small pure helper. Keep left_secs: STATE still needs the number for rec_left_s. | New test in apps/wavcraft/src/recording.rs, the_status_numbers_come_from_the_take_readout: 3_120_000 frames at 48000 Hz mono 24-bit with 86_400_000 free bytes gives text containing '1:05', '10:00 left on disk' and no 'DROPPED'; with 480 dropped frames it contains 'DROPPED 480 frames (10 ms)'. | easy | pure-fn | script | none |  |
| J4.2 | Add Recording::meter() returning the take's latest peak and its latched clip flag, both kept by refresh(). | New test in apps/wavcraft/src/recording.rs, the_take_meter_follows_the_input_and_latches_clip. The test builds the take around a ring that only the test feeds (the fake input feeds a second ring). Before any audio meter() is Some((0.0, false)). After pushing 480 samples of 0.5 and refresh(), meter() is Some((0.5, false)). After pushing one 1.0 sample and refresh(), clip is true, and it is still true after two more refreshes with no new audio. With no take, meter() is None. | easy | state | script | none |  |
| J4.3 | Add pure bars_source(take_peak, input_open, playing, input, output) in app.rs returning the peaks and whether they are the input. Use it in meter_peaks for the peaks and in sync_input for meters.input_view, so the Levels bars, clip light and the input label show the take while recording. | New apps/wavcraft/src/j4_meter_tests.rs, test the_bars_show_the_take_while_recording: (Some(0.5), true, false, [0.1,0.1], [0.0,0.0]) gives ([0.5,0.5], input view true); (None, true, false) gives the input; (None, true, true) and (None, false, false) give the output, view false. | easy | wiring | script | J4.2 |  |
| J4.4 | Clear the Levels clip lights when a take starts, and add a scripted check that the meter shows the take: record the fake sine and read METERS on the control channel. | tools/alpha/f25.sh prints status pass with two new checks. 'a new take clears the clip lights': before recording 'METERS PUSH 1.0 1.0' answers l_clip=1; right after RECORD start METERS answers l_clip=0. 'the Levels meter shows the take': one second into the take METERS answers l between -7.0 and -5.0, l_clip=0 and view=input. | easy | wiring | script | J4.3, J6.9 |  |
| J4.5 | Hand check on the 64 GB Linux tablet during a 5-minute USB mic take, with a phone stopwatch and a terminal beside the app. | Must see: Levels bars move with my voice; a clap lights the clip light and it stays lit; at 5:00 elapsed reads 5:00 within 1 s; size matches ls -l within 150 kB; 'left on disk' within 5% of df free bytes / 144000 per channel; dropped absent. Write into docs/checklists/J4.md. | easy | docs | hand, me, linux | J4.1, J4.3 |  |

#### J5: Input device preference and clean unplug (built, check not recorded)

- **On main:** input_device key in crates/core/src/settings.rs (crates/core/tests/j5_input_device.rs, 7 tests). Preferences row in apps/wavcraft/src/app.rs. apps/wavcraft/src/rec_input.rs opens that device and flags a lost one; recording.rs test a_lost_input_stops_the_take_and_keeps_its_frames.
- **Done check:** Unplug test by hand on the 64 GB Linux tablet.
- **Recorded so far:** not recorded
- **Item issue:** #55

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J5.1 | Hand unplug test on the 64 GB Linux tablet: pick the USB mic in Preferences, Audio Hardware, restart, confirm it is still picked, record 30 seconds, pull the cable. | Must see: the take stops within 2 seconds; the status bar says 'input device unplugged, take saved: <path>' or 'input device stopped, ...'; the take opens and plays to the unplug; no .recording file beside it. Write date, commit and the message into docs/checklists/J5.md. | easy | docs | hand, me, linux | none |  |

#### J6: Recording works on every machine (partial)

- **On main:** Fake input in crates/core/src/record.rs (WAVCRAFT_FAKE_INPUT) and the scripted run tools/alpha/f25.py for the Windows build machine. Core recording tests run on three CI systems. No refusal message is shown yet (J6.2 wires it). Merged 2026-11-12: J6.1 the pure mic_blocked_hint in recording.rs (#1161); J6.3 tools/macos/bundle.sh and Info.plist with the microphone usage text (#1167).
- **Done check:** A 10-minute take with a USB mic on each machine, opened and played back afterwards.
- **Recorded so far:** not recorded
- **Item issue:** #56

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J6.2 | Track the take's highest peak in refresh() and show the hint once as the app's notice. On macOS also add the same text to an input-open error. | New tests in apps/wavcraft/src/recording.rs. a_silent_take_raises_the_hint_once: a take fed 96000 zero frames, after waiting up to 5 seconds for the writer to hold 96000 frames, returns the hint from take_hint("macos") once and None on the next call; the same take with "linux" returns None. an_input_open_error_on_macos_names_the_permission: open_error_text("macos", "no audio input device") contains 'Privacy & Security' and the same call with "linux" returns the error unchanged. | easy | wiring | script | J6.1 | #1147 |
| J6.4 | On the Windows build machine, which has no mic, run the two scripted fake-input runs (record and kill, then kill and recover) and write the result down. | Commands in Git Bash: tools/alpha/f25.sh, then tools/alpha/f55.sh. Expected: every check ok and status pass in both, including 'the Levels meter shows the take' in f25 and 'RECOVERTAKE answers OK' and 'the marker is gone' in f55. Paste date, commit and both pass lines into docs/checklists/J6.md under 'Windows, fake input'. | easy | docs | script, windows | J4.4, J6.9 |  |
| J6.5 | Hand check on the 64 GB Linux tablet: a 10-minute take with the USB mic, Stop, then play it back. | Must see: length 10:00 within 2 s, no DROPPED in the status bar, the take opens in the editor after Stop and plays start to end. Write date, commit, mic model and length into docs/checklists/J6.md under 'Linux handheld 1'. | easy | docs | hand, me, linux | J1.5 |  |
| J6.6 | The same 10-minute hand check on the second Linux handheld, when a mic is with it. | Must see: length 10:00 within 2 s, no DROPPED, opens after Stop, plays start to end. Write date, commit, mic model and length into docs/checklists/J6.md under 'Linux handheld 2'. | easy | docs | hand, me, linux | J6.5, a USB mic at that machine |  |
| J6.7 | Hand check on the M1 Mac mini: open WavCraft.app, press Record, refuse the prompt, record 5 seconds, then allow the mic in System Settings and record 10 minutes. | Must see: the system prompt with my usage text on first Record; after refusing, the notice naming Privacy & Security; after allowing, a 10:00 take with no DROPPED that opens and plays. Write into docs/checklists/J6.md under 'M1 Mac mini'. | easy | docs | hand, me, mac | J6.2, J6.3, the M1 Mac mini being set up |  |
| J6.8 | The same Mac hand check on the second user's Mac, when it is scheduled. | Must see: the prompt, the refusal notice, then a 10:00 take with no DROPPED that opens and plays. Write date, commit and macOS version into docs/checklists/J6.md under 'Second Mac'. | easy | docs | hand, me, mac | J6.7, a scheduled session on that Mac |  |
| J6.9 | Repair the scripted recording check: kill the app with proc.kill() in place of signal.SIGKILL, and count only .wav files in the check 'both takes are still in the recordings folder'. | tools/alpha/f25.sh prints status pass on Linux and in Git Bash on Windows. The check 'both takes are still in the recordings folder' is ok while the killed take's .recording marker is still beside it. | easy | wiring | script | none |  |

#### J7: After Stop the take opens in the editor (built, check not recorded)

- **On main:** App::finish_recording in apps/wavcraft/src/app.rs stops the take and opens its file with the notice 'Recorded <path>'. Reached from the Stop Recording button, Shift+Space, RECORD stop, the time limit and a lost input. Only tools/alpha/f25.py checks it.
- **Done check:** One click from Stop to editing.
- **Recorded so far:** not recorded
- **Item issue:** none

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| J7.1 | Hand check on the 64 GB Linux tablet: record 20 seconds from the USB mic, click 'Stop Recording' once, then cut. | Must see after that one click: the take's waveform in the editor, its file name in the title, the notice 'Recorded <path>'. Select 2 seconds, press Delete, the length drops by 2 s; Ctrl+Z restores it. Write date, commit and result into docs/checklists/J7.md. | easy | docs | hand, me, linux | none |  |

### Stage 3: Repair

**Done when** I clean a real episode end to end and prefer the result to my current tool's, by ear.

**Complete:**

- **E2 Amplify.** crates/core/src/effects/amplify.rs: left and right gain, -80 to +40 dB, default 0, link on by default, with a streaming form for the rack. Tests in crates/core/tests/amplify.rs. Done check: Exact gain to 0.001 dB on a test signal. Float headroom kept above 0 dBFS until save. Recorded: crates/core/tests/amplify.rs: amplify_exact_gain_to_thousandth_db_across_block_boundaries and amplify_keeps_float_headroom_above_zero_dbfs_until_save.
- **E3 Normalize.** crates/core/src/effects/normalize.rs: peak target in percent or dB, default 100 %, all channels equally on, DC bias adjust off. Tests in crates/core/tests/normalize.rs. Done check: Result peak equals the target to 0.01 dB (sample peak). Recorded: crates/core/tests/normalize.rs: normalize_to_a_db_target and normalize_default_brings_the_whole_file_to_100_percent.
- **E4 Invert and Reverse.** Invert in crates/core/src/effects.rs and Reverse in crates/core/src/effects/reverse.rs, both in the Effects menu. Test in crates/core/tests/reverse.rs. Done check: Bit-exact (test). Recorded: crates/core/tests/reverse.rs: reverse_and_invert_are_bit_exact_and_commute.

**Remains:**

#### E1: Fade In and Fade Out handles (partial)

- **On main:** Curves in crates/core/src/dsp/fade.rs, effects fade_in and fade_out in crates/core/src/effects/fade.rs (Effects menu, with dialog), handle maths in crates/core/src/fade_handles.rs. Tests: crates/core/tests/e1_fade_curve.rs, e1_fade_effect.rs, e1_fade_handles.rs.
- **Done check:** The applied gain matches the curve's formula to 0.01 dB at 100 points (test).
- **Recorded so far:** crates/core/tests/e1_fade_effect.rs, e1_effect_fade_in_and_out_match_formula_to_hundredth_db_at_100_points
- **Item issue:** #27

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| E1.1 | Add a function that turns a FadeGesture from fade_handles::drag_fade into the effect id, curve param and frame range to apply. | New test e1_gesture_maps_to_effect_call in apps/wavcraft/src/fade_drag.rs: a fade-in gesture over frames 0..48000 with the cosine curve gives id fade_in, curve Cosine, range 0..48000. A fade-out gesture gives id fade_out. | easy | pure-fn | script | none |  |
| E1.2 | Draw the two handles at the top corners of the waveform. Send pointer moves to handle_near and drag_fade. On release apply the fade as one undo step. | A core test drags the fade-in handle to frame 24001 on a 1 s file of constant 1.0 at 48 kHz with the linear curve. Sample 0 is 0.0, sample 12000 is 0.5 within 1e-6, and sample 24001 is still 1.0. | medium | ui | script | E1.1 |  |
| E1.3 | Add a Favorites table to apps/wavcraft/src/menus.rs and draw it as a new top-level menu in the menu bar code in apps/wavcraft/src/app.rs. The table holds the fade and effect entries the row names. | New test e1_favorites_menu_lists_fade_in_and_fade_out in apps/wavcraft/src/menus.rs: the Favorites menu data has exactly two entries, named Fade In and Fade Out, bound to effect ids fade_in and fade_out. | easy | ui | script | none |  |
| E1.4 | Hand check of the handles on the Linux handhelds. | Open a 10 s tone. Drag the left handle inward level, then up, then with Ctrl held. I must see a straight, a logarithmic and an S-shaped fade, each undone by one Undo. Write the result into docs/checklists/E1.md. | easy | docs | hand, linux | E1.2, E1.3 |  |

#### E5: Repeat Last Command (partial)

- **On main:** Editor::repeat in crates/core/src/actions.rs runs the last effect again. The dialog half is inline in apps/wavcraft/src/app.rs. Keys F3, Shift+R, F2, Ctrl+R in crates/core/src/shortcuts.rs. Tests in crates/core/tests/e5_repeat_last.rs.
- **Done check:** Works for every effect in this list. Delete is not repeatable.
- **Recorded so far:** Half recorded: crates/core/tests/e5_repeat_last.rs covers Shift+R and Delete. The dialog half is tested only through a copy of the logic.
- **Item issue:** #31

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| E5.1 | Replace the todo!() body of Editor::repeat_dialog with the logic of the helper in the test file. Delete the helper and make the tests call the real function. | A core test applies Amplify with 3 dB and then calls Editor::repeat_dialog. It returns RepeatDialog::Effect(<amplify index>, [3.0, 0.0, 1.0]). On a fresh document after Delete alone it returns RepeatDialog::Unavailable. The helper copy in crates/core/tests/e5_repeat_last.rs is removed and the test calls the real method. | easy | state | script | none |  |
| E5.2 | Make the Ctrl+R and F2 branch in the app call Editor::repeat_dialog and open the dialog from its answer. Remove the inline copy. | New test e5_ctrl_r_reopens_dialog_with_last_params in apps/wavcraft/src/app.rs: run Amplify at left_db 3.0, send the repeat-with-dialog action, the open effect dialog is Amplify with left_db 3.0 and the audio is unchanged. | easy | wiring | script | E5.1 |  |
| E5.3 | Hand check of the four keys on the Linux handhelds. | Run Amplify at +3 dB, then Normalize, then a Fade In. After each press Shift+R, F3, Ctrl+R and F2. I must see the same change with no dialog for the first two and the dialog with the last settings for the others. Write into docs/checklists/E5.md. | easy | docs | hand, linux | E5.2 |  |

#### E6: Preview in every effect dialog (partial)

- **On main:** crates/core/src/effects/preview.rs renders an effect block by block with live settings (5 unit tests) but the app never calls it. apps/wavcraft/src/effect_dialog.rs and start_preview in app.rs offer Preview for generators only, as a 2 s one-shot.
- **Done check:** Changing a setting while previewing is heard within 100 ms, by ear.
- **Recorded so far:** not recorded
- **Item issue:** #32

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| E6.1 | Add a preview worker thread that owns a copy of the document, loops the selection through effects::preview::Preview in 1024-frame blocks into a MonitorRing, and takes new settings over a channel. | New tests in apps/wavcraft/src/effect_preview.rs. e6_worker_loops_selection: a 4800-frame ramp read for 9600 frames gives the ramp twice. e6_worker_applies_new_params_within_100_ms: 0.5 input, Amplify 0 dB changed to -6.02 dB, output is 0.25 within 0.001 after at most 4800 frames at 48 kHz. | medium | state | script | none |  |
| E6.2 | Add a PREVIEW sub-verb to the EFFECT_DIALOG control verb. It does not exist today. It starts the effect preview the same way the dialog's Preview button does. | New test e6_effect_preview_starts_and_stops in apps/wavcraft/src/app.rs: with an Amplify dialog open, EFFECT_DIALOG PREVIEW answers OK and preview is running, a second call stops it, and the undo count is unchanged. | medium | wiring | script | E6.1 |  |
| E6.3 | Show the Preview toggle in every effect dialog, not only generators. Send the dialog's settings to the worker each time one changes. | New test e6_every_effect_dialog_offers_preview in apps/wavcraft/src/effect_dialog.rs: for every index in effects::all(), a new dialog reports that it offers Preview. A changed setting produces exactly one settings message. | easy | ui | script | E6.2 |  |
| E6.4 | When the output device rate differs from the file rate, resample the preview blocks with the Resampler in crates/core/src/resample.rs before the ring. | New test e6_preview_resamples_44100_to_48000 in apps/wavcraft/src/effect_preview.rs: a 1 kHz sine in a 44.1 kHz file read from a 48 kHz ring has 2000 zero crossings per 48000 frames, within 2. | medium | state | script | E6.1 |  |
| E6.5 | By-ear check on the Linux handhelds. | Loop a 5 s speech selection in the Amplify and Parametric EQ dialogs and move a slider. I must hear the change with no noticeable lag and no clicks at the loop point. Write the result into docs/checklists/E6.md. | easy | docs | hand, me, linux | E6.3, E6.4 |  |
| E6.6 | By-ear check on the Windows build machine. | Same check as on Linux: loop 5 s of speech in the Amplify and Parametric EQ dialogs, move a slider, hear the change at once with no clicks. Add a Windows section to docs/checklists/E6.md. | easy | docs | hand, me, windows | E6.5 |  |
| E6.7 | By-ear check on the 8 GB Mac. | Same check as on Linux: loop 5 s of speech in the Amplify and Parametric EQ dialogs, move a slider, hear the change at once with no clicks. Add a Mac section to docs/checklists/E6.md. | easy | docs | hand, me, mac | E6.5 |  |

#### G4: Loudness analysis (built, check not recorded)

- **On main:** crates/core/src/stats.rs and crates/core/src/loudness.rs give peak, true peak, RMS, integrated LUFS, DC offset and clipped samples. Shown in apps/wavcraft/src/statspanel.rs and the statistics window in apps/wavcraft/src/meters_ui.rs. Control verbs STATS and LOUDNESS.
- **Done check:** Values match the reference meter.
- **Recorded so far:** not recorded
- **Item issue:** #39, #927

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| G4.1 | Read STATS and LOUDNESS for the kit tone, pink noise and long files through the control channel, and measure the same files with ffmpeg (ebur128 with peak=true, and astats). | A test compares peak, RMS and DC offset with ffmpeg on the kit files within the stated tolerances. It does not compare clipped counts with ffmpeg. It asserts the clipped count is 0 on the kit files. The clipped rule itself stays covered by crates/core/tests/stats.rs. | easy | docs | script, linux | none |  |

#### G3: Match Loudness panel (partial)

- **On main:** crates/core/src/match_loudness.rs holds Target, the presets and MatchResult with before and after values. The tests are in crates/core/tests/match_loudness.rs. The panel is apps/wavcraft/src/match_ui.rs and the MATCH control verb is in apps/wavcraft/src/app.rs.
- **Done check:** At -16 LUFS / -1 dBTP: within 0.2 LU and true peak at or below -1.0 dBTP, by my meter and an independent reference meter. Presets for my targets.
- **Recorded so far:** The test done_when_minus_16_and_minus_1_dbtp_lands_within_0_2_lu_and_peak_at_or_below_minus_1 in crates/core/tests/match_loudness.rs passes.
- **Item issue:** #38

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| G3.1 | Add a Match To choice to the target: ITU-R BS.1770-3 Loudness, EBU R128 Loudness, True Peak Amplitude. The true peak mode gains the file so its true peak lands on the target. | New file crates/core/tests/g3_match_to.rs. g3_true_peak_amplitude_mode_lands_peak_on_target: a 1 kHz sine at -12 dBTP matched to -3 dBTP has true peak -3.0 within 0.1. g3_ebu_r128_matches_bs1770_result: both modes give equal output at -16 LUFS. | medium | pure-fn | script | none |  |
| G3.2 | Replace the fixed Match to label in the panel with a combo of the three choices. | New test g3_panel_offers_three_match_to_choices in apps/wavcraft/src/match_ui.rs: the combo lists exactly ITU-R BS.1770-3 Loudness, EBU R128 Loudness and True Peak Amplitude, with the first as default. | easy | ui | script | G3.1 |  |
| G3.3 | After Run, show a table of integrated LUFS, loudness range and true peak, with a Before and an After column, in place of the one-line status message. | New test g3_result_rows_show_before_and_after in apps/wavcraft/src/match_ui.rs: for a result with before -23.0 LUFS and after -16.0 LUFS the rows function returns three rows, and the first reads -23.0 LUFS and -16.0 LUFS. | easy | ui | script | none |  |
| G3.4 | Add a command that toggles the Match Loudness panel and bind the key to it. Panel toggles exist today only as control names, with no command or action a key can reach. | crates/core/tests/shortcuts.rs, new test alt_5_toggles_match_loudness_panel: the default key map resolves Alt+5 to the action that toggles panel id match, and no other action uses that key. | medium | wiring | script | none |  |
| G3.5 | Match the long kit file to -16 LUFS / -1 dBTP with the MATCH control verb, save it, and measure the saved file with ffmpeg's ebur128 filter with peak=true. | ffmpeg must report integrated loudness between -16.2 and -15.8 LUFS and true peak at or below -1.0 dBTP. Write both numbers, the ffmpeg version, the commit and the date into docs/baseline.md under a heading G3 independent meter. | easy | docs | script, linux | none |  |

#### G1: Capture Noise Print (partial)

- **On main:** crates/core/src/noise.rs captures a print from a selection of at least 4096 frames and keeps it in memory. Shift+P is bound in crates/core/src/shortcuts.rs. Control verb NOISE_PRINT in app.rs. Tests in crates/core/tests/noise.rs.
- **Done check:** A print can be captured from as little as 0.5 s, saved to a file and loaded back.
- **Recorded so far:** not recorded
- **Item issue:** #36

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| G1.1 | Add NoisePrint::save(path) and NoisePrint::load(path) with a magic string and a version number at the start of the file. | New file crates/core/tests/g1_noise_print_file.rs. g1_print_from_half_second_round_trips: capture from 24000 frames at 48 kHz, save, load, sample_rate is 48000 and every density value is bit-equal. g1_load_rejects_bad_magic_and_version: both give Err. | easy | pure-fn | script | none |  |
| G1.2 | Add Save Noise Print and Load Noise Print to the Effects menu with a file dialog, and the control verbs NOISE_PRINT SAVE <path> and NOISE_PRINT LOAD <path>. | New test g1_control_save_then_load_restores_current_print in apps/wavcraft/src/app.rs: capture, NOISE_PRINT SAVE to a temp file, clear the print, NOISE_PRINT LOAD, then noise::current() equals the captured print. LOAD of a missing file answers ERR. | medium | wiring | script | G1.1 |  |

#### G2: Noise Reduction (process) (partial)

- **On main:** crates/core/src/effects/noise_reduction.rs with Reduce By, Noise Reduction, two smoothing controls, decay, FFT size 1024 to 8192 and Output Noise Only, shown in the generic effect dialog. Tests in crates/core/tests/noise.rs.
- **Done check:** A second listener and I hear no underwater swirl at the suggested settings on the dialogue test file, and a 1-hour stereo file finishes in under 60 s on the CPU of the Windows build machine.
- **Recorded so far:** not recorded
- **Item issue:** #37

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| G2.1 | Set the defaults to the row's values: Noise Reduction 50 %, FFT size choices 512, 1024, 2048, 4096, 8192, 16384 with 4096 as default. | New file crates/core/tests/g2_noise_reduction.rs, test g2_defaults_match_audition: reduction 50, reduce_db 16, decay 65, noise_only off, fft_size 4096, and the choice list is exactly 512, 1024, 2048, 4096, 8192, 16384. A 512 and a 16384 run both finish without error. | easy | data-shape | script | none |  |
| G2.2 | Confirm the four ranges marked unconfirmed in docs/v1/audition-behaviour.md: Spectral Decay Rate, Smoothing, Precision Factor and Transition Width. Also write down the range for Noise Print Snapshots, which has none. | docs/v1/audition-behaviour.md lists a minimum, a maximum and the default (1, 7, 0 dB, 4000) for each of the four controls, with the Audition version and the date. The word unconfirmed is gone from those lines. | easy | docs | hand, me | none |  |
| G2.3 | Add the four advanced controls as parameters, mapped onto my internals as docs/v1/architecture.md describes: Smoothing, Precision Factor, Transition Width, Noise Print Snapshots. | Core tests, one per new control. Spectral Decay Rate 0 % leaves a longer tail after a burst than 100 %. Smoothing 0 gives a wider spread of bin gains than the maximum. Transition Width 0 dB gives a hard gate (each bin gain is either 1.0 or the floor). Each new control at its default keeps the residual of the kit noise file within 0.5 dB of today's value. | medium | pure-fn | script | G2.1, G2.2 |  |
| G2.4 | Add a command that opens the Noise Reduction dialog and bind Ctrl+Shift+P to it. Today a key runs the effect at once through run_action. Only the menu code opens a dialog. | A row in the shortcuts! macro in crates/core/tests/shortcuts.rs resolves Ctrl+Shift+P to the new command. A headless app test sends the key, the Noise Reduction dialog is open, and the audio is unchanged. | medium | wiring | script | none |  |
| G2.5 | Add a function that returns the threshold curve in dB per frequency bin for a print and a set of parameters, the same values the processor uses. | crates/core/tests/g2_noise_reduction.rs, new test g2_threshold_curve_matches_processor: for a flat print at -60 dB the curve has one value per bin, all equal, and equals the threshold the processor computes for the same settings within 1e-6. | easy | pure-fn | script | none |  |
| G2.6 | Give Noise Reduction its own dialog: the generic controls plus a graph of the print with the threshold curve, and a Select Entire File button. | New test g2_dialog_has_graph_and_select_entire_file in apps/wavcraft/src/noise_reduction_ui.rs: with a print set, the dialog's graph data has one print line and one threshold line of equal length. Select Entire File sets the selection to 0..total frames. | medium | ui | script | G2.5, E6.3 |  |
| G2.7 | Add a new bench binary in the testkit crate for noise reduction, following the one-binary-per-measure pattern, not a mode of wavcraft-bench. | New test g2_measure_reports_floor_drop_and_speech_change in crates/testkit/src/lib.rs: on a synthetic kit (tone bursts plus white noise at 20 dB SNR, Reduce By 16 dB, Noise Reduction 100 %) it reports a floor drop of 14 to 18 dB and a speech change under 1 dB. | medium | pure-fn | script | G2.1 |  |
| G2.8 | I record 60 s of clean speech and 60 s of room noise on a real mic, and build the kit with the testkit's speech option. | The kit folder holds speech-clean.wav, speech-plus-noise-snr10.wav and speech-plus-noise-snr20.wav. Where the two source recordings are kept is written into docs/checklists/G2.md. | easy | docs | hand, me | none |  |
| G2.9 | Run nr-measure on the real kit at 10 and 20 dB SNR, and time a 1-hour stereo file, on the Windows build machine. Record all numbers. | docs/baseline.md gets a heading G2 noise reduction with: floor drop within 2 dB of 16 dB at both SNRs, speech change under 1 dB at both, and seconds for the 1-hour stereo file under 60, with the commit and the date. | easy | docs | script, windows | G2.3, G2.7, G2.8, my word on whether the floor is measured at Noise Reduction 50 % or 100 % |  |
| G2.10 | By-ear check on a real episode at the default settings. | The second listener and I play the dialogue test file before and after at the suggested settings. Neither of us hears underwater swirl. I record the date and the settings used. | easy | docs | hand, me | G2.6, G2.9 |  |
| G2.11 | Make the noise reduction bench time a full 1-hour stereo 48 kHz run at the suggested settings and print one line nr_1h_stereo_seconds=<n>. | A testkit test runs the bench on a 10 s file with a scale flag and finds a line matching nr_1h_stereo_seconds=<number>. The real 1-hour run is the timing check on the Windows build machine. | easy | wiring | script | G2.7 |  |

#### F1: Effects Rack panel with Apply (partial)

- **On main:** crates/core/src/rack.rs (slots with power, input and output gain, Mix, apply as one undo step) and apps/wavcraft/src/rack_panel.rs (floating window). Tests: crates/core/tests/rack.rs, rack_tail.rs, rack_reset.rs, using only invert, gain and de-esser.
- **Done check:** A chain of 3 effects applied to a 1-hour file in under 30 s on the Windows build machine (measured). Bypassed slots cost nothing.
- **Recorded so far:** not recorded
- **Item issue:** #33

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| F1.1 | Give the filter and dynamics effects a stream() that keeps state across blocks. Leave DC Offset out: it needs the mean of the whole range, so it goes with the effects the rack refuses in step 3. | New test f1_rack_filters_match_offline_apply in crates/core/tests/f1_rack_stream.rs: for each of the five, a one-slot rack on 2 s of pink noise equals effects::apply within 1e-6, at block sizes 64 and 1024. | medium | pure-fn | script | none |  |
| F1.2 | Give Dynamics (gate, expander, compressor, limiter) a real Effect::stream that keeps its envelope state across blocks. | New test f1_rack_dynamics_matches_offline_apply in crates/core/tests/f1_rack_stream.rs: a one-slot Dynamics rack on a 2 s tone burst (-6 dBFS then -40 dBFS) equals effects::apply within 1e-6 at block sizes 64 and 1024. | medium | pure-fn | script | none |  |
| F1.3 | Make effects::open_stream return an error for an effect that cannot stream, instead of passing audio through unchanged. List only streamable effects in the rack slot menu. | New tests in crates/core/tests/f1_rack_stream.rs. f1_open_stream_refuses_effects_that_cannot_stream: normalize gives Err. f1_every_slot_menu_effect_changes_audio_like_offline: every effect the slot menu lists equals effects::apply within 1e-6 on 1 s of pink noise. | medium | pure-fn | script | F1.1, F1.2 |  |
| F1.4 | Add the lock test for a real 3-effect chain and for bypass cost. | A unit test inside crates/core/src/rack.rs builds a RackProcessor from a rack with one powered slot and one powered-off slot. It asserts the processor holds exactly 1 stream. With both slots off it holds 0 streams and the output equals the input bit for bit. | easy | pure-fn | script | F1.1, F1.2 |  |
| F1.5 | Register the rack in the panel REGISTRY in crates/core/src/panels.rs with id rack, and draw rack_panel as a dockable panel instead of a floating window. | A unit test in the tests module of crates/core/src/panels.rs asserts REGISTRY has an entry with id rack, and that toggling window.toggle.rack flips its open flag. | medium | ui | script | none |  |
| F1.6 | Add a new bench binary wavcraft-rack-bench in the testkit crate, following the one-binary-per-measure pattern. It runs a full rack over generated audio and prints one line with the time per block. The step 7 check runs this binary by name. | Command wavcraft-bench rack prints one line of the form rack_3fx_1h_seconds=<number>. New test f1_bench_rack_reports_seconds in crates/testkit/src/lib.rs runs it on a 1 s file and parses a number above 0. | medium | wiring | script | F1.4 |  |
| F1.7 | Run the rack bench on the Windows build machine and record the number. | Run wavcraft-bench rack from a release build. The printed rack_3fx_1h_seconds must be under 30. Write the number, the commit and the date into docs/baseline.md under a heading F1 rack apply. | easy | docs | script, windows | F1.6 |  |

#### F2: Built-in processors from SoundCraft dsp (partial)

- **On main:** crates/core/src/effects/eq.rs (parametric EQ, high-pass and other biquads) and effects/deesser.rs use soundcraft_dsp. effects/dynamics.rs (gate, compressor, limiter) is my own code. Tests: crates/core/tests/filters.rs, dynamics.rs, deesser.rs.
- **Done check:** Each one makes the same output as its SoundCraft test vectors.
- **Recorded so far:** not recorded
- **Item issue:** #34

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| F2.1 | Add a test file that feeds the same input and settings to my effect and to soundcraft_dsp directly, for parametric EQ, high-pass and de-esser. | A core test runs Parametric EQ with a non-zero gain on each band (for example +6 dB at 1 kHz) and compares it with the shared biquad at the same settings, within 1e-6. The de-esser is compared in Multiband mode only, at freq 6000, threshold -30, range 10. | easy | pure-fn | script | none |  |
| F2.2 | Write into the F2 row of docs/v1/v1.md that gate, compressor and limiter are my own code by my ruling of 2026-10-26, checked by crates/core/tests/dynamics.rs. | The F2 row names crates/core/tests/dynamics.rs as the check for the three dynamics processors and f2_soundcraft_vectors.rs for the rest. No other row changes in the diff. | easy | docs | script | F2.1, my word that the dynamics tests stand in for SoundCraft vectors |  |

#### F3: Presets for effects and racks (partial)

- **On main:** crates/core/src/presets.rs saves, loads, lists and deletes effect and rack presets as files in the config folder, with unit tests in the same file. apps/wavcraft/src/rack_panel.rs has rack presets and a Presets menu per slot.
- **Done check:** Save, quit, reopen, load: same settings.
- **Recorded so far:** not recorded
- **Item issue:** #35

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| F3.2 | Add a Presets menu (save as, load, delete) to the effect dialog, so effects run from the Effects menu have presets too. | A headless app test hands the effect dialog a Presets handle on a temp folder. It saves a preset named Test with Amplify at 3 dB, builds a second app on the same folder, loads Test, and the dialog shows 3 dB. Nothing is written to the real config folder. | medium | ui | script | none |  |
| F3.3 | Hand check of save, quit, reopen, load on the Linux handhelds. | Save an Amplify preset at +6 dB and a 3-slot rack preset. Quit, reopen, load both. I must see +6 dB and the same three slots. Copy the preset folder to a second machine and load there. Write the result into docs/checklists/F3.md. | easy | docs | hand, linux | F3.2 |  |

#### A4: Convert Sample Type and dither (partial)

- **On main:** crates/core/src/dither.rs has the dither types (None, Triangular, Rectangular) and the Ditherer. The convert dialog in apps/wavcraft/src/convert_ui.rs has one Dither combo. A second dither path in crates/core/src/save.rs (DitherOptions, Tpdf, save_wav_dithered) sits behind a save hook that is off by default and has no caller in the app. Its tests are in crates/core/tests/a4_dither.rs.
- **Done check:** A 24-bit to 16-bit convert with TPDF on has no harmonic distortion on a -90 dBFS tone, measured on a spectrum, and with dither off the result is bit-exact truncation.
- **Recorded so far:** The test t4_sixteen_to_twentyfour_to_sixteen_is_bit_identical in crates/core/tests/save.rs passes, with the a4a, a4b and a4c tests in crates/core/tests/a4_dither.rs.
- **Item issue:** #12

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| A4.1 | Bind F11 to open Convert Sample Type, beside Shift+T. | crates/core/tests/shortcuts.rs, new test f11_opens_convert_sample_type: the default key map resolves both F11 and Shift+T to the convert sample type action, and no other action uses F11. | easy | wiring | script | none |  |
| A4.2 | In the convert dialog replace the single Dither combo with a Dithering checkbox (off) and a Dither Type combo that lists Triangular first. Keep Noise Shaping with None as default. | New test a4_dialog_defaults_dither_off_triangular_first in apps/wavcraft/src/convert_ui.rs: a new dialog has dithering false, the type list is Triangular then Rectangular, and shaping is None. Ticking the box gives DitherSettings with type Triangular. | easy | ui | script | none |  |
| A4.3 | Store the chosen dither type and shaping in the settings, not only an on/off flag, and have the convert dialog read and write it. | New test a4_dither_on_is_remembered in apps/wavcraft/src/convert_ui.rs: confirm the dialog with dithering on and Triangular, save settings to a temp file, load them, and a new dialog opens with dithering on and Triangular. | easy | state | script | A4.2 |  |
| A4.4 | Add the same three dither controls to the sample type choice in Save As, used when the saved depth is lower than the file's depth. | New test a4_save_as_16_bit_uses_the_dither_choice in crates/core/tests/dither.rs: a -90 dBFS 1 kHz float sine saved as 16-bit with dither off equals round(x * 32767) per sample. With Triangular on, the saved samples differ and are not limited to -1, 0, 1. | medium | wiring | script | A4.2 |  |
| A4.5 | Keep one dither implementation. Point a4b_minus_90_dbfs_sine_gives_noise_not_steps at the Ditherer the app uses, then delete DitherOptions, Tpdf, quantize_dithered and save_wav_dithered from save.rs. | After the second path is removed, crates/core/tests/a4_dither.rs is ported to the dither.rs path: the a4a, a4b and a4c checks still exist and pass, including dither surviving a save and load. A search finds no DitherOptions, Tpdf or save_wav_dithered in crates/core. | medium | pure-fn | script | none |  |

### Stage 4: Feel

B1 and B2 are not in v1.md's order of work; I place them here with the rest of section B.

**Done when** I work without hunting for things.

**Remains:**

#### B1: Waveform, amplitude ruler, vertical zoom (built, check not recorded)

- **On main:** Peak pyramid and waveform_columns in crates/core/src/document.rs, AmpScale and AmpZoom in crates/core/src/view.rs, ruler and zoom actions in apps/wavcraft/src/app.rs. Tests live inside document.rs and view.rs; scripted check tools/alpha/f19.py.
- **Done check:** Lanes hold 60 fps while scrolling a 3-hour file zoomed in on the Windows build machine (frame times measured). A one-sample click shows at full zoom-out.
- **Recorded so far:** not recorded
- **Item issue:** #15, #69

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| B1.1 | Add a test that one full-scale sample in a silent file shows at full zoom-out. | crates/core/tests/b1_click_visible.rs, fn a_one_sample_click_shows_at_full_zoom_out: 4,800,000 silent mono frames, frame 2,345,678 = 1.0. waveform_columns(0, 0.0, 4800000.0, 1000): column 488 max is 1.0, all others (0.0, 0.0). With 100 columns: column 48. | easy | pure-fn | script | none |  |
| B1.2 | Add a pure function that sums up a list of frame times: count, median, worst, and how many are over 16.7 ms. | crates/core/tests/b1_frame_stats.rs, fn summary_of_five_frames: input [10.0, 12.0, 14.0, 16.0, 100.0] gives count 5, median_ms 14.0, worst_ms 100.0, over_budget 1. fn an_empty_list_is_all_zero. | easy | pure-fn | script | none |  |
| B1.3 | Add a frame timer and a FRAMETIMES control verb. The timer measures the work inside one frame (from the start of logic to the end of ui), not the gap between two frames. FRAMETIMES reset clears it; FRAMETIMES answers with count, median, p95 and max in milliseconds. | Control channel on a running app: 'FRAMETIMES reset' answers 'OK n=0'; after 'NAV 0 48000' and one second, 'FRAMETIMES' answers 'OK n=<1 or more> median_ms=<x> worst_ms=<x> over=<n>'. The verb is listed in the header of apps/wavcraft/src/control.rs. | easy | wiring | script | B1.2 |  |
| B1.4 | Add a script that makes a 3-hour 48 kHz stereo WAV, opens it, zooms to 2 seconds, scrolls for 10 seconds with NAV, and prints FRAMETIMES. | Keep the draft's scroll run, and state the pass rule on work time: over the scroll, FRAMETIMES reports p95 at or under 16.7 ms of work per frame and count at least 300. The script prints one PASS line per check and exits 0. | easy | wiring | script | B1.3 |  |
| B1.5 | Run the frame-time script on the Windows build machine and write the numbers down. | On the Windows build machine, release build: tools/alpha/b1-frames.py prints PASS. The printed line, the date and the commit hash are written into docs/checklists/B1.md and a 'B1 frame times' section of docs/baseline.md. | easy | docs | script, windows | B1.1, B1.4 |  |

#### B2: Spectral Frequency Display toggle, keys, refresh (partial)

- **On main:** Drawn display, divider drag and double-click toggle (apps/wavcraft/src/app/spectral.rs), resolution keys Ctrl+Shift+Up/Down, three scales through the View menu and the SPECTRAL verb, incremental refresh (crates/core/src/spectrogram.rs). Tests: crates/core/tests/incremental_overviews.rs, crates/core/tests/shortcuts.rs.
- **Done check:** Toggle on Shift+D and F9, the four log/linear keys, a resizable split, and the picture right within 0.5 s of an edit on a 3-hour file (measured).
- **Recorded so far:** not recorded
- **Item issue:** #16, #68

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| B2.1 | Add action view.spectral.toggle ('Spectral Frequency Display'), bind Shift+D and F9, and make the app flip the 'spectral' panel when it runs. | crates/core/tests/shortcuts.rs: new tests spectral_toggle_shift_d ('Shift+D') and spectral_toggle_f9 ('F9') resolve to ActionId::ViewSpectralToggle; the_default_table_has_no_chord_on_two_commands still passes. Running app: 'ACTION view.spectral.toggle' flips spectral= in STATE from 1 to 0. | easy | wiring | script | none |  |
| B2.2 | Add Scale::more_log and Scale::more_linear in freq_axis.rs: one step along Linear, LogPartial, LogFull, stopping at each end. | crates/core/tests/b2_scale_steps.rs, fn the_scale_steps_and_stops: Linear.more_log() is LogPartial; LogPartial.more_log() is LogFull; LogFull.more_log() is LogFull; LogFull.more_linear() is LogPartial; Linear.more_linear() is Linear. | easy | pure-fn | script | none |  |
| B2.3 | Add four actions (more log, more linear, fully log, fully linear) bound to Ctrl+Alt+Up, Ctrl+Alt+Down, Ctrl+Alt+PageUp, Ctrl+Alt+PageDown; the app sets the frequency axis scale. | Read the scale from the freqband= field of STATE (freqband=<lo>-<hi>:<lin\|log_partial\|log>), or from the answer of 'SPECTRAL scale ...'. Plain 'SPECTRAL' does not carry the scale. | medium | wiring | script | B2.2 |  |
| B2.4 | Put the spectral toggle on the toolbar, after the zoom buttons, with the glyph text 'Spectral'. | apps/wavcraft/src/menus.rs, new test the_toolbar_has_the_spectral_toggle: toolbar_item("view.spectral.toggle") is Some, and glyph() of it is "Spectral". | easy | ui | script | B2.1 |  |
| B2.5 | Time each edit: record when the document changes and when the first spectral picture of the new state is shown; report it in the SPECTRAL verb. | Running app with a file open: after 'ACTION edit.delete' on a selection, 'SPECTRAL' answers with 'edit_to_picture_ms=pending' and then, once drawn, 'edit_to_picture_ms=<whole number>'. Before any edit it reads 'edit_to_picture_ms=none'. | medium | state | script | none |  |
| B2.6 | Add a script that opens a 3-hour 48 kHz stereo WAV, deletes one minute from the middle, and polls SPECTRAL until edit_to_picture_ms is a number. | python3 tools/alpha/b2-refresh.py --exe <built app> prints 'edit_to_picture_ms=<n>' for three edits (delete, undo, redo) and 'PASS' when every n <= 500, else 'FAIL'; exit code matches. | easy | wiring | script | B2.5 |  |
| B2.7 | Run the refresh script on the Windows build machine and write the numbers down. | On the Windows build machine, release build: tools/alpha/b2-refresh.py prints PASS. The three numbers, date and commit hash go into docs/checklists/B2.md and a 'B2 edit to picture' section of docs/baseline.md. | easy | docs | script, windows | B2.6 |  |
| B2.8 | Hand check of the keys, the toolbar button and the split on one desktop. | On the Windows build machine: Shift+D, F9 and the toolbar button each hide and show the display; double-click on the divider does the same; dragging the divider resizes; the four Ctrl+Alt keys change the frequency ruler. What was seen is written into docs/checklists/B2.md. | easy | docs | hand, windows | B2.1, B2.3, B2.4 |  |

#### B3: Zoom keys and the navigator bar (built, check not recorded)

- **On main:** Viewport limits and navigator maths in crates/core/src/view.rs (16 pixels per sample at most), navigator and zoom actions in apps/wavcraft/src/app.rs, keys =, -, Shift+S, Ctrl+\, Alt+=, Alt+- in crates/core/src/shortcuts.rs. Tests: view.rs, crates/core/tests/shortcuts.rs; scripts tools/alpha/f19.py, f69.py.
- **Done check:** Every shortcut works; dragging the navigator scrolls; zoom in stops at one sample per few pixels.
- **Recorded so far:** not recorded
- **Item issue:** #17

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| B3.1 | Run the two zoom scripts against a release build and record the result. | tools/alpha/f19.sh and tools/alpha/f69.sh each print only PASS lines, end with a JSON line holding "ok": true, and exit 0. Those last lines, the date and the commit hash go into the checklist file. | easy | docs | script, linux | B3.2 |  |
| B3.2 | Hand check of the zoom keys and the navigator drag with a real keyboard and mouse. | On the Windows build machine with a 10-minute file: =, -, Shift+S, Ctrl+\, Alt+= and Alt+- each change the view; dragging the navigator handle scrolls; holding = stops with single samples about 16 pixels apart. Seen results go into docs/checklists/B3.md. | easy | docs | hand, windows | none |  |
| B3.3 | Extend f19.py to press the zoom keys through the KEY verb and to check the zoom-in stop in the running app. | tools/alpha/f19.py gains checks: 'KEY =' narrows STATE view=, 'KEY -' widens it, and the other zoom chords of the row each change view= or the vertical zoom as their action does. After 'KEY =' is sent 40 times, HOVER reports frames_per_px=0.0625 (16 pixels per sample) and one more 'KEY =' leaves view= unchanged. Each prints PASS. | easy | wiring | script | none |  |

#### B4: Levels meter: peak, RMS, hold, clip light (partial)

- **On main:** Peak bars, hold tick, clip light, 60 dB range and colours from -18 and -6 in apps/wavcraft/src/meters_ui.rs over crates/core/src/levels.rs. RmsMeter in crates/core/src/levels/rms.rs and levels_scale in crates/core/src/meters/ exist but the app does not use them. Tests: peak_modes.rs, rms_reading.rs, b4_levels_scale.rs.
- **Done check:** Matches the dsp crate's meter on a test tone to 0.1 dB. Shows while playing.
- **Recorded so far:** not recorded
- **Item issue:** #18

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| B4.1 | Add the 0.1 dB test: Levels against the dsp crate's PeakMeter on a 1 kHz tone at -20 dBFS. | Say how the tone is fed: Levels takes block peaks, so the test calls Levels::update([p, p], 0.01) once per 480-frame block of a 1 kHz sine of amplitude 0.1 at 48 kHz, where p is that block's peak. After 1 s, reading(0).level_db is -20.0 within 0.1. This is the value to assert; comparing with the dsp peak meter alone proves nothing. | easy | pure-fn | script | none |  |
| B4.2 | Make the Levels panel use meters::levels_scale (bar_fraction, zone) and delete its own AMBER_FROM and RED_FROM numbers. | grep finds no AMBER_FROM or RED_FROM in apps/wavcraft/src/meters_ui.rs. New unit test in meters_ui.rs, fn the_bar_uses_the_core_scale: a 600-point lane puts -30 dB at x 300, -18 dB at x 420 and -6 dB at x 540. | easy | ui | script | none |  |
| B4.3 | Let LevelTap carry energy as well as peaks: record_energy(ch, sum_of_squares, frames) and take_energy(), lock-free. | crates/core/tests/b4_rms_tap.rs, fn energy_adds_up_and_take_starts_over: record_energy(0, 120.0, 480) twice, then take_energy()[0] is (240.0, 960) and channel 1 is (0.0, 0); a second take_energy()[0] is (0.0, 0). | easy | data-shape | script | none |  |
| B4.4 | Add RmsLevels::feed_energy(ch, sum_of_squares, frames) so the 300 ms window can be fed from block totals. | crates/core/tests/b4_rms_tap.rs, fn a_block_of_half_power_reads_minus_three_db: RmsLevels::new(48000.0, 300.0); feed_energy(0, 7200.0, 14400) gives reading(0).rms_db within 0.01 of -3.01; then feed_energy(0, 0.0, 14400) reads the floor. | easy | pure-fn | script | none |  |
| B4.5 | Record energy at the player's two meter points, feed RmsLevels in MetersUi each frame, and add l_rms and r_rms to the METERS verb. | New script tools/alpha/b4-rms.py: play a 10 s stereo 1 kHz sine of amplitude 0.1; while playing, METERS reports l and r within 0.1 of -20.0 and l_rms and r_rms within 0.1 of -23.0; two seconds after STOP both rms values read -60.0. | medium | state | script | B4.3, B4.4 |  |
| B4.6 | Draw the RMS level as a solid inner bar inside each peak bar, with the RMS number beside the peak number. | Unit test in apps/wavcraft/src/meters_ui.rs, fn the_rms_bar_sits_inside_the_peak_bar: for a 600-point lane 20 points high, rms -30 dB gives a rect from x 0 to x 300 whose height is under 20; rms -60 dB gives no rect. | easy | ui | script | B4.2, B4.5 |  |
| B4.7 | Run the RMS script on a release build, look at the meter while it plays, and record both. | Keep the draft's run, but say the script prints PASS lines (not OK) and exits 0, and record the last line. | easy | docs | hand, linux | B4.1, B4.6 |  |
| B4.8 | Test that the RMS reading matches the dsp crate's RMS meter on a steady tone. | crates/core/tests/b4_meter_matches_dsp.rs, fn rms_matches_the_dsp_meter_on_a_tone: 3 s of a 1 kHz sine of amplitude 0.1 at 48 kHz fed to RmsLevels::new(48000, 300) and to soundcraft_dsp::meter::RmsMeter::new(48000.0). Both read -23.01 dB within 0.1, and they differ from each other by at most 0.1. | easy | pure-fn | script | none |  |

#### B5: Selection/View panel and time formats (built, check not recorded)

- **On main:** selection_view_panel and time_box in apps/wavcraft/src/app.rs, panel 'selection' in crates/core/src/panels.rs, TimeFormat (decimal, samples, timecode and more) in crates/core/src/view.rs. Tests in view.rs; scripts tools/alpha/f66.py and f83.py.
- **Done check:** Typing a start and end selects exactly those samples.
- **Recorded so far:** crates/core/src/view.rs tests typed_selection_values_move_the_selection, typed_times_in_each_format, what_is_formatted_parses_back_to_the_same_text
- **Item issue:** #19

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| B5.1 | Add a control verb that types a value into a Selection/View box through the same code the panel uses, and a script check on it. | New check in tools/alpha/f66.py on a 48 kHz file with selection 100-1000: 'SELBOX start 150' gives selection=150-1000; 'SELBOX end 900' gives 150-900; 'SELBOX duration 500' gives 150-650; 'SELBOX start 1,000' in the samples format gives start 1000. Each prints PASS. | easy | wiring | script | none |  |

#### C4: Spectral selection: Marquee and Lasso (partial)

- **On main:** Marquee tool on key E, drag and move, Band and SpectralSelection with a raised-cosine mask (crates/core/src/view.rs), drawn band and GESTURE marquee verb in apps/wavcraft/src/app.rs. The Lasso is only a greyed entry in the Tools panel list (apps/wavcraft/src/menus.rs). Tests: crates/core/tests/spectral_selection.rs.
- **Done check:** Amplify -20 dB on a marquee round a 1 kHz tone in pink noise lowers the tone 20 +/- 1 dB, noise outside changes under 0.5 dB; feathered edges, no click.
- **Recorded so far:** not recorded
- **Item issue:** #23

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| C4.1 | Add blend_in_band: given untouched and processed samples and a mask function of (frame, Hz), return untouched plus the masked STFT of their difference. Uses crates/core/src/stft.rs. | crates/core/tests/c4_spectral_blend.rs, fn the_tone_drops_and_the_noise_stays: 5 s at 48 kHz, seeded pink noise at -30 dBFS plus 1 kHz at -12 dBFS; processed is input times 0.1; box 2.0-3.0 s, 900-1100 Hz. 1 kHz level drops 20 +/- 1 dB; 2-8 kHz power changes under 0.5 dB; samples before 1.9 s are bit-identical. | medium | pure-fn | script | none |  |
| C4.2 | Give a new marquee a default feather (DEFAULT_FEATHER_HZ 50.0, DEFAULT_FEATHER_MS 20.0 in view.rs) and prove there is no click at the box edge. | crates/core/tests/c4_spectral_blend.rs, fn the_default_feather_leaves_no_click: full-scale 1 kHz sine at 48 kHz, box 1.0-2.0 s over 0-24000 Hz, processed times 0.1; the largest sample-to-sample step of the result is at most 0.1355. Also asserts both default constants are above 0. | easy | pure-fn | script | C4.1 |  |
| C4.3 | Add Editor::run_effect_in_band: run the effect on the time range widened by the feather, blend it through the selection's mask, and push one History step named after the effect. | crates/core/tests/c4_effect_in_band.rs, fn amplify_in_a_band_is_one_undo_step: on the step 1 signal, Amplify -20 dB in box 2.0-3.0 s, 900-1100 Hz lowers 1 kHz by 20 +/- 1 dB; History grows by one step labelled 'Amplify'; undo gives back bit-identical samples. | medium | state | script | C4.1 |  |
| C4.4 | In the app, send every effect run (Effects menu, effect dialog, EFFECT verb) through run_effect_in_band when a band is live. | Same script, but the effect line is 'EFFECT amplify left_db=-20' (link stays on by default, so both channels drop). The script prints PASS per check and exits 0. | medium | wiring | script | C4.2, C4.3 |  |
| C4.5 | Add LassoSelection in view.rs: a closed outline of (frame, Hz) points with the same feather fields, and mask(frame, hz): 1 inside, raised-cosine fade outside. | crates/core/tests/c4_lasso_mask.rs: fn a_rectangle_lasso_equals_the_marquee (corners (1000,900),(2000,900),(2000,1100),(1000,1100), no feather: (1500,1000) is 1.0, (500,1000) and (1500,2000) are 0.0); fn a_triangle_knows_inside_from_outside (corners (0,0),(1000,0),(0,1000): (100,100) is 1.0, (900,900) is 0.0). | easy | pure-fn | script | none |  |
| C4.6 | Add the Lasso tool as an action bound to D: ActionId::ToolLasso ('tool.lasso', 'Lasso Selection Tool'), Tool::Lasso, and the Tools-panel entry made live. Add its row to LISTED in crates/core/tests/shortcuts.rs and to the fixed list in the locked help test file (a test-author change). | crates/core/tests/shortcuts.rs: new test tool_lasso_d ('D' resolves to ActionId::ToolLasso; Ctrl+D still resolves to DeselectAll). apps/wavcraft/src/menus.rs test: toolbar_item("tool.lasso") is Some. Running app: 'ACTION tool.lasso' then STATE shows tool=lasso. | easy | wiring | script | none |  |
| C4.7 | Lasso drag on the spectral display: collect the outline, draw it, set the time selection to its extent, add 'GESTURE lasso', and use its mask for effects. | As drafted, with 'EFFECT amplify left_db=-20' in place of 'db=-20', and PASS lines. | medium | ui | script | C4.4, C4.5, C4.6 |  |
| C4.8 | Run the band script on a release build and record it, with a look at the drawn box and outline. | On a Linux handheld: tools/alpha/c4-band.sh prints only PASS lines and exits 0. Dragging with E draws a box and with D an outline on the spectral display. The script's last line, what was seen, date and commit go into docs/checklists/C4.md. | easy | docs | hand, linux | C4.7 |  |

#### C6: Scrub by dragging the playhead (built, check not recorded)

- **On main:** Playhead drag scrub in apps/wavcraft/src/app.rs (run_scrub, scrub_to, scrub_end), ScrubTracker and grain plan in crates/core/src/transport.rs, grain fade in apps/wavcraft/src/player.rs, SCRUB control verb. Tests: transport.rs scrub_speed_follows_the_drag, player.rs a_grain_fades_in_and_out; script tools/alpha/f36.py.
- **Done check:** Heard while dragging; no glitch on release, by ear.
- **Recorded so far:** not recorded
- **Item issue:** #25

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| C6.1 | By-ear check on the Windows build machine: drag the playhead through speech and listen, then release and listen. | With a speech file: sound is heard while dragging forward and backward, slow and fast; ten releases give no click and no stuck sound. What I heard, the date and commit hash are written into docs/checklists/C6.md under 'Windows'. | easy | docs | hand, me, windows | none |  |
| C6.2 | The same by-ear check on a Linux handheld. | With a speech file: sound is heard while dragging forward and backward, slow and fast; ten releases give no click and no stuck sound. What I heard, the date and commit hash are written into docs/checklists/C6.md under 'Linux'. | easy | docs | hand, me, linux | none |  |

#### I1: Audition's default layout (partial)

- **On main:** Fifteen docked panels with default sides in crates/core/src/panels.rs (REGISTRY, Layout::default), built-in Default workspace in crates/core/src/workspaces.rs, Levels strip along the bottom in apps/wavcraft/src/app.rs. Test: panels.rs the_default_layout_follows_the_registry.
- **Done check:** I, shown it cold, find Noise Reduction, Match Loudness, Markers and History without being told where.
- **Recorded so far:** not recorded
- **Item issue:** #44

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I1.1 | Change the Markers panel's default side to Left, placed after Files. | crates/core/src/panels.rs test the_default_layout_follows_the_registry asserts visible(Left) is ["tools", "files", "markers"] and visible(Right) is ["history", "playlist", "match"]. The workspaces.rs tests still pass. | easy | data-shape | script | none |  |
| I1.2 | Split RackPanel::show into the window shell and a new body(ui, playing, can_apply) that draws the rack's contents. No behaviour change. | The app builds, crates/core/tests/rack.rs passes, and a scripted run that drives the RACK verb (tools/alpha/f103.py) still prints only PASS lines. The rack window looks the same by eye. | easy | ui | script | none |  |
| I1.3 | Register the Effects Rack as panel 'rack' (default side Right, before History), draw it through panel_body, drop the floating window and its open flag, and make Effects > Effects Rack… show and front the panel instead. | panels.rs test asserts visible(Right) is ["rack", "history", "playlist", "match"]. Running app: PANELS answers with 'rack=1' and 'rack' first in zone.right; 'PANEL close rack' gives 'rack=0'. grep finds no 'rack.open' in apps/wavcraft/src. | medium | ui | script | I1.1, I1.2 |  |
| I1.4 | Cold check by me on the Windows build machine with a fresh settings folder. | Fresh start, one file open, no hints: I find Noise Reduction, Match Loudness, Markers and History. For each I write found or not found and where I looked first into docs/checklists/I1.md, with the date and commit hash. | easy | docs | hand, me, windows | I1.3 |  |

#### I2: Panels show, hide, resize; layout remembered (partial)

- **On main:** PanelSet and Layout (hide, move, divider and corner drags, floats, encode and decode) in crates/core/src/panels.rs, settings keys hidden_panels, panel_layout, zone_sizes and workspace, Workspaces::reset and the Window > Workspace menu. Tests: panels.rs, crates/core/tests/settings.rs, workspace_keys.rs.
- **Done check:** Survives a restart.
- **Recorded so far:** not recorded
- **Item issue:** #45

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I2.1 | Add a settings key show_levels (default on) to crates/core/src/settings.rs, in KEYS, load and save. | crates/core/tests/settings.rs, new fn hiding_the_levels_strip_survives_a_settings_save_and_load: default Settings has show_levels true; set false, save, load: show_levels is false and every other key is unchanged. | easy | data-shape | script | none |  |
| I2.2 | Read show_levels into MetersUi at start, save it when Window > Levels is ticked or unticked, add 'METERS show <0\|1>' to the control channel, and report 'levels=<0\|1>' in the PANELS answer. | Running app: 'METERS show 0', QUIT, start again with the same home folder: PANELS answers with levels=0. 'METERS show 1' then gives levels=1. Added as two PASS checks to tools/alpha/i2-restart.py. | easy | wiring | script | I2.1, I2.3 |  |
| I2.3 | Add a restart script: hide History, drag the left divider, quit, start again with the same home folder, read the layout back, then reset the workspace. | python3 tools/alpha/i2-restart.py --exe <built app>: run 1 reads size.left from PANELS, then 'PANEL close history' and 'DIVIDER drag left 40 0', then QUIT; run 2 PANELS shows history=0 and the same size.left as after the drag; after 'WORKSPACE reset' history=1 and size.left is the first value. Prints PASS per check and 'N/N checks passed', exit 0. | easy | wiring | script | none |  |
| I2.4 | Run the restart script on a release build and record it, with one hand pass of the menu item. | On the Windows build machine: tools/alpha/i2-restart.py exits 0. By hand: hide History, quit, reopen, History is still hidden; Window > Workspace > Reset to Saved Layout brings it back. Both results, date and commit go into docs/checklists/I2.md. | easy | docs | hand, windows | I2.2, I2.3, I1.3 |  |

#### I3: Audition's default shortcuts and Help list (partial)

- **On main:** One table, BINDINGS in crates/core/src/shortcuts.rs, read by menus and keys; 54 action rows. crates/core/tests/shortcuts.rs has one test per row and fails when a row has none. Edit > Keyboard Shortcuts editor in apps/wavcraft/src/shortcut_editor.rs.
- **Done check:** One test per shortcut, through the same action table the menus use.
- **Recorded so far:** crates/core/tests/shortcuts.rs (every_row_of_the_table_has_a_test_above) for rows in the table; nothing for the Help list or the missing keys
- **Item issue:** #46

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I3.1 | Write the real shortcut_help_rows and shortcut_help_content in app.rs (action table order, one row per bound chord) and make the test file import them instead of its copies. | apps/wavcraft/src/i3_shortcut_help_tests.rs starts with 'use super::{pressed_commands, shortcut_help_content, shortcut_help_rows};' and defines neither. cargo test -p wavcraft i3_ passes, including i3_help_rows_are_in_menu_order_with_no_unbound_or_extra_rows (54 rows, first is edit.undo, Undo, Ctrl+Z). No unimplemented!() left in app.rs. | easy | pure-fn | script | none |  |
| I3.2 | Add Help > Keyboard Shortcuts: a menu item that opens a scrolling window drawing shortcut_help_content, and a control verb 'HELP shortcuts' that opens it. | Running app: 'HELP shortcuts' answers 'OK rows=54' (the count of action rows in BINDINGS at that commit) and the window titled 'Keyboard Shortcuts' is open with 'Undo' and 'Ctrl+Z' on its first line. The verb is listed in apps/wavcraft/src/control.rs. | easy | ui | script | I3.1 |  |
| I3.3 | Bind the second keys Audition has: F8 and Shift+8 to Add Marker, F11 to Convert Sample Type. | crates/core/tests/shortcuts.rs: new tests marker_add_f8, marker_add_shift_8 and convert_sample_type_f11 resolve to ActionId::MarkerAdd, MarkerAdd and ConvertSampleType; the_default_table_has_no_chord_on_two_commands passes. The Help list test gains the three rows. | easy | wiring | script | I3.1 |  |
| I3.4 | Move J, K and L into the table: three actions (transport.shuttle.back, .stop, .forward) bound to J, K, L; the app's shuttle code runs from the commands. Holding K to crawl stays. | crates/core/tests/shortcuts.rs: new tests shuttle_back_j, shuttle_stop_k, shuttle_forward_l. Running app with a file: 'ACTION transport.shuttle.forward' gives playing=1 in PLAYBACK; 'ACTION transport.shuttle.stop' gives playing=0. grep finds no 'Key::J \| Key::K \| Key::L' press filter in app.rs. | medium | wiring | script | I3.1 |  |
| I3.5 | Move the backtick into the table: action window.maximizePanel ('Maximize Panel Under Pointer') bound to Backtick; the app runs it on the hovered panel. | crates/core/tests/shortcuts.rs: new test maximize_panel_backtick ('`' resolves to the new action). Running app: 'PANEL maximize history' still works; grep finds no consume_key(.., Key::Backtick) in apps/wavcraft/src/app.rs. | easy | wiring | script | I3.1 |  |
| I3.6 | Add one test that walks a fixed list of every key named in docs/v1/audition-behaviour.md's table and in the rows of docs/v1/v1.md and checks each resolves in the default keymap. | crates/core/tests/i3_audition_keys.rs, fn every_key_in_the_v1_list_is_in_the_table: the list includes Space, Home, End, Alt+ArrowLeft, Shift+X, J, K, L, =, -, M, F8, Shift+R, Ctrl+R, Shift+T, F11, Shift+P, Shift+D, F9, Ctrl+Alt+PageUp, E, D, `, Ctrl+Shift+P, Alt+5; each resolves. | easy | pure-fn | script | I3.3, I3.4, I3.5, B2.1, B2.3, C4.6, C5.2, G2.4, G3.4 |  |

#### I4: Never lose work: autosave and recovery (built, check not recorded)

- **On main:** Journal, Autosaver, recover and discard in crates/core/src/autosave.rs; start-up scan and the Recover / Discard / Later window in apps/wavcraft/src/recovery.rs. Tests: crates/core/tests/autosave.rs, 990_autosave_markers.rs, journal_frame.rs, sweep.rs; kill script tools/alpha/f10.py.
- **Done check:** Kill the app mid-edit, restart, recover, and the samples match the last autosave (test).
- **Recorded so far:** crates/core/tests/autosave.rs a_crashed_session_comes_back_with_its_document_and_history (samples, crash simulated by dropping the store); the real kill run is not recorded
- **Item issue:** #47

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I4.1 | Make f10.py run on Windows as well: kill the first run with a call that works on both systems, stop requiring the Linux-only return code, and compare the recovered samples with an export taken before the kill. | The script prints 'PASS run 2: the recovered samples equal the export taken before the kill' and its last line reads 'N/N checks passed' with both numbers equal; exit 0. | easy | wiring | script | none |  |
| I4.2 | Run the kill script on a release build on Linux and record it. | On a Linux handheld: tools/alpha/f10.sh ends with every check OK and exit code 0. The sample-compare line, the last line, date and commit hash go into docs/checklists/I4.md under 'Linux'. | easy | docs | script, linux | I4.1 |  |
| I4.3 | Hand check of the offer window on the Windows build machine. | Open a file, make three edits, wait 10 seconds, end the app from Task Manager, start it: a window offers Recover, Discard and Later. Recover brings back the three History steps. Repeat with Later: the offer shows again next start. Results go into docs/checklists/I4.md under 'Windows'. | easy | docs | hand, windows | none |  |
| I4.4 | Run the crash-and-recover script on the Windows build machine with a release build and record it. | python tools/alpha/f10.py --exe <built app> prints only PASS lines and 'N/N checks passed', exit 0. The last line, date and commit hash go into docs/checklists/I4.md under 'Windows'. | easy | docs | hand, windows | I4.1 |  |

#### A5: Files panel with several files open (built, check not recorded)

- **On main:** Files panel rows (name, unsaved mark, length, rate, channels) in apps/wavcraft/src/files.rs, document order in apps/wavcraft/src/doclist.rs, per-file DocSlot (editor, view, selection, saved state) in apps/wavcraft/src/app.rs. Tests: files.rs, doclist.rs switching_swaps_and_keeps_the_order; script tools/alpha/f04.py.
- **Done check:** Two files open at once; switching between them keeps each one's zoom, selection and undo history.
- **Recorded so far:** not recorded
- **Item issue:** #13

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| A5.1 | Extend f04.py: zoom file A, switch to B and back, and check A's view is unchanged; do the same for B. | tools/alpha/f04.py gains checks 'A keeps its zoom after the round trip' and 'B keeps its zoom': with the View sync options off (STATE sync=-), after 'ACTION view.zoomIn' twice on A, STATE view= is equal before 'SWITCH 1' and after 'SWITCH 0'; B's view is still its whole length. Both print PASS. | easy | wiring | script | none |  |
| A5.2 | Run the script on a release build and record it, with one look at the Files panel. | On a Linux handheld: tools/alpha/f04.sh prints only PASS lines and ends with 'N/N checks passed', exit 0. In the Files panel each open file shows sample rate, channels, length and a * when edited; clicking a row shows that file. Output line, what was seen, date and commit go into docs/checklists/A5.md. | easy | docs | hand, linux | A5.1 |  |

### Stage 4b: Episode Check, transcript and faces

My ruling, 2026-11-01: "Into v1". Order: M3 and M5 first; K1, K2 and L1 can start alongside; K3 to K5 after stage 3's tools exist; then M2, M1 and M4. M4's last steps and M2.8 follow M1, since M1 needs the map, the findings panel and the transcript panel. M1.12 (on a Mac) is taken in stage 5.

**Done when** the Podcast face's user cleans a real episode in it without help.

**Complete:**

- **L0 Speech-to-text library, model and licence.** The choice is written in docs/v1/architecture.md, section "Transcription (L0, 2026-11-03)": candle runner, Whisper base multilingual. The weights' licence is in THIRD-PARTY-NOTICES.txt, written by tools/notices.py (tools/test_notices.py). Done check: v1.md has no L0 row. Section L's intro: a pure-Rust MIT or Apache runner; the weights' licence checked and written into the notices file before the model ships. Recorded: docs/v1/architecture.md (Transcription section) and THIRD-PARTY-NOTICES.txt ("Speech-to-text model weights"). Open issue to close: #1056.

**Remains:**

#### M3: One theme from named design values (partial)

- **On main:** Colours are done: apps/wavcraft/src/theme.rs has Palette with DARK and LIGHT, pal() and set_dark, with tests no_inline_colour_values_in_ui_sources and scanner_flags_inline_colour_values. Sizes and spacing are not: theme.rs holds no sizes; about 150 inline size literals remain in the UI files.
- **Done check:** A test finds no inline colour values in the UI code. Switching dark and light changes every panel.
- **Recorded so far:** apps/wavcraft/src/theme.rs test no_inline_colour_values_in_ui_sources covers the colour half; the every-panel switch is not recorded
- **Item issue:** none

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| M3.1 | Add a Metrics struct and metrics() to theme.rs with named sizes: nav_height 34, ruler_height 22, freq_ruler_width 54, amp_ruler_width 46, handle_grab 6, control_height 18, text_small 11, space_xs 2, space_s 4, space_m 8, space_l 12. | Unit test metrics_have_the_default_values in apps/wavcraft/src/theme.rs: metrics().nav_height is 34.0, ruler_height 22.0, handle_grab 6.0, space_m 8.0, and every field is above 0. | easy | data-shape | script | none |  |
| M3.2 | Replace the four size constants at the top of app.rs, the navigator's 6.0 edge width and GRAB_PX in markers_panel.rs with theme::metrics() fields. | grep -n 'const NAV_HEIGHT\\|const RULER_HEIGHT\\|const FREQ_RULER_WIDTH\\|const AMP_RULER_WIDTH' apps/wavcraft/src/app.rs and grep -n 'const GRAB_PX' apps/wavcraft/src/markers_panel.rs print nothing; cargo test -p wavcraft passes. | easy | rename | script | M3.1 |  |
| M3.3 | Add inline_sizes(src) to the theme.rs test module: it lists lines with a non-zero float literal as an argument of add_space, vec2, min_size, default_size, default_width, desired_width, FontId:: or .size( calls. | Unit test scanner_flags_inline_sizes in theme.rs: "ui.add_space(6.0)" and "vec2(640.0, 480.0)" are flagged; "ui.add_space(theme::metrics().space_s)" and "vec2(0.0, 0.0)" are not; a line inside a #[cfg(test)] module is not. | easy | pure-fn | script | M3.1 |  |
| M3.4 | Move every inline size and spacing literal in app.rs onto theme::metrics(), adding named fields where a value has no name yet. | Test moved_files_have_no_inline_sizes in apps/wavcraft/src/theme.rs (not ignored) runs inline_sizes on a list of moved files, here app.rs, and reports 0 lines; cargo test -p wavcraft passes; in a debug build at 100 percent scale the navigator is 34 px high and the time ruler 22 px, as before. | medium | rename | script | M3.2, M3.3 |  |
| M3.5 | Move every inline size and spacing literal in multitrack_ui.rs (HEADER_W 230, ROW_H 64, RULER_H 20 and about 31 call sites) onto theme::metrics(). | multitrack_ui.rs is added to the list in moved_files_have_no_inline_sizes (apps/wavcraft/src/theme.rs) and the test passes; grep -n 'const HEADER_W\\|const ROW_H\\|const RULER_H' on that file prints nothing; cargo test -p wavcraft passes. | medium | rename | script | M3.3, M3.4 |  |
| M3.6 | Move the inline sizes in five panel files onto theme::metrics(): freq_panel, meters_ui, shortcut_editor (with UNIT 27.0), convert_ui, mixpaste. | The five files are added to the list in moved_files_have_no_inline_sizes (apps/wavcraft/src/theme.rs) and the test passes; grep -n 'const UNIT' apps/wavcraft/src/shortcut_editor.rs prints nothing; cargo test -p wavcraft passes. | medium | rename | script | M3.3, M3.4 |  |
| M3.7 | Turn the size scan into a standing test over every UI source file except theme.rs, the same way the colour scan runs (ui_sources), replacing moved_files_have_no_inline_sizes. | Test no_inline_sizes_in_ui_sources in apps/wavcraft/src/theme.rs passes. Adding ui.add_space(6.0) to any panel file makes it fail and name the file and line (shown in the PR). | easy | pure-fn | script | M3.4, M3.5, M3.6, M3.9, M3.10 |  |
| M3.8 | Hand check on the Linux handheld: switch Preferences, Appearance, Theme between Dark and Light and look at every panel and dialog. | In a debug build open each Window menu panel, Preferences, an effect dialog, the Effects Rack and About in Light, then Dark. Each must change ground and text and stay readable. Write each name with pass or fail, date and commit into docs/checklists/M3.md. | easy | docs | hand | none |  |
| M3.9 | Move the inline sizes in five more panel files onto theme::metrics(): batch_panel, statspanel, recovery, rack_panel, effect_dialog. | The five files are added to the list in moved_files_have_no_inline_sizes (apps/wavcraft/src/theme.rs) and the test passes; cargo test -p wavcraft passes. | medium | rename | script | M3.3, M3.4 |  |
| M3.10 | Move the inline sizes in the last five UI files onto theme::metrics(): mixer, diagnostics_panel, cd_ui, markers_panel and app/spectral.rs (with its HANDLE 8.0 constant). | The five files are added to the list in moved_files_have_no_inline_sizes (apps/wavcraft/src/theme.rs) and the test passes; grep -n 'const HANDLE' apps/wavcraft/src/app/spectral.rs prints nothing; cargo test -p wavcraft passes. | medium | rename | script | M3.3, M3.4 |  |

#### M5: Out-of-v1 panels hidden in release (partial)

- **On main:** apps/wavcraft/src/v1.rs (RELEASE, Feature, offered, tool_offered_in) gates Multitrack, CD Layout, Video, Batch, Mix Paste, Diagnostics and Spot Healing in app.rs. crates/core/src/panels.rs OUT_OF_V1 hides the Frequency Analysis panel. Two tests in v1.rs, one in panels.rs.
- **Done check:** A release build's menus and panels list nothing from the Deliberately not in v1 table.
- **Recorded so far:** not recorded
- **Item issue:** #1051

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| M5.1 | In crates/core/src/effects.rs add OUT_OF_V1 = ["dehummer", "dereverb"] and listed(release), which yields (index, effect) pairs with registry indexes unchanged and skips those ids in a release build. | New file crates/core/tests/m5_effects_listed.rs, test a_release_build_lists_no_dehummer_or_dereverb: names from listed(true) include "Noise Reduction" and exclude "DeHummer" and "DeReverb"; listed(false) has all().len() entries; each index equals its position in all(). | easy | pure-fn | script | none |  |
| M5.2 | Add v1::effect_rows_in(release), the action rows the Effects menu draws, and rack_panel::slot_effects(release), the built-in effects the rack slot picker lists. Both leave out the effects::OUT_OF_V1 ids in a release build. Use them in effects_menu() and in the slot picker. | Test a_release_effects_menu_and_rack_list_no_dehummer_or_dereverb in apps/wavcraft/src/v1.rs: the menu texts of effect_rows_in(true) include Noise Reduction and Reverse and exclude DeHummer and DeReverb; effect_rows_in(false) includes both; the names from rack_panel::slot_effects(true) exclude both and slot_effects(false) has effects::all().len() entries. | easy | wiring | script | M5.1 |  |
| M5.3 | Add v1::save_formats_in(release, first) and use it for the Save As dialog's filters: release offers WAV, AIFF, FLAC; debug adds Ogg Vorbis. In a release build an Ogg document's own format is not put first either. Opening .ogg files is unchanged. | Unit test a_release_build_offers_no_ogg_save in apps/wavcraft/src/v1.rs: save_formats_in(true, Wav) is [Wav, Aiff, Flac]; save_formats_in(true, Vorbis) is [Wav, Aiff, Flac]; save_formats_in(false, Vorbis) is [Vorbis, Wav, Aiff, Flac]; save_formats_in(false, Wav) is [Wav, Aiff, Flac, Vorbis]. | easy | wiring | script | none |  |
| M5.4 | Gate the Window menu's Mixer panel checkbox and the mixer panel itself behind v1::offered(Feature::Multitrack). | grep -n -B2 '"Mixer panel"' apps/wavcraft/src/app.rs shows the v1::offered(Feature::Multitrack) guard; the panel block reads show_mixer && v1::offered(Feature::Multitrack); a debug build still shows the checkbox. | easy | wiring | script | none |  |
| M5.5 | Add menus::bar_labels(release): every label the menu bar, the Window list, the Tools panel, the rack slot picker, the Keyboard Shortcuts list and the Save As filters draw, built from the same tables and gate functions that menu() uses. | Test release_menus_list_nothing_deliberately_not_in_v1 in apps/wavcraft/src/v1.rs: bar_labels(true) has no entry containing Multitrack, CD Layout, Video, Batch Process, Mix Paste, Diagnostics, DeClipper, DeClicker, Click Remover, DeHummer, DeReverb, Frequency Analysis, Spot Healing, Mixer panel, VST3 or Ogg; bar_labels(false) has an entry for each of them. | hard | wiring | script | M5.2, M5.3, M5.4, M5.7, M5.8 |  |
| M5.6 | Hand check on the Linux handheld: build a release binary, open every menu and the Window list, and compare with the Deliberately not in v1 table. | With a release build, write every entry of File, Edit, Window, Effects, Generate, View and Help, the Tools panel, the rack slot picker and the Keyboard Shortcuts list into docs/checklists/M5.md, with date and commit. None may match a row of the table. Any match gets its own bug issue. | easy | docs | hand | M5.5 |  |
| M5.7 | Add Feature::Plugins to v1.rs. In the Effects Rack hide the Scan VST3 button, the found count and the VST3 plugins list of the slot picker when the feature is not offered. | Tests in apps/wavcraft/src/v1.rs: Feature::ALL has 7 entries and offered_in(true, Feature::Plugins) is false. Test a_release_rack_lists_no_plugin in apps/wavcraft/src/rack_panel.rs: with one found plugin named Demo given to vst3::set_found, slot_plugins(true) is empty and slot_plugins(false) is ["vst3:Demo"]. | easy | wiring | script | none |  |
| M5.8 | Add v1::command_offered_in(release, name) and use it to leave the out-of-v1 commands out of the Keyboard Shortcuts dialog's list in a release build. | Unit test a_release_shortcut_list_has_no_out_of_v1_command in apps/wavcraft/src/v1.rs: command_offered_in(true, n) is false for diagnostics.declip, diagnostics.declick and diagnostics.autoClickRemover and true for edit.delete; with release false all four are true. | easy | wiring | script | none |  |

#### K1: Run Episode Check with progress and Cancel (not started)

- **On main:** Nothing of K1 itself. Only the detectors' entry point `scan` in crates/core/src/check/mod.rs, with no cancel flag and no progress. Nothing in apps/wavcraft calls it. No action, menu entry or toolbar button.
- **Done check:** 1-hour 48 kHz stereo file scans in under 60 s on the 64 GB Linux tablet, adds at most 200 MB of memory (both measured); Cancel stops within 1 s.
- **Recorded so far:** not recorded
- **Item issue:** #1049

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| K1.1 | Add `check::scan_with(channels, cfg, cancel, progress) -> Option<Report>`. The main chunk loop checks cancel and reports 0 to 0.5. `scan` becomes a wrapper. | New tests in crates/core/tests/episode_check.rs: `a_cancelled_scan_returns_none` (flag set first gives None, progress never called), `progress_rises_to_one` (non-decreasing, last value 1.0), `scan_with_matches_scan` (same Report on the planted-fault signal). | easy | pure-fn | script | none |  |
| K1.2 | Check cancel and report progress (0.5 to 1.0) once per window inside the DC, hum, plosive and drift passes. | New test `cancel_after_the_main_loop_stops_the_passes` in crates/core/tests/episode_check.rs: on the 240 s signal the callback sets cancel at the first progress above 0.5; the result is None and at most 1 more progress call follows. | medium | pure-fn | script | K1.1 |  |
| K1.3 | Add `check::scan_doc(state, cancel, progress) -> io::Result<Option<Report>>`: one StoreChannel per channel, `Config::new` at the document's rate. | New file crates/core/tests/episode_check_doc.rs, test `scan_doc_matches_the_slice_scan`: the 20 s planted-fault signal written to a temp store gives a Report equal to the slice scan's. | easy | wiring | script | K1.1 |  |
| K1.4 | Add `ActionId::EpisodeCheck`, name `check.run`, menu text "Run Episode Check", enabled when the document has frames. | Unit test `episode_check_is_in_the_table` in crates/core/src/actions.rs: `by_name("check.run")` has menu text "Run Episode Check"; enabled is false on an empty document and true on a 1-frame one. | easy | data-shape | script | none |  |
| K1.5 | Add the worker state `CheckRun` (start, cancel, poll, progress, report) in a new app module, following statspanel.rs. | Unit tests in apps/wavcraft/src/check_panel.rs: `a_run_ends_with_the_report` (20 s signal clipped at 3.0 s gives exactly 1 Clipping finding starting at frame 144000) and `a_cancelled_run_keeps_no_report`. | medium | state | script | K1.2, K1.3 |  |
| K1.6 | Wire the action in the app: start the run, draw a progress bar with Cancel, and mark the report stale when the history moves. Add control command `CHECK run\|cancel\|state` (state carries `stale=<0\|1>`). | New script `python3 tools/alpha/k1.py --exe <release build>`: opens a generated 20 s WAV clipped at 3.0 s, sends `ACTION check.run`, polls `CHECK state` to `running=0 progress=100 findings>=1 stale=0`. Then it opens a generated 30-minute WAV, sends `ACTION check.run` and `CHECK cancel` in the next command, and `CHECK state` must show `running=0 findings=0` within 1 s of the cancel. | medium | wiring | script | K1.4, K1.5 |  |
| K1.7 | Add the Effects menu entry and a toolbar button for `check.run`, glyph "Check", after the zoom buttons. | New tests in apps/wavcraft/src/menus.rs. `toolbar_has_episode_check`: `toolbar_item("check.run")` is Some with label "Run Episode Check", `glyph` gives "Check", toolbar names stay unique. `effects_menu_has_episode_check`: a new plain-data list `menus::effects_extra()` holds the `check.run` item, and `effects_menu` in app.rs draws its entries from that list. | easy | ui | script | K1.6 |  |
| K1.8 | Add an ignored bench test that scans 3600 s of generated 48 kHz stereo from a temp store and prints time, peak memory growth and cancel latency. | `cargo test -p wavcraft-core --release --test episode_check_bench -- --ignored --nocapture` runs `one_hour_stereo_scan_is_fast_and_small`; it prints three numbers and asserts under 60 s, under 200 MB (Linux VmHWM growth), cancel under 1 s. | easy | pure-fn | script | K1.2, K1.3 |  |
| K1.9 | Run the bench on the 64 GB Linux tablet and record the result. | Hand check on Linux: run the K1.8 command in the build container. Write the scan seconds, memory growth in MB, cancel latency and the commit hash into docs/checklists/K1.md. All three must be inside the limits. | easy | docs | script, linux | K1.8 |  |

#### K2: Episode Check detectors (seven kinds) (partial)

- **On main:** All seven detectors are in crates/core/src/check/ (mod.rs, drift.rs, hum.rs, plosive.rs). Tests in crates/core/tests/episode_check.rs cover clipping, true peak, DC, dead air and drift at 50 ms. Mains hum edges are only good to 341 ms.
- **Done check:** One test file with one of each fault planted: every finding within 50 ms of where it was planted; a clean speech file reports none. A test checks both.
- **Recorded so far:** crates/core/tests/episode_check.rs (the test is the check, but hum is asserted to 16,384 frames, not 2,400)
- **Item issue:** none

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| K2.1 | Refine each hum region's start and end with a short sliding measure of the fundamental (4,096-frame window, 480-frame hop) around the coarse edge. | In crates/core/tests/episode_check.rs, `mains_hum_is_reported_with_its_frequency` replaces the 16_384 bounds with `assert_near` (2,400 frames) on start and end, for 50 Hz and 60 Hz. `clean_speech_reports_nothing` still passes. | medium | pure-fn | script | none |  |
| K2.2 | Assert the plosive finding's end as well as its start; fix plosive.rs only if the assertion fails. | `a_40_hz_burst_in_a_gap_is_a_plosive` also calls `assert_near("plosive end", plosive[0].end, at + 3_840)` (the 80 ms burst ends 3,840 frames after its start; limit 2,400 frames). | easy | pure-fn | script | none |  |
| K2.3 | Add one 240 s two-channel test signal carrying all seven faults at fixed times and assert each one. | New test `one_file_with_every_fault_reports_each_within_50_ms` on `speechlike_for(.., 240.0, 0.0)` for both channels: clip at 3.0 s, true peak 7 to 8 s on the other channel, DC 12 to 14 s, dead air 16 to 19.5 s, 50 Hz hum 30 to 40 s, +9 LU drift 101 to 113 s, and a plosive at 65.55 s placed inside a stretch 65.5 to 66.0 s that is set to the noise floor first. After dropping the true peak finding that overlaps the clip on the clipped channel, there is exactly one finding per kind, each start and end within 2,400 frames. | easy | pure-fn | script | K2.1, K2.2 |  |
| K2.4 | I run the K2.5 test on a real clean speech file of mine (kept outside the repo) and record the result. | Hand check on Linux: run ignored test `a_real_clean_speech_file_reports_nothing` in crates/core/tests/episode_check.rs. It must report 0 findings. Write the clip length, the result and the commit hash into docs/checklists/K2.md. | easy | docs | hand, me, linux | K2.1, K2.5 |  |
| K2.5 | Add ignored test `a_real_clean_speech_file_reports_nothing`: it reads a WAV whose path is in WAVCRAFT_CLEAN_SPEECH, scans it with `Config::new` at the file's rate and asserts 0 findings. | `cargo test -p wavcraft-core --test episode_check -- --ignored a_real_clean_speech_file_reports_nothing` passes when the variable points at a WAV written from the test file's own `clean()` signal, and fails when it points at a copy with 20 ms of full scale at 3.0 s. | easy | pure-fn | script | none |  |

#### K3: Findings panel, map pins, next and previous keys (not started)

- **On main:** Nothing. No findings panel in apps/wavcraft/src, no `findings` entry in crates/core/src/panels.rs, no next or previous finding action. The Episode Map (M2) does not exist either.
- **Done check:** Clicking a row selects exactly the finding's range.
- **Recorded so far:** not recorded
- **Item issue:** #1053

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| K3.1 | Add pure `check::list`: `rows(&Report, rate)` giving kind label, time text (m:ss.mmm), severity label and range; `next_after(frame)`, `prev_before(frame)`, both wrapping. | Unit tests in crates/core/src/check/list.rs: `row_range_is_exactly_the_finding`; `labels_for_every_kind` (Clipping, True peak, DC offset, Dead air, Loudness drift, Mains hum, Plosive); `time_of_frame_144000_at_48k_is_0_03_000`; `next_and_previous_wrap` on findings at 100, 200, 300. | easy | pure-fn | script | none |  |
| K3.2 | Register panel `findings` (title "Episode Check", right dock) and actions `check.next` (F8) and `check.prev` (Shift+F8). | Unit tests: in panels.rs `by_id("findings")` is Some and closable, `PanelSet::for_build(true).report()` ends with `match=1 findings=1`, and the existing right-zone tests are updated to list `findings` after `match`. In shortcuts.rs `Keymap::new().check()` is Ok, `resolve_text("F8")` is the next-finding command and `resolve_text("Shift+F8")` the previous one. The `PANELS` doc line in control.rs gains ` findings=<0\|1>` after `match=<0\|1>` and `tools/checks/1003-panels-doc-lists-tools.sh` passes. | medium | data-shape | script | K1.4 |  |
| K3.3 | App logic: choosing a finding sets the selection to its range and plays it. Add control commands `CHECK list` and `CHECK click <row>`. | New script `python3 tools/alpha/k3.py --exe <release build>`: file clipped at frames 144000 to 144480; after the check, `CHECK list` gives one row per finding with its kind and range; the script takes the index i of the Clipping row, sends `CHECK click i`, and `STATE` shows `selection=144000-144480` and `playing=1`. | medium | wiring | script | K1.6, K3.1 |  |
| K3.4 | Wire `check.next` and `check.prev`: move to the next or previous finding from the cursor and select and play it. | tools/alpha/k3.py gains a case with clips at 3.0 s and 9.0 s. It reads the row ranges from `CHECK list` (four rows: a Clipping and a True peak finding per clip). With the cursor at frame 0, `KEY F8` gives row 0's range in `selection=`, a second `KEY F8` row 1's, `KEY Shift+F8` row 0's again, and one more `KEY Shift+F8` wraps to the last row. | easy | wiring | script | K3.2, K3.3 |  |
| K3.5 | Draw the findings panel: one row per finding (type, time, severity); clicking a row runs the K3.3 logic; the current row is highlighted. | Headless egui test in apps/wavcraft/src/check_panel.rs, `clicking_a_row_returns_its_index` (pattern of issue1000_tests.rs): a pointer click on the second row's rectangle returns Some(1). In tools/alpha/k3.py, `ACTION window.hide.findings` then `PANELS` shows `findings=0`, and `ACTION window.show.findings` shows `findings=1`. | medium | ui | script | K3.2, K3.3 |  |
| K3.6 | Add pure `check::list::pin_x(frame, total_frames, width_px)` for placing a pin on the map strip. | Unit test `pin_x_maps_frame_to_pixel` in crates/core/src/check/list.rs: frame 0 gives 0; frame 500 of 1000 at width 200 gives 100; the last frame gives 199. | easy | pure-fn | script | K3.1 |  |
| K3.7 | Make a click on a finding pin on the Episode Map (drawn by M2.7) run the K3.3 logic for that finding. | tools/alpha/k3.py: with the two planted clips, the control line `CHECK pin <n>` (the code a click on pin n runs) answers OK, and SELECTION then equals that row's start and end frames from `CHECK list`, for each of the 4 pins. | easy | ui | script | K3.3, M2.7 |  |
| K3.8 | Look at the panel and pins once in the running app on the 64 GB Linux tablet. | Hand check on Linux: run the check on a file with two clips. I must see four rows (a Clipping and a True peak row per clip) with type, time and severity, four pins on the map in two pairs, and a click on each row and each pin selecting and playing its range. Result written into docs/checklists/K3.md. | easy | docs | hand, linux | K3.5, K3.7 |  |

#### K4: Suggested fix per finding, with Before/After (not started)

- **On main:** Nothing. No mapping from a finding to a fix. The tools exist: effects amplify, normalize, high_pass, dc_offset in crates/core/src/effects/, and delete and silence in crates/core/src/doc/edit.rs.
- **Done check:** Applying a fix is one History step; Undo restores the samples bit-identical; Before/After switches with no gap, by ear.
- **Recorded so far:** not recorded
- **Item issue:** #1054

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| K4.1 | Keep the sign on a LoudnessDrift finding's value: positive when louder than the file, negative when quieter. Update the doc comment on `Finding::value`. | In crates/core/tests/episode_check.rs: the existing +9 LU test still asserts a value between 6.0 and 10.0; new test `a_quiet_stretch_reports_negative_drift` plants a 9 LU quieter section and asserts between -10.0 and -6.0. | easy | pure-fn | script | none |  |
| K4.2 | I decide which existing tool fixes Clipping and True peak (the row names none) and write the answer down. | docs/v1/questions-and-answers.md has a dated entry naming one tool and its setting for Clipping and one for True peak. | easy | docs | hand, me | none |  |
| K4.3 | Add pure `check::fix::suggest(&Finding) -> Option<Fix>` (label, range, channel mask, operation) for drift, dead air, hum, plosive and DC. | Unit test `every_named_kind_has_one_fix` in crates/core/src/check/fix.rs: drift +8.0 gives effect `amplify` at -8.0 dB; drift -8.0 gives `amplify` at +8.0 dB; dead air gives Delete of its range; hum gives `noise_reduction`; plosive gives `high_pass` with cutoff 120 Hz and slope 24 dB per octave; DC gives `dc_offset`. Clipping gives None. | easy | pure-fn | script | K4.1 |  |
| K4.4 | Add `check::fix::apply(doc, &Fix, crossfades, cancel) -> Result<Edit, EditError>` using the existing effect and delete paths. | New file crates/core/tests/episode_check_fix.rs, test `each_fix_is_one_step_and_undo_is_bit_identical` for drift, dead air, plosive and DC: History steps grow by exactly 1; a rescan no longer reports the finding; after undo every sample equals the original by `to_bits`. | medium | state | script | K4.3 |  |
| K4.5 | Apply the hum fix through Noise Reduction: capture a noise print from the quietest 0.5 s inside the finding with `noise::capture`, make it current with `noise::set_current`, then run `noise_reduction` on the finding's range. Add it to the fix test. | crates/core/tests/episode_check_fix.rs, test `hum_fix_is_one_step_and_undo_is_bit_identical`: 60 Hz hum planted 30 to 40 s; one History step; the rescan reports no MainsHum; undo is bit-identical. | medium | state | script | K4.4 |  |
| K4.6 | Add the Clipping and True peak fixes that I chose to `suggest` and `apply`. | `every_named_kind_has_one_fix` gains the two rows with the tool and setting from questions-and-answers.md; the fix test covers both kinds with one History step and a bit-identical undo. | easy | pure-fn | script | K4.2, K4.4 |  |
| K4.7 | Add pure `check::list::after_fix(report, row, removed_range)`: drop the fixed row and shift later rows when the fix deleted audio. | Unit test `a_delete_fix_shifts_later_rows` in crates/core/src/check/list.rs: rows at 100000, 768000 and 2000000; fixing the middle one by deleting 768000 to 936000 leaves rows at 100000 and 1832000. | easy | pure-fn | script | K3.1 |  |
| K4.8 | Add a Fix button on each findings row and control command `CHECK fix <row>`; the list is marked stale when history moves any other way. | New script `python3 tools/alpha/k4.py --exe <release build>`: file with DC planted 12 to 14 s; `CHECK fix 0` answers with `steps` up by 1 and `findings` down by 1; `ACTION edit.undo` then `CHECK state` shows `stale=1`. | medium | ui | script | K3.5, K4.4, K4.7 |  |
| K4.9 | Add a Before/After toggle that swaps the player between the step before the fix and the current step at the playhead. A Delete fix restarts at the range start. | tools/alpha/k4.py with the silent test sink (WAVCRAFT_NO_AUDIO=1). After the DC fix, while the fixed range plays, `CHECK ab before` answers `OK source=before`; `PLAYBACK` still shows `playing=1`, `position` has not gone back, and `underruns` is unchanged. `CHECK ab after` the same. After a dead air Delete fix of 768000 to 936000, `CHECK ab before` while playing gives `PLAYBACK` `position` between 768000 and 768000 plus one second. | medium | state | script | K4.8 |  |
| K4.10 | Listen to Before/After on real speakers on the 64 GB Linux tablet. | Hand check on Linux, by ear: apply the plosive fix and the DC fix, loop the range, press Before/After ten times each. I must hear no gap, click or jump. Result written into docs/checklists/K4.md. | easy | docs | hand, me, linux | K4.9 |  |

#### K5: Ready to publish summary (not started)

- **On main:** Nothing. The inputs exist: `loudness::measure` in crates/core/src/loudness.rs and `Target::PODCAST` (-16 LUFS, -1 dBTP) in crates/core/src/match_loudness.rs. No -19 LUFS mono target exists anywhere.
- **Done check:** The summary's numbers match G4's Amplitude Statistics to 0.1 LU and 0.1 dB.
- **Recorded so far:** not recorded
- **Item issue:** #1055

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| K5.1 | Add pure `check::summary::Summary` built from a `Loudness`, the channel count and a Report: integrated, target (-16 for 2 channels or more, -19 mono), true peak, findings left, text lines. | Unit tests in crates/core/src/check/summary.rs: `target_is_minus_16_stereo_minus_19_mono`; `lines_are_exact` for -16.3 LUFS, -1.4 dBTP, 2 findings, stereo: "Integrated loudness: -16.3 LUFS (target -16.0)", "True peak: -1.4 dBTP", "Findings left: 2". | easy | pure-fn | script | none |  |
| K5.2 | Add a test that the summary agrees with `loudness::measure` and that the check's own true peak value agrees with it too. | New file crates/core/tests/episode_check_summary.rs, test `summary_matches_loudness_measure_to_a_tenth`: on a stereo 1 kHz sine at -20 dBFS the summary's integrated value is within 0.1 of -20.0 LUFS, and its integrated and true peak values are each within 0.1 of `loudness::measure` on the same document. Second test `true_peak_finding_agrees_with_measure`: on the quarter-rate 0.95 sine used in episode_check.rs, the TruePeak finding's value is within 0.1 dB of `measure`'s true peak for that range. | easy | pure-fn | script | K5.1 |  |
| K5.3 | Show the summary at the bottom of the findings panel once a check has run; refresh it after each fix. Add control command `CHECK summary`. | New script `python3 tools/alpha/k5.py --exe <release build>`: stereo 1 kHz sine at -20 dBFS; `CHECK summary` answers `OK lufs=-20.0 target=-16.0 peak=<p> left=0`, and `lufs` and `peak` are within 0.1 of `integrated=` and `true_peak=` in the `LOUDNESS` reply. A mono file gives `target=-19.0`. | medium | ui | script | K5.1, K3.5, K4.8 |  |
| K5.4 | Add the Save As MP3 settings line to the summary, read from the MP3 save settings. | tools/alpha/k5.py: `CHECK summary` also carries `mp3=192kbps_CBR_stereo` with default settings and `mp3=128kbps_CBR_mono` after the MP3 settings are changed to 128 kbps mono. | easy | ui | script | K5.3, #11, craft-mp3 v1.0.0 tag |  |
| K5.5 | Compare the summary with the Amplitude Statistics panel in the running app on the 64 GB Linux tablet. | Hand check on Linux: open one real episode, run Episode Check, then open Window > Amplitude/Loudness Statistics. Integrated loudness and True peak in that window must agree with the summary to 0.1 LU and 0.1 dB. Both pairs of numbers written into docs/checklists/K5.md. | easy | docs | hand, linux | K5.3 |  |

#### L1: Transcribe the open file, saved with it (not started)

- **On main:** Nothing. No transcript type, no model runner, no candle dependency, no sidecar file, no action. docs/v1/architecture.md itself says no transcription code is in the tree.
- **Done check:** 1-hour episode transcribes in under 15 minutes on the 64 GB Linux tablet and fits the 8 GB Mac's budget (I7), both measured. English accuracy good enough to edit from, by eye on a real episode.
- **Recorded so far:** not recorded
- **Item issue:** #1057

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| L1.1 | Add `transcript::Transcript { version: 1, sample_rate, words }` and `Word { text, start, end }` (frames), with JSON read and write that refuse bad data. | New file crates/core/tests/transcript.rs: `round_trips_through_json` (three words with exact frames), `rejects_unknown_version` (version 2 is an error), `rejects_overlapping_words` (a word starting before the previous end is an error). | easy | data-shape | script | none |  |
| L1.2 | Add `sidecar_path`, `save_beside` (atomic, through `save::write_file_atomic`) and `load_beside` (a missing file is `Ok(None)`). | crates/core/tests/transcript.rs: `sidecar_of_episode_wav` gives `episode.wav.transcript.json`; `transcript_survives_save_and_reload` in a temp folder; `a_missing_sidecar_is_none`. | easy | state | script | L1.1 |  |
| L1.3 | Let DocState carry `Option<Arc<Transcript>>` (`transcript()`, `with_transcript`) and add `History::push_transcript(label, transcript)`, modelled on `push_markers`. | New file crates/core/tests/transcript_state.rs, test `undo_of_transcribe_removes_the_transcript`: after the push the current state has 3 words and the step's `made_bytes` is 0; after undo `transcript()` is None; redo brings the 3 words back. | easy | state | script | L1.1 |  |
| L1.4 | Add `transcript::mel::windows`: mix the channels to mono, resample to 16 kHz with the existing Resampler, and yield 480,000-sample windows with their source frame offsets. | Unit tests in crates/core/src/transcript/mel.rs: `a_65_s_file_makes_3_windows` (the last one zero padded); `offsets_at_48k_are_0_1440000_2880000`. | easy | pure-fn | script | none |  |
| L1.5 | Add `transcript::mel::log_mel`: Whisper's 80-bin log-mel frames (400-sample window, 160 hop) for one 30 s window. | Unit tests in mel.rs: `a_window_is_3000_frames_of_80_bins`; `a_1_khz_sine_peaks_in_mel_bin_26` (0-based); `silence_is_the_floor_value` (every bin equal). | medium | pure-fn | script | L1.4 |  |
| L1.6 | Add the `SpeechEngine` trait, a `FakeEngine` (one word "w<k>" per window, 1.0 to 1.5 s into it), and the runner `transcribe(doc, engine, cancel, progress) -> io::Result<Option<Transcript>>`. | New file crates/core/tests/transcript_run.rs: `fake_words_land_at_window_offsets` (65 s at 48 kHz: words at 48000..72000, 1488000..1512000, 2928000..2952000); `cancel_stops_before_the_next_window` (None); `progress_reaches_one`. | medium | state | script | L1.1, L1.4 |  |
| L1.7 | Add the model catalogue for whisper-base (file names, pinned revision, SHA-256 each) and `verify(path, sha256)`. Adds a pure-Rust SHA-256 crate. | Unit tests in crates/core/src/transcript/models.rs: `catalogue_hashes_are_64_hex`; `a_wrong_hash_is_refused` (temp file of other bytes); `a_right_hash_passes`. `python3 tools/test_notices.py` passes with the new crate listed. | medium | data-shape | script | none |  |
| L1.8 | Add `models::download(entry, folder, cancel, progress)`: fetch to a temp file, verify the hash, then rename. Adds an MIT or Apache HTTP client behind the `whisper` feature. | `cargo test -p wavcraft-core --features whisper --test transcript_models` against a listener on 127.0.0.1: `download_verifies_then_keeps_the_file`; `a_wrong_hash_leaves_no_file`; `cancel_leaves_no_file`. `cargo tree -p wavcraft-core` without the feature prints the same list as before the change. `python3 tools/test_notices.py` passes with the new crates listed. | medium | wiring | script | L1.7 |  |
| L1.9 | Add the real engine: Whisper base on candle behind the off-by-default `whisper` feature, giving text with segment times, using the shared Whisper crate. | Ignored test `whisper_transcribes_the_known_clip` in crates/core/tests/transcript_whisper.rs (clip and expected text from WAVCRAFT_SPEECH_CLIP and WAVCRAFT_SPEECH_TEXT): at least 9 of every 10 expected words appear in order. CI builds with and without `--features whisper` on Linux, Mac and Windows. | hard | wiring | script | L1.5, L1.6, L1.7, a shared Whisper crate split out of FilmCraft's speech crate (no issue yet) |  |
| L1.10 | Give every word its own start and end frame from the decoder's cross-attention alignment, mapped back to source frames. | The ignored test in crates/core/tests/transcript_whisper.rs also asserts: words are sorted, none overlap, each is 1,440 frames or longer at 48 kHz, and the first word starts within 9,600 frames of the clip's first speech given in WAVCRAFT_SPEECH_FIRST_FRAME. | hard | pure-fn | script | L1.9 |  |
| L1.11 | App: action `transcript.run` starts a worker (statspanel.rs pattern) with progress and Cancel; the result is pushed as History step "Transcribe". Control command `TRANSCRIPT run\|cancel\|state`; WAVCRAFT_FAKE_SPEECH=1 selects the fake engine. | New script `python3 tools/alpha/l1.py --exe <release build>` with the fake engine: 65 s generated WAV; after `ACTION transcript.run`, `TRANSCRIPT state` reaches `running=0 words=3 first=48000-72000` and `STATE` shows `steps` up by 1. On a generated 30-minute WAV, `ACTION transcript.run` followed at once by `TRANSCRIPT cancel` ends with `running=0 words=0` within 1 s and `steps` unchanged. | medium | wiring | script | L1.3, L1.6 |  |
| L1.12 | App: Save and Save As write the sidecar beside the file; Open loads it when present. | tools/alpha/l1.py: after the fake run, `SAVE <tmp>/x.wav`; the file `<tmp>/x.wav.transcript.json` exists; after reopening x.wav, `TRANSCRIPT state` shows `words=3 first=48000-72000`. A WAV with no sidecar opens with `words=0` and no ERROR line in the log. | easy | wiring | script | L1.2, L1.11 |  |
| L1.13 | Add the Edit menu entry "Transcribe" and, when the model files are missing, a dialog that asks before downloading them. | tools/alpha/l1.py with the app built with `--features whisper`, the fake engine off and the model folder pointed at an empty temp folder: `ACTION transcript.run` gives `TRANSCRIPT state` `confirm=1 running=0`; `TRANSCRIPT confirm no` leaves the folder empty and `confirm=0`. Unit test in crates/core/src/actions.rs: `by_name("transcript.run")` has menu text "Transcribe". | medium | ui | script | L1.8, L1.9, L1.11 |  |
| L1.14 | Measure a 1-hour episode on the 64 GB Linux tablet: wall time and peak resident memory. Replace the "Owed" list in architecture.md with the figures. | On Linux: `python3 tools/alpha/l1.py --exe <release build with --features whisper> --measure --wav <1-hour 48 kHz episode>`. `seconds` must be under 900. Seconds, peak MB, word count and commit hash written into docs/checklists/L1.md, and the same figures replace the `Owed before L1 is done` list in docs/v1/architecture.md. | easy | docs | hand, linux | L1.10, L1.13, L1.17 |  |
| L1.15 | Measure the same 1-hour run on the 8 GB Mac against the I7 memory budget. | On the 8 GB Mac: the same L1.17 command on the same file. `peak_mb` must be under 1536 (I7's 1.5 GB). The figure and the commit hash are added to docs/checklists/L1.md. | easy | docs | hand, mac | L1.10, L1.13, L1.17 |  |
| L1.16 | I read the transcript of one real episode of mine and judge whether I can edit from it. | Hand check on Linux, by eye: I read ten minutes of the transcript against the audio and write my verdict (good enough to edit from, yes or no, with the worst errors) into docs/checklists/L1.md. A no means trying Whisper small. | easy | docs | hand, me, linux | L1.14 |  |
| L1.17 | Add a `--measure --wav <file>` mode to tools/alpha/l1.py: it opens the file, answers the download question yes, sends `ACTION transcript.run`, and prints the seconds until `running=0`, the word count and the app's peak resident memory in MB. | `python3 tools/alpha/l1.py --exe <release build> --measure --wav <65 s generated WAV>` with the fake engine prints one line `seconds=<n> words=3 peak_mb=<n>` and exits 0. | easy | wiring | script | L1.13 |  |

#### L2: Transcript panel and lane (not started)

- **On main:** Nothing. No transcript panel file in apps/wavcraft/src, no `transcript` id in crates/core/src/panels.rs, no lane drawing, no word lookup.
- **Done check:** Each word's time is within 50 ms of where it is spoken, on a hand-labelled test clip (measured).
- **Recorded so far:** not recorded
- **Item issue:** #1058

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| L2.1 | Add `Transcript::word_at(frame) -> Option<usize>` by binary search: start inclusive, end exclusive, None in a gap. | crates/core/tests/transcript.rs, test `word_at_start_end_and_gap`: words 48000..72000 and 1488000..1512000; frame 48000 gives Some(0), 71999 Some(0), 72000 None, 0 None, 1488000 Some(1), 1512000 None. | easy | pure-fn | script | L1.1 |  |
| L2.2 | Register panel `transcript` (title "Transcript", right dock) and list it in the panels doc. | Unit tests in crates/core/src/panels.rs: `by_id("transcript")` is Some and closable; `PanelSet::for_build(true).report()` contains `transcript=1`; after `apply_action("window.hide.transcript")` it contains `transcript=0`; the existing right-zone tests are updated to include `transcript`. The `PANELS` doc line in control.rs lists `transcript=<0\|1>` and `tools/checks/1003-panels-doc-lists-tools.sh` passes. | medium | data-shape | script | none |  |
| L2.3 | App logic: choosing a word moves the cursor to its start; the word under the playhead is tracked each frame. Control: `TRANSCRIPT click <i>`, and `current=<i\|->` in `TRANSCRIPT state`. | New script `python3 tools/alpha/l2.py --exe <release build>` with the fake engine: `TRANSCRIPT click 1` then `STATE` shows `cursor_frame=1488000`; during `PLAY 50000 70000`, `TRANSCRIPT state` shows `current=0`; stopped at frame 0 it shows `current=-`. | medium | wiring | script | L1.11, L2.1 |  |
| L2.4 | Draw the panel: the words wrapped in order, the current word highlighted, a click on a word running the L2.3 logic. | Headless egui test in apps/wavcraft/src/transcript_panel.rs, `clicking_a_word_returns_its_index` (pattern of issue1000_tests.rs): a pointer click inside the second word's rectangle returns Some(1); a click on empty space returns None. | medium | ui | script | L2.2, L2.3 |  |
| L2.5 | Add pure `transcript::lane_boxes(words, view_start, view_end, width_px) -> Vec<(x0, x1, index)>`; words outside the view are dropped, partly visible ones clipped. | crates/core/tests/transcript.rs, test `lane_boxes_follow_the_view`: view 0..96000 at 960 px, word 48000..72000 gives (480, 720, 0); a word at 90000..120000 gives (900, 960, 1); a word at 200000 is absent. | easy | pure-fn | script | L1.1 |  |
| L2.6 | Draw the transcript lane under the waveform from `lane_boxes`, with the word text inside each box; a click on a box runs the L2.3 logic. Control query `TRANSCRIPT lane`. | tools/alpha/l2.py: with the view set to frames 0 to 96000, `TRANSCRIPT lane` answers `OK boxes=1 first=<x0>-<x1>` where x0 and x1 are one half and three quarters of the reported lane width. | medium | ui | script | L2.3, L2.5 |  |
| L2.7 | I label a 30 to 60 s clip of real speech by hand: each word's text, start frame and end frame. The clip stays outside the repo; only the labels are committed. | crates/core/tests/fixtures/l2_labels.json exists in the L1.1 transcript format (version 1, sample rate 48000, words with text, start and end frames) and holds 50 words or more. The length of the clip and where I keep it are written into docs/checklists/L2.md. | easy | docs | hand, me | L1.1 |  |
| L2.8 | Add an ignored test that transcribes the labelled clip (path from WAVCRAFT_L2_CLIP, `whisper` feature) and compares every word's times with the labels. | New file crates/core/tests/transcript_timing.rs. `labels_file_is_well_formed` (always runs): the fixture loads with the L1.1 reader, has 50 words or more, sorted, none overlapping, sample rate 48000. Ignored `word_times_within_50_ms_of_labels` (`cargo test -p wavcraft-core --release --features whisper --test transcript_timing -- --ignored --nocapture`, clip path from WAVCRAFT_L2_CLIP): words are paired with labels by text in order; at least 9 of every 10 labelled words pair; every paired start and end is within 2,400 frames; it prints the worst offset in ms and the number unpaired. | easy | pure-fn | script | L1.10, L2.7 |  |
| L2.9 | Run the timing test on the 64 GB Linux tablet and look at the panel and lane once while playing. | Hand check on Linux: the L2.8 command passes; while a real clip plays I must see the highlighted word follow the playhead in the panel and the lane, and a click on a word move the cursor. Worst offset and result written into docs/checklists/L2.md. | easy | docs | hand, me, linux | L2.4, L2.6, L2.8 |  |

#### L3: Select text to select and cut audio (not started)

- **On main:** Only an unused helper: crates/core/src/transcript_cut.rs `cut_ranges`, which takes word times in seconds, with 12 unit tests in the same file. Nothing calls it. No word-drag selection, no transcript update on edit, no Done test.
- **Done check:** Deleting a sentence from the text leaves the same samples as cutting that range by hand (a test checks this).
- **Recorded so far:** not recorded
- **Item issue:** #1059

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| L3.1 | Add `Transcript::range_of(first, last) -> Range<u64>`: the first word's start to the last word's end, in either drag direction. | crates/core/tests/transcript.rs, test `range_of_words`: words at 48000..72000, 80000..90000, 1488000..1512000; `range_of(0, 1)` is 48000..90000; `range_of(1, 0)` the same; `range_of(2, 2)` is 1488000..1512000. | easy | pure-fn | script | L1.1 |  |
| L3.2 | Add `Transcript::moved_by(&markers::Shift)`: a delete drops words inside the range, trims a word that straddles an edge and shifts later words; an insert shifts later words. | crates/core/tests/transcript.rs: `delete_shifts_later_words` (Delete 48000..90000 leaves one word at 1446000..1470000); `a_straddling_word_is_trimmed` (Delete 60000..80000 gives 48000..60000); `insert_shifts_later_words` (Insert 1000 at 0 gives 49000..73000); `replace_drops_inside_and_shifts_later` (Replace 48000..90000 by 1000 frames leaves one word at 1447000..1471000); `crop_keeps_only_words_inside` (Crop 40000..100000 gives 8000..32000 and 40000..50000); `none_changes_nothing`. | easy | pure-fn | script | L1.1 |  |
| L3.3 | Make every length-changing edit in edit.rs move the transcript where it already moves the markers, and add `transcript::delete_words(doc, first, last, crossfades)`. | New file crates/core/tests/transcript_edit.rs: `deleting_a_sentence_equals_the_hand_cut` (delete_words 3 to 7 versus `edit::delete` on `range_of(3, 7)` with the same Crossfades: every sample equal by `to_bits`, 5 fewer words); `undo_restores_audio_and_transcript` (History grew by 1; after undo both equal the original); `a_same_length_edit_keeps_the_transcript` (after `edit::silence` on a range the word list is unchanged). | medium | state | script | L1.3, L3.1, L3.2 |  |
| L3.4 | Remove the unused seconds-based `transcript_cut.rs` and its row in the code map, now that `range_of` and `moved_by` replace it. | `grep -rn transcript_cut crates apps docs/code-map.md` prints nothing; `cargo test -p wavcraft-core` passes. | easy | rename | script | L3.3 |  |
| L3.5 | Panel: dragging over words sets the time selection to `range_of`; Delete and Cut then run as the ordinary edits. Control command `TRANSCRIPT select <i> <j>`. | New script `python3 tools/alpha/l3.py --exe <release build>` with the fake engine: `TRANSCRIPT select 0 1` gives `selection=48000-1512000`; `ACTION edit.delete` raises `steps` by 1 and leaves `words=1`; `ACTION edit.undo` gives `words=3`. Headless egui test `dragging_across_two_words_returns_both`. | medium | ui | script | L2.4, L3.3 |  |

#### L4: Filler and pause finder (not started)

- **On main:** Nothing. No filler list, pause finder or batch remove. The nearest code is the level-based Delete Silence in crates/core/src/diagnostics/silence.rs, which shows the pattern for many cuts in one step.
- **Done check:** Removing all fillers from a 1-hour episode is one History step, done in under 1 s; no click at any splice (D7's check).
- **Recorded so far:** not recorded
- **Item issue:** #1060

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| L4.1 | Add `transcript::filler::fillers(&Transcript, list) -> Vec<usize>` with default list um, uh, er, erm, ah, hm, hmm, mm; case and punctuation ignored, whole words only. | Unit test `finds_fillers_through_case_and_punctuation` in crates/core/src/transcript/filler.rs: words "Um," "the" "UH." "umbrella" "uh" give indices [0, 2, 4]. | easy | pure-fn | script | L1.1 |  |
| L4.2 | Add `pauses(&Transcript, min_frames) -> Vec<Range<u64>>` (gap between one word's end and the next start, default 1.5 s) and `shorten_cut(pause, keep_frames)`, the middle part to remove. | Unit tests in filler.rs at 48 kHz: `a_1_6_s_gap_is_a_pause_a_1_4_s_gap_is_not` (76,800 frames yes, 67,200 no); `shortening_a_2_s_pause_to_half_a_second` (pause 100000..196000, keep 24000: cut is 112000..184000). | easy | pure-fn | script | L1.1 |  |
| L4.3 | Add `transcript::delete_ranges(doc, ranges, crossfades, label)`: all cuts applied last to first into one Edit, as Delete Silence does. | crates/core/tests/transcript_edit.rs: `removing_all_fillers_is_one_history_step` (3 ranges: History grows by 1, and the result equals three single `edit::delete` calls done last to first, every sample by `to_bits`); `no_click_at_any_filler_splice` (2 s full-scale 1 kHz sine, three ranges, default Crossfades: `max_step` of the whole result is at most 1.035 times `max_step` of the untouched sine). | medium | state | script | L3.3 |  |
| L4.4 | App logic and control: `FILLERS list`, `FILLERS remove all\|<i,j>` (step "Remove Fillers"), `FILLERS shorten <ms>` (step "Shorten Pauses"). | New script `python3 tools/alpha/l4.py --exe <release build>`: a 30 s WAV with a sidecar holding 3 fillers and one 2.0 s pause. `FILLERS list` gives `fillers=3 pauses=1`; `remove all` raises `steps` by 1 and gives `fillers=0`; `shorten 500` shortens the file by 72000 frames. | medium | wiring | script | L1.12, L4.1, L4.2, L4.3 |  |
| L4.5 | Draw the finder in the transcript panel: a row per filler and pause with a checkbox and time, buttons Remove Selected and Remove All, and a Shorten Pauses To field. | Headless egui test in apps/wavcraft/src/transcript_panel.rs, `remove_selected_returns_the_ticked_rows`: with rows 0 and 2 ticked, a click on Remove Selected returns [0, 2]; Remove All returns every row. | medium | ui | script | L2.4, L4.4 |  |
| L4.6 | Add an ignored bench test: remove 600 fillers from 3600 s of 48 kHz stereo in a temp store and time it. | `cargo test -p wavcraft-core --release --test transcript_bench -- --ignored --nocapture` runs `removing_600_fillers_from_an_hour_takes_under_1_s`: prints the milliseconds and asserts under 1,000 and exactly 1 new History step. | easy | pure-fn | script | L4.3 |  |
| L4.7 | Run the bench on the 64 GB Linux tablet and record it. | Hand check on Linux: run the L4.6 command in the build container. The time must be under 1 s. The milliseconds and the commit hash are written into docs/checklists/L4.md. | easy | docs | script, linux | L4.6 |  |

#### L5: Transcript search, text and SRT export (not started)

- **On main:** Nothing. No transcript search, no text or SRT writer, and Ctrl+F is unbound in crates/core/src/shortcuts.rs.
- **Done check:** A search finds every match; the SRT plays in sync with the exported audio.
- **Recorded so far:** not recorded
- **Item issue:** #1061

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| L5.1 | Add `transcript::search::find(&Transcript, query) -> Vec<Range<usize>>` (word-index ranges; case and punctuation ignored; phrases span words), plus `next_match` and `prev_match`, both wrapping. | Unit tests in crates/core/src/transcript/search.rs: `finds_every_match` ("the" three times in 12 words gives three exact ranges, and "then" is not a match); `a_phrase_spans_words` ("quick brown" gives 1..3); `next_wraps_to_the_first`. | easy | pure-fn | script | L1.1 |  |
| L5.2 | Add `transcript::export::plain_text`: words joined by single spaces, one trailing newline. | Unit test `plain_text_is_exact` in crates/core/src/transcript/export.rs: words "Hello" "world." "This" "works." give exactly "Hello world. This works.\n"; an empty transcript gives an empty string. | easy | data-shape | script | L1.1 |  |
| L5.3 | Add `transcript::export::srt`: cues numbered from 1, ending at a word that ends in . ? or !, or after 10 words; times HH:MM:SS,mmm from frames and rate. | Unit tests in export.rs: `srt_is_exact` against a literal two-cue string; `time_of_3661_5_s_is_01_01_01_500`; `cue_times_follow_a_cut` (after `moved_by` a delete of 48000 frames at 48 kHz, later cue times are 1.000 s earlier). | easy | data-shape | script | L1.1, L3.2 |  |
| L5.4 | Add actions `transcript.find` (Ctrl+F), `transcript.find_next` (Ctrl+G), `transcript.find_prev` (Ctrl+Shift+G), `transcript.export_text`, `transcript.export_srt`. | Unit tests: in crates/core/src/shortcuts.rs `Keymap::new().check()` is Ok and `resolve_text("Ctrl+F")` is the find command; in crates/core/src/actions.rs `by_name` finds all five names with menu texts "Find in Transcript", "Next Match", "Previous Match", "Transcript as Text", "Transcript as SRT". | easy | data-shape | script | none |  |
| L5.5 | Panel: a search box opened by Ctrl+F, matches highlighted, next and previous moving the cursor to the match's first word. Control command `TRANSCRIPT find <text>`. | New script `python3 tools/alpha/l5.py --exe <release build>`: sidecar with "the" at word starts 48000, 240000 and 480000. `TRANSCRIPT find the` gives `matches=3`; `KEY Ctrl+G` twice gives `cursor_frame=240000`; `KEY Ctrl+Shift+G` gives `cursor_frame=48000`. | medium | ui | script | L1.12, L2.4, L5.1, L5.4 |  |
| L5.6 | Wire the two export actions to a save dialog and an atomic write. Control command `TRANSCRIPT export txt\|srt <path>`. | tools/alpha/l5.py: `TRANSCRIPT export txt <tmp>/a.txt` and `export srt <tmp>/a.srt` write files whose bytes equal the script's literal expected text and SRT for the fixture sidecar; no temp file is left in the folder. | medium | wiring | script | L5.2, L5.3, L5.4, L1.11 |  |
| L5.7 | Play an exported episode with its exported SRT in a media player on the 64 GB Linux tablet. | Hand check on Linux: transcribe and cut a real episode, export the audio and the SRT, open both in a player. At the start, the middle and the end each cue must appear as its words are spoken. Result written into docs/checklists/L5.md. | easy | docs | hand, me, linux | L5.6, L1.14, L3.5 |  |

#### M2: Episode Map replaces the zoom navigator (not started)

- **On main:** Only the plain navigator: navigator() in apps/wavcraft/src/app.rs draws peak columns in one colour plus the view handle. Its maths and three B3 tests are in crates/core/src/view.rs (nav_handle, nav_jump, nav_drag, nav_hit).
- **Done check:** Drawn for a 3-hour file within 2 s of opening and redrawn within 0.5 s of an edit, measured. B3's navigator checks still pass.
- **Recorded so far:** not recorded
- **Item issue:** #1052

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| M2.1 | New crates/core/src/episode_map.rs: Class enum (Silence, Speech, Loud), classify(peak_db), and columns(&[(min,max)]) giving one Class per peak column. Silence under -60 dBFS, Loud from -6 dBFS. | New file crates/core/tests/m2_episode_map.rs, test columns_are_classed_by_peak: columns of [(0.0,0.0), (-0.1,0.1), (-0.9,0.9)] gives [Silence, Speech, Loud]; classify(-60.0) is Speech; classify(-6.0) is Loud; an empty slice gives an empty Vec. | easy | pure-fn | script | none |  |
| M2.2 | Add pin_x(frame, total_frames, width_px) and span_x(start, end, total_frames, width_px) to episode_map.rs for placing markers and findings on the strip. | Test pins_land_on_their_columns in crates/core/tests/m2_episode_map.rs: with 48000 frames and 100 px, pin_x(24000) is 50.0; span_x(0, 480) is (0.0, 1.0); with 0 total frames pin_x is 0.0. | easy | pure-fn | script | M2.1 |  |
| M2.3 | Add three colour roles to the Palette in theme.rs, map_silence, map_speech and map_loud, in both DARK and LIGHT, and to the themed() test list. | cargo test -p wavcraft theme: every_themed_role_differs_between_dark_and_light passes with the three new names in its list; no_inline_colour_values_in_ui_sources still passes. | easy | data-shape | script | none |  |
| M2.4 | In navigator() paint each column in its class colour from episode_map::columns. Leave the handle, keys, click and drag code as it is. Add control verb EPISODEMAP reporting class counts. | New apps/wavcraft/src/m2_map_tests.rs (hooked from app.rs like issue366_tests.rs), test the_map_classes_silence_speech_and_loud: on a written 3 s WAV of 1 s digital silence, 1 s tone at -20 dBFS and 1 s at -3 dBFS, EPISODEMAP answers cols above 0 with silence, speech and loud each one third of cols, within 1. cargo test -p wavcraft-core navigator: 3 tests pass. | medium | ui | script | M2.1, M2.3 |  |
| M2.5 | Draw each marker on the map as a tick at pin_x, range markers as a span, in the marker colour. Add markers=<n> and ticks=<x in thousandths of the strip, comma separated> to the EPISODEMAP reply. | Test the_map_shows_markers in apps/wavcraft/src/m2_map_tests.rs: on the 3 s file, after adding markers at 0.5 s and 2.0 s, EPISODEMAP contains markers=2 and ticks=167,667 (each within 1). | easy | ui | script | M2.2, M2.4 |  |
| M2.6 | Measure the map on a 3-hour stereo 48 kHz file: time to build all columns after open, and time to rebuild after one delete. Record both in docs/baseline.md. | Ignored test episode_map_3h_build_and_rebuild_times in crates/core/tests/m2_episode_map.rs, run with --release -- --ignored --nocapture on the Linux handheld: it writes the testkit 3-hour 48 kHz stereo file to a scratch folder, times from the start of the open until episode_map::columns over waveform_columns for 1920 columns returns (build_ms), then deletes 10 s from the middle and times the same call again (rebuild_ms). It asserts build_ms under 2000 and rebuild_ms under 500. Both numbers, date and commit are added to docs/baseline.md. | easy | docs | script, linux | M2.4 |  |
| M2.7 | Draw Episode Check findings on the map as pins at span_x, errors in the alert colour and warnings in the caution colour. Add pins=<n> to EPISODEMAP. | Test the_map_shows_finding_pins in apps/wavcraft/src/m2_map_tests.rs: on a 15 s file with digital silence from 5 s to 10 s, after Episode Check runs, EPISODEMAP contains pins=1 and the pin span 333-667 in thousandths (within 1). | easy | ui | script | M2.5, K1.3, K3.1 |  |
| M2.8 | Show the map across the top in the Podcast face as well as Studio. | Test the_map_is_shown_in_both_faces in apps/wavcraft/src/m1_face_tests.rs: after FACE podcast with a file open, EPISODEMAP answers OK with cols above 0 for each of the four steps. | easy | wiring | script | M2.4, M1.6 |  |

#### M1: Studio and Podcast face switch (not started)

- **On main:** Nothing for the faces is on main. No face setting in crates/core/src/settings.rs, no faces module, no View or toolbar switch in apps/wavcraft/src/app.rs. The word podcast appears only as a Match Loudness preset name.
- **Done check:** The second user records, cleans, checks and exports a real episode in Podcast without opening a Studio panel. Checked by hand, with the second user.
- **Recorded so far:** not recorded
- **Item issue:** #1063

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| M1.1 | Add a Face setting (studio or podcast) under the key face in core settings, default studio, saved and read like the theme key. | New test the_face_is_saved_and_a_bad_value_is_the_default in crates/core/tests/settings.rs: default get("face") is "studio"; set to "podcast" survives save and load; "banana" is refused and the value stays "studio". | easy | data-shape | script | none |  |
| M1.2 | New apps/wavcraft/src/faces.rs: Step enum (Record, Clean, Check, Publish), Step::ALL, label(), and tools(step) returning tool names. Clean tools are action names (edit.delete, edit.silence, effect.noise_reduction). Record, Check and Publish tools get their own names (record.start, record.stop, check.run, match.run, file.saveAsMp3) because the action table has no rows for them. | Unit test each_step_lists_only_its_tools in faces.rs: ALL labels are Record, Clean, Check, Publish in order; tools(Clean) is [edit.delete, edit.silence, effect.noise_reduction] and each resolves with wavcraft_core::actions::id_by_name; tools(Record) is [record.start, record.stop]; tools(Check) is [check.run]; tools(Publish) is [match.run, file.saveAsMp3]; no name is in two steps. | easy | pure-fn | script | none |  |
| M1.3 | Hold the face in the app state, loaded from the setting. Add control verb FACE studio\|podcast and face= in the STATE reply. Switching writes the setting. | New file apps/wavcraft/src/m1_face_tests.rs, test switching_face_leaves_document_selection_and_history_unchanged: after a delete of frames 100 to 200, switch to podcast and back; history cursor, selection and frame count are equal. FACE podcast then STATE contains face=podcast. | medium | state | script | M1.1 |  |
| M1.4 | Add Studio and Podcast radio entries to the View menu and a two-way switch at the right of the toolbar. Both call the same set-face function from step 3. | Debug build by hand on any machine: View shows Studio and Podcast; choosing Podcast makes the control reply to STATE contain face=podcast; the toolbar switch shows Podcast selected; after a restart STATE still says face=podcast. | easy | ui | hand | M1.3 |  |
| M1.5 | In Podcast draw a bar of four step buttons, Record, Clean, Check, Publish. Keep the current step in app state. Add control verbs STEP <name> and STEPTOOL <tool name> (runs a tool of the current step, ERR for a tool of another step) and step= in STATE. | Test the_step_bar_moves_between_the_four_steps in apps/wavcraft/src/m1_face_tests.rs: in podcast, STEP clean then STATE contains step=clean; STEP banana answers ERR; in studio STEP answers ERR; a new podcast session starts at step=record; at step=record STEPTOOL edit.delete answers ERR. | medium | ui | script | M1.2, M1.3 |  |
| M1.6 | Add faces::panels_shown(face, step) and make the frame layout use it: Podcast lays out the editor and the step's own panel only, no Studio dock zones, no Window menu panels. | Test podcast_shows_no_studio_panel in faces.rs: panels_shown(Podcast, Record) is ["waveform"]; (Podcast, Publish) is ["waveform", "match"]; no Podcast step lists history, files, tools, markers or playlist; Studio returns every offered panel. Test podcast_lays_out_no_dock_zone in apps/wavcraft/src/m1_face_tests.rs: after FACE podcast the PANELS reply has zone.left=-, zone.right=- and zone.bottom=-; after FACE studio the PANELS reply equals the one taken before the switch. | hard | ui | script | M1.5 |  |
| M1.7 | Record step tools: a Record and Stop button and the input level meter, running the existing recording start and stop. | Test the_record_step_starts_and_stops_a_take in apps/wavcraft/src/m1_face_tests.rs with WAVCRAFT_FAKE_INPUT=sine:440:-12: FACE podcast, STEP record, STEPTOOL record.start, run frames for half a second, STEPTOOL record.stop; STATE then has frames above 0, face=podcast and step=record. | medium | wiring | script | M1.5, M1.6 |  |
| M1.8 | Clean step tools: buttons for Delete, Silence and Noise Reduction that run the existing actions on the selection. | Test the_clean_step_runs_delete_silence_and_noise_reduction in apps/wavcraft/src/m1_face_tests.rs: on a 48000-frame file with SELECT 100 200, STEPTOOL edit.delete leaves frames=47900 and steps one higher in STATE; STEPTOOL edit.silence on SELECT 100 200 keeps frames=47900 and adds one more step; STEPTOOL effect.noise_reduction makes EFFECT_DIALOG answer open=noise_reduction. | easy | wiring | script | M1.5, M1.6 |  |
| M1.9 | Clean step: show the Transcript panel and lane so text edits cut audio there. | Test the_clean_step_shows_the_transcript in faces.rs: panels_shown(Podcast, Clean) is ["waveform", "transcript"]; deleting a word in the transcript in Podcast shortens the file by that word's frames. | easy | wiring | script | M1.8, #1058, #1059 |  |
| M1.10 | Check step: show the Episode Check Findings panel and a Run Check button. | Test the_check_step_lists_findings in m1_face_tests.rs: on a testkit file with 5 s of digital silence in the middle, STEP check and Run Check give a findings list with one Dead air row. | easy | wiring | script | M1.6, #1049, #1053 |  |
| M1.11 | Publish step tools: the Match Loudness panel and a Save As MP3 button. | Test the_publish_step_matches_and_saves_mp3 in apps/wavcraft/src/m1_face_tests.rs: FACE podcast, STEP publish, STEPTOOL match.run with the podcast preset, wait for busy=0, SAVE <scratch>/out.mp3, wait for busy=0; out.mp3 exists; after OPEN of out.mp3 the LOUDNESS reply is -16 LUFS within 0.5 LU. | medium | wiring | script | M1.6, #11, #38 |  |
| M1.12 | Hand check on the 8 GB Mac with the second user: record, clean, check and export one real episode in Podcast only. | On the 8 GB Mac I watch the second user do all four steps without opening a Studio panel. I write the date, commit, each step's pass or fail and any place the second user got stuck into docs/checklists/M1.md. No episode name goes in the file. | easy | docs | hand, me, mac | M1.4, M1.7, M1.9, M1.10, M1.11, M2.8, I5.6 |  |

#### M4: Large controls preference, 44 px minimum (partial)

- **On main:** the large_controls setting in crates/core/src/settings.rs (default off, tested in crates/core/tests/settings.rs); theme::metrics_for in apps/wavcraft/src/theme.rs (44 px control height and 22 px handle grab when large); navigator edges (crates/core/src/view.rs nav_hit) and marker flags (apps/wavcraft/src/markers_panel.rs) take their grab width from theme::metrics().handle_grab. Missing: apply_look does not yet switch the metrics from the setting (M4.3), no Preferences checkbox (M4.4), Podcast buttons have no 44 px minimum (M4.6). Merged 2026-11-12: M4.1, M4.2 and M4.5 (#1207).
- **Done check:** Every control in Podcast is usable by touch on the Linux handheld with the touch screen, by hand.
- **Recorded so far:** not recorded
- **Item issue:** #1062

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| M4.3 | Make apply_look call theme::set_large from the setting and set egui's spacing.interact_size to at least control_height in both directions; include the setting in the applied_look key. | Test large_controls_set_the_interact_size in apps/wavcraft/src/m4_large_tests.rs (hooked from app.rs), on a window built as in issue366_tests.rs: after PREF set large_controls true and one apply_look, ctx.style().spacing.interact_size.y is 44.0 and .x is at least 44.0; after PREF set large_controls false the y is 18.0. | easy | state | script | M4.1, M4.2 |  |
| M4.4 | Add a Large controls checkbox to Preferences, Appearance using pref_check with the key large_controls. | Test the_large_controls_pref_answers_on_the_control_channel in apps/wavcraft/src/m4_large_tests.rs: PREF set large_controls true answers OK; PREF get large_controls answers OK true; PREF set large_controls banana answers ERR. By hand in a debug build: Preferences, Appearance shows a Large controls checkbox, and ticking it makes the toolbar buttons taller at once. | easy | ui | script | M4.3 |  |
| M4.6 | Add faces::button_min_size() returning control_height in both directions, and give it to every Podcast step bar button and step tool button as its minimum size. | Test podcast_buttons_are_44_px_with_large_controls in apps/wavcraft/src/faces.rs: with Large controls on, button_min_size() is (44.0, 44.0); with it off it is (18.0, 18.0). | easy | ui | script | M4.3, M1.8 |  |
| M4.7 | Hand check on the Linux handheld with the touch screen: Large controls on, Podcast face, tap every button and drag every handle with a finger. | In SteamOS Desktop Mode, in each of the four steps, tap each button and drag the map handle and a marker by finger. Each must respond on the first try. Write each control with pass or fail, date and commit into docs/checklists/M4.md. | easy | docs | hand, me, linux | M4.4, M4.5, M4.6, M1.7, M1.10, M1.11, M2.8 |  |

### Alongside: the MP3 encoder

The `craft-mp3` crate (its own repo) has its `v1.0.0` tag, so A3 is no longer blocked: depend on it by git tag and wire it in.

**Done when** a 1-hour episode exports to MP3 and passes A3's Done check.

**Remains:**

#### A3: Save As MP3 with the craft-mp3 encoder (not started)

- **On main:** Nothing in this repo. No craft-mp3 dependency in Cargo.toml, SaveFormat in crates/core/src/save.rs has only Wav, Aiff, Flac and Vorbis, and the save dialog in apps/wavcraft/src/app.rs offers no MP3. Only docs/v1/tasks/mp3-encoder-brief.md exists.
- **Done check:** A 1-hour episode exports, plays in Apple Podcasts' validator and in a browser, and one more listener and I can't tell it from the WAV at 128 kbps mono speech by ear.
- **Recorded so far:** not recorded
- **Item issue:** #11

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| A3.1 | Add craft-mp3 as a git dependency for wavcraft-core, pinned with rev to the commit that tag v1.0.0 names, as the SoundCraft crates are pinned; update Cargo.lock and regenerate THIRD-PARTY-NOTICES.txt. | 'cargo tree -p wavcraft-core -i craft-mp3' prints 'craft-mp3 v1.0.0'; 'python3 tools/notices.py --check' passes; 'grep -c craft-mp3 THIRD-PARTY-NOTICES.txt' is at least 1; the workspace builds on all three CI systems. | medium | wiring | script | none |  |
| A3.2 | Pure MP3 options: Mp3Options{bitrate_kbps, mono}, MP3_BITRATES, MP3_DEFAULT_BITRATE, and mp3_check(rate, channels, options) giving the output channel count or a reason. | New crates/core/tests/mp3_options.rs, test the_v1_choices: MP3_BITRATES is [32,40,48,56,64,80,96,112,128,160,192,224,256,320]; default 192; check(44100,2,192,stereo) is Ok(2); check(48000,2,128,mono) is Ok(1); 96000 Hz is Err containing '32000, 44100 or 48000'; 6 channels Err containing 'mono or stereo'; 100 kbps Err. | easy | pure-fn | script | none |  |
| A3.3 | SaveFormat::Mp3: from_path and parse ('mp3'), name 'MP3', extension 'mp3', accepts every sample type (ignored, as Vorbis). Until the writer lands, saving it returns SaveError::Unsupported. | New crates/core/tests/save_mp3.rs, test mp3_is_a_save_format: from_path('a.MP3') and parse(' mp3 ') are Some(Mp3); name() is 'MP3'; extension() is 'mp3'; accepts(Float32) is true. crates/core/tests/987_save_format_parse.rs passes with Mp3 added to its ALL list. | easy | data-shape | script | none |  |
| A3.4 | The MP3 writer: save_mp3(doc, target, options, progress) streams 64 K-frame blocks into the encoder through the atomic temporary file, then writes the Info tag over the start. save() with Mp3 uses the defaults. | save_mp3.rs: a_stereo_tone_round_trips_at_192 (10 s, 1 kHz, -6 dBFS, 44.1 kHz: file is 238000 to 243000 bytes; reopens stereo 44100; 441000 to 444500 frames; middle-second RMS -9.03 dBFS within 0.5 dB); a_failed_mp3_save_leaves_the_target_whole (target bytes unchanged, temp_files empty); a_96k_file_is_refused. | medium | data-shape | script | A3.1, A3.2, A3.3 |  |
| A3.5 | Mono from a stereo document: with options.mono the writer encodes (L+R)/2 as one channel. | save_mp3.rs, test mono_from_stereo_is_the_average: left 1 kHz at -6 dBFS, right silent, 5 s at 48 kHz, 128 kbps mono: the file is 78400 to 81600 bytes, reopens with 1 channel, and its middle-second RMS is -15.05 dBFS within 0.5 dB. | easy | pure-fn | script | A3.4 |  |
| A3.6 | App save path: save_with accepts .mp3 and calls save_mp3 with new app state mp3_bitrate (192) and mp3_mono; control 'SAVE <path> bitrate=<kbps> channels=1\|2'; the refused-format message lists .mp3. | New apps/wavcraft/src/a3_mp3_tests.rs with N1.5's harness, test save_mp3_over_the_control_line: 'SAVE out.mp3 bitrate=128 channels=1' on a 5 s 48 kHz stereo WAV answers OK; out.mp3 is 78400 to 81600 bytes; OPEN of it gives STATE channels=1 rate=48000. 'bitrate=100' answers ERR containing 'bitrate='. | medium | wiring | script | A3.5, N1.6 |  |
| A3.7 | Save As choices: an 'MP3' filter in the file dialog; in the 'Save As / Export' window an MP3 format button with a bit-rate list (14 rates, 192 preselected) and a Mono box; sample type row disabled. | a3_mp3_tests.rs, test the_dialog_choice_reaches_the_file: with a 5 s 48 kHz stereo WAV open and the dialog's bit rate set to 128 and Mono ticked, 'SAVEOPTS dialog mp3 16 <dir>/d.mp3' answers OK; after settle d.mp3 is 78400 to 81600 bytes and reopens with 1 channel. 'SAVEOPTS show' then contains 'format=mp3'. By eye in a debug build: choosing MP3 shows the list '32 kbps' to '320 kbps' with '192 kbps' selected, a Mono box that is greyed for a mono file, and a greyed sample type row. | medium | ui | hand | A3.6, A3.10 |  |
| A3.8 | Hand check on the 8 GB Mac: export a real 1-hour episode as 192 kbps stereo and as 128 kbps mono; play both in a browser; run the file through Apple Podcasts' validator. | On the 8 GB Mac I must see: both exports finish with no error; the browser shows about 1:00:00 and seeks to the middle; the validator reports no errors. Export seconds, file sizes, browser name and validator result are written into docs/checklists/A3.md. | easy | docs | hand, me, mac | A3.7 |  |
| A3.9 | By-ear check: blind ABX of the 128 kbps mono export against the WAV, 16 trials on each of two speech clips, for me and one more listener. | Using the ABX tool from the added step: each listener does 16 trials on each of two speech clips, the 128 kbps mono export against the WAV. Each listener scores 11 of 16 or fewer on both clips (12 or more is a fail, as docs/v1/tasks/mp3-encoder-brief.md says). The four scores from the tool's log, the clips used and the date are written into docs/checklists/A3.md. | easy | docs | hand, me | A3.6, A3.12 |  |
| A3.10 | MP3 size estimate: estimate_size gives the CBR size for Mp3 (bit rate times duration divided by 8) and the Save As size line shows it. | crates/core/tests/save_mp3.rs, test the_mp3_estimate_is_the_cbr_size: a 10 s 44.1 kHz stereo document at 192 kbps estimates 240000 bytes within 1 percent, exact is false; at 128 kbps mono for 5 s at 48 kHz it estimates 80000 within 1 percent. In the app test file, 'SAVEOPTS estimate mp3 16' answers 'OK bytes=' with a value in that range. | easy | pure-fn | script | A3.2, A3.3 |  |
| A3.11 | Headless honours SAVE's bitrate= and channels= for .mp3, as the window does after A3.6. | crates/automation/tests/headless.rs, new test save_mp3_with_options: on a 5 s 48 kHz stereo WAV, 'SAVE out.mp3 bitrate=128 channels=1' answers OK and out.mp3 is 78400 to 81600 bytes; 'SAVE out.mp3' with no options is 119000 to 122500 bytes (192 kbps stereo); 'bitrate=100' answers ERR containing 'bitrate='. | easy | wiring | script | A3.6, N1.3 |  |
| A3.12 | ABX tool: one local web page that loads a WAV and an MP3, plays A, B and a random X for 16 trials, and shows and saves the score. | By hand in a browser: load a clip as both A and B and answer 16 trials; the page shows 'n of 16' and downloads a log with 16 rows (trial, X was, answer). Load a WAV and a 32 kbps export and answer by ear: the score is 15 or 16 of 16. The page's pure scoring function has a unit test in the page's test file: 12 right answers of 16 give 'fail', 11 give 'pass'. | medium | ui | hand | none |  |

### Stage 5: Platform checks

Also taken here: the Mac steps of I7, J6 and M1 (M1.12), and A3 working on each machine.

**Done when** the app is built, launched and checked by hand on Windows, Linux and the 8 GB Mac, and every hand step in the earlier stages that names a machine is recorded.

**Complete:**

- **I6 Third-party notices under Help, About.** THIRD-PARTY-NOTICES.txt at the repo root, made by tools/notices.py, compiled in by apps/wavcraft/src/notices.rs and shown by about_window() in apps/wavcraft/src/app.rs from Help, About WavCraft. Three tests in notices.rs. Done check: The notices are present before any binary leaves the build machine. Recorded: apps/wavcraft/src/notices.rs tests the_notices_carry_the_mpl_and_font_statements and every_library_in_the_lock_file_is_listed. Open issue to close: #49.

**Remains:**

#### I5: Runs on Windows, Linux and macOS (built, check not recorded)

- **On main:** .github/workflows/check.yml runs fmt, clippy and cargo test --workspace on ubuntu, windows and macos runners. Platform code is on main (apps/wavcraft/src/win_acl.rs, tools/winlist.swift). No launch on a real machine is written down in the repo.
- **Done check:** Built and launched on each. A real episode opened on the Linux handheld and scrubbed without stutter. On the 8 GB Mac the same episode opened, edited and saved.
- **Recorded so far:** not recorded
- **Item issue:** #48

| Step | What | Verify | Size | Kind | By | Blocked by | Issue |
|---|---|---|---|---|---|---|---|
| I5.1 | Create docs/checklists/I5.md: one section per machine (the Windows build machine, the two Linux handhelds, the M1 Mac mini, the 8 GB Mac) with fields for commit, build command, launched, opened, scrubbed, edited, saved, date. | The file exists with five sections, each listing the eight empty fields, and a rule at the top that no episode name or library path is written in it. | easy | docs | script | none |  |
| I5.2 | Hand check on the Windows build machine: release build, launch, open a copy of a real episode, scrub, make one delete, Save As, reopen. | cargo build --release -p wavcraft succeeds; the window opens; the episode shows a waveform; scrubbing end to end has no stutter; the saved file reopens with the shorter length. I write the commit, date and what I saw into the Windows section of docs/checklists/I5.md. | easy | docs | hand, me, windows | I5.1 |  |
| I5.3 | Hand check on the first Linux handheld in SteamOS Desktop Mode: release build in the build container, launch under Wayland, open the real episode with File Open, scrub, make one delete, save. | The window opens and the episode shows a waveform and plays. Dragging the playhead across the full episode at three zoom levels (whole file, about 10 minutes wide, about 10 seconds wide) shows no stutter in sound or picture. One delete saves and the file reopens shorter. I write commit, date and what I saw into the first Linux section of docs/checklists/I5.md. | easy | docs | hand, me, linux | I5.1 |  |
| I5.4 | Hand check on the M1 Mac mini: release build, launch, open a test WAV, make one delete, save, reopen. | On the M1 Mac mini cargo build --release -p wavcraft succeeds, the window opens, a test WAV shows a waveform, one delete saves and the file reopens shorter. I write commit, date and what I saw into the M1 Mac mini section of docs/checklists/I5.md. | easy | docs | hand, me, mac | I5.1, J3.1, the M1 Mac mini is set up |  |
| I5.5 | Hand check on the second Linux handheld in SteamOS Desktop Mode: release build in the build container, launch under Wayland, open a WAV with File Open, make one delete, save. | cargo build --release -p wavcraft succeeds in the build container; the window opens; the file shows a waveform and plays; one delete saves and the file reopens shorter. I write commit, date and what I saw into the second Linux section of docs/checklists/I5.md. | easy | docs | hand, me, linux | I5.1 |  |
| I5.6 | Hand check on the 8 GB Mac: launch the release build, open the same real episode as on the first Linux handheld, make one delete, save, reopen. | On the 8 GB Mac the window opens, the real episode shows a waveform, one delete is saved and the saved file reopens shorter by the deleted length. I write commit, date and what I saw into the 8 GB Mac section of docs/checklists/I5.md. No episode name goes in the file. | easy | docs | hand, me, mac | I5.4, a scheduled session on the 8 GB Mac |  |

### Stage 6: Design pass (the last stage before 1.0)

When every stage above is done, the Project Manager runs the design skill on the running build, screen by screen, against the one design system all my apps share. Each difference that matters (layout, spacing, type, colour, components, states) is filed as an issue labelled `design-pass` and `kind:ui`, linked to its mockup under `docs/design/<date>/`. M3's named sizes and spacing must be in first, so the pass changes values in one place.

**Done when** every `design-pass` issue is closed or moved after 1.0. Then [PIPELINE.md](PIPELINE.md) moves to Prototype / Beta, and I call 1.0.

## After 1.0

Everything outside v1.md's sections A to N: multitrack sessions and the mixer, CD layout, batch processing and extra export formats, third-party plugins, video, and the remaining Audition parity rows. Their issues are on the `After 1.0` milestone with the `after-1.0` label and one `feature:` label each. Code for some of them is already on `main` (the code map marks it "present, out of v1"); M5 keeps it hidden in v1.

## Mac, PC and Linux

- **Code skeleton:** the tree the code map describes: one Cargo workspace, the app in `apps/wavcraft`, the engine in `crates/core`, the same code on all three systems.
- **Memory target:** an 8 GB Mac. All audio lives in the on-disk store; RAM holds a bounded cache; undo costs disk, not RAM (I7, [docs/v1/architecture.md](docs/v1/architecture.md)).
- **Platform-specific code:** audio output and input through `cpal` (WASAPI on Windows, CoreAudio on macOS, ALSA/PipeWire on Linux), including recording devices, their unplug handling (J5) and the macOS microphone permission prompt (J6); file dialogs through `rfd`; free disk space through `sysinfo` and `rustix`; folder access rules on Windows (`apps/wavcraft/src/win_acl.rs`). The code map's "Mac, PC and Linux" section lists every site.
- **Machines:** Windows (the build machine; no microphone, so recording is tested there with a fake input), Linux (the 64 GB Linux tablet and a SteamOS handheld in Desktop Mode; the real microphone tests), macOS on Apple Silicon (the M1 Mac mini first, then the 8 GB Mac, which is the memory target). The steps use these names: the Windows build machine, the 64 GB Linux tablet, a Linux handheld (the tablet or the SteamOS handheld), the M1 Mac mini, the 8 GB Mac.
- **CI:** `.github/workflows/check.yml` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on Linux, Windows and macOS runners, on every push to `main` and every pull request. It cannot play or record audio or open a window, which is why the hand steps exist.
- **Steps that need one system:** linux 41, mac 11, windows 20. The "By" column names it.

## Open-source pieces

In use (root `Cargo.toml`; the code map says where each is used):

- SoundCraft's engine crates, all pinned to one commit of my fork (`e3c5c67`): `soundcraft-audio-io`, `soundcraft-dsp`, `soundcraft-model`, `soundcraft-time`, `soundcraft-mix`, `soundcraft-playback`, `soundcraft-video`, `soundcraft-vst3-host`. MIT OR Apache-2.0. Bump all together, deliberately.
- `symphonia` (MPL-2.0, decoders, unmodified), `vorbis_rs` (BSD-3-Clause; needs a C compiler), `rustfft`, `rayon`, `cpal`, `egui` / `eframe` (fonts under OFL-1.1 and the Ubuntu Font Licence), `rfd`, `arboard`, `bytemuck`, `serde`, `postcard`, `dirs`, `sysinfo`, `rustix`, `getrandom`, `proptest` (tests). `THIRD-PARTY-NOTICES.txt` lists them and a test fails when it misses one (I6).
- My own: `craft-log` (self-logging), `craft-stage`, `wavcraft-automation`.

To add for v1: `craft-mp3` at its `v1.0.0` tag (A3), and the `candle` runner with the Whisper base multilingual weights (L1; the choice and the weights' licence are recorded, L0). The clean-room rule stands: no GPL, LGPL or AGPL code or dependency, and Audacity's code is never read.
