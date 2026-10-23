# B030-a: Dynamics graph: curve, reduction and unity points

Refs #312 (the roadmap row has no issue; #312 is the issue of the base PR #322). Never write "Closes". Files you may touch: `crates/core/src/effects/dynamics_graph.rs` and ONE new line in `crates/core/src/effects.rs`. Nothing else.
Local only: do not push. The harness checks, commits and opens the PR.

## Step 0: check the base first (before you edit anything)
Run, from the repo root:
- `ls crates/core/src/effects/dynamics_graph.rs`
- `grep -n "dynamics_graph" crates/core/src/effects.rs`
- `grep -n "pub fn static_curve\|pub struct Dynamics" crates/core/src/effects/dynamics.rs`
`ls` must say the file does not exist, the first `grep` must find nothing, and the second must find both lines. If a check fails, STOP and report it; write nothing.

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit files.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` in production code (the crate denies them). Test code may use `.unwrap_or_else(|e| panic!("{e})")` exactly as written below. Use no `assert_eq!(x, true)`.
- Do not edit, rename, split or reformat any existing test or any existing line (except the one new line stated). Add no dependencies, no other files and no `debug_*.rs` scratch files.
- If a stated literal or a stated line cannot be matched, stop and report it. Do not change it.
- Type the file text below exactly (the design is fixed; your job is the typing). Run `rustfmt --edition 2024` on the one new file only, never the crate.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output (the compiler or test log) and quote it in your report.

## Why
The Dynamics effect has the transfer-curve maths (`static_curve`) but nothing that samples it for a graph. This job adds the sampled points: the curve, the gain change, and the unity line. No drawing, no UI.

## Behaviour
- `curve_points(params, lo_db, hi_db, n)` returns `n` points `(input dB, static_curve output dB)` from `lo_db` to `hi_db`; both ends are included when `n >= 2`, only `lo_db` when `n == 1`. It returns an empty Vec for `n == 0`, for `lo_db >= hi_db`, for a NaN or infinite level, and for a `params` whose length is not the Dynamics parameter count (`static_curve` indexes the slice, so this guard prevents a panic).
- `reduction_points` is the same with the second value `static_curve(..) - x` (the gain change in dB).
- `unity_line(lo, hi)` returns `[(lo, lo), (hi, hi)]`.
- The parameter order is the one of `Dynamics.parameters()`; the defaults are gate -60, expand 2, comp -20, ratio 3, knee 6, limit -1, makeup 0. The tests read them from `parameters()` and set values by key, never by a hard-coded index.

## Where it goes and the code around it
- Base branch: `feat/b029-dynamics`.
- Map: `maps/wavcraft/crates__core__src__effects.md` (it describes main; on this base read `crates/core/src/effects.rs` and `crates/core/src/effects/dynamics.rs` themselves). The neighbouring file is `crates/core/src/effects/dynamics.rs`.
- Registration: Put `pub mod dynamics_graph;` on its own line directly after `pub mod dynamics;` (about line 18) in `crates/core/src/effects.rs`.
- One existing function from this base, as the pattern to follow (do not edit it):

```rust
/// The output level in dB for an input level in dB under `params` (in the order of
/// [`Effect::parameters`]), makeup included. The limiter comes last, so the output never
/// exceeds `limit_db` whatever the makeup. A later curve editor draws this.
pub fn static_curve(params: &[f64], in_db: f64) -> f64 {
    (curve_core(params, in_db) + params[MAKEUP]).min(params[LIMIT])
}
```

## The queue row, verbatim
| 10 | B030-a | B030 | T5 | Dynamics graph editor (transfer curve) | easy | feat/b029-dynamics (#322) | new -> #322 -> #153 -> main (#153 sits on closed #93/#106) | `crates/core/src/effects/dynamics_graph.rs` | crates/core/src/effects.rs | partly |

### 10. B030-a (Dynamics graph editor (transfer curve)), easy
- Base: feat/b029-dynamics (#322). Chain: new -> #322 -> #153 -> main (#153 sits on closed #93/#106). Depends-on: B029.
- Scope: Curve data for the transfer-curve graph: `curve_points(params: &[f64], lo_db: f64, hi_db: f64, n: usize) -> Vec<(f64, f64)>` (input dB, output dB, using `dynamics::static_curve`); `reduction_points(...)` (input dB, gain change dB); `unity_line(lo_db, hi_db) -> [(f64,f64);2]`.
- Tests: default params give the unity line below the thresholds; compressor point matches the formula by hand; limiter flat at ceiling; n = 0/1/2; lo >= hi; reduction is <= 0 for a compressor.
- Where: new `crates/core/src/effects/dynamics_graph.rs`; module line in `crates/core/src/effects.rs`; pattern function: `effects/dynamics.rs::static_curve` (:70) on #322; map: crates__core__src__effects.md.
- Screen: partly: #322 has `static_curve(params, in_db)` (the maths) and says there is no curve editor (B030). The sampled data and the drag mapping (B030-b) are not there.

## The complete new file
Type it exactly. It is rustfmt-formatted; run `rustfmt --edition 2024` on the file afterwards anyway. There are exactly 8 new tests and all 8 must exist and pass.

```rust
//! The data behind the Dynamics transfer-curve graph (B030): sampled points for the graph to
//! draw. No drawing and no UI here.
//!
//! A point is `(input level in dB, value in dB)`. The output levels come from [`static_curve`]
//! in `effects/dynamics.rs`, so the graph always shows what the effect does.

use super::Effect;
use super::dynamics::{Dynamics, static_curve};

/// The `n` input levels from `lo_db` to `hi_db`, both ends included when `n` is 2 or more.
/// Empty when `n` is 0, when a level is not finite, or when `lo_db >= hi_db`; just `lo_db`
/// when `n` is 1.
fn levels(lo_db: f64, hi_db: f64, n: usize) -> Vec<f64> {
    if n == 0 || !lo_db.is_finite() || !hi_db.is_finite() || lo_db >= hi_db {
        return Vec::new();
    }
    if n == 1 {
        return vec![lo_db];
    }
    let last = (n - 1) as f64;
    (0..n).map(|i| if i + 1 == n { hi_db } else { lo_db + (hi_db - lo_db) * (i as f64) / last }).collect()
}

/// Whether `params` holds one value per Dynamics parameter ([`static_curve`] indexes them).
fn fits(params: &[f64]) -> bool {
    params.len() == Dynamics.parameters().len()
}

/// `n` points of the transfer curve: (input dB, output dB), from `lo_db` to `hi_db`.
/// Empty for a `params` of the wrong length and for the cases `levels` refuses.
pub fn curve_points(params: &[f64], lo_db: f64, hi_db: f64, n: usize) -> Vec<(f64, f64)> {
    if !fits(params) {
        return Vec::new();
    }
    levels(lo_db, hi_db, n).into_iter().map(|x| (x, static_curve(params, x))).collect()
}

/// `n` points of the gain change: (input dB, output dB minus input dB). Zero or negative
/// for a compressor, negative below the gate threshold. Empty in the same cases as
/// [`curve_points`].
pub fn reduction_points(params: &[f64], lo_db: f64, hi_db: f64, n: usize) -> Vec<(f64, f64)> {
    if !fits(params) {
        return Vec::new();
    }
    levels(lo_db, hi_db, n).into_iter().map(|x| (x, static_curve(params, x) - x)).collect()
}

/// The straight line where output equals input, from `lo_db` to `hi_db`.
pub fn unity_line(lo_db: f64, hi_db: f64) -> [(f64, f64); 2] {
    [(lo_db, lo_db), (hi_db, hi_db)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<f64> {
        Dynamics.parameters().iter().map(|d| d.default).collect()
    }

    fn set(params: &mut [f64], key: &str, value: f64) {
        if let Some(i) = Dynamics.parameters().iter().position(|d| d.id == key) {
            params[i] = value;
        }
    }

    fn assert_close(got: &[(f64, f64)], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6, "got {g:?}, wanted {w:?}");
        }
    }

    #[test]
    fn default_params_give_the_unity_line_below_the_thresholds() {
        let pts = curve_points(&defaults(), -50.0, -30.0, 5);
        assert_eq!(pts, vec![(-50.0, -50.0), (-45.0, -45.0), (-40.0, -40.0), (-35.0, -35.0), (-30.0, -30.0)]);
    }

    #[test]
    fn compressor_points_match_the_formula_by_hand() {
        let pts = curve_points(&defaults(), -20.0, 0.0, 3);
        assert_close(&pts, &[(-20.0, -20.5), (-10.0, -16.666667), (0.0, -13.333333)]);
    }

    #[test]
    fn limiter_is_flat_at_the_ceiling() {
        let mut p = defaults();
        set(&mut p, "comp_ratio", 1.0);
        set(&mut p, "makeup_db", 24.0);
        let pts = curve_points(&p, -10.0, 0.0, 3);
        assert_eq!(pts, vec![(-10.0, -1.0), (-5.0, -1.0), (0.0, -1.0)]);
    }

    #[test]
    fn n_of_zero_one_and_two() {
        let p = defaults();
        assert!(curve_points(&p, -80.0, 0.0, 0).is_empty());
        assert_eq!(curve_points(&p, -80.0, 0.0, 1), vec![(-80.0, -100.0)]);
        assert_close(&curve_points(&p, -80.0, 0.0, 2), &[(-80.0, -100.0), (0.0, -13.333333)]);
    }

    #[test]
    fn lo_not_below_hi_gives_no_points() {
        let p = defaults();
        assert!(curve_points(&p, -20.0, -20.0, 3).is_empty());
        assert!(curve_points(&p, 0.0, -60.0, 3).is_empty());
        assert!(curve_points(&p, f64::NAN, 0.0, 3).is_empty());
        assert!(reduction_points(&p, -20.0, -20.0, 3).is_empty());
        assert!(reduction_points(&p, 0.0, f64::INFINITY, 3).is_empty());
    }

    #[test]
    fn reduction_is_at_most_zero_for_a_compressor() {
        let pts = reduction_points(&defaults(), -20.0, 0.0, 3);
        assert_close(&pts, &[(-20.0, -0.5), (-10.0, -6.666667), (0.0, -13.333333)]);
        assert!(pts.iter().all(|p| p.1 <= 0.0));
    }

    #[test]
    fn unity_line_joins_the_two_ends() {
        assert_eq!(unity_line(-60.0, 0.0), [(-60.0, -60.0), (0.0, 0.0)]);
    }

    #[test]
    fn wrong_parameter_count_gives_no_points() {
        assert!(curve_points(&[0.0, 1.0, 2.0], -60.0, 0.0, 3).is_empty());
        assert!(reduction_points(&[], -60.0, 0.0, 3).is_empty());
    }
}
```

## Expected values (computed beforehand by an independent Python reference or by hand where trivial)
Python reference: `qb1/b030-expected.py` (a mirror of `static_curve`, run beforehand). 8-digit literals below are rounded; the tests compare within 1e-6.
| test | expected |
|---|---|
| default_params_give_the_unity_line_below_the_thresholds | lo -50, hi -30, n 5: `[(-50,-50),(-45,-45),(-40,-40),(-35,-35),(-30,-30)]` exactly |
| compressor_points_match_the_formula_by_hand | n 3 over -20..0: `(-20,-20.5) (-10,-16.666667) (0,-13.333333)` |
| limiter_is_flat_at_the_ceiling | comp_ratio 1, makeup 24, -10..0 n 3: y is exactly -1 each |
| n_of_zero_one_and_two | n 0 empty; n 1 `[(-80,-100)]`; n 2 `[(-80,-100),(0,-13.333333)]` |
| lo_not_below_hi_gives_no_points | empty for lo == hi, lo > hi, NaN lo, infinite hi (both functions) |
| reduction_is_at_most_zero_for_a_compressor | `(-20,-0.5) (-10,-6.666667) (0,-13.333333)`, all y <= 0 |
| unity_line_joins_the_two_ends | `[(-60,-60),(0,0)]` |
| wrong_parameter_count_gives_no_points | empty for 3 values and for 0 values |

## Mutation checks (the harness runs these, not you)
Each change is applied to the production line named, one at a time, and the named test must FAIL with a real `test result: FAILED` line; the file is restored afterwards.
- `crates/core/src/effects/dynamics_graph.rs`: change `(i as f64) / last` to `(i as f64) / (last + 1.0)`: `default_params_give_the_unity_line_below_the_thresholds` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `|x| (x, static_curve(params, x))` to `|x| (x, x)`: `compressor_points_match_the_formula_by_hand` must FAIL.
- `crates/core/src/effects/dynamics.rs`: change `.min(params[LIMIT])` to `.min(params[LIMIT] + 3.0)`: `limiter_is_flat_at_the_ceiling` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `if n == 1 {` to `if n == 2 {`: `n_of_zero_one_and_two` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `|| lo_db >= hi_db {` to `|| lo_db > hi_db {`: `lo_not_below_hi_gives_no_points` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `static_curve(params, x) - x` to `static_curve(params, x)`: `reduction_is_at_most_zero_for_a_compressor` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `[(lo_db, lo_db), (hi_db, hi_db)]` to `[(lo_db, hi_db), (hi_db, hi_db)]`: `unity_line_joins_the_two_ends` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `params.len() == Dynamics.parameters().len()` to `params.len() > 0`: `wrong_parameter_count_gives_no_points` must FAIL.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core dynamics_graph
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/effects/dynamics_graph.rs
```
rustfmt that one file only. Stop when the dynamics_graph tests pass (8 of 8), clippy is clean and rustfmt has been run.
