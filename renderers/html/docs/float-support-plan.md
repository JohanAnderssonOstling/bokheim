# Plan: `float` / `clear` support for static EPUB

Status: proposed (not yet started)
Target: reflowable EPUB content only (no scripting, single-pass per-chapter layout).

## Goal & scope

Cover the three float idioms that actually appear in EPUB content, in priority
order:

1. **Drop caps** — `float:left` on a leading inline element
   (`<span class="dropcap">T</span>`) or a leading image inside a paragraph;
   the following lines wrap to its right.
2. **Floated figures/images** — `<img style="float:right">` / `<figure>` as a
   block sibling; subsequent text wraps for the float's height.
3. **`clear`** — a block pushed below preceding floats.

Out of scope (explicit follow-ups):

- `::first-letter` pseudo-element drop caps (no pseudo-element matching exists).
- Float interaction with multi-column pagination beyond the common
  single-column case.
- Full CSS 2.1 float-stacking rules for many adjacent floats.

### Relevant prior finding

lightningcss (alpha.71, current lockfile) has **no first-class `Float`/`Clear`
property** — they arrive as `Property::Unparsed`, exactly like
`border-collapse` / `empty-cells` / `caption-side`, which are already parsed via
`first_ident_token` in `style/resolver.rs` (~lines 673–702). Style plumbing
therefore follows an existing pattern.

## Core design: a float exclusion context keyed on absolute Y

Key enabling fact: every box is positioned in a single continuous **document
coordinate space** (root at `0,0`; lines emitted at absolute `position.y + y` in
`inline.rs`). So float regions can be stored as absolute-Y rectangles and
queried directly by any line or block being placed — no per-block coordinate
translation.

New module `layout/float_context.rs`:

```rust
struct FloatBand { left: f64, right: f64, top: f64, bottom: f64 } // absolute coords
struct FloatContext { lefts: Vec<FloatBand>, rights: Vec<FloatBand> }
```

Methods:

- `available(y, height) -> (left_edge, right_edge)` — narrowest band overlapping
  `[y, y+height)`.
- `place_left(width, height, top, container_left, container_right) -> Point`
  and `place_right(...)` — find the lowest Y at/below `top` where the float
  fits, return its origin and push the band.
- `clear_to(y, side) -> f64` — lowest Y below all bands on `side`.
- `lowest_bottom() -> f64` — for container containment (clearfix).

Store a **stack** on `LayoutContext`: `float_stack: Vec<FloatContext>`. A new
context is pushed when a box establishes a block formatting context. For EPUB
simplicity, establish a BFC at: the **root box**, **table cells**, and **each
float's own contents**. Body-level floats therefore share the root BFC, which is
what makes wrap-around-siblings work.

## Phases

### Phase 0 — Style plumbing (no visual change; independently testable)

Files: `document/computed_style.rs`, `style/resolver.rs`.

- Add `Float { None, Left, Right }` and `Clear { None, Left, Right, Both }`
  enums (same `Eq`/`Hash` derive as the other box enums).
- Add `float: Float` and `clear: Clear` to `BoxModel`; update
  `impl_style_key!(BoxModel …)`, `Default for BoxModel`, `DEFAULT_BOX_MODEL`, and
  add `StyleView::float()` / `clear()` accessors.
- In `resolver.rs`: add `"float"` / `"clear"` to `is_supported_property_name`,
  and parse them in the `Property::Unparsed` arm of `apply_property` via
  `first_ident_token` (mirror the `border-collapse` block).
- **Test:** `float:left` / `clear:both` round-trip to the computed `BoxModel`.

### Phase 1 — Box generation: blockify and hoist floats

File: `layout/box_builder.rs`.

- A floated element is **block-level regardless of `display`** (CSS §9.7
  blockification) and is **shrink-to-fit**. In `build_box_for_element`, if
  `float != None`, force `LayoutMode::Block`.
- Read `float`/`clear` from the box's `StyleView` at layout time (no new
  `LayoutBox` field needed).
- **Hoisting inline-origin floats (drop caps):** in `process_inline_node`, when
  an inline child has `float != None`, do **not** emit it as an `InlineRun`.
  Build a block float box and record a *pending float* anchored at the current
  inline run index, via a new `InlineKind::FloatAnchor(box_idx)` placeholder run
  so inline layout knows where the float enters the stream (≈ index 0 for drop
  caps).
- **Tests:** floated `<span>` produces a block box with `Float::Left` and a
  `FloatAnchor` run (not text runs in the parent stream); floated block sibling
  produces a normal block box flagged as float.

> Cost note: adding an `InlineKind` variant forces edits at every exhaustive
> `match run.kind` — `build_inline_tokens`, `summarize_token_span`, shaping
> `text_runs`, `get_dom_node_for_glyph`, etc. Each is a one-line arm, but missing
> the *semantics* in one causes silent glyph/selection misalignment.

### Phase 2 — Placement + inline wrap (covers figures *and* drop caps)

Files: `layout/box_layout.rs`, `layout/block_layout.rs`, `layout/inline.rs`,
`layout/engine.rs`.

**BFC setup** (`layout_box`): when the box establishes a BFC,
`float_stack.push(FloatContext::default())` before laying out children and pop
after (optionally growing the box to `lowest_bottom()` — see Phase 3).

**Block-level floats** (`layout_block_children`): when a child has
`float != None`:

- Lay it out shrink-to-fit (`fills_available_width = false`, so
  `used_content_width = content_size.width`).
