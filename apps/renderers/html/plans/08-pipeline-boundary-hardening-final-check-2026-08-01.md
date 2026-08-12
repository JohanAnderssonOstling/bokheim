# Plan 08 — Final checkpoint report (2026-08-01)

## Executive summary

Plan 08 boundary hardening is substantially complete in validation coverage and partially complete in representation isolation. The critical runtime pipeline invariants are now mostly enforced at phase boundaries, and renderer coupling to layout internals has been substantially removed.

## Evidence collected

- Boundary script: `bash apps/renderers/html/scripts/check-html-boundaries.sh`
  - Result: `HTML crate boundaries are valid`
- Plan checkpoint files reviewed:
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening.md`
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening-status-2026-08-01.md`
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01.md`
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01c.md`
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01e.md`
  - `apps/renderers/html/plans/08-pipeline-boundary-hardening-final-report-2026-08-01.md`

## Current milestone status (approximate)

- 8a — Glyph validation: **~90%**
  - `GlyphMetric::try_new` and negative/infinite guards are in place.
  - Remaining: full error-surface audit for every invalid/foreign metric path through `ShapeError`.
- 8b — Computed style validation: **~90%**
  - Builder `push`/`finish()` validation pathways exist.
  - Remaining: continue to validate all edge-path surfaces and brand/string-edge cases.
- 8c — Sealed DOM builder: **~70%**
  - Builder state typing (`NeedsRoot`/`Rooted`), root validation, and several malformed-tree acceptance tests are implemented.
  - Remaining: tighten remaining mutable post-build/document-facing paths and widen acceptance tests.
- 8d — Stable render contract: **~90%**
  - Renderer consumes `LaidOutDocument::render_view()` and contract read APIs in key flows.
- 8e — Hide mutable layout internals: **~85%**
  - Renderer-level leakage is mostly removed; raw layout types no longer form the renderer interface.
  - Remaining: `html-layout-model` still exists as a public layout-implementation crate; continued hardening is needed if you want zero consumer access.
- 8f — Transition policy: **~90%**
  - Phase ordering remains explicitly typed through the pipeline API.

## External checkpoint comparison

Compared with the external review (55–60% estimate), the implementation is now approximately **78–82% complete** when considering the boundary migration and 8d contract work that has landed.

## Next actions to reach 100%

1. Harden type-system-first boundaries further in `html-dom`/`html-style-model` query APIs (`impl Into<DomNodeId>` etc.) where practical.
2. Tighten or remove any remaining mutable post-build mutation surfaces in completed phases.
3. Continue acceptance-test expansion for malformed IDs, cycles, foreign lineage, and all high-risk boundary error paths.
4. Keep boundary script + targeted crate checks as merge gate.

## Recommendation

If you want a strict “all in” completion before the next review, prioritize finishing **8e** by containing `html-layout-model` exposure and finish **8c** by reducing all avoidable runtime-only checks through typed APIs where feasible.
