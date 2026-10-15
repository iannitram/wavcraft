# WavCraft

An Audition-like audio editor, built in Rust for Linux, Mac, and Windows.

A single-file waveform editor in the style of Adobe Audition's Waveform view, built on
[SoundCraft](https://github.com/storytold/soundcraft)'s engine crates. Open one file, see it as a
waveform and a spectrogram, select, fix, save. Made for spoken-word cleanup: podcasts and
audiobooks, files of one to three hours.

**Clean room:** its behaviour is copied from Audition's published behaviour and documentation only.
No code from Audacity (GPL) or any other GPL-family project is read or used. See [Rules](#rules).

**Status: Phase 1, viewer; Phase 2 under way.** It opens a file, draws it, zooms, scrolls and
plays. It does not edit or save yet in the app. Underneath, the editor's storage is in place (task 03):
an opened file is streamed into an on-disk store and held as a piece table; and the core has the
edits and the undo history (task 04: delete, cut, copy, paste, silence, insert silence, crop, and
the streamed `replace_range` effects will use, with Audition's edit crossfades), not yet wired to
the app's menus (tasks 05 and 07).

| Phase | Goal | State |
|---|---|---|
| 1. Viewer | Open, waveform + spectral display, zoom, play | first version |
| 2. Editor | Selection edits (cut, copy, paste, delete, silence), undo history, save | under way: store, document model and core edits with undo done |
| 3. Repair | Effects rack, noise reduction from a noise print, match loudness | not started |
| 4. Feel | Audition-style layout and shortcuts | not started |

## Run it

```sh
cargo run --release -p wavcraft -- path/to/file.wav
```

Rust 1.95 or later. On Linux the audio output needs the ALSA headers (`libasound2-dev`).

**The session folder** (where an opened file is kept while you work on it, about 1.4 GB per hour
of 48 kHz stereo; see "Where the samples live" below) is under the system cache folder unless
you choose another, for example a bigger disk. The first of these that is set wins:

1. `--session-dir DIR` on the command line (this run only);
2. the `WAVCRAFT_SESSION_DIR` environment variable;
3. File › Session Folder… in the app, which saves the choice in `settings.txt` in WavCraft's
   config folder (`%APPDATA%\wavcraft` on Windows, `~/Library/Application Support/wavcraft` on
   a Mac, `~/.config/wavcraft` on Linux) as `session_folder = <path>`. File › Use the Default
   Session Folder clears it;
4. the default, `<system cache folder>/wavcraft/sessions`.

A folder that can't be made or written to is not used: WavCraft says why and uses the default.
Before opening a file WavCraft checks that the folder's disk has room for it (see below) and
refuses with the size needed and the space free if not.

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

- `crates/core` (`wavcraft-core`): no UI. The on-disk sample store (`store`), the document as a
  piece table over it (`doc`), the edits (`doc::edit`) and undo history with a disk cap
  (`doc::history`), streamed open (`io`), the open file as the app sees it
  (`document`), waveform columns at any zoom, the spectrogram for any time range at screen
  resolution, and the zoom, scroll, selection and time-format arithmetic. Everything here has tests.
- `apps/wavcraft`: the egui window and the audio output.
- `crates/testkit` (`wavcraft-testkit`): `wavcraft-testkit OUT_DIR [--long] [--huge]` writes the
  synthetic test files (same seed, same bytes; keep them outside the repo), and
  `wavcraft-bench FILE` measures open, memory, peaks, spectrogram and read times, and
  `wavcraft-store-bench FILE` the store's streamed open and reads, and `wavcraft-edit-bench FILE`
  the edits, undo and history on a long file. Numbers:
  `docs/baseline.md`. Layout of the code: `docs/code-map.md`. The bench's one Windows memory call
  is the only `unsafe` in the tree (that crate denies `unsafe` instead of forbidding it).

**Where the samples live.** Opening streams the file, 64 K frames at a time, into a session
folder under the system cache directory (`wavcraft/sessions/<id>/`, removed when the file is
closed): append-only pack files of 32-bit float blocks (1 GiB each). The document is a list of
pieces of those blocks per channel, so an edit only rearranges pieces (new blocks only for new
audio: effects, crossfades, a stereo clip mixed to mono), undo keeps old piece lists, and RAM holds
only a block cache with a budget (1/16 of physical memory, 256 MB to 1 GB). Every sample takes 4
bytes there whatever the file's own format, so a 16-bit file needs twice its size: before
writing, opening checks the session folder's disk for frames × channels × 4 bytes plus 1 % and
64 MB (for a compressed file whose length the decoder can't tell, the length is guessed from its
size at 128 kbit/s), and a disk that fills up anyway stops the open with an error and removes the
partial session. WAV, RF64 and AIFF are
read by our own chunk reader, converting exactly as `soundcraft-audio-io` does (tests compare every
sample); everything else is decoded packet by packet with `symphonia`. There is no length limit
apart from disk space. **For now** the app still reads the whole document back into memory after
opening (about 1.4 GB per hour of 48 kHz stereo), because its views and player want one buffer per
channel; task 05 moves them onto the store.

Zoomed in (a pixel column spans one 2048-frame FFT window or less) each spectrogram
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
`cargo run --release -p wavcraft-core --example spectrogram_cost` measures all of this.

### Known limits (found in the 2026-10-12 review, not fixed yet)

- **The spectrogram can still trail the view by a frame or two.** A picture takes up to about
  12 ms per channel at any zoom (it was 140 ms per channel zoomed out on three hours before the
  overview), and the old one is stretched into place meanwhile. The file's first picture waits
  for the overview: about 170 ms per channel for three hours, as long as that picture took before.
- **The app still holds the whole file in memory after opening** (see above; task 05), and needs
  the file's length in disk space for its session store too.
- **The output device is chosen at start-up.** It is tried again on Play after an error or when
  there was none, but a change of the system's default device is not followed.
- When the file's rate differs from the device's, playback converts with plain linear
  interpolation (dull above 10 kHz at 44.1 → 48 kHz). Fine for finding your place, not for judging tone.

### The SoundCraft dependency

`soundcraft-audio-io` is a git dependency pinned to one commit in the root `Cargo.toml`.
Today that is our fork's merged build (upstream 0.6.1 plus long-file fixes and parallel decode),
tagged `wavecraft-pin-2026-10-12` on the fork so the commit stays fetchable if its branch goes.
(The tag keeps the app's old name, WaveCraft; it is on another repo, so it stays as it is.)
To move it: tag the new commit on the fork, change `rev`, run
`cargo update -p soundcraft-audio-io`, run the tests. Pin a commit or a tag, never a branch name.

## Rules

- **Clean room.** Audition's published behaviour and documentation are the only reference. Audacity
  is GPL: its code is never read or copied. No GPL, LGPL or AGPL code or dependencies. Dependencies are MIT, Apache-2.0, BSD, Zlib or ISC compatible, with two
  exceptions that come in unmodified: the `symphonia` decoders (MPL-2.0, through
  `soundcraft-audio-io`) and egui's default fonts (OFL-1.1 and the Ubuntu Font Licence). Both
  need a third-party notices file before any binary is handed out; there is none yet.
- **Never crash.** No `unwrap`, `expect` or `panic!` outside tests, no `unsafe`. Numbers that come
  from a file or the mouse are checked before use. The audio callback never blocks or allocates.
- Before a commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.

## License

MIT or Apache-2.0, at your option.
