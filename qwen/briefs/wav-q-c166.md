# C166: show and hide mixer sections (core state and text form)

Refs #306 (the row has no issue of its own; this is the issue of its base PR 330 (f104, mixer panel)). Change only these two files: the NEW file `crates/core/src/mixer_sections.rs` and ONE added line `pub mod mixer_sections;` in `crates/core/src/lib.rs`. Touch nothing else.

## Step 0: check the base first (before you edit anything)
Run `ls crates/core/src/mixer_sections.rs` and `grep -n "mixer_sections" crates/core/src/lib.rs`.
- If `crates/core/src/mixer_sections.rs` already exists, or the grep finds `mixer_sections`, STOP and report that. Write nothing.
- Otherwise continue. `grep -n "^pub mod io;\|^pub mod random;" crates/core/src/lib.rs` must find both lines; if not, STOP and report (wrong base).

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only write the file and add the one line.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` (denied by the crate lib), and no `assert_eq!(x, true)`.
- Do not edit any existing test or any existing line. Add no other files, no `debug_*.rs` scratch files, no dependencies.
- Type the file below exactly (every literal, every error string). If a stated literal cannot be matched, stop and report it; do not change it.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- Run rustfmt on the changed files only: `rustfmt --edition 2024 <file>` (never the whole crate).

## Roadmap row (verbatim)
`| 477 | C166 | T9 | Show/hide mixer sections | Toggle EQ, effects, sends, I/O rows | missing |  |  |  | S | C152 |  |`

## Why and behaviour
The mixer strips will have four groups of rows: EQ, effects, sends and I/O. The row asks for toggles that show or hide each group. PR 330 says section show/hide is not done. This job is the core state only, with no drawing and no dependency on the mixer file: an enum `MixerSection` (Eq, Effects, Sends, Io) with id and label, and a small `MixerSections` value that says which are shown (default: all four), flips one, lists the shown ones in fixed order, and converts to and from a short text (`eq,sends` or `none`) so the choice can be kept in a settings file. SoundCraft has no such type (checked: no `section`/`visible` type in the pinned model or mix crates).

## Where it goes (code context)
- New file `crates/core/src/mixer_sections.rs`, a plain std-only module beside `view.rs`, `space.rs` and `settings.rs` in `crates/core/src/` (base f104, PR 330). It imports nothing.
- Core is `crates/core/src/lib.rs`, which denies `unwrap`, `expect`, `panic`, `todo`, `unimplemented` and `unreachable` crate-wide, so type the file exactly as given (it uses none of them).
- Style pattern: the existing `format_time` in `crates/core/src/view.rs` (maps: `maps/wavcraft/crates__core__src__view.md`).

One existing pattern function from the base, for style (do not edit it):
```rust
/// `frame` at `sample_rate` as `m:ss.mmm`, or `h:mm:ss.mmm` from one hour up.
pub fn format_time(frame: usize, sample_rate: u32) -> String {
    if sample_rate == 0 {
        return "0:00.000".to_string();
    }
```

Add this line to `crates/core/src/lib.rs`: `pub mod mixer_sections;` directly after the existing line `pub mod io;` and before `pub mod random;`.

## The complete new file `crates/core/src/mixer_sections.rs` (type it exactly)
```rust
//! Show and hide mixer sections (roadmap C166). A mixer strip has four groups of rows: the EQ,
//! the effects, the sends and the I/O (input and output) rows. The user can hide any of them to
//! make the strips shorter. This file is the core of that: which sections are shown, how to flip
//! one, and a short text form to keep the choice in a settings file.
//!
//! The text form lists the shown sections by id, in the fixed order eq, effects, sends, io, joined
//! by commas ("eq,sends"). No shown section is written as "none". Reading the text ignores case,
//! spaces around items and repeated items, and refuses an empty text, an empty item and any id it
//! does not know. Drawing the strips is not here.

/// One group of rows on a mixer strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerSection {
    Eq,
    Effects,
    Sends,
    Io,
}

