# WaveCraft (working title)

A single-file waveform editor in the style of Adobe Audition's Waveform view, built on
[SoundCraft](https://github.com/storytold/soundcraft)'s engine crates. Open one file, see it as a
waveform and a spectrogram, select, fix, save. Made for spoken-word cleanup: podcasts and
audiobooks, files of one to three hours.

**Status: Phase 1, viewer.** It opens a file, draws it, zooms, scrolls and plays. It does not
edit or save yet.

| Phase | Goal | State |
|---|---|---|
| 1. Viewer | Open, waveform + spectral display, zoom, play | first version |
| 2. Editor | Selection edits (cut, copy, paste, delete, silence), undo history, save | not started |
| 3. Repair | Effects rack, noise reduction from a noise print, match loudness | not started |
| 4. Feel | Audition-style layout and shortcuts | not started |

## Run it

```sh
cargo run --release -p wavecraft -- path/to/file.wav
```

Rust 1.95 or later. On Linux the audio output needs the ALSA headers (`libasound2-dev`).

| Do this | To |
|---|---|
| File › Open, Ctrl+O, or drop a file on the window (dropping does not work under Wayland yet) | open a file (WAV, AIFF, FLAC, MP3, OGG, M4A, CAF) |
| Mouse wheel | zoom in and out around the pointer |
| Shift + wheel, or sideways scroll | move through time |
| `\` | zoom out to the whole file |
| Click | place the cursor |
| Drag | select a range |
| Space | play from the cursor (or play the selection); again to stop |
| Home | cursor back to the start |

## How it is built

- `crates/core` (`wavecraft-core`): no UI. The open file, waveform columns at any zoom, the
  spectrogram for any time range at screen resolution, and the zoom, scroll, selection and
  time-format arithmetic. Everything here has tests.
- `apps/wavecraft`: the egui window and the audio output.
- `crates/testkit` (`wavecraft-testkit`): `wavecraft-testkit OUT_DIR [--long]` writes the
  synthetic test files (same seed, same bytes; keep them outside the repo), and
  `wavecraft-bench FILE` measures open, memory, peaks, spectrogram and read times. Numbers:
  `docs/baseline.md`. Layout of the code: `docs/code-map.md`. The bench's one Windows memory call
  is the only `unsafe` in the tree (that crate denies `unsafe` instead of forbidding it).

The whole file is decoded into memory, as in SoundCraft today (about 1.4 GB per hour of 48 kHz
stereo). Zoomed in (a pixel column spans one 2048-frame FFT window or less) each spectrogram
column is one FFT centred on its moment. Zoomed out, a column shows the loudest level in each
frequency band across all the audio it spans: FFTs are taken half a window apart on a grid fixed
to the file, and each column keeps the per-bin maximum of the ones inside it. So nothing audible
falls between two columns, and scrolling moves the picture without changing it.

Analysing all the audio on screen takes about 140 ms per channel for a view of three hours at
48 kHz (on 24 threads), so two things keep the window quick. Pictures are made on a worker thread,
and the window draws the last one, in its right place, until the next arrives. And the worker
builds an overview of each channel before the file's first picture (that picture shows the whole
file, so it would run the same FFTs anyway; about 170 ms per channel for three hours). The
overview keeps the loudest level per frequency bin of every 16 FFTs of the grid (a third of a
second of audio), one byte per bin: 10.8 MB per channel-hour at 48 kHz, 1.6 % of what the samples
take. A column that holds whole blocks of 16 reads them and runs FFTs only for the ends that stick
out, which gives the same picture, level for level, in 4 to 12 ms per channel for three hours at
640 to 2048 columns. Blocks twice as long would halve the memory and double those times. Views
closer than one block per column are analysed from the audio as before, and there the audio on
screen is short. The overview does not depend on the frequency axis or the picture's size.
`cargo run --release -p wavecraft-core --example spectrogram_cost` measures all of this.

### Known limits (found in the 2026-10-12 review, not fixed yet)

- **The spectrogram can still trail the view by a frame or two.** A picture takes up to about
  12 ms per channel at any zoom (it was 140 ms per channel zoomed out on three hours before the
  overview), and the old one is stretched into place meanwhile. The file's first picture waits
  for the overview: about 170 ms per channel for three hours, as long as that picture took before.
- **Files over 2^30 samples do not open** (3 h 06 min of 48 kHz stereo): the decode limit of
  `soundcraft-audio-io`.
- **Opening needs several times the file's size in memory for a moment**, a load cannot be
  cancelled, and opening a second file while the first is loading decodes both.
- **The output device is chosen at start-up.** It is tried again on Play after an error or when
  there was none, but a change of the system's default device is not followed.
- When the file's rate differs from the device's, playback converts with plain linear
  interpolation (dull above 10 kHz at 44.1 → 48 kHz). Fine for finding your place, not for judging tone.

### The SoundCraft dependency

`soundcraft-audio-io` is a git dependency pinned to one commit in the root `Cargo.toml`.
Today that is our fork's merged build (upstream 0.6.1 plus long-file fixes and parallel decode),
tagged `wavecraft-pin-2026-10-12` on the fork so the commit stays fetchable if its branch goes.
To move it: tag the new commit on the fork, change `rev`, run
`cargo update -p soundcraft-audio-io`, run the tests. Pin a commit or a tag, never a branch name.

## Rules

- **Clean room.** Audition and Audacity are references for behaviour only. No GPL, LGPL or AGPL
  code or dependencies. Dependencies are MIT, Apache-2.0, BSD, Zlib or ISC compatible, with two
  exceptions that come in unmodified: the `symphonia` decoders (MPL-2.0, through
  `soundcraft-audio-io`) and egui's default fonts (OFL-1.1 and the Ubuntu Font Licence). Both
  need a third-party notices file before any binary is handed out; there is none yet.
- **Never crash.** No `unwrap`, `expect` or `panic!` outside tests, no `unsafe`. Numbers that come
  from a file or the mouse are checked before use. The audio callback never blocks or allocates.
- Before a commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.

## License

MIT or Apache-2.0, at your option.
