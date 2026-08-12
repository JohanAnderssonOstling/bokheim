# Plan 08 — Final verification report (2026-08-01)

## Scope

- Verify implementation state of milestones 8a–8f after recent 8d/8e migration work.
- Record concrete validation results and remaining technical debt.
- Produce a disk artifact for follow-up work.

## Validation runs

1. Boundary contract script
- Command: `bash apps/renderers/html/scripts/check-html-boundaries.sh`
- Result: `HTML crate boundaries are valid`

2. Representation-coupling scan (external consumers)
- Command: `rg -n "\\b(LayoutTree|LayoutState|LayoutBox|InlineRun|InlineKind|ImageFragment|Line\\b|\\.document\\.(styles|style_view|layout_tree|inline_content|layout_state|inline_runs|inline_run|text_runs)|\.line\(|\.glyphs\.|\.inline_runs\(" apps/renderers/html/crates`
- Result: No disallowed render-to-layout internals from `html-view-core`/adapters.
- Remaining hits are within `apps/renderers/html/crates/html-layout` internals and `apps/renderers/html/crates/html-layout-model`, which is expected for implementation ownership.

3. Layout crate dependency exposure check
- Command: `rg -n "html-layout-model" apps/renderers/html/crates -g 'Cargo.toml'`
- Result: only `html-layout` declares the internal dependency on `html-layout-model`.

## Current milestone status

- 8a Glyph validation: **~85–90%**
  - `GlyphMetric::try_new` enforces finite/negative guards.
  - `GlyphMetrics::register` is append-only.
  - `ShapeError` still should be rechecked for all foreign/metric-failure surfaces in follow-up.

- 8b Style validation: **~90%**
  - Builder and finish-time checks are in place.
  - Type-level preference for safe transitions has been applied where practical.

- 8c Sealed DOM builder: **~65–70%**
  - `DocumentBuilder` state typing (`NeedsRoot`/`Rooted`) and stricter root transitions are present.
  - Builder errors exist for malformed tree/state conditions, but parser-level acceptance coverage can be broadened.

- 8d Render contract: **~90%**
  - `html-view-core` paths now consume the `RenderView` contract APIs.
  - Layout internals are no longer accessed by renderer dependency surfaces.

- 8e Hide layout internals: **~80–85%**
  - `html-layout` no longer wildcard-reexports `html_layout_model::*`.
  - Public raw layout-stage accessor methods in `stages.rs` have been restricted to crate-internal (`pub(crate)` where still required).
  - Remaining risk: hardening the `html-layout-model` public API itself.

- 8f Transition policy: **~90%**
  - Ordered phase progression remains encoded by types (`ParsedHtml` -> `StyledDocument` -> `PreparedDocument` -> `ShapedDocument` -> `LaidOutDocument`).

## Residual risk list (high priority)

1. `html-layout-model` still exposes mutable internals (layout model crate is public in workspace), so any downstream consumer can still couple to `Line`, `LayoutState`, `InlineContent`, etc.
2. Additional 8c acceptance tests for invalid IDs/cycles/forged roots remain desirable.
3. 8a/8b error-channel assertions should be expanded to explicitly cover all invalid-shaper and foreign-value edge-cases.

## Suggested completion plan (next pass)

1. Expand plan 8e by constraining `html-layout-model` visibility or splitting a narrow internal crate API used by `html-layout` and an immutable render contract crate.
2. Add focused compile-fail or runtime negative tests for remaining 8c/8a/8b boundary cases.
3. Keep `check-html-boundaries.sh` as the pre-merge gate and add the above test buckets to CI.
