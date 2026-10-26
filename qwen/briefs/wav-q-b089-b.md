# B089-b: Doppler delay line `render` and `doppler` (add to `crates/core/src/dsp/doppler.rs`)

Refs #351 (the row has no issue of its own; #351 is the issue of its base PR 356, Pitch Shifter and Saturator).

Roadmap row B089 (Medium, T9): "Doppler Shifter". Sub-job b, scope (verbatim from the queue): "`render(input: &[f32], delays: &[f32], gains: &[f32]) -> Vec<f32>`: fractional delay line with linear interpolation, output length = input length; `doppler(input, path, rate, ref_m) -> Vec<f32>` chains B089-a and render."
Location (verbatim): new `crates/core/src/dsp/doppler.rs (same file as B089-a)`; module line in `none`; pattern function: `dsp/varispeed.rs::resample_linear` (interpolation) on main; map: crates__core__src__dsp.md.
Base branch: `qwen/wav-q-b089-a` (the branch of sub-job a, which created `doppler.rs` with `Path`, `distances`, `delays_samples`, `gains`, `SPEED_OF_SOUND`). This job is stacked on that draft PR.
Queue sentence fix: the queue says both "output length = input length" and "mismatched slice lengths use the shorter". Decision: the output has `min(input.len(), delays.len(), gains.len())` samples; with three equal-length slices that is the input length.

## Files you may touch
ONLY `crates/core/src/dsp/doppler.rs` (add two functions and new tests; no new file, no `mod.rs` change). Never edit an existing function or an existing test.

## Step 0: check the base first
If any of the first two commands finds nothing, or the third finds a line, STOP and report; write nothing.
```
grep -n "pub fn distances" crates/core/src/dsp/doppler.rs
grep -n "pub const SPEED_OF_SOUND" crates/core/src/dsp/doppler.rs
grep -n "pub fn render\|pub fn doppler" crates/core/src/dsp/doppler.rs
```

## Rules
- No git or gh commands, no network. No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!` (the crate denies them, tests included). No `assert_eq!(x, true)`.
- No extra files, no `debug_*.rs`. Do not edit any existing test or function.
- Stop and report if a stated literal cannot be matched by the code you typed.
- Run rustfmt on the changed file only: `rustfmt --edition 2024 crates/core/src/dsp/doppler.rs`.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output and quote it in your report.

## Why and behaviour
Sub-job a computes, per sample, the distance, delay (in samples) and gain of a moving source. `render` plays a recording through those: output sample `i` reads the input at the fractional position `i - delay[i]`, interpolated linearly between the two neighbouring samples, and is multiplied by `gain[i]`. When the delay shrinks over time (source approaching) the read position runs faster than the output and the pitch rises; when it grows, the pitch falls. `doppler` chains the three functions of sub-job a and `render`.
- Samples outside the input (before its start, or after its end) read as 0.0, also inside the interpolation (so a delay of 0.5 at output sample 0 gives half of `input[0]`).
- A delay below 0 counts as 0. If a delay or a gain is not finite the output sample is 0.0.
- Output length is `min(input.len(), delays.len(), gains.len())`.
- All maths in f64, cast to f32 once at the end.
Code map: `maps/wavcraft/crates__core__src__dsp.md` (main). The file is `crates/core/src/dsp/doppler.rs`, created by sub-job a; its `use super::*;` test module stays as is. Pattern for the interpolation (do not copy or change it), `dsp/varispeed.rs` on main: read `resample_linear` there first (`grep -n "fn resample_linear" crates/core/src/dsp/varispeed.rs`).

## Code to add: the two functions
Type them exactly, directly above the line `#[cfg(test)]` in `doppler.rs`.
```rust
/// Play `input` through a fractional delay line with linear interpolation.
///
/// Output sample `i` is `gains[i]` times the input read at position `i - delays[i]` (a delay
/// below 0 counts as 0). Positions outside the input read as 0.0. When a delay or a gain is not
/// finite the sample is 0.0. The result has `min(input.len(), delays.len(), gains.len())` samples.
pub fn render(input: &[f32], delays: &[f32], gains: &[f32]) -> Vec<f32> {
    let frames = input.len().min(delays.len()).min(gains.len());
    let at = |k: i64| -> f64 { usize::try_from(k).ok().and_then(|k| input.get(k)).map_or(0.0, |&v| f64::from(v)) };
    (0..frames)
        .map(|index| {
            let (Some(&delay), Some(&gain)) = (delays.get(index), gains.get(index)) else {
                return 0.0;
            };
            if !delay.is_finite() || !gain.is_finite() {
                return 0.0;
            }
            let position = index as f64 - f64::from(delay.max(0.0));
            let base = position.floor();
            let frac = position - base;
            let k = base as i64;
            (f64::from(gain) * ((1.0 - frac) * at(k) + frac * at(k + 1))) as f32
        })
        .collect()
}

/// Doppler-shift `input` along `path`: the distances, delays (at [`SPEED_OF_SOUND`]) and gains of
/// the path model, then [`render`]. `rate` is the sample rate, `ref_m` the distance inside which
/// the gain stays 1.
pub fn doppler(input: &[f32], path: &Path, rate: f32, ref_m: f32) -> Vec<f32> {
    let d = distances(path, input.len(), rate);
    let delays = delays_samples(&d, rate, SPEED_OF_SOUND);
    let g = gains(&d, ref_m);
    render(input, &delays, &g)
}
```

