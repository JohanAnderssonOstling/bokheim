# Plan 02 — `margin: auto` horizontal centering

## Goal
`margin-left: auto; margin-right: auto` on a block with a definite width should
center it in its containing block. Today it pins to the left.

## Current behavior (the bug)
- Margins are stored as `LengthPct` (`Px`/`Pct`) on `BoxModel`
  (`src/document/computed_style.rs:74`). `LengthPct` has **no `Auto`**.
- `length_or_auto_to_lengthpct` (`src/style/resolver.rs:943`) maps
  `LengthPercentageOrAuto::Auto => LengthPct::Px(0.0)`. So `auto` is
  indistinguishable from `0` by the time layout runs.
- In `layout_block_children` (`src/layout/block_layout.rs`) the parent positions
  each child at `position + (item.margin_left, y_offset)` with `margin_left`
  resolved to px — never centered.

## Design
Represent `auto` margins, then in block-child positioning apply CSS 2.1 §10.3.3:
when `width` is definite and the leftover horizontal space is positive,
distribute it to whichever of left/right margins are `auto` (both auto → split
evenly = centering; one auto → that side absorbs all leftover).

### Margin representation
Add an `Auto` case. Two options:
- **(A, recommended)** New enum `Margin { Length(LengthPct), Auto }` used only for
  the four `margin_*` fields. Keeps `LengthPct` clean (padding/text-indent never
  take `auto`).
- (B) Add `Auto` to `LengthPct`. Simpler diff but pollutes padding/text-indent
  which can't be auto.

Go with (A). `Margin` needs the same bitwise `Eq`/`Hash` treatment as `LengthPct`
(it's part of `BoxModel`'s dedup key — see `impl_style_key!` at
`computed_style.rs:113`). Add `Margin::resolve(cb_width) -> f64` returning `0.0`
for `Auto` (used by callers that don't do centering, e.g. decoration rects).

## Steps
1. **`computed_style.rs`:**
   - Define `pub enum Margin { Length(LengthPct), Auto }` with `Default =
     Length(Px(0.0))`, bitwise `PartialEq/Eq/Hash`, `resolve(cb_width)`,
     `is_zero()`, and `is_auto()`.
   - Change the four `margin_*` fields and the `DEFAULT_BOX_MODEL` constant to
     `Margin`. Update `impl_style_key!(BoxModel …)` (margins move out of the
     plain-field list since they need custom hashing — either give `Margin` a
     derived-compatible repr or list them among the float-hashed group).
   - Update `StyleView::margin_*` accessors to return `Margin`. Add helpers:
     `margin_left_is_auto()`, etc. Keep `get_horizontal_margin(cb_width)` using
     `Margin::resolve` (auto→0) — its callers (width available calc) treat auto
     as 0, which is correct for the *available width* computation.
2. **`resolver.rs`:** `length_or_auto_to_lengthpct` for margins → new
   `length_or_auto_to_margin` returning `Margin::Auto` on
   `LengthPercentageOrAuto::Auto`. Padding keeps `LengthPct` (auto is invalid for
   padding; treat as 0). Update the margin arms (lines 494–529); leave padding
   arms on the `LengthPct` path.
3. **`block_layout.rs` — centering.** In `layout_block_children`, the child's
   used border-box width must be known *before* setting its point (the subtree
   is positioned during `layout_box`). Extract the width computation so the
   parent can pre-compute it:
   - Factor the width math out of `box_layout.rs::resolve_content_width` +
     padding/border into a helper `used_border_box_width(box_idx, available_width)`
     usable from the parent.
   - For each in-flow block child: if `width` is definite (not `Auto`) and
     leftover `= available_width - used_border_box_width > 0`:
     - both margins auto → `x += leftover / 2`
     - left auto only → `x += leftover`
     - right auto only / none → unchanged.
   - Replaced images (`is_image_box`) have a definite intrinsic width, so
     `display:block; margin:0 auto` on an image should center too — treat the
     scaled image width as the definite width in `used_border_box_width`.
4. Keep the existing `margin_left.resolve()` fallback for the non-auto path.

## Edge cases
- `width:auto` + `margin:auto` → margins compute to 0 (no centering); block
  fills available width. Matches browsers.
- Negative leftover (child wider than container) → no shift.
- Only meaningful for in-flow block-level boxes; skip floats and inline.

## Tests
- `<div style="width:100px;margin:0 auto">` in a 500px column → box x ≈ 200.
- `<img style="display:block;width:100px;margin:0 auto">` centers.
- `width:auto;margin:0 auto` → x = 0, fills width.
- `margin-left:auto` only, definite width → right-aligned.

## Effort / risk
Medium. The `Margin` type touches the `BoxModel` dedup key and every margin
accessor call site. Centering itself is localized to `block_layout.rs`. Risk:
the two-phase "width before position" extraction — verify the extracted
`used_border_box_width` matches what `layout_box` ultimately sets.
