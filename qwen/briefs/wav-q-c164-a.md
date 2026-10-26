# C164-a: Strip EQ knobs mapped to the Parametric EQ

Refs #76 (the roadmap row has no issue; #76 is the Qwen roadmap issue that the other main-based Qwen PRs cite). Never write "Closes". Files you may touch: `crates/core/src/strip_eq.rs` and ONE new line in `crates/core/src/lib.rs`. Nothing else.
Local only: do not push. The harness checks, commits and opens the PR.

## Step 0: check the base first (before you edit anything)
Run, from the repo root:
- `ls crates/core/src/strip_eq.rs`
- `grep -n "strip_eq\|StripEq" crates/core/src/lib.rs`
- `grep -n "pub use eq::" crates/core/src/effects.rs`
`ls` must say the file does not exist, the first `grep` must find nothing, and the second must find the line that exports `ParametricEq`. If a check fails, STOP and report it; write nothing.

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit files.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` in production code (the crate denies them). Test code may use `.unwrap_or_else(|e| panic!("{e})")` exactly as written below. Use no `assert_eq!(x, true)`.
- Do not edit, rename, split or reformat any existing test or any existing line (except the one new line stated). Add no dependencies, no other files and no `debug_*.rs` scratch files.
- If a stated literal or a stated line cannot be matched, stop and report it. Do not change it.
- Type the file text below exactly (the design is fixed; your job is the typing). Run `rustfmt --edition 2024` on the one new file only, never the crate.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output (the compiler or test log) and quote it in your report.

## Why
A mixer strip should offer a quick 3-band EQ that drives the existing Parametric EQ. This job adds only the mapping from the strip's knobs to the effect's 10 settings and back; the processing is the existing effect. No UI.

## Behaviour
- Base choice: the queue named base PR #330, but that branch has no `effects` module at all (`git ls-tree origin/feat/ap-f104-mixer-panel crates/core/src` shows none), so the file would not compile there. The job is a pure mapping onto `effects::ParametricEq`, which `main` has, so its base is `main`.
- `StripEq { low_db, mid_db, mid_hz, mid_q, high_db }` has plain `f64` fields. `Default` is flat: 0, 0, 500, 1, 0 (the effect's defaults).
- `clamped()` clamps each value to the matching `ParametricEq` setting range (found by key: `low_gain_db`, `mid1_gain_db`, `mid1_freq`, `mid1_q`, `high_gain_db`); NaN becomes that setting's default.
- `to_params()` returns, in the effect's order, `[100, low, mid_hz, mid_db, mid_q, 2500, 0, 1, 8000, high]` after clamping (low shelf 100 Hz, bell 1 is the mid band, bell 2 left at 0 dB, high shelf 8 kHz), so it always passes `effects::validate`.
- `from_params(params)` returns `Some` only when `params` is exactly what `to_params` would write for the knobs it reads (so the wrong length, an out-of-range value, NaN, another shelf frequency or a non-zero bell 2 all give `None`).
- `is_flat()` is true when all three (clamped) gains are 0; the bell's frequency and Q do not matter.

## Where it goes and the code around it
- Base branch: `main`.
- Map: `maps/wavcraft/crates__core__src__effects.md`. The neighbouring files are `crates/core/src/effects.rs` and `crates/core/src/effects/eq.rs`.
- Registration: Put `pub mod strip_eq;` on its own line directly after `pub mod store;` in `crates/core/src/lib.rs`.
- One existing function from this base, as the pattern to follow (do not edit it):

```rust
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::frequency("low_freq", "Low shelf frequency", 20.0, 2000.0, 100.0),
            ParamDesc::db("low_gain_db", "Low shelf gain", -24.0, 24.0, 0.0),
            ParamDesc::frequency("mid1_freq", "Bell 1 frequency", 20.0, 20000.0, 500.0),
            ParamDesc::db("mid1_gain_db", "Bell 1 gain", -24.0, 24.0, 0.0),
            ParamDesc::number("mid1_q", "Bell 1 Q", 0.1, 20.0, 1.0, ""),
            ParamDesc::frequency("mid2_freq", "Bell 2 frequency", 20.0, 20000.0, 2500.0),
            ParamDesc::db("mid2_gain_db", "Bell 2 gain", -24.0, 24.0, 0.0),
            ParamDesc::number("mid2_q", "Bell 2 Q", 0.1, 20.0, 1.0, ""),
            ParamDesc::frequency("high_freq", "High shelf frequency", 1000.0, 20000.0, 8000.0),
            ParamDesc::db("high_gain_db", "High shelf gain", -24.0, 24.0, 0.0),
        ];
        P
    }
