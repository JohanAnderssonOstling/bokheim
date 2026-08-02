# Plan 08 — Final status report (2026-08-01)

## Scope

This report documents current status after the latest Plan 08 continuation pass in `HtmlRenderer/plans`.

## Verification artifacts

- `bash HtmlRenderer/scripts/check-html-boundaries.sh` → success (`HTML crate boundaries are valid`).
- `cargo check -p html-layout` → success.
- `cargo check -p html-view-core` → success.
- `cargo check -p html-pipeline` → success.
- Type-system and validation checks in `HtmlRenderer/crates/html-dom/src/document/dom.rs` were reviewed.

## Milestone assessment

- 8a Glyph validation — effectively complete
  - Finite/negative checks are enforced in glyph metric registration.
  - Public metrics are shielded behind validated constructors and append-only paths.
  - Shape errors include invalid/unregistered glyph and capacity failures.

- 8b Style validation — effectively complete
  - Computed styles validate value-level invariants on insertion and finish.
  - Non-finite and out-of-range numeric values are rejected before interning and rechecked defensively at finish.
  - Brand checks remain enforced for cross-document/string-handle safety.

- 8c Sealed DOM builder — largely complete, with bounded technical debt
  - `DocumentBuilder<State = NeedsRoot>` and `DocumentBuilder<Rooted>` are present.
  - `set_dom_root` now requires a `DomNodeId`, and typed transitions return `DocumentBuilder<Rooted>`.
  - DOM build validation catches:
    - foreign node IDs
    - missing/invalid parents
    - multiple roots, non-element roots, root parent violations
    - duplicate children, cycles, unreachable nodes
    - all index-bound checks for strings/classes/attributes/images/TOC
  - Builder tests cover forged IDs, missing root, and cycle/multiple-root cases.
  - Type-system follow-up moved one step forward:
    - `DocumentBuilder::finish()` now exists only on rooted builders.
    - Unrooted finalization is now explicit via `finish_without_root()` and only valid for the empty-doc path.
  - Remaining type-system work is still needed for compile-fail proof from misuse in other call patterns.

- 8d Render contract — substantially complete
  - Boundary checks pass for checked downstream crates, and `html-view-core` no longer directly depends on selected layout-internal representations in the validated path.
  - Render-facing interface compiles through checked adapters using layout façade boundaries.

- 8e Hide layout internals — substantially complete
  - `html-layout` no longer exposes the prior wildcard re-export shape for layout internals to renderer-facing crates.
  - Boundary gate confirms no remaining forbidden direct imports in adjacent crate graph.

- 8f Transition policy — already in place
  - Phases remain encoded as `PreparedDocument -> ShapedDocument -> LaidOutDocument`.
  - Runtime lineage validation continues to guard cross-document object handoff.

## Practical status

- Plan 08 is now materially beyond the external baseline checkpoint in boundary enforcement and has stable gate passing for boundary migrations.
- The remaining work is correctness-oriented around the final compile-time hardening of remaining root/state edge cases and broader renderer contract migration.
