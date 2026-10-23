# B040-a: key-driven gain computer for sidechain dynamics (new file `crates/core/src/effects/sidechain.rs`)

Refs #122 (the row has no issue of its own; #122 is the issue of its base PR 155, Compressor, Hard Limiter, DeEsser).

Roadmap row B040 (Medium, T9): "Sidechain / key input for dynamics". Scope (verbatim from the queue): "Key-driven gain computer, no trait wiring: `key_gain(key: &[f32], threshold_db, ratio, attack_ms, release_ms, rate) -> Vec<f32>` (linear gain per sample from the level of `key`, hard knee, attack/release one-pole with the coefficient of `dynamics.rs::coef`); `apply_gain(program: &mut [f32], gain: &[f32])`; `duck(program: &mut [f32], key: &[f32], ...)` (key_gain + apply_gain). Letting the Effect trait take a second input is NOT in this job."
Location (verbatim): new `crates/core/src/effects/sidechain.rs`; module line in `crates/core/src/effects.rs`; pattern function: `effects/dynamics.rs::coef` (:27) and the Compressor run (:103) on #155; map: crates__core__src__effects.md.
Base branch: `feat/ap-f28-dynamics` (PR 155; its own base is `main`).

## Files you may touch
ONLY these two: `crates/core/src/effects/sidechain.rs` (new) and `crates/core/src/effects.rs` (add ONE line). Nothing else. Never edit an existing test.

## Step 0: check the base first
If the first command lists the file, or the second finds a line, STOP and report; write nothing. The third must list `dynamics.rs`.
```
ls crates/core/src/effects/sidechain.rs
grep -n "sidechain" crates/core/src/effects.rs
ls crates/core/src/effects/
```

## Rules
- No git or gh commands, no network. No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!` (the crate denies them, tests included). No `assert_eq!(x, true)`.
- No extra files, no `debug_*.rs`. Do not edit any existing test or function. Do not change `dynamics.rs`.
- Stop and report if a stated literal cannot be matched by the code you typed.
- Run rustfmt on the changed files only: `rustfmt --edition 2024 crates/core/src/effects/sidechain.rs crates/core/src/effects.rs`.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output and quote it in your report.

## Why and behaviour
Sidechain ducking: the level of one signal (the key, for example a voice) turns down another (the program, for example music). The existing compressor can only listen to its own input. This job is the pure maths: from a key signal compute a gain per sample, then multiply a program by it. It does NOT change the `Effect` trait.
- Per key sample: level = `|sample|`; level in dB = `20 * log10(level)` (a level of 1e-9 or less, and NaN, count as -180 dB). `over = level_db - threshold_db`. Hard knee: when `over` is finite and above 0 the target gain is `10^(-over * (1 - 1/ratio) / 20)`, else 1.0. A ratio that is not above 1 (or NaN) means slope 0: target 1.0. A ratio of infinity is a hard limit (slope 1).
- Smoothing: the gain starts at 1.0. For each sample it moves to the target as a one-pole: `gain = target + (gain - target) * coef`, where `coef` is the attack coefficient when `target < gain` (going down) and the release coefficient otherwise. A coefficient is `exp(-1 / n)` with `n = ms * 0.001 * rate` samples (a time constant: after `n` samples the gain has covered 63.2 % of the way), and 0.0 (reacts at once) when `n` is not above 0 or NaN.
- `key_gain` returns one f32 per key sample. `apply_gain` multiplies `program[i]` by `gain[i]` for the first `min(program.len(), gain.len())` samples and leaves the rest of the program untouched. `duck` is `key_gain` then `apply_gain`, so a longer program keeps its tail untouched and a longer key is ignored past the program's end.
- All maths in f64, cast to f32 once, when a gain is stored.
Code map: `maps/wavcraft/crates__core__src__effects.md` (describes main; read `effects.rs` and `effects/dynamics.rs` on this base). New file sits beside `crates/core/src/effects/dynamics.rs` and `gain.rs`. Registration: in `crates/core/src/effects.rs` add the single line `pub mod sidechain;` directly after the existing line `mod gain;` (the file has `mod dynamics;` and `mod gain;` then `pub use` lines; leave all of them as they are). The coefficient helper of the existing dynamics module is private, so this file has its own copy (do not change or call it). Existing pattern function, `effects/dynamics.rs` on this base:
```rust
/// Per-sample smoothing coefficient for a time constant in milliseconds.
fn coef(ms: f64, rate: f64) -> f32 {
    let n = ms * 0.001 * rate;
    if n < 1.0e-3 { 0.0 } else { (-1.0 / n).exp() as f32 }
}
```

## The complete new file `crates/core/src/effects/sidechain.rs`
Type it exactly (then run rustfmt on it).
```rust
//! Sidechain key gain (roadmap B040, sub-job a): a gain per sample worked out from the level of a
//! key signal, and a way to apply it to another signal (the program). This is the pure maths;
//! the `Effect` trait does not take a second input yet.

