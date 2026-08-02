# Plan 30: Painter draws by `GlyphId`

Second step of the HtmlRenderer decoupling program (29 → 30 → 31 → 32). Small, and it is what
makes a non-floem paint backend possible at all.

## Problem

`renderer/render.rs` already paints through `impl Painter` rather than `PaintCx` — the
abstraction exists and works. But one floem type leaks through the trait
(`renderer/painter.rs:11`):

```rust
fn draw_text(&mut self, layout: &TextLayout, point: Point);
```

So the paint loop must pull a `&TextLayout` out of the glyph cache and hand it over, which
means every painter implementation must speak floem's text types. `SoftwarePainter::draw_text`
consequently cannot be implemented and is a stub (`painter.rs:59`) — **PNG export silently
renders no text**, which is why layout has to be verified through integration tests rather
than rendered output.

## Change

1. **Invert the lookup.** The paint loop passes the id; the backend resolves it.

   ```rust
   pub trait Painter {
       fn fill_rect(&mut self, rect: Rect, color: Color);
       fn draw_glyph(&mut self, glyph: GlyphId, origin: Point);
       fn draw_image(&mut self, image: &Image, hash: &[u8], rect: Rect);
   }
   ```

   No generics and no associated type — the floem painter captures `&GlyphCache` when it is
   constructed and does the `TextLayout` lookup internally. `Rect`/`Point`/`Color`/`Image`
   come from `kurbo`/`peniko` directly (see plan 32 step 1), so the trait names no floem type.

2. **Update the five call sites** in `render.rs` (`:76`, `:81`, `:287`) — they already have
   the `GlyphId` in hand and are currently calling `glyph_cache().get(...)` first, so this
   deletes code rather than adding it. Also drop the `floem::kurbo::Rect` /
   `floem::peniko::Color` spellings in the signatures at `render.rs:173,253`.

3. **A recording painter for tests.** Add a `RecordingPainter` that pushes
   `(GlyphId, Point)` and `(Rect, Color)` into vectors. Combined with plan 29's metrics table
   it can assert on rendered text and glyph placement with no rasterization and no window —
   this is the payoff, and it should replace the current reliance on PNG inspection.

4. **`SoftwarePainter`: decide its fate.** Either implement `draw_glyph` properly (rasterize
   from the cache via swash, which floem already pulls in at 0.1.19) or delete it along with
   `bin/render_to_png.rs`. Do not leave a third silently-no-op painter in the tree. Default to
   **deleting** unless PNG output is wanted for something other than testing — the recording
   painter covers the test need better.

## Acceptance

- `renderer/painter.rs` names no `floem::text` type.
- A test asserts the visible text of a rendered page via `RecordingPainter` without opening a
  window.
- No painter implementation contains a silently-empty draw method.
