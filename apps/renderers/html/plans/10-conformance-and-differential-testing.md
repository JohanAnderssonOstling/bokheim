# Plan 10 — Semantic conformance and differential regression harness

## Prerequisites

- Plan 08: finalized pipeline values are validated and internals are hidden.
- Plan 09 is required only for milestone 10c: incremental and full builds expose
  the same stable render contract. Parser, style, shaping, layout, and render
  correctness fixtures can be implemented before Plan 09 is complete.

## Goal

Turn renderer correctness into measurable data. Build a deterministic,
framework-agnostic harness that can:

- describe expected behavior at parser, style, preparation, shaping, layout,
  and render-contract boundaries;
- compare incremental output with a clean build;
- optionally compare selected cases with a reference browser;
- fuzz malformed and adversarial HTML/CSS without panics or invalid geometry;
- reduce failures into small permanent regression fixtures;
- rank the existing CSS feature plans by real corpus impact.

After this plan, architecture work should stop unless the harness exposes a
specific structural defect. Normal development moves to standards correctness
and user-visible features.

## Why this is next

Plans 08 and 09 make changes safer and cheaper, but they do not prove that the
renderer agrees with HTML/CSS behavior. Existing unit tests cover known cases,
while the TestData profiles measure time and memory without a correctness
oracle. The next risk is confidently producing the wrong result.

The harness must compare semantic outputs, not only screenshots. Pixel tests
alone are fragile across font engines, operating systems, and GPU backends.

## Foundation — crate-local correctness tests first

Before adding a shared conformance tool, expand focused tests inside the crate
that owns each rule. These tests should call the smallest relevant API, use
ordinary assertions where practical, and fail without requiring snapshot
regeneration or another workspace package.

| Owning crate | First-wave coverage |
|---|---|
| `html-dom` | builder/finalization invariants, branded IDs, tree reachability, metadata and index validation |
| `html-parse` | malformed markup, normalized tree construction, text/attribute ordering, head metadata and resource discovery |
| `html-style-model` | value-domain validation, branded handles, deduplication and finalization invariants |
| `html-style` | cascade, specificity, source order, inheritance, CSS-wide keywords, selectors, units and unsupported declarations |
| `html-layout` | preparation semantics, shaping contracts, ranges, geometry, relayout idempotence, links, anchors, images and decorations |
| `html-pipeline` | correct stage sequencing, metadata/resource transfer and representative end-to-end documents |
| `html-view-core` | stable render-contract consumption, hit testing, navigation, search, selection and framework-neutral paint semantics |

Rules for this first wave:

- put a regression at the lowest stage that can demonstrate the behavior;
- use a deterministic test shaper for ordinary layout tests;
- assert semantic values, not private storage order or dedup indices;
- keep one behavior per test and give it a descriptive standards-oriented name;
- add negative and boundary cases alongside the successful case;
- do not expose new mutable internals merely to make a test convenient;
- retain allocation, size, and zero-copy checks where representation cost is an
  intentional contract.

A bug spanning multiple stages gets a focused test in the owning crate first.
It becomes a shared fixture later only when the cross-stage result itself is
important. The conformance harness therefore consolidates and compares proven
stage behavior; it does not replace normal unit and integration tests.

### Implementation progress — 2026-08-01

The first crate-local slice and broad direct parser/style standards runners are
implemented:

- `html-parse`: three WPT-derived tests cover implicit paragraph closing,
  adoption-agency reparenting, and representative named/numeric character
  references;
- `html-style`: two WPT-derived tests cover author `!important` versus inline
  style and the inherited/non-inherited behavior of `unset`;
- `html-layout`: one WPT-derived semantic test covers preserved segment breaks
  under `white-space: break-spaces` using the deterministic shaper.
- `html-wpt-test-support`: a dev-only, renderer-independent adapter parses the
  original WPT/html5lib tree-construction `.dat` format and extracts literal
  declarative CSS WPT helper calls without executing JavaScript;
- `html-parse`: the crate-owned adapter discovers every case from all 61 pinned
  tree corpora in every applicable scripting mode: 3,828 possible runs with fragments,
  templates, comments, namespaces, doctypes, processing-instruction
  distinctions, and explicit scripting state, plus three document-mode tests;
- `html-style`: the crate-owned adapter scans all 142 pinned box, cascade, text,
  and selector parsing files, extracts 1,260 literal calls, and directly runs
  780 property-value, selector, whole-rule, and import-rule assertions in its
  known syntax scope.

