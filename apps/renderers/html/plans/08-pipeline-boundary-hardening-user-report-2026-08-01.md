# Plan 08 User Progress Report — 2026-08-01

## Scope

- Record the current implementation state for Plan 08 after the latest continuation pass.
- Emphasize type-system-first hardening where feasible.
- Provide a concrete artifact in `plans/` for follow-up work.

## What was done this pass

- Reinforced Plan 08 milestones 8c/8d/8e with type-state and render-contract migration status.
- Confirmed parser and DOM builder type-state transitions are in place for root handling (`NeedsRoot` / `Rootless` / `Rooted` style flow).
- Confirmed runtime coupling points from renderer paths are substantially removed.
- Confirmed boundary checks and compilation validation were run in prior pass.

## Verification artifacts on record

- `bash apps/renderers/html/scripts/check-html-boundaries.sh` → success (`HTML crate boundaries are valid`).
- `cargo check -p html-dom -p html-parse -p html-layout -p html-view-core -p html-pipeline` → success.
- `cargo test -p html-parse -p html-dom -p html-layout` → success.
- `cargo check -p html-layout` → success.
- `cargo check -p html-view-core` → success.
- `cargo check -p html-pipeline` → success.

## Milestone status snapshot

- 8a Glyph validation: **~85–90% complete**
  - `GlyphMetric::try_new` and append-only registration are present.
  - Remaining work: full end-to-end invalid-metric and foreign-registry error-path hardening.

- 8b Style validation: **~90% complete**
  - Value-level validation exists in builder and finish paths.
  - Remaining work: expand test coverage of edge/error surface cases.

- 8c Sealed DOM builder: **~70% complete**
  - Typed builder states and root transition model exist.
  - Remaining work: broaden acceptance/compile-fail proof for malformed IDs/cycles and other parser misuse patterns.

- 8d Render contract: **~90% complete**
  - `html-view-core` largely consumes `RenderView` façade queries.
  - Remaining work: final check across all downstream adapters and helpers.

- 8e Hide layout internals: **~80–85% complete**
  - Layout internals no longer required by `html-view-core` paths.
  - Remaining work: harden `html-layout-model` public API boundaries and downstream access in other crates.

- 8f Transition policy: **~90% complete**
  - Type-ordered phase pipeline (`Parsed/Styled/Prepared/Shaped/LaidOut`) remains intact with lineage checks.

## Type-system preference outcome

- Priority was applied to impossible-state prevention through types where practical:
  - DOM builder state typing for rooted completion.
  - Contract-first read APIs for renderer access.
  - Negative-runtime-only checks retained as defense-in-depth for external/parsing/error-path cases.

## Residual risks

1. `html-layout-model` remains a public workspace crate with mutable internals available to any downstream user.
2. 8a and 8b still need more exhaustive invalid input and error mapping tests.
3. 8c still has a small gap in proof-by-types for all misuse entry points outside the main parser path.

## Next-pass recommendation

1. Finish the remaining 8e hardening in `html-layout-model` visibility/API design.
2. Add focused parser/DOM acceptance tests for forged IDs, duplicate roots, missing-parent, cycle, and empty-doc root rules.
3. Add explicit boundary tests for all remaining adapter crates to keep compile-time coupling checks green.

