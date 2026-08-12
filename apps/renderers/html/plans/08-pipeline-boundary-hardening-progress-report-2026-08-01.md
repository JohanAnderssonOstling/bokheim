# Plan 08 Progress Report — 2026-08-01

## Scope

- Report the current status of milestones 8a–8f in `08-pipeline-boundary-hardening.md` after the latest migrations.
- Confirm where the render boundary is still coupled to layout internals.
- List concrete follow-up work to reach 100% for milestone 8d and enable 8e.

## Milestone status snapshot

- 8a Glyph validation: **~85–90% complete**
  - `GlyphMetric::try_new` rejects non-finite and invalid advance values.
  - Negative `advance` has been explicitly addressed and rejected.
  - Remaining risk: verify full boundary coverage across `ShapeError` variants and registry mutation paths.

- 8b Style validation: **~90% complete**
  - Value validation exists in builder and finish paths.
  - `ComputedStylesBuilder::push` path is fallible with style-value checks.
  - Finish-time defense-in-depth still present.

- 8c Sealed DOM builder: **~65% complete**
  - Typed `DocumentBuilder<NeedsRoot>/<Rooted>` and stricter root-state transitions are present.
  - `set_dom_root` state progression is enforced.
  - Full builder acceptance coverage remains incomplete (negative/foreign IDs, cycles, sparse test cases).

- 8d Render contract migration: **~90% complete**
  - `LaidOutDocument::render_view()` exists and exposes borrowed, allocation-light accessors.
  - `selection`, `highlight`, `interaction`, `render`, `nav`, and `load` paths now use render-contract access.

- 8e Hide mutable layout representations: **~80–85% complete**
  - Contract types are available on the layout boundary.
  - Public raw `LaidOutDocument` storage accessors were demoted to `pub(crate)` and remaining callers moved to `RenderView`.
  - Renderer no longer uses mutable-like `LaidOutDocument` storage APIs; public surface hardening is now largely complete on the layout stage.

- 8f Transition policy: **~90% complete**
  - Phase ordering is explicit in types and validation paths remain in place.

## Evidence from current code

- Contract facade implemented
  - [`apps/renderers/html/crates/html-layout/src/stages.rs`](apps/renderers/html/crates/html-layout/src/stages.rs): defines `RenderView`, `RenderLine`, `RenderTextRun`, `RenderImageFragment`, `RenderDecorations`, `RenderLines`, `RenderTextRuns` and accessor methods.
  - [`apps/renderers/html/crates/html-layout/src/lib.rs`](apps/renderers/html/crates/html-layout/src/lib.rs): re-export list no longer includes `Line`, `InlineRun`, `InlineKind`, etc.

- Partial migration completed
  - [`apps/renderers/html/crates/html-view-core/src/renderer/selection.rs`](apps/renderers/html/crates/html-view-core/src/renderer/selection.rs)
  - [`apps/renderers/html/crates/html-view-core/src/renderer/highlight.rs`](apps/renderers/html/crates/html-view-core/src/renderer/highlight.rs)
  - [`apps/renderers/html/crates/html-view-core/src/renderer/interaction.rs`](apps/renderers/html/crates/html-view-core/src/renderer/interaction.rs)
  - [`apps/renderers/html/crates/html-view-core/src/load.rs`](apps/renderers/html/crates/html-view-core/src/load.rs) (tests)

- Remaining boundary coupling (important)
  - No unresolved 8d storage-coupled paths remain in `html-view-core`.

## Current assessment against the external checkpoint

- The external checkpoint estimate is now improved to **~78–82%**.
- Major remaining risk is acceptance coverage and preventing future boundary drift.

## Immediate next steps

1. Keep the boundary checks script aligned with the new hardening targets as part of CI.
2. Expand acceptance tests in 8c/8e around impossible states and boundary regressions.