The broad runners use exact known-failure ledgers and reasoned ignore manifests.
The reasoned manifests now quarantine 199 tree assertions and 377 error-count
assertions for parser-stack gaps or intentionally unsupported JavaScript
execution. The active HTML audit runs 3,629 tree assertions and 3,451
error-count assertions with no known divergences; passing cases in the same
corpora remain covered. The CSS audit has 19 known syntax divergences and seven
explicit dependency-owned selector ignores, while reporting 473
unknown-property assertions and 15 dynamic/unimplemented calls. Any new
mismatch, stale ledger entry, or stale ignore fails. Upstream revision, 206
source hashes, original files, and license
are recorded under `apps/renderers/html/testdata/wpt/`.

The remaining 19 CSS divergences are now split between CSS Text (10) and CSS
Cascade (9). The next rendering-relevant CSS Text slice is `tab-size`: syntax
is only three assertions, but correct static rendering also requires
position-dependent tab stops in preserved-white-space layout. The other text
entries are modern language-sensitive transforms/breaking values or
serialization-only cases; cascade entries concern strict `@import`/`all`
parsing.

Next crate-local slices should add preparation/shaping range cases, box geometry
cases, pipeline resource transfer, and view-core interaction cases. CSS
computed serialization and additional declarative helper forms remain separate,
truthfully classified follow-up work.

## Stage-by-stage correctness model

Each pipeline stage gets its own observable snapshot, invariants, focused
fixtures, and failure label. A downstream mismatch must not obscure the first
stage that diverged.

| Stage | Stable observations | Primary correctness oracle |
|---|---|---|
| Parse/DOM | normalized tree, text, attributes, metadata, resources, TOC | explicit fixture expectations and imported standards cases |
| Style | selected computed values by stable DOM path, inheritance and cascade provenance | explicit expectations and optional browser computed styles |
| Prepare | generated box/run semantics, DOM association, suppressed content, anonymous structure summaries | explicit fixtures and stage invariants |
| Shape | character-to-glyph ranges, validated metrics, font/style selection, link/text ownership | deterministic test shaper plus adapter-specific font tests |
| Layout | lines, boxes, fragments, geometry, anchors, images and decorations | explicit geometry expectations and optional browser measurements |
| Render contract | semantic paint order, visible text mapping, navigation, search and selection | explicit command snapshots and interaction assertions |

Correctness is checked at four levels:

1. **Local invariant:** the stage cannot emit malformed output.
2. **Expected fixture:** output matches a small reviewed semantic expectation.
3. **Pipeline differential:** equivalent inputs or incremental/clean execution
   produce equivalent observations.
4. **External oracle:** selected standards-sensitive results agree with a
   controlled reference browser, or the difference is explicitly classified.

Every failure reports the stage, fixture, stable path, expected value, actual
value, and the smallest surrounding semantic context. Snapshot comparison stops
at the first divergent stage by default, with an option to show downstream
effects.

## Direct standards ingestion and harness structure

Do not manually recreate a standards test when its upstream representation can
be consumed. The repository has two distinct test layers:

1. `apps/renderers/html/test-support/html-wpt` parses upstream WPT formats and contains
   no renderer knowledge. It is `publish = false` and used only as a dev
   dependency.
2. Each owning stage crate supplies a small adapter from its public semantic
   output to the upstream expectation. Unsupported capabilities are classified
   and counted; they are never silently dropped.

The parser adapter consumes original tree-construction `.dat` files directly,
and the CSS adapter consumes literal declarative calls from original WPT HTML.
Normal CI does not access the network: the upstream subset is vendored
byte-for-byte at a pinned revision under `apps/renderers/html/testdata/wpt/`.
Refreshing that subset is an explicit reviewable operation.

CSS needs two additional adapter kinds:

- a semantic adapter for tests whose assertions can be expressed as computed
  style or layout-stage values;
- a reftest adapter that renders both the test and its `<link rel="match">`
  reference through a controlled paint target and compares the resulting
  output.

Tests requiring `testharness.js`, DOM mutation, browser APIs, platform fonts, or
unsupported CSS are classified by capability. A future JavaScript/DOM bridge or
reference-browser oracle may run those groups, but the direct runner must not
claim conformance by deleting their browser-side assertions.

After the crate-local foundation is established, add a framework-neutral test
crate/tool, tentatively
`apps/renderers/html/crates/html-conformance`, with no GPUI/Floem dependency.

```text
Fixture manifest
   |
   +--> HTML/CSS/resources
   |
   +--> parser assertions
   +--> computed-style assertions
   +--> preparation/shaping assertions
   +--> layout/render-contract assertions
   +--> optional reference-browser expectations
```

