## Goal

Centralize ISBN extraction behind three source-aware entry points — metadata values, filenames, and book content — removing the duplicated scanners in `book-inspection`, `book-metadata`, and `book-enrichment` with consistent, explicit acceptance rules for each source. Intentional behavior changes are allowed; preserving divergent legacy behavior is not a requirement.

## Success Criteria

- One implementation exists for each source: whole-value metadata check, filename scan (multi-candidate plus single-unambiguous policy), and shared content-candidate validation.
- Every current call site routes through the centralized functions with tests covering the intended acceptance rules and downstream identifier scopes. Output differences must be explained by those rules.
- No new crates, no new third-party dependencies, no dependency cycles; `Scope` assignment (Book vs Edition) stays at the call sites.

## Context And Current Facts

- Primitives live in `shared/book-model`: `canonical_isbn()` (`shared/book-model/src/canonical.rs:2`), `metadata_isbn()` (`shared/book-model/src/lib.rs:833`), `pub(crate) isbns_in_text()` (`shared/book-model/src/canonical.rs:57`), and `infer_embedded_subject_isbns()` on `BookMetadata` (`shared/book-model/src/credits.rs:180`).
- Filename/title scanners reimplement tokenize-then-`canonical_isbn` independently: `title_isbn::extract/record` (`client/library-backend/crates/book-enrichment/src/title_isbn.rs:5`), `filename_identifiers::clean/record` (`client/library-backend/crates/book-enrichment/src/filename_identifiers.rs:4`), `isbn_from_filename_like_title()` (`client/library-backend/crates/book-metadata/src/identity.rs:7`), `isbn_from_opf_path()/push_epub_isbn()` (`client/library-backend/crates/book-inspection/src/lib.rs:311`).
- Content scanning lives in `client/library-backend/crates/book-inspection/src/evidence.rs:14` (`isbn_pattern/complete_isbn/number_run_isbns/inspect_section`) with edition-scope and format-label logic on top of `canonical_isbn`.
- Metadata-value call sites: `book-inspection/src/pdf.rs` (`title_isbn::record`, `metadata_isbn`, `book_identifier`), `book-inspection/src/mobi.rs:48`, `book-inspection/src/lib.rs:271` (EPUB identifiers), `book-metadata/src/lib.rs:197` (M4B tags via `canonical_isbn`), `book-enrichment/src/lib.rs:12` (re-exports `metadata_isbn`), `title_identity.rs`, `filename_identity.rs:78`, `editions.rs:27` (`isbn_from_filename_like_title`).
- Layering verified from Cargo manifests: `book-model` has no `regex` dependency and is depended on by all; `book-enrichment` depends only on `book-model` (+ contracts); `book-inspection` depends on `book-model` + `book-enrichment` and owns `regex`; `book-metadata` depends on `book-inspection`.
- Known divergences: `title_isbn` accepts space-separated groups via word-windows, `isbn_from_filename_like_title` ends candidates at spaces; only the latter enforces exactly-one-distinct-ISBN; metadata whole-value validation and content validation reject all-zero placeholders, while the legacy filename scanners accept all-zero ISBN-10 values.

## Constraints And Non-goals

- Constraints: verification uses optimized release commands (`cargo test --release`) per user global preference; intentional extraction changes must be documented and tested.
- Non-goals: no changes to `IsbnScope`/format-label assignment, LCC logic, authority lookup and resolution, or title-cleaning beyond ISBN spans; no revival of the workspace-excluded `shared/book-identity` crate.

## Key Decisions

1. Filename policy lives in `book-enrichment`, not `book-model`. `book-model` would otherwise absorb filename/OPF concepts and a scanning implementation, while `book-enrichment` already owns the title/filename heuristics and depends only on `book-model`, so no cycle is introduced. A brand-new crate was rejected: it adds plumbing without benefit since `book-enrichment` already plays that role.
2. Metadata whole-value check stays in `book-model` (current `metadata_isbn`, exposed as `from_metadata_value`). Metadata-value callers use this entry point; canonical and candidate-validation primitives remain available where appropriate.
3. Content orchestration stays in `book-inspection` (`inspect_section`, marker windows, side-by-side splitting, scope/labels); only candidate validation delegates to the shared `book-model` helper. This keeps `regex` out of the base crate.
4. Unified filename tokenizer: whole-run scan accepting spaces, hyphens, and unicode dashes, `10`-digit or `978/979`-prefixed `13`-digit length filter, checksum via `canonical_isbn`, all-zeros rejected. Multi-candidate `from_filename()` plus single-unambiguous `from_filename_single()` preserves `book-metadata`'s current exactly-one policy.
5. Dedup-push idiom (`any(Isbn && canonical == isbn)` then `Identifier::new(.., Scope::X)`) becomes one `push_isbn()` helper in `book-model`, which owns the `Identifier` types; the `Scope` argument stays with the caller.

