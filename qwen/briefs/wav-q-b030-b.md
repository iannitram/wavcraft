# B030-b: Dynamics graph: drag the nodes

Refs #312 (the roadmap row has no issue; #312 is the issue of the base PR #322). Never write "Closes". Files you may touch: `crates/core/src/effects/dynamics_graph.rs` (it already exists; add to it). Nothing else.
Local only: do not push. The harness checks, commits and opens the PR.

## Step 0: check the base first (before you edit anything)
Run, from the repo root:
- `grep -n "pub fn unity_line\|pub fn curve_points\|^use super::Effect;\|^use super::dynamics::{Dynamics, static_curve};" crates/core/src/effects/dynamics_graph.rs`
- `grep -n "pub fn apply_drag\|pub enum Node\|pub fn node_positions" crates/core/src/effects/dynamics_graph.rs`
- `grep -n "pub mod dynamics_graph;" crates/core/src/effects.rs`
The first `grep` must find all four lines (the B030-a file is there), the second must find nothing, the third must find the `pub mod` line. If `apply_drag`, `Node` or `node_positions` is already there, or B030-a's file is missing, STOP and report; write nothing. If a check fails, STOP and report it; write nothing.

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit files.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` in production code (the crate denies them). Test code may use `.unwrap_or_else(|e| panic!("{e})")` exactly as written below. Use no `assert_eq!(x, true)`.
- Do not edit, rename, split or reformat any existing test or any existing line (except the one new line stated). Add no dependencies, no other files and no `debug_*.rs` scratch files.
- If a stated literal or a stated line cannot be matched, stop and report it. Do not change it.
- Type the file text below exactly (the design is fixed; your job is the typing). Run `rustfmt --edition 2024` on the one new file only, never the crate.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output (the compiler or test log) and quote it in your report.

## Why
B030-a samples the curve for the graph. This job adds the editing side: the four draggable handles, what a drag does to the settings, and where each handle sits. No drawing, no UI.

## Behaviour
- Handles: `GateThreshold`, `CompThreshold`, `Ratio`, `Limiter`. All parameters are found by key (`gate_db`, `comp_db`, `comp_ratio`, `limit_db`) through `Dynamics.parameters()`; every stored value is clamped to that parameter's min and max.
- `apply_drag`: the two thresholds take `in_db`; `Ratio` takes `(in_db - comp) / (out_db - comp)`, the largest allowed ratio when `out_db <= comp`, and changes nothing when `in_db <= comp`; `Limiter` takes `out_db`. Non-finite numbers the drag needs, or a `params` of the wrong length, change nothing.
- `node_positions`: gate and comp handles at (threshold, `static_curve` of it); the Ratio handle at `comp + RATIO_HANDLE_DB` (12 dB above comp) on the curve; the Limiter handle at input 0 dB on the ceiling `limit_db`.
- Decisions made here (the queue left them open): the Ratio handle's offset is 12 dB; the Limiter handle sits at 0 dB in, so the queue's "dragged point lies on `static_curve`" holds for the gate, comp and ratio handles, and for the limiter the ceiling check is `static_curve(0 dB)`.

## Where it goes and the code around it
- Base branch: `qwen/wav-q-b030-a` (the branch of job B030-a: its file must be there).
- Map: `maps/wavcraft/crates__core__src__effects.md` (describes main; read `crates/core/src/effects/dynamics.rs` on this base). The file already exists (job B030-a); this job adds to it.
- Registration: No `pub mod` line is needed: the file is already registered by job B030-a.
- One existing function from this base, as the pattern to follow (do not edit it):

```rust
/// The output level in dB for an input level in dB under `params` (in the order of
/// [`Effect::parameters`]), makeup included. The limiter comes last, so the output never
/// exceeds `limit_db` whatever the makeup. A later curve editor draws this.
pub fn static_curve(params: &[f64], in_db: f64) -> f64 {
    (curve_core(params, in_db) + params[MAKEUP]).min(params[LIMIT])
}
```

```rust
const GATE: usize = 0;
const EXPAND: usize = 1;
const COMP: usize = 2;
const RATIO: usize = 3;
const KNEE: usize = 4;
const LIMIT: usize = 5;
const ATTACK: usize = 6;
const RELEASE: usize = 7;
const LOOKAHEAD: usize = 8;
const MAKEUP: usize = 9;
const LINK: usize = 10;
```

## The queue row, verbatim
| 20 | B030-b | B030 | T5 | Dynamics graph editor (transfer curve) | needs-care | feat/b029-dynamics (#322) | new -> #322 -> #153 -> main (#153 sits on closed #93/#106) | `crates/core/src/effects/dynamics_graph.rs (same file as B030-a)` | none | needs-work |

### 20. B030-b (Dynamics graph editor (transfer curve)), needs-care
- Base: feat/b029-dynamics (#322). Chain: new -> #322 -> #153 -> main (#153 sits on closed #93/#106). Depends-on: B029.
- Scope: Dragging graph nodes to edit settings: `enum Node {GateThreshold, CompThreshold, Ratio, Limiter}`; `apply_drag(params: &mut [f64], node: Node, in_db: f64, out_db: f64)` (thresholds take in_db; Ratio takes (in-comp)/(out-comp) clamped to the parameter range; Limiter takes out_db; every result clamped to the `ParamDesc` range); `node_positions(params) -> [(Node, f64, f64); 4]`.
- Tests: after apply_drag the dragged point lies on `static_curve` within 0.05 dB; clamps; positions then drag is stable; look params up by key through `Dynamics.parameters()`, not hard-coded indexes.
- Where: new `crates/core/src/effects/dynamics_graph.rs (same file as B030-a)`; module line in `none`; pattern function: `effects/dynamics.rs::static_curve` and the `GATE`..`LINK` index consts (:28) on #322; map: crates__core__src__effects.md.
- Screen: needs-work (after B030-a).
- Note: Must follow B030-a.

## The two blocks to add
Type it exactly. It is rustfmt-formatted; run `rustfmt --edition 2024` on the file afterwards anyway. There are exactly 9 new tests and all 9 must exist and pass.

### Production block: insert it directly ABOVE the line `#[cfg(test)]` (leave one blank line between the blocks)
```rust
/// The four handles of the transfer-curve graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    GateThreshold,
    CompThreshold,
    Ratio,
    Limiter,
}

/// How far above the compressor threshold (dB of input) the Ratio handle sits.
pub const RATIO_HANDLE_DB: f64 = 12.0;

/// The index of the parameter with this key, found through `Dynamics.parameters()`.
fn index_of(key: &str) -> Option<usize> {
    Dynamics.parameters().iter().position(|d| d.id == key)
}

fn value_of(params: &[f64], key: &str) -> Option<f64> {
    index_of(key).and_then(|i| params.get(i)).copied()
}

/// Store `value` in the parameter `key`, clamped to that parameter's range.
fn set_clamped(params: &mut [f64], key: &str, value: f64) {
    let Some(i) = index_of(key) else { return };
    let Some(desc) = Dynamics.parameters().get(i) else { return };
    if let Some(slot) = params.get_mut(i) {
        *slot = value.clamp(desc.min, desc.max);
    }
}

/// Apply a drag of `node` to the graph point (`in_db`, `out_db`) to `params`.
/// - `GateThreshold` and `CompThreshold` take `in_db`.
/// - `Ratio` takes `(in_db - comp) / (out_db - comp)` (comp is the compressor threshold);
///   when `out_db` is not above comp the ratio is the largest allowed, and when `in_db` is
///   not above comp nothing changes.
/// - `Limiter` takes `out_db`.
///
/// Every stored value is clamped to its parameter's range. A non-finite number the drag needs,
/// or a `params` of the wrong length, changes nothing.
pub fn apply_drag(params: &mut [f64], node: Node, in_db: f64, out_db: f64) {
    if params.len() != Dynamics.parameters().len() {
        return;
    }
    match node {
        Node::GateThreshold => {
            if in_db.is_finite() {
                set_clamped(params, "gate_db", in_db);
            }
        }
        Node::CompThreshold => {
            if in_db.is_finite() {
                set_clamped(params, "comp_db", in_db);
            }
        }
        Node::Ratio => {
            let Some(comp) = value_of(params, "comp_db") else { return };
            if !in_db.is_finite() || !out_db.is_finite() || in_db <= comp {
                return;
            }
            let rise = out_db - comp;
            let ratio = if rise <= 0.0 { f64::INFINITY } else { (in_db - comp) / rise };
            set_clamped(params, "comp_ratio", ratio);
        }
        Node::Limiter => {
            if out_db.is_finite() {
                set_clamped(params, "limit_db", out_db);
            }
        }
    }
}

/// Where each handle sits as (node, input dB, output dB): the two thresholds and the Ratio
/// handle (at `comp + RATIO_HANDLE_DB`) lie on [`static_curve`]; the Limiter handle sits at the
/// right edge (0 dB in) on the ceiling `limit_db`. A `params` of the wrong length gives the
/// positions of the default settings.
pub fn node_positions(params: &[f64]) -> [(Node, f64, f64); 4] {
    let defaults: Vec<f64> = Dynamics.parameters().iter().map(|d| d.default).collect();
    let p: &[f64] = if params.len() == defaults.len() { params } else { &defaults };
    let gate = value_of(p, "gate_db").unwrap_or(0.0);
    let comp = value_of(p, "comp_db").unwrap_or(0.0);
    let limit = value_of(p, "limit_db").unwrap_or(0.0);
    let ratio_in = comp + RATIO_HANDLE_DB;
    [
        (Node::GateThreshold, gate, static_curve(p, gate)),
        (Node::CompThreshold, comp, static_curve(p, comp)),
        (Node::Ratio, ratio_in, static_curve(p, ratio_in)),
        (Node::Limiter, 0.0, limit),
    ]
}
```

### Test functions: insert them at the END of the existing `mod tests`, after its last test and before the `}` that closes the module (indent every line by 4 spaces; they use `Dynamics`, `static_curve` and `Effect` already imported at the top of the file by B030-a, and `use super::*` in the tests module)
```rust
fn base_params() -> Vec<f64> {
    Dynamics.parameters().iter().map(|d| d.default).collect()
}

fn value(params: &[f64], key: &str) -> f64 {
    Dynamics.parameters().iter().position(|d| d.id == key).and_then(|i| params.get(i)).copied().unwrap_or(f64::NAN)
}

fn with_value(mut params: Vec<f64>, key: &str, v: f64) -> Vec<f64> {
    if let Some(i) = Dynamics.parameters().iter().position(|d| d.id == key) {
        params[i] = v;
    }
    params
}

#[test]
fn gate_drag_sets_the_threshold_and_the_point_lies_on_the_curve() {
    let mut p = base_params();
    apply_drag(&mut p, Node::GateThreshold, -70.0, 0.0);
    assert_eq!(value(&p, "gate_db"), -70.0);
    assert_eq!(p, with_value(base_params(), "gate_db", -70.0));
    let (node, x, y) = node_positions(&p)[0];
    assert_eq!((node, x), (Node::GateThreshold, -70.0));
    assert!((y - static_curve(&p, -70.0)).abs() <= 0.05);
    assert!((y - -70.0).abs() <= 0.05);
}

#[test]
fn comp_drag_sets_the_threshold_and_the_point_lies_on_the_curve() {
    let mut p = base_params();
    apply_drag(&mut p, Node::CompThreshold, -30.0, 0.0);
    assert_eq!(value(&p, "comp_db"), -30.0);
    assert_eq!(p, with_value(base_params(), "comp_db", -30.0));
    let (node, x, y) = node_positions(&p)[1];
    assert_eq!((node, x), (Node::CompThreshold, -30.0));
    assert!((y - static_curve(&p, -30.0)).abs() <= 0.05);
    assert!((y - -30.5).abs() < 1e-6);
}

#[test]
fn ratio_drag_puts_the_point_on_the_curve() {
    let mut p = base_params();
    apply_drag(&mut p, Node::Ratio, -10.0, -18.0);
    assert!((value(&p, "comp_ratio") - 5.0).abs() < 1e-9);
    assert!((static_curve(&p, -10.0) - -18.0).abs() <= 0.05);
}

#[test]
fn ratio_is_clamped_to_the_parameter_range() {
    let ratio_after = |in_db: f64, out_db: f64| {
        let mut p = base_params();
        apply_drag(&mut p, Node::Ratio, in_db, out_db);
        value(&p, "comp_ratio")
    };
    assert_eq!(ratio_after(-10.0, -5.0), 1.0);
    assert_eq!(ratio_after(-10.0, -20.0), 20.0);
    assert_eq!(ratio_after(-10.0, -40.0), 20.0);
    assert_eq!(ratio_after(-10.0, -19.9), 20.0);
    assert!((ratio_after(-10.0, -12.0) - 1.25).abs() < 1e-9);
}

#[test]
fn limiter_drag_sets_the_ceiling() {
    let mut p = with_value(base_params(), "makeup_db", 24.0);
    apply_drag(&mut p, Node::Limiter, -30.0, -6.0);
    assert_eq!(value(&p, "limit_db"), -6.0);
    assert_eq!(static_curve(&p, 0.0), -6.0);
}

#[test]
fn drags_are_clamped_to_the_parameter_ranges() {
    let after = |node: Node, in_db: f64, out_db: f64, key: &str| {
        let mut p = base_params();
        apply_drag(&mut p, node, in_db, out_db);
        value(&p, key)
    };
    assert_eq!(after(Node::GateThreshold, -200.0, 0.0, "gate_db"), -80.0);
    assert_eq!(after(Node::GateThreshold, 5.0, 0.0, "gate_db"), 0.0);
    assert_eq!(after(Node::CompThreshold, -100.0, 0.0, "comp_db"), -60.0);
    assert_eq!(after(Node::CompThreshold, 5.0, 0.0, "comp_db"), 0.0);
    assert_eq!(after(Node::Limiter, 0.0, -40.0, "limit_db"), -24.0);
    assert_eq!(after(Node::Limiter, 0.0, 3.0, "limit_db"), 0.0);
}

#[test]
fn bad_numbers_and_wrong_length_leave_the_settings_alone() {
    let mut p = base_params();
    apply_drag(&mut p, Node::GateThreshold, f64::NAN, 0.0);
    apply_drag(&mut p, Node::CompThreshold, f64::INFINITY, 0.0);
    apply_drag(&mut p, Node::Limiter, 0.0, f64::NAN);
    apply_drag(&mut p, Node::Ratio, -25.0, -30.0);
    apply_drag(&mut p, Node::Ratio, f64::NAN, -10.0);
    assert_eq!(p, base_params());
    let mut short = vec![1.0, 2.0, 3.0];
    apply_drag(&mut short, Node::GateThreshold, -70.0, 0.0);
    assert_eq!(short, vec![1.0, 2.0, 3.0]);
}

#[test]
fn positions_of_the_defaults_and_drag_back_is_stable() {
    let p = base_params();
    let pos = node_positions(&p);
    let want = [(Node::GateThreshold, -60.0, -60.0), (Node::CompThreshold, -20.0, -20.5), (Node::Ratio, -8.0, -16.0), (Node::Limiter, 0.0, -1.0)];
    for (got, w) in pos.iter().zip(want) {
        assert_eq!(got.0, w.0);
        assert!((got.1 - w.1).abs() < 1e-6 && (got.2 - w.2).abs() < 1e-6, "got {got:?}, wanted {w:?}");
    }
    let mut q = base_params();
    for (node, x, y) in pos {
        apply_drag(&mut q, node, x, y);
    }
    for (a, b) in q.iter().zip(&p) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn wrong_length_params_give_the_default_positions() {
    assert_eq!(node_positions(&[1.0]), node_positions(&base_params()));
}
```

## Expected values (computed beforehand by an independent Python reference or by hand where trivial)
Python reference: `qb1/b030-expected.py`. Literals are rounded to 8 digits; the tests compare within 1e-6 or 0.05 where stated.
| test | expected |
|---|---|
| gate_drag_sets_the_threshold_and_the_point_lies_on_the_curve | gate_db -70; every other setting unchanged; handle (-70, -70) |
| comp_drag_sets_the_threshold_and_the_point_lies_on_the_curve | comp_db -30; handle (-30, -30.5) |
| ratio_drag_puts_the_point_on_the_curve | drag (-10, -18): comp_ratio 5; `static_curve(-10)` = -18 within 0.05 |
| ratio_is_clamped_to_the_parameter_range | (-10,-5) gives 1; (-10,-20) gives 20; (-10,-40) gives 20; (-10,-19.9) gives 20; (-10,-12) gives 1.25 |
| limiter_drag_sets_the_ceiling | with makeup 24, drag out -6: limit_db -6 and `static_curve(0)` = -6 exactly |
| drags_are_clamped_to_the_parameter_ranges | gate -80 / 0; comp -60 / 0; limit -24 / 0 |
| bad_numbers_and_wrong_length_leave_the_settings_alone | settings equal the defaults after five bad drags; a 3-value slice unchanged |
| positions_of_the_defaults_and_drag_back_is_stable | `(Gate,-60,-60) (Comp,-20,-20.5) (Ratio,-8,-16) (Limiter,0,-1)`; dragging each handle to its own position leaves the defaults unchanged within 1e-9 |
| wrong_length_params_give_the_default_positions | `node_positions(&[1.0])` equals `node_positions(defaults)` |

## Mutation checks (the harness runs these, not you)
Each change is applied to the production line named, one at a time, and the named test must FAIL with a real `test result: FAILED` line; the file is restored afterwards.
- `crates/core/src/effects/dynamics_graph.rs`: change `set_clamped(params, "gate_db", in_db);` to `set_clamped(params, "comp_db", in_db);`: `gate_drag_sets_the_threshold_and_the_point_lies_on_the_curve` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `set_clamped(params, "comp_db", in_db);` to `set_clamped(params, "gate_db", in_db);`: `comp_drag_sets_the_threshold_and_the_point_lies_on_the_curve` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `(in_db - comp) / rise` to `(out_db - comp) / rise`: `ratio_drag_puts_the_point_on_the_curve` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `{ f64::INFINITY }` to `{ 1.0 }`: `ratio_is_clamped_to_the_parameter_range` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `set_clamped(params, "limit_db", out_db);` to `set_clamped(params, "limit_db", in_db);`: `limiter_drag_sets_the_ceiling` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `value.clamp(desc.min, desc.max)` to `value.max(desc.min)`: `drags_are_clamped_to_the_parameter_ranges` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `|| in_db <= comp {` to `|| in_db < comp - 10.0 {`: `bad_numbers_and_wrong_length_leave_the_settings_alone` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `if params.len() != Dynamics.parameters().len() {` to `if params.len() > 100 {`: `bad_numbers_and_wrong_length_leave_the_settings_alone` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `(Node::Limiter, 0.0, limit),` to `(Node::Limiter, 1.0, limit),`: `positions_of_the_defaults_and_drag_back_is_stable` must FAIL.
- `crates/core/src/effects/dynamics_graph.rs`: change `if params.len() == defaults.len() {` to `if params.len() > 0 {`: `wrong_length_params_give_the_default_positions` must FAIL.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core dynamics_graph
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/effects/dynamics_graph.rs
```
rustfmt that one file only. Stop when the dynamics_graph tests pass (9 of 9), clippy is clean and rustfmt has been run.
