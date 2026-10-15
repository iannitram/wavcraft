# Baseline numbers

*Measured 2026-10-15 on Hyperion (Windows 11, 24 rayon threads, 32 GB), release build, by task 02
(`plan/tasks/02-code-map-test-kit-baseline.md` in the vault). Code: commit `4176652` (the binaries
were built from `50e8dfe`; `4176652` only reformats the source). Tool: `wavcraft-bench FILE`
(`crates/testkit`), run through the build gate in the default slot. Files from
`wavcraft-testkit OUT --long` (seed 1463899717), in `B:\dev\craft\wavcraft-testkit-out\`.*

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

To repeat: `wavcraft-testkit B:/dev/craft/wavcraft-testkit-out --long`, then
`wavcraft-bench B:/dev/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav`, both through the gate.

## Task 03: the store and streamed open (2026-10-15, Hyperion)

*Release build, `wavcraft-store-bench FILE` (`crates/testkit`), run through the build gate in the
default slot, branch `feat/03-store-and-document`. Session store on C: (Samsung SSD 970 EVO Plus
1 TB, NVMe), under `%LOCALAPPDATA%\wavcraft\sessions\`. Source files on B:, written by the test
kit minutes before, so read from the OS file cache. Block cache budget 512 MB. Gate status around
the runs: build slot free before each, 14.3-14.5 GB free, GPU 16-19 % (not ours), a PhotoCraft
evaluation script had just run in the build slot; other sessions' ungated tools may have used CPU
and disk. Three runs on the 3-hour file; the range is given.*

| Measure | `long-3h-48k-stereo-16.wav` (2.07 GB, 518,400,000 frames) | Task 02 (whole-file decode) | Target |
|---|---|---|---|
| Streamed open: time | 1.75 / 1.85 / 1.92 s, once 3.46 s (store written: 4.147 GB of f32, 7,911 blocks per channel; 1.2-2.4 GB/s) | 1.55 s | under 10 s |
| Streamed open: peak memory above the process at rest | **5.3 MB** (peak working set 13-14 MB) | 6.58 GB peak | ≤ 300 MB |
| Sequential read of the whole document, 64 K frames at a time, block cache cleared first | 1,292-1,443 ms (2.9-3.2 GB/s of f32); again: 1,539-1,673 ms | 598 ms (from RAM) | record |
| 10,000 random 4,096-frame reads, 512 MB cache (12 % of the document; 86 % of reads miss) | median **50.8-52.2 µs**, p90 108-125 µs, p99 222-258 µs, worst 405-632 µs (first run, before reusing evicted buffers: median 60.7 µs) | n/a | median under 50 µs |
| Closing (session folder deleted) | 468 ms | n/a | |

**Cold disk:** the OS file cache could not be emptied (Windows has no per-file drop and Hyperion has
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

To repeat: `wavcraft-testkit B:/dev/craft/wavcraft-testkit-out --long --huge`, then
`wavcraft-store-bench B:/dev/craft/wavcraft-testkit-out/long-3h-48k-stereo-16.wav` and
`wavcraft-store-bench B:/dev/craft/wavcraft-testkit-out/huge-4h10m-48k-stereo-24-rf64.wav --huge`,
all through the gate.
