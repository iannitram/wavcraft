# How Audition's Waveform Editor behaves: WavCraft's clean-room reference

*Gathered 2026-10-15 by the WavCraft design session, from Adobe's published Audition user guide (helpx.adobe.com, the "desktop" pages, most updated June 2026) and from published loudness standards. Everything here is paraphrased. **Nothing comes from Audacity or any other GPL code.** Audacity can show us what a feature is for. Its code is never read for implementation and never copied. Each section names its Adobe source page. Anything Adobe's pages don't state is marked **(unconfirmed)**, with the choice WavCraft makes until someone checks it in Audition itself. The research notes this was distilled from, with a URL per fact, are in the fleet's shared notes (`wavcraft-audition-docs-notes.md`).*

*Adobe's help site refuses plain fetchers (HTTP 403). The pages were read in a browser.*

## 1. Two editors, one destructive

The Waveform Editor edits one file **destructively**: changes are written into the audio and kept on Save. The Multitrack Editor is non-destructive. The key `1` toggles between them. WavCraft v1 has only the Waveform Editor, and its Multitrack comes later.
*Source: workspace-and-setup/viewing-zooming-navigating-audio.html*

## 2. Keyboard shortcuts

Adobe's table is labelled a *partial* list. These keys are confirmed on Adobe's pages (Windows; on a Mac Ctrl→Cmd, Alt→Option):

