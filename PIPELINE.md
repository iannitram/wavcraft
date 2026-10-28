Stage: Dev Loop
Since: 2026-10-27
Kind: own
Spec: SPEC.md   Roadmap: ROADMAP.md   Code map: docs/code-map.md
Platforms: mac, windows, linux
Notes: Stage 0 merged into main on 2026-10-27 (44 merge commits; all 30 old PRs and stray branches triaged, unmerged ones kept as archive/ tags; review findings filed as #987-#1011). main does not pass its own checks yet: take #1012 (make main green), #1010 (remove the stub limiter) and #1013 (add CI) before any other issue. No CI yet, so check fmt, clippy and tests locally before a PR. Issues outside after-1.0 are sized; about 450 older ones stay unsized and after-1.0.

Stages: Proposal -> Spec -> Roadmap -> Dev Loop -> Prototype / Beta.
What each stage means and who moves a project: the pipeline repo (private), PIPELINE.md.
The Project Manager updates this file when the stage changes and adds a CHANGELOG line.
