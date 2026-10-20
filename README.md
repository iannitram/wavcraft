# WavCraft

An Audition-like audio editor, built in Rust for Linux, Mac, and Windows.

A single-file waveform editor in the style of Adobe Audition's Waveform view, built on
[SoundCraft](https://github.com/storytold/soundcraft)'s engine crates. Open one file, see it as a
waveform and a spectrogram, select, fix, save. Made for spoken-word cleanup: podcasts and
audiobooks, files of one to three hours.

**Clean room:** its behaviour is copied from Audition's published behaviour and documentation only.
No code from Audacity (GPL) or any other GPL-family project is read or used. See [Rules](#rules).

**Status: Phase 1, viewer; Phase 2 under way.** It opens a file, draws it, zooms, scrolls and
plays, and edits and saves WAV in the app (tasks 06a, 07a, 07b). Underneath, the editor's storage is in place (task 03):
an opened file is streamed into an on-disk store and held as a piece table; and the core has the
edits and the undo history (task 04: delete, cut, copy, paste, silence, insert silence, crop, and
the streamed `replace_range` effects will use, with Audition's edit crossfades), not yet wired to
the app's menus (tasks 05 and 07).

**Roadmap:** the ranked plan to Audition parity, with what is built and what is next, is in [docs/roadmap-audition-parity.md](docs/roadmap-audition-parity.md).

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
| = / − | zoom in / out around the middle of the view |
| Ctrl+\ (⌘\ on a Mac) | zoom out to the whole file |
| Shift+S | zoom to the selection |
| Click | place the cursor |
| Drag | select a range |
| Space | play from the cursor (or play the selection); again to stop; resumes after Pause |
| Transport panel | Play, Pause, Stop, Loop (selection, or the view), Skip Selection (play around the selection to hear a cut), go to start / end; marker buttons are off until markers exist; Auto-Scroll (pagewise or centred; moving the view by hand turns it off) |
| Shift+X | Return to Start Position on Stop on / off (on: Stop puts the cursor back where play started) |
| Home / End | cursor to the start / end |
| I / O | selection start / end at the playhead (the cursor when stopped) |
| Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y | undo, redo (the Edit menu names the step) |
| Ctrl+X, Ctrl+C, Ctrl+V | cut, copy, paste (with nothing selected, cut and copy take the whole file) |
| Delete or Backspace | delete the selection (one active channel: silence it there, keeping the length) |
| Ctrl+T | crop to the selection |
| Ctrl+A / Ctrl+Shift+A or Ctrl+D / G | select all / deselect / clear the time selection |
| ↑ / ↓ | edit the left / right channel only (again: both) |
| Edit › Silence, Edit › Insert Silence… | silence the selection / insert silence at the cursor (no keys, as in Audition) |
| Ctrl+S, Ctrl+Shift+S | save (WAV), save as |
| Window › History panel | click a step to go to it; Clear History… |

Keys follow Adobe Audition's defaults (`plan/audition-parity.md` in the depot), with Ctrl as ⌘ on a Mac.

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
  is one of the two `unsafe` exceptions in the tree (that crate denies `unsafe` instead of
  forbidding it). The other, and the last: the app's `win_acl.rs` (Windows only), the Win32
  calls that make the control channel's token file owner-only (wavcraft#8, 2026-10-19).

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

## Third-party code and credits

Adopted or recommended sources, by project, link and licence only:

- SoundCraft, https://github.com/storytold/soundcraft (MIT OR Apache-2.0)
- FilmCraft, https://github.com/storytold/filmcraft (MIT OR Apache-2.0)
- EffectCraft, https://github.com/storytold/effectcraft (MIT OR Apache-2.0)
- craft-libs, https://github.com/storytold/craft-libs (MIT OR Apache-2.0)
- Symphonia, https://github.com/pdeljanov/Symphonia (MPL-2.0, unmodified dependency)
- rubato, https://github.com/HEnquist/rubato (MIT OR Apache-2.0)
- ebur128, https://github.com/sdroege/ebur128 (MIT)
- bwavfile, https://github.com/iluvcapra/bwavfile (MIT)
- lofty, https://crates.io/crates/lofty (MIT OR Apache-2.0)
- id3, https://crates.io/crates/id3 (MIT)
- quick-xml, https://crates.io/crates/quick-xml (MIT); xmp_writer, https://crates.io/crates/xmp_writer (MIT OR Apache-2.0)
- RustFFT, https://crates.io/crates/rustfft (MIT OR Apache-2.0); realfft, https://crates.io/crates/realfft (MIT)
- fft-convolver, https://crates.io/crates/fft-convolver (MIT)
- Signalsmith Stretch and its Rust wrapper, https://crates.io/crates/signalsmith-stretch (MIT)
- nnnoiseless, https://github.com/jneem/nnnoiseless (BSD-3-Clause, with RNNoise)
- ogg, https://crates.io/crates/ogg (BSD-3-Clause)
- vorbis_rs, https://crates.io/crates/vorbis_rs (BSD-3-Clause, with libvorbis and libogg)
- flac-codec, https://crates.io/crates/flac-codec (MIT OR Apache-2.0)
- rtrb, https://crates.io/crates/rtrb; basedrop, https://crates.io/crates/basedrop; creek, https://codeberg.org/Meadowlark/creek (MIT OR Apache-2.0)
- triple_buffer, https://crates.io/crates/triple_buffer (MPL-2.0, unmodified dependency)
- egui_plot, egui_table, egui_kittest, undo (MIT OR Apache-2.0); egui_dock, egui_knob, egui-bind (MIT), all on crates.io
- cpal, https://crates.io/crates/cpal (Apache-2.0)
- VST 3 SDK (MIT), clap-sys and vst3 (MIT OR Apache-2.0), libloading (ISC)

Every source is checked against WavCraft's MIT OR Apache-2.0 licence before any code is reused.
The full table (versions, rows served, notes, and the "do not use" list) is in
[docs/roadmap-audition-parity.md](docs/roadmap-audition-parity.md), section "Reuse sources".

## License

MIT or Apache-2.0, at your option.