The harness loads one fixture, builds the normal production pipeline, and
serializes only stable observable values.

Stage-owned extraction keeps private representations private. Where an existing
public contract is insufficient, the owning crate may expose a read-only
`conformance` module behind a non-default Cargo feature. Such adapters return
semantic records rather than references to layout trees, style stores, mutable
caches, or raw internal indices. The conformance crate enables these features;
normal renderer builds do not.

Proposed repository shape:

```text
apps/renderers/html/crates/html-conformance/
  src/
    fixture.rs
    snapshot.rs
    diff.rs
    runner.rs
  tests/
    fixtures.rs
apps/renderers/html/conformance/
  fixtures/<behavior-name>/
    fixture.toml
    input.html
    style.css          # optional
    expected.json
  schemas/
    semantic-v1.json
  seeds/
```

The runner supports selecting a stage, fixture, or feature tag and can emit an
updated snapshot only with an explicit update flag. Normal test execution never
rewrites expectations.

## Stage acceptance suites

### Parser/DOM

- malformed and fragmentary HTML never panics;
- normalized parent/child structure, text ordering, attributes, namespaces,
  metadata, stylesheet references, images, and TOC match expectations;
- all branded IDs and stored references resolve in their owning document;
- equivalent parser runs produce identical semantic trees.

### Style

- cascade origin, specificity, source order, inheritance, `initial`, `inherit`,
  and `unset` are covered independently;
- resolved values are asserted by stable element path or fixture ID, never by
  style-store index;
- all computed numeric values remain finite and within property invariants;
- adding an unmatched rule or reordering independent declarations preserves the
  snapshot.

### Preparation and shaping

- `display: none`, anonymous boxes, inline/block splitting, generated list
  markers, replaced content, and DOM/run ownership are observable semantically;
- every shaped range is ordered and in bounds and every glyph has validated
  metrics;
- a deterministic conformance shaper removes platform-font variability from
  normal CI;
- a small adapter-specific suite separately verifies real font family, weight,
  style, advance, ascent, descent, and baseline selection.

### Layout

- block, inline, float, list, table, image, decoration, and fragmentation cases
  assert geometry with an explicit tolerance;
- all output geometry is finite and required dimensions are non-negative;
- line/run/fragment ranges and anchor/link/image references are valid;
- repeated layout at identical constraints is byte-equivalent semantically;
- width changes followed by restoration reproduce the original result.

### Render contract and interactions

- semantic paint commands preserve observable paint order without naming a UI
  framework;
- visible text, hit testing, link lookup, CFI navigation, search, selection,
  images, and decorations are checked against the same snapshot coordinate
  space;
- GPUI and Floem receive equivalent semantic input for shared fixtures;
- creating or traversing the contract remains allocation-free outside the
  test-only snapshot serializer.

## Milestone 10a — Deterministic semantic snapshots

Define versioned snapshots at all six observable boundaries.

### DOM snapshot

- normalized element/text tree;
- tag, namespace, stable attributes, and text;
- parent/child structure;
- discovered stylesheet/resource references;
- image metadata and document TOC;
- no allocator addresses or internal string-table indices.

### Style snapshot

- selected computed properties by stable DOM path or fixture ID;
- resolved inherited/reset groups expressed as semantic values;
- no style-store indices or dedup-layout details;
- optional declaration provenance for cascade debugging.

### Preparation snapshot

- semantic box/run kinds and their stable DOM association;
- anonymous structure described by purpose and stable ancestry, not internal
  allocation or traversal IDs;
- suppressed content, replaced content, list markers, and inline/block splits;
- no mutable box-tree objects or style-store handles.

### Shaping snapshot

- source character and stable text ownership for each shaped range;
- normalized font/style request and validated advance/ascent/descent/baseline;
- link and DOM-text associations;
- no backend-specific glyph IDs in deterministic fixtures.

### Layout snapshot

- stable DOM/anonymous-box association where observable;
- line glyph/text ranges;
- box/line/fragment geometry rounded with an explicit tolerance policy;
- decorations, images, anchors, and link mappings;
- no internal traversal order unless order is observable.

### Render-contract snapshot

- paint-order semantic commands independent of a UI framework;
- glyph ID replaced by character plus stable font/style attributes where
  possible;
- image/resource identity, rectangles, colors, and decorations;
- navigation/search/selection-visible text mapping.

Snapshots include a schema version. A deliberate contract change updates the
schema or uses an explicit migration, not a blind mass rewrite.

