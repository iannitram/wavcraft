# C106: track height presets (core setters on Multitrack)

Refs #305 (the row has no issue of its own; this is the issue of its base PR 318 (f103, multitrack basics)). Change only these two files: the NEW file `crates/core/src/multitrack/track_height.rs` and ONE added line `pub mod track_height;` in `crates/core/src/multitrack.rs`. Touch nothing else.

## Step 0: check the base first (before you edit anything)
Run `ls crates/core/src/multitrack/track_height.rs` and `grep -n "track_height" crates/core/src/multitrack.rs` and `grep -n "TrackHeight" crates/core/src/multitrack.rs`.
- If `crates/core/src/multitrack/track_height.rs` already exists, or the grep finds `track_height`, STOP and report that. Write nothing.
- Otherwise continue. `grep -n "pub fn set_mute" crates/core/src/multitrack.rs` must find one line; if it does not, STOP and report (wrong base). The folder `crates/core/src/multitrack/` may not exist yet: create it by creating the file in it (the module `multitrack` is `crates/core/src/multitrack.rs`, so a child module lives in that folder).

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only write the file and add the one line.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` (denied by the crate lib), and no `assert_eq!(x, true)`.
- Do not edit any existing test or any existing line. Add no other files, no `debug_*.rs` scratch files, no dependencies.
- Type the file below exactly (every literal, every error string). If a stated literal cannot be matched, stop and report it; do not change it.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- Run rustfmt on the changed files only: `rustfmt --edition 2024 <file>` (never the whole crate).

## Roadmap row (verbatim)
`| 465 | C106 | T9 | Track height presets | Small/medium/large track height | missing |  | SoundCraft model TrackHeight (reusable) |  | S | C097 |  |`

## Why and behaviour
The multitrack editor draws every track row at a fixed height (a constant in the app). The row asks for Small, Medium and Large track heights. SoundCraft's model already has the `TrackHeight` enum (Micro 16, Mini 24, Small 46, Medium 88, Large 132, Jumbo 220, Extreme 440 points; `points()`, `from_id(name)`, `step(taller)`) and the field `Track::height` (public, default Medium), pinned by the base `Cargo.toml` (`soundcraft-model` rev 734761cd). Nothing in wavcraft reads or sets that field yet. This job adds the five `Multitrack` methods that do: `track_height`, `track_height_points`, `set_track_height`, `set_track_height_named`, `step_track_height`. Drawing with the height is a later UI job; no existing file except the one `pub mod` line changes.

## Where it goes (code context)
- Module `crates/core/src/multitrack.rs` (base f103). `Multitrack` has the public field `session: soundcraft_model::Session`; `self.session.track(id)` and `self.session.track_mut(id)` return `Option<&Track>` / `Option<&mut Track>`.
- The private helper `fn err<T>(s: impl Into<String>) -> Result<T, MtError>` and `pub struct MtError(pub String)` (derives Debug, Clone, PartialEq) are in `multitrack.rs`; a child module reaches them with `use super::{MtError, Multitrack, err};`. `return err("..")` is already a `Result` return.
- The neighbouring pattern is `set_mute` below. No code map covers multitrack (maps describe main, and `multitrack.rs` is only on this base).

One existing pattern function from the base, for style (do not edit it):
```rust
impl Multitrack {
    // ... (inside the existing `impl Multitrack` block in multitrack.rs)
    pub fn set_mute(&mut self, id: TrackId, on: bool) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.mute = on;
        Ok(())
    }
}
```

Add this line to `crates/core/src/multitrack.rs`: `pub mod track_height;` directly after the existing line `pub use soundcraft_model;` (leave one blank line between them). Do not put it among other lines that a sibling PR may also touch.

## The complete new file `crates/core/src/multitrack/track_height.rs` (type it exactly)
```rust
//! Track height presets (roadmap C106). A multitrack track is drawn at one of a few heights
//! (micro, mini, small, medium, large, jumbo, extreme). SoundCraft's model already has the
//! `TrackHeight` type, its sizes in points and the `Track::height` field, so none of that is
//! rebuilt here. What is added, in core only, is the way to read and change a track's height by
//! track id: set an exact height, set one by its name, or step one preset taller or shorter.
//!
//! A new track is `Medium` (88 points). Stepping stops at `Micro` and `Extreme`. A custom height
//! is kept as given and drawn clamped to 16 to 440 points. A name is one of the seven preset
//! names (any case, spaces around it ignored); "custom" and "fit to window" are not names here.
//!
//! Every call that fails changes nothing. Setting returns the old height; stepping returns the
//! new height. Drawing the rows is not here.