```

## The queue row, verbatim
| 12 | C164-a | C164 | T9 | Track EQ section on strip | easy | feat/ap-f104-mixer-panel (#330) | new -> #330 -> main | `crates/core/src/strip_eq.rs` | crates/core/src/lib.rs | needs-work |

### 12. C164-a (Track EQ section on strip), easy
- Base: feat/ap-f104-mixer-panel (#330). Chain: new -> #330 -> main. Depends-on: C152.
- Scope: Quick 3-band EQ setting for a mixer strip that drives the existing Parametric EQ: `struct StripEq {low_db, mid_db, mid_hz, mid_q, high_db}` (Default flat, values clamped to the effect ranges); `to_params(&self) -> Vec<f64>` in the order of the 10 Parametric EQ parameters (low shelf 100 Hz, bell 1 = the mid band, bell 2 left at 0 dB, high shelf 8 kHz); `from_params(&[f64]) -> Option<StripEq>`; `is_flat()`.
- Tests: default is flat and its params equal the effect defaults; to_params passes `effects::validate` for the EQ; round trip; clamping; from_params wrong length gives None.
- Where: new `crates/core/src/strip_eq.rs`; module line in `crates/core/src/lib.rs`; pattern function: `effects/eq.rs` Parametric EQ `P` table (:57) on main; map: crates__core__src__effects.md.
- Screen: needs-work: #330 text lists "no EQ section" (C164); main has the Parametric EQ effect.
- Note: Processing is the existing effect; this job only maps strip knobs to its parameters.

## The complete new file
Type it exactly. It is rustfmt-formatted; run `rustfmt --edition 2024` on the file afterwards anyway. There are exactly 7 new tests and all 7 must exist and pass.

```rust
//! A mixer strip's quick EQ (C164): three knobs that map onto the existing Parametric EQ.
//!
//! `StripEq` holds a low shelf gain (shelf at 100 Hz), one bell (gain, frequency and Q) and a
//! high shelf gain (shelf at 8 kHz). [`StripEq::to_params`] gives the 10 settings of
//! [`ParametricEq`] in its own order, with its second bell left at 0 dB. Nothing is processed
//! here; the effect does that.

use crate::effects::{Effect, ParametricEq};

/// The low shelf's frequency, in Hz.
const LOW_HZ: f64 = 100.0;
/// The high shelf's frequency, in Hz.
const HIGH_HZ: f64 = 8000.0;
/// The Parametric EQ's second bell, left as it is by default: frequency, gain (dB) and Q.
const BELL2_HZ: f64 = 2500.0;
const BELL2_DB: f64 = 0.0;
const BELL2_Q: f64 = 1.0;
/// How many settings the Parametric EQ takes.
const PARAM_COUNT: usize = 10;

/// The strip's EQ knobs. The fields are plain numbers; [`StripEq::clamped`] and
/// [`StripEq::to_params`] bring them into the effect's ranges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripEq {
    pub low_db: f64,
    pub mid_db: f64,
    pub mid_hz: f64,
    pub mid_q: f64,
    pub high_db: f64,
}

impl Default for StripEq {
    /// Flat: every gain 0 dB, the bell at 500 Hz with Q 1 (the effect's own defaults).
    fn default() -> Self {
        StripEq { low_db: 0.0, mid_db: 0.0, mid_hz: 500.0, mid_q: 1.0, high_db: 0.0 }
    }
}

/// `v` clamped to the range of the Parametric EQ setting `key`; NaN becomes its default.
fn limit(key: &str, v: f64) -> f64 {
    match ParametricEq.parameters().iter().find(|d| d.id == key) {
        Some(d) if v.is_nan() => d.default,
        Some(d) => v.clamp(d.min, d.max),
        None => v,
    }
}

