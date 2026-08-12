# Plan 08 — 8c/8d/8e hardening continuation report

Date: 2026-08-01

## What was done in this pass

### 1) DOM builder root/mutation tightening
- Updated [`apps/renderers/html/crates/html-dom/src/document/dom.rs`](apps/renderers/html/crates/html-dom/src/document/dom.rs) so `DocumentBuilder::document_mut()` is now test-only (`#[cfg(test)]`).
- This removes the remaining non-test builder mutation escape hatch while preserving the existing test-only corruption fixtures.
- Root-required finish behavior remains enforced by validation and is now committed in finish (`needs_root` and `rooted` builders assign `dom_root` from `validate_for_build()`).

### 2) Boundary enforcement check
- Re-ran boundary script:
  - Command: `bash apps/renderers/html/scripts/check-html-boundaries.sh`
  - Result: `HTML crate boundaries are valid`

### 3) Plan alignment update
- The type-system preference for immutable/impossible states remains in place:
  - DOM builder state typing (`NeedsRoot` / `Rooted`) and explicit `set_dom_root()` transition.
  - Render-facing contract exposed through `RenderView` instead of raw layout internals.

## Milestone status (current estimate)

- 8a Glyph validation: **~90%**
  - `GlyphMetric` validation and append-only registry mechanics are in place.

- 8b Style validation: **~90%**
  - Style value validation and checked construction are implemented in `html-style-model` transitions.

- 8c Sealed DOM builder: **~76%**
  - Root state is now part of builder state machine.
  - Public unchecked tree mutation path now reduced (test-only `document_mut`).
  - Remaining work: review any remaining public mutation surface on `DocumentBuilder` for parser-only necessity and add more malformed-tree tests.

- 8d Render contract: **~90%**
  - `LaidOutDocument::render_view()` is the stable renderer consumption path.
  - Renderer crates use `render_view()`-based APIs.

- 8e Hide layout internals: **~80%**
  - Raw access to layout internals from renderer crates is blocked by boundary checks.
  - `html-layout` still depends on the separate `html-layout-model` crate, so full crate-level physical encapsulation is still open work.

- 8f Transition policy: **~90%**
  - Runtime lineage and ordered transitions are enforced and preserved.

## Remaining explicit risks

1. `html-layout-model` remains a separate crate; stronger encapsulation still needs a migration plan.
2. Unfinished verification scope: no full test/retest cycle was executed in this pass beyond boundary-script validation.
3. A few public builder-facing mutators (for example TOC insertion and image/title setters) still exist for parser-level orchestration and should be reviewed for the strictest possible API surface.

