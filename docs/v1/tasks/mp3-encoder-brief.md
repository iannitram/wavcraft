# Brief: our own MP3 encoder (for the Z13)

*Written 2026-10-15 by the WavCraft design session, on my decision of the same day: "wait for our own encoder, and v1 is not done without MP3". Machine: **the Z13** (SteamOS, Rust 1.95), after its other items for today. This is a brief for a run of sessions, not one build cycle: it ends with a list of steps (M1–M8), and each step is one cycle.*

## What and why

WavCraft's v1 must export MP3 (v1 item A3), and SoundCraft wants it too (upstream gap G-21). There is no usable MIT/Apache MP3 encoder in Rust: the nightly routine tested `oxideav-mp3` at 0.8–1.4× realtime and 12–13 dB SNR at 128 kbps. LAME and the other mature encoders are LGPL or GPL, which our licence wall forbids. So we write one: **a standalone, pure-Rust, MIT/Apache crate** that WavCraft and SoundCraft both depend on.

## Done means (acceptance, from my v1 list)

1. A 1-hour episode exported at 128 kbps **plays in Apple Podcasts' validator and in a browser** (Chrome, Firefox and Safari's `<audio>`, with the right duration shown and seeking that works).
2. **Lani and I can't tell it from the WAV at 128 kbps mono speech, by ear.** Blind ABX: 16 trials each on two speech clips, and neither scores better than chance (12/16 or more would be a fail).
3. Every file decodes without errors or warnings in `symphonia` (already in our tree), and decoded length minus the encoder delay and padding written in the Info tag equals the input length exactly (gapless).
4. Speed: at least **30× realtime** for mono 128 kbps on one Z13 core (an hour in under 2 minutes), and stereo 192 kbps at least 15×.
5. Never panics: NaN, infinities, full-scale square waves, silence, one sample, and odd lengths are all fuzzed.

## Scope

**In:** MPEG-1 Layer III. Sample rates 32, 44.1 and 48 kHz. Mono, stereo, and joint stereo (mid/side). **CBR** 32–320 kbps (the standard's list), default 128 kbps mono / 192 kbps stereo. Bit reservoir. A Xing/"Info" header frame (frame count, byte count, seek table) with encoder delay and padding, for duration, seeking and gapless playback. ID3v2.4 tags (title, artist, album, year, comment, cover art), with the values passed in by the app. A streaming API:

```rust
let mut enc = Encoder::new(Config { sample_rate, channels, bitrate_kbps, mode })?;
enc.encode(&[left, right])?;   // planar f32 chunks, any length
enc.finish(&mut writer)?;      // writer: Write + Seek, so the Info frame can be filled in at the end
```

**Out (later):** VBR/ABR, MPEG-2/2.5 low sample rates (22.05/24/16 kHz and below: callers resample to 44.1/48 first), intensity stereo, CRC, and multithreaded encoding (only if speed demands it).

## Where it starts: SoundCraft's `feat/mp3-export`

The nightly routine began the encoder inside SoundCraft's `audio-io`, on our public fork `iannitram/soundcraft`, branch **`feat/mp3-export`** (not merged). From [SoundCraft's notes](../../../SoundCraft/app.md) (2026-10-12):

- It has **the analysis stage, frame layout and quantizer**. The 2026-10-12 review checked the frame header, side information, padding and quantizer against the standard and found them right. **The filterbank was not reviewed.**
- **It makes no sound yet.**
- **Blocked on the Huffman tables:** the routine's sandbox refused a conversion script. The plan was to generate them from minimp3's public-domain copy (`minimp3.h`, about 70 KB, licensed **CC0**). That download was asked of me on 2026-10-12 and **approved on 2026-10-15** (me, via the Coordinator). Fetch it once, from minimp3's own repository, into a scratch folder outside the repo. Generate the tables from it with a script kept in the repo's `tools/`, commit the generated tables with a header naming the source file, its commit and its CC0 licence, and don't commit `minimp3.h` itself. CC0 is compatible with MIT/Apache. The alternative is to type the tables in from the standard's published table listings, which is slower.

Read that branch first: its commits, the code, and the nightly logs on the fork's `nightly-notes` branch that describe it. Then move what's good into the new crate (step M1). Cite the source commit in the new repo's first commit. Upstream SoundCraft only ever gets a dependency on the crate, or a PR, on my word.

## The licence wall (read before writing a line)

