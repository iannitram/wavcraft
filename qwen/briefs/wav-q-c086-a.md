# C086-a: Track selection and clips in the time selection

Refs #321 (the roadmap row has no issue; #321 is the issue of the base PR #323). Never write "Closes". Files you may touch: `crates/core/src/multitrack/track_selection.rs` and ONE new line in `crates/core/src/multitrack.rs`. Nothing else.
Local only: do not push. The harness checks, commits and opens the PR.

## Step 0: check the base first (before you edit anything)
Run, from the repo root:
- `ls crates/core/src/multitrack/track_selection.rs`
- `grep -n "TrackSelection\|fn clips_in_selection" crates/core/src/multitrack.rs`
- `grep -n "pub fn selection_seconds\|pub fn track_ids\|pub fn set_selection" crates/core/src/multitrack.rs`
`ls` must say the file does not exist, the first `grep` must find nothing, and the second must find all three functions. If a check fails, STOP and report it; write nothing.

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit files.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` in production code (the crate denies them). Test code may use `.unwrap_or_else(|e| panic!("{e})")` exactly as written below. Use no `assert_eq!(x, true)`.
- Do not edit, rename, split or reformat any existing test or any existing line (except the one new line stated). Add no dependencies, no other files and no `debug_*.rs` scratch files.
- If a stated literal or a stated line cannot be matched, stop and report it. Do not change it.
- Type the file text below exactly (the design is fixed; your job is the typing). Run `rustfmt --edition 2024` on the one new file only, never the crate.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output (the compiler or test log) and quote it in your report.

## Why
A time selection should apply to a chosen set of tracks, not all of them. This job adds the pure side: a list of chosen tracks (kept beside the session, because the model has no field for it) and a query for the clips of those tracks that overlap the time selection. No UI.

## Behaviour
- `TrackSelection` wraps `Vec<TrackId>` (private field). `select_only` replaces it with one track; `toggle` adds or removes one; `clear` empties; `contains`; `ids` returns the ids in chosen order.
- `extend_range(mt, from, to)` replaces the selection with every track from `from` to `to` inclusive in `mt.track_ids()` order (either direction). A track that is not in the session gives `Err(MtError("no such track"))` and leaves the selection alone. (The queue says only "all tracks between two": this job defines it as replacing the selection and counting both ends.)
- `Multitrack::clips_in_selection(sel)` returns `(TrackId, ClipId)` pairs: tracks in track order, clips in playlist order, only clips of chosen tracks that overlap the time selection (`self.session.edit.selection`, in samples). Overlap is strict: `clip.start < sel.end && clip.start + clip.length > sel.start`, so a clip that only touches an edge is out. A point selection (end not after start) gives an empty list. Ids of removed tracks are ignored because the query walks the session's real tracks.

## Where it goes and the code around it
- Base branch: `feat/ap-f110-session-markers`.
- Map: none for multitrack (the maps describe main, which has no multitrack.rs): read `crates/core/src/multitrack.rs` on this base. The folder `crates/core/src/multitrack/` may not exist yet (another job adds a sibling file there): create it if needed. Rust finds the file from the `pub mod` line in `multitrack.rs`.
- Registration: Put `pub mod track_selection;` on its own line directly after the line `impl std::error::Error for MtError {}` (about line 40), with a blank line before it. Another job puts its own `pub mod` line near line 23: do not put yours there.
- One existing function from this base, as the pattern to follow (do not edit it):

```rust
    /// The time selection in seconds, or `None` when it is only a point.
    pub fn selection_seconds(&self) -> Option<(f64, f64)> {
        let r = self.session.edit.selection;
        let rate = self.session.sample_rate;
        (r.end > r.start).then(|| (rate.seconds(r.start), rate.seconds(r.end)))
    }
```

```rust
    /// Set the time selection (the two times in either order; equal times make it a point).
    pub fn set_selection(&mut self, a: f64, b: f64) -> Result<(), MtError> {
        if !a.is_finite() || !b.is_finite() {
            return err("a time is a finite number of seconds");
        }
        let rate = self.session.sample_rate;
        self.session.edit.selection = soundcraft_model::time::Range::new(rate.samples(a.max(0.0)), rate.samples(b.max(0.0)));
        Ok(())
    }
```

## The queue row, verbatim
| 14 | C086-a | C086 | T9 | Time selection and playhead in multitrack | needs-care | feat/ap-f110-session-markers (#323) | new -> #323 -> #318 -> main | `crates/core/src/multitrack/track_selection.rs` | crates/core/src/multitrack.rs | partly |

### 14. C086-a (Time selection and playhead in multitrack), needs-care
- Base: feat/ap-f110-session-markers (#323). Chain: new -> #323 -> #318 -> main. Depends-on: C074.
- Scope: Which tracks a time selection applies to: `struct TrackSelection(Vec<TrackId>)`; `select_only(id)`, `toggle(id)`, `extend_range(mt, from, to)` (all tracks between two, in track_ids order), `clear()`, `contains(id)`; `Multitrack::clips_in_selection(&self, sel: &TrackSelection) -> Vec<(TrackId, ClipId)>` (clips of selected tracks that overlap `selection_seconds()`).
- Tests: extend_range both directions; toggle twice; selection with no time range gives no clips; a clip touching the edge only; track removal leaves stale ids ignored.
- Where: new `crates/core/src/multitrack/track_selection.rs`; module line in `crates/core/src/multitrack.rs`; pattern function: `Multitrack::set_selection` (:333) and `selection_seconds` (:326) in #323; map: none: read #323 `multitrack.rs`.
- Screen: partly: #323 has playhead and time selection (ruler click/drag, `SMARKER playhead|select`). Selection across a chosen set of tracks and the clip query are not there.
- Note: Needs the model clip fields (start, length): read `add_clip_buffer` in `multitrack.rs` first.

## The complete new file
Type it exactly. It is rustfmt-formatted; run `rustfmt --edition 2024` on the file afterwards anyway. There are exactly 7 new tests and all 7 must exist and pass.

```rust
//! Which tracks a time selection applies to (C086).
//!
//! `TrackSelection` is a list of track ids kept beside the session (the model has no field for
//! it). `Multitrack::clips_in_selection` then lists the clips of the chosen tracks that
//! overlap the session's time selection. A selected id whose track was removed is simply
//! ignored by that query.

use soundcraft_model::{ClipId, TrackId};

use super::{MtError, Multitrack, err};

/// The chosen tracks, in the order they were chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackSelection(Vec<TrackId>);

impl TrackSelection {
    /// Choose this track and nothing else.
    pub fn select_only(&mut self, id: TrackId) {
        self.0 = vec![id];
    }

    /// Add the track when it is not chosen, remove it when it is.
    pub fn toggle(&mut self, id: TrackId) {
        match self.0.iter().position(|t| *t == id) {
            Some(i) => {
                self.0.remove(i);
            }
            None => self.0.push(id),
        }
    }

    /// Replace the selection with every track from `from` to `to` inclusive, in the session's
    /// track order (`from` may come after `to`). Either track missing is an error and leaves
    /// the selection alone.
    pub fn extend_range(&mut self, mt: &Multitrack, from: TrackId, to: TrackId) -> Result<(), MtError> {
        let ids = mt.track_ids();
        let (Some(a), Some(b)) = (ids.iter().position(|t| *t == from), ids.iter().position(|t| *t == to)) else {
            return err("no such track");
        };
        let (lo, hi) = (a.min(b), a.max(b));
        self.0 = ids[lo..=hi].to_vec();
        Ok(())
    }

    /// Choose no track.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.0.contains(&id)
    }

    /// The chosen ids, in the order they were chosen.
    pub fn ids(&self) -> &[TrackId] {
        &self.0
    }
}

