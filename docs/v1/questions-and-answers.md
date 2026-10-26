# WavCraft: design questions and my answers

*Written 2026-10-15 by the WavCraft design session. **All eight answered on 2026-10-15** (a grilling session with the Coordinator, about 09:20, confirmed "Yes, go"). The answers are under each question. Each question has a **default**, and work goes ahead on the default until I answer, so nothing waits on me. When I answer, write it in under the question with the date and my words, and change the plan files it affects.*

## Q1. Walk me through a real cleanup, in Audition terms

Which panels, which effects, in what order, from the raw recording to the file you upload? For example: do you capture a noise print and run Noise Reduction on every episode? Do you EQ, compress or limit? Do you cut first or clean first? Where does Match Loudness come in?

**Default:** the order the plan assumes: (1) cut the rough edit, (2) capture a noise print from room tone, (3) Noise Reduction on the whole file, (4) high-pass and EQ, (5) a compressor, (6) Match Loudness, (7) save WAV, then MP3. This sets the order of Phase 3 ([v1.md](v1.md), "Order of the work"). It changes nothing in tasks 01–08.

**Answer (2026-10-15, via the Coordinator):** as the default. v1's purpose is confirmed: cleaning podcast episodes, one file at a time, done when Lani and I prefer it by ear. An episode is one recorded file, so v1 has no multitrack.

## Q2. What loudness do you aim for, and are episodes mono or stereo?

**Default:** Apple Podcasts' figure, **−16 LUFS integrated, true peak no higher than −1 dBTP**, for stereo. For a mono file, −19 LUFS (the usual rule of thumb that it plays as loud as −16 stereo). The other presets are AES speech (−18), EBU R128 (−23) and Custom. Affects v1 item G3.

**Answer (2026-10-15):** −16 LUFS integrated / −1 dBTP, stereo.

## Q3. Do you rely on any third-party plugins (iZotope RX, Waves, FabFilter…)? Which ones, on which machine?

**Default:** none. Then CLAP/VST3 hosting (v1 item F4) drops out of v1, and the Effects Rack uses SoundCraft's built-in processors only. If you name one, it becomes the last item of v1 and is tested with that plugin.

**Answer (2026-10-15):** no third-party plugins. F4 (CLAP/VST3 in the rack) is out of v1.

## Q4. MP3 export: how should WavCraft make MP3s until our own encoder works?

No usable MIT/Apache MP3 encoder exists in Rust. Ours is being written on the SoundCraft fork (`feat/mp3-export`, a step per night) and doesn't make sound yet. The choices for now:

- **(a) Wait for our encoder.** Clean, but no MP3 from WavCraft for a while.
- **(b) Call an encoder you install yourself** (ffmpeg or LAME) as a separate program. None of its code enters our tree, so the licence wall holds. It means installing a tool on each machine, which needs your OK.
- **(c) Save WAV from WavCraft and make the MP3 with whatever you use now.**

**Default: (c) now, (a) when the encoder is ready.** (b) only if you say so. Affects v1 item A3.

**Answer (2026-10-15):** **(a) wait for our own encoder, and v1 is not done without MP3.** A3 is a v1 requirement. The encoder becomes active work: a standalone pure-Rust MIT/Apache crate that WavCraft and SoundCraft both use, built on the Z13 ([tasks/mp3-encoder-brief.md](tasks/mp3-encoder-brief.md)).

## Q5. The name: WavCraft, or something else ("CraftRecording" came up)?

**Default:** WavCraft. Renaming later costs a repo rename and a find-and-replace, so it isn't urgent.

**Answer (2026-10-15):** WavCraft.

## Q6. Do you still have Audition on any machine?

If so, about 15 minutes in it would settle the things Adobe's pages don't: a handful of default keys (the tool letters, Shift+D, S, Ctrl+T), Noise Reduction's default settings, the default edit crossfade (Preferences > Data), and whether Audition reads back markers WavCraft writes. The list is at the end of [audition-behaviour.md](audition-behaviour.md).

**Default:** no. WavCraft uses the widely cited keys, our own Noise Reduction starting values, and a **5 ms** edit crossfade, all changeable in one place.

**Answer (2026-10-15):** **Audition is installed on Hyperion.** Hyperion's session opens it and looks, changing nothing ([tasks/audition-check.md](tasks/audition-check.md)).

## Q7. Will Lani use WavCraft on her Mac, and how much memory does it have?

This sets the undo-history memory cap (default 25 % of memory, at most 8 GB) and whether "spill old undo steps to disk" has to be in v1.

**Default:** she'll try it once before v1 is called done (v1 item I5), and her Mac has at least 16 GB, so spill-to-disk waits until after v1.

**Answer (2026-10-15):** **Lani's Mac has 8 GB of memory (Apple Silicon).** So v1 reads long files from disk as it needs them (no whole-file load) and keeps undo steps on disk. [architecture.md](architecture.md) ("Memory") and tasks 03–05 were revised to match. Shuttlecraft 6 (a Mac mini M1 joining the fleet later) tests the Mac build before Lani does.

## Q8. Do you approve the v1 list?

[v1.md](v1.md): what's in, what "done" means for each item, and what's out (Multitrack, spot healing, the other restoration effects, recording, batch). This is Phase 0's "Done".

**Default:** coding starts on Phase 2 (tasks 01–08: editing, undo, save, markers), which v1 needs whatever the answers above are. Phase 3's scope waits for your OK.

**Answer (2026-10-15):** **v1 approved, with the changes above and one addition: recording is in v1** (one USB mic, each take a new 48 kHz 24-bit WAV, monitoring through the mic's own headphone jack, crash-safe; on Hyperion, the Z13, the Legion Go and Lani's Mac). It's built right after the editor (tasks 09–11) and before repair. See [v1.md](v1.md) section J.

## Follow-up answers (2026-10-15, via the Coordinator)

- **MP3 encoder repo:** `iannitram/craft-mp3`, **public**, so the public SoundCraft fork and GitHub Actions release builds can fetch it without a login. See [tasks/mp3-encoder-brief.md](tasks/mp3-encoder-brief.md).
- **`minimp3.h` (CC0) download for the Huffman tables:** approved 2026-10-15.
- **USB mic on Hyperion:** there is none. "We'll have to do our tests on the z13 or Lani's machine when we can schedule them." Task 10 (Hyperion) now tests recording without a device, and every by-hand mic test moves to task 11: the Z13 first, then Lani's Mac when scheduled.

## Q9. One-channel Paste near the end of a file (2026-10-16, WavCraft worker, run 2026-10-16-wavcraft-audition)

When only one channel of a stereo file is active, Audition pastes by writing over that channel and keeps the file the same length (your check of 2026-10-16). It was only seen with the cursor in the middle of the file. WavCraft now does this too, and had to choose what happens in two cases nobody has looked at in Audition yet:

1. **The clip is longer than what is left of the file** (cursor near the end): WavCraft writes as much as fits and drops the rest, so the file never gets longer.
2. **A range is selected, not just a cursor:** WavCraft writes the whole clip starting at the selection's start, whatever the selection's length.

**Default:** keep both choices. They are written down in the code map under "Known differences from Audition". If you'd like, the next time Audition is open we can try both cases (2 minutes, on test copies) and match it exactly.
