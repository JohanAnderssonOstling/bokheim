# Plan 08 — Implementation report (2026-08-01)

## Objective

Drive Plan 08 toward complete boundary hardening with a stronger “type-first”
preference where practical, while preserving a narrow render contract and
preventing invalid final phase states from leaking across crates.

## Completed this pass

1. 8e hide layout internals: contract tightening completed through renderer-facing APIs
   - Removed direct `html-layout-model` dependency/export from top-level renderer crate:
     - `HtmlRenderer/Cargo.toml`
     - `HtmlRenderer/src/lib.rs`
   - Removed direct internal `html_layout_model` type leakage from `html_renderer` public exports and converted renderer calls to contract types:
     - `HtmlRenderer/crates/html-layout/src/lib.rs`
     - `HtmlRenderer/crates/html-layout/src/stages.rs`
     - `HtmlRenderer/src/renderer/painter.rs`
   - Migrated list-marker and anchor-position accesses in renderer consumers to façade accessors:
     - `HtmlRenderer/crates/html-layout/src/stages.rs`
     - `HtmlRenderer/crates/html-view-core/src/load.rs`
     - `HtmlRenderer/crates/html-view-core/src/renderer/nav.rs`
   - Added wrapper contract type:
     - `RenderListItemMarker` with `marker_box()` and `position()` accessors.
   - `RenderAnchorPosition` now uses accessor-style methods for contract use (`y()`, `order()`).

2. Verification already in place and re-run:
   - `bash HtmlRenderer/scripts/check-html-boundaries.sh`
   - Result: `HTML crate boundaries are valid`

3. Plan documentation updated with explicit type-system-first routing:
   - `HtmlRenderer/plans/08-pipeline-boundary-hardening.md`
   - Added explicit note that invariants that are representable in types should be encoded
     as domain-checked types (`PositiveF32`, `NonNegativeF32`, bounded enums), with
     runtime checks retained only where type-level encoding is insufficient.

## Current milestone status (estimation as of 2026-08-01)

- 8a Glyph validation: ~90%
- 8b Style validation: ~90%
- 8c Sealed DOM builder: ~70%
- 8d Render contract: ~90%
- 8e Hide layout internals: ~80–85%
- 8f Transition policy: ~90%

## What remains to fully close Plan 08

- Add remaining acceptance tests for 8c malformed trees (cycles, forged ids, multiple roots, missing parents).
- Finish 8e by removing any remaining public layout-internal exposure surface outside
  `html-layout` and confirming no dependent crate can rebind to mutable `html-layout-model`
  representations through public transit.
- Extend 8b/8a property coverage where runtime validation still exists, prioritizing
  conversion to type-level invariants when inputs are structurally representable.

## Type-system vs runtime (explicit decision)

- Continue with the existing strategy:
  - **Type-level first** for invariants that can be encoded with constructor-guarded value
    types and sealed state transitions.
  - **Runtime validation fallback** for foreign-brand ownership checks and values tied to
    unresolved external context (e.g. cross-document brand mismatch, parser-sourced IDs
    before validation).
- No change to boundary direction or phase-order contracts; continue using compile-time
  transitions as the primary hardening vector where feasible.

## Produced artifacts

- Plan update:
  - `HtmlRenderer/plans/08-pipeline-boundary-hardening.md`
- New report:
  - `HtmlRenderer/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01e.md`