- `float_ctx.place_left/right(...)` → origin; `set_box_point`. **Do not** advance
  `y_offset` and **do not** feed it into margin collapsing — it is out of normal
  flow.
- Register its band.

Normal (non-float) children then read
`float_ctx.available(absolute_y, line_height)` to offset their box position /
shorten their content width.

**Inline wrap** (`layout_runs` / `break_lines` / `emit_lines`) — the substantive
change:

- Keep the clean fast path: **if the active `FloatContext` has no band
  overlapping this inline context's Y-range and there is no `FloatAnchor`, run
  today's code unchanged.**
- Float-aware path: line breaking currently defers metrics to `emit_lines`, but
  wrapping needs each line's *height* (hence its Y-band) **before** breaking the
  next line. Resolve by **interleaving**: choose a tentative break for the
  current line at the current band width, measure it (reuse `measure_line` for
  `line_height`), commit it at the float-adjusted `x_offset`/width, advance `y`,
  then query `float_ctx.available(y, next_height_estimate)` for the next line. A
  `FloatAnchor` at line start is placed first (`place_left/right` at the current
  Y), its band registered, then the remaining tokens wrap.
- Drop cap falls out naturally: the floated letter is placed at the paragraph's
  top-left; the next ~N lines see a reduced left edge until `y` passes the float
  bottom, then full width returns.

**Tests:** golden line-geometry tests —
(a) `float:left` image then paragraph: first lines start at `image_width`, lines
below the image start at 0;
(b) drop-cap span: first N lines indented by the cap width, line N+1 full width;
(c) `float:right`: line widths shortened on the right, `x` unchanged.

### Phase 3 — `clear`, containment, paint, pagination

- **`clear`** (`layout_block_children`): before placing a block with
  `clear != None`, advance `y_offset` so its top is at
  `float_ctx.clear_to(absolute_y, side)`.
- **Containment:** at BFC pop, grow the establishing box's height to include
  `lowest_bottom()` (clearfix case where a float is taller than in-flow content).
- **Paint / pagination interaction (must verify):** the renderer
  (`renderer/mod.rs`, `render.rs`) re-flows continuous-Y lines into columns at
  paint time. Float boxes are emitted as block decorations / `ImageFragment`s in
  continuous Y, so they must go through the **same column remap** as lines.
  Confirm float-box fragments are remapped; a float taller than the column is a
  documented limitation, not a crash. Single-column reading (dominant EPUB mode)
  is straightforward.

## Risks / tradeoffs

- **Interleaved measure-in-break (Phase 2b) is the main complexity** and ~60% of
  the real risk. It folds measurement back into the break loop and interacts with
  trailing-whitespace trimming, justification (`compute_justification`), and the
  soft/hard-break state machine — all currently isolated. Mitigated by keeping
  the no-float fast path byte-for-byte unchanged.
- **Multi-column + tall floats:** explicitly limited; acceptable for EPUB.
- **General mid-line float anchoring** is structurally supported via
  `FloatAnchor` but only well-tested for the leading-anchor case; deprioritized.
- All changes are additive: a chapter with no floats exercises none of the new
  paths, so regression surface on existing content is near zero.

## What keeps this tractable (vs. a browser)

- No-float content path untouched.
- Continuous-Y coordinates — floats and lines share one space, no translation.
- Single-pass, immutable, per-chapter layout — floats never have to work under
  incremental reflow or DOM mutation.
- Margin collapsing is already implemented and floats are *out* of that flow, so
  they don't perturb it.

## Complexity estimate

| Phase | LOC | Difficulty | Cost driver |
|---|---|---|---|
| 0 — Style | ~60–100 | Trivial | Boilerplate, follows `border-collapse` |
| 1 — Box gen + hoist | ~150–250 | Moderate | New `InlineKind` variant ⇒ edits at every match |
| 2a — `FloatContext` + figures | ~200–300 | Moderate | Self-contained; geometry straightforward |
| 2b — Inline wrap | ~250–400 | **Hard** | Breaks break→measure→emit separation |
| 3 — clear / containment / paint | ~150–250 | Moderate + 1 unknown | Pagination remap unverified |

**Total ≈ 800–1300 LOC across ~8–10 files**, concentrated in
`layout/inline.rs`, `layout/box_layout.rs`, `layout/block_layout.rs`,
`layout/box_builder.rs`, a new `layout/float_context.rs`, and the style files.

Effort for someone fluent in the codebase: **~3–6 focused days** (Phases 0–1 well
under a day combined; 2a ≈ a day; **2b is the multi-day item**; 3 ≈ a day plus
whatever the pagination check uncovers).

## Suggested commit breakdown

1. Phase 0 (style) — small, isolated.
2. Phase 1 (box gen + hoist) — verify via box-tree assertions.
3. Phase 2a — `FloatContext` + block-sibling figures.
4. Phase 2b — inline-wrap float-aware path + drop caps.
5. Phase 3 — clear / containment / paint + pagination.

## De-risking option

Phase 0–2 scope (drop caps + figures, skip clear/containment) captures most of
the visible EPUB value, sidesteps the Phase 3 pagination unknown, and still
front-loads the hard Phase 2b work where it can be validated with golden
line-geometry tests before committing to the rest.

## Open questions (resolve before implementing)

1. **First-pass scope:** drop caps + figures (Ph 0–2), full plan (Ph 0–3), or
   just plumbing (Ph 0–1)?
2. **Drop-cap authoring in target books:** explicit element
   (`<span class="dropcap">`) — handled by the hoisting path — vs. `::first-letter`
   — needs pseudo-element matching, currently out of scope.