impl StripEq {
    /// The same knobs with every value inside the effect's range (NaN becomes the default).
    pub fn clamped(&self) -> StripEq {
        StripEq {
            low_db: limit("low_gain_db", self.low_db),
            mid_db: limit("mid1_gain_db", self.mid_db),
            mid_hz: limit("mid1_freq", self.mid_hz),
            mid_q: limit("mid1_q", self.mid_q),
            high_db: limit("high_gain_db", self.high_db),
        }
    }

    /// The 10 Parametric EQ settings, in the effect's order: low shelf frequency and gain,
    /// bell 1 frequency, gain and Q, bell 2 frequency, gain and Q, high shelf frequency and gain.
    /// The knobs are clamped first, so the result always passes `effects::validate`.
    pub fn to_params(&self) -> Vec<f64> {
        let s = self.clamped();
        vec![LOW_HZ, s.low_db, s.mid_hz, s.mid_db, s.mid_q, BELL2_HZ, BELL2_DB, BELL2_Q, HIGH_HZ, s.high_db]
    }

    /// The knobs behind a Parametric EQ setting list, or `None` when the list is not one this
    /// strip can write: wrong length, a value out of range or NaN, or a shelf frequency or bell 2
    /// setting other than the ones `to_params` writes.
    pub fn from_params(params: &[f64]) -> Option<StripEq> {
        if params.len() != PARAM_COUNT {
            return None;
        }
        let strip = StripEq { low_db: params[1], mid_db: params[3], mid_hz: params[2], mid_q: params[4], high_db: params[9] };
        (strip.to_params() == params).then_some(strip)
    }