| Action | Key |
|---|---|
| Play / stop | Space |
| Playhead to start / end | Home / End |
| Playhead to previous / next marker, clip or selection edge | ~~Ctrl+Left / Ctrl+Right~~ Alt+Left / Alt+Right ("Move Playhead to Previous / Next"; Ctrl+Left / Ctrl+Right scroll the view) (checked in Audition 26.5.0.82, 2026-10-15) |
| Toggle "Return Playhead to Start Position on Stop" | Shift+X |
| Shuttle back / stop / forward (press J or L again to go faster) | J / K / L |
| Zoom in / out in time | `=` / `-` |
| Zoom in / out in amplitude | Alt+`=` / Alt+`-` |
| Add marker | M or `*` (also F8, Shift+8; `*` is the numpad key) (checked in Audition 26.5.0.82, 2026-10-15) |
| Previous / next marker | Ctrl+Alt+Left / Ctrl+Alt+Right |
| Repeat last command with no dialog | Shift+R (also F3) (checked in Audition 26.5.0.82, 2026-10-15) |
| Repeat last command, opening its dialog | Ctrl+R (also F2) (checked in Audition 26.5.0.82, 2026-10-15) |
| Convert Sample Type | Shift+T (also F11) (checked in Audition 26.5.0.82, 2026-10-15) |
| Capture Noise Print | Shift+P |
| Edit only the left / only the right channel | Up / Down |
| Spectral display more log / more linear | Ctrl+Alt+Up / Ctrl+Alt+Down |
| Spectral display fully log / fully linear | Ctrl+Alt+PageUp / Ctrl+Alt+PageDown |
| Spectral resolution up / down | Ctrl+Shift+Up / Ctrl+Shift+Down |
| Step through History (History panel focused) | Up / Down |
| Maximise the panel under the pointer, and back | `` ` `` |
| Wheel over ruler, navigator or waveform | zoom |
| Shift+wheel over the spectral display | more log / more linear |
| Middle-drag or horizontal scroll | scroll |

**Adobe contradicts itself on repeat.** Its Undo/Redo page says Ctrl+R repeats *without* a dialog; the shortcut table (newer) says Shift+R does. WavCraft follows the table. Settled: the table is right. Repeat Last Command (opens the dialog) is F2 or Ctrl+R; Repeat Last Command (no dialog) is F3 or Shift+R (checked in Audition 26.5.0.82, 2026-10-15).

The rest of this table matches Audition's "Default Set" (checked in Audition 26.5.0.82, 2026-10-15; the full list is `../reports/2026-10-15-audition-check/shortcuts-default-set.txt`).

**~~(Unconfirmed)~~ Keys WavCraft uses anyway**, because they are standard or widely cited. Someone with Audition should check them and correct this table: Ctrl+Z undo, Ctrl+Shift+Z redo, Ctrl+X / C / V cut / copy / paste, Delete, Ctrl+A select all, Ctrl+Shift+A deselect, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+T crop. Tool keys: V move, T time selection, E marquee, D lasso, P paintbrush, B spot healing. Also Shift+D for the spectral display, S for snapping, Ctrl+L for loop, Ctrl+Alt+V for paste to new, and F8 as a second add-marker key. Zoom to selection and zoom out full get buttons and menu items; their keys are left blank until checked.

**Checked (checked in Audition 26.5.0.82, 2026-10-15):** every key in the paragraph above is in Audition's Default Set, none wrong. Second keys Audition adds: Redo also Ctrl+Y; Delete also Backspace; Deselect All also Ctrl+D; spectral display also F9. Zoom to Selection is **Shift+S**; Zoom Out Full (All Axes) is **Ctrl+\\**. Also: Record (Toggle Recording) **Shift+Space**; Export File Ctrl+Shift+E; Copy to New Alt+Shift+C; Mix Paste Ctrl+Shift+V; Noise Reduction (process) Ctrl+Shift+P; Match Loudness panel Alt+5; zero crossings Adjust Selection Inward / Outward Shift+I / Shift+O. **Silence, Insert Silence and the History panel have no default key.**

*Sources: keyboard-shortcuts/default-keyboard-shortcuts.html; importing-recording-and-playing/navigating-time-playing-audio.html; editing-audio-files/undo-redo-history.html; workspace-and-setup/customizing-workspaces.html, viewing-zooming-navigating-audio.html*

## 3. Selecting

- **Time Selection:** drag in the Editor. Drag an edge to resize it, and Shift-click past an edge to extend it. With the preference on, right-click extends or shortens it too. **Double-click selects the visible range; triple-click selects the whole file.**
- **Channels:** a selection covers every channel by default. Edit > Enable Channels, the channel buttons on the amplitude ruler, or Up / Down make only one channel active. Dragging along the very top or bottom edge selects one channel **only** when the preference "Allow context-sensitive channel editing" is on. That preference is **off** by default (checked in Audition 26.5.0.82, 2026-10-15).
- **Spectral selection:** Marquee (rectangle), Lasso (freehand) and Paintbrush (freehand, with Size and Opacity giving a graded strength). Drag inside to move a selection and drag corners or edges to resize. Lasso and Paintbrush add with Shift-drag and subtract with Alt-drag. By default, playback plays only the selected frequencies. Adobe documents **no feather** for Marquee/Lasso; Paintbrush Opacity is its graded mechanism. *WavCraft: Marquee and Lasso get a small fixed feather so applying an effect never clicks (our choice). Paintbrush comes after v1.*
- **Spot Healing Brush:** paint over a noise in the spectral display and it is repaired automatically, for selections of 4 s or less. Out of v1.
- **Zero crossings:** Edit > Zero Crossings > Adjust Selection Inward (and outward, left and right variants).
- **Edit crossfade:** Audition crossfades every edit join, with the length set in Preferences > Data. Defaults: "Smooth delete/cut boundaries with crossfades of 2 ms" on, and "Smooth all edit boundaries by crossfading 5 ms" on (checked in Audition 26.5.0.82, 2026-10-15). *(WavCraft's choice: [questions-and-answers.md](questions-and-answers.md) Q6.)*
- **Snapping:** a toggle above the Editor. Edit > Snapping offers markers, ruler coarse or fine (one at a time), zero crossings and frames. Snapping also moves the playhead.
- **Selection/View panel:** Begin, End and Duration for the selection and for the view, in the current time format, and you can type into them.
- **Preference:** keep selection, zoom and playhead in step across open files.

*Sources: editing-audio-files/selecting-audio.html; workspace-and-setup/viewing-zooming-navigating-audio.html*

## 4. Cut, copy, paste, delete

- **With nothing selected, Copy and Cut act on the whole file.**
- Paste goes in at the playhead, or replaces the selection. **Copy to New** and **Paste to New** make a new file (Paste to New takes the clipboard's sample type).
- **Mix Paste** dialog: copied and existing volume %, invert copied audio, crossfade in ms, and a paste type of Insert, Overlap (mix), Overwrite or Modulate. It can paste from the clipboard or a file, and loop the paste N times. *Out of v1.*
- **Delete** removes the selection and closes the gap: that holds only when all channels are active. With only one channel of a stereo file active, Delete **silences that range in that channel and keeps the file length**; the other channel is untouched, and History names it "Delete Audio" (checked in Audition 26.5.0.82, 2026-10-15). **Crop** keeps only the selection.
- **Checked in Audition 26.5.0.82, 2026-10-16** (audition-check Part 2 items 4–6; test copies only, every edit undone, nothing saved):
  - **With nothing selected** (cursor only), Edit > **Crop, Cut and Delete are greyed out** and Ctrl+T does nothing (History unchanged, length unchanged, Save stays greyed). Copy stays available. This contradicts the first bullet above for **Cut** in this version; Copy with no selection was not isolated. *WavCraft:* a Crop, Cut or Delete of an empty range is **no edit**: the document is unchanged and no history step is added.
  - **Crop with one channel active** (stereo, left only, 1–3 s selected): Audition **silences everything outside the selection in that channel and keeps the file length**; the other channel is untouched. History: "Crop Audio".
  - **Cut with one channel active** (right only, 1–3 s): **silences that range in that channel and keeps the length**, like Delete; the cut range goes on the clipboard. History: "Cut Audio".
  - **Paste with one channel active** (right only, cursor at 6 s, a 2 s clip): the file **stays the same length** and the pasted range (6–8 s) is selected, so it **overwrites** that stretch of the active channel rather than inserting. History: "Paste Audio". (Measured by the length and the selection only: the clip and the file held the same tone.)
  - **Stereo pasted into mono:** Audition writes the **mix (L+R)/2**. Pasted from a file with 440 Hz on the left and 660 Hz on the right, each at 0.5 peak: Amplitude Statistics read peak −6.44 dB, maximum sample 15613. Left alone would peak at 16384 (−6.02 dB); (L+R)/2 computes to 15614 (−6.44 dB); a sum to −0.42 dB. **WavCraft's (L+R)/2 matches.** Screenshot `../reports/2026-10-15-audition-check/stereo-into-mono-amplitude-stats.jpg`.
- **Silence** turns the selection to silence and keeps its length. **Insert Silence** (Edit > Insert > Silence) puts N seconds in at the playhead and pushes the rest later.
- **Invert** (180° phase) and **Reverse** act on the selection, or on the whole file if nothing is selected.

*Sources: editing-audio-files/copying-cutting-pasting-deleting-audio.html; editing-audio-files/inverting-reversing-silencing-audio.html*

## 5. Undo and History

- Adobe describes undo as effectively **unlimited**. Undo and Redo name the step ("Undo Delete").
- **History panel:** click any state to go back to it, and step with Up / Down. The panel menu has **Clear History**. Deleting one selected state also removes its undo. Adobe suggests clearing history on very large files to save disk and speed.
- **History is gone when the file is closed.** Undo doesn't survive closing.
- **Repeat Last Command** works for most commands (Delete is an exception); keys in section 2.

*WavCraft:* the same, except that history is capped by memory ([architecture.md](architecture.md)) and the panel says when old steps are dropped.

*Source: editing-audio-files/undo-redo-history.html*

## 6. Amplitude and fades

- **Fade handles** at the top left and top right of the waveform: drag inward for the length. Dragging level gives a **linear** fade, dragging up or down gives **logarithmic**, and holding Ctrl gives **cosine** (an S-curve). A preference can make cosine the default, and the keys then give the others. Destructive in the Waveform Editor. Favorites > Fade In / Fade Out exist too.
- **Amplify:** a gain slider per channel, with Link Sliders. *(Range: sliders marked −80 to +40 dB, default 0 dB, Link Sliders on (checked in Audition 26.5.0.82, 2026-10-15); WavCraft: −96 to +48 dB.)*
- **Normalize (process):** Normalize To a % of full scale or, with the dB box ticked, dB (100 % = 0 dBFS; Adobe suggests −3 to −6 dBFS before mastering). One gain for the whole selection. **Normalize All Channels Equally**: on means one gain from the loudest channel, off means a gain per channel. **DC Bias Adjust** (0 % centres the waveform). While its dialog is open, you can't edit, change the selection or move the playhead.
- **Hard Limiter:** Maximum Amplitude (≤ −0.3 dB for 16-bit), Input Boost, Look-Ahead (≥ 5 ms), Release (default 100 ms), Link Channels.
- **Gain HUD:** a floating knob over the waveform that applies relative gain to the selection, or the whole file. *Out of v1.*

*Sources: effects-reference/amplitude-compression-effects.html; editing-audio-files/visually-fading-changing-amplitude.html*

## 7. Effects Rack (Waveform Editor)

- Up to **16 slots**. Insert from a slot's menu, reorder by dragging, remove from the slot menu or with Delete. Remove All Effects is in the panel menu.
- **Input and Output** level knobs with meters, and a **Mix** slider (100 % wet, 0 % dry).
- A power button on each effect and a main power button (A/B compare).
- **Process** (selection or entire file) and **Apply**, which writes the rack into the audio. *(Exact menu wording unconfirmed.)*
- Playback previews the rack live. Presets exist for the whole rack and for each effect. Ctrl+C / Ctrl+V copy effects between racks.
- **"(process)" effects** (Noise Reduction, Hiss Reduction, Normalize) are offline, Waveform-only, and **cannot go in the rack**. They run from the Effects menu.
- **One-shot effects** from the Effects menu open a dialog with Preview (play), a power button for A/B, and Apply. While previewing you can still move the selection and playhead (not for Normalize).

*Sources: applying-effects/effects-controls.html; applying-effects/applying-effects-waveform-editor.html*

## 8. Noise Reduction (process)

**The workflow:**
1. Select at least about 0.5 s of noise only.
2. Effects > Noise Reduction/Restoration > **Capture Noise Print** (Shift+P). This is disabled if the selection is too short for the FFT size. A Marquee selection captures noise in a frequency band only.
3. Select what to clean, or use **Select Entire File** in the dialog.
4. Effects > Noise Reduction/Restoration > **Noise Reduction (process)**, set it, Preview, Apply.

Adobe advises removing DC offset first, or quiet passages may click. It also suggests recording a few seconds of room tone for the print.

**The dialog:**

| Control | What it does (Adobe's advice) | WavCraft default (ours; ~~Adobe's are unconfirmed~~ Audition's are in the next column) | Audition default (checked in Audition 26.5.0.82, 2026-10-15) |
|---|---|---|---|
| Graph | Frequency across, amount of reduction up. A blue control curve scales reduction by frequency and can be dragged. Reset flattens it | Flat curve | *(not checked)* |
| Noise Floor display | High (most noise found per frequency), Low (least), Threshold (below this, reduction happens) | Threshold shown | *(not checked)* |
| Scale | Log or linear frequency | Log | *(not checked)* |
| Channel | Which channel the graph shows. Reduction is the same for all channels | First channel | *(not checked)* |
| Noise Reduction % | How much of the reduction reaches the output. Too high can sound flanged | 80 % | 50 % (slider 0–100 %) |
| Reduce By (dB) | How far noise is turned down. **6 to 30 dB** works well; lower it if you hear bubbles | 12 dB | 16 dB (slider 1–100 dB, logarithmic) |
| Output Noise Only | Play only what is being taken out | Off | *(not checked)* |
| Spectral Decay Rate (%) | How much keeps being processed after audio drops below the floor. **40–75 %**; lower sounds bubbly, higher leaves noise | 60 % | 65 % (range *unconfirmed*) |
| Smoothing | Allows for variance of noise per band. Up to about 2 reduces burbling but raises broadband noise | 1 | 1 (range *unconfirmed*) |
| Precision Factor | **5–10** best, odd values ideal. 3 or less processes in large blocks (level dips), more than 10 gains nothing and is slower | 7 | 7 (range *unconfirmed*) |
| Transition Width (dB) | The range between noise and wanted audio. 0 is a hard gate per band. Example: width 10, floor −60: −60 is untouched, −62 slightly reduced, −70 removed | 10 dB | 0 dB (range *unconfirmed*) |
| FFT Size | The biggest quality lever. **4096–8192** recommended. Bigger is finer in frequency but can swoosh; smaller responds faster in time but can sound hollow | 4096 | 4096 (choices 512, 1024, 2048, 4096, 8192, 16384) |
| Noise Print Snapshots | How many snapshots make up the print. 4000 "optimal" | 4000 | 4000 |

**Noise print files:** Save Current Noise Print writes a file holding the sample type, the FFT size and three sets of coefficients (lowest noise found, highest, average power). Load only applies to the same sample type: a 22 kHz mono print can't be used on 44 kHz stereo. *WavCraft keeps the same three statistics in its own file format (not Adobe's `.fft`), and re-bins a print when the FFT size changes.*

**Related effects, after v1:** Adaptive Noise Reduction (live, learns noise from the selection's first seconds), DeNoise (one Amount slider plus a frequency focus), Hiss Reduction (process, captures a noise floor like a print), DeHummer (root frequency, Q, gain, number of harmonics, harmonic slope, output hum only), Automatic Click Remover, Click/Pop Eliminator, DeReverb, Sound Remover.

*Source: effects-reference/noise-reduction-restoration-effects.html*

## 9. Match Loudness

- **Window > Match Loudness.** It was called Match Volume until Audition CC 2015.1. Add files by dragging, with the button, or by double-clicking the empty panel. **Scan** measures loudness and amplitude statistics per file. Open **Match Loudness Settings**, pick **Match To** (a standard), set the **target loudness**, **tolerance** and **Maximum True Peak Level**, then **Run**. Files are processed in parallel.
- **True-peak limiting** keeps the output from ever going over the ceiling.
- **Export option:** filename prefix and suffix (from metadata templates), location, format, sample type, format settings, remove or close files when done, and include markers and metadata. Without Export, files are corrected in place, open and unsaved. *(In-place wording unconfirmed.)*
- **Standards Adobe lists:** ITU BS.1770 (-1, -2, -3; -3 only drops the optional filters from true-peak measurement), EBU R128 (−23 LUFS ±0.5, true peak −1 dB), its short-form and Tech 3341–3344 variants, ATSC A/85 (−24 LUFS ±2, true peak −2 dB), FreeTV OP-59, ARIB TR-B32, PRSS (NPR).
- **Default** (Adobe's blog): BS.1770-2 at −23 LUFS, and the last settings used become the new default. In today's UI the default is **ITU-R BS.1770-3 Loudness at −23 LUFS**, Tolerance 0.5 LU, Max True Peak Level −1 dBTP (checked in Audition 26.5.0.82, 2026-10-15).
- ~~*(Unconfirmed)*: the exact Match To list in today's UI (loudness, total RMS, peak), and a Use Limiting checkbox with Look-Ahead and Release controls.~~ **Match To list:** ITU-R BS.1770-3 Loudness (default), ATSC A/85 Loudness, EBU R128 Loudness, Total RMS, Total RMS (Match to File), Peak Amplitude, True Peak Amplitude, Loudness (Legacy), Perceived Loudness (Legacy). **Limiter:** a **Use True Peak Limiting** checkbox (ticked by default) with **Look-Ahead Time 12 ms** and **Release Time 200 ms** (checked in Audition 26.5.0.82, 2026-10-15).

**Podcast targets (not Adobe):**

| Who | Target |
|---|---|
| Apple Podcasts | about −16 LKFS ±1 dB, true peak ≤ −1 dBFS, measured per ITU-R BS.1770 (podcasters.apple.com/support/audio-requirements) |
| AES TD1008 | −18 LUFS for speech / podcast-style content (as reported; read the AES paper for its true-peak figure) |
| Spotify (music) | −14 LUFS, true peak below −1 dBTP |
| Common mono rule of thumb | −16 LUFS stereo is about −19 LUFS mono (podnews, not a standard) |

*WavCraft v1:* Match Loudness works on the open file. Its presets are Apple Podcasts (−16 LUFS / −1 dBTP), AES speech (−18 / −1), EBU R128 (−23 / −1) and a custom one. The default is my answer to [questions-and-answers.md](questions-and-answers.md) Q2, and Apple's until I answer. A list of several files is after v1.

*Sources: editing-audio-files/match-loudness.html; blog.adobe.com 2014-10-16 "Audition CC as a loudness correction appliance"*

## 10. Markers

- Two kinds: **point** markers and **range** markers (a start and an end, both draggable). They show on the timeline at the top of the Editor.
- **M** (or the Markers panel's Add button) adds one at the playhead, while playing or stopped. With a selection, it adds a range marker.
- **Markers panel:** rename by editing the name, and edit Start (and End and Duration for ranges). Columns can be shown or hidden. Shift- or Ctrl-click selects several. Double-click moves the playhead there and selects a range. **Merge** keeps the first name and turns the merged markers into a range. Right-click a marker for Convert to Range, Convert to Point or Delete Marker.
- Edit > Marker > Move Playhead to Next / Previous.
- Exporting the audio under selected range markers to separate files, and the Playlist panel, are *out of v1*.
- **Saving:** markers are kept only when "Include markers and other metadata" is ticked on save. Audition stores metadata as XMP inside the file, and WAV keeps BWF too. *(How markers sit in a WAV file is unconfirmed.)* *WavCraft:* writes markers as standard RIFF `cue ` points with `LIST`/`adtl` labels (`labl`, and `ltxt` lengths for ranges), the common form other tools read, with "include markers" on by default. Whether Audition reads them back is the check in [v1.md](v1.md) A6. Marker types (CD Track, Cart Timer and so on) are left out: CD authoring was removed in Audition 26.3.

*Source: editing-audio-files/markers.html*

## 11. Saving and sample types

- **Save** writes the current file; **Save As** writes a new name and you then edit that one; **Export > File** writes a copy and keeps the current file open; also Save Selection As and Save All.
- The save dialog shows Sample Type (Change… opens Convert Sample Type), Format Settings, Include Markers and Other Metadata (**ticked** by default, checked in Audition 26.5.0.82, 2026-10-15), and an estimated file size.
- WAV: for 32-bit, int or float; files over 4 GB as RF64. **MP3:** constant or variable bit rate, with Bitrate (CBR) or Quality (VBR). *(Audition: Type Constant (default) or Variable; bit rates 16, 18, 20, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320 Kbps (the low ones resample to 8–32 kHz); default **192 Kbps CBR**; encoder LAME 3.100 (checked in Audition 26.5.0.82, 2026-10-15). WavCraft offers 64 to 320 kbps CBR, default 128 kbps mono / 192 kbps stereo.)*
- **Convert Sample Type** (Edit menu, Shift+T): sample rate, channels (mono, stereo, with mix %), bit depth with dither on or off, dither type (Triangular is the best trade-off; Gaussian is also offered), noise shaping. *(Audition's default when lowering the bit depth: Dithering **Disabled**, Dither Type Triangular, Noise Shaping None (checked in Audition 26.5.0.82, 2026-10-15). WavCraft: Triangular, on, no noise shaping.)*
- **Interpret Sample Rate** changes the rate in the header without resampling (pitch and speed change). *After v1.*

*Sources: saving-and-exporting/saving-exporting-files1.html; editing-audio-files/converting-sample-types.html*

## 12. Seeing: layout, zoom, meters, time

- **Zoom:** the zoom navigator bar along the top shows the whole file, and you drag it to scroll. Right-drag on it or on the ruler zooms to a range. Buttons at the lower right of the Editor: Zoom In, Zoom Out, Zoom In at In Point, Zoom In at Out Point, Zoom to Selection, Zoom Out Full. Right-drag on the spectral frequency ruler zooms in frequency.
- **Auto-scroll** while playing: page by page, or centred.
- **Amplitude ruler** (right-click): dB, %, sample values, or normalised (−1 to 1).
- **Spectral display:** a toolbar button, or double-click the divider. Colours run from dark blue (quiet) to bright yellow (loud). Preferences cover windowing function (Hamming or Blackman recommended), resolution and dB range.
- **Time display** at the lower left of the Editor. **Decimal (mm:ss.ddd) is the default.** Also SMPTE rates, samples, bars and beats, and a custom fps.
- **Transport:** Stop, Play, Pause, previous, rewind, fast-forward, next, record, Loop Playback. Right-click Play for Return Playhead to Start Position on Stop and Play Spectral Selection Only.
- **Levels meter:** dBFS with 0 at the top. Horizontal: left on top, right below. Peak hold is yellow for 1.5 s. A red clip light stays on until you click it. Right-click offers the range, show valleys, a colour gradient (off means yellow from −18 and red from −6 dBFS), LED style, and dynamic or static peaks. *(Audition's default: range **60 dB** (choices 120, 96, 72, 60, 48, 24 dB) with **Dynamic Peaks**; Show Valleys, Color Gradient and LED Meters off (checked in Audition 26.5.0.82, 2026-10-15). WavCraft uses −60 to 0 dBFS.)*
- **Workspaces:** Window > Workspace. Panels dock, group and float, and custom workspaces can be saved. Audition's named workspaces: Advanced Mixing, Classic, Default, Edit Audio to Video, Essential Video Mixing, Loudness, Mastering and Analysis, Maximum Editing (Dual Monitor), Radio Production, Restoration, Simple Editing. The Default layout: Files and Favorites top left; Media Browser / Effects Rack / Markers / Properties below; History and Video bottom left; Editor and Mixer in the centre; Essential Sound on the right; Levels across the bottom; Selection/View bottom right (screenshot `../reports/2026-10-15-audition-check/default-layout.jpg`) (checked in Audition 26.5.0.82, 2026-10-15). Which panels the other named workspaces hold is still *(unconfirmed)*; WavCraft uses the layout drawn in [app.md](../app.md).

*Sources: workspace-and-setup/viewing-zooming-navigating-audio.html; editing-audio-files/displaying-audio-waveform-editor.html; importing-recording-and-playing/navigating-time-playing-audio.html, monitoring-recording-playback-levels.html; workspace-and-setup/customizing-workspaces.html*

## Things to check in Audition itself

Audition is installed on Hyperion (my answer, 2026-10-15). The check is [tasks/audition-check.md](tasks/audition-check.md); it updates this file in place. What it settles:

1. ~~The unconfirmed keys in section 2 (the tool letters, Shift+D, S, the save and undo keys), plus **Audition's record key**.~~ Done 2026-10-15 (section 2; record is Shift+Space).
2. ~~Noise Reduction's default settings.~~ Done 2026-10-15 (section 8), except the ranges of the four Advanced numbers.
3. ~~The default edit crossfade length (Preferences > Data).~~ Done 2026-10-15 (section 3: 2 ms delete/cut, 5 ms all edits, both on).
4. ~~Match Loudness: the exact Match To list, and whether there are limiter controls.~~ Done 2026-10-15 (section 9).
5. ~~What Delete does with only one channel of a stereo file active.~~ Done 2026-10-15: Audition **silences** that range in the active channel and keeps the file length (it neither deletes on both channels nor refuses). Section 4's "Delete removes the selection and closes the gap" holds only when all channels are active; task 04 assumed both channels.
6. ~~Crop with no selection; Crop, Cut and Paste with one channel active; stereo pasted into mono.~~ Done 2026-10-16 (section 4).
7. *(Still open: audition-check Part 2 items 1–3, after task 08.)* Whether Audition reads back markers from a WAV that WavCraft wrote, and how Audition itself stores them.
