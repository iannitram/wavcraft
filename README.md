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

The whole file is decoded into memory, as in SoundCraft today (about 1.4 GB per hour of 48 kHz
stereo). The spectrogram is not precomputed, so memory stays flat. Zoomed in (a pixel column spans
one 2048-frame FFT window or less) each column is one FFT centred on its moment. Zoomed out, a
column shows the loudest level in each frequency band across all the audio it spans: FFTs are taken
half a window apart on a grid fixed to the file, and each column keeps the per-bin maximum of the
ones inside it. So nothing audible falls between two columns, and scrolling moves the picture
without changing it. The price is that a zoomed-out view costs time in proportion to the audio on
screen (about 140 ms per channel for three hours at 48 kHz, on 24 threads), so pictures are made
on a worker thread and the window draws the last one, in its right place, until the next arrives.
`cargo run --release -p wavecraft-core --example spectrogram_cost` measures it.

### Known limits (found in the 2026-10-12 review, not fixed yet)

- **Zoomed far out on a long file, the spectrogram trails the view.** Each new view of three
  hours takes about 140 ms per channel, so while zooming or scrolling there the picture updates
  a few times a second (the old one is stretched into place meanwhile). A cached overview would
  make it instant; there is none yet. This one is the price of the review's spectrogram fix.
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
