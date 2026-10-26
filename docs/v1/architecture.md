# WavCraft architecture

*Written 2026-10-15 by the WavCraft design session (laptop). Clean room: nothing here comes from Audacity's code or any other GPL code. The SoundCraft crate names and purposes come from [app.md](../app.md) and [../SoundCraft/app.md](../SoundCraft/app.md), **not from reading code**. This was written without the WavCraft repo open: the laptop has no clone. Where this file says "today's code", it means what app.md says the viewer does. Task 02 checks it against the real tree and corrects this file.*

## The shape in one picture

```
            ┌──────────────────────── wavcraft (app, egui/eframe) ────────────────────────┐
            │ panels · action table (menus + shortcuts) · tools · dialogs · playback output │
            └───────┬──────────────────────┬───────────────────────────┬───────────────────┘
                    │ commands             │ snapshots                 │ jobs
            ┌───────▼────────┐    ┌────────▼─────────┐      ┌──────────▼──────────┐
            │ core::doc      │    │ core::view       │      │ core::jobs          │
            │ blocks, pieces │◄───│ peaks, spectro-  │      │ effects on a worker │
            │ edits, history │    │ gram + overview  │      │ (cancel, progress)  │
            │ markers, sel.  │    │ caches           │      └──────────┬──────────┘
            └───────┬────────┘    └──────────────────┘                 │
                    │ open/save                                        │
            ┌───────▼────────┐                               ┌─────────▼──────────┐
            │ core::io       │                               │ wavcraft-dsp      │
            │ (over audio-io)│                               │ fades, gain, norm, │
            └───────┬────────┘                               │ noise reduction,   │
                    │                                        │ loudness, spectral │
     ┌──────────────▼──────────────┐                         │ selection mixing   │
     │ soundcraft-audio-io (pinned)│                         └─────────┬──────────┘
     └─────────────────────────────┘                     ┌─────────────▼──────────────┐
                                                         │ soundcraft-dsp (pinned)    │
                                                         │ meter, spectrum, processors│
                                                         └────────────────────────────┘
```

## Crates

The repo today has one core library and one app (app.md: 21 tests in the core, 9 in the app). **Keep two crates until a third one is needed.** Split only when it makes something easier to build or test, not ahead of time.

