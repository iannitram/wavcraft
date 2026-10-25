# Add two tests to dsp/envelope.rs (tests only)

Refs #76. Change only `crates/core/src/dsp/envelope.rs`, and only by adding two functions inside the existing `mod tests` (it starts with `use super::*;`). No production code change.

## Step 0: check the base first (before you edit anything)
Run `grep -n "fn monotone_validation_too_few_precedes_nonfinite\|fn monotone_validation_nonfinite_precedes_not_increasing" crates/core/src/dsp/envelope.rs` and `grep -n "pub fn gain_envelope\|GainError::TooFewPoints\|GainError::NonFinite\|GainError::NotIncreasing" crates/core/src/dsp/envelope.rs`.
- If either test name is already there, or `gain_envelope` or one of the three error returns is missing, STOP and report that. Write nothing.
- Otherwise add the two tests. The harness checks the same thing before it loads the model.

## Rules
- Do not edit, rename, split or reformat any existing test. Add exactly the two tests below, with exactly these assertions and literals. Do not weaken them (no `is_err()`).
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` (denied by the crate), and no `assert_eq!(x, true)`.
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit the file.
- Do not add dependencies, files or `debug_*.rs` scratch files.
- If a stated literal cannot be matched, stop and report it. Do not change it.

## Why
Existing tests check each validation error alone, but not the precedence when an input is invalid in two ways. A reorder of the guards in `gain_envelope` would change the public error while every current test passes. These tests use `Interp::Monotone`.

## Behaviour (already in the code, do not change)
`gain_envelope` checks in this order: fewer than two points gives `GainError::TooFewPoints`; then any non-finite time or gain gives `GainError::NonFinite`; then a time pair that is not strictly increasing gives `GainError::NotIncreasing`. Both tests PASS on the code as it is. They guard it against a later reorder.

## Add inside `mod tests`
```rust
    #[test]
    fn monotone_validation_too_few_precedes_nonfinite() {
        assert_eq!(
            gain_envelope(&[(f32::NAN, 0.0)], &[0.5], Interp::Monotone),
            Err(GainError::TooFewPoints)
        );
    }

    #[test]
    fn monotone_validation_nonfinite_precedes_not_increasing() {
        assert_eq!(
            gain_envelope(&[(0.0, 0.0), (0.0, f32::NAN)], &[0.5], Interp::Monotone),
            Err(GainError::NonFinite)
        );
    }
```

## Expected values (computed beforehand by an independent Python reference)
- `monotone_validation_too_few_precedes_nonfinite`: `Err(GainError::TooFewPoints)`.
- `monotone_validation_nonfinite_precedes_not_increasing`: `Err(GainError::NonFinite)`.

## Mutation checks (the harness runs these, not you)
One mutation per test is applied to the production code and the test must FAIL with a real `test result: FAILED` line; a compile error or a missing result line counts as NOT killed.
- Letting a single point through the length guard (`points.len() < 2` becomes `< 1`) makes the first input give `NonFinite` instead of `TooFewPoints`: kills `monotone_validation_too_few_precedes_nonfinite`.
- Skipping the non-finite check makes the second input give `NotIncreasing` instead of `NonFinite`: kills `monotone_validation_nonfinite_precedes_not_increasing`.

## Commands (run from the repo root; this job runs on a Linux machine, so use `qcargo`, never plain `cargo`)
```sh
qcargo test -p wavcraft-core envelope
qcargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/dsp/envelope.rs
```
rustfmt that one file only, never the whole crate. Stop when the envelope tests pass, clippy is clean, and rustfmt has been run.
