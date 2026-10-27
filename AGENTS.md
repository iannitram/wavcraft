# Rules for agents in this repo

Everything you need to work here is in this repo. The full pipeline and rules live in the pipeline repo (private): `PIPELINE.md`, `RULES.md`, `RUBRIC.md`, `roles/`; this is the short copy.

1. **Find the stage** in `PIPELINE.md`. The spec is `SPEC.md` (the v1 list in `docs/v1/v1.md`), the plan `ROADMAP.md`, the code map `docs/code-map.md`. While the stage is Roadmap (stage 0, the merge plan), take no issues here.
2. **Work from an issue.** Take one that is not `in-progress` and not blocked; add `in-progress` before you start. Its body is your brief; an Easy issue has a "Qwen brief" section with the expected results and the code map snippet. Only v1 items are built before 1.0; `after-1.0` issues wait.
3. **Your own platform's issues first.** Mac, Windows and Linux all matter: say which you built on.
4. **Check that it builds**, then open the PR. CI and the review do the rest.
5. **Label the PR** `by:<machine>` and `model:<model>`, and write `Closes #N`. Move the item's status in `ROADMAP.md`.
6. **A failed job** gets a comment on the issue saying what happened, and the issue moves up one size (Easy -> Medium -> Hard).
7. **Use the project's words**: Audition's names for menus, panels and effects ([docs/v1/audition-behaviour.md](docs/v1/audition-behaviour.md)), and the decisions in `docs/v1/`.
8. **No private details** in code, commits or PR text: no names, e-mail addresses, home paths or machine names. Commit as the no-reply identity.

## This project

Read `README.md` for the layout, how to run it and the known limits. Its rules, in short:

- **Clean room.** Audition's published behaviour and documentation are the only reference. Audacity is GPL: its code is never read or copied. No GPL, LGPL or AGPL code or dependencies (licences allowed: MIT, Apache-2.0, BSD, Zlib, ISC; plus `symphonia` MPL-2.0 and egui's fonts, unmodified, with a notices file).
- **Never crash.** No `unwrap`, `expect` or `panic!` outside tests, no `unsafe`. Numbers from a file or the mouse are checked before use. The audio callback never blocks or allocates.
- **Memory.** Never load a whole file into RAM: everything goes through the on-disk store (I7).
- **Before a commit:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- **SoundCraft pin.** All `soundcraft-*` crates share one commit; bump them together (README, "The SoundCraft dependency").
