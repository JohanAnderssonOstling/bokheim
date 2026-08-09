# Plan 08 Implementation Report — 2026-08-01 (Session follow-up)

## Scope

This report captures the latest Plan 08 hardening changes that landed after the 8e
representation-isolation work on this date.

## What was implemented

1. Completed the `html-layout` internal-migration step for mutable layout types:
   - `HtmlRenderer/crates/html-layout/src/lib.rs`
   - `HtmlRenderer/crates/html-layout/src/layout_model/`
   - Inlined model modules were added under `html-layout/src/layout_model`:
     - `glyph_metrics.rs`
     - `inline.rs`
     - `layout_state.rs`
     - `layout_tree.rs`
   - Removed the direct dependency on `html-layout-model` from
     `HtmlRenderer/crates/html-layout/Cargo.toml`.
   - Updated `stages.rs`/`shaping.rs`/`document` module imports to source types
     from `crate::layout_model` instead of `html_layout_model`.

2. Kept the render boundary contract stable while avoiding allocation in
   `RenderView`:
   - Kept `RenderView` APIs backed by borrowed slices/iterators.
   - Fixed list-marker mapping mismatch in
     `HtmlRenderer/crates/html-layout/src/stages.rs`:
     - `RenderView::list_marker()` now returns `LaidOutDocument::list_marker()` directly.
   - Fixed ancestor iterator index-type mismatch:
     - `AncestorIter` now tracks `usize` consistently and maps parent links from
       internal `u32` box ids to `usize` once per step.
   - Fixed missing import in
     `HtmlRenderer/crates/html-layout/src/layout/decorations.rs` by adding
     `InlineKind`.

3. Updated boundary enforcement:
   - `HtmlRenderer/scripts/check-html-boundaries.sh`
   - Removed `html-layout-model` from the allowed direct dependency list for
     `html-layout`; dependency now remains `html-dom` and `html-style-model`
     only.

## Milestone status after this session

- 8a Glyph validation: **~90% complete**
  - `GlyphMetric::try_new` enforces finite/non-negative constraints for relevant
    fields.
  - `GlyphMetrics` append-only registry is present and validated in shaper boundary.
- 8b Style validation: **~90% complete**
  - Existing style-builder runtime checks remain in place; value-category hardening
    still being expanded.
- 8c Sealed DOM builder: **~70% complete**
  - Builder-state typing and root/lineage validation work continues; public mutation
    surfaces are reduced.
- 8d Render contract: **~95% complete**
  - Core renderer consumers are now on `RenderView` contract methods.
- 8e Hide layout internals: **~90% complete**
  - Mutable layout internals were moved into `html-layout` private modules and no
    longer flow through `html-layout-model` in `html-layout` codepaths.
  - Remaining work: retire/contain the standalone `html-layout-model` crate and
    lock down any external usage.
- 8f Transition policy: **~90% complete**
  - Phase ordering and lineage checks remain encoded and active.

## Known blockers / next actions

1. Validate the crate graph and compile after this migration by running:
   - `bash HtmlRenderer/scripts/check-html-boundaries.sh`
   - `cargo check -p html-layout` (or the equivalent workspace layout checks)
2. Remove `html-layout-model` as a public dependency surface in the remaining
   downstream migration and verify no downstream crate references its mutable
   types.
3. Add/extend compile-fail + boundary tests for:
   - `Ancestors` iterator parent-id conversions
   - public boundary APIs on `LaidOutDocument`/`RenderView`
   - invalid legacy imports through removed layout dependency paths.

## Evidence written by this pass

- `HtmlRenderer/crates/html-layout/src/lib.rs`
- `HtmlRenderer/crates/html-layout/src/layout_model/*`
- `HtmlRenderer/crates/html-layout/Cargo.toml`
- `HtmlRenderer/crates/html-layout/src/stages.rs`
- `HtmlRenderer/crates/html-layout/src/layout/decorations.rs`
- `HtmlRenderer/scripts/check-html-boundaries.sh`
- `HtmlRenderer/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01h.md`