use soundcraft_model::{TrackHeight, TrackId};

use super::{MtError, Multitrack, err};

impl Multitrack {
    /// A track's height.
    pub fn track_height(&self, id: TrackId) -> Result<TrackHeight, MtError> {
        match self.session.track(id) {
            Some(t) => Ok(t.height),
            None => err("no such track"),
        }
    }

    /// A track's height in points, as the model draws it (custom heights clamped to 16 to 440).
    pub fn track_height_points(&self, id: TrackId) -> Result<f32, MtError> {
        Ok(self.track_height(id)?.points())
    }

    /// Set a track's height; returns the height it had. An unknown track is an error.
    pub fn set_track_height(&mut self, id: TrackId, height: TrackHeight) -> Result<TrackHeight, MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        Ok(std::mem::replace(&mut t.height, height))
    }

    /// Set a track's height from a preset name; returns the height it had. An unknown name or an
    /// unknown track is an error and changes nothing.
    pub fn set_track_height_named(&mut self, id: TrackId, name: &str) -> Result<TrackHeight, MtError> {
        let Some(height) = TrackHeight::from_id(name.trim()) else {
            return err(format!("a track height is one of micro, mini, small, medium, large, jumbo, extreme, not {name:?}"));
        };
        self.set_track_height(id, height)
    }

    /// Move a track's height to the next preset taller (`taller` true) or shorter; returns the
    /// new height. At the largest or smallest preset it stays. An unknown track is an error.
    pub fn step_track_height(&mut self, id: TrackId, taller: bool) -> Result<TrackHeight, MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.height = t.height.step(taller);
        Ok(t.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;

    fn setup() -> Result<(Multitrack, TrackId), MtError> {
        let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24)?;
        let id = m.add_track(Some("Voice"), 1)?;
        Ok((m, id))
    }

    #[test]
    fn new_track_height_is_medium_88() -> Result<(), MtError> {
        let (m, id) = setup()?;
        assert_eq!(m.track_height(id), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height_points(id), Ok(88.0));
        Ok(())
    }

    #[test]
    fn set_track_height_returns_old_and_applies() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.set_track_height(id, TrackHeight::Large), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Large));
        assert_eq!(m.track_height_points(id), Ok(132.0));
        Ok(())
    }

    #[test]
    fn set_track_height_named_ignores_case_and_spaces() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.set_track_height_named(id, "  SMALL "), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height_points(id), Ok(46.0));
        Ok(())
    }

    #[test]
    fn named_unknown_and_custom_are_refused_and_change_nothing() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        let msg = |name: &str| MtError(format!("a track height is one of micro, mini, small, medium, large, jumbo, extreme, not {name:?}"));
        assert_eq!(m.set_track_height_named(id, "huge"), Err(msg("huge")));
        assert_eq!(m.set_track_height_named(id, "custom"), Err(msg("custom")));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Medium));
        Ok(())
    }

    #[test]
    fn custom_height_points_are_clamped() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Custom(5000))?;
        assert_eq!(m.track_height_points(id), Ok(440.0));
        m.set_track_height(id, TrackHeight::Custom(1))?;
        assert_eq!(m.track_height_points(id), Ok(16.0));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Custom(1)));
        Ok(())
    }

    #[test]
    fn step_taller_from_medium_is_large() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Large));
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Jumbo));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Jumbo));
        Ok(())
    }

    #[test]
    fn step_taller_stops_at_extreme() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Jumbo)?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Extreme));
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Extreme));
        Ok(())
    }

    #[test]
    fn step_shorter_stops_at_micro() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Small)?;
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Mini));
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Micro));
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Micro));
        Ok(())
    }

    #[test]
    fn step_from_custom_goes_to_the_next_preset() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.set_track_height(id, TrackHeight::Custom(100))?;
        assert_eq!(m.step_track_height(id, true), Ok(TrackHeight::Large));
        m.set_track_height(id, TrackHeight::Custom(100))?;
        assert_eq!(m.step_track_height(id, false), Ok(TrackHeight::Medium));
        Ok(())
    }

    #[test]
    fn unknown_track_is_refused_everywhere() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        m.remove_track(id)?;
        let gone = Err(MtError("no such track".to_string()));
        assert_eq!(m.track_height(id), gone);
        assert_eq!(m.track_height_points(id), Err(MtError("no such track".to_string())));
        assert_eq!(m.set_track_height(id, TrackHeight::Large), Err(MtError("no such track".to_string())));
        assert_eq!(m.set_track_height_named(id, "large"), Err(MtError("no such track".to_string())));
        assert_eq!(m.step_track_height(id, true), Err(MtError("no such track".to_string())));
        Ok(())
    }

    #[test]
    fn setting_one_track_leaves_the_others() -> Result<(), MtError> {
        let (mut m, id) = setup()?;
        let other = m.add_track(Some("Guitar"), 2)?;
        m.set_track_height(id, TrackHeight::Jumbo)?;
        assert_eq!(m.track_height(other), Ok(TrackHeight::Medium));
        assert_eq!(m.track_height(id), Ok(TrackHeight::Jumbo));
        Ok(())
    }
}
```

## Expected values (computed beforehand by an independent reference: the file was compiled and run for real in a scratch copy)
- `new_track_height_is_medium_88`: height `Medium`, points `88.0`.
- `set_track_height_returns_old_and_applies`: set Large returns `Ok(Medium)`; then height `Large`, points `132.0`.
- `set_track_height_named_ignores_case_and_spaces`: `"  SMALL "` returns `Ok(Medium)`; points `46.0`.
- `named_unknown_and_custom_are_refused_and_change_nothing`: `"huge"` and `"custom"` give the exact message `a track height is one of micro, mini, small, medium, large, jumbo, extreme, not "huge"` (and with `"custom"`); height stays `Medium`.
- `custom_height_points_are_clamped`: `Custom(5000)` gives `440.0`, `Custom(1)` gives `16.0`, and the stored height stays `Custom(1)`.
- `step_taller_from_medium_is_large`: taller gives `Large`, then `Jumbo`.
- `step_taller_stops_at_extreme`: from Jumbo, taller gives `Extreme` twice.
- `step_shorter_stops_at_micro`: from Small, shorter gives `Mini`, `Micro`, `Micro`.
- `step_from_custom_goes_to_the_next_preset`: `Custom(100)` (100 points) taller gives `Large` (132), shorter gives `Medium` (88).
- `unknown_track_is_refused_everywhere`: after `remove_track`, all five methods give `Err(MtError("no such track"))`.
- `setting_one_track_leaves_the_others`: setting track 1 to Jumbo leaves track 2 `Medium`.

## Mutation checks (the harness runs these, not you)
One mutation per test is applied to the production line and the test must FAIL with a real `test result: FAILED` line; a compile error or a missing result line counts as NOT killed.
- `new_track_height_is_medium_88`: in `track_height` change `Some(t) => Ok(t.height),` to `Some(_) => Ok(TrackHeight::Large),`; the default check sees Large instead of Medium.
- `set_track_height_returns_old_and_applies`: in `set_track_height` replace `Ok(std::mem::replace(&mut t.height, height))` by `{ t.height = height; Ok(height) }`; it returns Large, not the old Medium.
- `set_track_height_named_ignores_case_and_spaces`: in `set_track_height_named` change `TrackHeight::from_id(name.trim())` to `TrackHeight::from_id(name)`; the spaced name is refused.
- `named_unknown_and_custom_are_refused_and_change_nothing`: change the message start `return err(format!("a track height is one of` to `return err(format!("a height is one of`; the exact message differs.
- `custom_height_points_are_clamped`: in `track_height_points` change `.points())` to `.points() + 1.0)`; points are 441.0, not 440.0.
- `step_taller_from_medium_is_large`: in `step_track_height` change `t.height.step(taller)` to `t.height.step(!taller)`; taller gives Small.
- `step_taller_stops_at_extreme`: in `step_track_height` change the last line `Ok(t.height)` to `Ok(TrackHeight::Jumbo)`; it returns Jumbo, not Extreme.
- `step_shorter_stops_at_micro`: change `t.height = t.height.step(taller);` to `t.height = t.height.step(taller).step(taller);`; from Small the first step gives Micro, not Mini.
- `step_from_custom_goes_to_the_next_preset`: change `t.height.step(taller)` to `t.height.step(true)`; shorter from Custom(100) gives Large, not Medium.
- `unknown_track_is_refused_everywhere`: inside `step_track_height` only, change `else { return err("no such track") };` to `else { return Ok(TrackHeight::Medium) };`; the last assertion gets Ok.
- `setting_one_track_leaves_the_others`: inside `set_track_height` only, change `self.session.track_mut(id)` to `self.session.tracks.last_mut()`; the second track changes instead of the first.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core track_height
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/multitrack/track_height.rs
```
Stop when the track_height tests pass, clippy is clean and rustfmt has been run.