- **Never read the code of LAME, shine, ffmpeg/libavcodec, twolame, or any other GPL or LGPL encoder.** Not even "to see how they do X".
- Allowed references: the standards (ISO/IEC 11172-3; 13818-3 for context); published papers and textbooks on perceptual coding, the MDCT, psychoacoustic models and bit allocation; **CC0 or public-domain tables** (minimp3's, on my OK); format descriptions of the Xing/Info header and ID3v2.4 (format specs, not code).
- MP3's patents have expired (2017), so there is nothing to license.
- Any dependency must be MIT, Apache-2.0, BSD or zlib. Ideally there are none beyond `std`, with `rustfft` only if the MDCT wants it.

## Where the code lives

- **`github.com/iannitram/craft-mp3`, public** (my decision, 2026-10-15): the public SoundCraft fork and GitHub Actions release builds must fetch it without a login. Licence: MIT OR Apache-2.0, like the Craft apps. **Because it's public, nothing private goes in:** no real episode audio, no show names, no vault paths. Install the depot's pre-push private-words check (`depot-sync.sh start` does it for depot apps; until the repo has a depot entry, copy the hook the same way by hand). Commits are authored under my no-reply address (`iannitram <4624371+iannitram@users.noreply.github.com>`), as for upstream PRs.
- Clone at `~/dev/craft/<repo>` on the Z13, never in the vault.
- **Bundles:** until the Coordinator gives it a depot folder and a `depot.json` entry, write `<repo>.z13.bundle` into `apps/WavCraft/code/`. `depot-sync.sh` ignores files there that aren't `wavcraft.<machine>.bundle`, so it does no harm.
- WavCraft depends on it as a git dependency pinned to a tag, as it does SoundCraft.

## Test kit and measurement

- **Signals:** sine sweeps, pink noise, silence, clicks, full-scale square, a stereo file with different left and right, and **clean speech**. Lani and I supply real speech clips. These are never committed: keep them in `~/dev/craft/mp3-testkit/` on the Z13 and copy them through the vault's scratch only if they say so. Until then, the synthetic signals and any public-domain speech recording (LibriVox readings are public domain; check each one) will do.
- **Every step reports:** speed (× realtime); decoded-vs-source **segmental SNR** at 128 kbps on the speech clip (a regression number, not the goal); a spectrogram of the difference signal; any decoder warnings from `symphonia`; and, from M4 on, a short listening note.
- **ABX tool:** a tiny script or page that plays A, B and X at random and logs answers, for Lani and me. Keep it in the repo's `tools/`.

## Steps (one build-and-test cycle each)

| Step | What | Done when |
|---|---|---|
| **M1** | New repo; move the good parts of `feat/mp3-export` in; tests for the frame header, side info and quantizer carried over; CI-equivalent `cargo test` + clippy clean | Builds alone, tests pass, a README with the licence wall above |
| **M2** | Huffman tables, generated from minimp3's CC0 copy (approved 2026-10-15), with tests that each table is prefix-free and matches the standard's code lengths | Table tests pass |
| **M3** | **First sound:** polyphase filterbank + MDCT (review the filterbank against the standard), a simple fixed allocation, mono CBR 128 kbps, no reservoir | A sine and the speech clip decode in `symphonia` without errors and sound right; frame sizes exact |
| **M4** | Psychoacoustic model (a published approach: absolute threshold of hearing + spreading function + tonality estimate) driving the quantizer's outer/inner loops; bit reservoir | Segmental SNR and listening clearly better than M3; `main_data_begin` never invalid (test) |
| **M5** | Stereo and joint stereo (mid/side switching per frame); all CBR bit rates; 32/44.1/48 kHz | Stereo test file decodes right; every rate and bit rate round-trips |
| **M6** | Xing/Info frame with delay and padding; ID3v2.4; the streaming `Encoder` API above | Gapless length test exact; duration and seeking right in Chrome, Firefox and Safari |
| **M7** | Speed and robustness: profile, fuzz (`cargo fuzz` only if it's already installed, otherwise a proptest fuzz), the speed targets | Targets met; no panics |
| **M8** | **Lani's and my ABX** at 128 kbps mono speech; Apple Podcasts validator on a real exported episode | Acceptance 1 and 2 above. Then WavCraft wires it into Save As (a WavCraft task the coordinator writes then), and SoundCraft's use comes to me |

## Report

Each session appends a dated `## Result` section to this file (what was done, commits, the numbers, what wasn't tried by ear) and a line to the WavCraft Log in [../../app.md](../../app.md). Questions only I can answer go to the Coordinator, with a recommended default.
