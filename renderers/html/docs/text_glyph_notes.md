# Text Rendering Notes (Floem/Floem-Renderer)

## How `draw_text` works

- `PaintCx::draw_text` calls `floem_renderer::Renderer::draw_text(layout, pos)`.
- The renderer backends iterate `TextLayout::layout_runs()` and draw glyphs from those runs.
- In Vello, `draw_text` groups glyphs by `(font_id, font_size, color, metadata)` and then calls a lower-level glyph API (`draw_glyph_run`) that uses `scene.draw_glyphs` with `glyph_id`, `x`, `y`.

Relevant sources:
- `$CARGO_HOME/registry/src/.../floem_renderer-0.2.0/src/lib.rs`
- `$CARGO_HOME/registry/src/.../floem_vello_renderer-0.2.0/src/lib.rs`

## What `TextLayout` contains

- `TextLayout` wraps a `cosmic_text::Buffer` and optional width/height bounds.
- `TextLayout::set_text` builds `BufferLine`s and shapes using `FONT_SYSTEM` (`shape_until_scroll`).
- `layout_runs()` returns `LayoutRun` objects containing `&[LayoutGlyph]`.

Relevant sources:
- `$CARGO_HOME/registry/src/.../floem_renderer-0.2.0/src/text/layout.rs`
- `$CARGO_HOME/registry/src/.../cosmic-text-0.12.1/src/layout.rs`

## Minimal glyph representation for rendering

You can render with a minimal payload if you keep run-level info separate.

Per run:
- `font_id`
- `font_size`
- `color`/brush
- optional `metadata`
- transform/baseline offset (can bake into `y` or keep per-line `line_y`)

Per glyph:
- `glyph_id`
- `x`
- `y`

This is effectively what Vello does internally in `draw_glyph_run`.

## Batching strategy

Batch glyphs by a run key that matches backend draw state:
- `(font_id, font_size, color/brush, transform, hinting/aa flags)`

For each batch:
- Append glyphs into a single `Vec<Glyph { id, x, y }>`
- Submit a single draw call (e.g., Vello `scene.draw_glyphs`)

You cannot mix glyphs with different run-level properties in a single Vello `draw_glyphs` call.

## High-level changes to external libraries

### 1) Expose low-level shaping output

Add a public shaping API in `floem_renderer::text` that returns glyph runs without requiring a `TextLayout`.
Internally this can still use `TextLayout::set_text()` + `layout_runs()` but return only minimal glyph data.

Example output type:
- `Vec<GlyphRun>` where `GlyphRun` contains run-level state + `Vec<Glyph { id, x, y }>`.

### 2) Add a renderer-level glyph draw API

Extend `floem_renderer::Renderer` (and `PaintCx`) with something like:
- `draw_glyphs(run: &GlyphRun, pos: Point)`

### 3) Implement per backend

- **Vello:** reuse `draw_glyph_run` logic.
- **TinySkia/Vger:** refactor existing `draw_text` to accept glyph runs directly.

### 4) Keep `draw_text` as a fallback

Optionally implement `draw_text` in terms of the new shaping + glyph draw path.

## What “transform” means here

“Transform” is the affine matrix applied to glyph positions before drawing (translate/scale/rotate/skew).

In Vello:
- Start with renderer transform
- Translate by text position (`pos`)
- Scale by HiDPI window scale
- For each line, translate by `line.line_y` before drawing glyphs

Sources:
- `$CARGO_HOME/registry/src/.../floem_vello_renderer-0.2.0/src/lib.rs`

## Memory sizing notes

`TextLayout` has a non-trivial fixed overhead (buffer + line/layout structs + small `Vec`s). A single
character or short word still allocates multiple small vectors and layout objects inside `cosmic_text`.
Expect low-KB heap usage even for 1–8 glyphs; the per-glyph cost is small compared to the baseline.

If you want a precise number on your build, measure `Vec::capacity()` for the internal containers and
sum `capacity * size_of::<T>()` plus the `TextLayout` struct size.

## Glossary

- `TextLayout`: High-level shaped text container. Owns a `cosmic_text::Buffer`, layout ranges, and bounds.
- `LayoutRun`: A visible line/run produced by `TextLayout::layout_runs()`; exposes `&[LayoutGlyph]`.
- `LayoutGlyph`: Shaped glyph data from `cosmic_text` (font id/size, glyph id, x/y, etc.).
- `GlyphRun`: Backend-specific grouping of glyphs by draw state (font, size, color, metadata).
