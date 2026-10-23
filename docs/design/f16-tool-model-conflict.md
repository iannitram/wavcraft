# f16 tool model: conflict with main (design note, nothing implemented)

Refs #115. Source: PR #115 "f16: tool model, tools bar, right-click extend (alpha)"
(branch `feat/ap-f16-tools`, roadmap A049, A112, A057). It conflicts with main and the
conflict is a design one, so #115 was **not** re-cut. This note says what clashes and what to do.

## What #115 adds

- `crates/core/src/tools.rs`: a public `Tool` enum (`TimeSelection` default and only enabled tool;
  `Move`, `Marquee`, `Lasso`, `Paintbrush` listed disabled), with `name`, `label`, `enabled`,
  `key` (`T`) and `by_name`. A tools bar above the editor, an action `tool.time_selection`, a
  setting `right_click = extend|menu` and a `GESTURE right_click <frame>` control verb.
- Right-click in the waveform moves the nearest selection edge to the click. The maths is a new
  `Selection::extend_to(&self, frame, total) -> Selection` in `view.rs`: pure, clamped to the
  file, a cursor becomes a selection from the cursor to the click, a tie moves the end.
- STATE gains `tool=` and `right_click=`.

## What main has instead (f64, #234, and the selection gestures of f08)

- A **private** `Tool` enum in `apps/wavcraft/src/app.rs`: `Select` (default), `Hand`, `Zoom`.
  It decides what a primary-button drag does (select, scroll, zoom to range). Actions
  `tool.select`, `tool.hand`, `tool.zoom` (keys V, H, Z) switch it; STATE already reports
  `tool=select|hand|zoom`. The action table says "a minimal switch; a fuller tool model is on
  another branch", so main expects #115 or its successor.
- `Selection::extend_to(&mut self, frame: usize)` in `view.rs`: the **same name** with a
  different signature and contract. It is the Shift-click rule (move the nearer edge, via
  `grab`, then set `head`; a tie moves the start; no clamping; mutates in place). It is used by
  Shift-click and by `GESTURE shift_click`, and has its own test.
- `GESTURE` is already a control verb (f08) and the editor already handles the edge grab
  (`GRAB_PX`), Shift-click, double and triple click.

## The conflicts

1. **Two `Selection::extend_to`.** #115's takes `&self`, a `total`, and returns a new value;
   main's takes `&mut self`. They cannot coexist, and they differ on behaviour: tie-break
   (end vs start), clamping (yes vs no), and what a cursor does (both produce cursor..click).
2. **Two `Tool` types** with different variant sets. #115's is public in core and lists the
   Audition tools (Time Selection, Move, spectral); main's is private to the app and lists the
   navigation tools (Select, Hand, Zoom). Both are Audition tools and both belong on one bar.
   The action names differ too: `tool.time_selection` (key T) against `tool.select` (key V).
   STATE's `tool=` value would change meaning (`time_selection` vs `select`).
3. **Secondary button.** Main already uses a secondary-button drag in the ruler for range zoom
   (A091); #115 turns a secondary click in the waveform into an edge move. They do not overlap
   (ruler vs body) but both need the origin test main now has (`drag_origin`).
4. **Duplicate `GESTURE`.** #115 adds a minimal `GESTURE` arm; main's `control_gesture` is the
   real one. `right_click` would be one more gesture inside it.
5. **Snapping (f09, #95).** Right-click should pass through `Editor::snap` like click and drag.

## Options

**A. Adopt main's model, port only the right-click idea.** Keep main's `Tool` (app-private) and
`Selection::extend_to(&mut self, frame)`. Add right-click as `Selection::extend_nearest_to(&self,
frame, total) -> Selection` (or a clamped variant of the existing method) plus
`GESTURE right_click`, snapped. Skip the tools bar and `tools.rs`; do the `right_click = menu`
setting later. Smallest and no conflict, but the disabled Move/Marquee/Lasso/Paintbrush slots and
the core-level, testable tool list are dropped.

**B. Move main's tool model into core, then add #115's items.** Create `core::tools::Tool` with
main's three tools plus #115's disabled ones (Time Selection is `Select`; keep the names
`select`, `hand`, `zoom` and the keys V/H/Z so STATE and scripts stay valid; map `T` to
`tool.select` as a second key). The app's private enum goes away. Resolve `extend_to` by keeping
main's mutating method (the Shift-click rule) and implementing right-click on top of it, with
#115's clamp and cursor tests re-expressed against it. Add the tools bar and the setting. A
medium change that touches `app.rs`, `actions.rs`, `view.rs`, `settings.rs`; the most complete.

**C. Re-cut #115 over main unchanged and rename.** Take #115's `tools.rs` and a pure
`extend_to` renamed `Selection::with_nearest_edge_at`, leave main's `Tool` and `extend_to` in
place, and have both tool types exist for now. Fast, but two tool models, two STATE `tool=`
vocabularies and two key sets for one bar: the debt grows.

## Recommendation

**Option B**, done in two steps so each is reviewable: (1) a refactor-only PR that moves main's
`Tool` into core with unchanged names, keys and STATE output; (2) the tools bar, the disabled
slots, the clamped right-click (built on main's `extend_to`, snapped) and the setting. Reasons:
main's tools already ship and scripts read `tool=select|hand|zoom`, so main's names win; #115's
real value is the core-level list, the bar and right-click, which B keeps; and A or C leave
the app with a second tool model to remove later. If the bar is not wanted soon, do A now and B
when the Move or spectral tools start.

Questions for the owner: is `T` meant to be an alias of `V` (Select) or a separate Time
Selection tool? Should a tie in extend go to the start (main) or the end (#115)? Should
right-click as "menu" be in scope for the first step?