### Acceptance tests

- Repeated runs produce byte-identical snapshots.
- Debug and release builds produce equivalent snapshots.
- Internal storage/dedup changes do not alter snapshots.
- Failure output shows a focused structural diff.

## Milestone 10b — Fixture manifest and focused corpus

Use a manifest per fixture, for example TOML:

```toml
name = "nested-inline-link"
features = ["html.anchor", "layout.inline", "navigation.link"]
viewport = [600, 800]
root_font_size = 16
assertions = ["dom", "style", "prepare", "shape", "layout", "render"]
tolerance_px = 0.01
```

Organize fixtures by behavior rather than by bug number:

- parsing/tree construction;
- cascade, inheritance, selectors, and units;
- inline formatting and line breaking;
- block, float, list, and table layout;
- fragmentation/pagination;
- images/SVG/resources;
- links, CFI, selection, search, and accessibility-facing text.

Start by migrating high-value existing unit-test HTML into fixtures without
deleting focused unit tests. Add representative EPUB chapters from TestData
only when redistribution and provenance are clear; otherwise store a reduced
synthetic reproduction.

Every fixed correctness bug adds or reduces to a fixture before the fix lands.

## Milestone 10c — Incremental-versus-clean differential testing

Use Plan 09's stable render snapshot as the oracle:

1. apply a sequence of typed changes incrementally;
2. construct a clean pipeline from the final inputs;
3. require semantic snapshots to match.

Generate deterministic sequences covering:

- viewport widths and resize order;
- font-size/style changes;
- stylesheet addition, removal, and reordering;
- font-environment revisions;
- image metrics arriving in different batches/orders;
- navigation away, cache eviction, and return;
- repeated no-op changes.

On failure, print the first divergent phase. This distinguishes an invalidation
bug from a layout/render bug and drastically reduces debugging time.

## Milestone 10d — Reference-browser oracle

Add an optional developer tool that renders a controlled subset of fixtures in
a reference browser and captures normalized measurements.

### Reproducibility requirements

- fixed viewport, device scale, default stylesheet assumptions, and locale;
- bundled deterministic test fonts where licensing permits;
- animations/transitions disabled;
- network disabled; all resources local;
- explicit browser/tool version recorded with generated oracle data.

### What to compare

Prefer DOM APIs and computed measurements over pixels:

- computed property values;
- `getBoundingClientRect` geometry;
- line/element counts where observable;
- link/image/list/table structure;
- pagination expectations defined by this renderer's column model.

Use screenshots only for cases whose semantics cannot be captured otherwise.
Apply documented tolerances to font-dependent geometry and exact comparisons
to colors, enums, tree shape, and text.

Reference-browser differences are triaged as:

- renderer bug;
- unsupported feature;
- deliberate product behavior;
- unstable/non-comparable oracle.

Do not automatically bless the browser output without classification.

## Milestone 10e — Invariant and fuzz testing

Add deterministic seeded fuzz/property tests around public boundaries.

### Parser/DOM invariants

- arbitrary input never panics;
- finished DOM passes Plan 08 validation;
- every node is reachable and parent/child links agree;
- all IDs and resource/string references are valid.

### Style invariants

- arbitrary declaration text never creates non-finite computed values;
- every element receives exactly one valid computed style;
- inherited properties equal the specified parent result when undeclared;
- style-store/string handles never cross owners.

### Shaping/layout invariants

- every glyph ID has valid metrics;
- all ranges are ordered and in bounds;
- every emitted coordinate/size is finite;
- widths/heights that must be non-negative are non-negative;
- line, image, decoration, anchor, and link indices are valid;
- relayout and clean layout agree.

### Metamorphic properties

- adding an irrelevant unused CSS rule does not change output;
- reordering independent declarations does not change output;
- wrapping text in a style-neutral inline element preserves addressable text;
- repeated relayout at the same constraints is idempotent;
- applying a change and its inverse restores the semantic snapshot.

Keep seeds and reduced failures in the repository. Fuzz tests in normal CI run
a bounded deterministic corpus; longer exploration runs in nightly/manual CI.

## Milestone 10f — Failure reduction and triage

Provide a reducer that can minimize:

- HTML nodes/attributes/text;
- CSS rules, selectors, declarations, and values;
- resource sets;
- incremental change sequences.

The reduction predicate is a command returning success only while the semantic
diff, invariant failure, or panic remains. A reduced failure is stored with:

- the minimal fixture;
- expected/current diff;
- seed and tool versions;
- feature tags;
- issue/plan reference.

