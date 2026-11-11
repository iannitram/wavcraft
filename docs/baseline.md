# Baseline numbers

*Measured 2026-10-15 on the Windows dev machine (Windows 11, 24 rayon threads, 32 GB), release build, by task 02
(`plan/tasks/02-code-map-test-kit-baseline.md` in the vault). Code: commit `4176652` (the binaries
were built from `50e8dfe`; `4176652` only reformats the source). Tool: `wavcraft-bench FILE`
(`crates/testkit`), run through the build gate in the default slot. Files from
`wavcraft-testkit OUT --long` (seed 1463899717), in `<dev root>/craft/wavcraft-testkit-out/`.*

**What else was running.** Gate status just before the 3-hour run: 11.0 GB free, GPU at 0 %, upscaler
not seen. The build slot was held by Adobe After Effects test renders (`ae_render_one.py`, 6–7 s
each, about 2.4 GB) that finished at 10:23:13. The bench held the build slot from 10:23:14 to 10:23:18,
so no other gated job ran during it. The tone run (10:37:22) also came straight after an Adobe
render. Ungated processes (EffectCraft, PhotoCraft and Adobe sessions' own tools) may have been using
CPU; each number is one run, except the pictures (best and median of 5 after a warm-up).

**The 3-hour file was read from the OS file cache:** it had been written about 40 s before. A cold
open from disk will take longer to read the file.

| Measure | `long-3h-48k-stereo-16.wav` (2,073,600,044 bytes; 3 h, 48 kHz stereo 16-bit) | `tone-1k-m20-48k-stereo-16.wav` (1,920,044 bytes; 10 s, 48 kHz stereo 16-bit) |
|---|---|---|
| Open: read + decode + peaks (`Document::open`) | 1,548 ms | 3 ms |
| ... of which reading the file (`fs::read`, timed again separately) | 610 ms (warm cache) | 1 ms |
| ... of which decoding (`audio_io::decode`, timed again separately) | 258 ms | 2 ms |
| ... of which waveform peaks (`Peaks::build`, both channels side by side, timed again separately) | 261 ms | under 1 ms |
| Peak process memory during open (`PeakWorkingSetSize`) | **6.58 GB** | 0.01 GB |
| Memory held after open (working set) | 4.50 GB | 0.01 GB |
| Decoded samples alone (f32) | 4.15 GB | 3.8 MB |
| Spectrogram overview build (FFT 2048, both channels, one after the other as the worker does) | 242 ms (120 / 122 ms per channel) | 2 ms |
| Overview memory | 64.9 MB | 0.1 MB |
| Whole-file picture, 640 columns x 384 rows per channel, with the overview | 6.1 ms median (3.0 ms per channel), best 5.9 | 1.8 ms (0.9 per channel) |
| Whole-file picture, **1280 columns** x 384 rows per channel | **11.6 ms median (5.8 ms per channel)**, best 11.2 | 3.2 ms (1.6 per channel) |
| Whole-file picture, 2048 columns x 384 rows per channel | 17.5 ms median (8.7 ms per channel), best 17.3 | 5.0 ms (2.5 per channel) |
| Full ordered read of every sample, 65,536 frames at a time into a scratch buffer (the baseline for task 03's `read()`) | 598 ms (6.94 GB/s of f32) | under 1 ms |
| Working set after the spectrogram work | 4.57 GB | 0.01 GB |
| Peak process memory, whole run | 6.58 GB | 0.02 GB |

**Against the earlier figures** (app.md, 2026-10-12: 3 hours of 48 kHz **mono**, mostly measured on
samples made in memory):

- Open: 0.6 s for 3 hours of 16-bit mono before; 1.55 s for stereo now, about 2.6 times, warm cache.
  About a sixth of it is the waveform peaks, which the old figure may or may not have included.
- Overview build: 165 ms for one channel before; 120 ms per channel now.
- Pictures: 4.5 / 7.8 / 11.6 ms at 640 / 1280 / 2048 columns for one channel before; 3.0 / 5.8 /
  8.7 ms per channel now (6.1 / 11.6 / 17.5 ms for the stereo picture). The worker does channels one
  after another, so a stereo picture takes the sum.
- Open's memory peak is the file's bytes plus the decoded samples plus the peaks (2.07 + 4.15 +
  about 0.35 GB), as the code map says. That is the number task 03's blocks should not make worse.

To repeat: `wavcraft-testkit <dev root>/craft/wavcraft-testkit-out --long`, then
`wavcraft-bench <dev root>/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav`, both through the gate.

## Task 03: the store and streamed open (2026-10-15, the Windows dev machine)

*Release build, `wavcraft-store-bench FILE` (`crates/testkit`), run through the build gate in the
default slot, branch `feat/03-store-and-document`. Session store on C: (Samsung SSD 970 EVO Plus
1 TB, NVMe), under `%LOCALAPPDATA%\wavcraft\sessions\`. Source files on B:, written by the test
kit minutes before, so read from the OS file cache. Block cache budget 512 MB. Gate status around
the runs: build slot free before each, 14.3-14.5 GB free, GPU 16-19 % (not ours), a PhotoCraft
evaluation script had just run in the build slot; other sessions' ungated tools may have used CPU
and disk. Three runs on the 3-hour file; the range is given.*

| Measure | `long-3h-48k-stereo-16.wav` (2.07 GB, 518,400,000 frames) | Task 02 (whole-file decode) | Target |
|---|---|---|---|
| Streamed open: time | the Windows dev machine, source from the OS file cache: 1.75 / 1.85 / 1.92 s, once 3.46 s. The Linux handheld (see below): 3.0-4.1 s (store written: 4.147 GB of f32, 7,911 blocks per channel; 1.2-2.4 GB/s) | 1.55 s | under 10 s |
| Streamed open: peak memory above the process at rest | **5.3 MB** (peak working set 13-14 MB) | 6.58 GB peak | ≤ 300 MB |
| Sequential read of the whole document, 64 K frames at a time, block cache cleared first | 1,292-1,443 ms (2.9-3.2 GB/s of f32); again: 1,539-1,673 ms | 598 ms (from RAM) | record |
| 10,000 random 4,096-frame reads, 512 MB cache (12 % of the document; 86 % of reads miss) | median **50.8-52.2 µs**, p90 108-125 µs, p99 222-258 µs, worst 405-632 µs (first run, before reusing evicted buffers: median 60.7 µs) | n/a | median under 50 µs |
| Closing (session folder deleted) | 468 ms | n/a | |

**Open time depends on the machine and the cache (2026-10-16).** The figures under 2 s above are
the Windows dev machine's, with the source file in the OS file cache; they are not a general claim. A Linux handheld's verifier (the Linux handheld review's W2) opened a 3-hour stereo 48 kHz 24-bit WAV
(3.11 GB, ext4) with `io::open` in 4.05 s with the file's pages evicted (fadvise) and 3.05 to
3.68 s warm; `wavcraft-store-bench` gave 3.65 s. The target stays under 10 s, which both machines
meet.

**Cold disk:** the OS file cache could not be emptied (Windows has no per-file drop and the Windows dev machine has
32 GB), so the "cleared" runs above are cold for WavCraft's block cache only; the packs were still
in the OS cache. A real cold read from this NVMe would be slower. Not measured.

**Random reads:** a miss reads the whole 256 KB block with one positioned read, and most of the
50 µs is that copy out of the OS cache. A hit takes a few µs. Views and playback read sequentially
and locally, so they mostly hit; this random pattern is the worst case.

**Files over 4 GiB:** `huge-4h10m-48k-stereo-24-rf64.wav` (`wavcraft-testkit OUT --huge`: 4,320,000,080
bytes, RF64, 4 h 10 min 48 kHz stereo 24-bit, 1.44 billion samples, past audio-io's 2^30 decode
limit) opens: 720,000,000 frames, as expected, and the right channel's frame counter is right at
four places including the last 1,000 frames. Open 3.31 s (5.760 GB written), 6.2 MB above rest;
sequential read 2.0-2.1 s; random reads median 53.1 µs.

To repeat: `wavcraft-testkit <dev root>/craft/wavcraft-testkit-out --long --huge`, then
`wavcraft-store-bench <dev root>/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav` and
`wavcraft-store-bench <dev root>/craft/wavcraft-testkit-out/huge-4h10m-48k-stereo-24-rf64.wav --huge`,
all through the gate.

## Task 04: edits and undo history (2026-10-15, the Windows dev machine)

*Release build, `wavcraft-edit-bench <dev root>/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav`
(block cache 512 MB, as on an 8 GB Mac) and test 10 (`cargo test --release -p wavcraft-testkit
--test edits -- --ignored --nocapture`), both through the build gate in the default slot. Session
store on C: (Samsung 970 EVO Plus NVMe), history cap 78 GB (half the free space). Gate status
around the runs: build slot free, 12.6 to 13.2 GB memory free, GPU 15 to 21 % (not ours), no
upscaler seen; PhotoCraft had been running tests in the build slot just before. Four bench runs;
the numbers are from the last, the others agree within the noise noted.*

| Measure | Result | Target |
|---|---|---|
| Cut 1 hour out of the middle | 0.34 to 0.36 ms with crossfades 0, nothing written; 0.94 to 0.99 ms with the default 2 ms join, 768 bytes written (one 96-frame block per channel) | < 50 ms, nothing written |
| History push of that cut | 1.0 to 1.2 ms | |
| Undo of the cut | 0.58 to 0.60 ms | < 50 ms |
| Paste the hour back | 0.55 to 0.67 ms (7,917 pieces per channel after, from 7,911) | < 50 ms |
| `replace_range` of 1 minute, both channels (gain) | 12 to 15 ms; store grew 23 MB; working set +0.0 to +0.5 MB | disk ≈ 23 MB, RAM growth < 10 MB |
| `replace_range` of the whole 3 hours (gain) | 3.9 to 6.5 s; store grew 4.147 GB; peak working set 0.555 GB (the 512 MB cache full, plus 1 MB) | under 30 s; peak within 1.5 GB |
| 200 random edits (60 s ranges at most, default crossfades), history kept | 0.45 to 0.95 s in all; per kind, medians 0.2 ms (Crop) to 2.5 ms (Amplify of up to 10 s) | |
| Working set after them, 202 steps kept | **0.633 GB** (peak 0.633 GB) | under 1.5 GB |
| History push with 200 steps behind it (disk-cap sum included) | 1.0 to 1.1 ms (5,711 pieces per channel by then) | < 5 ms |
| Undo all 200 | 31 to 81 ms in all, slowest single undo 0.64 ms; bit-identical to the opened file | |
| Test 10 total (open, hash, 200 edits, undo all, hash) | 8.7 s (open 1.92 s, each whole-file hash about 3.1 s) | record |

The slowest push of a run (310 to 450 ms) is always the first after the whole-file
`replace_range` was undone: it drops that redo step, which releases 15,800 blocks and deletes
4.1 GB of packs on the calling thread. Moving pack deletion off the UI thread is left for task 05
or 07.

To repeat: `wavcraft-edit-bench <dev root>/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav` and
test 10 as above, through the gate.

## Issue #68: zoomed-out spectrogram, floor and picture lag (2026-11-03)

Measured on this Windows host (24 threads, release build), not the machine the table above used.
Nothing in the code changed; these are the numbers the issue's "Done when" needs and the decision
they leave open.

**Picture lag on a 3-hour mono 48 kHz file** (`cargo run --release -p wavcraft-core --example spectrogram_cost`):

| Measure | Result here | Issue #68 / table above |
|---|---|---|
| Overview build, one channel | 1,103 ms best, 1,342 ms median; 32.4 MB | 170 ms first overview (issue); 120 ms per channel (table above) |
| Picture with overview, 640 / 1280 / 2048 columns, one channel | 43.6 / 66.6 / 166.0 ms best; 180 / 103 / 199 ms median | 3.0 / 5.8 / 8.7 ms per channel (table above) |
| Picture from audio alone, same columns | 957 / 910 / 601 ms best | |

The two pictures are identical in all three cases. The "one frame" bound in the issue is not met
on this host. The gap to the table above (about 9 times slower overview build, 15 to 30 times
slower overview picture) is not explained. Check the host's CPU state and the build profile before
blaming the design.

**Noise floor, zoomed out against zoomed in** (10 minutes of uniform noise, amplitude 0.002, 384
rows, 640 columns; a throwaway probe, not kept). Mean drawn level in dB:

| View | Log axis | Linear axis |
|---|---|---|
| Zoomed in, one FFT a column (1,024 frames a column) | -89.9 | -88.5 |
| Zoomed out, whole file in 640 columns, peak per column (the design) | -83.2 | -82.8 |
| Zoomed out, same, mean of levels per bin over the column (tried, not kept) | -91.8 | -91.6 |

The design's peak gives a floor 6.7 dB (log) and 5.7 dB (linear) brighter than zoomed in, as the
issue says. A mean of levels per bin lands 1.9 dB and 3.1 dB darker. It misses from the other side
because a row shows the loudest bin in its band, and zoomed in that maximum is taken per FFT.
Zoomed out, the maximum is taken after averaging. Matching zoomed in exactly needs the per-FFT row
maximum averaged over the column, which the overview (per-bin levels, independent of picture height)
cannot store. A mean mode would also stop a short tone from reaching full scale when zoomed out.
That is the guarantee `a_short_tone_is_never_lost_by_zooming_out` tests. That test was not run
against a mean mode.

**Decision:** the zoomed-out spectrogram keeps the peak column (decided 2026-11-04, see issue #68),
so a short tone is never lost by zooming out; the zoomed-out noise floor is a known 6.7 dB (log) /
5.7 dB (linear) brighter than zoomed in; the mean and per-FFT row-maximum modes are not built. The
one-frame picture-lag item stays open until the run-to-run gap on one host (about 6 times) is explained.

## Issue #68: per-FFT row maximum, measured (2026-11-03, second run)

Same host and build settings as the section above. Nothing in the code changed: the peak column
stays the default. A throwaway probe (not kept) gave each zoomed-out column the mean, over its FFTs,
of each FFT's row maximum (the level of the loudest bin in the row's band, per FFT), which is what
zoomed in shows for one FFT. Noise: the same 10 minutes of uniform noise, amplitude 0.002, 384 rows,
640 columns zoomed out; zoomed in at 1,024 frames a column over its first 2 minutes. Levels are the
0-255 drawn values; the dB figures map the drawn mean back to the -100..0 scale, so only the
differences compare with the table above.

| View | Mean drawn level | Against zoomed in |
|---|---|---|
| Zoomed in, one FFT a column | 40.77 (-84.0 dB) | |
| Zoomed out, peak per column (the design in the code) | 58.22 (-77.2 dB) | +17.45 levels, **+6.85 dB** |
| Zoomed out, mean over the column of each FFT's row maximum | 40.78 (-84.0 dB) | **+0.01 dB** |

The per-FFT row maximum matches zoomed in on stationary noise. It does not keep a short tone. The
probe ran the geometry of `a_short_tone_is_never_lost_by_zooming_out` (a full-scale 1 s, 3 kHz tone in
quiet hiss, 40 minutes at 8 kHz, 256-point FFT, 128 rows) and read the tone in its own column:

| Columns (zoomed out) | 640 | 960 | 1280 | 2048 |
|---|---|---|---|---|
| Tone, per-FFT row maximum averaged | 103 | 128 | 153 | 152 |
| Hiss, highest elsewhere | 56 | 56 | 57 | 57 |
| The test needs | at least 250 | at least 250 | at least 250 | at least 250 |

So the two requirements conflict. An average over a column's FFTs matches a steady sound and dilutes
a sound that fills part of a column; the tone test asserts the opposite. The peak column keeps the
tone and is 6.85 dB bright on noise. No averaging of FFT levels meets both, so the test cannot pass
with this design as written.

A second cost: the overview keeps the loudest level per bin over 16 FFTs, which cannot give the mean of
row maxima at any picture height. The mode would need every FFT's per-bin levels: one byte per bin per
FFT, which by calculation (not measured) is about 173 MB per channel-hour against the 10.8 MB now.

**Cost example re-run** (`cargo run --release -p wavcraft-core --example spectrogram_cost`, default
180 minutes, mono, current code). Overview built in 191 ms (median 195), 32.4 MB. Picture times in ms,
best / median, audio alone then with the overview; "same" yes at all three widths:

| Columns | Audio alone | With overview | Tone level | Row median |
|---|---|---|---|---|
| 640 | 170.6 / 176.0 | 4.0 / 4.3 | 255 | 48 |
| 1280 | 171.5 / 177.0 | 7.7 / 8.6 | 255 | 47 |
| 2048 | 178.1 / 183.9 | 13.0 / 14.8 | 255 | 47 |

These differ from this same day's earlier run: there the overview took 1,103 ms and the audio-alone
picture 601 to 957 ms. The host's state changes them by several times, so one run does not settle
the lag item, and this section does not claim it met.

**Decision:** the zoomed-out spectrogram keeps the peak column (decided 2026-11-04, see issue #68),
so a short tone is never lost by zooming out; the zoomed-out noise floor is a known 6.7 dB (log) /
5.7 dB (linear) brighter than zoomed in; the mean and per-FFT row-maximum modes are not built. The
one-frame picture-lag item stays open until the run-to-run gap on one host (about 6 times) is explained.