| Crate | Holds | When |
|---|---|---|
| `wavcraft-core` (today's core library) | `store` (session folder, packs, block cache), `doc` (blocks, pieces, edits, history, markers, selection), `io` (streaming open, save, markers), `record` (take writer and recovery; no audio device), `view` (peaks and spectrogram caches, already here or in the app: task 02 checks), `jobs` (worker jobs). No window, no audio device. Everything here can be tested without a screen | Now |
| `wavcraft` (today's app) | The egui front end: panels, the action table that drives menus and shortcuts, tools, dialogs, audio output | Now |
| `wavcraft-dsp` | Our own DSP: fade curves, gain, normalize, the STFT framework, noise print and noise reduction, Match Loudness and the true-peak limiter, spectral-selection mixing. Pure functions over `&[f32]` buffers, no document types, so it can be tested and benchmarked alone | When Phase 3 starts (the first effect after Silence) |

Crate names: the core library may already have a different name. Keep whatever it is; don't rename for the plan's sake.

## Which SoundCraft crates we reuse

All are MIT/Apache, used as **git dependencies on our fork (`iannitram/soundcraft`) pinned to a tag**, as `audio-io` is today (`wavecraft-pin-2026-10-12` → `3527bb4`). The MP3 encoder is not a SoundCraft crate: it gets its own repo and its own pin. Every SoundCraft crate we use is pinned to the **same** tag, so they agree with each other. Moving the pin is a deliberate step with its own commit and a new `wavcraft-pin-YYYY-MM-DD` tag on the fork; never a floating branch.

| SoundCraft crate | Use it? | For what |
|---|---|---|
| `audio-io` | **Yes (already)** | Its encoder works on whole buffers in memory (task 02), so for long files it's too big: WAV and AIFF are written by our own streaming writers, quantised exactly as `audio-io` does, and FLAC goes through `audio-io` only below a size limit (task 06). Its decoder reads whole files, so it is used only where it can stream: opening is now streamed into the store, because a whole-file decode is too big for an 8 GB Mac. `peaks.rs` for waveform peaks if it suits our caches. MP3 comes from the shared encoder crate ([questions-and-answers.md](questions-and-answers.md) Q4), not from `audio-io` |
| `dsp` | **Yes, from Phase 3** | `meter.rs`: BS.1770 / EBU R128 loudness (momentary, short-term, gated integrated) and 4x-oversampled true peak, for Match Loudness, Amplitude Statistics and the Levels meter. The cloud routine measured it within 0.02 LU of an independent reference. `spectrum.rs` (FFT) only if it beats calling `rustfft` directly. The built-in processors (EQ, dynamics, reverb, pitch, utility) for the Effects Rack. `offline.rs`: read it for how SoundCraft runs a processor over a whole buffer; use it if its interface takes plain buffers |
| `time` | **Yes, when the Selection/View panel is built** | Timecode, decimal time and sample formatting, so our time display matches SoundCraft's |
| `playback` | **Evaluate in task 02, don't adopt by default** | Today's viewer already plays (Space, moving playhead). Keep its output unless `playback` gives something it lacks (device change, resampling to the device's rate). Its `record.rs` is read in task 10 for how SoundCraft opens an input. We use it only if it takes a plain callback and doesn't pull in `engine` |
| `clap-host`, `vst3-host` | **Only for v1 item F4**, and only if I use a plugin | Third-party plugins in the Effects Rack |
| `engine` | **No** | Its command system and copy-on-write undo are built for a multitrack session of clips. Ours is one destructive file; our own `doc` is smaller and fits it. (Decided 2026-10-12: "depend only on `audio-io`, `dsp` and `time`".) Read its strip-silence and AudioSuite code for ideas only |
| `ui-egui` | **Never** | SoundCraft's screen layer. WavCraft has its own front end. Its theme or widgets are not to be copied in either |

Licences to keep track of: `symphonia` (MPL-2.0, through `audio-io`) and our fonts (OFL) need a notices file before any binary leaves Hyperion (v1 item I6). Before adding any new dependency, check that it's MIT, Apache, BSD, zlib, MPL or OFL. Never GPL or LGPL in the tree.

## What we write ourselves

| Piece | Size | Notes |
|---|---|---|
| **The document model, the on-disk store and undo** (`core::store`, `core::doc`) | Medium | Below. Everything else stands on it, so it comes first (tasks 03 and 04) |
| **Streaming open** (WAV/AIFF ourselves, others through `symphonia`) | Small–medium | Below. Replaces the whole-file decode, so the 3 h 06 min limit goes |
| **Recording** (crash-safe take writer, audio input) | Medium | Below, "Recording". Tasks 09–11 |
| **MP3 encoder** | Large | A standalone crate in its own repo, shared with SoundCraft, built on the Z13 ([tasks/mp3-encoder-brief.md](tasks/mp3-encoder-brief.md)). WavCraft only calls it |
| **Noise print + Noise Reduction** | **Large: the biggest new DSP piece** | Below |
| Spectral selection (marquee, lasso, feather) and applying any effect inside one | Medium | Below |
| Match Loudness: gain to target plus a true-peak limiter | Small | `dsp`'s meter does the measuring. We write the gain calculation and a look-ahead limiter that watches 4x-oversampled peaks, unless `dsp`'s limiter already does that (check first) |
| Fade curves, Amplify, Normalize, Invert, Reverse, Silence | Small | Plain loops over blocks |
| Markers, including WAV cue/label chunks | Small | If `audio-io` doesn't read and write `cue ` and `LIST/adtl` chunks, we add that in `core::io` on our side, not on the fork |
| Autosave and crash recovery | Small (thanks to the store) | Phase 4. Below |
| The Audition layout, panels and shortcut table | Medium | egui panels, one action table |

## The document model, undo, and memory

*Revised 2026-10-15 after my answer to Q7: **Lani's Mac has 8 GB.** The first version of this section held all audio in RAM and left spill-to-disk for later. That doesn't fit 8 GB: three hours of stereo is about 4.1 GB of `f32` before any undo. Now **all sample data lives on disk, in the session's store, from the moment a file is opened. RAM holds only a bounded cache of blocks.** Tasks 03–05 were rewritten to match before any of this was coded.*

**The problem.** WavCraft edits destructively, as Audition's Waveform Editor does, and the files are long. An hour of stereo at 48 kHz is 1.38 GB of 32-bit float, and three hours is about 4.1 GB. Copying the whole file on every edit, keeping a copy per undo step, or even holding the file in RAM once, won't fit an 8 GB Mac.

**Memory budget (v1 requirement I7):** with a 3-hour stereo file open, edited and with 20 undo steps, WavCraft's resident memory stays **under 1.5 GB** on any machine, and opening the file never needs more than **300 MB** above what the app used before. Measured on Hyperion in tasks 03–05, checked on Shuttlecraft 6 (Mac mini M1) before Lani.

### The store: where samples live

- **Session folder:** `<cache>/wavcraft/sessions/<session-id>/`, where `<cache>` is the system cache directory (`%LOCALAPPDATA%`, `~/.cache`, `~/Library/Caches`). It's never inside the vault, and it's deleted when the file is closed and the session is clean.
- **Pack files:** samples are written as raw little-endian `f32`, one channel per block, appended to pack files (`pack-0001.f32`, …, rolling over at 1 GiB). A block is `(pack, byte offset, frames)`, written once and never changed. Packs are append-only. A thousand small files per hour would be slow on every OS, which is why it's packs and not one file per block.
- **Writing:** new blocks (from opening, effects, crossfades) are appended by the thread that makes them, through a buffered writer, so memory stays at about one block per channel in flight.
- **Reading:** a **block cache** in RAM: an LRU of whole blocks keyed by block id, with a byte budget. The default is 1/16 of physical memory, at least 256 MB and at most 1 GB (512 MB on an 8 GB Mac). A miss reads the block with a positioned read (`read_at` / `seek_read`). The OS page cache helps as well, and it gives memory back under pressure, which our own RAM never would. No `mmap` in v1: mapped files fault with undefined behaviour if they shrink underneath, and positioned reads are fast enough (measured in task 03).
- **The zero block** is virtual: it isn't on disk, and a read of it fills zeros.
- **Freeing disk:** each pack keeps a count of live blocks (blocks that any `DocState` in the history still uses). A pack with no live blocks, other than the one being written, is deleted. There's no compaction in v1. Disk use is shown beside the History panel.
- **Disk cap for the history** (it replaces the RAM cap of the first version): the history drops its oldest steps when the session folder's live bytes would go over **the smaller of 100 GB and half the free disk space at open**, and the History panel says so. Before a job that would write more than the free space minus 2 GB, WavCraft refuses it with a message, and nothing is half-applied.

### Opening a file: streamed into the store

Opening no longer decodes the whole file into RAM. That also removes the ~3 h 06 min limit and the "several times the file size in memory" peak from app.md's known limits.

- **WAV/BWF/RF64 and AIFF** (PCM and float): read the header ourselves (or with `audio-io` if task 02 finds it can stream), then read the data in 64 K-frame chunks, convert to `f32`, de-interleave, and append to the store.
- **MP3, FLAC and anything else:** decode packet by packet with `symphonia` (MPL-2.0, already a dependency through `audio-io`), straight into the store.
- It runs on a worker with progress and **can be cancelled**. As the store fills, the views can draw the part already decoded (nice to have, not required).
- **Why not read samples straight out of the original WAV instead of copying it?** Because Save writes a temp file and renames it over the original (task 06). If our blocks pointed into the original, Save would fail on Windows (the file is open) or cut the ground from under the document. Copying into the store costs a few seconds per hour of audio, and every block then behaves the same.

### The model: immutable blocks plus a list of pieces (a piece table)

- **Block:** a run of samples for **one channel**, up to 65,536 frames (256 KB of `f32`), never changed after it's made. In memory it's a small handle (`Arc<BlockRef>`: id, pack, offset, frames). The samples are on disk, and in the cache when they've been read recently.
- **Piece:** `(block, offset, len)`: part of one block.
- **Channel:** an ordered list of pieces, plus a running total of lengths, so "which piece holds frame N" is a binary search. Three hours at 48 kHz is about 7,900 pieces per channel when freshly opened, so a plain `Vec<Piece>` is enough. An insert or delete moves at most a few thousand small structs (microseconds). Use no tree structure unless a measurement says so.
- **Document state:** `Arc<DocState>` holding one `Arc<Channel>` per channel, the sample rate, the source format, the markers and the metadata. All channels are always the same length. A `DocState` with all its piece lists for a 3-hour stereo file is about 0.5 MB, so keeping hundreds of them in history costs little.
- **One read path:** `DocState::read(channel, start, &mut [f32]) -> io::Result<usize>` copies through the cache. Views, effects, save and playback's read-ahead all use it, and none of them sees blocks, pieces or packs.

**Edits never copy samples they don't change.**

| Edit | What happens | Cost |
|---|---|---|
| Delete / Cut | Split the pieces at both edges, drop the ones in between | O(pieces), no sample copy |
| Copy | The clipboard is a list of pieces (sharing blocks) | O(pieces) |
| Paste | Split at the cursor, splice the clipboard's pieces in | O(pieces) |
| Crop | Keep only the pieces inside | O(pieces) |
| Silence / Insert Silence | The virtual zero block, referenced as often as needed | Nothing on disk |
| An effect on a range (Amplify, Fade, Noise Reduction…) | Read the range in chunks, write **new** blocks for it to the store, splice them in place of the old pieces. The rest of the file still shares the old blocks | New disk = size of the range; RAM = a few chunks |
| One channel only (left-only Amplify) | Only that channel's piece list changes; the other channel keeps sharing | Half the disk of a both-channel edit |

**Edit crossfade at joins** (v1 item D7; Audition crossfades every join). When the crossfade length is above 0, Delete, Cut, Paste and Crop also write one small new block at each join: the crossfaded few milliseconds, built from audio on both sides that the pieces still reference. The extra cost is a few KB per edit, and undo is unchanged. At length 0 the splice is exact. Tests for the edit model run at 0 so they can compare bit for bit; the crossfade has its own tests.

*Correction to D7, 2026-10-16 (the Legion Go review, `verification-by-legiongo.md` W6):* the crossfade tests' click tolerance (a step at most 1.035 × the sine's own largest step at 2 ms, 1.07 × at 64 frames) holds for a full-scale sine at 1 kHz and above, which is what the tests use. It is not a general guarantee. At 220 Hz and 50 Hz the 2 ms fade's own slope is larger than the sine's step (Delete 1.43 × and 4.12 ×, Paste 1.10 × and 1.43 ×). That is a smooth slope of about 0.027 of full scale at 50 Hz, not a discontinuity, so not a click. The same note is added to the "Crossfade curve" line under "Corrections from task 04" below.

**Undo is keeping the old `Arc<DocState>`.** The history is a list of `(label, Arc<DocState>)` steps with a cursor. Undo and redo move the cursor and swap the current state, so undoing a cut takes the time of an `Arc` swap. Undo steps are "on disk" automatically, because the blocks they refer to already are. Only their small piece lists sit in RAM. Effects and edits run against a snapshot, so the current state never half-changes.

**Playback reads ahead from a snapshot.** The audio callback must never touch the disk, the block cache or an `Arc` it could free. A **read-ahead thread** holds the current `Arc<DocState>` (swapped in lock-free, for example with `arc-swap`, MIT/Apache), reads through `DocState::read`, and fills a lock-free ring buffer (`rtrb` or similar, MIT/Apache) about 0.5 s ahead. The callback only copies from the ring, and if it runs dry it outputs silence and counts an underrun. When the state changes during playback, the read-ahead thread flushes the ring and refills from the callback's current position from the new state. The short gap that leaves is measured in task 05. (Today's player uses a `Mutex<Option<Arc<…>>>` with `try_lock` in the callback, per task 02. That pattern is fine for handing the ring's read side to the callback. What changes is that the callback reads from the ring and not from the samples, because the samples are now on disk.)

**Long jobs.** An effect on a long range runs on a worker thread (`core::jobs`) with a snapshot of the state. It reports progress, can be cancelled (the blocks it wrote become dead and their space is freed with their pack), and returns the new state. The UI applies it as one history step. While a job runs, edits are disabled; viewing and playing still work.

**Autosave and crash recovery come almost free (Phase 4, v1 item I4).** All samples are already on disk. After each history push, write the current `DocState`'s piece lists and markers as a small file (`state.json`, written to a temp name and renamed) in the session folder, and fsync the packs at the same moment. On start, a session folder that wasn't closed cleanly is offered for recovery. Recording uses its own simpler crash path, because a take is a plain WAV (section "Recording").

**Memory used by the views** stays in RAM and is small: the spectrogram overview is 10.8 MB per channel-hour (65 MB for 3 hours of stereo), and peaks are a few MB. Both count against the 1.5 GB budget.

## How the views fit: peaks and the spectrogram overview

Phase 1 already has a waveform, a worker-thread spectrogram, and a **spectrogram overview**: for each channel, the loudest level in each frequency bin for every 16 FFTs of the zoomed-out grid (about a third of a second), one byte per bin. That is 10.8 MB per channel-hour at 48 kHz and takes about 165 ms to build for 3 hours. The grid is fixed to the file, so scrolling doesn't change the picture.

After an edit, "the file" is the document, so the grid is fixed to **document time**. What happens to the caches:

| Edit | Peaks and overview |
|---|---|
| Same length (Silence, Amplify, Fade, Noise Reduction, any effect) | Rebuild only the overview blocks that cover the changed range, plus one block on each side because the FFT window reaches across a block edge. Peaks: the same, by range |
| Length change at frame F (Delete, Cut, Paste, Crop, Insert Silence) | Everything before F's block stays. Everything from F's block to the end is rebuilt on the worker. Worst case is the whole file: about 165 ms per channel for 3 hours (measured 2026-10-12), so this simple rule is fast enough and shifting the cache cleverly isn't worth the code |
| Undo / redo | Keep the caches **per `DocState`** in a small cache (current state plus the last 2) so undoing straight after an edit shows at once. Otherwise rebuild as above |

While a rebuild runs, the old picture stays up in its place (the viewer already does this). Changed regions are drawn from their new samples as soon as they're ready.

**Reading samples for views and effects** goes through one function, `DocState::read(channel, range, &mut [f32])`, which copies from the pieces. The views never see blocks or pieces. That keeps the edit model free to change.

## Noise Reduction: the design

Clean room: this comes from textbook speech-enhancement methods, not from any editor's code. Spectral subtraction (Boll, 1979) and the decision-directed Wiener / MMSE estimators (Ephraim and Malah, 1984) are decades old and published; their patents, if any, are long expired. Audition's names and controls are copied from its published docs ([audition-behaviour.md](audition-behaviour.md)); what happens inside is our own choice.

1. **STFT:** Hann window, FFT size N (Audition's FFT Size control; default 4096 at 44.1/48 kHz), hop N/4. `rustfft` (MIT/Apache) through `dsp`'s `spectrum.rs` or directly. Weighted overlap-add for resynthesis, with a test that passing audio through at unity gain comes back within −120 dB of the input.
2. **Noise print** (Capture Noise Print, Shift+P): for each channel and each bin, the mean power and its spread across all frames of the selected room tone. It is saved with the FFT size and sample rate it was made at. If a different FFT size is chosen later, the print is re-binned. It can be saved to and loaded from a file.
3. **Gain per bin and frame:** a Wiener-style gain from the a-priori signal-to-noise ratio, estimated decision-directed (this smooths the gain over time and is the standard defence against "musical noise"). The gain floor is the **Reduce By** amount. **Noise Reduction %** blends from unity gain (0 %) to the full gain (100 %).
4. **How Audition's advanced controls map onto ours** (our mapping, to be tuned by ear):
   - *Precision Factor* → how many standard deviations above the mean noise power a bin must be before it counts as signal.
   - *Transition Width* → a soft knee in dB between "noise" and "signal", so the gain changes gradually.
   - *Spectral Decay Rate* → how fast the gain is allowed to fall after the signal drops (release smoothing), which keeps reverb tails natural.
   - *Smoothing* → smoothing of the gain across neighbouring bins and frames, the second defence against musical noise.
   - *Noise Print Snapshots* → how many frames of the print are kept for the statistics (frames are sampled evenly if the selection has more).
   - Starting values for all of these are in [audition-behaviour.md](audition-behaviour.md) section 8. Adobe documents ranges, not defaults.
5. **Output Noise Only:** output the input minus the result, so you can hear what is being taken out.
6. **Speed:** frames are independent once the gain smoothing is computed, so the file is split into chunks with overlap and processed across all cores with `rayon` (MIT/Apache). One hour of stereo at N = 4096 is about 340,000 FFTs each way. The target (v1 item G2) is under 60 s on Hyperion's CPU. The Z13 is Track B's job.
7. **Tests:** a test kit of clean speech mixed with recorded room noise at 10 dB and 20 dB SNR, where the clean speech is known. Measured: the noise floor in gaps, speech level change, and segmental SNR before and after. The decision belongs to Lani's ears and mine; the numbers catch regressions.

## Spectral selection: applying an effect inside a time and frequency area

1. Run the effect over the selection's **time range**, across all frequencies, giving a processed copy.
2. Take the STFT of the original and the processed copy over that range (plus a window's length either side).
3. Mix them per bin with a mask: 1 inside the marquee or lasso, 0 outside, with a soft edge (the feather) in both time and frequency.
4. Resynthesise and splice the result in as new blocks.

This works for every effect, the built-in ones and plugins alike, with no per-effect code. The cost is two extra STFTs over the range.

## The app: action table, panels, tools

- **One action table:** each command (`edit.cut`, `effects.amplify`, `view.toggle_spectral`, `markers.add`…) has a name, a default shortcut (Audition's), an enabled check, and a function. Menus, shortcuts, the shortcuts list under Help and the tests all read the same table. A shortcut can never do something its menu item doesn't.
- **Tools:** Time Selection (T), Marquee (E), Lasso (D), plus the Move tool (V) later for Multitrack. Only one is active at a time, shown in a toolbar.
- **Panels:** Files, Markers, Editor, Effects Rack, History, Selection/View, Levels, Transport (layout in [app.md](../app.md)). egui's own panels to start with. A docking crate (for example `egui_dock`, MIT) only if I want panels dragged about. Audition's default layout with show, hide and resize covers v1.
- **Settings and presets** in the system config directory (`%APPDATA%\WavCraft`, `~/.config/wavcraft`, `~/Library/Application Support/WavCraft`) as TOML/JSON. Never inside the vault.

## Recording (v1 section J, tasks 09–11)

*Added 2026-10-15: I put recording in v1.*

**What I asked for:** one USB mic at a time. Each take goes into a new file: 48 kHz, 24-bit WAV, with as many channels as the mic sends. Monitoring is through the mic's own headphone jack, so **WavCraft plays nothing back while recording.** A crash mid-take keeps everything up to the crash. It has to work on Hyperion (Windows), the Z13 and the Legion Go (SteamOS, PipeWire) and Lani's Mac.

**A take is written straight to its WAV file, not into the store.** It's one plain file that grows. That gives the simplest crash story, and the file is useful even if WavCraft never starts again. When the take stops, it is opened like any other file (streamed into the store).

```
 audio input callback ──► lock-free ring (10 s) ──► take-writer thread ──► Take 2026-10-15 141502.wav
   (cpal; never blocks,        overflow → counted,       24-bit LE PCM, header           + .wavcraft-take journal
    never allocates)           shown, never blocks       rewritten every second
```

- **Input:** `cpal` (Apache-2.0), the same crate family Phase 1's output uses. Ask for 48 kHz; if the device can't do 48 kHz, record at its own rate and say so on screen, with no resampling while recording. Take the samples in whatever format the device gives (i16, i24/i32, f32) and convert to 24-bit integer: i16 pads exactly, f32 is scaled by 2²³, rounded and clamped. The input device is chosen in Preferences > Audio Hardware and remembered by name.
- **Take writer** (`core::record`, no audio device, so it can be tested alone):
  1. Create `<folder>/Take <date> <time>.wav` and, beside it, a journal `<same name>.wavcraft-take` (a few lines: format, start time, app version).
  2. Write a WAV header with a `JUNK` chunk the size of an RF64 `ds64` chunk right after `RIFF/WAVE` (the standard trick that lets a file become RF64 in place), then `fmt `, then `data`.
  3. Append audio as it arrives. **About once a second, rewrite the `RIFF` and `data` sizes in the header to match what's written, then flush.** At most every 5 s, `fsync`. A killed process (not a power cut) loses nothing already written, because the OS keeps it. A power cut loses at most the last 5 s.
  4. When the data passes 4 GB minus a margin, turn the header into RF64 in place (`RIFF`→`RF64`, `JUNK`→`ds64`). 3 hours of stereo 24-bit is 3.1 GB, so this is rare but must work.
  5. On Stop: final sizes, `fsync`, close, delete the journal.
- **Recovery at the next start:** for any journal whose WAV is still there, fix the header from the real file length (rounded down to whole frames), delete the journal, and tell the person: "Recovered a take from <time>: <length>". Offer to open it. If the WAV is empty, remove both.
- **Where takes go:** a folder chosen in Preferences, default `Music/WavCraft Recordings` in the user's home. Never inside the vault. Before recording, show the free space as time ("31 h left at this format"); stop cleanly with a message if under 1 GB remains.
- **On screen while recording:** the input level meter (peak and clip light), elapsed time, file size, dropped-sample count (always 0 when healthy), and the take's waveform drawn live from the ring (nice to have).
- **Unplugging the mic mid-take** stops the take cleanly. The file is kept and a message is shown.
- **macOS:** the app bundle needs a microphone usage description (`NSMicrophoneUsageDescription`), and the first recording triggers the system's permission prompt. Without permission, cpal delivers silence or errors: detect that and say so.
- **SteamOS:** `cpal` goes through ALSA to PipeWire. Desktop Mode is the target. Game Mode is not tested for v1.

## Multitrack later

Multitrack is a later phase of WavCraft. It will have its own session model with clips referring to files, each clip a list of pieces over the same immutable blocks, so the Waveform Editor and Multitrack share one storage layer. SoundCraft's `engine` and `ui-egui` remain off limits as code. Its mixer is a reference for ideas only. Nothing in v1 should assume there is only ever one document. Already in v1 the Files panel holds several, which keeps that honest.

## Where things run and get built

- Code: `B:\dev\craft\wavcraft` on Hyperion, `~/dev/craft/wavcraft` on the Z13 and Macs. Never in the vault.
- On Hyperion every cargo command goes through the build gate (`Workspaces/CLAUDE.md` part 1); running the app uses `--slot gpu-run`. Today the GPU belongs to the video upscaler, so tests that need no window are worth more than ones that do.
- The Mac build is tested on Shuttlecraft 6 (Mac mini M1, joining the fleet later) before Lani's 8 GB Mac.
- Test audio that's synthetic is generated by code in the repo (task 02). Real episode audio is never committed: the repo is private today but may not stay so.

## Corrections from task 02 (2026-10-15, Hyperion)

*Read from the tree at `3cdf38f` and the pinned SoundCraft checkout (`3527bb4`). Full map: the repo's `docs/code-map.md`. Baseline numbers: the repo's `docs/baseline.md` (3-hour stereo 16-bit WAV, warm cache: open 1.55 s, peak memory 6.58 GB, 4.50 GB held after open, overview 120 ms per channel, whole-file picture at 1280 columns 5.8 ms per channel, full ordered read 0.60 s).*

**Held:**

- Two crates, `wavcraft-core` (`crates/core`) and `wavcraft` (`apps/wavcraft`). Names kept.
- `soundcraft-audio-io` is the only SoundCraft dependency, pinned by `rev = 3527bb4…` (the `wavecraft-pin-2026-10-12` commit). `soundcraft-dsp` and `soundcraft-time` sit in the same fork at the same commit, so they can be added with the same `rev` (their own deps: `serde`, `log`, `rustfft`; `serde`, `thiserror`).
- Opening decodes whole-channel buffers: `Document.audio: Arc<AudioBuffer>`, one `Vec<f32>` per channel. Chopping into blocks at open is as planned.
- The spectrogram overview is as app.md says (16 FFT hops per block, one byte per bin). The overview does not depend on the frequency axis or picture size.
- `playback` is not worth adopting: its `Player::new` takes a SoundCraft `Session` (engine/mix model), so it cannot play a plain buffer. Keep our own cpal player.
- Markers: `audio-io` has **no** `cue `, `LIST/adtl`, `labl`, `smpl` or AIFF `MARK` support, read or write. The WAV reader skips unknown chunks, so marked files open and lose their markers. Task 08 adds it in our own code, as planned.

**Did not hold, and what it changes:**

1. **No single read function.** Each consumer indexes the channel `Vec` itself: `Document::waveform_columns` (UI), the spectrogram worker (passes `&audio.channels[ch]`), and the player callback. Worse for task 05: **`spectrogram::render_columns`, `Overview::build` and audio-io's `Peaks::build` all take the whole channel as one contiguous `&[f32]`** (`Analyser::add` slices `samples[at..at+n]`). A piece table cannot hand them that without materialising the channel (4.1 GB for 3 h stereo). Task 05 must change `Analyser::add` to fill its window through `read(channel, range, &mut buf)` (an FFT window is at most 16,384 frames, so a copy per FFT is cheap) and build `Peaks` per block. `Peaks` levels are 32 × 4^k samples, so a 65,536-frame block aligns with levels 0–5 (32,768 = 32 × 4^5): per-block peaks can be built once per block and merged above that, and a new block (an effect's output) only needs its own peaks.
2. **The spectrogram worker and the overview cache live in the app** (`apps/wavcraft/src/spectrum.rs`), not in the core. Only the maths is in `core::spectrogram`. So "caches per `DocState`" and "rebuild only the changed range" (task 05) means changing the app's `Overviews` struct; its key today is `(generation, fft_size, floor_db, ceil_db)` and any new file rebuilds everything. Suggest moving the worker's cache logic (not the egui colouring) into `core` in task 05 so it can be tested without a window. The bench (task 02) calls the core functions in the worker's order, since it cannot link the app binary.
3. **Overview checks length.** `Overview::serves` compares the channel's length; after a length-changing edit an old overview is silently ignored (whole-file FFTs, ~140 ms/channel for 3 h) rather than wrong. Task 05's partial rebuild has to replace this check.
4. **Playback uses a `Mutex<Option<Arc<AudioBuffer>>>` with `try_lock` in the callback**, not a lock-free swap. The callback only borrows, so the audio thread never drops a buffer. This pattern already meets the "never free on the audio thread" rule for `Arc<DocState>` too: the UI replaces the state under the lock and the old one drops on the UI thread. `arc-swap` is optional, not required. Position is `pos`/`end` in file frames (`f64` bits in atomics): after an edit the UI must map or reset them.
5. **Saving (task 06): `audio-io` encodes only whole buffers to bytes in memory** (`encode(&AudioBuffer, &EncodeOptions) -> Vec<u8>`, no file access, no streaming). Saving a 3-hour stereo document that way needs the document flattened into one `AudioBuffer` (4.1 GB f32) plus the output (2 GB at 16-bit) on top of the open document: over 10 GB. Recommendation for task 06: write WAV with our own streaming writer from blocks (header first, then a block at a time; RF64 above 4 GiB; the test kit's `WavWriter` is a start), quantising exactly as audio-io does (`round(x·2^(bits-1))`, clamped, optional TPDF dither) so files round-trip bit for bit. AIFF and FLAC can go through `encode` for v1 with a documented memory cost, or be streamed later. FLAC cannot store float (`Unsupported`); AIFF float is AIFF-C `fl32`; AIFF is limited to 4 GiB.
6. **Open's memory peak** is the whole file's bytes (`std::fs::read`) plus the decoded `f32` at once, then peaks: measured 6.58 GB for the 3-hour stereo 16-bit file (2.07 GB bytes + 4.15 GB samples + about 0.35 GB peaks). The decode limit is 2^30 samples total (`MAX_DECODE_SAMPLES`): 3 h 06 min of 48 kHz stereo.
7. **`dsp` for later phases:** `meter::LoudnessMeter` (BS.1770 momentary/short-term/integrated, 4× true peak) takes planar `&[Vec<f32>]` chunks, so it can be fed from blocks. `offline` (gain, normalize, fade with five shapes, invert, reverse, resample) works on a whole `&mut [Vec<f32>]`: fine for a range copied out of blocks. The `maximizer` plugin is a sample-peak brickwall (1.5 ms lookahead), **not** a true-peak limiter, so Match Loudness still needs our own true-peak limiter. `spectrum::magnitude_db` plans an FFT per call; keep calling `rustfft` directly. `time` offers `TimeFormat` (MinSecs, Samples, Timecode, …) and `format_position`, which needs a `TempoMap` and `FrameRate` even for min:sec.
8. The `tones-10min` generator did not exist in the repo; the test kit writes its own (four tones, 50 ms to 20 s, listed in a sidecar).
9. The repo has no `CLAUDE.md`; `AGENTS.md` points to `README.md`. The workspace forbids `unsafe`; the bench's one Windows memory call is the only exception (its crate denies rather than forbids).

## Revision after task 02 and my answers (2026-10-15, design session)

- **Lani's Mac has 8 GB** (my answer). The document model section was rewritten: samples live in an on-disk store of append-only packs; RAM holds a block cache with a budget; undo costs disk, not RAM; opening streams into the store (our own WAV/AIFF reader, `symphonia` for the rest), which also removes the 3 h 06 min limit. Budget: under 1.5 GB resident with 3 hours open and edited (v1 item I7).
- Task 02's items 1–3 (every consumer slices whole channels; the overview cache lives in the app; `Overview::serves` checks length) are now part of task 05: `Analyser::add` reads through `DocState::read`, peaks are built per block and cached by block id, and the overview cache logic moves into the core.
- Item 4 (the player's `Mutex` + `try_lock`): kept as the hand-off pattern, but the callback now reads from a ring filled by a read-ahead thread, because the samples are on disk.
- Item 5 (`audio-io` encodes only whole buffers): task 06 writes WAV and AIFF with our own streaming writers, and FLAC through `audio-io` only under a size limit.
- Item 7 (`maximizer` is a sample-peak limiter): confirmed. Match Loudness gets our own true-peak limiter (Phase 3).
- **Recording is in v1** (section "Recording", tasks 09–11). **MP3 export is required** and comes from a shared encoder crate built on the Z13 ([tasks/mp3-encoder-brief.md](tasks/mp3-encoder-brief.md)). Third-party plugins are out of v1.


## Corrections from task 03 (2026-10-15, Hyperion)

*The store, the document model and streamed open as built (`main` at `73b09a2`). Most of "The document model, undo, and memory" held as written. What differs:*

- **Block handles keep the store alive.** `Store::append` returns `Arc<BlockHandle>`, and each handle holds a clone of the `Store`, which is itself a cheap `Arc` handle (`DocState` holds a `Store`, not an `Arc<Store>`). A pack's live count is the number of `BlockHandle`s in it. A handle is shared by every piece of its block, so many pieces of one block count once. The session folder goes when the last `Store` clone and the last handle drop. A sealed pack is deleted as soon as its count reaches 0. The pack being written is deleted when it is sealed, if its count is 0 by then.
- **The cache holds `Arc<Vec<f32>>`** (saves a copy per miss) and reuses up to 4 evicted buffers (1 MiB). Measured: random 4,096-frame reads over the 3-hour file with a 512 MB cache have a median of about 51 µs. The target was 50 µs. A miss copies a whole 256 KB block out of the OS cache. Sequential reads run at 2.9 to 3.2 GB/s.
- **The zero block is `u32::MAX` frames long**, so a zero piece can cover up to 24 hours at 48 kHz.
- **Session folder:** `<cache>/wavcraft/sessions/<16 hex>/` (the code's name is now WavCraft), with `session.lock` (pid and app version). `keep` leaves the lock file as well, so a kept folder looks the way a crash leaves one.
- **Open** reads WAV, RF64 and AIFF with our own chunk reader. It follows audio-io's rules exactly (the same lenient truncation, data size 0, `ds64`, the same integer-to-float arithmetic), and a test compares every sample of every layout with audio-io. Everything else goes through symphonia (MPL-2.0, now a direct dependency at audio-io's version and features). Peak memory while opening the 3-hour stereo file: 5.3 MB above rest. ~~Open time: 1.75 to 1.9 s.~~ *(2026-10-16, W2: that is Hyperion's figure with the file in the OS cache. The Legion Go took 3.0 to 4.1 s for a 3-hour 24-bit stereo file on ext4. The target stays under 10 s.)* `SourceFormat` keeps every WAV/AIFF chunk with its offset and size (`skipped_chunks()` for task 06, `marker_chunks()` for task 08), plus warnings for damaged files.
- **Channel masks (for task 04):** every channel is its own `Arc<Channel>`, and `DocState::from_shared` builds a state from channels shared with another. A channel-masked edit that keeps the length, such as Silence or the new one-channel Delete, rebuilds only the masked channels' piece lists and writes nothing to disk.
- **Until task 05**, `Document::open` streams into the store and then reads it all back into the old `AudioBuffer` (`DocState::to_audio_buffer`). The app therefore still holds the whole file in RAM, plus its f32 copy in the store on disk.
- **New dependencies:** `bytemuck` (Zlib/Apache/MIT), `dirs` (MIT/Apache; brings in `option-ext`, MPL-2.0), `sysinfo` (MIT, for physical memory), `proptest` (dev). The notices file (I6) now also needs `option-ext`.
- **Noise Reduction starting values:** the ones in "Noise Reduction: the design" above (via audition-behaviour.md section 8) are superseded by [v1.md](v1.md) item G2, after the 2026-10-15 Audition check. This note doesn't rewrite them. Read G2.

### Decision: store 16- and 24-bit audio at its own bit depth (design session, 2026-10-15)

*The question came from task 03's Result, question 2. The store keeps `f32`, so a 16-bit source takes twice its size on the session disk: a 3-hour stereo episode is a 2.1 GB file and a 4.1 GB store. On Lani's 8 GB Mac, which probably has a small system disk, that is 4+ GB per open file before any undo.*

**Decision: each block records its sample encoding, `F32`, `I16` or `I24` (3 bytes, packed little-endian).** Opening a 16-bit source writes `I16` blocks, and a 24-bit source (including 20-bit in a 24-bit container) writes `I24` blocks. FLAC does the same when symphonia hands back integer samples. Everything else is `F32` as now: 8-bit, 32-bit int, float sources, MP3 and other lossy formats. **New audio is always `F32`**: effect output, crossfade blocks, the stereo-to-mono paste mix and a recovered autosave. It can hold values above full scale and between integer steps, which Audition's 32-bit float editing also keeps.

**What it costs:**

| | Effect |
|---|---|
| Disk at open | 16-bit: **half** (3 h stereo: 2.1 GB, not 4.1 GB). 24-bit: three quarters (3.1 GB). Opening also writes less, so it gets a little faster |
| Correctness | Lossless: every 16- and 24-bit integer is exact in `f32`, and the conversion stays audio-io's arithmetic (task 03's test 8 keeps pinning it). Nothing a user can hear or measure changes |
| Reads | A cache miss reads fewer bytes and converts to `f32` into the cache buffer. **The cache still holds `f32`**, so its budget, hits and the read API are unchanged. Conversion runs at several GB/s, below the cost of the read it saves, and random-read misses should get faster (measure) |
| Edits that move audio (Delete, Cut, Paste, Crop, Silence, one-channel Delete, undo) | **None.** They rearrange pieces. The integer blocks are shared, never converted, never rewritten |
| Edits that make audio (any effect, crossfades) | The **edited range** becomes `F32` blocks, exactly as before. An unedited block stays an integer block. A whole-file effect on a 3-hour 16-bit file therefore adds 4.1 GB, as it does today. That is the price of float headroom, and the disk cap and space check (task 04) cover it |
| Mixed packs | One pack holds blocks of any encoding. A `BlockRef` already carries its offset; it gains the encoding, and its byte length follows from frames × width |
| Save (task 06) | No change: it reads `f32` through `DocState::read`. Saving untouched 16-bit audio back to 16-bit stays bit-exact |
| Recording (tasks 09–11) | A finished 24-bit take opens as `I24` blocks, so a take costs 3/4 of `f32` |

**Not done, considered:** pointing blocks straight at the source file (no copy at all). Rejected again: Save renames over the original, and on Windows that fails while the file is open. A copy also protects the document if the source changes on disk. Storing effect output as integers was rejected too, because it would clip and requantise every edit.

**Also on disk space** (task 03's question 2): Hyperion's next small item gives the session folder a location setting and a free-space check at open, so Lani can put sessions on whichever disk has room. That and this decision together are the fix. With both, a 3-hour 16-bit episode needs about 2.1 GB free plus whatever its edits make.

**Random reads** (task 03's question 1): accepted at a median of about 51 µs. Real reads are sequential and hit the cache. The integer blocks above should bring misses down on their own.

**Work:** [tasks/04b-store-at-source-depth.md](tasks/04b-store-at-source-depth.md). It's small, it doesn't block task 04 (the edits never see encodings), and it goes **after task 04 and before task 05**.


## Corrections from task 04 (2026-10-15, Hyperion)

*Edits and history as built (`main` at `0f86766`). "The document model, undo, and memory" held. What differs:*

- **Edits return `Edit { state, label, change }`.** `label` is the History name. Delete's label depends on its channel mask ("Delete" or "Delete Audio"). `change` is a `Change` (`None`, `SameLength { range, channels }`, `LengthFrom { frame }`), which task 05's caches can use directly, and each history `Step` keeps it. `History::push(label, DocState)` also exists; it records `LengthFrom { 0 }`.
- **Crossfade curve:** a raised cosine whose gains sum to 1, not equal-power. Equal-power lifts the same audio on both sides by 3 dB mid-fade, so it clicks at effect edges. Worst step at any phase of a 1 kHz sine, as a multiple of the sine's own: 1.031 at 2 ms and 1.005 at 5 ms (equal-power 1.41 to 1.43; a hard splice up to 8.6). The strict "no step larger than the sine's own" can't be met by any crossfade between out-of-phase audio, so tests 8 and 9d allow 7 % (64 frames) and 3.5 % (2 and 5 ms). *(2026-10-16, W6: those allowances hold for a full-scale sine at 1 kHz and above only; at 220 Hz and 50 Hz the 2 ms fade's own slope exceeds it, which is not a click. See the correction under D7 above.)*
- **Where the fades sit:** the delete/cut join is centred on the join. Paste, Silence, Insert Silence, one-channel Delete and `replace_range` fade **inside** the new material, because it has no audio past its own ends. Fades shorten where audio runs out, and two fades share a short segment half and half.
- **Paste:** a multi-channel clip into a mono file is the mean of its channels. A clip whose blocks live in another store (another open file) is copied into this one, so every block of a `DocState` lives in its own store.
- **History disk cap:** per-block counts of the steps that use each block, plus a running total. The bytes held beyond the current state are that total minus the current state's own bytes, so a push walks only the new state's pieces. Measured: about 1 ms with 200 steps behind. Dropping a step that holds GBs of blocks (a whole-file effect's redo step) deletes its packs on the calling thread, which takes 0.3 to 0.45 s; hand dropped steps to a worker when the UI pushes (task 05/07).
- **`replace_range` producer:** `FnMut(start, &[&[f32]], &mut [&mut [f32]]) -> io::Result<()>`, every masked channel at once. The function also takes a label, crossfades and a cancel flag. Whole 3 hours of stereo: 3.9 to 6.5 s, peak working set 0.555 GB with a 512 MB cache.