Generate a machine-readable support report:

- pass/fail/unsupported counts by feature tag;
- most common failure clusters;
- TestData documents affected by each cluster;
- performance cost of each fixture group.

## Milestone 10g — CI tiers and stability policy

### Per-change CI

- boundary script and compile-fail tests;
- unit tests;
- small semantic fixture suite;
- deterministic incremental differential sequences;
- short fixed fuzz corpus;
- no reference-browser installation required.

### Nightly/manual CI

- full fixture/TestData corpus;
- longer fuzzing;
- optional reference-browser comparison;
- release timing and memory profiles;
- snapshot/support report artifact.

### Snapshot policy

- snapshot updates require a human-readable explanation;
- a change that improves one fixture but regresses another cannot be blindly
  accepted;
- internal representation changes should normally produce no snapshot diff;
- intentional product behavior is documented in the fixture manifest.

## Prioritizing feature work

Use conformance results to order the existing plans:

- Plan 01 — media queries/supports;
- Plan 02 — auto-margin centering;
- Plan 04 — calc and units;
- Plan 06 — structural pseudo-classes and pseudo-elements;
- Plan 07 — fragmentation.

Rank work using:

1. number of real TestData documents affected;
2. severity of visible/navigation breakage;
3. size and risk of the implementation;
4. availability of a stable oracle;
5. performance impact.

This prevents implementing standards features in numerical plan order when the
corpus shows a different payoff.

## Performance constraints

- Snapshot serialization is test/tooling-only and absent from production
  builds.
- Production types do not derive heavyweight serialization solely for tests;
  adapters extract semantic snapshots through read-only contracts.
- Per-change CI should remain short enough for normal development; move corpus
  breadth, not essential invariants, to nightly runs.
- Reference-browser tooling must not become a runtime or release dependency.

## Ordered implementation sequence

1. Inventory existing tests by owning crate and stage, recording duplicated,
   missing, overly broad, and representation-coupled coverage.
2. Add the first-wave crate-local correctness suites in dependency order:
   DOM, parse, style model, style, preparation/shaping/layout, pipeline, then
   view core.
3. Run all crate-local tests in debug and the deterministic semantic subset in
   release; fix nondeterminism before introducing snapshots.
4. Scaffold `html-conformance`, the manifest loader, schema versioning, and
   structural diff output.
5. Implement DOM and style snapshots with five representative fixtures each.
6. Implement preparation, shaping, layout, and render-contract snapshots with a
   deterministic shaper and at least five fixtures per boundary.
7. Promote representative cross-stage regression cases into fixtures without
   deleting their
   focused unit tests.
8. Add invariants, metamorphic checks, and a bounded deterministic seed corpus.
9. After Plan 09, add incremental-versus-clean sequences and first-divergent-
   stage reporting.
10. Add failure reduction and feature-tagged support reports.
11. Add the optional controlled reference-browser oracle.
12. Split per-change and nightly/manual CI tiers.

The reference-browser work comes after native snapshots and invariants; it is a
useful oracle, not the foundation of the test architecture.

## Definition of done

- Every pipeline stage has focused correctness tests in its owning crate before
  equivalent cross-stage fixtures are introduced.
- Crate-local tests cover successful, invalid, and boundary behavior without
  exposing mutable implementation storage.
- Parser, style, layout, and render behavior can be asserted without UI
  framework dependencies.
- Preparation and shaping have direct semantic coverage rather than being
  inferred only from final layout output.
- At least five reviewed fixtures exercise each pipeline boundary, and every
  fixture can be selected by stage and feature tag.
- A deliberate corruption or expectation change at each boundary produces a
  focused diff naming that boundary.
- Incremental output is continuously checked against clean rebuilds.
- Malformed HTML/CSS cannot panic or violate finalized-state invariants across
  the maintained deterministic fuzz corpus.
- Reference comparisons are reproducible and explicitly classified.
- Failures reduce into small permanent fixtures.
- A feature-tagged report identifies the highest-impact correctness gaps.
- Subsequent work is selected from measured conformance impact rather than
  speculative architectural improvement.

## What follows Plan 10

Stop creating general architecture plans. Execute the highest-impact existing
HTML/CSS feature plan identified by the conformance report, then add its cases
to the permanent corpus. New architectural work requires evidence from a
reduced failure, performance profile, or boundary violation.

## Effort and risk

Medium–large, but independently shippable. Semantic snapshot design is the key
risk: snapshots must be stable enough to survive internal refactors without
hiding observable regressions. Start small and version the schema rather than
capturing every internal field.
