# Plan 07 — Fragmentation: `break-before/after`, `break-inside: avoid`, widows/orphans

## Goal
Honor CSS fragmentation in the column paginator: start chapters/sections on a
fresh column (`break-before: page/column: always`), keep figures and headings
from splitting across a column boundary (`break-inside: avoid`), and keep
minimum line counts at column edges (`widows`/`orphans`). This is the
paged-media model — directly relevant because the renderer already paginates
into columns.

## Current behavior (the gap)
- `break-*` / `page-break-*` are parsed then dropped as unsupported
  (`src/style/resolver.rs:678`, early `return`).
- Pagination is purely geometric: `HTMLRenderer::resolve_point_at`
  (`src/renderer/render.rs:21`) maps each document Y to a column, advancing to
  the next column only when a single element would overflow the column height.
- `Line` (`src/document/layout_state.rs:23`) carries `glyphs/point/height/
  baseline/spacing` — **no box identity and no break flags**, so the paginator
  cannot know where a block starts/ends or whether a break is requested.

The core problem is that fragmentation needs *box structure* at paint time, but
only flat lines survive into the renderer. So most of this plan is about
threading minimal break metadata from style → layout → the line list.

## Design overview
Three phases, shippable in order: 7a (forced breaks), 7b (`break-inside:avoid`),
7c (widows/orphans).

### Style plumbing (shared prerequisite)
1. `resolver.rs`: replace the unsupported early-return (`:678`) with real
   parsing into new `BoxModel` fields:
   - `break_before: BreakValue`, `break_after: BreakValue`
     (`enum BreakValue { Auto, Always }`; map `page`/`column`/`always`/`left`/
     `right` → `Always`, else `Auto`). Treat legacy `page-break-*` as aliases.
   - `break_inside_avoid: bool` (from `break-inside: avoid` / `avoid-page` /
     `avoid-column`).
   - `widows: u8`, `orphans: u8` (default 2, per CSS).
   - Add to `impl_style_key!(BoxModel …)` and defaults.

### Layout → line metadata
2. Annotate lines with the break info needed at paint time. Add to `Line`:
   - `break_before: bool` (set on the **first** line emitted for a box whose
     style has `break_before == Always`).
   - `group_id: u32` (the box index — or an id assigned per block — shared by
     all lines belonging to the same `break-inside:avoid` block, so the
     paginator can treat them atomically). `0`/`u32::MAX` = none.
   These are populated where lines are pushed (`inline.rs::write_line_fragments`
   via the owning box, and in `block_layout`/`box_layout` where a block's line
   range is known). The `sort_lines_and_remap_images` pass in
   `engine.rs:70` already reorders lines by Y — keep the flags attached to the
   `Line` so they survive the sort.

## 7a — Forced breaks (`break-before/after: always`)
Smallest, highest value (chapter starts).
- In the forward paint loop (where `resolve_point_at` is called per line), before
  placing a line whose `break_before` is set and which is **not** already at the
  top of a column, force a column advance: push `render_state.y_offset` by the
  remaining column height (same mechanism the overflow branch uses at
  `render.rs:27-32`) and bump `col_index`.
- `break-after: Always` on a box = `break_before` on the next box's first line;
  simplest to normalize at annotation time (set the following line's
  `break_before`).
- Backward paint (`paint_backward`) must apply the symmetric rule so paging back
  lands consistently.

## 7b — `break-inside: avoid`
- Group lines by `group_id`. Before placing the first line of a group, compute
  the group's total height (sum of member line heights up to the next forced
  break). If it does not fit in the remaining column space **and** it fits in a
  full column, force a column advance first (push the whole group down).
- Cap the behavior: if a group is taller than a full column, give up and let it
  split (avoid infinite push). Figures/headings are small, so this is rare.
- Nested avoids: use the outermost group id; do not try to satisfy conflicting
  nested constraints in v1.

## 7c — Widows / orphans
- Orphans: when a block's lines start near the column bottom and fewer than
  `orphans` lines fit, push the whole block start to the next column.
- Widows: when a block would leave fewer than `widows` lines at the top of the
  next column, pull an extra line forward by breaking earlier (advance the break
  point up by `widows - leftover` lines).
- Implement as an adjustment in the same per-line placement loop, using the
  block's line range (needs `group_id` or a block→line-range map from layout).

## Steps (ordered)
1. Style fields + parsing (`resolver.rs`, `computed_style.rs`). Prerequisite.
2. `Line` fields `break_before`, `group_id` + populate during emission
   (`inline.rs`, `block_layout.rs`/`box_layout.rs`); preserve through
   `engine.rs::sort_lines_and_remap_images`.
3. 7a forced-break advance in the forward and backward paint loops
   (`render.rs`, and `renderer/mod.rs` paint entry points).
4. 7b group-fit advance.
5. 7c widow/orphan adjustment.

## Edge cases
- A forced break on the very first line of the document/chapter → no-op (already
  column top).
- Interaction with floats: a break should also reset/clear active float context
  for the new column (coordinate with the float implementation).
- `render_current_page_to_png` shares the paint path, so PNG export inherits the
  behavior automatically.

## Tests
- `h2 { break-before: page }` → the `<h2>` starts column 0 of a fresh page even
  if space remained.
- `figure { break-inside: avoid }` shorter than a column never splits across the
  column boundary.
- `p { orphans: 3 }` with 2 lines fitting at column bottom → whole paragraph
  moves to next column.
- Backward paging over a forced break lands on the same boundary as forward.

## Effort / risk
Largest of the set. The risky part is threading `group_id`/`break_before` onto
`Line` and keeping it correct through the Y-sort and through both paint
directions. Recommend landing **7a alone first** (chapter breaks — high value,
contained), then 7b, then 7c. Widows/orphans (7c) are polish and can wait.