## Code to add: helpers and tests
Type them exactly, inside `mod tests`, directly before the final `}` of the file (after the last existing test). Do not touch the existing helpers (`approx`, `fly_by`).
```rust
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-6, "{a} != {b}");
    }

    fn sine(frames: usize) -> Vec<f32> {
        (0..frames).map(|k| (2.0 * std::f64::consts::PI * 1000.0 * k as f64 / 48_000.0).sin() as f32).collect()
    }

    fn crossings(samples: &[f32]) -> usize {
        samples.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn zero_delay_and_unit_gain_is_the_identity() {
        let input = [0.5, -0.25, 1.0, 0.0, 0.75];
        assert_eq!(render(&input, &[0.0; 5], &[1.0; 5]), input.to_vec());
    }

    #[test]
    fn an_integer_delay_shifts_exactly() {
        let input = [0.5, -0.25, 1.0, 0.0, 0.75];
        assert_eq!(render(&input, &[2.0; 5], &[1.0; 5]), vec![0.0, 0.0, 0.5, -0.25, 1.0]);
    }

    #[test]
    fn a_fractional_delay_interpolates_linearly() {
        let out = render(&[1.0, 2.0, 3.0, 4.0], &[0.5; 4], &[1.0; 4]);
        assert_eq!(out.len(), 4);
        near(out[0], 0.5);
        near(out[1], 1.5);
        near(out[2], 2.5);
        near(out[3], 3.5);
    }

    #[test]
    fn each_sample_is_scaled_by_its_gain() {
        let out = render(&[1.0, 1.0, 1.0], &[0.0; 3], &[0.5, 0.25, 2.0]);
        assert_eq!(out, vec![0.5, 0.25, 2.0]);
    }

    #[test]
    fn an_approaching_source_raises_the_pitch() {
        let n = 40_000;
        let delays: Vec<f32> = (0..n).map(|i| 3000.0 - 0.05 * i as f32).collect();
        let out = render(&sine(n), &delays, &vec![1.0; n]);
        let count = crossings(&out[4000..36_000]);
        assert!((1389..=1417).contains(&count), "got {count} crossings, expected about 1403 (1000 Hz * c / (c - v) over 32000 samples)");
    }

    #[test]
    fn a_receding_source_lowers_the_pitch() {
        let n = 40_000;
        let delays: Vec<f32> = (0..n).map(|i| 1000.0 + 0.05 * i as f32).collect();
        let out = render(&sine(n), &delays, &vec![1.0; n]);
        let count = crossings(&out[4000..36_000]);
        assert!((1257..=1283).contains(&count), "got {count} crossings, expected about 1270 (1000 Hz * c / (c + v) over 32000 samples)");
    }

    #[test]
    fn mismatched_slice_lengths_use_the_shorter() {
        let input = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(render(&input, &[0.0; 3], &[1.0; 4]), vec![1.0, 2.0, 3.0]);
        assert_eq!(render(&input, &[0.0; 5], &[1.0; 2]), vec![1.0, 2.0]);
        assert_eq!(render(&input[..2], &[0.0; 5], &[1.0; 5]), vec![1.0, 2.0]);
        assert!(render(&[], &[0.0; 3], &[1.0; 3]).is_empty());
    }

    #[test]
    fn a_non_finite_delay_or_gain_gives_a_silent_sample() {
        let out = render(&[1.0, 1.0, 1.0, 1.0], &[0.0, f32::NAN, 0.0, f32::INFINITY], &[1.0, 1.0, f32::NAN, 1.0]);
        assert_eq!(out, vec![1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_negative_delay_counts_as_zero() {
        let input = [0.5, -0.25, 1.0, 0.0];
        assert_eq!(render(&input, &[-2.0, -0.5, -100.0, -1.0], &[1.0; 4]), input.to_vec());
    }

    #[test]
    fn doppler_chains_the_path_model_and_the_delay_line() {
        let mut input = vec![0.0; 1500];
        input[0] = 1.0;
        let path = Path { start: (343.0, 0.0), end: (343.0, 0.0), listener: (0.0, 0.0), speed_m_s: 0.0 };
        let quiet = doppler(&input, &path, 1000.0, 34.3);
        assert_eq!(quiet.len(), 1500);
        near(quiet[999], 0.0);
        assert!((quiet[1000] - 0.1).abs() <= 1e-5, "{}", quiet[1000]);
        near(quiet[1001], 0.0);
        let loud = doppler(&input, &path, 1000.0, 400.0);
        near(loud[1000], 1.0);
    }
```

