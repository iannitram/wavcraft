# Baseline numbers

*Measured 2026-10-15 on Hyperion (Windows 11, 24 rayon threads, 32 GB), release build, by task 02
(`plan/tasks/02-code-map-test-kit-baseline.md` in the vault). Code: commit `4176652` (the binaries
were built from `50e8dfe`; `4176652` only reformats the source). Tool: `wavecraft-bench FILE`
(`crates/testkit`), run through the build gate in the default slot. Files from
`wavecraft-testkit OUT --long` (seed 1463899717), in `B:\dev\craft\wavecraft-testkit-out\`.*

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

To repeat: `wavecraft-testkit B:/dev/craft/wavecraft-testkit-out --long`, then
`wavecraft-bench B:/dev/craft/wavecraft-testkit-out/long-3h-48k-stereo-16.wav`, both through the gate.
