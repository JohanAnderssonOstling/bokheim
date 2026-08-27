# Plan 08 — 8e hardening continuation report (2026-08-01c)

## Scope

- Continue closing milestone 8e (hide layout internals) by removing remaining
  renderer-facing leaks and replacing internal layout types in public-like APIs
  with contract wrappers.

## Changes made

1. `apps/renderers/html/Cargo.toml`
- Removed direct dependency on `html-layout-model` from the top-level renderer crate.

2. `apps/renderers/html/src/lib.rs`
- Replaced `pub use html_layout_model::*` document re-export with
  `pub(crate)` imports of only renderer-internal metric/types required by this crate.
- This prevents renderer public API consumers from being able to reach layout-model
  types through the renderer adapter that existed at the time.

3. `apps/renderers/html/src/renderer/painter.rs`
- Replaced `html_layout_model::GlyphId` usage with `crate::document::GlyphId`.
- Glyph ID source remains `html_layout` contract output.

4. `apps/renderers/html/crates/html-layout/src/lib.rs`
- Removed internal `AnchorPosition` and `ListItemMarker` from public re-exports.
- Kept `GlyphId`, `GlyphMetric`, `GlyphMetrics` as stable contract-facing
  primitives.

5. `apps/renderers/html/crates/html-layout/src/stages.rs`
- Added `RenderListItemMarker` contract wrapper (`marker_box()`, `position()`).
- Changed `LaidOutDocument::list_marker()` to return `Option<RenderListItemMarker>`.
- Kept internal mapping from `ListItemMarker` local to layout crate.
- Updated `RenderAnchorPosition` accessors to borrow `&self` (safer copy semantics
  for downstream iteration).

6. `apps/renderers/html/crates/html-view-core/src/load.rs`
- Updated list-marker tests/usages to the new wrapper accessor API:
  `marker.marker_box()` and `marker.position()`.

7. `apps/renderers/html/crates/html-view-core/src/renderer/nav.rs`
- Updated anchor-position access to call `pos.y()` / `pos.order()` accessors.
- Updated iteration to `anchor_positions().iter()` after introducing accessor-only
  contract iterator interface.

## Verification

- `bash apps/renderers/html/scripts/check-html-boundaries.sh` → `HTML crate boundaries are valid`
- `cargo check -p html-view-core -p html-layout --quiet` → success (warnings only)

## Current completion view

- 8e is improved but not complete:
  - public renderer crate `html_layout_model` leakage has been removed;
  - major list-marker leakage is now contractized.
- Remaining gap remains for full Plan 08:
  - 8c acceptance test breadth and some transition-time validation coverage.