## Expected values (computed beforehand by an independent Python reference, kept in the report folder)
| test | expected |
|---|---|
| zero_delay_and_unit_gain_is_the_identity | output equals input `[0.5, -0.25, 1.0, 0.0, 0.75]` |
| an_integer_delay_shifts_exactly | `[0.0, 0.0, 0.5, -0.25, 1.0]` |
| a_fractional_delay_interpolates_linearly | `[0.5, 1.5, 2.5, 3.5]` |
| each_sample_is_scaled_by_its_gain | `[0.5, 0.25, 2.0]` |
| an_approaching_source_raises_the_pitch | 1 kHz sine, delay 3000 - 0.05 i: 1399 crossings in samples 4000..36000 (accept 1389 to 1417; no change would give 1334) |
| a_receding_source_lowers_the_pitch | delay 1000 + 0.05 i: 1267 crossings (accept 1257 to 1283) |
| mismatched_slice_lengths_use_the_shorter | `[1,2,3]`, `[1,2]`, `[1,2]`, empty |
| a_non_finite_delay_or_gain_gives_a_silent_sample | `[1.0, 0.0, 0.0, 0.0]` |
| a_negative_delay_counts_as_zero | output equals input |
| doppler_chains_the_path_model... | length 1500; out[999]=0, out[1000]=0.1 (ref 34.3), out[1001]=0; out[1000]=1.0 (ref 400) |
10 new tests must exist and pass (plus the 11 of sub-job a, untouched).

## Mutation checks (the driver runs them; you do not)
Each is made on the rustfmt-formatted file; the named test must then print `test result: FAILED`.
1. zero_delay_and_unit_gain_is_the_identity: change `(1.0 - frac) * at(k)` to `(1.0 - frac) * at(k + 1)`.
2. an_integer_delay_shifts_exactly: change `let k = base as i64;` to `let k = base as i64 - 1;`.
3. a_fractional_delay_interpolates_linearly: change `let frac = position - base;` to `let frac = (position - base) * 0.5;`.
4. each_sample_is_scaled_by_its_gain: change `(f64::from(gain) * ((1.0 - frac)` to `(1.0 * ((1.0 - frac)`.
5. an_approaching_source_raises_the_pitch: change `index as f64 - f64::from(delay.max(0.0))` to `index as f64 + f64::from(delay.max(0.0))`.
6. a_receding_source_lowers_the_pitch: change `f64::from(delay.max(0.0))` to `f64::from(delay.min(1000.0))`.
7. mismatched_slice_lengths_use_the_shorter: change `let frames = input.len().min(delays.len()).min(gains.len());` to `let frames = input.len();`.
8. a_non_finite_delay_or_gain_gives_a_silent_sample: change `if !delay.is_finite() || !gain.is_finite() {` to `if false {`.
9. a_negative_delay_counts_as_zero: change `f64::from(delay.max(0.0))` to `f64::from(delay)`.
10. doppler_chains_the_path_model...: change `let g = gains(&d, ref_m);` to `let g = gains(&d, 1.0e9);`.

## Commands (plain cargo; this is a local run)
```
cargo test -p wavcraft-core doppler
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/dsp/doppler.rs
```
Stop when all 21 doppler tests pass, clippy is clean and rustfmt was run. Do not push, do not open a PR.
