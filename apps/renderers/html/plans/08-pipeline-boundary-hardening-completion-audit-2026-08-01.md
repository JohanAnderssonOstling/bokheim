# Plan 08 Completion Audit — 2026-08-01

## Verdict

Complete. The implementation now satisfies milestones 8a–8f and the Plan 08
definition of done. No known correctness or representation-isolation item from
the plan remains open.

## Implemented boundary guarantees

- `GlyphMetric` has private fields and a validating constructor. Shapers and
  auxiliary users receive append-only registries and cannot replace existing
  metrics.
- `ComputedStylesBuilder` validates values before interning, rejects foreign
  handles, and repeats full validation in `finish()` as defense in depth.
- `DocumentBuilder` encodes root state in its type. `DomNodeId` is opaque and
  branded with a process-monotonic, non-zero lineage that cannot alias a later
  document during normal execution. Finalization validates reachability,
  parent/child reciprocity, acyclicity, and all stored indices.
- `RenderView` is borrowed and allocation-free. It exposes semantic line, run,
  glyph, metric, image, decoration, ancestry, anchor, and metadata queries while
  keeping ranges and layout storage private.
- `html-view-core` and the root painter use `RenderView` for immutable reads.
  Relayout and auxiliary glyph registration remain narrow mutation contracts.
- Mutable tree, inline, and layout-state representations are private to
  `html-layout`; the old `html-layout-model` crate and dependency are absent.
- All internal `html-*` crates set `publish = false`.
- The boundary script rejects dependency-direction violations, raw layout
  reach-through, forgeable DOM ID APIs, renderer bypasses of `RenderView`,
  public layout-model modules, and publishable internal crates.

## Test evidence

Boundary gate:

```text
bash apps/renderers/html/scripts/check-html-boundaries.sh
HTML crate boundaries are valid
```

Focused suite:

```text
cargo test -p html-dom -p html-parse -p html-style-model -p html-style \
  -p html-layout -p html-pipeline -p html-view-core
```

All tests passed:

- `html-dom`: 11 unit tests and 2 compile-fail doctests;
- `html-parse`: 2 tests;
- `html-style-model`: 7 unit tests and 1 compile-fail doctest;
- `html-style`: 32 tests;
- `html-layout`: 57 passed, 2 ignored, and 6 compile-fail doctests;
- `html-pipeline`: 4 tests;
- `html-view-core`: 9 passed, 2 ignored profile tests.

The new regression coverage includes stale/foreign DOM IDs, malformed DOM
indices and parent links, invalid style categories and finish-time
revalidation, private construction compile failures, checked render ranges,
append-only glyph registration, and an exact zero-allocation render-view test.

Historical root-adapter tests (the adapter has since been removed):

```text
6 passed; 0 failed
```

These tests also consume metrics and branded font-family IDs through the real
pipeline and `RenderView`; no test-only raw store or forged integer handle is
required.

Historical adapter build:

```text
Finished `dev` profile successfully
```

## Performance evidence

Command:

```text
cargo test -p html-view-core --release profile_ -- --ignored --nocapture \
  --test-threads=1
```

Result:

- 173 TestData documents processed, 0 failed;
- total pipeline time: 1,838.737 ms;
- layout: 460.751 ms total, 2.663 ms/document;
- build-document pipeline: 942.758 ms total, 5.449 ms/document;
- shaping: 219.891 ms total, 1.271 ms/document;
- largest retained document: 4,903.76 KiB (`app0025.html`);
- both ignored profile tests passed.

Creating `RenderView` and traversing its semantic iterators is separately
asserted to allocate exactly zero times.

No pre-change release profile was preserved, so a before/after percentage
cannot be reconstructed honestly. These measurements are the checked baseline
for the next pipeline-boundary change; future results above the plan's 2%
threshold should be investigated against it.

## Definition-of-done mapping

- 8a glyph validation: complete;
- 8b computed-style validation: complete;
- 8c sealed and branded DOM construction: complete;
- 8d stable borrowed render contract: complete;
- 8e private mutable layout representation: complete;
- 8f explicit transition policy and lineage validation: complete;
- correctness, compile-fail, boundary, allocation, release-profile, root, and
  adapter verification: complete.

Existing compiler warnings are non-fatal dead-code or upstream warnings and do
not identify a Plan 08 boundary violation.
