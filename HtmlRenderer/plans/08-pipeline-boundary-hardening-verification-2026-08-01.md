# Plan 08 Verification Report — 2026-08-01

## Scope validated

- Typed DOM builder transitions (`NeedsRoot` / `Rootless` / `Rooted`) in parser entry points.
- Compile-level alignment with type-state APIs in `html-parse` and `html-dom` tests.
- Boundary script check for forbidden cross-crate coupling and render-contract usage.

## Files updated

- `HtmlRenderer/crates/html-parse/src/lib.rs`
- `HtmlRenderer/crates/html-dom/src/document/dom.rs`
- `HtmlRenderer/crates/html-layout/src/layout/inline.rs`
- `HtmlRenderer/crates/html-layout/src/stages.rs`
- `HtmlRenderer/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01k.md`

## Verification run

- `cargo check -p html-dom -p html-parse -p html-layout -p html-view-core -p html-pipeline`
- `cargo test -p html-parse -p html-dom -p html-layout`
- `bash HtmlRenderer/scripts/check-html-boundaries.sh`

## Result

- All requested type-system adjustments in parser-to-DOM builder handoff are applied and compile.
- Targeted unit tests for `html-parse`, `html-dom`, and `html-layout` pass.
- Boundary script currently reports valid crate boundaries.
- Remaining residual warnings are non-fatal `unused`/visibility warnings; no compile-blocking errors.

## Notes

- 8c completion path now rejects non-root finalization at the type level for non-empty sequences by moving DOM root establishment into an explicit state transition.
- Runtime invalid cases that are still useful for defense-in-depth remain (for example, direct mutation paths in tests that intentionally construct malformed trees for assertions).
