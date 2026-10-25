# WavCraft roadmap

**Purpose.** This file sorts every feature on the WavCraft parity list into three groups: **Merged** (already in the app on `main`), **PR** (code exists only in an open pull request) and **Unwritten** (no pull request yet), and gives each feature a predicted complexity. It is built from the ranked feature list in `docs/roadmap-audition-parity.md` (open PR #75), which holds the full detail for each row: behaviour, reuse sources and licences.

## Plan

Build every piece first as separate PRs (Qwen on Hyperion and Z13 does the Low-complexity unwritten features, one feature per PR, stacked on the PR it depends on, never merged by the builders). Then a merge phase fits the pieces together; that phase is also where each feature is checked against the vendor's live guide, because some rows were written from memory (verify a row before relying on it).

### Integration checkpoint (2026-10-24)

Draft PR #903 on `integration/branch-triage-2026-10-24` combines the reviewed
branches recorded in [`branch-integration.md`](branch-integration.md). This is
integration-branch status; the category tables below remain the 2026-10-23
snapshot of `main` and open PRs.

| Roadmap area | Available on the integration branch | Remaining work |
| --- | --- | --- |
| A099 time display (F18/F47) | View menu and panels select decimal, compact, samples or six SMPTE rates; the format persists in settings. Frame snapping is available in the app. | Other ruler formats and related preference UI. |
| A257 / A260 Save Selection As and Export (F43) | File menu and control verbs write a WAV copy without changing the open document's save path. | Other output formats and export options. |
| C074 / C097 multitrack base (F103) | New session dialog, basic track/timeline view, track controls and file or document clip import. | Session open/save, undo, clip editing and waveforms, playback, mixdown and disk-backed imports. |
| C078, C082, C083, C094, C104, C105, C110 multitrack children | Session properties and notes, track details and reorder, timeline and ruler models, and track polarity are present in core. | Wire these models into the app, undo and playback/mixdown where relevant. |
| B089 Doppler, B098 stereo width and other core additions | Tested DSP, effect state, metadata and band-planning code is available to later features. | Most of these additions still need app or playback wiring; Doppler audio rendering is separate. |
| B058 EQ graph and M02 envelope tests | EQ graph coordinate and drag model plus validation-order regression tests are present in core. | Connect EQ graph to its dialog; the M02 branch adds tests only. |

The desktop app opened a 48 kHz test WAV and exited cleanly in an Xvfb smoke
run. Strict workspace Clippy and workspace tests gate each committed merge.

### Qwen run log

PRs opened 2026-10-22, all marked alpha and all core only (nothing is reachable from the app yet):

- #473 B019: `rack::apply_with_tail` and `Editor::run_rack_with_tail` are built and tested (one undo step, length grows by the tail); not wired: no Tail field in the rack panel and no control command, and no effect with a tail exists yet.
- #474 B026: `headroom_db` and `amplify_guarded` are built and tested; not wired into the Amplify action, any dialog or the control verbs, no ceiling setting and no Calculate Now button.
- #475 B041: `match_gain_db` and `match_volume` (selection gain to a reference's integrated loudness, one undo step) are built and tested; no menu entry, dialog or reference picker.
- #476 A315: RMS reference (sine or square), account-for-DC and window-width settings are built and tested in core; the Statistics panel and the `STATS` command do not use them yet.
- #477 A214: dynamic and static peak modes, settable hold time and valley tracking are built and tested in core; LED, gradient and range styling and all drawing are not done, and the panel does not call the new methods.
- Rows A033 and B024 were found already built (A033 in #113, B024 in #146), so no new PR was opened for them.

Overnight PRs opened 2026-10-23 (#479 to #501), also alpha and core only; none is reachable from the app yet:

- #479 B169: `AbCompare` (A and B settings for one effect, toggle, copy A to B, differs) is built and tested; the dialog buttons are not wired, nothing is stored per rack slot or in presets.
- #480 A217: `RmsMeter` and `RmsLevels` (sliding-window RMS in dBFS, settable window) are built and tested; the player tap does not feed them and no RMS bar is drawn.
- #481 A319: `band_filter` (Butterworth highpass then lowpass, edges in Hz) and `BandOnly` settings are built and tested; not hooked into playback and no toggle or spectral-selection input.
- #482 B111: 15 named Pitch Shifter presets (octave, fifth, fourth, thirds, semitone, quarter tone, A440 to A432) with lookup are built; no formant presets (the Pitch Shifter has no formant parameter) and no menu entry.
- #483 B112: `fit_ratio` and `target_frames` (stretch ratio to reach a target length, with limits) are built and tested; no dialog, menu or control command.
- #484 B086: a Plate reverb effect (size, decay, damping, pre-delay, diffusion, wet) is registered and tested; only the generic effect dialog shows it, no presets and no tail extension.
- #485 C041: `PunchAgain` (remember the last punch range and re-arm it) is built and tested; no command, key or button, and nothing records a finished punch yet.
- #486 C104: `Multitrack::move_track(from, to)` is built and tested; no drag gesture on the headers, no control verb, no undo.
- #487 C110: per-track polarity flag (`set_invert`, `apply_invert`, saved with the session) is built and tested; the mixdown does not call it and the track header has no button.
- #488 C094: session title and notes get/set with length and text rules, round-tripping through the session JSON, are built and tested; no UI or control verb.
- #489 C111: `split_to_mono` (one stereo track into a left and a right track, settings and playlists copied) is built and tested; no menu, no undo, stereo only.
- #490 C179: step and hold segment shapes for automation keyframes (`SegmentShape`, `shape_value`) are built and tested; playback evaluation still ramps linearly and shapes are not saved.
- #491 C080: `RecentSessions` (newest-first list, dedupe, maximum, prune) is built and tested; not saved to settings, not called from open or save, no menu.
- #492 C202: `plan_record_count_in` (pre-roll frames and the clicks to sound before recording) is built and tested; no recorder uses it and there is no setting in the UI.
- #493 C245: `TrackFade` (fade-in and fade-out lengths, half-track limit, linear curve) is built and tested; not applied on export or in the CUE sheet, no UI.
- #494 A283: three Multitrack preference keys (track height, clip crossfade, clip colour) with ranges and defaults are built and tested; no Preferences page and nothing reads them.
- #495 A326: `SourceLink` (an encoded, parseable link back to the source session) is built and tested; nothing writes it into an exported file and there is no Edit Original action.
- #496 C171: `MasterMode` (mono, stereo, 5.1) with parsing and the mono fold-down rule is built and tested; session creation, mixdown and the dialog do not use it.
- #497 C078: session property setters for opening tempo, meter and sample rate (each returns the old value) are built and tested; no dialog, menu or undo.
- #498 C105: track colour setter, hex parsing and a named palette are built and tested; no picker, no notes box, new tracks do not use the palette.
- #499 A324: `rate_from_fps` and `rate_from_ratio` (a video's frame rate mapped to the nearest SMPTE rate) are built and tested; the app does not read a video's rate and the snap still uses the display format.
- #500 C083: a ruler model (`ruler_ticks`, step table, decimal seconds and minutes:seconds labels) is built and tested; the multitrack view still draws its own seconds-only ruler.
- #501 A287: four Video preference keys (timecode overlay on or off, corner, size, spot frame on scrub) are built and tested; no Preferences page and nothing draws an overlay.

Medium-trial PRs opened 2026-10-23 (#503 to #506), sub-job `a` only (core logic, none wired to the UI; rows in the PR table marked "(part a)" are not complete):

- #503 A077 (part a): `fade_curve` module with `CurveKind` (linear, logarithmic, cosine, exponential), `fade_gain`, `fade_in`, `fade_out` and `curve_from_drag` is built and tested; no handles, drawing or editor action, and not connected to the Fade effects on #91.
- #504 C082 (part a): `TimelineView` (fit, zoom around an anchor, scroll clamping, vertical zoom, time and pixel conversion) is built and tested; `multitrack_ui.rs` still uses its own zoom slider and scroll area. Stacked on #318.
- #505 A040 (part a): `scope_points` (goniometer points), `balance` and `mid_side_rms_db` are built and tested; no Phase Analysis panel, no drawing, no running meter.
- #506 C239 (part a): `parse_batch_args` (the `--batch` command-line parser into a `BatchCli`) is built and tested; no runner, no exit codes, the app does not call it. Stacked on #259.

Medium trial: 4 Medium sub-jobs gave 4 PRs from 4 single Qwen runs with 0 fix runs, plus 1 row (B080, built in #139) that was already built. Qwen time was about 1 to 2 minutes per job; driver time was about 35 to 55 minutes per job (design, hand-computed expected values, mutation checks). The briefs need real-code checks (signatures, SoundCraft crates, target folders) before they go to Qwen.

Rows found already built in this pass: B080 (Output noise only) in #139, and C156 (track pan) through the pan law of the pinned SoundCraft crate in #330 (mixer demo session only).

### Merge order

Each stacked PR needs the PR it sits on merged first. Chains recorded in the PR bodies:

- #326 on #290 (bars and beats, itself on #113); #492 on #326.
- #205 on #113 (time display); #499 on #205.
- #355 on #318 (multitrack session); #493 on #355.
- Straight on #318 (multitrack session): #323, #327, #339, #345, #346, #353, #355, #486, #487, #488, #497, #498, #500. Second level: #490 on #327, #489 on #339, #491 on #346, #496 on #345.
- #364 on #352 (on #342 and #318); #485 on #364.
- On #153 (effect run state, on #106 and #93): #356, #357, #362; #482 on #356, #484 on #357, #483 on #362.
- #473 on #146 (on #106 and #93).
- #475 and #477 on #131; #480 on #131.
- #474 on #91; #476 on #159.
- #494 and #501 on #123.
- Stand-alone on main: #479, #481, #495, #503, #505.
- #504 on #318 (multitrack session); #506 on #259 (batch), which is on main.

### Known conflict points

- The registry and module lines: `effects.rs` (#482, #484 each add a `pub mod` or registry line), `multitrack.rs` (#486, #487, #488, #489, #490, #497, #498 each add one `pub mod` line), plus `lib.rs` (#499, #500, #506), `settings.rs` (#494, #501), `metronome.rs` (#492), `cd.rs` (#493), `meta.rs` (#495), `dsp/mod.rs` (#481, #503), `meters/mod.rs` (#505) and `timepitch.rs` (#483). #504 adds one `pub mod` line to `multitrack.rs`. Each is a one-line clash; keep every line.
- `levels.rs`: #477 (peak modes) and #480 (RMS module) both touch the levels code on top of #131.
- `Settings::save`: it rewrites `settings.txt` from its own five keys, so the new preference lines from #494 are dropped on the next save until the save is changed or the dialog calls the new save; #501 was written to avoid this.

### Rows left for Qwen

Low-complexity rows still unwritten and not suited to a core-only job:

- A044: needs UI panels (#330, #325, Session Properties), so it is not core work.
- B083: Studio Reverb, a preset variant of the reverb in #357.
- C224: Essential Sound presets: vendor-named, and there is no panel to apply them to.
- A187: probably drop.
- C207 and A234: the row text is vague.

Medium rows: the remaining Medium queue lives in the Qwen queue file `wavcraft-medium.md` (about 28 jobs). 7 Medium rows need a decision before they can be queued: B044, B093, B109, C220, C221, C222, C261.

Hard rows have not been started.

**Date (as of):** 2026-10-23.

**How the categories were made.** Categories come from matching feature ids (A###, B###, C###) in pull request text, including the "Roadmap rows ..." lists in open PR bodies, and were corrected after a screening pass of the Low-complexity rows (some were found already built in an open PR). Nothing was built or run to check them, so a row can be marked Merged because a PR names its id while only part of it landed. **The live source of truth is the GitHub PR state**; this file is a snapshot of 2026-10-23 and is not updated by coding PRs.

- **Merged** = the 46 rows the source already marks "have", plus any row whose id appears in a merged PR.
- **PR** = the id appears in an open PR and in no merged PR. The open PR numbers are listed.
- **Unwritten** = everything else. A row that appears only in a closed, unmerged PR is Unwritten and noted "closed PR #n".
- Merged "re-cut" PRs that name only an f-number (#459, #467, #468, #469) take the row ids of the PR they replace, read from the linked roadmap issue (#77, #94, #110, #121). A few open f-number PRs were mapped the same way (#221, #240, #315, #335, #470).

**Complexity rule.** Start from the source's size column: S = Low, M = Medium, L = Hard. Then move up one step (Low to Medium, Medium to Hard; Hard stays Hard) if the row is flagged careful-path (CP), or is marked "wait for upstream" or upstream-in-progress, or has 3 or more unfinished dependencies (a dependency is unfinished unless its row is Merged). The step is applied once, however many of these hold. No other ratings are used. The 46 "already have" rows have no size in the source, so their complexity is shown as n/a.

## Summary

### Rows per category

| Category | Rows |
|---|---|
| Merged | 106 |
| PR | 338 |
| Unwritten | 241 |
| **Total** | **685** |

### Tier by category

| Tier | Merged | PR | Unwritten | Total |
|---|---|---|---|---|
| T0 Seams | 5 | 6 | 0 | 11 |
| T1 Core editing | 12 | 50 | 1 | 63 |
| T2 UI shell and layout | 5 | 88 | 0 | 93 |
| T3 Playback | 0 | 16 | 0 | 16 |
| T4 Recording | 0 | 27 | 2 | 29 |
| T5 Effects rack and core effects | 8 | 38 | 10 | 56 |
| T6 Metering and analysis | 2 | 13 | 3 | 18 |
| T7 Spectral tools and repair | 2 | 13 | 14 | 29 |
| T8 Output, export, batch | 8 | 27 | 20 | 55 |
| T9 Multitrack and everything else | 18 | 60 | 191 | 269 |
| none (already-have rows; the source gives them no tier) | 46 | 0 | 0 | 46 |
| **Total** | **106** | **338** | **241** | **685** |

### Category by complexity

| Category | Low | Medium | Hard | n/a | Total |
|---|---|---|---|---|---|
| Merged | 32 | 20 | 8 | 46 | 106 |
| PR | 170 | 108 | 60 | 0 | 338 |
| Unwritten | 34 | 87 | 120 | 0 | 241 |
| **Total** | **236** | **215** | **188** | **46** | **685** |

## Merged (106)

Rows in the app on `main`. Rows from merged PRs come first, in source rank order, with the PR number; the 46 rows the source already marks "have" come last, in id order, with no tier or size in the source.

| id | tier | feature | complexity | CP | PR | depends-on |
|---|---|---|---|---|---|---|
| B001 | T0 | Effect trait + action table + panel slot | Medium |  | #459 |  |
| A008 | T0 | Panel registry and Window menu | Medium |  | #87 |  |
| B006 | T0 | Effect dialog UI (params, presets, preview, Apply) | Medium |  | #467 | B001 |
| B166 | T0 | Effect undo step labels | Low |  | #459 | B001 |
| B167 | T0 | Process on selection / whole file / channel subset | Low |  | #459 | B001 |
| A110 | T1 | Drag selection edges to adjust | Low |  | #88 |  |
| A111 | T1 | Shift-click to extend selection | Low |  | #88 |  |
| A113 | T1 | Double-click selects visible range | Low |  | #88 |  |
| A114 | T1 | Triple-click selects whole waveform | Low |  | #88 |  |
| A303 | T1 | Generate Tones (merged: B118) | Hard |  | #468 | B001 |
| A122 | T1 | Keyboard selection extension | Low |  | #88 |  |
| A154 | T1 | Repeat Last Command (with/without dialog) (merged: A338, B010, C317) | Medium |  | #467 | B001 |
| A304 | T1 | Generate Noise (merged: B119) | Medium |  | #468 | A303 |
| A297 | T1 | Click state to revert, arrow keys to move through states | Low |  | #99 |  |
| A298 | T1 | Delete single state, Clear History | Low |  | #99 |  |
| A300 | T1 | Audition-style step names | Low |  | #99 |  |
| A308 | T1 | Generator dialogs with preview | Medium |  | #468 | A303 |
| A050 | T2 | Hand tool | Low |  | #234 |  |
| A051 | T2 | Zoom tool | Low |  | #234 |  |
| A084 | T2 | Horizontal scroll (wheel/middle drag) | Low |  | #234 |  |
| A091 | T2 | Right-drag in ruler/navigator to zoom to range | Low |  | #234 |  |
| A236 | T2 | Reveal in Explorer/Finder | Low |  | #396 |  |
| B117 | T5 | DC Offset removal | Low |  | #469 | B001 |
| B057 | T5 | Parametric EQ | Medium |  | #469 | B001, B002 |
| B060 | T5 | High-pass / low-pass quick filters | Low |  | #469 | B057 |
| B056 | T5 | Notch Filter | Low |  | #469 | B002 |
| B031 | T5 | Fade Envelope | Medium |  | #452, #464 | B001 |
| A332 | T5 | Invert (merged: B113) | Low |  | #459 | B001 |
| A076 | T5 | Amplitude HUD (heads-up gain knob) | Medium |  | #454 |  |
| B032 | T5 | Gain Envelope | Medium |  | #453, #466 | B031 |
| A317 | T6 | Phase meter (merged: A039, C062) | Medium |  | #412, #442 | A213 |
| A314 | T6 | RMS histogram | Medium |  | #448 | A313 |
| B048 | T7 | Mark Audio (Mark Audio Pieces) (merged: A186) | Medium |  | #450 | A170 |
| B147 | T7 | Spectral Display fast refresh after edit | Medium |  | #461 |  |
| A272 | T8 | Preserve unknown chunks on save | Hard | CP | #169 |  |
| A265 | T8 | BWF metadata (description, originator, time reference) | Hard | CP | #169 |  |
| A256 | T8 | Write BWF bext on save | Medium | CP | #169 | A265 |
| A036 | T8 | Metadata panel (merged: C249) | Hard |  | #169 | A265 |
| A135 | T8 | Channel names (iXML, labels) | Low |  | #282 | A265 |
| A267 | T8 | RIFF INFO tags | Medium | CP | #282 |  |
| C296 | T8 | WAV channel mask writing/reading | Low |  | #397 | A253 |
| A321 | T9 | Video panel preview (merged: A043, C285) | Hard |  | #354 | A231, C284 |
| A306 | T9 | Generate DTMF signals (older versions; not fetched) (merged: B120) | Low |  | #414 | A303 |
| A322 | T9 | Video panel scaling, best fit, full-screen monitor | Low |  | #455 | A321 |
| B043 | T9 | Echo | Low |  | #405, #419, #440 | B042 |
| B062 | T9 | Chorus (merged: B065) | Low |  | #410 | B001, B002, B063 |
| B063 | T9 | Flanger | Low |  | #415, #446 | B001, B002 |
| B064 | T9 | Phaser | Low |  | #418, #443 | B001, B002 |
| B095 | T9 | Bass Enhancer / Exciter | Low |  | #449 | B088 |
| B100 | T9 | Mid/Side conversion | Low |  | #392 | B027 |
| B101 | T9 | Pan/Expand (waveform) | Low |  | #392 | B027 |
| B110 | T9 | Varispeed | Low |  | #451 | B108 |
| C295 | T9 | Downmix 5.1 to stereo | Low |  | #399 | C293 |
| B059 | T9 | Scientific Filter | Medium |  | #425 | B002 |
| B087 | T9 | Distortion | Medium |  | #403, #432 | B001, B088 |
| C205 | T9 | Tap tempo / detect tempo | Medium |  | #401, #409 | C199 |
| C244 | T9 | CD-Text and ISRC/UPC metadata | Medium |  | #406, #424 | C241 |
| B084 | T9 | Convolution Reverb | Hard |  | #422 | B001 |
| C251 | T9 | Voice-over edit tools | Hard |  | #462 |  |
| C325 | T8 | FLAC, Opus and AAC metadata round-trips | Hard | CP | #460 |  |
| A028 | none | History panel (merged: C316) | n/a |  | already in app |  |
| A059 | none | Waveform display per channel | n/a |  | already in app |  |
| A060 | none | Spectral frequency display (merged: B132, C319) | n/a |  | already in app |  |
| A066 | none | Spectral colour scheme (merged: B134) | n/a |  | already in app |  |
| A081 | none | Selection highlight | n/a |  | already in app |  |
| A083 | none | Mouse-wheel zoom around pointer | n/a |  | already in app |  |
| A085 | none | Zoom in/out horizontally (= and -) | n/a |  | already in app |  |
| A086 | none | Zoom to selection | n/a |  | already in app |  |
| A087 | none | Zoom out full | n/a |  | already in app |  |
| A097 | none | Decimal time (mm:ss.ddd) | n/a |  | already in app |  |
| A106 | none | Timeline ruler with adaptive tick spacing | n/a |  | already in app |  |
| A108 | none | Click places cursor | n/a |  | already in app |  |
| A109 | none | Drag to select a range | n/a |  | already in app |  |
| A116 | none | Select All | n/a |  | already in app |  |
| A117 | none | Deselect All | n/a |  | already in app |  |
| A118 | none | Clear Time Selection | n/a |  | already in app |  |
| A119 | none | Set In/Out point to playhead (I / O) | n/a |  | already in app |  |
| A130 | none | Zero-crossing selection adjusts (6 commands) | n/a |  | already in app |  |
| A139 | none | Cut | n/a |  | already in app |  |
| A140 | none | Copy | n/a |  | already in app |  |
| A141 | none | Paste (insert at cursor / replace selection) | n/a |  | already in app |  |
| A142 | none | Delete | n/a |  | already in app |  |
| A143 | none | Crop | n/a |  | already in app |  |
| A144 | none | Silence (mute selection) (merged: B115) | n/a |  | already in app |  |
| A145 | none | Insert Silence (merged: B116, A302) | n/a |  | already in app |  |
| A155 | none | Undo / Redo with named steps | n/a |  | already in app |  |
| A192 | none | Play / Stop (Space) | n/a |  | already in app |  |
| A193 | none | Play selection only | n/a |  | already in app |  |
| A207 | none | Playback via cpal stream with error recovery | n/a |  | already in app |  |
| A209 | none | Channel downmix for mono device | n/a |  | already in app |  |
| A220 | none | File > Open (WAV, AIFF, FLAC, MP3, OGG, M4A, CAF) (merged: C321) | n/a |  | already in app |  |
| A222 | none | Large files (hours, > 4 GB RF64) | n/a |  | already in app |  |
| A238 | none | Unsaved changes prompt on close/quit | n/a |  | already in app |  |
| A240 | none | Session folder (disk store) for working files | n/a |  | already in app |  |
| A241 | none | Free-space check before open | n/a |  | already in app |  |
| A242 | none | Save (Ctrl+S) (merged: C253) | n/a |  | already in app |  |
| A244 | none | Atomic save, no temp file on failure | n/a |  | already in app |  |
| A296 | none | Shortcuts shown in menus | n/a |  | already in app |  |
| A299 | none | History disk cap | n/a |  | already in app |  |
| A301 | none | History cleared on close | n/a |  | already in app |  |
| A307 | none | Streamed replace/insert producer for generators | n/a |  | already in app |  |
| A331 | none | Open warnings shown | n/a |  | already in app |  |
| A343 | none | Local control channel for scripted tests | n/a |  | already in app |  |
| A344 | none | Self-logging/diagnostics | n/a |  | already in app |  |
| A352 | none | Undo covers edits only, not view | n/a |  | already in app |  |
| C320 | none | Waveform playback with playhead | n/a |  | already in app |  |

## PR (338)

Rows whose code exists only in an open PR. Merge state and conflicts change daily; check the PR itself.

| id | tier | feature | complexity | CP | PR | depends-on |
|---|---|---|---|---|---|---|
| B002 | T0 | Add soundcraft-dsp dependency | Low |  | #91 |  |
| A224 | T0 | Multiple open documents (tabs / Files list) | Hard | CP | #125 |  |
| A348 | T0 | Views and player read from store, no whole-file RAM copy | Hard | CP | #149 |  |
| B003 | T0 | Offline job runner (worker, cancel, progress) | Hard | CP | #109 | B001 |
| B012 | T0 | Float headroom kept above 0 dBFS until save | Medium | CP | #109 |  |
| A170 | T1 | Marker and range data model (merged: C315, B049) | Hard | CP | #100 |  |
| A239 | T1 | Auto Save and crash recovery (merged: C018) | Hard | CP | #103 |  |
| A171 | T1 | Add marker (M or *) | Low |  | #100 | A170 |
| A172 | T1 | Markers panel list (merged: A026) | Medium |  | #179 | A008, A170 |
| A176 | T1 | Next/previous marker | Low |  | #100 | A170 |
| A161 | T1 | Convert Sample Type dialog (Shift+T) (merged: B121, C262, C263) | Hard | CP | #105 |  |
| A123 | T1 | Toggle snapping button | Low |  | #95 |  |
| A127 | T1 | Snap to zero crossings | Low |  | #95 | A123 |
| A237 | T1 | File > Close / Close All / Close Unused Media | Low |  | #145 | A224 |
| A147 | T1 | Copy to New (merged: B164) | Medium |  | #145 | A224 |
| A148 | T1 | Paste to New | Medium |  | #145 | A224 |
| A183 | T1 | Save/load markers in WAV (cue, adtl labl/ltxt, smpl) (merged: C276) | Hard | CP | #117 | A170 |
| A182 | T1 | Markers move with edits | Hard | CP | #117 | A170 |
| A149 | T1 | Mix Paste (merged: C142) | Medium |  | #120 |  |
| A132 | T1 | Enable channels (Edit > Enable Channels) | Low |  | #95 |  |
| A163 | T1 | Dither types and noise shaping (merged: B122, C264) | Hard | CP | #170 | A161 |
| A134 | T1 | Surround 5.1 / up to 32 channel files (merged: C298) | Medium |  | #291 | A132 |
| A121 | T1 | Move playhead to previous/next marker, clip or selection edge | Low |  | #100 | A170 |
| A152 | T1 | Five internal clipboards (Set Current Clipboard) | Low |  | #97 |  |
| A153 | T1 | System clipboard interop | Low |  | #97 |  |
| A136 | T1 | Convert channels (mono/stereo/5.1) with left/right mix % (merged: C265, B123, B124) | Hard | CP | #105 | A161 |
| A162 | T1 | Resample quality + pre/post filter | Hard | CP | #105 | A161 |
| A169 | T1 | Edit > Insert audio from file at cursor | Medium |  | #108 |  |
| A049 | T1 | Time Selection tool | Low |  | #115 |  |
| A058 | T1 | Context-sensitive channel editing | Low |  | #189 | A132 |
| A112 | T1 | Right-click to extend/shorten | Low |  | #115 |  |
| A124 | T1 | Snap to markers | Low |  | #222 | A123, A170 |
| A125 | T1 | Snap to ruler (coarse) | Low |  | #207 | A123 |
| A126 | T1 | Snap to ruler (fine) | Low |  | #207 | A123 |
| A129 | T1 | Snapping menu and ruler context menu | Low |  | #207 | A123 |
| A131 | T1 | Stereo zero-crossing rule matches Audition | Low |  | #222 |  |
| A156 | T1 | Select submenu variants | Low |  | #189 |  |
| A157 | T1 | Edit menu: Enable Channels, Snapping, Marker submenus | Low |  | #189 |  |
| A160 | T1 | Interpret Sample Rate | Medium | CP | #105 | A161 |
| A164 | T1 | Conversion presets | Medium | CP | #105 | A161 |
| A173 | T1 | Rename marker | Low |  | #179 | A172 |
| A174 | T1 | Select markers (click, shift-click, ctrl-click, double-click) | Low |  | #179 | A172 |
| A175 | T1 | Drag marker handles in timeline | Low |  | #179 | A170 |
| A177 | T1 | Convert point to range and range to point | Low |  | #117 | A170 |
| A178 | T1 | Merge markers | Low |  | #228 | A170 |
| A179 | T1 | Delete marker | Low |  | #117 | A170 |
| A180 | T1 | Marker colour/description fields | Low |  | #228 | A170 |
| A181 | T1 | Markers Display columns | Low |  | #179 | A172 |
| A221 | T1 | Drag-and-drop open (merged: C318) | Low |  | #108 |  |
| A150 | T1 | Mix Paste types: Insert, Overlap, Overwrite, Modulate | Medium |  | #120 | A149 |
| A151 | T1 | Mix Paste source: clipboard or file; Loop Paste count | Medium |  | #120 | A149 |
| A184 | T1 | Markers in AIFF (MARK) and other formats | Hard | CP | #228, #308 | A183 |
| A219 | T1 | File > New > Audio File | Hard | CP | #145 | A224 |
| A223 | T1 | Open many files at once | Hard | CP | #220 | A224 |
| A225 | T1 | Open Append to current / to new | Hard | CP | #220 | A161, A224 |
| A273 | T2 | Preferences dialog (Edit > Preferences) (merged: C014) | Medium |  | #123 |  |
| A099 | T2 | Time display format selector (View > Time Display) | Medium |  | #113 |  |
| A090 | T2 | Zoom navigator bar | Medium |  | #119 |  |
| A088 | T2 | On-panel zoom buttons | Low |  | #119 |  |
| A071 | T2 | Amplitude ruler with scale modes | Medium |  | #119 |  |
| A072 | T2 | Vertical (amplitude) zoom | Low |  | #119 | A071 |
| A020 | T2 | Files panel | Hard |  | #180 | A224 |
| A291 | T2 | Keyboard Shortcuts editor (Edit > Keyboard Shortcuts) (merged: C304) | Hard |  | #200 | A295 |
| A295 | T2 | Audition default shortcuts for implemented commands (merged: C303) | Medium |  | #156 |  |
| A009 | T2 | Save custom workspace | Medium |  | #171 | A008 |
| A329 | T2 | Status bar customization menu | Low |  | #230 |  |
| A002 | T2 | Dock panels to edge zones | Hard |  | #128 | A008 |
| A033 | T2 | Time panel | Low |  | #113 | A099 |
| A057 | T2 | Tool switching keys and options bar | Low |  | #115 |  |
| A030 | T2 | Selection/View panel | Low |  | #113 | A099 |
| A098 | T2 | SMPTE formats (24, 25, 29.97 df/ndf, 30, 59.94) | Medium |  | #205 | A099 |
| A128 | T2 | Snap to frames | Low |  | #205 | A098, A123 |
| A003 | T2 | Group panels as tabs | Medium |  | #128 | A002 |
| A069 | T2 | Scroll and zoom the frequency axis | Medium |  | #230 |  |
| A101 | T2 | Bars and Beats format with tempo | Medium |  | #290 | A099 |
| A004 | T2 | Undock panel or frame to floating window | Hard |  | #202 | A002 |
| A120 | T2 | Typed selection start/end/length | Low |  | #113 | A030 |
| A006 | T2 | Maximise panel under pointer with accent key | Low |  | #128 | A008 |
| A007 | T2 | Close panel (Ctrl+W) and scroll hidden tabs | Low |  | #128 | A003 |
| A010 | T2 | Reset and delete workspace | Low |  | #171 | A009 |
| A012 | T2 | Keyboard shortcuts to switch workspace | Low |  | #295 | A009, A291 |
| A018 | T2 | Reset preferences with Shift at launch | Low |  | #123 | A273 |
| A021 | T2 | Files panel metadata columns and sorting | Low |  | #250 | A020 |
| A022 | T2 | Import file into Files panel without opening in editor | Low |  | #250 | A020 |
| A032 | T2 | Zoom panel | Low |  | #242 | A088 |
| A048 | T2 | Panel menus and Metadata Display dialogs | Low |  | #261 | A008 |
| A061 | T2 | Show/hide spectral display, drag divider, double-click handle (merged: B146) | Low |  | #244 |  |
| A062 | T2 | Spectral log/linear frequency (partial and full) | Low |  | #296 |  |
| A063 | T2 | Spectral resolution inc/dec (Shift+Ctrl+Up/Down) (merged: B133) | Low |  | #244 |  |
| A064 | T2 | Spectral windowing function choice | Low |  | #244 | A273 |
| A065 | T2 | Spectral decibel range | Low |  | #244 | A273 |
| A070 | T2 | Frequency ruler | Low |  | #296 |  |
| A073 | T2 | Layered / uniquely coloured channels | Low |  | #241 |  |
| A074 | T2 | Channel indicators in vertical ruler (L, R, C, LFE...) | Low |  | #241 | A071 |
| A079 | T2 | Data under cursor readout | Low |  | #272 | A329 |
| A082 | T2 | High-DPI rendering (merged: C306) | Low |  | #265 |  |
| A089 | T2 | Zoom in at In point / Out point | Low |  | #242 |  |
| A092 | T2 | Zoom to Time presets (5 slots) | Low |  | #242 | A088 |
| A093 | T2 | Zoom factor preference | Low |  | #242 | A273 |
| A095 | T2 | Synchronise selection, zoom and playhead across files | Low |  | #254 | A224 |
| A096 | T2 | Frequency-axis scroll by dragging ruler | Low |  | #296 | A069 |
| A100 | T2 | Samples format | Low |  | #113 | A099 |
| A102 | T2 | Custom frame rate format (2-1000 fps) | Low |  | #252 | A099 |
| A103 | T2 | Compact Disc 75 fps format | Low |  | #252 | A099 |
| A104 | T2 | Jump to time (typed entry) | Low |  | #238 | A033, A099 |
| A105 | T2 | Drag across digits to scrub time | Low |  | #238 | A033 |
| A107 | T2 | Ruler follows time format; subdivision (fine) ticks | Low |  | #238 | A099 |
| A133 | T2 | Channel buttons in amplitude ruler | Low |  | #241 | A071 |
| A146 | T2 | Insert Silence length in current time format | Low |  | #238 | A099 |
| A165 | T2 | Open Convert dialog by double-clicking status bar sample type | Medium | CP | #265 | A161, A329 |
| A226 | T2 | File > Import > File | Low |  | #250 | A020 |
| A235 | T2 | Open Recent | Low |  | #250 |  |
| A278 | T2 | Auto Save | Low |  | #240 | A239 |
| A280 | T2 | Data (crossfade durations, sample rate conversion quality) | Low |  | #236 | A273 |
| A281 | T2 | Media & Disk Cache (temp folders, peak files, DLMS) (merged: C019) | Low |  | #240 | A273 |
| A282 | T2 | Markers & Metadata | Low |  | #236 | A273 |
| A284 | T2 | Playback and Recording (scroll type, JKL speed, auto-scroll) (merged: C017) | Low |  | #236 | A273 |
| A285 | T2 | Spectral Displays (window, resolution, dB range, play only selected frequencies) | Low |  | #274 | A273 |
| A286 | T2 | Time Display | Low |  | #274 | A099, A273 |
| A288 | T2 | Loudness / Diagnostics prefs | Low |  | #274 | A273 |
| A290 | T2 | Restore preferences defaults | Low |  | #277 | A273 |
| A294 | T2 | Conflict detection when assigning | Low |  | #200 | A291 |
| A327 | T2 | Status bar (operation time, transport status) | Low |  | #272 |  |
| A328 | T2 | Right-side info: sample type, uncompressed size, duration, free space, free time | Low |  | #272 | A329 |
| A350 | T2 | Tooltips on tools with shortcuts | Low |  | #272 |  |
| A005 | T2 | Resize panel groups by dividers | Medium |  | #287 | A008 |
| A011 | T2 | Built-in workspaces (merged: C096) | Medium |  | #299 | A009, A273 |
| A013 | T2 | Multiple monitors in workspace | Medium |  | #265, #313 | A004 |
| A014 | T2 | Toolbar docked under menu | Medium |  | #269 | A057 |
| A016 | T2 | Appearance: colours, brightness, gradients, presets (merged: C307) | Medium |  | #293 | A273 |
| A017 | T2 | Export / import application settings | Medium |  | #277 | A009, A273 |
| A023 | T2 | Files panel channel expander | Medium |  | #303 | A020, A134 |
| A027 | T2 | Properties panel | Medium |  | #279 |  |
| A035 | T2 | Tools panel | Medium |  | #269 | A057 |
| A075 | T2 | Peak files for fast reopen | Medium |  | #279 |  |
| A274 | T2 | General (zoom factor, extend selection, context-sensitive channel editing, circular selection, default fade, HUD, sync files) | Medium |  | #274 | A273 |
| A275 | T2 | Appearance | Medium |  | #274 | A273 |
| A289 | T2 | UI language (merged: C308) | Medium |  | #278 | A273 |
| A293 | T2 | Shortcut sets (default, ProTools read-only, custom) | Medium |  | #218 | A291 |
| A351 | T2 | Right-click context menus on editor, ruler, panels | Medium |  | #269 |  |
| A001 | T2 | Default workspace layout | Hard |  | #299 |  |
| A029 | T2 | Editor panel | Hard |  | #299 |  |
| A292 | T2 | Visual keyboard layout editor | Hard |  | #218 | A291 |
| A195 | T3 | Loop playback | Low |  | #137 |  |
| A194 | T3 | Pause | Low |  | #137 |  |
| A196 | T3 | Return playhead to start on stop (Shift+X) | Low |  | #137 |  |
| A034 | T3 | Transport panel | Medium |  | #137 | A195 |
| A080 | T3 | Playhead / current-time indicator | Low |  | #137 |  |
| A201 | T3 | Scrub by dragging playhead | Medium |  | #188 | A080 |
| A202 | T3 | Preroll / postroll (merged: C049) | Low |  | #188 |  |
| A198 | T3 | Rewind / Fast Forward shuttle | Low |  | #188 |  |
| A199 | T3 | JKL shuttle with stepped speed | Medium |  | #188 |  |
| A205 | T3 | Sample-rate conversion to device (merged: C007) | Low |  | #267 |  |
| A208 | T3 | Edit while playing / playback follows edits | Medium |  | #195 |  |
| A094 | T3 | Auto-scroll during playback (pagewise / centered) | Low |  | #137 |  |
| A197 | T3 | Go to previous/next marker or start/end buttons | Low |  | #263 | A176 |
| A212 | T3 | Playhead sync across files | Low |  | #254 | A224 |
| A200 | T3 | Variable playback speed display | Medium |  | #188 | A199 |
| A203 | T3 | Skip selection playback | Medium |  | #137 |  |
| A276 | T4 | Audio Hardware (merged: C001) | Hard |  | #123 | A273 |
| C004 | T4 | Choose input device | Low |  | #123 | A276 |
| C003 | T4 | Choose output device | Low |  | #123 | A276 |
| C020 | T4 | Record into a new waveform file (merged: A342) | Hard | CP | #132 | A276, C004 |
| C022 | T4 | Crash-safe take writing | Hard | CP | #132 | C020 |
| C021 | T4 | New-file recording dialog (rate, channels, bit depth) | Medium | CP | #132 | C020 |
| A213 | T4 | Peak meters L/R in dBFS (merged: A031, B129, C053, C058) | Medium |  | #131 | B002 |
| C023 | T4 | Recording status readout | Low |  | #132 | C020 |
| C024 | T4 | No playback while recording / mute output | Low |  | #132 | C020 |
| C035 | T4 | Record-ready dropped-sample warning (merged: A330) | Medium | CP | #150 | C023 |
| C029 | T4 | Pause recording | Medium | CP | #150 | C020 |
| C028 | T4 | Timed recording | Medium | CP | #150 | C020 |
| A216 | T4 | Meter input signal toggle (merged: C059) | Low |  | #224 | A213, C004 |
| A211 | T4 | Audio Channel Mapping (merged: A277, C008) | Medium |  | #229 | A273, A276 |
| C026 | T4 | Record into an existing file at cursor/selection | Hard | CP | #192 | C020 |
| A019 | T4 | Machine-specific hardware defaults | Low |  | #314 | A276 |
| A206 | T4 | Follow default output device change (merged: C002, C009) | Low |  | #195 |  |
| C013 | T4 | Hardware test / mic check (merged: A341) | Low |  | #224 | A213, C004 |
| C030 | T4 | Record target channel selection | Medium | CP | #192 | C021 |
| C031 | T4 | Start recording immediately with defaults | Medium | CP | #150 | C020 |
| C032 | T4 | Recovered take after crash | Medium | CP | #221 | A239, C022 |
| C033 | T4 | Test input levels before record (arm monitor meter) | Low |  | #224 | A213, C004 |
| C010 | T4 | Latency compensation / record offset (merged: C050) | Hard | CP | #212 | A276 |
| C025 | T4 | Input monitoring (software) | Medium |  | #212 | C020 |
| C027 | T4 | Punch-in recording (Waveform) | Hard | CP | #192 | C026 |
| C034 | T4 | Record from several mics in Waveform | Medium |  | #255 | C020 |
| A210 | T4 | ASIO / CoreAudio / WASAPI choice and buffer size (merged: C005, C012, C006) | Hard |  | #229 | A276 |
| B004 | T5 | Real-time effect Preview (looped) (merged: A340) | Hard |  | #146 | B001, B002 |
| B005 | T5 | Real-time insert path in the player | Hard |  | #146 | B002 |
| B013 | T5 | Effects Rack panel (Waveform editor) (merged: A025) | Hard |  | #146 | B001, B004, B005 |
| B007 | T5 | Effect presets (save/load/delete per effect) (merged: C173) | Medium |  | #146 | B001 |
| B018 | T5 | Apply Effects Rack to selection/file (one undo step) | Medium |  | #146 | B003, B013 |
| A336 | T5 | Fade In / Fade Out commands | Low |  | #91 |  |
| B037 | T5 | Single-band Compressor | Medium |  | #155 | B001, B002 |
| B034 | T5 | Hard Limiter | Medium |  | #155 | B001, B002 |
| B028 | T5 | DeEsser | Medium |  | #155 | B001, B002 |
| B125 | T5 | Loudness measurement (LUFS/true peak) (merged: A318, C064, C065) | Medium |  | #131 | B002 |
| B011 | T5 | Shared STFT / overlap-add engine | Medium |  | #139 |  |
| A337 | T5 | Capture noise print (Shift+P) (merged: B066) | Medium |  | #139 | B011 |
| B067 | T5 | Noise Reduction (process) | Hard | CP | #139 | A337, B003, B011 |
| B126 | T5 | Match Loudness panel (merged: A042) | Hard | CP | #160 | B034, B125 |
| B127 | T5 | Match Loudness: targets (ITU-R BS.1770-3, EBU R128, True Peak, ATSC A/85, Total RMS, Peak) | Hard | CP | #160 | B126 |
| B128 | T5 | Match Loudness: scan, results list, Run | Hard | CP | #160 | B126 |
| C247 | T5 | Podcast loudness presets | Medium |  | #160 | B125 |
| A334 | T5 | Amplify / gain (merged: B025) | Low |  | #91 | B001 |
| B027 | T5 | Channel Mixer (merged: A138) | Medium |  | #167 | B001 |
| B029 | T5 | Dynamics (compressor, limiter, expander, gate in one with curve) | Hard |  | #322 | B001, B002 |
| B038 | T5 | Speech Volume Leveler | Hard |  | #208 | B001, B125 |
| A333 | T5 | Reverse (merged: B114) | Low |  | #91 | B001 |
| A335 | T5 | Normalize (merged: B036) | Low |  | #91 | B001 |
| B014 | T5 | Rack slot power / bypass | Low |  | #146 | B013 |
| B015 | T5 | Slot reorder / drag, replace, remove | Low |  | (#146, unverified) | B013 |
| B016 | T5 | Rack input and output gain | Low |  | #146 | B013 |
| B017 | T5 | Rack Mix (dry/wet) and per-effect wet | Low |  | (#146, unverified) | B013 |
| B019 | T5 | Process-with-tail / Apply to extend length | Low |  | #473 | B018 |
| B021 | T5 | Rack presets (whole-chain) | Low |  | #146 | B007, B013 |
| B022 | T5 | Master bypass of rack / A-B compare | Low |  | (#146, unverified) | B013 |
| B024 | T5 | Re-edit effect in slot from dialog | Low |  | #146 | B006, B013 |
| B026 | T5 | Amplify: clip-guard / Calculate Now | Low |  | #474 | A334 |
| B041 | T5 | Match Volume (gain, selection to ref) (merged: C144) | Low |  | #475 | B125 |
| B165 | T5 | Time Shift (Insert delay) | Low |  | #178 | B001 |
| B169 | T5 | A/B compare effect settings | Low |  | #479 | B006 |
| B055 | T5 | Graphic EQ (10, 20, 30 band) | Medium |  | #216 | B001, B002 |
| A077 | T5 | Fade in/out handles with linear, log, cosine curves (merged: B033) | Medium |  | #503 (part a) | B001 |
| B080 | T5 | Output noise only (monitor residue) | Medium | CP | #139 | B067 |
| A313 | T6 | Amplitude statistics (peak, RMS, DC, clipped, bit depth, dynamic range, loudness) (merged: A038, B053, B131, C070, C071) | Hard |  | #159 | B125 |
| A316 | T6 | Navigate to clipped sample / peak | Low |  | #159 | A313 |
| B130 | T6 | Loudness range (LRA) (merged: C067) | Medium |  | #131 | B125 |
| A309 | T6 | Frequency analysis (FFT graph) (merged: A037, B052, C068) | Hard |  | #198 | B011 |
| A214 | T6 | Dynamic/static peaks, valleys, LED, gradient, ranges (merged: C054, C057, C072) | Low |  | #131, #477 | A213 |
| A215 | T6 | Reset clip indicators (merged: C056, C168) | Low |  | #131 | A213 |
| A217 | T6 | RMS display (merged: C055) | Low |  | #480 | A213 |
| A310 | T6 | Frequency hold snapshots (8) | Low |  | #198 | A309 |
| A311 | T6 | Scan selection | Low |  | #198 | A309 |
| A312 | T6 | Copy graph data | Low |  | #198 | A309 |
| A315 | T6 | RMS settings (sine/square reference, account for DC, window width) | Low |  | #159, #476 | A313 |
| A319 | T6 | Play only selected frequencies toggle | Low |  | #481 |  |
| A040 | T6 | Phase Analysis panel (merged: B051, C063) | Medium |  | #505 (part a) | A317 |
| B047 | T7 | Delete Silence (merged: A158) | Medium |  | #187 | B002 |
| B046 | T7 | DeClicker (Diagnostics) | Hard |  | #211 | B001 |
| B045 | T7 | DeClipper (Diagnostics) | Hard |  | #177 | B001 |
| B050 | T7 | Diagnostics panel (scan list, Scan, Repair All, Repair) (merged: A046, C073) | Medium |  | #311 | B045, B046 |
| B073 | T7 | DeHummer | Medium |  | #292 | B056 |
| B074 | T7 | DeReverb | Hard |  | #214 | B005, B011 |
| B072 | T7 | Automatic Click Remover (merged: B078) | Medium |  | #211 | B046 |
| B135 | T7 | Spectral selection (frequency + time) | Hard |  | #215 |  |
| A052 | T7 | Marquee selection (merged: B136) | Hard |  | #215 | B135 |
| A115 | T7 | Move selection by dragging inside it | Low |  | #215 | A052 |
| B141 | T7 | Spectral selection feather edges | Low |  | #215 | B135 |
| B076 | T7 | Automatic Phase Correction | Medium |  | #217 | B011 |
| B054 | T7 | FFT Filter | Hard |  | #203 | B011 |
| A246 | T8 | Save as AIFF (merged: C255) | Medium | CP | #165 |  |
| A243 | T8 | Save As (Ctrl+Shift+S) | Hard | CP | #165 | A246 |
| A245 | T8 | Sample type on save (16/24/32int/32float) | Medium | CP | #165 |  |
| A253 | T8 | WAV options: 4GB+ via RF64 or multi-chunk, 32-bit float variants (merged: C254) | Medium | CP | #165 |  |
| A247 | T8 | Save as FLAC (merged: C256) | Medium | CP | #165 |  |
| A257 | T8 | Save Selection As (merged: C267) | Medium | CP | #184 | A243 |
| A260 | T8 | File > Export > File (keep current open) (merged: C269) | Medium | CP | #184 | A243 |
| A262 | T8 | Include Markers and Other Metadata option (merged: C266) | Medium | CP | #194 | A183 |
| A263 | T8 | Progress and cancel during save (merged: C272) | Medium | CP | #194 |  |
| B159 | T8 | Multi-file queue and progress (merged: C233) | Hard | CP | #259 | B003, B158 |
| B158 | T8 | Batch Process panel (merged: A259, C231) | Hard | CP | #259 | A224, B008, B159 |
| A268 | T8 | XMP metadata with schemas and custom schemas | Hard | CP | #249 | A036 |
| A254 | T8 | AIFF options: byte order, AIFC | Medium | CP | #286 | A246 |
| A255 | T8 | Audio-only MOV | Low |  | #319, #470 |  |
| A258 | T8 | Save All (merged: C268) | Medium | CP | #145 | A224 |
| A261 | T8 | Estimated file size in dialog | Low |  | #286 | A243 |
| A269 | T8 | Metadata Display sets (show/hide, save sets) | Low |  | #307 | A268 |
| A270 | T8 | Search within XMP | Low |  | #307 | A268 |
| A271 | T8 | AIFF author via Dublin Core Creator | Low |  | #307 | A268 |
| A346 | T8 | Third-party notices file (merged: C313) | Low |  | #277 |  |
| C273 | T8 | Export presets | Low |  | #286 |  |
| A137 | T8 | Extract Channels to Mono Files | Hard | CP | #271 | A246 |
| A185 | T8 | Export audio of selected range markers to files (merged: C240, C248) | Medium |  | #271 | A170, A246 |
| A227 | T8 | Import Raw Data | Medium |  | #304 |  |
| A250 | T8 | Save as OGG Vorbis (VBR quality) (merged: C259) | Hard | CP | #294 |  |
| A252 | T8 | Other libsndfile-type formats (AU, W64, CAF, VOC, etc.) | Hard | CP | #304 |  |
| C239 | T8 | Command line batch | Medium |  | #506 (part a) | B158 |
| C074 | T9 | New multitrack session | Hard |  | #318 |  |
| C097 | T9 | Audio tracks | Medium |  | #318 | C074 |
| C152 | T9 | Mixer panel | Hard |  | #330, #335 | C097 |
| C215 | T9 | Audio types: Dialogue, Music, SFX, Ambience | Low |  | #353 | C214 |
| B081 | T9 | Reverb (algorithmic, generic) | Medium |  | #357 | B001, B002 |
| B148 | T9 | VST3 plugin hosting (merged: C162) | Hard |  | #365 | B002, B005, C157 |
| B088 | T9 | Saturator / soft clip building block | Low |  | #356 | B002 |
| B107 | T9 | Pitch Shifter (Effects > Time and Pitch) | Medium |  | #356 | B001, B002 |
| C174 | T9 | Automation lanes per track | Medium |  | #327 | C097 |
| C178 | T9 | Edit points: add, drag, delete, thin | Medium |  | #327 | C174 |
| A188 | T9 | Playlist panel | Medium |  | #325 | A170 |
| B108 | T9 | Stretch and Pitch (process) | Medium |  | #362 | B001, B002 |
| C098 | T9 | Mono, stereo and 5.1 track formats | Medium |  | #339 | C097 |
| C293 | T9 | 5.1 tracks and master | Medium |  | #345 | C098 |
| C101 | T9 | Video track | Hard |  | #363 | C097 |
| C241 | T9 | CD Layout (Red Book) tracks | Hard |  | #355 |  |
| B042 | T9 | Delay | Low |  | #357 | B001, B002 |
| B103 | T9 | Pitch detection / tracking (YIN) | Low |  | #362 | B002 |
| C039 | T9 | Punch in/out with range | Medium |  | #364 | C036 |
| C084 | T9 | Session markers and ranges | Medium |  | #323 | C074 |
| C201 | T9 | Metronome | Medium |  | #326 | C199 |
| C204 | T9 | Clip tempo/beat metadata (Clip Properties tempo) | Medium |  | #348 | C129, C199 |
| C208 | T9 | Edit clip source in Waveform editor and return (merged: A159, C095, C147) | Medium |  | #340 | C074, C116 |
| C075 | T9 | Save/open session file | Hard |  | #346 | C074 |
| C100 | T9 | Master track | Low |  | #345 | C097 |
| C037 | T9 | Per-track input selection | Medium |  | #352 | C036 |
| C038 | T9 | Overdub while playing existing tracks | Medium |  | #352 | C036 |
| C043 | T9 | Take numbering and take stacking | Medium |  | #352 | C038 |
| A168 | T9 | Edit Tempo | Low |  | #290 | A101 |
| A190 | T9 | Reorder and remove playlist items | Low |  | #325 | A188 |
| A283 | T9 | Multitrack / Multitrack Clips (merged: C016) | Low |  | #494 | A273 |
| A287 | T9 | Video (spot frame, timecode overlay) | Low |  | #501 | A273, A321 |
| A324 | T9 | Snap to video frames and spot frame preview | Low |  | #499 | A128, A321 |
| A326 | T9 | Edit Original link data in mixdown | Low |  | #495 |  |
| B086 | T9 | Plate reverb | Low |  | #484 | B081 |
| B111 | T9 | Pitch presets (octave, formant shift) | Low |  | #482 | B107 |
| B112 | T9 | Time-stretch to fit duration | Low |  | #483 | B108 |
| C041 | T9 | Punch Again | Low |  | #485 | C039 |
| C061 | T9 | Master meter | Low |  | #330 | C060 |
| C078 | T9 | Session properties | Low |  | #497 | C074 |
| C080 | T9 | Recent sessions list | Low |  | #491 | C075 |
| C083 | T9 | Time ruler formats | Low |  | #500 | C074 |
| C094 | T9 | Session notes/metadata | Low |  | #488 | C074 |
| C104 | T9 | Reorder tracks by dragging | Low |  | #486 | C097 |
| C105 | T9 | Track names, colours, notes | Low |  | #498 | C097 |
| C110 | T9 | Invert phase per track | Low |  | #487 | C097 |
| C111 | T9 | Split stereo to mono tracks | Low |  | #489 | C098 |
| C153 | T9 | Mute | Low |  | #330 | C152 |
| C155 | T9 | Track volume fader | Low |  | #330 | C152 |
| C171 | T9 | Master output mode mono/stereo/5.1 | Low |  | #496 | C100 |
| C179 | T9 | Curve/hold shapes for keyframes | Low |  | #490 | C178 |
| C188 | T9 | Mixdown of selected clips | Low |  | #336 | C185 |
| C193 | T9 | Normalize on export | Low |  | #336 | C185 |
| C202 | T9 | Count-in / pre-roll click | Low |  | #492 | C201 |
| C211 | T9 | Insert a waveform file into multitrack | Low |  | #318 | C074 |
| C245 | T9 | Track gap and fades | Low |  | #493 | C241 |
| C131 | T9 | Clip properties panel | Medium |  | #348 | C116 |
| C190 | T9 | Mix down session to new waveform (Bounce to new file) (merged: C212) | Medium |  | #336 | C185, C208 |
| C082 | T9 | Timeline zoom and scroll | Medium |  | #504 (part a) | C074 |
| C156 | T9 | Track pan / balance | Medium |  | #330 (SoundCraft crate) | C152 |
| C322 | T0 | Disk-backed sample source trait for soundcraft-model (unlocks SoundCraft multitrack reuse) | Hard | CP | #315 | A348 |

## Unwritten (241)

Rows with no PR. Where a closed, unmerged PR names the row it is shown in the PR column.

| id | tier | feature | complexity | CP | PR | depends-on |
|---|---|---|---|---|---|---|
| A347 | T1 | Memory fits an 8 GB Mac (merged: C302) | Medium |  |  |  |
| C011 | T4 | Duplex low-latency stream | Hard | CP |  | A276 |
| C015 | T4 | Sample-rate/clock sync (word clock/external) | Hard |  |  | A276 |
| B008 | T5 | Favorites menu / user-recorded actions (merged: A167, C232) | Medium |  |  | B007 |
| B058 | T5 | Parametric EQ graph / interactive display | Medium |  |  | B057 |
| B068 | T5 | NR graph with threshold curve | Hard | CP |  | B067 |
| B035 | T5 | Multiband Compressor | Hard |  |  | B001, B002 |
| A078 | T5 | Default fade type pref and HUD pref | Low |  |  | A077, A273 |
| B009 | T5 | Default Favorites (Fade In/Out, Normalize to 0 dB, Dynamics presets, etc.) | Low |  |  | B008 |
| B023 | T5 | Rack Favorites / Rack to Favorite | Low |  |  | B008, B013 |
| B030 | T5 | Dynamics graph editor (transfer curve) | Medium |  |  | B029 |
| B069 | T5 | DeNoise (real-time) | Hard | CP |  | B005, B067 |
| B168 | T5 | Preview of destructive processes (NR) using partial render | Hard | CP |  | B004, B067 |
| A041 | T6 | Loudness Radar panel (merged: B091, C066) | Hard |  |  | B125 |
| A218 | T6 | Dock horizontally or vertically | Low |  |  | A002, A213 |
| C069 | T6 | Spectral/spectrogram meter during record | Medium |  |  | A309 |
| B071 | T7 | Hiss Reduction | Hard |  |  | A337, B011 |
| A053 | T7 | Lasso selection (merged: B137) | Hard |  |  | A052, B135 |
| A055 | T7 | Spot Healing Brush (merged: B079, B139) | Hard |  |  | A052, B011, B135 |
| B077 | T7 | Auto Heal (spectral) (merged: A339, B143) | Hard |  |  | B011, B135 |
| B075 | T7 | Sound Remover (merged: A047) | Hard |  |  | A337, B011 |
| A067 | T7 | Spectral Pitch Display (older versions) (merged: B105, B145) | Hard |  |  |  |
| A056 | T7 | Add/subtract spectral selection (Shift/Alt drag) | Medium |  |  | A053 |
| A054 | T7 | Paintbrush selection (merged: B138) | Hard |  |  | A052, B135 |
| A068 | T7 | Spectral Phase Display (older versions) | Hard |  |  |  |
| A204 | T7 | Play spectral selection only | Hard |  |  | A052 |
| B070 | T7 | Adaptive Noise Reduction | Hard |  |  | B005, B011 |
| B140 | T7 | Frequency band selection (limit effect to band) | Hard |  |  | B011, B135 |
| B142 | T7 | Effects acting only inside spectral selection | Hard |  |  | A334, B135 |
| B144 | T7 | Spectral Repair / Frequency Repair | Hard |  |  | A055 |
| A345 | T8 | Windows, Linux, macOS builds (merged: C300) | Medium |  |  |  |
| A249 | T8 | Save as MP2 | Hard | CP |  |  |
| A251 | T8 | Save as Monkey's Audio (APE) | Hard | CP |  |  |
| A248 | T8 | Save as MP3 (constant/variable bitrate) (merged: C257) | Hard | CP |  |  |
| A266 | T8 | ID3 tags incl. album art for MP3 | Hard | CP |  | A248 |
| B160 | T8 | Batch: apply Favorites / Effects Rack (merged: C234) | Hard | CP |  | B013, B158 |
| B161 | T8 | Batch: Match Loudness (merged: C236) | Hard | CP |  | B125, B126, B158 |
| B162 | T8 | Batch: Noise Reduction with shared print | Hard | CP |  | B067, B158 |
| C235 | T8 | Output folder, format, name template | Hard | CP |  | B158 |
| C237 | T8 | Batch sample-rate/bit-depth conversion | Hard | CP |  | A161, B158 |
| C260 | T8 | Opus export | Medium |  |  |  |
| C261 | T8 | Other formats (WMA, AMR, APE, CAF write) | Medium |  |  |  |
| C277 | T8 | Streamed export for large files | Hard | CP |  |  |
| A228 | T8 | Read formats: AAC/HE-AAC, AIFF/AIFC, AC-3, APE, AU, AVR, BWF, CAF, EC-3, FLAC, HTK, IFF, M4A, MAT, MPC, MP2, MP3, OGA/OGG, PAF, PCM, PVF, RAW, RF64, SD2, SDS, SF, SND, VOC, VOX, W64, WAV, WMA, WVE, XI | Hard |  |  |  |
| B163 | T8 | Scripting / macro recording of effect sequences (merged: C238) | Hard | CP |  | B008 |
| C258 | T8 | AAC/M4A export | Hard |  |  |  |
| C275 | T8 | Immersive/ADM export | Hard |  |  |  |
| A229 | T8 | AAC gapless trimming and edit list | Medium |  |  |  |
| A230 | T8 | iXML field-recorder metadata (merged: C289) | Medium |  |  | A265 |
| C116 | T9 | Import audio as clips | Hard |  |  | C097 |
| C214 | T9 | Essential Sound panel (merged: A045) | Hard |  |  | C097, C116 |
| C060 | T9 | Per-track meters in Mixer and track headers (merged: C167) | Hard |  |  | C152 |
| C036 | T9 | Arm tracks for recording | Hard |  |  | C004, C060 |
| C199 | T9 | Session tempo and time signature | Hard |  |  | C074 |
| C129 | T9 | Clip time stretch | Hard |  |  | C116 |
| C218 | T9 | Dialogue: Clarity (Dynamics, EQ presets, Enhance Speech) | Hard |  |  | C215 |
| C224 | T9 | Essential Sound presets | Low |  |  | C215 |
| B098 | T9 | Stereo Expander (merged: B099) | Medium |  |  | B001 |
| B153 | T9 | Audio Plug-in Manager | Medium |  |  | B148 |
| C077 | T9 | Session templates | Medium |  |  | C074 |
| C086 | T9 | Time selection and playhead in multitrack | Medium |  |  | C074 |
| C091 | T9 | Multitrack transport and loop playback | Medium |  |  | C074 |
| C099 | T9 | Bus tracks | Hard |  |  | C097 |
| C112 | T9 | Track output routing | Medium |  |  | C099 |
| C146 | T9 | Clip effects (per-clip rack) | Hard |  |  | C116 |
| C226 | T9 | Per-clip non-destructive application | Medium |  |  | C146 |
| C102 | T9 | MIDI tracks | Hard |  |  | C097 |
| C195 | T9 | Remix: fit a music clip to a target duration (merged: C229) | Hard |  |  | C129, C204 |
| C175 | T9 | Track and clip envelopes | Hard |  |  | C174 |
| C223 | T9 | Auto-ducking | Hard |  |  | C175, C215 |
| C185 | T9 | Export Multitrack Mixdown | Hard |  |  | C074 |
| C274 | T9 | Export video with mixed audio | Hard |  |  | C101, C185 |
| C278 | T9 | Import/export Final Cut XML | Hard |  |  | C074 |
| C284 | T9 | Video playback in session | Hard |  |  | C101 |
| C216 | T9 | Dialogue: Loudness (Auto-Match) (merged: C145, C227) | Medium |  |  | B125, C215 |
| C217 | T9 | Dialogue: Repair (reduce noise, rumble, DeHum, DeEss, reverb) | Hard |  |  | C215 |
| A044 | T9 | Session Properties / Mixer / Playlist panels | Low |  |  |  |
| A187 | T9 | CD track markers (merged: C242) | Low |  |  | C084 |
| A191 | T9 | Save playlist in WAV | Low |  |  | A183, A188 |
| A323 | T9 | Timecode overlay | Low |  |  | A321 |
| A349 | T9 | Help menu / About / tutorials (merged: C309) | Low |  |  |  |
| B061 | T9 | EQ analysis overlay (Spectrum behind curves) | Low |  |  | B011, B058 |
| B083 | T9 | Studio Reverb | Low |  |  | B081 |
| C047 | T9 | Record-enable key and transport keys | Low |  |  | C036 |
| C052 | T9 | Recording trouble and error reporting | Low |  |  | C036 |
| C087 | T9 | Edit cursor vs playhead | Low |  |  | C086 |
| C088 | T9 | Snapping | Hard |  |  | C074 |
| C089 | T9 | Snap to options | Low |  |  | C088 |
| C092 | T9 | Loop range playback | Low |  |  | C091 |
| C093 | T9 | Metering of preview/audition | Low |  |  |  |
| C106 | T9 | Track height presets | Low |  |  | C097 |
| C123 | T9 | Clip fade-in/out handles | Hard |  |  | C116 |
| C124 | T9 | Fade curve shapes (linear, cosine, logarithmic) | Low |  |  | C123 |
| C125 | T9 | Crossfades between overlapping clips | Hard |  |  | C123 |
| C126 | T9 | Auto-crossfade on overlap | Low |  |  | C125 |
| C132 | T9 | Lock clips in time | Low |  |  | C116 |
| C133 | T9 | Clip colour and naming | Low |  |  | C116 |
| C135 | T9 | Mute clip | Low |  |  | C116 |
| C166 | T9 | Show/hide mixer sections | Low |  |  | C152 |
| C181 | T9 | Snap keyframes to grid | Low |  |  | C088, C178 |
| C176 | T9 | Automation modes (Off, Read, Write, Touch, Latch) | Hard |  |  | C152, C174 |
| C183 | T9 | Automation Read/write lanes bypass | Low |  |  | C176 |
| C194 | T9 | Render tail for effects | Low |  |  | C185 |
| C207 | T9 | MIDI clip-to-audio for click | Low |  |  | C201 |
| C210 | T9 | Convert multitrack clip to new waveform | Low |  |  | C116 |
| C225 | T9 | Custom presets for the panel | Low |  |  | C224 |
| C230 | T9 | Reset panel | Low |  |  | C226 |
| A024 | T9 | Media Browser panel | Medium |  |  | A020 |
| A189 | T9 | Play list items/loops | Medium |  |  | A188 |
| A231 | T9 | Open audio of video files (AVI, MOV, MP4, MPEG, 3GP, FLV, WMV...) (merged: A320, C287, C213) | Medium |  |  | C101, C284 |
| B020 | T9 | Pre/post fader positioning of slots | Medium |  |  | B013 |
| B039 | T9 | Tube-modeled Compressor | Medium |  |  | B037, B088 |
| B040 | T9 | Sidechain / key input for dynamics | Medium |  |  | B037 |
| B044 | T9 | Dynamic Delay / Echo Chamber | Medium |  |  | B042, B081 |
| B082 | T9 | Full Reverb | Medium |  |  | B081 |
| B089 | T9 | Doppler Shifter | Medium |  |  | B107 |
| B093 | T9 | Vocal Enhancer | Medium |  |  | B028, B057 |
| B106 | T9 | Pitch Bender | Medium |  |  | B107 |
| B109 | T9 | Stretch modes (Audition, iZotope Radius) | Medium |  |  | B108 |
| B150 | T9 | CLAP plugin hosting | Medium |  |  | B002, B005 |
| B151 | T9 | Audio Unit hosting (macOS) | Medium |  |  | B002, B005 |
| B155 | T9 | Plugin state presets and Favorites | Medium |  |  | B007, B148 |
| B156 | T9 | Plugin latency compensation and tail | Medium |  |  | B148 |
| C040 | T9 | Punch and Roll with pre-roll and countdown | Medium |  |  | C039 |
| C051 | T9 | Record to disk (not RAM) for long takes | Hard | CP |  | C036 |
| C079 | T9 | Files panel: import into session | Medium |  |  | C074 |
| C081 | T9 | Session-level undo/redo | Medium |  |  | C074 |
| C085 | T9 | Marker/cue list panel (Markers) | Medium |  |  | C084 |
| C090 | T9 | Scrubbing in multitrack | Medium |  |  | C074 |
| C107 | T9 | Collapse/hide tracks, track groups/folders | Medium |  |  | C097 |
| C108 | T9 | Bounce to new track | Medium |  |  | C097, C185 |
| C109 | T9 | Freeze track | Medium |  |  | C097 |
| C113 | T9 | Track input routing | Medium |  |  | A211 |
| C114 | T9 | Track effect rack per track (inserts) | Medium |  |  | C097 |
| C121 | T9 | Copy, cut, paste, duplicate clips | Medium |  |  | C116 |
| C127 | T9 | Clip gain (on-clip handle) | Medium |  |  | C116 |
| C128 | T9 | Clip gain envelope / volume keyframes | Medium |  |  | C127, C175 |
| C130 | T9 | Clip pitch shift (Properties) | Medium |  |  | C129 |
| C136 | T9 | Selection tools: Move, Razor, Time selection, Slip | Medium |  |  | C116 |
| C118 | T9 | Trim clip edges | Hard |  |  | C116 |
| C137 | T9 | Slip edit (change audio within clip) | Medium |  |  | C118 |
| C141 | T9 | Overlapping clips and clip stacking | Medium |  |  | C116 |
| C150 | T9 | Merge clips / Convert to unique copy | Medium |  |  | C116 |
| C157 | T9 | Track effects inserts (pre/post fader) | Hard |  |  | C152 |
| C161 | T9 | Effects Rack for tracks | Medium |  |  | C157 |
| C164 | T9 | Track EQ section on strip | Medium |  |  | C152 |
| C165 | T9 | Mixer input/output selectors | Medium |  |  | C112 |
| C170 | T9 | Monitor/Control Room output | Medium |  |  | A211 |
| C117 | T9 | Move clips (time and between tracks) | Hard |  |  | C116 |
| C180 | T9 | Move automation with clips | Medium |  |  | C117, C178 |
| C182 | T9 | Copy/paste automation | Medium |  |  | C178 |
| C184 | T9 | Auto-generated volume keyframes (ducking) | Medium |  |  | C223 |
| C187 | T9 | Mixdown channel layouts (mono/stereo/5.1 at once) | Medium |  |  | C185 |
| C198 | T9 | Export with loudness target | Medium |  |  | B125, C185 |
| C200 | T9 | Tempo/meter changes along the timeline | Medium |  |  | C199 |
| C203 | T9 | Bars-and-beats ruler and snapping | Medium |  |  | C199 |
| C209 | T9 | Apply effect to clip source file (destructive) | Medium |  |  | C208 |
| C219 | T9 | Dialogue: Creative (Reverb) | Medium |  |  | C215 |
| C220 | T9 | Music: Loudness, Ducking, Remix (duration), Stereo Width, Dynamics | Medium |  |  | C215 |
| C221 | T9 | SFX: Loudness, Reverb, panner, EQ | Medium |  |  | C215 |
| C222 | T9 | Ambience: Loudness, Reverb, stereo width, dialogue-aware ducking | Medium |  |  | C215 |
| C246 | T9 | Podcast session templates | Medium |  |  | C077 |
| C250 | T9 | Chapter markers for podcasts | Medium |  |  | A248, C084 |
| C271 | T9 | Export queue / background export | Medium |  |  | C185 |
| C286 | T9 | Video/audio sync and frame rate display | Medium |  |  | C284 |
| C291 | T9 | Import/export MIDI files | Medium |  |  | C102 |
| C294 | T9 | Surround Panner | Medium |  |  | C293 |
| C297 | T9 | Surround meters | Medium |  |  | C060, C293 |
| A015 | T9 | Waveform / Multitrack editor switch (key 1) | Hard |  |  |  |
| A166 | T9 | Frequency Band Splitter | Hard |  |  |  |
| A232 | T9 | Import Premiere sequence (.prproj) | Hard |  |  |  |
| A233 | T9 | Open session files (sesx, Audition XML, Premiere XML, FCP XML, OMF) | Hard |  |  |  |
| A279 | T9 | Control Surface (FaderPort V1, HUI) | Hard |  |  | A273 |
| A305 | T9 | Generate Speech (OS voices) | Hard |  |  |  |
| A353 | T9 | Accessibility (screen reader, high-contrast) (merged: C305) | Hard |  |  |  |
| B085 | T9 | Surround Reverb | Hard |  |  | B081 |
| B090 | T9 | Guitar Suite | Hard |  |  | B037, B057, B088 |
| B092 | T9 | Mastering effect | Hard |  |  | B034, B037, B057, B098 |
| B096 | T9 | Dynamic EQ (band-limited dynamic) | Hard |  |  | B037, B057 |
| B097 | T9 | Center Channel Extractor | Hard |  |  | B011 |
| B102 | T9 | Automatic Pitch Correction | Hard |  |  | B103, B107 |
| B104 | T9 | Manual Pitch Correction (graphical) | Hard |  |  | A067, B103 |
| B149 | T9 | Plugin editor windows | Hard |  |  | B148 |
| B152 | T9 | VST (2) plugin hosting | Hard |  |  | B005 |
| B154 | T9 | Plugin scan crash isolation / blacklist | Hard |  |  | B153 |
| B157 | T9 | Plugin parameter automation | Hard |  |  | B148 |
| C044 | T9 | Composite takes / comping | Hard |  |  | C043 |
| C169 | T9 | Time-aligned latency compensation across tracks | Hard |  |  | C157 |
| C196 | T9 | Remix segment detection and quality options | Hard |  |  | C195 |
| C228 | T9 | Enhance Speech | Hard |  |  | C218 |
| C243 | T9 | Burn audio CD | Hard |  |  | C241 |
| C252 | T9 | Speech to text transcription | Hard |  |  |  |
| C280 | T9 | OMF import/export | Hard |  |  | C074 |
| C288 | T9 | Replace audio in exported video | Hard |  |  | C274 |
| C292 | T9 | Timecode sync and external sync | Hard |  |  |  |
| C299 | T9 | Dolby Atmos/immersive | Hard |  |  |  |
| C119 | T9 | Split clip at cursor/selection | Hard |  |  | C116 |
| C042 | T9 | Loop recording with takes | Hard |  |  | C036 |
| C076 | T9 | Session media: consolidate, relink, missing-file handling | Hard |  |  | C075 |
| C158 | T9 | Sends | Hard |  |  | C099 |
| C138 | T9 | Heal separation after delete | Medium |  |  | C119 |
| C139 | T9 | Nudge clips by keys | Medium |  |  | C117 |
| C140 | T9 | Clip snapping to edges and grid | Medium |  |  | C088 |
| C148 | T9 | Offline media / missing file replace | Medium |  |  | C076 |
| C154 | T9 | Solo / Solo Safe | Medium |  |  | C152 |
| C189 | T9 | Mixdown of entire session or time range | Medium |  |  | C185 |
| C191 | T9 | Dither on mixdown | Medium |  |  | A163, C185 |
| C192 | T9 | Bounce report (peaks, LUFS) | Medium |  |  | B125, C185 |
| C197 | T9 | Bounce from command line | Medium |  |  | C185 |
| C301 | T9 | Windows on ARM / Apple Silicon native | Medium |  |  | A345 |
| C045 | T9 | Per-track input monitoring | Hard |  |  | C036 |
| C046 | T9 | Live clip drawing while recording | Hard |  |  | C036 |
| C048 | T9 | Record several tracks at once | Hard |  |  | C037 |
| C103 | T9 | Add/remove/duplicate tracks | Hard |  |  | C097 |
| C115 | T9 | Auto channel conversion between formats | Hard |  |  | C098 |
| C120 | T9 | Delete/ripple delete clips | Hard |  |  | C116 |
| C122 | T9 | Loop clip | Hard |  |  | C116 |
| C134 | T9 | Group clips | Hard |  |  | C116 |
| C143 | T9 | Strip silence / split at transients | Hard |  |  | C119 |
| C149 | T9 | Extract audio from video clip | Hard |  |  | C101 |
| C151 | T9 | Clip variations/playlists | Hard |  |  | C042 |
| C159 | T9 | Sidechain routing | Hard |  |  | C158 |
| C160 | T9 | Track groups / VCA | Hard |  |  | C152 |
| C172 | T9 | Track presets | Hard |  |  | C157 |
| C177 | T9 | Automate effect parameters | Hard |  |  | C157, C174 |
| C186 | T9 | Per-track/bus stems export | Hard |  |  | C185 |
| C206 | T9 | Clips follow session tempo | Hard |  |  | C204 |
| C283 | T9 | OpenTimelineIO exchange | Hard |  |  | C074 |
| C312 | T9 | Scripting / automation API | Hard |  |  |  |
| C163 | T9 | Built-in track effects set (EQ, comp, reverb, delay...) | Hard |  |  | C157 |
| C279 | T9 | AAF import/export | Hard |  |  | C074 |
| C290 | T9 | Pro Tools session read | Hard |  |  | C074 |
| C314 | T9 | AI features (generative, Enhance Speech, tagging) | Hard |  |  |  |
| A234 | T9 | Legacy ASND handling | Low |  |  |  |
| A264 | T9 | Export with Adobe Media Encoder (merged: C270) | Low |  |  | C185 |
| C310 | T9 | Share / upload (Creative Cloud, social) | Medium |  |  |  |
| C281 | T9 | Premiere Pro round trip (Edit in Audition) (merged: A325) | Hard |  |  | C278 |
| C282 | T9 | Dynamic Link | Hard |  |  |  |
| C311 | T9 | Cloud sync / libraries | Hard |  |  |  |
| C324 | T8 | Crash-safe export finalize step | Hard | CP |  |  |
| C323 | T9 | DAWproject interchange | Hard |  |  | C074 |