/// The level in dB of a silent (or not a number) key sample.
const SILENCE_DB: f64 = -180.0;

/// The one-pole coefficient `exp(-1 / n)` for a time constant of `ms` milliseconds at `rate` Hz
/// (`n = ms * 0.001 * rate` samples), or 0.0 (react at once) when `n` is not above 0.
fn smoothing(ms: f32, rate: f32) -> f64 {
    let n = f64::from(ms) * 0.001 * f64::from(rate);
    if n > 0.0 { (-1.0 / n).exp() } else { 0.0 }
}

/// The linear gain for each sample of `key`, with a hard knee.
///
/// Above `threshold_db` the target gain is `10^(-over * (1 - 1/ratio) / 20)`, where `over` is the
/// key level in dB minus the threshold; otherwise 1.0. A ratio that is not above 1 (or NaN) gives
/// no change; a non-finite threshold or key sample gives no reduction. The gain starts at 1.0 and
/// follows the target as a one-pole: the attack time when it goes down, the release time when it
/// goes up. The result has one gain per key sample.
pub fn key_gain(key: &[f32], threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, rate: f32) -> Vec<f32> {
    let slope = if ratio > 1.0 { 1.0 - 1.0 / f64::from(ratio) } else { 0.0 };
    let threshold = f64::from(threshold_db);
    let attack = smoothing(attack_ms, rate);
    let release = smoothing(release_ms, rate);
    let mut gain = 1.0_f64;
    key.iter()
        .map(|&sample| {
            let level = f64::from(sample).abs();
            let level_db = if level > 1.0e-9 { 20.0 * level.log10() } else { SILENCE_DB };
            let over = level_db - threshold;
            let target = if over.is_finite() && over > 0.0 { 10f64.powf(-over * slope / 20.0) } else { 1.0 };
            let coef = if target < gain { attack } else { release };
            gain = target + (gain - target) * coef;
            gain as f32
        })
        .collect()
}

/// Multiply `program[i]` by `gain[i]` for the first `min(program.len(), gain.len())` samples; the
/// rest of `program` is left as it is.
pub fn apply_gain(program: &mut [f32], gain: &[f32]) {
    for (sample, g) in program.iter_mut().zip(gain) {
        *sample *= g;
    }
}

