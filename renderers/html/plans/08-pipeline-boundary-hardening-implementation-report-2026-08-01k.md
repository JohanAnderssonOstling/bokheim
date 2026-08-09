# Plan 08 Implementation Report — 2026-08-01

## Scope

- Continue Plan 08 milestone 8c hardening by pushing impossible builder transitions into the type system where practical.
- Keep parser-to-DOM construction consistent with the new `DocumentBuilder` state machine.
- Validate boundary and type-state wiring with focused checks.

## Files changed

- `HtmlRenderer/crates/html-parse/src/lib.rs`
- `HtmlRenderer/crates/html-dom/src/document/dom.rs`
- `HtmlRenderer/crates/html-layout/src/layout/inline.rs`
- `HtmlRenderer/crates/html-layout/src/stages.rs`
- `HtmlRenderer/plans/08-pipeline-boundary-hardening-verification-2026-08-01.md`

## What changed

- Removed the `DocumentBuilder<NeedsRoot>` consume-then-later-mutate pattern that depended on moved ownership in `build_dom_document`.
- Added shared helper:
  - `build_dom_element_data<State>(document: &mut DocumentBuilder<State>, element: &ElementRef) -> DomElementData`
- Updated root parsing path in `build_dom_document` to:
  - build `DomElementData` first,
  - append the first node through `DocumentBuilder::append_dom_element` (state transition),
  - recurse/build children with `DocumentBuilder<Rootless>`,
  - finalize through `set_dom_root(...).finish()`.
- Updated recursive element construction in `build_dom_element` to use shared `build_dom_element_data`.
- Made `assign_generated_id` state-generic:
  - `fn assign_generated_id<State>(document: &mut DocumentBuilder<State>, node: DomNodeId) -> String`
- Adjusted `html-dom` root-state tests for new typed transitions.
- Fixed parser/layout test helper path in `html-layout` for DOM node access (`node_id_from_raw` mapping in float pipeline test) and test imports (`ShapeError`).

## Verification performed

- `cargo check -p html-dom -p html-parse -p html-layout -p html-view-core -p html-pipeline`
- `cargo test -p html-parse -p html-dom -p html-layout`
- `bash HtmlRenderer/scripts/check-html-boundaries.sh`

## Verification outcome

- All three commands completed successfully.
- `html-parse`, `html-dom`, and `html-layout` unit tests pass (all relevant tests green).
- Boundary script currently reports valid crate boundaries.

## Current assessment

- Milestone 8c builder-state hardening has moved from runtime-heavy semantics to explicit typed transitions for non-empty root handling:
  - `NeedsRoot -> Rootless` transition when appending first node.
  - `set_dom_root(...).finish()` for rooted documents.
  - `finish_without_root()` retained only for explicit empty-tree completion path.
- Remaining 8c/8d/8e work is now primarily regression and coverage policy rather than type-level blocking issues in these crates.