impl Multitrack {
    /// The clips of the chosen tracks that overlap the time selection, as (track, clip), in
    /// track order and then clip order. A clip that only touches an edge of the selection is
    /// not included. No time range (the selection is only a point) gives no clips.
    pub fn clips_in_selection(&self, sel: &TrackSelection) -> Vec<(TrackId, ClipId)> {
        let range = self.session.edit.selection;
        if range.end <= range.start {
            return Vec::new();
        }
        let mut out = Vec::new();
        for id in self.track_ids() {
            if !sel.contains(id) {
                continue;
            }
            let Some(track) = self.session.track(id) else { continue };
            for clip in track.clips() {
                if clip.start < range.end && clip.start + clip.length > range.start {
                    out.push((id, clip.id));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;
    use soundcraft_model_audio::AudioBuffer as ModelBuffer;

    /// A session with four mono tracks a, b, c, d (in that order).
    fn session() -> (Multitrack, [TrackId; 4]) {
        let mut mt = Multitrack::new("s", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        let mut ids = Vec::new();
        for _ in 0..4 {
            ids.push(mt.add_track(None, 1).unwrap_or_else(|e| panic!("{e}")));
        }
        (mt, [ids[0], ids[1], ids[2], ids[3]])
    }

    /// A silent mono clip `seconds` long, placed at `at` seconds.
    fn add_clip(mt: &mut Multitrack, track: TrackId, at: f64, seconds: f64) -> ClipId {
        let frames = (seconds * 48_000.0) as usize;
        let audio = ModelBuffer { sample_rate: 48_000, channels: vec![vec![0.0; frames]] };
        mt.add_clip_buffer(track, "c", "c.wav", audio, at).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn extend_range_picks_the_tracks_between_in_both_directions() {
        let (mt, [a, b, c, d]) = session();
        let mut sel = TrackSelection::default();
        assert_eq!(sel.extend_range(&mt, b, d), Ok(()));
        assert_eq!(sel.ids(), [b, c, d]);
        assert_eq!(sel.extend_range(&mt, d, b), Ok(()));
        assert_eq!(sel.ids(), [b, c, d]);
        assert_eq!(sel.extend_range(&mt, a, a), Ok(()));
        assert_eq!(sel.ids(), [a]);
    }

    #[test]
    fn toggle_twice_leaves_the_selection_as_it_was() {
        let (_mt, [a, b, _, _]) = session();
        let mut sel = TrackSelection::default();
        sel.toggle(a);
        assert!(sel.contains(a));
        sel.toggle(b);
        sel.toggle(a);
        assert!(!sel.contains(a));
        assert_eq!(sel.ids(), [b]);
    }

    #[test]
    fn select_only_replaces_and_clear_empties() {
        let (_mt, [a, b, c, _]) = session();
        let mut sel = TrackSelection::default();
        sel.toggle(a);
        sel.toggle(b);
        sel.select_only(c);
        assert_eq!(sel.ids(), [c]);
        sel.toggle(a);
        sel.clear();
        assert!(sel.ids().is_empty());
    }

    #[test]
    fn no_time_range_gives_no_clips() {
        let (mut mt, [a, _, _, _]) = session();
        add_clip(&mut mt, a, 0.0, 1.0);
        let mut sel = TrackSelection::default();
        sel.select_only(a);
        assert!(mt.clips_in_selection(&sel).is_empty());
        mt.set_selection(0.5, 0.5).unwrap_or_else(|e| panic!("{e}"));
        assert!(mt.clips_in_selection(&sel).is_empty());
    }

    #[test]
    fn a_clip_touching_the_edge_only_is_left_out() {
        let (mut mt, [a, _, _, _]) = session();
        let c1 = add_clip(&mut mt, a, 0.0, 1.0);
        let c2 = add_clip(&mut mt, a, 2.0, 1.0);
        let mut sel = TrackSelection::default();
        sel.select_only(a);
        mt.set_selection(1.0, 2.0).unwrap_or_else(|e| panic!("{e}"));
        assert!(mt.clips_in_selection(&sel).is_empty());
        mt.set_selection(0.9, 2.1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(mt.clips_in_selection(&sel), vec![(a, c1), (a, c2)]);
    }

    #[test]
    fn only_chosen_tracks_count_and_the_result_follows_track_order() {
        let (mut mt, [a, b, c, _]) = session();
        let ca = add_clip(&mut mt, a, 0.0, 1.0);
        let cb = add_clip(&mut mt, b, 0.0, 1.0);
        add_clip(&mut mt, c, 0.0, 1.0);
        mt.set_selection(0.0, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let mut sel = TrackSelection::default();
        sel.select_only(b);
        assert_eq!(mt.clips_in_selection(&sel), vec![(b, cb)]);
        sel.toggle(a);
        assert_eq!(mt.clips_in_selection(&sel), vec![(a, ca), (b, cb)]);
    }

    #[test]
    fn a_removed_track_is_ignored_and_extend_range_refuses_it() {
        let (mut mt, [a, b, _, _]) = session();
        add_clip(&mut mt, a, 0.0, 1.0);
        let cb = add_clip(&mut mt, b, 0.0, 1.0);
        mt.set_selection(0.0, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let mut sel = TrackSelection::default();
        sel.toggle(a);
        sel.toggle(b);
        mt.remove_track(a).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(mt.clips_in_selection(&sel), vec![(b, cb)]);
        assert_eq!(sel.extend_range(&mt, a, b), Err(MtError("no such track".to_string())));
        assert_eq!(sel.ids(), [a, b]);
    }
}
```

## Expected values (computed beforehand by an independent Python reference or by hand where trivial)
| test | expected |
|---|---|
| extend_range_picks_the_tracks_between_in_both_directions | (b,d) and (d,b) give `[b, c, d]`; (a,a) gives `[a]` (tracks a,b,c,d in order) |
| toggle_twice_leaves_the_selection_as_it_was | after toggle a, toggle b, toggle a: ids `[b]`, a not contained |
| select_only_replaces_and_clear_empties | `[c]` after select_only(c); empty after clear |
| no_time_range_gives_no_clips | empty for the default selection and for a point selection at 0.5 s inside a clip 0..1 s |
| a_clip_touching_the_edge_only_is_left_out | clips 0..1 s and 2..3 s: selection 1..2 s gives empty; selection 0.9..2.1 s gives `[(a,c1),(a,c2)]` |
| only_chosen_tracks_count_and_the_result_follows_track_order | choose b: `[(b,cb)]`; choose b then a: `[(a,ca),(b,cb)]` |
| a_removed_track_is_ignored_and_extend_range_refuses_it | after remove_track(a): `[(b,cb)]`; extend_range(a,b) gives `Err("no such track")`; ids stay `[a, b]` |
All values are exact (clips are 48000 Hz, whole seconds); no python needed.

## Mutation checks (the harness runs these, not you)
Each change is applied to the production line named, one at a time, and the named test must FAIL with a real `test result: FAILED` line; the file is restored afterwards.
- `crates/core/src/multitrack/track_selection.rs`: change `let (lo, hi) = (a.min(b), a.max(b));` to `let (lo, hi) = (a, b);`: `extend_range_picks_the_tracks_between_in_both_directions` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `self.0.remove(i);` to `self.0.push(id);`: `toggle_twice_leaves_the_selection_as_it_was` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `self.0 = vec![id];` to `self.0.push(id);`: `select_only_replaces_and_clear_empties` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `self.0.clear();` to `self.0.pop();`: `select_only_replaces_and_clear_empties` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `if range.end <= range.start {` to `if range.end < range.start {`: `no_time_range_gives_no_clips` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `clip.start < range.end &&` to `clip.start <= range.end &&`: `a_clip_touching_the_edge_only_is_left_out` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `if !sel.contains(id) {` to `if sel.contains(id) {`: `only_chosen_tracks_count_and_the_result_follows_track_order` must FAIL.
- `crates/core/src/multitrack/track_selection.rs`: change `return err("no such track");` to `return Ok(());`: `a_removed_track_is_ignored_and_extend_range_refuses_it` must FAIL.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core track_selection
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/multitrack/track_selection.rs
```
rustfmt that one file only. Stop when the track_selection tests pass (7 of 7), clippy is clean and rustfmt has been run.
