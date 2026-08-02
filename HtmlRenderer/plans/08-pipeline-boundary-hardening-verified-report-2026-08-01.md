# Plan 08 — Verification Report (2026-08-01)

## Scope

This report captures the latest verification pass for Plan 08 work in this session, focused on completion of milestones 8d/8e and boundary gating.

## What was verified and fixed

### 1) Boundary migration now compiles and checks
- Ran boundary enforcement script:
  - `bash HtmlRenderer/scripts/check-html-boundaries.sh`
  - Result: `HTML crate boundaries are valid`
- Ran focused crate checks after each fix:
  - `cargo check -p html-layout`
  - `cargo check -p html-view-core`
  - `cargo check -p html-pipeline`
- All three commands now succeed (warnings only).

### 2) Remaining compile failures fixed
- `HtmlRenderer/crates/html-layout/src/layout/decorations.rs`
  - Fixed temporary borrows by copying inline runs and lines to local vectors before mutating layout inside the decoration pass.
- `HtmlRenderer/crates/html-view-core/src/renderer/selection.rs`
  - Fixed `box_tag` lifetime issue by returning owned `String` from `RendererCore::box_tag`.
  - Updated internal tag comparisons to borrow the owned string.
- `HtmlRenderer/crates/html-view-core/src/renderer/nav.rs`
  - Removed now-unused `GlyphMetrics` import.

## Milestone evidence snapshot

- 8d Render contract: compile/gate checks pass for render-view dependent crates (`html-view-core`, `html-pipeline`).
- 8e Hide mutable layout internals: `html-layout` no longer exposes a public wildcard re-export of `html_layout_model`, and boundary check confirms no external dependency use remains.

## Current implementation status (post-verification)

- 8a Glyph validation: previously implemented and now compiling within the migrated path.
- 8b Style validation: existing validated pipeline checks remain.
- 8c DOM builder state hardening: still partially implemented outside this session; not materially changed in this verification cycle.
- 8d Render contract: migration now compiling and boundary-clean for checked crates.
- 8e Hide layout internals: migration path now checks clean and builds.
- 8f Transition policy: unchanged from prior work.

## Notes

Warnings about unused methods remain in `html-layout` (from internal model types being copied into the new private module). They are non-fatal and do not block Plan 08 boundary gates.
