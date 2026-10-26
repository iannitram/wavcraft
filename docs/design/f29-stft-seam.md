# f29: the STFT engine on main's Run seam

Sources: #139 (STFT engine, Capture Noise Print, Noise Reduction) and #154 (the engine alone).
#139 added `Effect::context_frames` and `process_in_context` with a `ChunkCtx`, so `apply` fed each
chunk its surrounding audio. Main has a different seam since #469: `Effect::begin(params, &RunContext)`
returns an optional `Run` that sees the chunks of a range in order; `apply` stays unchanged.

## Decision: keep main's seam, add nothing to the traits

Smallest change: **no edit to `effects.rs`**. Noise Reduction implements `begin` and returns a `Run`.

- `RunContext` carries the `DocState`, the range, the channels and the rate. `DocState` is `Clone`
  (a handful of `Arc`s), so the run takes `ctx.doc.clone()` and owns it. A `Run` is `'static`,
  so it cannot hold the borrowed `&DocState`.
- Per chunk the run reads its own context from that snapshot: `lead` frames before the chunk and
  `trail` after it, both `n + WARMUP * hop`, clipped to the file (frames past either end read as
  silence, as in #139). Then it calls the stateless `Stft::process` with the grid fixed at
  `range.start`. A chunk's output depends only on audio within one FFT length (plus warm-up) of
  it, so the result is the same however the range is cut. #139's `context_frames` hook did the
  same read inside `apply`; here the effect does it, so only this effect pays for it.
- Why not carry the overlap tail between chunks in the run (true streaming)? It would add `n`
  frames of latency, and the seam is length-preserving per chunk. The context read costs about
  2x the reads for a 64 K-frame chunk at the default size, which is acceptable for an alpha.

`first_frame` is **absolute** (the document frame of the chunk's first frame; `replace_range`
starts at `range.start`). #139 had `range.start + at` in `apply`; main already passes `at` alone
and has tests for it. The run must not add `range.start` again: `in_abs = first_frame - lead`,
`out_abs = first_frame`, and `range.start` is used only as the grid origin. A test pins this
(selection not at 0, crossing chunk boundaries, `reduction=0` must reproduce the input). Note
the run reads its context with the same `in_abs` it labels it with, so a double-add inside the
run would cancel out; the bug that matters is in what `apply` passes, and that is what the test
breaks on (checked by mutating `apply`).

## Noise print

`noise::capture(doc, range)` (4096-point frames, 50 % overlap, mean magnitude as a density) and a
process-wide current print are unchanged from #139. Capture is an action (`nr.capture_print`,
Shift+P) that does not touch the history, and `NOISE_PRINT` is a control verb.
`Run` reads the print once in `begin` (so a bad rate or a missing print fails before any chunk).

## Not changed
No new dependency; no change to `Effect`, `Run`, `RunContext` or `apply`.
