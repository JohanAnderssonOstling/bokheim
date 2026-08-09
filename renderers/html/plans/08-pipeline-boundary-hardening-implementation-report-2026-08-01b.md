# Plan 08 — Implementation Report (2026-08-01, follow-up)

## Scope

- Record the latest Plan 08 checkpoint after the last slice of work on glyph validation, DOM builder validation, and boundary script hardening.
- Produce a disk artifact for handoff and next-cycle planning.

## What changed since the prior checkpoint

### 1) `8a` Glyph validation improved (`html-layout-model`)
- File: [`HtmlRenderer/crates/html-layout-model/src/glyph_metrics.rs`](HtmlRenderer/crates/html-layout-model/src/glyph_metrics.rs)
- Added unit tests verifying constructor-level rejection for:
  - `GlyphMetric::try_new` negative ascent
  - `GlyphMetric::try_new` negative descent
  - `GlyphMetric::try_new` NaN advance/ascent/descent/baseline_offset
- Existing negative advance and infinite baseline-offset coverage remains.
- Result: negative/invalid values are rejected at metric construction time, which is used by the shaper pipeline.

### 2) `8c` Sealed DOM builder hardening (`html-dom`)
- File: [`HtmlRenderer/crates/html-dom/src/document/dom.rs`](HtmlRenderer/crates/html-dom/src/document/dom.rs)
- Added builder acceptance tests that now cover:
  - non-element root rejection in `set_dom_root`
  - foreign `DomNodeId` rejection
  - duplicate top-level root rejection at `finish`
  - recursion-cycle detection at `finish`
- Result: key malformed-tree acceptance paths are now exercised and guarded by explicit tests.

### 3) `8d/8e` boundary hardening support (layout boundary checks and exports)
- Files:
  - [`HtmlRenderer/scripts/check-html-boundaries.sh`](HtmlRenderer/scripts/check-html-boundaries.sh)
  - [`HtmlRenderer/crates/html-layout/src/lib.rs`](HtmlRenderer/crates/html-layout/src/lib.rs)
- `check-html-boundaries.sh` now:
  - detects whitespace-indented wildcard re-export of `html_layout_model`
  - flags any `html_layout_model::` usage outside `html-layout`
- `html-layout/src/lib.rs` removed wildcard re-export of `html_layout_model::*` and now uses explicit internal re-exports.
- Result: compile-time API surface no longer advertises the full layout-model internals to downstream crates.

## Current milestone status (approximate)

- 8a Validate glyph registration: **~90%**
  - Constructor enforcement exists for all tested invalid scalar paths in this module.
  - Remaining: full end-to-end `ShapeError` surface audit for every foreign/invalid metric path.

- 8b Style validation: **~90%**
  - Existing builder/finish-time checks still hold.
  - Continue ensuring no residual edge path bypasses value validation.

- 8c Sealed DOM builder: **~70%**
  - Typestate (`NeedsRoot`/`Rooted`) remains a core defense and tests now cover several malformed transitions.
  - Remaining: broaden impossible-state tests and restrict mutable post-build surface (`push_toc_entry`, etc.) as planned.

- 8d Render contract migration: **~90%**
  - Contract-view work from prior slice is still in place.
  - Remaining: verify all remaining adapters outside `html-layout`/`html-view-core` do not re-introduce concrete internal coupling.

- 8e Hide layout internals: **~85%**
  - Wildcard export removed and boundary checks hardened.
  - Remaining: harden `html-layout-model` public API itself and tighten any remaining raw-access leakage if it appears.

- 8f Transition policy: **~90%**
  - Phase ordering and typed transition boundaries remain intact.

## Type-system preference note

- This pass keeps runtime checks where they are structurally necessary (graph cycles/reachability), but moves as much as possible to compile-time via typed builder state transitions and branded IDs.
- The updated DOM tests/guards still support a “fail-closed” behavior at runtime for inputs that cannot be fully expressed through types.

## Residual high-priority items for next pass

1. Complete mutation-surface containment in `html-dom` builder after parse (`push_toc_entry`, unchecked list/resource mutation paths).
2. Expand malformed-ID and cycle tests in `8c` to include additional forged IDs and parent-link edge cases.
3. Carry forward 8a error-surface audit to ensure invalid metrics are caught in all possible shape error branches.
4. Keep boundary script as a required gate after any future dependency reshapes.
