Stage: Dev Loop
Since: 2026-10-27
Kind: own
Spec: SPEC.md   Roadmap: ROADMAP.md   Code map: docs/code-map.md
Platforms: mac, windows, linux
Notes: Stage 0 merged into main on 2026-10-27. CI runs fmt, clippy and tests on Linux, Windows and macOS (#1012, #1010 and #1013 are closed); main passes its checks on Linux and Windows, and the timing test that failed on the macOS runner was fixed on 2026-11-12 (step J3.1, #1145). The roadmap was rewritten on 2026-11-11 from an audit of main at 11e49a5, brought up to date on 2026-11-12, and again on 2026-11-16 from an audit of main at 92d0f13: 75 v1 items (v1.md's 74 plus L0, the speech-to-text choice), 17 complete, 259 steps left, 12 of them with an issue filed (each step gets one issue on the 1.0 milestone with one feature: label before it is taken). Take steps in stage order: ROADMAP.md's Next up lists the ones ready now.
Peers reviewed: 2026-11-18 (findings are issues labelled from-peer)

Stages: Proposal -> Spec -> Roadmap -> Dev Loop -> Prototype / Beta.
What each stage means and who moves a project: the pipeline repo (private), PIPELINE.md.
The Project Manager updates this file when the stage changes and adds a CHANGELOG line.