impl MixerSection {
    /// Every section, in the fixed order used for drawing and for the text form.
    pub const ALL: [MixerSection; 4] = [MixerSection::Eq, MixerSection::Effects, MixerSection::Sends, MixerSection::Io];

    /// The short lower-case id used in the text form.
    pub fn id(self) -> &'static str {
        match self {
            MixerSection::Eq => "eq",
            MixerSection::Effects => "effects",
            MixerSection::Sends => "sends",
            MixerSection::Io => "io",
        }
    }

    /// The name shown on the menu.
    pub fn label(self) -> &'static str {
        match self {
            MixerSection::Eq => "EQ",
            MixerSection::Effects => "Effects",
            MixerSection::Sends => "Sends",
            MixerSection::Io => "I/O",
        }
    }

    /// The section with this id (any case, spaces around it ignored), or `None`.
    pub fn from_id(text: &str) -> Option<MixerSection> {
        let t = text.trim();
        MixerSection::ALL.into_iter().find(|s| s.id().eq_ignore_ascii_case(t))
    }

    fn bit(self) -> u8 {
        match self {
            MixerSection::Eq => 1,
            MixerSection::Effects => 2,
            MixerSection::Sends => 4,
            MixerSection::Io => 8,
        }
    }
}

/// Which mixer sections are shown. The default shows all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixerSections {
    shown: u8,
}

impl Default for MixerSections {
    fn default() -> Self {
        MixerSections::all_shown()
    }
}

impl MixerSections {
    /// All four sections shown.
    pub fn all_shown() -> MixerSections {
        MixerSections { shown: 0b1111 }
    }

    /// No section shown.
    pub fn none_shown() -> MixerSections {
        MixerSections { shown: 0 }
    }

    /// Whether this section is shown.
    pub fn is_shown(self, section: MixerSection) -> bool {
        self.shown & section.bit() != 0
    }

    /// Show or hide one section. Setting a state it already has changes nothing.
    pub fn set_shown(&mut self, section: MixerSection, on: bool) {
        if on {
            self.shown |= section.bit();
        } else {
            self.shown &= !section.bit();
        }
    }

    /// Flip one section and return its new state (true = now shown).
    pub fn toggle(&mut self, section: MixerSection) -> bool {
        let on = !self.is_shown(section);
        self.set_shown(section, on);
        on
    }

    /// How many sections are shown (0 to 4).
    pub fn shown_count(self) -> usize {
        MixerSection::ALL.into_iter().filter(|s| self.is_shown(*s)).count()
    }

    /// The shown sections, in the fixed order of [`MixerSection::ALL`].
    pub fn shown(self) -> Vec<MixerSection> {
        MixerSection::ALL.into_iter().filter(|s| self.is_shown(*s)).collect()
    }

    /// The text form: shown ids joined by commas, or "none".
    pub fn to_text(self) -> String {
        if self.shown == 0 {
            return "none".to_string();
        }
        self.shown().into_iter().map(MixerSection::id).collect::<Vec<_>>().join(",")
    }