    /// Whether all three gains are 0 dB (the bell's frequency and Q do not matter then).
    pub fn is_flat(&self) -> bool {
        let s = self.clamped();
        s.low_db == 0.0 && s.mid_db == 0.0 && s.high_db == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::validate;

    fn knobs() -> StripEq {
        StripEq { low_db: 3.0, mid_db: -4.5, mid_hz: 1200.0, mid_q: 2.5, high_db: 6.0 }
    }

    #[test]
    fn default_is_flat_and_its_params_equal_the_effect_defaults() {
        let strip = StripEq::default();
        assert!(strip.is_flat());
        let defaults: Vec<f64> = ParametricEq.parameters().iter().map(|d| d.default).collect();
        assert_eq!(strip.to_params(), defaults);
    }

    #[test]
    fn to_params_passes_the_effect_validation() {
        assert_eq!(validate(&ParametricEq, &knobs().to_params()), Ok(()));
        assert_eq!(knobs().to_params(), vec![100.0, 3.0, 1200.0, -4.5, 2.5, 2500.0, 0.0, 1.0, 8000.0, 6.0]);
    }

    #[test]
    fn params_round_trip_through_from_params() {
        assert_eq!(StripEq::from_params(&knobs().to_params()), Some(knobs()));
        assert_eq!(StripEq::from_params(&StripEq::default().to_params()), Some(StripEq::default()));
    }

    #[test]
    fn values_are_clamped_to_the_effect_ranges() {
        let wild = StripEq { low_db: 99.0, mid_db: -99.0, mid_hz: 5.0, mid_q: 100.0, high_db: f64::NAN };
        assert_eq!(wild.to_params(), vec![100.0, 24.0, 20.0, -24.0, 20.0, 2500.0, 0.0, 1.0, 8000.0, 0.0]);
        let other = StripEq { low_db: -99.0, mid_db: 99.0, mid_hz: 99999.0, mid_q: 0.01, high_db: 30.0 };
        assert_eq!(other.to_params(), vec![100.0, -24.0, 20000.0, 24.0, 0.1, 2500.0, 0.0, 1.0, 8000.0, 24.0]);
        assert_eq!(validate(&ParametricEq, &wild.to_params()), Ok(()));
        assert_eq!(wild.clamped().mid_q, 20.0);
    }

    #[test]
    fn from_params_of_the_wrong_length_is_none() {
        let params = knobs().to_params();
        assert_eq!(StripEq::from_params(&params[..9]), None);
        let mut longer = params.clone();
        longer.push(0.0);
        assert_eq!(StripEq::from_params(&longer), None);
        assert_eq!(StripEq::from_params(&[]), None);
    }

    #[test]
    fn from_params_refuses_settings_the_strip_cannot_write() {
        let mut bell2 = knobs().to_params();
        bell2[6] = 3.0;
        assert_eq!(StripEq::from_params(&bell2), None);
        let mut shelf = knobs().to_params();
        shelf[0] = 200.0;
        assert_eq!(StripEq::from_params(&shelf), None);
        let mut out_of_range = knobs().to_params();
        out_of_range[1] = 40.0;
        assert_eq!(StripEq::from_params(&out_of_range), None);
    }

    #[test]
    fn is_flat_looks_at_the_three_gains_only() {
        let flat_moved = StripEq { mid_hz: 3000.0, mid_q: 5.0, ..StripEq::default() };
        assert!(flat_moved.is_flat());
        assert!(!StripEq { low_db: 1.0, ..StripEq::default() }.is_flat());
        assert!(!StripEq { mid_db: -1.0, ..StripEq::default() }.is_flat());
        assert!(!StripEq { high_db: 0.5, ..StripEq::default() }.is_flat());
    }
}
```

## Expected values (computed beforehand by an independent Python reference or by hand where trivial)
| test | expected |
|---|---|
| default_is_flat_and_its_params_equal_the_effect_defaults | flat; `to_params()` equals the `default` of every `ParametricEq` setting (`[100,0,500,0,1,2500,0,1,8000,0]`) |
| to_params_passes_the_effect_validation | knobs (3, -4.5, 1200 Hz, Q 2.5, 6): `validate` is `Ok(())` and params are `[100,3,1200,-4.5,2.5,2500,0,1,8000,6]` |
| params_round_trip_through_from_params | `from_params(to_params(k))` is `Some(k)`, also for the default |
| values_are_clamped_to_the_effect_ranges | (99,-99,5 Hz,Q 100,NaN) gives `[100,24,20,-24,20,2500,0,1,8000,0]`; (-99,99,99999,0.01,30) gives `[100,-24,20000,24,0.1,2500,0,1,8000,24]` |
| from_params_of_the_wrong_length_is_none | lengths 9, 11 and 0 give `None` |
| from_params_refuses_settings_the_strip_cannot_write | bell 2 gain 3, low shelf 200 Hz, low gain 40 each give `None` |
| is_flat_looks_at_the_three_gains_only | moving mid_hz and mid_q stays flat; low 1, mid -1 or high 0.5 is not flat |
All are exact short decimals (0.1 is the f64 literal the effect itself uses); no python needed.

## Mutation checks (the harness runs these, not you)
Each change is applied to the production line named, one at a time, and the named test must FAIL with a real `test result: FAILED` line; the file is restored afterwards.
- `crates/core/src/strip_eq.rs`: change `mid_hz: 500.0,` to `mid_hz: 600.0,`: `default_is_flat_and_its_params_equal_the_effect_defaults` must FAIL.
- `crates/core/src/strip_eq.rs`: change `const HIGH_HZ: f64 = 8000.0;` to `const HIGH_HZ: f64 = 800.0;`: `to_params_passes_the_effect_validation` must FAIL.
- `crates/core/src/strip_eq.rs`: change `s.mid_hz, s.mid_db, s.mid_q` to `s.mid_db, s.mid_hz, s.mid_q`: `params_round_trip_through_from_params` must FAIL.
- `crates/core/src/strip_eq.rs`: change `Some(d) => v.clamp(d.min, d.max),` to `Some(_) => v,`: `values_are_clamped_to_the_effect_ranges` must FAIL.
- `crates/core/src/strip_eq.rs`: change `if params.len() != PARAM_COUNT {` to `if params.len() > PARAM_COUNT {`: `from_params_of_the_wrong_length_is_none` must FAIL.
- `crates/core/src/strip_eq.rs`: change `(strip.to_params() == params).then_some(strip)` to `Some(strip)`: `from_params_refuses_settings_the_strip_cannot_write` must FAIL.
- `crates/core/src/strip_eq.rs`: change `s.low_db == 0.0 && s.mid_db == 0.0 && s.high_db == 0.0` to `s.low_db == 0.0 && s.mid_db == 0.0`: `is_flat_looks_at_the_three_gains_only` must FAIL.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core strip_eq
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/strip_eq.rs
```
rustfmt that one file only. Stop when the strip_eq tests pass (7 of 7), clippy is clean and rustfmt has been run.
