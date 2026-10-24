# Branch integration log

Started 2026-10-24 from `origin/main` at `ad2a330`. Use this log with
[`code-map.md`](code-map.md) for module ownership and [`roadmap.md`](roadmap.md)
for feature dependencies and known conflict points. The roadmap is newer than
the code map for the multitrack and effects stacks.

## Inventory

- 254 distinct remote branch tips at the initial fetch.
- 59 tips were already ancestors of `main`; 195 were outside it.
- Of those 195, 79 were ancestors of another unmerged remote tip. This leaves
  116 maximal tips to review, not 116 automatic merges. Alternate trials,
  recuts and old integration work can overlap without sharing ancestry.
- After the batches below, 185 remote tips remain outside this
  integration branch.

## Integration branch

`integration/branch-triage-2026-10-24` starts from `origin/main`. The first
batch merges four independent core additions:

| Remote branch | Commit | Module | Product status |
| --- | --- | --- | --- |
| `feat/qwen-b169-ab-compare` | `afb9bf8` | effects | A/B parameter state; dialog wiring remains |
| `feat/qwen-a077-fade-curve` | `83f987f` | DSP | Fade math; editor handles remain |
| `feat/qwen-a319-band-only-filter` | `713e195` | DSP | Filter math; playback toggle remains |
| `feat/qwen-a326-source-link` | `7ca3ca4` | metadata | Source link parser; export writing and Edit Original remain |

The second batch adds focused tests and two more core models:

| Remote branch | Commit | Module | Product status |
| --- | --- | --- | --- |
| `fleet/wav-m03-echo-tests` | `2bd00ff` | DSP | Echo edge-case tests |
| `fleet/wav-q-b089-a2` | `0c93a13` | DSP | Doppler path model; delay-line rendering remains |
| `qwen-T06-chorus-guard-tests` | `bf6a6df` | DSP | Chorus guard tests |
| `fleet/wav-q-a166-a` | `d6c2cc5` | mix | Band crossover plan; filtering and track wiring remain |

The next batch integrates `qwen/b098-a` (stereo-width DSP, merge `fda27c3`)
and `feat/ap-f43-save-selection-export` (Save Selection As and Export, merge `6f05b50`).
The export merge also validates public `Part` ranges and channel indices before
writing so malformed requests return an error instead of panicking.

The Doppler merge corrects the one blocking item recorded in
[issue #665](https://github.com/iannitram/wavcraft/issues/665): `pub mod doppler;`
now sits between `distortion` and `echo`. This is the path-model part of B089;
it does not include the later fractional delay line or the complete effect.
`gemini-trial/b089a` has an identical `doppler.rs` plus unrelated changes and
needs a separate comparison before any merge.

Each merge passed strict workspace Clippy and `cargo test --workspace` before
commit. Added files were formatted with rustfmt. The repository-wide
`cargo fmt --all --check` currently reports pre-existing formatting differences
in unrelated files with the installed Rust 1.95 toolchain; do not mix a broad
formatting sweep into a feature merge.
The desktop app was also built and smoke-tested under Xvfb with a one-second
48 kHz WAV: it reported 48,000 frames through the control channel and exited
cleanly.

## Next passes

1. Review small independent fixes and tests against current code, then merge
   those that still add behavior. Avoid merging a test branch whose behavior
   has since changed.
2. Follow the stacks in `roadmap.md`: time display before SMPTE and bars/beats;
   preferences before preference children; meters before meter children; rack
   before tail handling; effect run state before pitch, reverb and stretch
   children; multitrack session before its children.
3. Reconcile shared registry lines and settings persistence during those
   stacks. Run formatting on changed files, strict Clippy and workspace tests
   for each merge.
4. Compare old `integration/wavcraft-2026-10-20` and trial/recut branches for
   unique behavior after the current feature stacks. Its tip has 149 commits
   outside `main` and 22 textual merge conflicts against the initial base.

Merging an alpha core branch does not finish its user-facing feature. Track
the remaining UI and playback work in the feature's own follow-up.
