# Plan 08 Implementation Report — 2026-08-01

## Scope

This report captures the latest work on milestone 8d/8e, and checks current boundary leakage against the render contract introduced in Plan 08.

## Work completed since previous report

- Migrated remaining layout-bound navigation/render coupling in `html-view-core` renderers.
- Replaced remaining internal line access in:
  - `HtmlRenderer/crates/html-view-core/src/renderer/render.rs`
  - `HtmlRenderer/crates/html-view-core/src/renderer/nav.rs`
  - `HtmlRenderer/crates/html-view-core/src/session.rs` (test fixture access)
- Removed leftover direct decoration coupling in render path by using `self.document.render_view().decorations()` and `RenderDecoration` accessors.
- Removed a stale unused variable in `draw_line_at` (`offset_iter`).
- Removed remaining public `LaidOutDocument` raw accessors for internals (`inline_runs`, `inline_run`, `lines`, `line`, `line_glyph_offsets`, `decorations`, `image_fragments`, `image_fragments_by_line`) by downgrading them to `pub(crate)` where still needed by internals.
- Updated internal uses in:
  - `HtmlRenderer/crates/html-layout/src/cfi.rs`
  - `HtmlRenderer/crates/html-layout/src/layout/inline.rs`
  so remaining in-crate callers rely on the render view contract and stable view wrappers.

## Concrete before/after in the code

- Direct internal calls removed from nav paths:
  - `document.lines()`
  - `document.line()`
  - field-style layout access (`line.point.y`, `line.glyphs.start/end`, `line.height`, etc.)
- Selection/interaction/highlight pipelines already using render-contract line accessors remain unchanged and continue to pass through `RenderLine`.
- `session.rs` and `nav.rs` tests now derive line metadata from `render_view().line(...)`.

## Current status against Plan 08 checkpoints

- 8a Glyph validation: **~85–90% complete**
  - `GlyphMetric::try_new` and negative advance rejection are in place.
  - Remaining: confirm every shape-path and all error-surface mapping for invalid/foreign registries is fully enforced in downstream error channels.

- 8b Style validation: **~90% complete**
  - Builder validation and `finish()` defense-in-depth exist.
  - Remaining: tighten residual edge paths and finish coverage.

- 8c Sealed DOM builder: **~65% complete**
  - Typed builder progression (`NeedsRoot` / `Rooted`) exists.
  - Remaining: broaden acceptance tests for invalid ID/ownership/cycle conditions.

- 8d Render contract: **~90% complete**
  - `LaidOutDocument::render_view()` + borrowed read APIs are now used by core renderer flows.
  - Remaining risk: confirm no residual coupling in any adapter outside `html-view-core` (e.g. remaining frontend backends in this workspace).

- 8e Hide mutable layout internals: **~80–85% complete**
  - Renderer consumers no longer require concrete `Line`/`ImageFragment`/`InlineKind` usage.
  - Remaining: keep a hardening sweep for any downstream consumers not touched in this pass and tighten check policy if new raw leaks reappear.

## Plan 08 continuation note

The user request to prefer type-level validation over runtime checking has been reflected where feasible in this slice by:

- keeping `DocumentBuilder` phase typing in 8c,
- using typed transition points (`NeedsRoot` -> `Rooted`), and
- continuing to prefer contract-safe read access (`RenderLine`, `RenderImageFragment`, render decorations/runs) over direct struct field reads.

Runtime validation is still used as defense-in-depth where data enters from external inputs or parse results.

## Next actions

1. Finish 8d by scanning all render adapters and test helpers (outside `html-view-core`) for any direct `Line`/`InlineRun`/`ImageFragment` uses.
2. Finish 8e by removing `html-layout-model` mutable surface exposure from public API boundaries.
3. Add/refresh acceptance tests for builder-state invariants and render boundary behavior.
4. Re-run a boundary grep audit after each major migration:
   - `document.lines()/line()` in renderer paths,
   - direct `html-layout-model` type references,
   - raw fragment indexing by container internals.