    /// Read the text form (see the module comment). Any refusal is a message for the user.
    pub fn from_text(text: &str) -> Result<MixerSections, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("a section list is empty: write none to hide every section".to_string());
        }
        let mut out = MixerSections::none_shown();
        if t.eq_ignore_ascii_case("none") {
            return Ok(out);
        }
        for raw in t.split(',') {
            let tok = raw.trim();
            if tok.is_empty() {
                return Err("a section list has an empty item".to_string());
            }
            match MixerSection::from_id(tok) {
                Some(s) => out.set_shown(s, true),
                None => return Err(format!("unknown mixer section {tok:?}: use eq, effects, sends or io")),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shows_all_four() {
        let s = MixerSections::default();
        assert_eq!(s, MixerSections::all_shown());
        assert_eq!(s.shown_count(), 4);
        assert_eq!(s.shown(), MixerSection::ALL.to_vec());
    }

    #[test]
    fn toggle_returns_new_state_and_flips_only_that_section() {
        let mut s = MixerSections::default();
        assert!(!s.toggle(MixerSection::Sends));
        assert!(!s.is_shown(MixerSection::Sends));
        assert!(s.is_shown(MixerSection::Eq));
        assert!(s.is_shown(MixerSection::Effects));
        assert!(s.is_shown(MixerSection::Io));
        assert!(s.toggle(MixerSection::Sends));
        assert_eq!(s, MixerSections::all_shown());
    }

    #[test]
    fn set_shown_is_idempotent() {
        let mut s = MixerSections::default();
        s.set_shown(MixerSection::Eq, false);
        s.set_shown(MixerSection::Eq, false);
        assert_eq!(s.shown(), vec![MixerSection::Effects, MixerSection::Sends, MixerSection::Io]);
        s.set_shown(MixerSection::Eq, true);
        s.set_shown(MixerSection::Eq, true);
        assert_eq!(s, MixerSections::all_shown());
    }

    #[test]
    fn shown_keeps_the_fixed_order() {
        let mut s = MixerSections::none_shown();
        s.set_shown(MixerSection::Io, true);
        s.set_shown(MixerSection::Eq, true);
        s.set_shown(MixerSection::Sends, true);
        assert_eq!(s.shown(), vec![MixerSection::Eq, MixerSection::Sends, MixerSection::Io]);
        assert_eq!(s.shown_count(), 3);
    }

    #[test]
    fn from_id_ignores_case_and_spaces() {
        assert_eq!(MixerSection::from_id(" IO "), Some(MixerSection::Io));
        assert_eq!(MixerSection::from_id("Sends"), Some(MixerSection::Sends));
        assert_eq!(MixerSection::from_id("mixer"), None);
        assert_eq!(MixerSection::from_id("i/o"), None);
        assert_eq!(MixerSection::from_id(""), None);
    }

    #[test]
    fn ids_and_labels_are_fixed() {
        let ids: Vec<&str> = MixerSection::ALL.into_iter().map(MixerSection::id).collect();
        assert_eq!(ids, vec!["eq", "effects", "sends", "io"]);
        let labels: Vec<&str> = MixerSection::ALL.into_iter().map(MixerSection::label).collect();
        assert_eq!(labels, vec!["EQ", "Effects", "Sends", "I/O"]);
    }

    #[test]
    fn to_text_lists_shown_in_order() {
        let mut s = MixerSections::all_shown();
        assert_eq!(s.to_text(), "eq,effects,sends,io");
        s.set_shown(MixerSection::Eq, false);
        assert_eq!(s.to_text(), "effects,sends,io");
        assert_eq!(MixerSections::none_shown().to_text(), "none");
    }

    #[test]
    fn from_text_round_trips_every_choice() {
        for mask in 0u8..16 {
            let mut s = MixerSections::none_shown();
            for (i, section) in MixerSection::ALL.into_iter().enumerate() {
                s.set_shown(section, (mask >> i) & 1 == 1);
            }
            assert_eq!(MixerSections::from_text(&s.to_text()), Ok(s));
        }
    }

    #[test]
    fn from_text_ignores_case_spaces_and_repeats() {
        let s = MixerSections::from_text(" EQ , sends,eq ");
        assert_eq!(s.map(MixerSections::shown), Ok(vec![MixerSection::Eq, MixerSection::Sends]));
        assert_eq!(MixerSections::from_text(" NONE "), Ok(MixerSections::none_shown()));
    }

    #[test]
    fn from_text_refuses_bad_text() {
        assert_eq!(MixerSections::from_text(""), Err("a section list is empty: write none to hide every section".to_string()));
        assert_eq!(MixerSections::from_text("   "), Err("a section list is empty: write none to hide every section".to_string()));
        assert_eq!(MixerSections::from_text("eq,,io"), Err("a section list has an empty item".to_string()));
        assert_eq!(MixerSections::from_text("eq, ,io"), Err("a section list has an empty item".to_string()));
        assert_eq!(MixerSections::from_text("eq,mixer"), Err("unknown mixer section \"mixer\": use eq, effects, sends or io".to_string()));
        assert_eq!(MixerSections::from_text("none,eq"), Err("unknown mixer section \"none\": use eq, effects, sends or io".to_string()));
    }
}
```

## Expected values (computed beforehand by an independent reference: the file was compiled and run for real in a scratch copy)
- `default_shows_all_four`: default equals `all_shown()`, count 4, `shown()` equals `ALL`.
- `toggle_returns_new_state_and_flips_only_that_section`: toggling Sends returns `false`, only Sends is hidden; toggling again returns `true` and all four are shown.
- `set_shown_is_idempotent`: hiding Eq twice leaves `[Effects, Sends, Io]`; showing it twice restores all four.
- `shown_keeps_the_fixed_order`: after showing Io, Eq, Sends (in that order) from none, `shown()` is `[Eq, Sends, Io]`, count 3.
- `from_id_ignores_case_and_spaces`: `" IO "` gives Io, `"Sends"` gives Sends, `"mixer"`, `"i/o"`, `""` give `None`.
- `ids_and_labels_are_fixed`: ids `eq, effects, sends, io`; labels `EQ, Effects, Sends, I/O`.
- `to_text_lists_shown_in_order`: all `eq,effects,sends,io`; Eq hidden `effects,sends,io`; none `none`.
- `from_text_round_trips_every_choice`: all 16 choices survive `to_text` then `from_text`.
- `from_text_ignores_case_spaces_and_repeats`: `" EQ , sends,eq "` gives `[Eq, Sends]`; `" NONE "` gives none shown.
- `from_text_refuses_bad_text`: `""` and `"   "` give `a section list is empty: write none to hide every section`; `"eq,,io"` and `"eq, ,io"` give `a section list has an empty item`; `"eq,mixer"` gives `unknown mixer section "mixer": use eq, effects, sends or io`; `"none,eq"` gives the same with `"none"`.

## Mutation checks (the harness runs these, not you)
One mutation per test is applied to the production line and the test must FAIL with a real `test result: FAILED` line; a compile error or a missing result line counts as NOT killed.
- `default_shows_all_four`: in `all_shown` change `shown: 0b1111` to `shown: 0b0111`; Io is missing, the count is 3.
- `toggle_returns_new_state_and_flips_only_that_section`: in `toggle` change `let on = !self.is_shown(section);` to `let on = self.is_shown(section);`; toggle changes nothing and returns true.
- `set_shown_is_idempotent`: in `set_shown` change `self.shown &= !section.bit();` to `self.shown = 0;`; hiding Eq hides all.
- `shown_keeps_the_fixed_order`: in `shown` change `ALL.into_iter().filter(|s| self.is_shown(*s)).collect()` to `ALL.into_iter().rev().filter(|s| self.is_shown(*s)).collect()`; the order is reversed.
- `from_id_ignores_case_and_spaces`: in `from_id` change `s.id().eq_ignore_ascii_case(t)` to `s.id() == t`; `" IO "` no longer matches.
- `ids_and_labels_are_fixed`: change `MixerSection::Io => "I/O",` to `MixerSection::Io => "IO",`; the last label differs.
- `to_text_lists_shown_in_order`: change `.join(",")` to `.join(" ")`; `eq effects sends io`.
- `from_text_round_trips_every_choice`: in `from_text` change `let mut out = MixerSections::none_shown();` to `let mut out = MixerSections::all_shown();`; a partial list reads back as all four.
- `from_text_ignores_case_spaces_and_repeats`: change `t.eq_ignore_ascii_case("none")` to `t == "none"`; `" NONE "` is an unknown section.
- `from_text_refuses_bad_text`: change `let tok = raw.trim();` to `let tok = raw;`; `"eq, ,io"` gives the unknown-section message, not the empty-item one.
- `from_text_refuses_bad_text`: change `return Err("a section list has an empty item"` to `return Err("a section list has an empty entry"`; the exact message differs.
- `from_text_refuses_bad_text`: change `if t.is_empty() {` to `if t.is_empty() && false {`; an empty text gives the empty-item message.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core mixer_sections
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/mixer_sections.rs
```
Stop when the mixer_sections tests pass, clippy is clean and rustfmt has been run.
