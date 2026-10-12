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
| File › Open, Ctrl+O, or drop a file on the window | open a file (WAV, AIFF, FLAC, MP3, OGG, M4A, CAF) |
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
stereo). The spectrogram is not precomputed: each pixel column is one FFT centred on its moment,
so any view costs the same and memory stays flat. Zoomed far out, a click shorter than one
column can fall between two of them.

### The SoundCraft dependency

`soundcraft-audio-io` is a git dependency pinned to one commit in the root `Cargo.toml`.
Today that is our fork's merged build (upstream v0.5.0 plus long-file fixes and parallel decode).
To move it: change `rev`, run `cargo update -p soundcraft-audio-io`, run the tests. Pin a commit
or a tag, never a branch name.

## Rules

- **Clean room.** Audition and Audacity are references for behaviour only. No GPL, LGPL or AGPL
  code or dependencies; every dependency is MIT, Apache-2.0, BSD, Zlib or ISC compatible.
- **Never crash.** No `unwrap`, `expect` or `panic!` outside tests, no `unsafe`. Numbers that come
  from a file or the mouse are checked before use. The audio callback never blocks or allocates.
- Before a commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.

## License

MIT or Apache-2.0, at your option.
