# Plan 29: Flat glyph-metrics table (frees layout from the glyph cache)

First step of the HtmlRenderer decoupling program (29 → 30 → 31 → 32). Load-bearing: it
fixes the signatures that plans 30–32 build on, so do it first and do not re-open it later.

**Prerequisite:** commit the in-flight HtmlRenderer refactor before starting. At the time of
writing there are 44 modified files under `HtmlRenderer/` on top of a `Checkpoint working
tree` commit; this plan and 30–32 touch many of the same files.

## Problem

`LoadedDocument` owns a concrete `GlyphCache` (`document/loaded.rs:13`), and `GlyphCache`
shapes through `floem::text::{Attrs, AttrsList, TextLayout}` (`text/glyph_cache.rs:3-4`).
That single field is the only thing tying `document/` and `layout/` to floem's text stack:

- `document/` → `text/` is one line (`loaded.rs:2`).
- `layout/` → `text/` in production is only `GlyphCache` (`engine.rs:3`, `inline.rs:11`);
  every `shape_document` reference in `layout/` is inside a `#[cfg(test)] mod tests`.

What layout actually consumes is four numbers per glyph: char, advance, ascent/descent, and
baseline offset (`inline.rs:546-549`, `inline.rs:813`, `table_layout.rs:766,782`). Only the
painter needs a real `TextLayout`.

## Decision

Use a **flat metrics table**, not a `dyn GlyphMetrics` trait and not a generic
`LoadedDocument<G>`. Rationale:

- `inline.rs:546-549` runs per glyph inside line breaking; Knuth-Plass re-reads it. Virtual
  dispatch there is on the hot path.
- A generic parameter would go viral through every signature in dom, layout, and renderer.
- Metrics are immutable once shaped and already keyed by a dense `GlyphId`, so a `Vec` indexed
  by `GlyphId` is the natural shape — and it has *better* locality than today's
  `Vec<(char, TextLayout, GlyphMetrics)>` (`glyph_cache.rs:68`), which drags a whole
  `TextLayout` through every metric read.

Keep a trait in reserve only if something later needs lazy/on-demand shaping. It does not
today.

## Change

1. **New type in `document/`** (lands in `html-dom` at plan 32):

   ```rust
   #[derive(Clone, Copy)]
   pub struct GlyphMetric { pub ch: char, pub advance: f32, pub ascent: f32, pub descent: f32, pub baseline_offset: f32 }

   #[derive(Default)]
   pub struct GlyphMetrics(Vec<GlyphMetric>);   // indexed by GlyphId
   ```

   ~20 bytes/glyph. Give it the same `memory_usage_report()` treatment as its neighbours so
   `bin/memory_profile.rs` keeps working.

2. **`GlyphCache` publishes into it.** `shape_with_font_properties` and `get_or_insert`
   (`glyph_cache.rs:96,135`) already compute `GlyphMetrics` from the layout run
   (`metrics_from_layout`, `glyph_cache.rs:82`) — push a `GlyphMetric` onto the table as each
   glyph is interned. The cache keeps its `TextLayout` vector for painting.

3. **`LoadedDocument` swaps the field.** `glyph_cache: GlyphCache` → `glyph_metrics:
   GlyphMetrics`. Replace the five accessors (`loaded.rs:53-74`) with
   `glyph_metrics()` / `glyph_metrics_mut()` / `take` / `replace`. `document/` then has zero
   references to `text/`.

4. **Layout reads the table.** `LayoutContext` (`layout/engine.rs:3`) and `inline.rs:11` take
   `&GlyphMetrics`. Rewrite the four consuming sites:
   - `inline.rs:546-549` — one indexed read yields ch + advance + ascent/descent.
   - `inline.rs:813`, `table_layout.rs:766` — `.ch`.
   - `table_layout.rs:782`, `inline.rs:548` — `.advance` replaces
     `glyph_cache.get(idx).size().width`.
   - `inline.rs:736` — `.baseline_offset`.

   After this, `layout/` has no production reference to `text/` at all.

5. **Renderer keeps the cache.** `renderer/` still holds `GlyphCache` for painting
   (`mod.rs:18,390-404`, `render.rs`, `selection.rs`, `highlight.rs`, `view.rs`). Sites that
   only want the char (`selection.rs` ×7, `highlight.rs`, `load.rs`, `render.rs` ×2) should
   read the metrics table instead, so that after plan 30 the *only* remaining use of the cache
   is glyph lookup for `draw_glyph`.

6. **Tests.** `layout/` and `style/` test modules construct a `GlyphCache` directly
   (`inline.rs:938,1224`, `box_layout.rs:224`); they now build a cache, shape, and pass
   `&GlyphMetrics` into layout. Keep them in place — plan 32 settles the cycle question.

## Acceptance

- `grep -rn 'crate::text' HtmlRenderer/src/document HtmlRenderer/src/layout` returns only
  `#[cfg(test)]` hits and the doc-comment at `document/inline.rs:7`.
- All 42 layout + 28 style tests pass unchanged in behavior.
- No measurable regression in `bin/memory_profile.rs` totals (the table replaces per-glyph
  `GlyphMetrics` already stored in the cache's tuple, so expect a small net win).
