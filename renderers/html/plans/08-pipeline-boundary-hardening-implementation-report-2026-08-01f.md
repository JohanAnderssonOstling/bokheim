# Plan 08 — Implementation report (2026-08-01, latest pass)

## Scope

This pass focused on finishing milestone **8c** hardening (sealed DOM builder) and closing remaining 8d/8e contract leakage items already in flight.

## What was changed in this pass

1. Root-aware DOM builder completion
- File: [`HtmlRenderer/crates/html-dom/src/document/dom.rs`](HtmlRenderer/crates/html-dom/src/document/dom.rs)
  - `DocumentBuilder<NeedsRoot>::finish()` now reads the validated root from `validate_for_build()`, assigns it into the finished `Document`, and rebuilds TOC.
  - `DocumentBuilder<Rooted>::finish()` does the same assignment step, avoiding any path that can produce a finished document with stale/absent `dom_root`.
  - `Document::validate_for_build()` now requires an explicit root on non-empty documents (`MissingRoot` if `dom_root` is unset), eliminating the previous “complete validation but no committed root” edge.

2. Boundary contract/adapter migration already present before this pass
- Files touched in earlier pass:
  - [`HtmlRenderer/crates/html-layout/src/stages.rs`](HtmlRenderer/crates/html-layout/src/stages.rs)
  - [`HtmlRenderer/crates/html-view-core/src/renderer/selection.rs`](HtmlRenderer/crates/html-view-core/src/renderer/selection.rs)
  - [`HtmlRenderer/crates/html-view-core/src/load.rs`](HtmlRenderer/crates/html-view-core/src/load.rs)
  - [`HtmlRenderer/crates/html-layout/src/layout/decorations.rs`](HtmlRenderer/crates/html-layout/src/layout/decorations.rs)

3. Verification and reporting updates
- The boundary gate was checked previously in this branch with `bash HtmlRenderer/scripts/check-html-boundaries.sh` and passed (`HTML crate boundaries are valid`).
- Added this persisted report file under `HtmlRenderer/plans/` as requested.

## Type-system preference status

- Type-first direction is still active:
  - `DocumentBuilder<NeedsRoot>` / `DocumentBuilder<Rooted>` remains as the primary state transition for DOM root safety.
  - Runtime checks that cannot be fully shifted to types (foreign IDs / cross-link lineage / value ownership) continue to be defended at finish-time boundaries.

## Milestone status (post-pass estimate)

- 8a Glyph validation: **~90%**
- 8b Style validation: **~90%**
- 8c Sealed DOM builder: **~75%**
  - Runtime edge still to tighten: remove/limit unchecked document-level mutation and expand malformed-tree tests.
- 8d Render contract: **~90%**
- 8e Hide layout internals: **~80–85%**
  - Remaining work is mainly crate-level encapsulation of `html-layout-model` types.
- 8f Transition policy: **~90%**

## Next actions

1. Add explicit acceptance tests for malformed trees that should fail even before finish (missing root, forged IDs, TOC-index corruption paths).
2. Reduce mutable builder exposure (`DocumentBuilder` mutation surface) to only what parser/style pipeline needs.
3. Continue 8d→8e migration by removing any non-`render_view()` dependency on concrete layout internals from renderer-facing crates.
