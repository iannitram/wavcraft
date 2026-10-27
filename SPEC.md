# Spec

**1.0 means:** the owner can clean a real podcast episode from start to finish in WavCraft and would rather use it than the tool used today, judged by ear ([docs/v1/v1.md](docs/v1/v1.md), "What v1 is"). All 55 items in sections A to J are done, including MP3 export (A3) and recording (J), and it runs within the memory of an 8 GB Mac (I7).

**Approval:** 2026-10-15 (v1 list approved with changes, [docs/v1/questions-and-answers.md](docs/v1/questions-and-answers.md)); re-confirmed 2026-10-26, the owner: "v1.md unchanged (Recommended)".

A changed Must or Won't sets the approval back to "not yet" until the owner approves again. The full "Done looks like" text for each Must lives in [docs/v1/v1.md](docs/v1/v1.md) and is not copied here, so there is one copy.

## Musts

Each Must is one row of the v1 list. Its "Done looks like" and its check (a test, a measurement written in a report, or by ear) are in that row.

| # | Must | Done looks like | Check |
|---|---|---|---|
| A1 | Open WAV/BWF/RF64, AIFF, MP3, FLAC, streamed into the store | [v1.md A](docs/v1/v1.md#a-files), row A1 | the check in that row |
| A2 | Save and Save As (WAV, AIFF, FLAC), atomic | [v1.md A](docs/v1/v1.md#a-files), row A2 | the check in that row |
| A3 | Save As MP3 (CBR, own pure-Rust encoder) | [v1.md A](docs/v1/v1.md#a-files), row A3 | the check in that row |
| A4 | Convert Sample Type with optional dither | [v1.md A](docs/v1/v1.md#a-files), row A4 | the check in that row |
| A5 | Files panel: several open files | [v1.md A](docs/v1/v1.md#a-files), row A5 | the check in that row |
| A6 | Markers survive a save (cue points and labels) | [v1.md A](docs/v1/v1.md#a-files), row A6 | the check in that row |
| B1 | Waveform per channel, amplitude ruler, vertical zoom | [v1.md B](docs/v1/v1.md#b-seeing-the-audio), row B1 | the check in that row |
| B2 | Spectral Frequency Display: toggle, scale, resolution | [v1.md B](docs/v1/v1.md#b-seeing-the-audio), row B2 | the check in that row |
| B3 | Zoom keys, zoom to selection, zoom navigator | [v1.md B](docs/v1/v1.md#b-seeing-the-audio), row B3 | the check in that row |
| B4 | Levels meter (peak, RMS, hold, clip light) | [v1.md B](docs/v1/v1.md#b-seeing-the-audio), row B4 | the check in that row |
| B5 | Selection/View panel, typed times, time formats | [v1.md B](docs/v1/v1.md#b-seeing-the-audio), row B5 | the check in that row |
| C1 | Time Selection tool and its gestures | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C1 | the check in that row |
| C2 | One channel of a stereo file | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C2 | the check in that row |
| C3 | Snapping (markers, ruler, zero crossings) | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C3 | the check in that row |
| C4 | Spectral selection: Marquee and Lasso | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C4 | the check in that row |
| C5 | Transport: play, loop, return to start, shuttle | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C5 | the check in that row |
| C6 | Scrub by dragging the playhead | [v1.md C](docs/v1/v1.md#c-selecting-and-playing), row C6 | the check in that row |
| D1 | Cut / Copy / Paste / Delete | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D1 | the check in that row |
| D2 | Copy to New / Paste to New | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D2 | the check in that row |
| D3 | Silence and Insert Silence | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D3 | the check in that row |
| D4 | Crop / Trim to selection | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D4 | the check in that row |
| D5 | Undo / Redo and the History panel | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D5 | the check in that row |
| D6 | Zero-crossing adjust | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D6 | the check in that row |
| D7 | Edit crossfades (2 ms / 5 ms) | [v1.md D](docs/v1/v1.md#d-editing-destructive-as-in-auditions-waveform-editor), row D7 | the check in that row |
| E1 | Fade In / Fade Out by handles | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E1 | the check in that row |
| E2 | Amplify | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E2 | the check in that row |
| E3 | Normalize | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E3 | the check in that row |
| E4 | Invert and Reverse | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E4 | the check in that row |
| E5 | Repeat Last Command | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E5 | the check in that row |
| E6 | Preview in every effect dialog | [v1.md E](docs/v1/v1.md#e-effects-one-shot-from-the-effects-menu), row E6 | the check in that row |
| F1 | Effects Rack panel | [v1.md F](docs/v1/v1.md#f-effects-rack), row F1 | the check in that row |
| F2 | Built-in processors (EQ, high-pass, compressor, limiter, de-esser, gate) | [v1.md F](docs/v1/v1.md#f-effects-rack), row F2 | the check in that row |
| F3 | Presets for effects and racks | [v1.md F](docs/v1/v1.md#f-effects-rack), row F3 | the check in that row |
| G1 | Capture Noise Print | [v1.md G](docs/v1/v1.md#g-repair-the-new-dsp), row G1 | the check in that row |
| G2 | Noise Reduction (process) | [v1.md G](docs/v1/v1.md#g-repair-the-new-dsp), row G2 | the check in that row |
| G3 | Match Loudness | [v1.md G](docs/v1/v1.md#g-repair-the-new-dsp), row G3 | the check in that row |
| G4 | Loudness analysis (Amplitude Statistics) | [v1.md G](docs/v1/v1.md#g-repair-the-new-dsp), row G4 | the check in that row |
| H1 | Add Marker | [v1.md H](docs/v1/v1.md#h-markers), row H1 | the check in that row |
| H2 | Markers panel | [v1.md H](docs/v1/v1.md#h-markers), row H2 | the check in that row |
| H3 | Next / previous marker | [v1.md H](docs/v1/v1.md#h-markers), row H3 | the check in that row |
| H4 | Markers move with edits | [v1.md H](docs/v1/v1.md#h-markers), row H4 | the check in that row |
| I1 | Audition default layout | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I1 | the check in that row |
| I2 | Panels shown, hidden, resized; layout remembered | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I2 | the check in that row |
| I3 | Audition default shortcuts and a shortcuts list | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I3 | the check in that row |
| I4 | Never lose work: autosave and recovery | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I4 | the check in that row |
| I5 | Runs on Windows, Linux and macOS (Apple Silicon) | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I5 | the check in that row |
| I6 | Notices file for MPL-2.0 and OFL dependencies | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I6 | the check in that row |
| I7 | Memory fits an 8 GB Mac | [v1.md I](docs/v1/v1.md#i-layout-and-feel), row I7 | the check in that row |
| J1 | Record a take into a new file | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J1 | the check in that row |
| J2 | No playback while recording | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J2 | the check in that row |
| J3 | A crash keeps the take | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J3 | the check in that row |
| J4 | While recording: meter, time, size, disk left, drops | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J4 | the check in that row |
| J5 | Input device chosen and remembered; unplug stops cleanly | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J5 | the check in that row |
| J6 | Recording checked on every machine | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J6 | the check in that row |
| J7 | After Stop, the take opens in the editor | [v1.md J](docs/v1/v1.md#j-recording-added-2026-10-15-my-decision), row J7 | the check in that row |

## Won'ts

Everything in [v1.md, "Deliberately not in v1"](docs/v1/v1.md#deliberately-not-in-v1), in short:

- Multitrack (a later phase of WavCraft itself; never SoundCraft's UI).
- Spot Healing Brush and spectral auto-heal.
- Adaptive Noise Reduction, DeNoise, Hiss Reduction, DeHummer, Click/Pop Eliminator, DeClipper; Sound Remover, DeReverb, Automatic Click Remover.
- AI speech enhancement (a v1.x feature, after a licence check of the model weights).
- Batch processing and Match Loudness on a list of files.
- Video, CD burning, the Diagnostics, Essential Sound and frequency analysis panels, the phase meter.
- AAC/M4A, OGG and Opus export (read-only where the decoder already reads them).
- Mix Paste, the Gain HUD, gain and fade envelope lines, Hard Limiter as a one-shot.
- Editing while playing in a loop, live effects on playback (E6 preview covers it).
- Third-party plugins (CLAP, VST3) in the Effects Rack.
- Recording several mics at once, punch-in, monitoring through WavCraft, recording into an existing file.
- A GPU spectrogram, unless measurements show the CPU picture is too slow.
- **Everything else on the old parity roadmap** ([docs/archive/roadmap-v1-fleet.md](docs/archive/roadmap-v1-fleet.md)) is after 1.0 (the owner's ruling, 2026-10-25): its issues carry the `after-1.0` label. Work for those rows that is already on main or on a branch stays; nothing new is built for them before 1.0.

## Platforms

Mac (Apple Silicon; an 8 GB Mac is the memory target, I7), Windows, Linux (including SteamOS Desktop Mode on the handhelds). See [ROADMAP.md](ROADMAP.md), "Mac, PC and Linux".

## Problem, solution, decisions

The problem, the design and every decision are in [docs/v1/v1.md](docs/v1/v1.md), [docs/v1/architecture.md](docs/v1/architecture.md), [docs/v1/audition-behaviour.md](docs/v1/audition-behaviour.md) and [docs/v1/questions-and-answers.md](docs/v1/questions-and-answers.md). The code map is [docs/code-map.md](docs/code-map.md).
