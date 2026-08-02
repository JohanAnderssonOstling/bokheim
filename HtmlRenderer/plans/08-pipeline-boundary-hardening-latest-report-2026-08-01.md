# Plan 08 — Pipeline Boundary Hardening latest report (2026-08-01)

## Scope

- Capture current Plan 08 completion state after the latest render-view migration pass.
- Confirm boundary checks and direct coupling scans.
- Record the concrete type-system-vs-runtime validation posture requested by the user.

## Verification run

- Command: `bash HtmlRenderer/scripts/check-html-boundaries.sh`
- Result: `HTML crate boundaries are valid`

## Code sweep log

- `HtmlRenderer/crates/html-view-core/src/renderer/nav.rs`
  - `anchor_positions()` now reads through `self.document.render_view().anchor_positions()`.
  - No remaining callsites were found to `LaidOutDocument::anchor_positions()` map-returning API.
- `HtmlRenderer/crates/html-view-core/src/load.rs`
  - marker reconstruction now uses `render_view().marker_runs()` and no longer uses removed run-range accessors.
- `HtmlRenderer/crates/html-layout/src/stages.rs`
  - Removed allocation-producing public `RenderLines::to_vec` helper from render contract façade.
  - `LaidOutDocument` no longer exposes direct `box_inline_run_range` query as part of public renderer contract.

## Status by milestone

1. 8a — Glyph validation: **~90%**
   - `GlyphMetric::try_new` and append-only registry behavior are in place.
   - Negative metrics are rejected on metric construction; residual risk is audit breadth across all shape error paths.

2. 8b — Style validation: **~90%**
   - Computed style builder and finish-time validation are present.
   - Float/length constraints remain in validation code; style APIs are still mostly runtime-enforced at this boundary.

3. 8c — Sealed DOM builder: **~70%**
   - Typed builder transition (`NeedsRoot`/`Rooted`) exists.
   - Builder transitions are more type-safe than pre-existing runtime-only checks.
   - Remaining gap is acceptance coverage for malformed trees and additional compile-fail guarantees.

4. 8d — Render contract: **~90%**
   - renderer is no longer using layout concrete structs in the main navigation and rendering flows.

5. 8e — Hide layout internals: **~80–85%**
   - Consumers outside `html-layout` no longer depend on concrete layout internals for normal render paths.
   - `html-layout-model` is still a separate public crate with mutable internals used internally by `html-layout`.

6. 8f — Transition policy: **~90%**
   - Phase ordering and cross-phase lineage checks remain explicit by type.

## Type-system validation note

- User request acknowledged: prefer static, typed transitions where practical.
- Applied already:
  - `DocumentBuilder` state machine for DOM root progression (runtime parse errors reduced to impossible states).
  - Render output contract accessors for renderer paths to prevent whole-class runtime coupling.
- Remaining major work:
  - Continue moving domain-unsafe runtime checks in style/DOM builders into branded/typed invariants at construction boundaries.
  - Keep defense-in-depth runtime checks for untrusted boundary inputs where type information cannot eliminate all invalidity.

## Residual risk (today)

- `html-layout-model` crate is still publicly visible and still exports mutable internals, so representation hiding is not complete.
- Full compile-fail suite and acceptance tests for invalid DOM IDs/cycles remain incomplete.
- Boundary scan confirms remaining `InlineKind`/`LayoutTree` usage is confined to `html-layout` and its internal model crate.

## Next actions

1. Add/extend acceptance tests for 8c/8a/8b invalid-state rejection.
2. Finalize 8e by removing remaining layout implementation exposure from public boundaries.
3. Keep boundary script as pre-merge gate and expand to catch any newly reintroduced 8d/8e regressions.