/// Turn `program` down by the level of `key`: [`key_gain`] then [`apply_gain`].
pub fn duck(program: &mut [f32], key: &[f32], threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, rate: f32) {
    let gain = key_gain(key, threshold_db, ratio, attack_ms, release_ms, rate);
    apply_gain(program, &gain);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-4, "{a} != {b}");
    }

    #[test]
    fn a_silent_key_leaves_the_program_untouched() {
        let mut program: Vec<f32> = (0..100).map(|k| k as f32 * 0.01 - 0.5).collect();
        let before = program.clone();
        duck(&mut program, &[0.0; 100], -20.0, 4.0, 1.0, 1.0, 1000.0);
        assert_eq!(program, before);
    }

    #[test]
    fn a_loud_key_reduces_the_gain_by_the_ratio_math_at_steady_state() {
        let g = key_gain(&[1.0; 2000], -20.0, 4.0, 1.0, 1.0, 1000.0);
        assert_eq!(g.len(), 2000);
        approx(g[1999], 0.177_827_94);
        let g = key_gain(&[0.5; 400], -12.0, 2.0, 1.0, 1.0, 1000.0);
        approx(g[399], 0.708_785_8);
    }

    #[test]
    fn the_attack_covers_63_percent_of_the_way_in_one_time_constant() {
        let g = key_gain(&[1.0; 10], -20.0, 4.0, 10.0, 100.0, 1000.0);
        approx(g[9], 0.480_288_14);
    }

    #[test]
    fn the_release_covers_63_percent_of_the_way_in_one_time_constant() {
        let mut key = vec![1.0; 200];
        key.extend(vec![0.0; 20]);
        let g = key_gain(&key, -20.0, 4.0, 1.0, 20.0, 1000.0);
        approx(g[199], 0.177_827_94);
        approx(g[219], 0.697_539_8);
    }

    #[test]
    fn different_lengths_use_the_shorter() {
        let mut program = [1.0, 1.0, 1.0, 1.0];
        apply_gain(&mut program, &[0.5, 0.5]);
        assert_eq!(program, [0.5, 0.5, 1.0, 1.0]);
        let mut short = [2.0];
        apply_gain(&mut short, &[0.5, 0.25, 0.125]);
        assert_eq!(short, [1.0]);
        let mut ducked = [1.0; 4];
        duck(&mut ducked, &[1.0, 1.0], -20.0, 4.0, 0.0, 0.0, 1000.0);
        approx(ducked[0], 0.177_827_94);
        approx(ducked[1], 0.177_827_94);
        assert_eq!(ducked[2], 1.0);
        assert_eq!(ducked[3], 1.0);
    }

    #[test]
    fn a_ratio_not_above_one_is_the_identity() {
        for ratio in [1.0, 0.5, f32::NAN] {
            assert_eq!(key_gain(&[1.0; 20], -20.0, ratio, 1.0, 1.0, 1000.0), vec![1.0; 20]);
        }
    }

    #[test]
    fn a_threshold_above_the_peak_is_the_identity() {
        for threshold in [0.0, f32::NAN] {
            assert_eq!(key_gain(&[0.5; 50], threshold, 4.0, 1.0, 1.0, 1000.0), vec![1.0; 50]);
        }
    }

    #[test]
    fn non_finite_key_samples_give_no_reduction() {
        let g = key_gain(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY], -20.0, 4.0, 1.0, 1.0, 1000.0);
        assert_eq!(g, vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn an_infinite_ratio_is_a_hard_limit() {
        let g = key_gain(&[1.0; 200], -20.0, f32::INFINITY, 1.0, 1.0, 1000.0);
        assert!((g[199] - 0.1).abs() <= 1e-3, "{}", g[199]);
    }

    #[test]
    fn a_zero_negative_or_nan_time_reacts_at_once() {
        for attack in [0.0, -5.0, f32::NAN] {
            let g = key_gain(&[1.0], -20.0, 4.0, attack, 10.0, 1000.0);
            assert_eq!(g.len(), 1);
            approx(g[0], 0.177_827_94);
        }
    }

    #[test]
    fn duck_applies_the_key_gain_to_the_program() {
        let mut program = [1.0, 2.0, 4.0];
        duck(&mut program, &[1.0, 1.0, 1.0], -20.0, 4.0, 0.0, 0.0, 1000.0);
        approx(program[0], 0.177_827_94);
        approx(program[1], 0.355_655_9);
        approx(program[2], 0.711_311_8);
    }
}
```
In `crates/core/src/effects.rs` add exactly: `pub mod sidechain;` (after `mod gain;`).

## Expected values (computed beforehand by an independent Python reference, kept in the report folder)
Steady target for key 1.0 (0 dB), threshold -20, ratio 4: `10^(-20 * 0.75 / 20) = 0.1778279`. Attack step after n = 10 samples: `0.1778279 + (1 - 0.1778279) * e^-1 = 0.4802881`. Release step after 20 samples from steady: `1 - (1 - 0.1778279) * e^-1 = 0.6975398`.
| test | expected |
|---|---|
| a_silent_key_leaves_the_program_untouched | program equals its copy |
| a_loud_key_reduces_the_gain_by_the_ratio_math_at_steady_state | g[1999] = 0.17782794; key 0.5, threshold -12, ratio 2: g[399] = 0.7087858 (tolerance 1e-4) |
| the_attack_covers_63_percent... | g[9] = 0.48028814 with attack 10 ms, release 100 ms, rate 1000 |
| the_release_covers_63_percent... | g[199] = 0.17782794, g[219] = 0.6975398 (attack 1 ms, release 20 ms) |
| different_lengths_use_the_shorter | `[0.5, 0.5, 1.0, 1.0]`; `[1.0]`; ducked `[0.17782794, 0.17782794, 1.0, 1.0]` |
| a_ratio_not_above_one_is_the_identity | twenty gains of exactly 1.0 for ratio 1, 0.5, NaN |
| a_threshold_above_the_peak_is_the_identity | fifty gains of exactly 1.0 for threshold 0 and NaN |
| non_finite_key_samples_give_no_reduction | `[1.0, 1.0, 1.0]` |
| an_infinite_ratio_is_a_hard_limit | g[199] = 0.1 (tolerance 1e-3) |
| a_zero_negative_or_nan_time_reacts_at_once | g[0] = 0.17782794 for attack 0, -5, NaN |
| duck_applies_the_key_gain_to_the_program | `[0.17782794, 0.3556559, 0.7113118]` |
11 tests must exist and pass.

## Mutation checks (the driver runs them; you do not)
Each is made on the rustfmt-formatted file; the named test must then print `test result: FAILED`.
1. a_silent_key_leaves_the_program_untouched: change `const SILENCE_DB: f64 = -180.0;` to `const SILENCE_DB: f64 = 0.0;`.
2. a_loud_key_reduces_the_gain_by_the_ratio_math_at_steady_state: change `1.0 - 1.0 / f64::from(ratio)` to `1.0 / f64::from(ratio)`.
3. the_attack_covers_63_percent...: change `if target < gain { attack } else { release }` to `if target < gain { release } else { attack }`.
4. the_release_covers_63_percent...: change `(-1.0 / n).exp()` to `(-2.0 / n).exp()`.
5. different_lengths_use_the_shorter: change `.zip(gain)` to `.zip(gain.iter().chain(std::iter::repeat(&0.0)))`.
6. a_ratio_not_above_one_is_the_identity: change `if ratio > 1.0 {` to `if ratio > 0.0 {`.
7. a_threshold_above_the_peak_is_the_identity: change `over > 0.0 {` to `over > -100.0 {`.
8. non_finite_key_samples_give_no_reduction: change `over.is_finite() && over > 0.0` to `over > 0.0`.
9. an_infinite_ratio_is_a_hard_limit: change `1.0 - 1.0 / f64::from(ratio)` to `1.0 - 1.0 / f64::from(ratio.min(100.0))`.
10. a_zero_negative_or_nan_time_reacts_at_once: change `if n > 0.0 {` to `if n != 0.0 {`.
11. duck_applies_the_key_gain_to_the_program: change `apply_gain(program, &gain);` to `let _ = (&program, &gain);`.

## Commands (plain cargo; this is a local run)
```
cargo test -p wavcraft-core sidechain
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/effects/sidechain.rs crates/core/src/effects.rs
```
Stop when the 11 tests pass, clippy is clean and rustfmt was run. Do not push, do not open a PR.