## Intended Behavior And API Details

- Filename extraction accepts space-separated groups and supported Unicode dashes, rejects all-zero placeholders and non-978/979 ISBN-13 values, and retains checksum validation. These rules intentionally replace differing legacy acceptance rules.
- OPF paths use the single-unambiguous filename policy after existing wrapper stripping. A single valid ISBN embedded in a descriptive stem is intentionally accepted with the existing `Scope::Book` assignment. Test descriptive stems and rejection of multiple distinct ISBNs.
- M4B ISBN tags route to `from_metadata_value`, intentionally accepting explicit ISBN labels and supported Unicode separators while rejecting all-zero placeholders and non-978/979 ISBN-13 values. Add tag-level regression cases.
- The filename scanner provides an internal match result containing the canonical ISBN and its original UTF-8 byte range. Extraction deduplicates values; `filename_identifiers::clean` consumes all occurrence spans. Test repeated and overlapping matches, Unicode byte offsets, and resulting cleaned titles.
- Route title/filename fields to filename extraction and metadata values to whole-value validation. Preserve the existing scope choice at each caller.

## Recommended Approach

Two layers: `book-model` exposes a small public ISBN surface (canonical check, metadata whole-value, text-scan, push helper); `book-enrichment` gains one filename module that all filename/title/OPF callers delegate to; `book-inspection` keeps evidence orchestration and delegates only validation. Migrate in dependency order behind compatibility shims, verify each phase, then remove the shims.

## Work Plan

1. `book-model` surface: make the text scanner public as `from_text` (keep existing internal callers working), add a candidate validator matching `complete_isbn` semantics (canonical + length filter + all-zeros reject) and a `push_isbn(identifiers, isbn, scope)` helper with unit tests.
2. `book-enrichment` filename module: implement `from_filename()` / `from_filename_single()` on the unified tokenizer; delegate `title_isbn::extract/record` and `filename_identifiers::clean` span detection to it; keep old names as shims; port the divergent space/unicode-dash cases into module tests.
3. Source-specific callers: point `book-metadata/identity.rs`, `book-inspection` `isbn_from_opf_path()` (thin wrapper: strip directory + `.opf` + `ISBN` prefix, then `from_filename_single`), `pdf.rs`, `mobi.rs`, `title_identity.rs`, `filename_identity.rs`, `editions.rs`, and M4B `recording_evidence()` at the centralized functions; `Scope` selection unchanged at each site.
4. Content validation: make `evidence.rs complete_isbn()` delegate to the shared `book-model` validator; keep marker windows, multiline joining, side-by-side splitting, and scope/label logic untouched.
5. Cleanup: remove shims, confirm no remaining direct `canonical_isbn`-as-scanner uses outside the centralized paths, run the full targeted suites plus `clippy` and `fmt`.

## Validation Plan

- Baseline first: `cargo test --release -p book-model -p book-enrichment -p book-inspection -p book-metadata` must pass before any edit.
- After each phase, re-run the same command; update expectations only for documented acceptance-rule changes, with regression cases demonstrating the new behavior. Content orchestration remains unchanged, so the `back-matter-evidence` fixture test and `inspect_section` scope tests must continue to pass unmodified.
- New unit tests: filename acceptance rules (space-separated groups, unicode dashes, `.pdf`/`.opf` wrappers, invalid checksums, multi-ISBN ambiguity), `from_filename_single` policy, OPF wrapper cases, `push_isbn` idempotency across `Scope::Book`/`Scope::Edition`.
- Highest-risk steps: phases 2–3 change acceptance and cleaning across callers; verify identifiers, scopes, ambiguity handling, and cleaned titles. Phase 4 must also preserve the content tests and `back-matter-evidence.epub` fixture results.
- `cargo clippy --release -p book-model -p book-enrichment -p book-inspection -p book-metadata` and `cargo fmt --check` on touched crates.

## Risks / Rollback

- Tokenizer unification intentionally changes identifiers on some filenames. Record representative before/after cases and test the intended rules. Temporary shims preserve call signatures during migration, not independent legacy implementations for rollback.
- No dependency churn by design: `regex` stays only in `book-inspection`, `book-enrichment` needs no new dependencies (hand-rolled scanning, as today).
- Investigate fixture differences: update expectations when they follow documented acceptance rules; fix unintended changes, especially scope, content orchestration, or unrelated title-cleaning regressions. Revert the relevant phase if necessary.

## Open Questions

None. Space-separated ISBN groups and supported Unicode dashes are accepted by both filename entry points; single-value extraction differs only in requiring exactly one distinct ISBN.
